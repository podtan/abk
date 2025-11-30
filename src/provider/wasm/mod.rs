//! Generic WASM Provider Loader
//!
//! This module provides a generic provider implementation that loads ANY WASM module
//! following the standard provider WIT interface. It embodies the "zero code changes"
//! principle - new providers can be added by simply dropping a .wasm file into the
//! providers/ directory.
//!
//! ## Architecture
//!
//! The WasmProvider:
//! 1. Loads a WASM module from providers/{name}/provider.wasm
//! 2. Calls get_provider_metadata() to discover capabilities and env vars
//! 3. Uses WASM exports for all provider logic (format, parse, headers)
//! 4. Makes HTTP calls from Rust (WASM provides JSON formatting only)
//!
//! ## Implementation
//!
//! Uses wasmtime component model to call WASM exports:
//! - get-provider-metadata: Returns JSON with name, version, models, env_vars
//! - format-request: Formats request based on backend (auto-detected from model)
//! - parse-response: Parses response based on backend
//! - build-tanbal-headers: Custom headers for multi-LLM providers

use crate::config::EnvironmentLoader;
use crate::provider::{
    GenerateConfig, GenerateResponse, InternalMessage, LlmProvider,
    ToolInvocation,
};
use anyhow::{Context, Result};
use reqwest;
use serde_json::{json, Value};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::OnceCell;
use wasmtime::component::*;
use wasmtime::{Engine};
use wasmtime_wasi::{WasiCtx, WasiView};

/// Conditional debug macro - only prints if RUST_LOG is set to debug
macro_rules! debug {
    ($($arg:tt)*) => {
        if std::env::var("RUST_LOG").map(|v| v.to_lowercase().contains("debug")).unwrap_or(false) {
            eprintln!("[DEBUG] {}", format!($($arg)*));
        }
    };
}
// WASI host state for component instantiation
struct ComponentState {
    ctx: WasiCtx,
    table: wasmtime::component::ResourceTable,
}

impl WasiView for ComponentState {
    fn ctx(&mut self) -> &mut WasiCtx {
        &mut self.ctx
    }
    
    fn table(&mut self) -> &mut wasmtime::component::ResourceTable {
        &mut self.table
    }
}

// Generate bindings from WIT interface
// This creates types matching wit/provider/provider.wit (generic interface)
bindgen!({
    path: "wit/provider",
    world: "provider",
    async: true,
});

/// Generic WASM-based LLM provider
///
/// This provider loads a WASM module and delegates all provider-specific
/// logic to the WASM exports. It's completely generic and works with any
/// WASM module that implements the standard provider interface.
pub struct WasmProvider {
    /// Provider name (e.g., "openai")
    name: String,
    
    /// WASM module path
    #[allow(dead_code)]
    wasm_path: PathBuf,
    
    /// Provider metadata (loaded lazily from WASM on first use)
    metadata: OnceCell<Value>,
    
    /// HTTP client for API calls
    client: reqwest::Client,
    
    /// Environment loader (for reading env vars)
    #[allow(dead_code)]
    env: EnvironmentLoader,
    
    /// Wasmtime engine (shared across instances)
    engine: Arc<Engine>,
    
    /// WASM component (loaded module)
    component: Component,
    
    /// Configuration overrides (takes precedence over environment variables)
    config_overrides: std::collections::HashMap<String, String>,
}

impl WasmProvider {
    /// Create a new WASM provider
    ///
    /// # Arguments
    /// * `name` - Provider name (e.g., "openai")
    /// * `wasm_path` - Path to the .wasm file
    /// * `env` - Environment loader for configuration
    pub fn new(name: String, wasm_path: PathBuf, env: EnvironmentLoader) -> Result<Self> {
        Self::with_config(name, wasm_path, env, std::collections::HashMap::new())
    }
    
    /// Create a new WASM provider with configuration overrides
    ///
    /// # Arguments
    /// * `name` - Provider name (e.g., "openai")
    /// * `wasm_path` - Path to the .wasm file
    /// * `env` - Environment loader for configuration
    /// * `config_overrides` - Configuration values that override environment variables
    ///   Keys should match the config keys in provider metadata (e.g., "api_key", "base_url")
    pub fn with_config(
        name: String,
        wasm_path: PathBuf,
        env: EnvironmentLoader,
        config_overrides: std::collections::HashMap<String, String>,
    ) -> Result<Self> {
        // Create wasmtime engine with component model and async support
        let mut config = wasmtime::Config::new();
        config.wasm_component_model(true);
        config.async_support(true);
        let engine = Engine::new(&config)?;
        
        // Load WASM component from file
        let component = Component::from_file(&engine, &wasm_path)
            .with_context(|| format!("Failed to load WASM component from {}", wasm_path.display()))?;
        
        let engine = Arc::new(engine);
        
        // Metadata will be loaded on first use
        Ok(Self {
            name,
            wasm_path,
            metadata: OnceCell::new(),
            client: reqwest::Client::new(),
            env,
            engine,
            component,
            config_overrides,
        })
    }
    
    /// Create a configured store and linker with WASI support
    fn create_store_and_linker(&self) -> Result<(wasmtime::Store<ComponentState>, Linker<ComponentState>)> {
        // Create WASI context
        let wasi_ctx = wasmtime_wasi::WasiCtxBuilder::new().inherit_env().build();
        let state = ComponentState {
            ctx: wasi_ctx,
            table: wasmtime::component::ResourceTable::new(),
        };
        
        // Create store with WASI state
        let store = wasmtime::Store::new(&*self.engine, state);
        
        // Create and configure linker with WASI
        let mut linker = Linker::new(&*self.engine);
        wasmtime_wasi::add_to_linker_async(&mut linker)
            .context("Failed to add WASI to linker")?;
        
        Ok((store, linker))
    }
    
    /// Load provider metadata from WASM get-provider-metadata export (async, called on first use)
    async fn get_metadata(&self) -> Result<&Value> {
        self.metadata.get_or_try_init(|| async {
            let (mut store, linker) = self.create_store_and_linker()?;
            
            // Instantiate the component
            let instance = Provider::instantiate_async(&mut store, &self.component, &linker)
                .await
                .context("Failed to instantiate WASM component")?;
            
            // Call get_provider_metadata export
            let metadata_json = instance.abk_provider_adapter()
                .call_get_provider_metadata(&mut store)
                .await
                .context("Failed to call get_provider_metadata")?;
            
            // Parse JSON
            serde_json::from_str(&metadata_json)
                .context("Failed to parse provider metadata JSON")
        }).await
    }
    
    /// Get configuration from environment based on metadata
    /// First checks config_overrides (from with_config()), then env vars, then defaults
    async fn get_config(&self) -> Result<Value> {
        let metadata = self.get_metadata().await?;
        let env_vars = metadata["env_vars"]
            .as_object()
            .context("Provider metadata missing env_vars")?;
        
        debug!("get_config() called for provider: {}", self.name);
        debug!("Found {} env vars in metadata", env_vars.len());
        
        // Get a read lock on config_overrides
        let overrides = self.config_overrides.read()
            .map_err(|e| anyhow::anyhow!("Failed to read config_overrides: {}", e))?;
        debug!("Found {} config overrides", overrides.len());
        
        let mut config = json!({});
        
        for (key, spec) in env_vars {
            let env_name = spec["name"].as_str()
                .with_context(|| format!("Missing env var name for {}", key))?;
            
            let required = spec.get("required")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            
            // Check config_overrides first, then env var
            let value = overrides.get(key).cloned()
                .or_else(|| std::env::var(env_name).ok());
            
            let source = if overrides.contains_key(key) {
                "config_override"
            } else if value.is_some() {
                "env_var"
            } else {
                "not_set"
            };
            
            debug!("  Config key '{}' (env: {}): source={}", key, env_name, source);
            
            if required && value.is_none() {
                // Try default value
                if let Some(default) = spec.get("default") {
                    config[key] = default.clone();
                    debug!("    → Using default value: {}", default);
                } else {
                    anyhow::bail!("Required environment variable {} not set (and no config override provided)", env_name);
                }
            } else if let Some(v) = value {
                config[key] = json!(v);
                debug!("    → Set from {}", source);
            } else if let Some(default) = spec.get("default") {
                config[key] = default.clone();
                debug!("    → Using default value: {}", default);
            }
        }
        
        debug!("Final config: {}", serde_json::to_string_pretty(&config).unwrap_or_else(|_| "error".to_string()));
        
        Ok(config)
    }
    
    /// Get custom headers from WASM provider
    async fn get_custom_headers(
        &self,
        messages: &[InternalMessage],
        x_request_id: Option<&str>,
    ) -> Result<Vec<(String, String)>> {
        let (mut store, linker) = self.create_store_and_linker()?;
        let instance = Provider::instantiate_async(&mut store, &self.component, &linker)
            .await
            .context("Failed to instantiate WASM component for headers")?;
        
        // Convert InternalMessage to simple Message format for headers
        let wasm_messages: Vec<crate::provider::wasm::exports::abk::provider::adapter::Message> = messages
            .iter()
            .map(|m| {
                let content = match &m.content {
                    crate::provider::types::internal::MessageContent::Text(text) => text.clone(),
                    crate::provider::types::internal::MessageContent::Blocks(blocks) => {
                        // Extract text from blocks
                        blocks
                            .iter()
                            .filter_map(|b| b.as_text().map(|s| s.to_string()))
                            .collect::<Vec<_>>()
                            .join(" ")
                    }
                };
                
                crate::provider::wasm::exports::abk::provider::adapter::Message {
                    role: m.role.as_str().to_string(),
                    content,
                }
            })
            .collect();
        
        // Call WASM to build headers
        let header_pairs = instance.abk_provider_adapter()
            .call_build_headers(&mut store, &wasm_messages, x_request_id)
            .await
            .context("Failed to call build-headers")?;
        
        // Convert from WIT HeaderPair to (String, String)
        let headers: Vec<(String, String)> = header_pairs
            .into_iter()
            .map(|hp| (hp.key, hp.value))
            .collect();
        
        Ok(headers)
    }
}

#[async_trait::async_trait]
impl LlmProvider for WasmProvider {
    fn provider_name(&self) -> &str {
        &self.name
    }
    
    fn default_model(&self) -> String {
        // Try to get the actual model from environment variables
        // For known providers, check their specific model env vars
        match self.name.as_str() {
            "tanbal" => {
                if let Ok(model) = std::env::var("TANBAL_DEFAULT_MODEL") {
                    return model;
                }
            }
            "openai" => {
                if let Ok(model) = std::env::var("OPENAI_DEFAULT_MODEL") {
                    return model;
                }
            }
            "anthropic" => {
                if let Ok(model) = std::env::var("ANTHROPIC_MODEL") {
                    return model;
                }
            }
            "github" => {
                if let Ok(model) = std::env::var("GITHUB_MODEL") {
                    return model;
                }
            }
            _ => {}
        }

        // Fallback to provider name
        format!("{}/default", self.name)
    }
    
    async fn generate(
        &self,
        messages: Vec<InternalMessage>,
        config: &GenerateConfig,
    ) -> Result<GenerateResponse> {
        // Get config from environment
        let wasm_config = self.get_config().await?;
        
        // Get base URL and API key
        let base_url = wasm_config["base_url"]
            .as_str()
            .context("Missing base_url in config")?;
        
        let api_key = wasm_config["api_key"]
            .as_str()
            .context("Missing api_key in config")?;
        
        // Get model from config or wasm_config
        let model = config.model.clone()
            .or_else(|| wasm_config.get("default_model").and_then(|v| v.as_str()).map(|s| s.to_string()))
            .unwrap_or_else(|| "unknown".to_string());
        
        // Serialize messages to JSON for WASM
        let messages_json = serde_json::to_string(&messages)?;
        
        // DEBUG: Log messages JSON when debug logging is enabled
        debug!("========== WASM PROVIDER DEBUG ==========");
        debug!("Provider: {}", self.name);
        debug!("Model: {}", model);
        debug!("Messages JSON being sent to WASM:");
        debug!("{}", serde_json::to_string_pretty(&messages).unwrap_or_default());
        debug!("=========================================");
        
        // Serialize tools to JSON if present
        let tools_json = config.tools.as_ref()
            .map(|tools| serde_json::to_string(tools))
            .transpose()?;
        
        // Serialize tool_choice to JSON if present
        let tool_choice_json = config.tool_choice.as_ref()
            .map(|tc| serde_json::to_string(tc))
            .transpose()?;
        
        // Call WASM format_request_from_json - ALL formatting logic in WASM!
        let (mut store, linker) = self.create_store_and_linker()?;
        let instance = Provider::instantiate_async(&mut store, &self.component, &linker)
            .await
            .context("Failed to instantiate WASM component")?;
        
        let request_body = instance.abk_provider_adapter()
            .call_format_request_from_json(
                &mut store,
                &messages_json,
                &model,
                tools_json.as_deref(),
                tool_choice_json.as_deref(),
                config.max_tokens,
                config.temperature,
                false, // Non-streaming mode for generate()
            )
            .await
            .context("Failed to call format-request-from-json")?
            .map_err(|e| anyhow::anyhow!("WASM format error: {}", e.message))?;
        
        // Get custom headers from WASM (if provider supports it)
        let custom_headers = self.get_custom_headers(&messages, config.x_request_id.as_deref()).await?;
        
        // Get full API URL with endpoint from WASM (provider-specific logic)
        let full_url = {
            let (mut store, linker) = self.create_store_and_linker()?;
            let instance = Provider::instantiate_async(&mut store, &self.component, &linker)
                .await
                .context("Failed to instantiate WASM component")?;
            
            instance.abk_provider_adapter()
                .call_get_api_url(&mut store, &base_url, &model)
                .await
                .context("Failed to call get-api-url")?
        };
        
        // Make HTTP request with custom headers
        let mut request = self.client
            .post(full_url)
            .header("Authorization", format!("Bearer {}", api_key))
            .header("Content-Type", "application/json");
        
        // Add custom headers from WASM
        for (key, value) in custom_headers {
            request = request.header(&key, &value);
        }
        
        let response = request
            .body(request_body)
            .send()
            .await
            .context("Failed to send HTTP request")?;
        
        let status = response.status();
        let response_body = response.text().await?;
        
        if !status.is_success() {
            anyhow::bail!("API error {}: {}", status, response_body);
        }
        
        // Parse response using WASM - it will detect backend from model string
        let (mut store, linker) = self.create_store_and_linker()?;
        let instance = Provider::instantiate_async(&mut store, &self.component, &linker)
            .await
            .context("Failed to instantiate WASM component")?;
        
        // WASM will parse the model string and determine backend itself
        let result = instance.abk_provider_adapter()
            .call_parse_response(&mut store, &response_body, &model)
            .await
            .context("Failed to call parse-response")?
            .map_err(|e| anyhow::anyhow!("WASM parse error: {}", e.message))?;
        
        // Convert WIT result to GenerateResponse
        if !result.tool_calls.is_empty() {
            let invocations: Vec<ToolInvocation> = result.tool_calls
                .into_iter()
                .map(|tc| {
                    let arguments: Value = serde_json::from_str(&tc.arguments)
                        .unwrap_or_else(|_| json!({}));
                    
                    ToolInvocation {
                        id: tc.id,
                        name: tc.name,
                        arguments,
                        provider_metadata: std::collections::HashMap::new(),
                    }
                })
                .collect();
            Ok(GenerateResponse::ToolCalls(invocations))
        } else if let Some(content) = result.content {
            Ok(GenerateResponse::Content(content))
        } else {
            anyhow::bail!("Empty response from WASM parse-response")
        }
    }
    
    async fn generate_stream(
        &self,
        messages: Vec<InternalMessage>,
        config: &GenerateConfig,
    ) -> Result<crate::provider::StreamingResponse> {
        // Get config from environment
        let wasm_config = self.get_config().await?;
        
        // Get base URL and API key
        let base_url = wasm_config["base_url"]
            .as_str()
            .context("Missing base_url in config")?
            .to_string();
        
        let api_key = wasm_config["api_key"]
            .as_str()
            .context("Missing api_key in config")?
            .to_string();
        
        // Get model from config or wasm_config
        let model = config.model.clone()
            .or_else(|| wasm_config.get("default_model").and_then(|v| v.as_str()).map(|s| s.to_string()))
            .unwrap_or_else(|| "unknown".to_string());
        
        // Check if streaming is supported for this model (WASM provider decides)
        let supports_streaming = {
            let (mut store, linker) = self.create_store_and_linker()?;
            let instance = Provider::instantiate_async(&mut store, &self.component, &linker)
                .await
                .context("Failed to instantiate WASM component")?;
            
            instance.abk_provider_adapter()
                .call_supports_streaming(&mut store, &model)
                .await
                .context("Failed to call supports-streaming")?
        };
        
        // If streaming not supported, use fallback: non-streaming generate() → simulate streaming
        if !supports_streaming {
            debug!("Streaming not supported for model {}, using fallback", model);
            let response = self.generate(messages.clone(), config).await?;
            
            // Simulate streaming by sending the whole response as chunks
            let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
            
            tokio::spawn(async move {
                use crate::provider::StreamChunk;
                match response {
                    GenerateResponse::Content(content) => {
                        // Send content in chunks
                        for chunk in content.chars().collect::<Vec<_>>().chunks(10) {
                            let chunk_str: String = chunk.iter().collect();
                            let _ = tx.send(Ok(StreamChunk::Text(chunk_str)));
                        }
                    }
                    GenerateResponse::ToolCalls(calls) => {
                        // Send tool calls as deltas
                        for (index, call) in calls.into_iter().enumerate() {
                            // Send tool call start (with id and name)
                            let _ = tx.send(Ok(StreamChunk::ToolCallDelta {
                                index,
                                id: Some(call.id),
                                name: Some(call.name),
                                arguments_delta: None,
                            }));
                            // Send arguments
                            let args_str = serde_json::to_string(&call.arguments).unwrap_or_default();
                            let _ = tx.send(Ok(StreamChunk::ToolCallDelta {
                                index,
                                id: None,
                                name: None,
                                arguments_delta: Some(args_str),
                            }));
                        }
                    }
                }
                // Send done marker
                let _ = tx.send(Ok(StreamChunk::Done));
            });
            
            return Ok(Box::pin(futures_util::stream::unfold(rx, |mut rx| async move {
                rx.recv().await.map(|item| (item, rx))
            })));
        }
        
        // Serialize messages to JSON for WASM
        let messages_json = serde_json::to_string(&messages)?;
        
        debug!("MESSAGES JSON BEING SENT TO WASM:");
        debug!("{}", serde_json::to_string_pretty(&messages).unwrap_or_default());
        
        // Serialize tools to JSON if present
        let tools_json = config.tools.as_ref()
            .map(|tools| serde_json::to_string(tools))
            .transpose()?;
        
        debug!("TOOLS JSON: {}", tools_json.as_deref().unwrap_or("None"));
        
        // Serialize tool_choice to JSON if present
        let tool_choice_json = config.tool_choice.as_ref()
            .map(|tc| serde_json::to_string(tc))
            .transpose()?;
        
        debug!("TOOL CHOICE JSON: {}", tool_choice_json.as_deref().unwrap_or("None"));
        
        // Call WASM format_request_from_json
        let (mut store, linker) = self.create_store_and_linker()?;
        let instance = Provider::instantiate_async(&mut store, &self.component, &linker)
            .await
            .context("Failed to instantiate WASM component")?;
        
        let request_body = instance.abk_provider_adapter()
            .call_format_request_from_json(
                &mut store,
                &messages_json,
                &model,
                tools_json.as_deref(),
                tool_choice_json.as_deref(),
                config.max_tokens,
                config.temperature,
                true, // Enable streaming for generate_stream()
            )
            .await
            .context("Failed to call format-request-from-json")?
            .map_err(|e| anyhow::anyhow!("WASM format error: {}", e.message))?;
        
        debug!("REQUEST BODY FROM WASM (with streaming enabled by WASM provider):");
        if let Ok(pretty) = serde_json::from_str::<serde_json::Value>(&request_body) {
            debug!("{}", serde_json::to_string_pretty(&pretty).unwrap_or_else(|_| request_body.clone()));
        } else {
            debug!("{}", request_body);
        }
        
        // Get custom headers from WASM
        let custom_headers = self.get_custom_headers(&messages, config.x_request_id.as_deref()).await?;
        
        // Get full API URL with endpoint from WASM (provider-specific logic)
        let full_url = {
            let (mut store, linker) = self.create_store_and_linker()?;
            let instance = Provider::instantiate_async(&mut store, &self.component, &linker)
                .await
                .context("Failed to instantiate WASM component")?;
            
            instance.abk_provider_adapter()
                .call_get_api_url(&mut store, &base_url, &model)
                .await
                .context("Failed to call get-api-url")?
        };
        
        // Make streaming HTTP request
        let mut request = self.client
            .post(full_url)
            .header("Authorization", format!("Bearer {}", api_key))
            .header("Content-Type", "application/json")
            .header("Accept", "text/event-stream");
        
        // Add custom headers from WASM
        for (key, value) in custom_headers {
            request = request.header(&key, &value);
        }
        
        let response = request
            .body(request_body)
            .send()
            .await
            .context("Failed to send HTTP streaming request")?;
        
        let status = response.status();
        debug!("WASM STREAM DEBUG Response status: {}", status);
        
        if !status.is_success() {
            let error_body = response.text().await?;
            anyhow::bail!("API error {}: {}", status, error_body);
        }
        
        // Create streaming response
        debug!("WASM STREAM DEBUG Creating byte stream...");
        let byte_stream = response.bytes_stream();
        
        // Clone necessary data for the stream closure
        let component = self.component.clone();
        let engine = self.engine.clone();
        
        // Buffer for incomplete SSE lines across HTTP chunks
        let line_buffer = std::sync::Arc::new(tokio::sync::Mutex::new(String::new()));
        
        // Create channel for emitting multiple chunks per HTTP chunk
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        
        // Spawn task to process stream and emit chunks
        tokio::spawn(async move {
            use futures_util::StreamExt;
            let mut byte_stream = byte_stream;
            
            while let Some(chunk_result) = byte_stream.next().await {
                match chunk_result {
                    Ok(bytes) => {
                        let text = String::from_utf8_lossy(&bytes).to_string();
                        debug!("WASM STREAM DEBUG Received {} bytes: {:?}", text.len(), &text[..text.len().min(200)]);
                        
                        // Acquire buffer lock and append new data
                        let mut buffer = line_buffer.lock().await;
                        buffer.push_str(&text);
                        
                        // Process complete SSE events (separated by double newline)
                        while let Some(event_end) = buffer.find("\n\n") {
                            // Extract complete event
                            let event = buffer[..event_end].to_string();
                            // Remove from buffer (including the \n\n separator)
                            *buffer = buffer[event_end + 2..].to_string();
                            
                            // Skip empty events
                            if event.trim().is_empty() {
                                continue;
                            }
                            
                            // Check for [DONE] marker (can be in "data: [DONE]" format)
                            if event.contains("data: [DONE]") {
                                let _ = tx.send(Ok(crate::provider::StreamChunk::Done));
                                return; // End the stream
                            }
                            
                            // Pass complete SSE event to WASM (could be single or multi-line)
                            // For OpenAI: "data: {...}"
                            // For Anthropic: "event: xxx\ndata: {...}"
                            if event.contains("data: ") {
                                // Call WASM to handle stream chunk (pass complete event including event: line if present)
                                if let Ok(delta) = process_stream_chunk(&component, &engine, &event).await {
                                    if let Some(content) = delta.content {
                                        // Print content to stdout in real-time
                                        print!("{}", content);
                                        use std::io::Write;
                                        let _ = std::io::stdout().flush();
                                        // EMIT this chunk immediately
                                        if tx.send(Ok(crate::provider::StreamChunk::Text(content))).is_err() {
                                            return; // Receiver dropped
                                        }
                                    } else if let Some(tool_delta_json) = delta.tool_call_delta {
                                        // Parse tool call delta from JSON
                                        match serde_json::from_str::<serde_json::Value>(&tool_delta_json) {
                                            Ok(tool_delta) => {
                                                // Extract tool call delta fields
                                                let index = tool_delta["index"].as_u64().unwrap_or(0) as usize;
                                                let id = tool_delta["id"].as_str().map(|s| s.to_string());
                                                let name = tool_delta["function"]["name"].as_str().map(|s| s.to_string());
                                                let arguments_delta = tool_delta["function"]["arguments"].as_str().map(|s| s.to_string());
                                                
                                                // EMIT this tool call delta immediately
                                                if tx.send(Ok(crate::provider::StreamChunk::ToolCallDelta {
                                                    index,
                                                    id,
                                                    name,
                                                    arguments_delta,
                                                })).is_err() {
                                                    return; // Receiver dropped
                                                }
                                            }
                                            Err(e) => {
                                                eprintln!("Warning: Failed to parse tool call delta JSON: {}", e);
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                    Err(e) => {
                        let _ = tx.send(Err(anyhow::anyhow!("Stream error: {}", e)));
                        return;
                    }
                }
            }
        });
        
        // Convert channel receiver to stream using futures_util
        let chunk_stream = futures_util::stream::unfold(rx, |mut rx| async move {
            rx.recv().await.map(|item| (item, rx))
        });
        
        Ok(Box::pin(chunk_stream))
    }
}

/// Process a single stream chunk through WASM
async fn process_stream_chunk(
    component: &wasmtime::component::Component,
    engine: &Arc<wasmtime::Engine>,
    chunk: &str,
) -> Result<exports::abk::provider::adapter::ContentDelta> {
    // Create store and linker
    let wasi_ctx = wasmtime_wasi::WasiCtxBuilder::new().inherit_env().build();
    let state = ComponentState {
        ctx: wasi_ctx,
        table: wasmtime::component::ResourceTable::new(),
    };
    let mut store = wasmtime::Store::new(engine, state);
    
    let mut linker = wasmtime::component::Linker::new(engine);
    wasmtime_wasi::add_to_linker_async(&mut linker)
        .context("Failed to add WASI to linker")?;
    
    // Instantiate component
    let instance = Provider::instantiate_async(&mut store, component, &linker)
        .await
        .context("Failed to instantiate WASM component for streaming")?;
    
    // Call handle_stream_chunk
    let result = instance.abk_provider_adapter()
        .call_handle_stream_chunk(&mut store, chunk)
        .await
        .context("Failed to call handle-stream-chunk")?;
    
    result.ok_or_else(|| anyhow::anyhow!("No delta returned from WASM"))
}

#[cfg(test)]
mod tests {
    // WASM provider tests should be done at integration test level
    // with actual WASM files and runtime environment
}

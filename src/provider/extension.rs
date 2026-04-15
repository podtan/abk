//! Extension-based LLM Provider
//!
//! This module provides an LLM provider implementation that uses the new
//! extension system instead of the old WASM plugin system.

use crate::config::EnvironmentLoader;
use crate::extension::ExtensionManager;
use crate::provider::traits::{GenerateResponse, LlmProvider, StreamingResponse, ToolInvocation};
use crate::provider::types::{GenerateConfig, InternalMessage};
use anyhow::{Context, Result};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;

/// Conditional debug macro
macro_rules! debug {
    ($($arg:tt)*) => {
        if std::env::var("RUST_LOG").map(|v| v.to_lowercase().contains("debug")).unwrap_or(false) {
            eprintln!("[DEBUG] {}", format!($($arg)*));
        }
    };
}

/// Extension-based LLM provider
///
/// This provider uses the new extension system to communicate with LLM APIs.
/// It wraps a `ProviderExtensionInstance` that implements the provider capability.
pub struct ExtensionProvider {
    /// Provider name (extension ID)
    name: String,

    /// HTTP client for API calls
    client: reqwest::Client,

    /// Environment loader
    env: EnvironmentLoader,

    /// Extension manager (shared, for loading extensions)
    manager: Arc<Mutex<ExtensionManager>>,

    /// Cached metadata (JSON string)
    metadata: Option<String>,
}

impl ExtensionProvider {
    /// Create a new extension-based provider
    ///
    /// # Arguments
    /// * `name` - Extension ID (e.g., "openai-unofficial")
    /// * `extensions_dir` - Directory containing extensions
    /// * `env` - Environment loader
    pub async fn new(
        name: String,
        extensions_dir: PathBuf,
        env: EnvironmentLoader,
    ) -> Result<Self> {
        debug!("ExtensionProvider::new - Creating manager for: {}", extensions_dir.display());
        let mut manager = ExtensionManager::new(&extensions_dir)
            .await
            .context("Failed to create extension manager")?;

        // Discover extensions
        debug!("ExtensionProvider::new - Discovering extensions");
        manager.discover().await.context("Failed to discover extensions")?;

        // Verify the extension exists and has provider capability
        let providers = manager.get_providers();
        debug!("ExtensionProvider::new - Found {} providers: {:?}", 
            providers.len(),
            providers.iter().map(|m| &m.extension.id).collect::<Vec<_>>()
        );
        
        let found = providers.iter().find(|m| m.extension.id == name);
        if found.is_none() {
            anyhow::bail!(
                "Provider extension '{}' not found. Available: {:?}",
                name,
                providers.iter().map(|m| &m.extension.id).collect::<Vec<_>>()
            );
        }

        // Instantiate and initialize using provider-only instance
        debug!("ExtensionProvider::new - Instantiating provider extension: {}", name);
        let instance = manager.instantiate_provider(&name)
            .await
            .map_err(|e| {
                debug!("ExtensionProvider::new - Instantiation failed: {:?}", e);
                anyhow::anyhow!("Failed to instantiate extension: {}", e)
            })?;
        debug!("ExtensionProvider::new - Initializing extension");
        instance.init()
            .await
            .map_err(|e| {
                debug!("ExtensionProvider::new - Initialization failed: {:?}", e);
                anyhow::anyhow!("Failed to initialize extension: {}", e)
            })?;

        debug!("ExtensionProvider created: {}", name);

        let timeout_secs = std::env::var("LLM_TIMEOUT_SECONDS")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(120);

        // pool_idle_timeout must be >= streaming timeout (600s) to prevent the
        // connection pool from reclaiming idle connections during slow LLM
        // streaming responses.  Configurable via LLM_POOL_IDLE_SECONDS env var.
        let pool_idle_secs = std::env::var("LLM_POOL_IDLE_SECONDS")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(600);

        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(timeout_secs))
            .connect_timeout(Duration::from_secs(30))
            .pool_idle_timeout(Duration::from_secs(pool_idle_secs))
            .build()
            .context("Failed to create HTTP client")?;

        Ok(Self {
            name,
            client,
            env,
            manager: Arc::new(Mutex::new(manager)),
            metadata: None,
        })
    }

    /// Get provider configuration from environment
    fn get_config(&self) -> Result<crate::extension::provider_only::provider::Config> {
        // Read from environment (OPENAI_BASE_URL, OPENAI_API_KEY, OPENAI_DEFAULT_MODEL)
        let base_url = std::env::var("OPENAI_BASE_URL")
            .unwrap_or_else(|_| "https://api.openai.com/v1".to_string());

        let api_key = std::env::var("OPENAI_API_KEY")
            .context("OPENAI_API_KEY not set")?;

        let default_model = std::env::var("OPENAI_DEFAULT_MODEL")
            .unwrap_or_else(|_| "gpt-4o-mini".to_string());

        Ok(crate::extension::provider_only::provider::Config {
            base_url,
            api_key,
            default_model,
        })
    }

    fn max_retries() -> u32 {
        std::env::var("LLM_MAX_RETRIES")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(3)
    }

    async fn make_request_with_retry(
        &self,
        api_url: &str,
        request_body: String,
        api_key: &str,
        stream: bool,
    ) -> Result<reqwest::Response> {
        let max_retries = Self::max_retries();
        let mut last_error = None;

        for attempt in 0..=max_retries {
            let mut request = self.client
                .post(api_url)
                .header("Authorization", format!("Bearer {}", api_key))
                .header("Content-Type", "application/json")
                .body(request_body.clone());

            if stream {
                request = request
                    .header("Accept", "text/event-stream")
                    // Override client timeout for streaming — LLM responses can take minutes
                    .timeout(Duration::from_secs(600));
            }

            match request.send().await {
                Ok(resp) => {
                    let status = resp.status();

                    if status.is_success() {
                        return Ok(resp);
                    }

                    if status.as_u16() == 429 {
                        let retry_after = resp.headers()
                            .get("retry-after")
                            .and_then(|v| v.to_str().ok())
                            .and_then(|v| v.parse::<u64>().ok())
                            .unwrap_or(60);

                        debug!("Rate limited (429), waiting {}s (attempt {}/{})", retry_after, attempt, max_retries);
                        tokio::time::sleep(Duration::from_secs(retry_after)).await;
                        last_error = Some(anyhow::anyhow!("Rate limited (429)"));
                        continue;
                    }

                    if status.is_server_error() {
                        let body = resp.text().await.unwrap_or_default();
                        debug!("Server error {}: {}, retrying (attempt {}/{})", status, body, attempt, max_retries);
                        tokio::time::sleep(Duration::from_secs(2u64.pow(attempt))).await;
                        last_error = Some(anyhow::anyhow!("Server error {}: {}", status, body));
                        continue;
                    }

                    let body = resp.text().await.unwrap_or_default();
                    return Err(anyhow::anyhow!("API error {}: {}", status, body));
                }
                Err(e) => {
                    if e.is_timeout() || e.is_connect() {
                        debug!("Network error: {}, retrying (attempt {}/{})", e, attempt, max_retries);
                        tokio::time::sleep(Duration::from_secs(2u64.pow(attempt))).await;
                        last_error = Some(e.into());
                        continue;
                    }
                    return Err(e.into());
                }
            }
        }

        Err(last_error.unwrap_or_else(|| anyhow::anyhow!("Max retries exceeded")))
    }
}

#[async_trait::async_trait]
impl LlmProvider for ExtensionProvider {
    fn provider_name(&self) -> &str {
        &self.name
    }

    fn default_model(&self) -> String {
        std::env::var("OPENAI_DEFAULT_MODEL")
            .unwrap_or_else(|_| "gpt-4o-mini".to_string())
    }

    async fn generate(
        &self,
        messages: Vec<InternalMessage>,
        config: &GenerateConfig,
    ) -> Result<GenerateResponse> {
        let provider_config = self.get_config()?;
        let model = config.model.clone().unwrap_or_else(|| provider_config.default_model.clone());

        // Serialize messages to JSON for format_request_from_json
        let messages_json = serde_json::to_string(&messages)?;
        let tools_json = config.tools.as_ref().map(|t| serde_json::to_string(t)).transpose()?;
        let tool_choice_json = config.tool_choice.as_ref().map(|tc| serde_json::to_string(tc)).transpose()?;

        // Format request using extension (async)
        let request_body = {
            let mut manager = self.manager.lock().await;
            let instance = manager.get_provider_instance_mut(&self.name)
                .context("Extension not instantiated")?;
            instance.format_request_from_json(
                &messages_json,
                &model,
                tools_json.as_deref(),
                tool_choice_json.as_deref(),
                config.max_tokens,
                config.temperature,
                false, // Non-streaming
            ).await.map_err(|e| anyhow::anyhow!("format_request_from_json failed: {}", e))?
        };

        // Get API URL (async)
        let api_url = {
            let mut manager = self.manager.lock().await;
            let instance = manager.get_provider_instance_mut(&self.name)
                .context("Extension not instantiated")?;
            instance.get_api_url(&provider_config.base_url, &model)
                .await
                .map_err(|e| anyhow::anyhow!("get_api_url failed: {}", e))?
        };

        debug!("ExtensionProvider: POST {}", api_url);

        let response = self.make_request_with_retry(&api_url, request_body, &provider_config.api_key, false).await?;

        let response_body = response.text().await?;

        // Parse response using extension (async)
        let assistant_msg = {
            let mut manager = self.manager.lock().await;
            let instance = manager.get_provider_instance_mut(&self.name)
                .context("Extension not instantiated")?;
            instance.parse_response(&response_body, &model)
                .await
                .map_err(|e| anyhow::anyhow!("parse_response failed: {}", e))?
        };

        // Convert to GenerateResponse
        if !assistant_msg.tool_calls.is_empty() {
            let invocations: Vec<ToolInvocation> = assistant_msg.tool_calls
                .into_iter()
                .map(|tc| ToolInvocation {
                    id: tc.id,
                    name: tc.name,
                    arguments: serde_json::from_str(&tc.arguments).unwrap_or_default(),
                    provider_metadata: std::collections::HashMap::new(),
                })
                .collect();
            Ok(GenerateResponse::ToolCalls(invocations))
        } else {
            Ok(GenerateResponse::Content {
                text: assistant_msg.content.unwrap_or_default(),
                reasoning: None, // Non-streaming doesn't have reasoning
            })
        }
    }

    async fn generate_stream(
        &self,
        messages: Vec<InternalMessage>,
        config: &GenerateConfig,
    ) -> Result<StreamingResponse> {
        use futures_util::StreamExt;
        use crate::provider::StreamChunk;

        let provider_config = self.get_config()?;
        let model = config.model.clone().unwrap_or_else(|| provider_config.default_model.clone());

        // Check if streaming is supported (async)
        let supports_streaming = {
            let mut manager = self.manager.lock().await;
            let instance = manager.get_provider_instance_mut(&self.name)
                .context("Extension not instantiated")?;
            instance.supports_streaming(&model)
                .await
                .map_err(|e| anyhow::anyhow!("supports_streaming failed: {}", e))?
        };

        if !supports_streaming {
            // Fallback to non-streaming
            debug!("Streaming not supported for {}, using fallback", model);
            let response = self.generate(messages, config).await?;

            let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
            tokio::spawn(async move {
                match response {
                    GenerateResponse::Content { text, reasoning } => {
                        // First send reasoning if present
                        if let Some(reasoning_text) = reasoning {
                            for chunk in reasoning_text.chars().collect::<Vec<_>>().chunks(10) {
                                let chunk_str: String = chunk.iter().collect();
                                let _ = tx.send(Ok(StreamChunk::Reasoning(chunk_str)));
                            }
                        }
                        // Then send content
                        for chunk in text.chars().collect::<Vec<_>>().chunks(10) {
                            let chunk_str: String = chunk.iter().collect();
                            let _ = tx.send(Ok(StreamChunk::Text(chunk_str)));
                        }
                    }
                    GenerateResponse::ToolCalls(calls) => {
                        for (index, call) in calls.into_iter().enumerate() {
                            let _ = tx.send(Ok(StreamChunk::ToolCallDelta {
                                index,
                                id: Some(call.id),
                                name: Some(call.name),
                                arguments_delta: None,
                            }));
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
                let _ = tx.send(Ok(StreamChunk::Done));
            });

            return Ok(Box::pin(futures_util::stream::unfold(rx, |mut rx| async move {
                rx.recv().await.map(|item| (item, rx))
            })));
        }

        // Serialize messages to JSON
        let messages_json = serde_json::to_string(&messages)?;
        let tools_json = config.tools.as_ref().map(|t| serde_json::to_string(t)).transpose()?;
        let tool_choice_json = config.tool_choice.as_ref().map(|tc| serde_json::to_string(tc)).transpose()?;

        // Format request with streaming enabled (async)
        let request_body = {
            let mut manager = self.manager.lock().await;
            let instance = manager.get_provider_instance_mut(&self.name)
                .context("Extension not instantiated")?;
            instance.format_request_from_json(
                &messages_json,
                &model,
                tools_json.as_deref(),
                tool_choice_json.as_deref(),
                config.max_tokens,
                config.temperature,
                true, // Enable streaming
            ).await.map_err(|e| anyhow::anyhow!("format_request_from_json failed: {}", e))?
        };

        // Get API URL (async)
        let api_url = {
            let mut manager = self.manager.lock().await;
            let instance = manager.get_provider_instance_mut(&self.name)
                .context("Extension not instantiated")?;
            instance.get_api_url(&provider_config.base_url, &model)
                .await
                .map_err(|e| anyhow::anyhow!("get_api_url failed: {}", e))?
        };

        debug!("ExtensionProvider streaming: POST {}", api_url);

        let response = self.make_request_with_retry(&api_url, request_body, &provider_config.api_key, true).await?;

        let byte_stream = response.bytes_stream();
        let manager = Arc::clone(&self.manager);
        let name = self.name.clone();

        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();

        tokio::spawn(async move {
            let mut byte_stream = byte_stream;
            let mut line_buffer = String::new();

            while let Some(chunk_result) = byte_stream.next().await {
                match chunk_result {
                    Ok(bytes) => {
                        let text = String::from_utf8_lossy(&bytes).to_string();
                        line_buffer.push_str(&text);

                        // Process complete SSE events
                        while let Some(event_end) = line_buffer.find("\n\n") {
                            let event = line_buffer[..event_end].to_string();
                            line_buffer = line_buffer[event_end + 2..].to_string();

                            if event.trim().is_empty() {
                                continue;
                            }

                            // Check for [DONE]
                            if event.contains("data: [DONE]") {
                                let _ = tx.send(Ok(StreamChunk::Done));
                                return;
                            }

                            // Process through extension (async)
                            if event.contains("data: ") {
                                // Debug: log raw SSE event before sending to WASM
                                debug!("[EXTENSION PROVIDER] Raw SSE event to WASM: {}", event);
                                
                                let mut mgr = manager.lock().await;
                                if let Some(instance) = mgr.get_provider_instance_mut(&name) {
                                    if let Ok(Some(delta)) = instance.handle_stream_chunk(&event).await {
                                        match delta.delta_type.as_str() {
                                            "reasoning" => {
                                                if let Some(reasoning) = delta.reasoning {
                                                    // Log reasoning to file only; display is handled by OutputSink
                                                    crate::observability::append_to_global_log(
                                                        &crate::observability::strip_ansi(&reasoning)
                                                    );
                                                    let _ = tx.send(Ok(StreamChunk::Reasoning(reasoning)));
                                                }
                                            }
                                            "content" => {
                                                if let Some(content) = delta.content {
                                                    // Log content to file only; display is handled by OutputSink
                                                    crate::observability::append_to_global_log(&content);
                                                    let _ = tx.send(Ok(StreamChunk::Text(content)));
                                                }
                                            }
                                            "tool_call" => {
                                                if let Some(tc) = delta.tool_call {
                                                    // Use tool_call_index from delta, default to 0
                                                    let index = delta.tool_call_index.unwrap_or(0) as usize;
                                                    
                                                    // Debug: log tool call delta received from WASM
                                                    debug!("[EXTENSION PROVIDER] Received tool_call delta from WASM:");
                                                    debug!("[EXTENSION PROVIDER]   index: {}", index);
                                                    debug!("[EXTENSION PROVIDER]   id: {:?}", if tc.id.is_empty() { None } else { Some(&tc.id) });
                                                    debug!("[EXTENSION PROVIDER]   name: {:?}", if tc.name.is_empty() { None } else { Some(&tc.name) });
                                                    debug!("[EXTENSION PROVIDER]   arguments: {:?}", if tc.arguments.is_empty() { None } else { Some(&tc.arguments) });
                                                    
                                                    let _ = tx.send(Ok(StreamChunk::ToolCallDelta {
                                                        index,
                                                        id: if tc.id.is_empty() { None } else { Some(tc.id) },
                                                        name: if tc.name.is_empty() { None } else { Some(tc.name) },
                                                        arguments_delta: if tc.arguments.is_empty() { None } else { Some(tc.arguments) },
                                                    }));
                                                }
                                            }
                                            "done" => {
                                                let _ = tx.send(Ok(StreamChunk::Done));
                                                return;
                                            }
                                            "error" => {
                                                if let Some(err) = delta.error {
                                                    crate::observability::tee_eprintln(
                                                        &format!("\n⚠️  Stream error from provider: {}\n", err)
                                                    );
                                                    let _ = tx.send(Err(anyhow::anyhow!("Stream error: {}", err)));
                                                    return;
                                                }
                                            }
                                            _ => {}
                                        }
                                    }
                                }
                            }
                        }
                    }
                    Err(e) => {
                        crate::observability::tee_eprintln(
                            &format!("\n⚠️  Stream byte error: {}\n", e)
                        );
                        let _ = tx.send(Err(anyhow::anyhow!("Stream error: {}", e)));
                        return;
                    }
                }
            }
        });

        Ok(Box::pin(futures_util::stream::unfold(rx, |mut rx| async move {
            rx.recv().await.map(|item| (item, rx))
        })))
    }
}

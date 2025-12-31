//! Extension-based LLM Provider
//!
//! This module provides an LLM provider implementation that uses the new
//! extension system instead of the old WASM plugin system.

use crate::config::EnvironmentLoader;
use crate::extension::{ExtensionManager, ProviderExtensionInstance};
use crate::provider::traits::{GenerateResponse, LlmProvider, StreamingResponse, ToolInvocation};
use crate::provider::types::{GenerateConfig, InternalMessage};
use anyhow::{Context, Result};
use std::path::PathBuf;
use std::sync::Arc;
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

        Ok(Self {
            name,
            client: reqwest::Client::new(),
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

        // Make HTTP request (only standard headers - no X-Request-Id, X-Initiator, etc.)
        let response = self.client
            .post(&api_url)
            .header("Authorization", format!("Bearer {}", provider_config.api_key))
            .header("Content-Type", "application/json")
            .body(request_body)
            .send()
            .await
            .context("HTTP request failed")?;

        let status = response.status();
        if !status.is_success() {
            let error_body = response.text().await?;
            anyhow::bail!("API error {}: {}", status, error_body);
        }

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
            Ok(GenerateResponse::Content(assistant_msg.content.unwrap_or_default()))
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
                    GenerateResponse::Content(content) => {
                        for chunk in content.chars().collect::<Vec<_>>().chunks(10) {
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

        // Make streaming HTTP request (only standard headers)
        let response = self.client
            .post(&api_url)
            .header("Authorization", format!("Bearer {}", provider_config.api_key))
            .header("Content-Type", "application/json")
            .header("Accept", "text/event-stream")
            .body(request_body)
            .send()
            .await
            .context("HTTP streaming request failed")?;

        let status = response.status();
        if !status.is_success() {
            let error_body = response.text().await?;
            anyhow::bail!("API error {}: {}", status, error_body);
        }

        // Process stream
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
                                let mut mgr = manager.lock().await;
                                if let Some(instance) = mgr.get_provider_instance_mut(&name) {
                                    if let Ok(Some(delta)) = instance.handle_stream_chunk(&event).await {
                                        match delta.delta_type.as_str() {
                                            "content" => {
                                                if let Some(content) = delta.content {
                                                    print!("{}", content);
                                                    use std::io::Write;
                                                    let _ = std::io::stdout().flush();
                                                    let _ = tx.send(Ok(StreamChunk::Text(content)));
                                                }
                                            }
                                            "tool_call" => {
                                                if let Some(tc) = delta.tool_call {
                                                    let _ = tx.send(Ok(StreamChunk::ToolCallDelta {
                                                        index: 0,
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

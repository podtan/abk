//! Native Rust OpenAI provider (no wasmtime dependency).
//!
//! Implements `LlmProvider` using pure Rust + `reqwest`.
//! Activated when `LLM_PROVIDER=openai-unofficial` (or no provider set).

mod messages;
mod tools;
mod request;
mod response;
mod sse;
mod client;

pub use client::HttpClient;

use crate::provider::traits::{GenerateResponse, LlmProvider, StreamingResponse};
use crate::provider::types::{GenerateConfig, InternalMessage};
use anyhow::{Context, Result};

/// Conditional debug macro
macro_rules! debug {
    ($($arg:tt)*) => {
        if std::env::var("RUST_LOG").map(|v| v.to_lowercase().contains("debug")).unwrap_or(false) {
            crate::observability::tee_eprintln(&format!("[DEBUG OPENAI-NATIVE] {}", format!($($arg)*)));
        }
    };
}

/// Native Rust OpenAI-compatible provider.
///
/// Reads configuration from environment variables:
/// - `OPENAI_API_KEY` (required)
/// - `OPENAI_BASE_URL` (default: `https://api.openai.com/v1`)
/// - `OPENAI_DEFAULT_MODEL` (default: `gpt-4o-mini`)
pub struct OpenAIProvider {
    http: HttpClient,
}

impl OpenAIProvider {
    /// Create a new native OpenAI provider.
    pub fn new() -> Result<Self> {
        let http = HttpClient::new()?;
        Ok(Self { http })
    }

    /// Get the API key from env.
    fn api_key(&self) -> Result<String> {
        std::env::var("OPENAI_API_KEY").context("OPENAI_API_KEY not set")
    }

    /// Get the base URL from env (default: `https://api.openai.com/v1`).
    fn base_url(&self) -> String {
        std::env::var("OPENAI_BASE_URL")
            .unwrap_or_else(|_| "https://api.openai.com/v1".to_string())
    }

    /// Build the chat-completions endpoint URL.
    fn api_url(&self) -> String {
        let base = self.base_url();
        if base.ends_with('/') {
            format!("{}chat/completions", base)
        } else {
            format!("{}/chat/completions", base)
        }
    }
}

#[async_trait::async_trait]
impl LlmProvider for OpenAIProvider {
    fn provider_name(&self) -> &str {
        "openai-unofficial"
    }

    fn default_model(&self) -> String {
        std::env::var("OPENAI_DEFAULT_MODEL")
            .unwrap_or_else(|_| "gpt-4o-mini".to_string())
    }

    async fn generate(
        &self,
        msgs: Vec<InternalMessage>,
        config: &GenerateConfig,
    ) -> Result<GenerateResponse> {
        let model = config
            .model
            .clone()
            .unwrap_or_else(|| self.default_model());

        // Convert messages
        let openai_messages = messages::messages_to_openai(&msgs);

        // Convert tools
        let openai_tools: Option<Vec<serde_json::Value>> = config
            .tools
            .as_ref()
            .map(|t| tools::tools_to_openai(t));

        let openai_tool_choice = config
            .tool_choice
            .as_ref()
            .map(|tc| tools::tool_choice_to_openai(tc));

        // Build request body
        let body = request::build_request_body(
            &openai_messages,
            &model,
            false, // non-streaming
            Some(config.temperature),
            config.max_tokens,
            openai_tools.as_deref(),
            openai_tool_choice.as_ref(),
        );

        let body_str = serde_json::to_string(&body)?;
        let api_key = self.api_key()?;
        let url = self.api_url();

        debug!("generate() POST {} model={}", url, model);

        let resp = self
            .http
            .post_with_retry(&url, body_str, &api_key, false)
            .await?;

        let resp_text = resp.text().await?;
        response::parse_response(&resp_text)
    }

    async fn generate_stream(
        &self,
        msgs: Vec<InternalMessage>,
        config: &GenerateConfig,
    ) -> Result<StreamingResponse> {
        use futures_util::StreamExt;

        let model = config
            .model
            .clone()
            .unwrap_or_else(|| self.default_model());

        // Convert messages
        let openai_messages = messages::messages_to_openai(&msgs);

        // Convert tools
        let openai_tools: Option<Vec<serde_json::Value>> = config
            .tools
            .as_ref()
            .map(|t| tools::tools_to_openai(t));

        let openai_tool_choice = config
            .tool_choice
            .as_ref()
            .map(|tc| tools::tool_choice_to_openai(tc));

        // Build request body
        let body = request::build_request_body(
            &openai_messages,
            &model,
            true, // streaming
            Some(config.temperature),
            config.max_tokens,
            openai_tools.as_deref(),
            openai_tool_choice.as_ref(),
        );

        let body_str = serde_json::to_string(&body)?;
        let api_key = self.api_key()?;
        let url = self.api_url();

        debug!("generate_stream() POST {} model={}", url, model);

        let resp = self
            .http
            .post_with_retry(&url, body_str, &api_key, true)
            .await?;

        let byte_stream = resp.bytes_stream();

        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();

        tokio::spawn(async move {
            let mut byte_stream = byte_stream;
            let mut line_buffer = String::new();

            while let Some(chunk_result) = byte_stream.next().await {
                match chunk_result {
                    Ok(bytes) => {
                        let text = String::from_utf8_lossy(&bytes).to_string();
                        line_buffer.push_str(&text);

                        // Process complete SSE events (separated by \n\n)
                        while let Some(event_end) = line_buffer.find("\n\n") {
                            let event = line_buffer[..event_end].to_string();
                            line_buffer = line_buffer[event_end + 2..].to_string();

                            if event.trim().is_empty() {
                                continue;
                            }

                            // Each SSE event has lines starting with "data: "
                            for line in event.lines() {
                                if let Some(data) = line.strip_prefix("data: ") {
                                    if let Some(chunks) = sse::parse_sse_event(data) {
                                        for chunk in chunks {
                                            match &chunk {
                                                crate::provider::StreamChunk::Text(t) => {
                                                    crate::observability::append_to_global_log(t);
                                                }
                                                crate::provider::StreamChunk::Reasoning(r) => {
                                                    crate::observability::append_to_global_log(
                                                        &crate::observability::strip_ansi(r),
                                                    );
                                                }
                                                _ => {}
                                            }
                                            let _ = tx.send(Ok(chunk));
                                        }
                                    }
                                }
                            }
                        }
                    }
                    Err(e) => {
                        crate::observability::tee_eprintln(&format!(
                            "\n⚠️  Stream byte error: {}\n",
                            e
                        ));
                        let _ = tx.send(Err(anyhow::anyhow!("Stream error: {}", e)));
                        return;
                    }
                }
            }

            // If the stream ended without an explicit [DONE], send one
            let _ = tx.send(Ok(crate::provider::StreamChunk::Done));
        });

        Ok(Box::pin(futures_util::stream::unfold(
            rx,
            |mut rx| async move { rx.recv().await.map(|item| (item, rx)) },
        )))
    }
}

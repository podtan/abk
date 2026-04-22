//! LLM Provider abstraction for multi-provider support.
//!
//! This module defines the core provider trait that abstracts away provider-specific
//! implementation details, enabling agents to work with multiple LLM providers
//! (OpenAI, Anthropic, etc.) through a unified interface.

use crate::provider::types::generate::GenerateConfig;
use crate::provider::types::internal::InternalMessage;
use anyhow::Result;
use futures_util::Stream;
use std::pin::Pin;

// Re-export streaming types from umf
pub use umf::StreamChunk;

// Re-export tool types from umf
pub use umf::{ToolCall, FunctionCall};

/// Response from a generation request
#[derive(Debug, Clone)]
pub enum GenerateResponse {
    /// Text content response with optional reasoning
    Content {
        /// The main response text
        text: String,
        /// Reasoning/thinking content (for thinking models like GLM, DeepSeek)
        reasoning: Option<String>,
    },
    /// Tool calls that need to be executed
    ToolCalls {
        /// The tool call invocations
        calls: Vec<ToolInvocation>,
        /// Reasoning/thinking content (for thinking models like GLM, DeepSeek)
        reasoning: Option<String>,
    },
}

/// A tool invocation from the LLM
#[derive(Debug, Clone)]
pub struct ToolInvocation {
    /// Unique identifier for this tool call
    pub id: String,
    /// Name of the tool to invoke
    pub name: String,
    /// Arguments as a JSON value (already parsed)
    pub arguments: serde_json::Value,
    /// Provider-specific metadata
    pub provider_metadata: std::collections::HashMap<String, String>,
}

/// Type alias for streaming response
pub type StreamingResponse = Pin<Box<dyn Stream<Item = Result<StreamChunk>> + Send>>;

/// Core trait that all LLM providers must implement
///
/// This trait provides a unified interface for interacting with different LLM providers,
/// handling message formatting, tool calling, and streaming transparently.
///
/// # Example
///
/// ```ignore
/// use abk::provider::LlmProvider;
/// use abk::provider::GenerateConfig;
/// use abk::provider::InternalMessage;
///
/// async fn use_provider(provider: &dyn LlmProvider) -> Result<()> {
///     let messages = vec![/* ... */];
///     let config = GenerateConfig::default();
///     
///     let response = provider.generate(messages, &config).await?;
///     match response {
///         GenerateResponse::Content { text, reasoning } => {
///             println!("Response: {}", text);
///             if let Some(r) = reasoning {
///                 println!("Reasoning: {}", r);
///             }
///         }
        ///     GenerateResponse::ToolCalls { calls, reasoning } => {
        ///         println!("Tool calls: {:?}", calls);
        ///         if let Some(r) = reasoning { println!("Reasoning: {}", r); }
        ///     }
///     }
///     Ok(())
/// }
/// ```
#[async_trait::async_trait]
pub trait LlmProvider: Send + Sync {
    /// Generate a non-streaming response from the LLM
    ///
    /// # Arguments
    /// * `messages` - Conversation history in internal format
    /// * `config` - Generation configuration (model, temperature, tools, etc.)
    ///
    /// # Returns
    /// Either text content or tool calls that need to be executed
    async fn generate(
        &self,
        messages: Vec<InternalMessage>,
        config: &GenerateConfig,
    ) -> Result<GenerateResponse>;

    /// Generate a streaming response from the LLM
    ///
    /// # Arguments
    /// * `messages` - Conversation history in internal format
    /// * `config` - Generation configuration (model, temperature, tools, etc.)
    ///
    /// # Returns
    /// Stream of chunks (text deltas, tool call events, completion)
    async fn generate_stream(
        &self,
        messages: Vec<InternalMessage>,
        config: &GenerateConfig,
    ) -> Result<StreamingResponse>;

    /// Get the provider name for logging and debugging
    ///
    /// # Returns
    /// Provider identifier (e.g., "openai", "anthropic")
    fn provider_name(&self) -> &str;

    /// Get the default model name for this provider
    ///
    /// This method should check environment variables first, then fall back to hardcoded defaults.
    /// For example, GitHub provider checks GITHUB_MODEL, OpenAI checks OPENAI_DEFAULT_MODEL.
    ///
    /// # Returns
    /// Model identifier (e.g., "gpt-4o-mini", "claude-sonnet-4-5")
    fn default_model(&self) -> String;
}

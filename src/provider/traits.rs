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

/// FAIL-LOUD ARGUMENT BOUNDARY (nghr b7c419cd, ruling 9359ae73).
///
/// Parses an LLM tool-call `arguments` payload at the emission/parse boundary.
/// A malformed or output-clipped arguments string fails with a NAMED error —
/// it must NEVER launder into silent `{}` and execute (the 863cbaff
/// data-loss class: the boundary must not convert a model-budget clip into a
/// silent successful no-argument tool call).
///
/// - absent / null arguments → `{}` (legitimate for no-argument tools)
/// - object-form arguments (provider drift) → passed through verbatim
/// - string-form arguments → parsed; parse failure = named error
/// - other scalar/array forms → passed through (schema validation downstream)
pub fn parse_tool_call_arguments(tool_name: &str, raw: Option<&serde_json::Value>) -> Result<serde_json::Value> {
    match raw {
        None | Some(serde_json::Value::Null) => Ok(serde_json::json!({})),
        Some(serde_json::Value::Object(map)) => Ok(serde_json::Value::Object(map.clone())),
        Some(serde_json::Value::String(s)) => serde_json::from_str(s).map_err(|e| {
            anyhow::anyhow!(
                "tool call '{}' arguments failed to parse (clipped or malformed JSON — refusing to execute with empty arguments): {} | args head: {:?}",
                tool_name,
                e,
                s.chars().take(160).collect::<String>()
            )
        }),
        Some(other) => Ok(other.clone()),
    }
}

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

    /// Get the display name of this provider's configured model.
    ///
    /// Display/log path only — implementations render an honest placeholder
    /// when no model is configured (never a fictional default). The WIRE
    /// path is the provider's fallible resolver (e.g.
    /// `OpenAIProvider::resolve_model`), which fails loud when nothing is
    /// configured (fffe9141 F5, owner-ordered).
    ///
    /// # Returns
    /// Model identifier (e.g., "GLM-5.3-Flash", "claude-sonnet-4-5") or an
    /// explicit "(no model configured)" placeholder.
    fn default_model(&self) -> String;
}

#[cfg(test)]
mod tool_argument_boundary_tests {
    use super::*;

    // GUARD (nghr b7c419cd-a): a clipped/truncated arguments string must fail
    // LOUD with a NAMED error — never launder into silent {} and execute.
    #[test]
    fn clipped_arguments_string_fails_loud_with_named_error() {
        // A tool-call arguments string cut mid-JSON by the model output budget.
        let clipped = r#"{"file_path": "/Projects/Tanbal/thq/src/main.rs", "old_str": "fn main() {"#;
        let err = parse_tool_call_arguments("edit", Some(&serde_json::Value::String(clipped.into())))
            .expect_err("clipped arguments must fail loud");
        let msg = format!("{err:#}");
        assert!(msg.contains("edit"), "error must NAME the tool: {msg}");
        assert!(
            msg.contains("refusing to execute with empty arguments"),
            "error must state the refusal: {msg}"
        );
    }

    // GUARD (nghr b7c419cd-a control): valid arguments still parse.
    #[test]
    fn valid_arguments_string_parses() {
        let v = parse_tool_call_arguments(
            "bash",
            Some(&serde_json::Value::String(r#"{"command":"ls"}"#.into())),
        )
        .expect("valid arguments must parse");
        assert_eq!(v["command"], "ls");
    }

    // Object-form arguments (provider drift) pass through verbatim — not {}.
    #[test]
    fn object_form_arguments_pass_through() {
        let obj = serde_json::json!({"command": "ls -la"});
        let v = parse_tool_call_arguments("bash", Some(&obj)).expect("object form must pass through");
        assert_eq!(v, obj);
    }

    // Absent / null arguments are legitimately empty (no-argument tools).
    #[test]
    fn absent_or_null_arguments_are_empty_object() {
        assert_eq!(parse_tool_call_arguments("submit", None).unwrap(), serde_json::json!({}));
        assert_eq!(
            parse_tool_call_arguments("submit", Some(&serde_json::Value::Null)).unwrap(),
            serde_json::json!({})
        );
    }

    // Non-string non-object scalars pass through (schema validation downstream).
    #[test]
    fn scalar_arguments_pass_through() {
        let v = parse_tool_call_arguments("odd", Some(&serde_json::json!(42))).unwrap();
        assert_eq!(v, serde_json::json!(42));
    }
}

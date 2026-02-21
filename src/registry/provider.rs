//! Tool Source Provider Trait
//!
//! This module defines the `ToolSourceProvider` trait which provides an abstraction
//! for different tool sources (native cats, MCP servers, WASM extensions, etc.).

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

/// Result of a tool execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolResult {
    /// Whether the tool execution was successful.
    pub success: bool,
    /// The output content from the tool.
    pub content: String,
    /// Optional structured data from the tool.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<serde_json::Value>,
}

impl ToolResult {
    /// Create a successful tool result.
    pub fn success(content: impl Into<String>) -> Self {
        Self {
            success: true,
            content: content.into(),
            data: None,
        }
    }

    /// Create a successful tool result with structured data.
    pub fn success_with_data(content: impl Into<String>, data: serde_json::Value) -> Self {
        Self {
            success: true,
            content: content.into(),
            data: Some(data),
        }
    }

    /// Create a failed tool result.
    pub fn failure(content: impl Into<String>) -> Self {
        Self {
            success: false,
            content: content.into(),
            data: None,
        }
    }

    /// Create a failed tool result with structured data.
    pub fn failure_with_data(content: impl Into<String>, data: serde_json::Value) -> Self {
        Self {
            success: false,
            content: content.into(),
            data: Some(data),
        }
    }
}

/// Uniform tool descriptor for LLM consumption.
///
/// This struct represents a tool's schema in a format suitable for
/// sending to an LLM provider (OpenAI, Anthropic, etc.).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolDescriptor {
    /// The unique name of the tool.
    pub name: String,
    /// Human-readable description of what the tool does.
    pub description: String,
    /// JSON Schema describing the tool's parameters.
    pub parameters: serde_json::Value,
    /// The name of the source that provides this tool.
    pub source: String,
}

impl ToolDescriptor {
    /// Create a new tool descriptor.
    pub fn new(
        name: impl Into<String>,
        description: impl Into<String>,
        parameters: serde_json::Value,
        source: impl Into<String>,
    ) -> Self {
        Self {
            name: name.into(),
            description: description.into(),
            parameters,
            source: source.into(),
        }
    }

    /// Convert to OpenAI function schema format.
    pub fn to_openai_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "function",
            "function": {
                "name": self.name,
                "description": self.description,
                "parameters": self.parameters
            }
        })
    }
}

/// Trait for tool source providers.
///
/// This trait abstracts different tool sources (native cats, MCP servers,
/// WASM extensions, etc.) behind a common interface.
///
/// # Thread Safety
///
/// Implementations must be `Send + Sync` to allow sharing across async tasks.
///
/// # Example
///
/// ```rust,ignore
/// use abk::registry::{ToolSourceProvider, ToolDescriptor, ToolResult};
/// use async_trait::async_trait;
///
/// struct MyToolSource;
///
/// #[async_trait]
/// impl ToolSourceProvider for MyToolSource {
///     fn name(&self) -> &str {
///         "my-source"
///     }
///
///     fn tool_descriptors(&self) -> Vec<ToolDescriptor> {
///         vec![ToolDescriptor::new(
///             "my_tool",
///             "Does something useful",
///             serde_json::json!({"type": "object"}),
///             self.name()
///         )]
///     }
///
///     async fn execute(&self, tool_name: &str, args: serde_json::Value) -> anyhow::Result<ToolResult> {
///         Ok(ToolResult::success("Done!"))
///     }
///
///     fn has_tool(&self, name: &str) -> bool {
///         name == "my_tool"
///     }
/// }
/// ```
#[async_trait]
pub trait ToolSourceProvider: Send + Sync {
    /// Get the source's identifier (e.g., "cats-opencode", "pdt-mcp").
    fn name(&self) -> &str;

    /// Get all tool descriptors this source provides.
    fn tool_descriptors(&self) -> Vec<ToolDescriptor>;

    /// Execute a tool by name.
    ///
    /// # Arguments
    /// * `tool_name` - The name of the tool to execute
    /// * `args` - The arguments as a JSON value
    ///
    /// # Returns
    /// The result of the tool execution, or an error if execution failed.
    async fn execute(&self, tool_name: &str, args: serde_json::Value) -> anyhow::Result<ToolResult>;

    /// Check if this source owns a tool with the given name.
    fn has_tool(&self, name: &str) -> bool;
}

/// Type alias for a boxed, reference-counted tool source provider.
pub type BoxedToolSource = Arc<dyn ToolSourceProvider>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tool_result_success() {
        let result = ToolResult::success("Hello");
        assert!(result.success);
        assert_eq!(result.content, "Hello");
        assert!(result.data.is_none());
    }

    #[test]
    fn test_tool_result_failure() {
        let result = ToolResult::failure("Error occurred");
        assert!(!result.success);
        assert_eq!(result.content, "Error occurred");
    }

    #[test]
    fn test_tool_result_with_data() {
        let data = serde_json::json!({"key": "value"});
        let result = ToolResult::success_with_data("Done", data.clone());
        assert!(result.success);
        assert_eq!(result.data, Some(data));
    }

    #[test]
    fn test_tool_descriptor() {
        let desc = ToolDescriptor::new(
            "bash",
            "Execute shell commands",
            serde_json::json!({"type": "object"}),
            "cats-opencode",
        );

        assert_eq!(desc.name, "bash");
        assert_eq!(desc.source, "cats-opencode");

        let schema = desc.to_openai_schema();
        assert_eq!(schema["type"], "function");
        assert_eq!(schema["function"]["name"], "bash");
    }
}

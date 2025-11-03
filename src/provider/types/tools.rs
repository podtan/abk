//! Tool abstraction for provider-agnostic tool definitions.
//!
//! This module defines the internal tool format that abstracts away provider-specific
//! tool calling formats (OpenAI's function calling vs Anthropic's tool use).

use serde::{Deserialize, Serialize};

/// Provider-agnostic tool definition
///
/// This represents a tool that can be called by the LLM, with a name,
/// description, and JSON Schema for parameters.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InternalToolDefinition {
    /// Tool name (function name)
    pub name: String,
    /// Human-readable description of what the tool does
    pub description: String,
    /// JSON Schema describing the tool's parameters
    pub parameters: serde_json::Value,
}

impl InternalToolDefinition {
    /// Create a new tool definition
    pub fn new(
        name: impl Into<String>,
        description: impl Into<String>,
        parameters: serde_json::Value,
    ) -> Self {
        Self {
            name: name.into(),
            description: description.into(),
            parameters,
        }
    }

    /// Validate that the parameters is a valid JSON Schema
    pub fn validate(&self) -> anyhow::Result<()> {
        // Check that parameters is an object
        if !self.parameters.is_object() {
            anyhow::bail!("Tool parameters must be a JSON object (schema)");
        }

        // Check for required schema fields
        let obj = self.parameters.as_object().unwrap();
        if !obj.contains_key("type") {
            anyhow::bail!("Tool parameters schema must have 'type' field");
        }

        Ok(())
    }
}

/// Tool choice option for controlling LLM tool usage
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ToolChoice {
    /// Let the LLM decide whether to use tools
    Auto,
    /// Force the LLM to use a tool
    Required,
    /// Prevent the LLM from using tools
    None,
    /// Force a specific tool to be used
    Specific { name: String },
}

impl ToolChoice {
    /// Convert to string representation
    pub fn as_str(&self) -> &str {
        match self {
            Self::Auto => "auto",
            Self::Required => "required",
            Self::None => "none",
            Self::Specific { .. } => "specific",
        }
    }
}

impl Default for ToolChoice {
    fn default() -> Self {
        Self::Auto
    }
}

/// Result from a tool execution
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolResult {
    /// ID of the tool invocation this is a result for
    pub invocation_id: String,
    /// Tool output content
    pub content: String,
    /// Whether this is an error result
    pub is_error: bool,
}

impl ToolResult {
    /// Create a successful tool result
    pub fn success(invocation_id: impl Into<String>, content: impl Into<String>) -> Self {
        Self {
            invocation_id: invocation_id.into(),
            content: content.into(),
            is_error: false,
        }
    }

    /// Create an error tool result
    pub fn error(invocation_id: impl Into<String>, error: impl Into<String>) -> Self {
        Self {
            invocation_id: invocation_id.into(),
            content: error.into(),
            is_error: true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tool_definition_creation() {
        let tool = InternalToolDefinition::new(
            "get_weather",
            "Get current weather for a location",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "location": {
                        "type": "string",
                        "description": "City name"
                    }
                },
                "required": ["location"]
            }),
        );

        assert_eq!(tool.name, "get_weather");
        assert_eq!(tool.description, "Get current weather for a location");
        assert!(tool.validate().is_ok());
    }

    #[test]
    fn test_tool_validation() {
        // Valid schema
        let tool = InternalToolDefinition::new(
            "test",
            "Test tool",
            serde_json::json!({
                "type": "object",
                "properties": {}
            }),
        );
        assert!(tool.validate().is_ok());

        // Invalid schema (not an object)
        let tool = InternalToolDefinition::new(
            "test",
            "Test tool",
            serde_json::json!("not an object"),
        );
        assert!(tool.validate().is_err());

        // Invalid schema (missing type)
        let tool = InternalToolDefinition::new(
            "test",
            "Test tool",
            serde_json::json!({
                "properties": {}
            }),
        );
        assert!(tool.validate().is_err());
    }

    #[test]
    fn test_tool_choice() {
        assert_eq!(ToolChoice::Auto.as_str(), "auto");
        assert_eq!(ToolChoice::Required.as_str(), "required");
        assert_eq!(ToolChoice::None.as_str(), "none");
        assert_eq!(
            ToolChoice::Specific {
                name: "test".to_string()
            }
            .as_str(),
            "specific"
        );

        // Test default
        let default: ToolChoice = Default::default();
        assert_eq!(default.as_str(), "auto");
    }

    #[test]
    fn test_tool_result() {
        let result = ToolResult::success("tool_123", "Success!");
        assert_eq!(result.invocation_id, "tool_123");
        assert_eq!(result.content, "Success!");
        assert!(!result.is_error);

        let result = ToolResult::error("tool_456", "Error occurred");
        assert_eq!(result.invocation_id, "tool_456");
        assert_eq!(result.content, "Error occurred");
        assert!(result.is_error);
    }

    #[test]
    fn test_tool_serialization() {
        let tool = InternalToolDefinition::new(
            "test",
            "Test tool",
            serde_json::json!({"type": "object"}),
        );

        let json = serde_json::to_string(&tool).unwrap();
        let deserialized: InternalToolDefinition = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.name, "test");
        assert_eq!(deserialized.description, "Test tool");
    }
}

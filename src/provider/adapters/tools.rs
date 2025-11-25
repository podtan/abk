//! Tool format adapter for converting between different tool representations.
//!
//! This module provides conversion between ABK's Tool format and
//! the provider-agnostic InternalToolDefinition format.

use crate::provider::types::tools::InternalToolDefinition;
use crate::provider::{Function, Tool, ToolCall, FunctionCall};
use crate::provider::ToolInvocation;
use anyhow::Result;

/// Adapter for converting between tool formats
pub struct ToolAdapter;

impl ToolAdapter {
    /// Convert ABK Tool to InternalToolDefinition
    ///
    /// # Arguments
    /// * `tool` - Tool in ABK format
    ///
    /// # Returns
    /// Internal tool definition
    pub fn to_internal(tool: &Tool) -> InternalToolDefinition {
        InternalToolDefinition {
            name: tool.function.name.clone(),
            description: tool.function.description.clone(),
            parameters: tool.function.parameters.clone(),
        }
    }

    /// Convert multiple tools to internal format
    ///
    /// # Arguments
    /// * `tools` - Vector of tools in ABK format
    ///
    /// # Returns
    /// Vector of internal tool definitions
    pub fn tools_to_internal(tools: &[Tool]) -> Vec<InternalToolDefinition> {
        tools.iter().map(Self::to_internal).collect()
    }

    /// Convert InternalToolDefinition back to ABK Tool
    ///
    /// # Arguments
    /// * `internal` - Internal tool definition
    ///
    /// # Returns
    /// Tool in ABK format
    pub fn from_internal(internal: &InternalToolDefinition) -> Tool {
        Tool {
            r#type: "function".to_string(),
            function: Function {
                name: internal.name.clone(),
                description: internal.description.clone(),
                parameters: internal.parameters.clone(),
            },
        }
    }

    /// Convert multiple internal tools back to ABK format
    ///
    /// # Arguments
    /// * `internals` - Vector of internal tool definitions
    ///
    /// # Returns
    /// Vector of tools in ABK format
    pub fn tools_from_internal(internals: &[InternalToolDefinition]) -> Vec<Tool> {
        internals.iter().map(Self::from_internal).collect()
    }

    /// Convert ToolInvocation to ToolCall (for executor compatibility)
    ///
    /// # Arguments
    /// * `invocation` - Tool invocation from provider
    ///
    /// # Returns
    /// ToolCall in simpaticoder format
    pub fn invocation_to_tool_call(invocation: &ToolInvocation) -> Result<ToolCall> {
        let arguments = serde_json::to_string(&invocation.arguments)?;

        Ok(ToolCall {
            id: invocation.id.clone(),
            r#type: "function".to_string(),
            function: FunctionCall {
                name: invocation.name.clone(),
                arguments,
            },
        })
    }

    /// Convert multiple invocations to tool calls
    ///
    /// # Arguments
    /// * `invocations` - Vector of tool invocations
    ///
    /// # Returns
    /// Vector of tool calls
    pub fn invocations_to_tool_calls(invocations: &[ToolInvocation]) -> Result<Vec<ToolCall>> {
        invocations
            .iter()
            .map(Self::invocation_to_tool_call)
            .collect()
    }

    /// Convert ToolCall to ToolInvocation
    ///
    /// # Arguments
    /// * `tool_call` - Tool call in simpaticoder format
    ///
    /// # Returns
    /// Tool invocation
    pub fn tool_call_to_invocation(tool_call: &ToolCall) -> Result<ToolInvocation> {
        let arguments: serde_json::Value = serde_json::from_str(&tool_call.function.arguments)?;

        Ok(ToolInvocation {
            id: tool_call.id.clone(),
            name: tool_call.function.name.clone(),
            arguments,
            provider_metadata: std::collections::HashMap::new(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tool_to_internal_conversion() {
        let tool = Tool {
            r#type: "function".to_string(),
            function: Function {
                name: "get_weather".to_string(),
                description: "Get current weather".to_string(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "location": {"type": "string"}
                    }
                }),
            },
        };

        let internal = ToolAdapter::to_internal(&tool);
        assert_eq!(internal.name, "get_weather");
        assert_eq!(internal.description, "Get current weather");
        assert!(internal.validate().is_ok());
    }

    #[test]
    fn test_internal_to_tool_conversion() {
        let internal = InternalToolDefinition {
            name: "calculate".to_string(),
            description: "Calculate something".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {}
            }),
        };

        let tool = ToolAdapter::from_internal(&internal);
        assert_eq!(tool.r#type, "function");
        assert_eq!(tool.function.name, "calculate");
        assert_eq!(tool.function.description, "Calculate something");
    }

    #[test]
    fn test_tool_round_trip() {
        let original = Tool {
            r#type: "function".to_string(),
            function: Function {
                name: "test_tool".to_string(),
                description: "Test".to_string(),
                parameters: serde_json::json!({"type": "object"}),
            },
        };

        let internal = ToolAdapter::to_internal(&original);
        let back = ToolAdapter::from_internal(&internal);

        assert_eq!(original.r#type, back.r#type);
        assert_eq!(original.function.name, back.function.name);
        assert_eq!(original.function.description, back.function.description);
    }

    #[test]
    fn test_invocation_to_tool_call() {
        let invocation = ToolInvocation {
            id: "call_123".to_string(),
            name: "get_weather".to_string(),
            arguments: serde_json::json!({"location": "SF"}),
            provider_metadata: std::collections::HashMap::new(),
        };

        let tool_call = ToolAdapter::invocation_to_tool_call(&invocation).unwrap();
        assert_eq!(tool_call.id, "call_123");
        assert_eq!(tool_call.function.name, "get_weather");
        
        let args: serde_json::Value = serde_json::from_str(&tool_call.function.arguments).unwrap();
        assert_eq!(args["location"], "SF");
    }

    #[test]
    fn test_tool_call_to_invocation() {
        let tool_call = ToolCall {
            id: "call_456".to_string(),
            r#type: "function".to_string(),
            function: FunctionCall {
                name: "calculate".to_string(),
                arguments: r#"{"x": 5, "y": 3}"#.to_string(),
            },
        };

        let invocation = ToolAdapter::tool_call_to_invocation(&tool_call).unwrap();
        assert_eq!(invocation.id, "call_456");
        assert_eq!(invocation.name, "calculate");
        assert_eq!(invocation.arguments["x"], 5);
        assert_eq!(invocation.arguments["y"], 3);
    }

    #[test]
    fn test_multiple_tools_conversion() {
        let tools = vec![
            Tool {
                r#type: "function".to_string(),
                function: Function {
                    name: "tool1".to_string(),
                    description: "First tool".to_string(),
                    parameters: serde_json::json!({"type": "object"}),
                },
            },
            Tool {
                r#type: "function".to_string(),
                function: Function {
                    name: "tool2".to_string(),
                    description: "Second tool".to_string(),
                    parameters: serde_json::json!({"type": "object"}),
                },
            },
        ];

        let internal = ToolAdapter::tools_to_internal(&tools);
        assert_eq!(internal.len(), 2);
        assert_eq!(internal[0].name, "tool1");
        assert_eq!(internal[1].name, "tool2");

        let back = ToolAdapter::tools_from_internal(&internal);
        assert_eq!(back.len(), 2);
        assert_eq!(back[0].function.name, "tool1");
        assert_eq!(back[1].function.name, "tool2");
    }
}

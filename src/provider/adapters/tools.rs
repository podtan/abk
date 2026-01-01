//! Tool format adapter for converting between different tool representations.
//!
//! This module provides conversion between ABK's Tool format and
//! the provider-agnostic InternalToolDefinition format.
//!
//! When the `invoker` feature is enabled, additional conversions are available
//! for working with the invoker module's types.

use crate::provider::types::tools::InternalToolDefinition;
use crate::provider::{Function, Tool, ToolCall, FunctionCall};
use crate::provider::ToolInvocation;
use anyhow::Result;

#[cfg(feature = "invoker")]
use crate::invoker::{InvokerDefinition, InvokerSource, InvokerRegistry};

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
    /// ToolCall in ABK format
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
    /// * `tool_call` - Tool call in ABK format
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

    // =========================================================================
    // Invoker Integration (requires "invoker" feature)
    // =========================================================================

    /// Convert InvokerDefinition to InternalToolDefinition
    ///
    /// This strips the source and metadata, keeping only the core tool info.
    ///
    /// # Arguments
    /// * `invoker` - Invoker definition from the invoker module
    ///
    /// # Returns
    /// Internal tool definition for use with providers
    #[cfg(feature = "invoker")]
    pub fn from_invoker(invoker: &InvokerDefinition) -> InternalToolDefinition {
        InternalToolDefinition {
            name: invoker.name.clone(),
            description: invoker.description.clone(),
            parameters: invoker.parameters.clone(),
        }
    }

    /// Convert InternalToolDefinition to InvokerDefinition
    ///
    /// Creates an invoker with Native source (since internal tools are local).
    ///
    /// # Arguments
    /// * `internal` - Internal tool definition
    ///
    /// # Returns
    /// Invoker definition with Native source
    #[cfg(feature = "invoker")]
    pub fn to_invoker(internal: &InternalToolDefinition) -> InvokerDefinition {
        InvokerDefinition::new(
            internal.name.clone(),
            internal.description.clone(),
            internal.parameters.clone(),
            InvokerSource::Native,
        )
    }

    /// Convert multiple InvokerDefinitions to InternalToolDefinitions
    ///
    /// # Arguments
    /// * `invokers` - Vector of invoker definitions
    ///
    /// # Returns
    /// Vector of internal tool definitions
    #[cfg(feature = "invoker")]
    pub fn invokers_to_internal(invokers: &[InvokerDefinition]) -> Vec<InternalToolDefinition> {
        invokers.iter().map(Self::from_invoker).collect()
    }

    /// Convert multiple InternalToolDefinitions to InvokerDefinitions
    ///
    /// All converted invokers will have Native source.
    ///
    /// # Arguments
    /// * `internals` - Vector of internal tool definitions
    ///
    /// # Returns
    /// Vector of invoker definitions
    #[cfg(feature = "invoker")]
    pub fn internals_to_invokers(internals: &[InternalToolDefinition]) -> Vec<InvokerDefinition> {
        internals.iter().map(Self::to_invoker).collect()
    }

    /// Generate InternalToolDefinitions from an InvokerRegistry
    ///
    /// This is the primary integration point for using the invoker module
    /// with the provider system. It converts all registered invokers to
    /// internal tool definitions that can be passed to providers.
    ///
    /// # Arguments
    /// * `registry` - Reference to an invoker registry
    ///
    /// # Returns
    /// Vector of internal tool definitions
    #[cfg(feature = "invoker")]
    pub fn from_registry(registry: &dyn InvokerRegistry) -> Vec<InternalToolDefinition> {
        registry.list().into_iter().map(Self::from_invoker).collect()
    }

    /// Generate InternalToolDefinitions from an InvokerRegistry, filtered by source
    ///
    /// # Arguments
    /// * `registry` - Reference to an invoker registry
    /// * `source` - Only include invokers from this source
    ///
    /// # Returns
    /// Vector of internal tool definitions from the specified source
    #[cfg(feature = "invoker")]
    pub fn from_registry_by_source(
        registry: &dyn InvokerRegistry,
        source: InvokerSource,
    ) -> Vec<InternalToolDefinition> {
        registry
            .list_by_source(source)
            .into_iter()
            .map(Self::from_invoker)
            .collect()
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

    // =========================================================================
    // Invoker Integration Tests (requires "invoker" feature)
    // =========================================================================

    #[cfg(feature = "invoker")]
    mod invoker_tests {
        use super::*;
        use crate::invoker::{InvokerDefinition, InvokerSource, DefaultInvokerRegistry, InvokerRegistry};

        #[test]
        fn test_invoker_to_internal_conversion() {
            let invoker = InvokerDefinition::new(
                "read_file",
                "Read contents of a file",
                serde_json::json!({
                    "type": "object",
                    "properties": {
                        "path": {"type": "string", "description": "File path"}
                    },
                    "required": ["path"]
                }),
                InvokerSource::Native,
            );

            let internal = ToolAdapter::from_invoker(&invoker);
            assert_eq!(internal.name, "read_file");
            assert_eq!(internal.description, "Read contents of a file");
            assert!(internal.validate().is_ok());
        }

        #[test]
        fn test_internal_to_invoker_conversion() {
            let internal = InternalToolDefinition::new(
                "write_file",
                "Write contents to a file",
                serde_json::json!({
                    "type": "object",
                    "properties": {
                        "path": {"type": "string"},
                        "content": {"type": "string"}
                    }
                }),
            );

            let invoker = ToolAdapter::to_invoker(&internal);
            assert_eq!(invoker.name, "write_file");
            assert_eq!(invoker.description, "Write contents to a file");
            assert_eq!(invoker.source, InvokerSource::Native);
            assert!(invoker.metadata.is_none());
        }

        #[test]
        fn test_invoker_round_trip() {
            let original = InvokerDefinition::new(
                "search",
                "Search for text",
                serde_json::json!({"type": "object"}),
                InvokerSource::Mcp, // Non-native source
            );

            let internal = ToolAdapter::from_invoker(&original);
            let back = ToolAdapter::to_invoker(&internal);

            // Name and description preserved
            assert_eq!(original.name, back.name);
            assert_eq!(original.description, back.description);
            // But source is reset to Native
            assert_eq!(back.source, InvokerSource::Native);
        }

        #[test]
        fn test_from_registry() {
            let mut registry = DefaultInvokerRegistry::new();
            
            registry.register(InvokerDefinition::new(
                "tool1",
                "First tool",
                serde_json::json!({"type": "object"}),
                InvokerSource::Native,
            )).unwrap();
            
            registry.register(InvokerDefinition::new(
                "tool2",
                "Second tool",
                serde_json::json!({"type": "object"}),
                InvokerSource::Mcp,
            )).unwrap();

            let tools = ToolAdapter::from_registry(&registry);
            assert_eq!(tools.len(), 2);
            
            // Tools can be in any order
            let names: Vec<&str> = tools.iter().map(|t| t.name.as_str()).collect();
            assert!(names.contains(&"tool1"));
            assert!(names.contains(&"tool2"));
        }

        #[test]
        fn test_from_registry_by_source() {
            let mut registry = DefaultInvokerRegistry::new();
            
            registry.register(InvokerDefinition::new(
                "native_tool",
                "Native tool",
                serde_json::json!({"type": "object"}),
                InvokerSource::Native,
            )).unwrap();
            
            registry.register(InvokerDefinition::new(
                "mcp_tool",
                "MCP tool",
                serde_json::json!({"type": "object"}),
                InvokerSource::Mcp,
            )).unwrap();
            
            registry.register(InvokerDefinition::new(
                "a2a_tool",
                "A2A tool",
                serde_json::json!({"type": "object"}),
                InvokerSource::A2a,
            )).unwrap();

            // Filter by Native
            let native_tools = ToolAdapter::from_registry_by_source(&registry, InvokerSource::Native);
            assert_eq!(native_tools.len(), 1);
            assert_eq!(native_tools[0].name, "native_tool");

            // Filter by Mcp
            let mcp_tools = ToolAdapter::from_registry_by_source(&registry, InvokerSource::Mcp);
            assert_eq!(mcp_tools.len(), 1);
            assert_eq!(mcp_tools[0].name, "mcp_tool");

            // Filter by A2a
            let a2a_tools = ToolAdapter::from_registry_by_source(&registry, InvokerSource::A2a);
            assert_eq!(a2a_tools.len(), 1);
            assert_eq!(a2a_tools[0].name, "a2a_tool");
        }

        #[test]
        fn test_multiple_invokers_conversion() {
            let invokers = vec![
                InvokerDefinition::new(
                    "invoker1",
                    "First invoker",
                    serde_json::json!({"type": "object"}),
                    InvokerSource::Native,
                ),
                InvokerDefinition::new(
                    "invoker2",
                    "Second invoker",
                    serde_json::json!({"type": "object"}),
                    InvokerSource::Mcp,
                ),
            ];

            let internals = ToolAdapter::invokers_to_internal(&invokers);
            assert_eq!(internals.len(), 2);
            assert_eq!(internals[0].name, "invoker1");
            assert_eq!(internals[1].name, "invoker2");

            let back = ToolAdapter::internals_to_invokers(&internals);
            assert_eq!(back.len(), 2);
            assert_eq!(back[0].name, "invoker1");
            assert_eq!(back[1].name, "invoker2");
            // Both have Native source after round-trip
            assert_eq!(back[0].source, InvokerSource::Native);
            assert_eq!(back[1].source, InvokerSource::Native);
        }
    }
}

//! MCP tool registration support.
//!
//! This module provides methods for registering tools from MCP servers.
//! Requires the `registry-mcp` feature.

use umf::{McpTool, ToInternal};

use super::{RegisteredTool, RegistryError, RegistryResult, ToolRegistry, ToolSource};

impl ToolRegistry {
    /// Register an MCP tool from a server.
    ///
    /// The tool is converted to internal format and stored with the
    /// MCP source and server name.
    ///
    /// # Arguments
    ///
    /// * `tool` - The MCP tool to register
    /// * `server` - The name/identifier of the MCP server
    ///
    /// # Errors
    ///
    /// Returns an error if a tool with the same name already exists.
    pub fn register_mcp(&self, tool: McpTool, server: impl Into<String>) -> RegistryResult<()> {
        let server = server.into();
        let internal = tool.to_internal();

        Self::validate_name(&internal.name)?;

        let mut inner = self.inner().write().unwrap();

        if let Some(&idx) = inner.name_index.get(&internal.name) {
            return Err(RegistryError::Conflict {
                name: internal.name,
                existing_source: inner.tools[idx].source(),
            });
        }

        let idx = inner.tools.len();
        inner.name_index.insert(internal.name.clone(), idx);
        inner.tools.push(RegisteredTool::mcp(internal, server));

        Ok(())
    }

    /// Register multiple MCP tools from a server.
    ///
    /// Unlike `register_native_batch`, this method skips conflicts instead
    /// of failing, making it suitable for batch registration where some tools
    /// may already exist.
    ///
    /// # Returns
    ///
    /// The count of successfully registered tools.
    pub fn register_mcp_batch(
        &self,
        tools: Vec<McpTool>,
        server: impl Into<String>,
    ) -> RegistryResult<usize> {
        let server = server.into();
        let mut count = 0;

        for tool in tools {
            match self.register_mcp(tool, &server) {
                Ok(()) => count += 1,
                Err(RegistryError::Conflict { .. }) => {
                    // Skip conflicts in batch mode
                    continue;
                }
                Err(e) => return Err(e),
            }
        }

        Ok(count)
    }

    /// List all MCP tools from a specific server.
    pub fn list_by_server(&self, server: &str) -> Vec<RegisteredTool> {
        let inner = self.inner().read().unwrap();
        inner
            .tools
            .iter()
            .filter(|rt| rt.source() == ToolSource::Mcp && rt.origin() == Some(server))
            .cloned()
            .collect()
    }

    /// Remove all tools from a specific MCP server.
    ///
    /// Returns the count of removed tools.
    pub fn remove_server_tools(&self, server: &str) -> usize {
        let tools_to_remove: Vec<String> = self
            .list_by_server(server)
            .into_iter()
            .map(|t| t.name().to_string())
            .collect();

        let mut count = 0;
        for name in tools_to_remove {
            if self.remove(&name).is_ok() {
                count += 1;
            }
        }

        count
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use umf::{McpInputSchema, McpToolAnnotations};

    fn sample_mcp_tool(name: &str) -> McpTool {
        McpTool {
            name: name.to_string(),
            title: Some(format!("Tool: {}", name)),
            description: Some(format!("Description for {}", name)),
            input_schema: McpInputSchema::object(
                json!({
                    "query": {"type": "string"}
                }),
                vec!["query".to_string()],
            ),
            output_schema: None,
            annotations: Some(McpToolAnnotations {
                read_only_hint: Some(true),
                ..Default::default()
            }),
        }
    }

    #[test]
    fn test_register_mcp_tool() {
        let registry = ToolRegistry::new();

        registry
            .register_mcp(sample_mcp_tool("get_weather"), "weather-server")
            .unwrap();

        assert_eq!(registry.len(), 1);

        let tool = registry.find("get_weather").unwrap();
        assert!(tool.is_mcp());
        assert_eq!(tool.origin(), Some("weather-server"));
    }

    #[test]
    fn test_mcp_tool_conversion() {
        let registry = ToolRegistry::new();

        let mcp_tool = sample_mcp_tool("search");
        registry.register_mcp(mcp_tool, "search-server").unwrap();

        let internal_tools = registry.to_internal_tools();
        assert_eq!(internal_tools.len(), 1);

        let tool = &internal_tools[0];
        assert_eq!(tool.name, "search");
        assert!(tool.has_metadata("mcp_annotations"));
    }

    #[test]
    fn test_register_mcp_batch() {
        let registry = ToolRegistry::new();

        let tools = vec![
            sample_mcp_tool("tool_a"),
            sample_mcp_tool("tool_b"),
            sample_mcp_tool("tool_c"),
        ];

        let count = registry
            .register_mcp_batch(tools, "batch-server")
            .unwrap();
        assert_eq!(count, 3);
        assert_eq!(registry.len(), 3);
    }

    #[test]
    fn test_batch_skips_conflicts() {
        let registry = ToolRegistry::new();

        // Register one tool first
        registry
            .register_mcp(sample_mcp_tool("existing"), "server-1")
            .unwrap();

        // Batch with a conflict
        let tools = vec![
            sample_mcp_tool("new_tool"),
            sample_mcp_tool("existing"), // This should be skipped
            sample_mcp_tool("another"),
        ];

        let count = registry.register_mcp_batch(tools, "server-2").unwrap();
        assert_eq!(count, 2); // Only 2 registered, 1 skipped
        assert_eq!(registry.len(), 3);
    }

    #[test]
    fn test_list_by_server() {
        let registry = ToolRegistry::new();

        registry
            .register_mcp(sample_mcp_tool("tool_1"), "server-a")
            .unwrap();
        registry
            .register_mcp(sample_mcp_tool("tool_2"), "server-a")
            .unwrap();
        registry
            .register_mcp(sample_mcp_tool("tool_3"), "server-b")
            .unwrap();

        let server_a = registry.list_by_server("server-a");
        assert_eq!(server_a.len(), 2);

        let server_b = registry.list_by_server("server-b");
        assert_eq!(server_b.len(), 1);
    }

    #[test]
    fn test_remove_server_tools() {
        let registry = ToolRegistry::new();

        registry
            .register_mcp(sample_mcp_tool("a1"), "server-a")
            .unwrap();
        registry
            .register_mcp(sample_mcp_tool("a2"), "server-a")
            .unwrap();
        registry
            .register_mcp(sample_mcp_tool("b1"), "server-b")
            .unwrap();

        let removed = registry.remove_server_tools("server-a");
        assert_eq!(removed, 2);
        assert_eq!(registry.len(), 1);
        assert!(registry.contains("b1"));
    }

    #[test]
    fn test_mixed_native_and_mcp() {
        use umf::InternalTool;

        let registry = ToolRegistry::new();

        // Register native tool
        registry
            .register_native(InternalTool::new("native_tool", "Native", json!({})))
            .unwrap();

        // Register MCP tool
        registry
            .register_mcp(sample_mcp_tool("mcp_tool"), "my-server")
            .unwrap();

        assert_eq!(registry.len(), 2);
        assert_eq!(registry.native_tools().len(), 1);
        assert_eq!(registry.mcp_tools().len(), 1);

        // Conflict between different sources
        let err = registry
            .register_mcp(sample_mcp_tool("native_tool"), "another-server")
            .unwrap_err();
        assert!(matches!(
            err,
            RegistryError::Conflict {
                existing_source: ToolSource::Native,
                ..
            }
        ));
    }
}

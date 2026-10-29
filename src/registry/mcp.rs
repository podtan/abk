//! MCP tool registration support.
//!
//! This module provides methods for registering tools from MCP servers.
//! Requires the `registry-mcp` feature.

use umf::{McpTool, ToInternal};

use super::{RegisteredTool, RegistryError, RegistryResult, ToolRegistry, ToolSource};

/// Namespaced MCP tool name: `mcp__<server>__<tool>` — the standard MCP
/// client pattern (issue 76cbecf0). Server-name namespacing lets multiple
/// same-service servers coexist in one registry; keyed by raw tool name
/// alone, the second same-named server was SILENTLY shadowed
/// (registration-order won).
pub fn mcp_tool_name(server: &str, tool: &str) -> String {
    format!("mcp__{}__{}", server, tool)
}

/// Strip the `mcp__<server>__` prefix from a namespaced tool name,
/// returning the RAW tool name the remote server expects.
///
/// Returns `None` when `name` does not carry `server`'s prefix.
pub fn strip_mcp_server_prefix<'a>(server: &str, name: &'a str) -> Option<&'a str> {
    name.strip_prefix(format!("mcp__{}__", server).as_str())
}

impl ToolRegistry {
    /// Register an MCP tool from a server.
    ///
    /// The tool is converted to internal format, NAMESPACED with the server
    /// name (`mcp__<server>__<tool>` — issue 76cbecf0), and stored with the
    /// MCP source and server name.
    ///
    /// # Arguments
    ///
    /// * `tool` - The MCP tool to register
    /// * `server` - The name/identifier of the MCP server
    ///
    /// # Errors
    ///
    /// Returns an error if a tool with the same (namespaced) name already
    /// exists.
    pub fn register_mcp(&self, tool: McpTool, server: impl Into<String>) -> RegistryResult<()> {
        let server = server.into();
        let mut internal = tool.to_internal();

        // 76cbecf0: namespace by server name — two same-service servers
        // with distinct names coexist in one registry; the raw name alone
        // silently shadowed the second server.
        internal.name = mcp_tool_name(&server, &internal.name);

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
    /// may already exist. With server-name namespacing (76cbecf0), conflicts
    /// only occur when two configured servers share the SAME name — the
    /// caller (McpToolLoader) warns loud on every skipped tool.
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

    /// Fetch tools from an MCP server and register them.
    ///
    /// This method connects to the MCP server, fetches the available tools,
    /// and registers them in the registry. Existing tools with conflicting
    /// names are skipped.
    ///
    /// # Arguments
    ///
    /// * `config` - The MCP server configuration
    ///
    /// # Returns
    ///
    /// The count of successfully registered tools.
    ///
    /// # Errors
    ///
    /// Returns an error if the server is unreachable or returns invalid data.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// use abk::registry::{ToolRegistry, McpServerConfig};
    ///
    /// let registry = ToolRegistry::new();
    /// let config = McpServerConfig::new("pdt", "http://127.0.0.1:8000/pdt");
    ///
    /// // Fetch and register tools
    /// let count = registry.fetch_from_server(&config).await?;
    /// println!("Registered {} tools from MCP server", count);
    /// ```
    pub async fn fetch_from_server(
        &self,
        config: &super::McpServerConfig,
    ) -> RegistryResult<usize> {
        use super::McpClient;

        let client = McpClient::new();
        let tools = client.fetch_tools(config).await?;

        self.register_mcp_batch(tools, &config.name)
    }

    /// Fetch tools from an MCP server with initialization.
    ///
    /// Like `fetch_from_server`, but also sends initialize/initialized
    /// messages first (required by some MCP servers).
    pub async fn fetch_from_server_with_init(
        &self,
        config: &super::McpServerConfig,
    ) -> RegistryResult<usize> {
        use super::McpClient;

        let client = McpClient::new();
        let tools = client.fetch_tools_with_init(config).await?;

        self.register_mcp_batch(tools, &config.name)
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

        // 76cbecf0: the stored name is the namespaced form.
        let tool = registry
            .find(&mcp_tool_name("weather-server", "get_weather"))
            .unwrap();
        assert!(tool.is_mcp());
        assert_eq!(tool.origin(), Some("weather-server"));
        assert!(registry.find("get_weather").is_none());
    }

    #[test]
    fn test_mcp_name_helpers() {
        let namespaced = mcp_tool_name("thq", "thq_list_agents");
        assert_eq!(namespaced, "mcp__thq__thq_list_agents");
        assert_eq!(
            strip_mcp_server_prefix("thq", &namespaced),
            Some("thq_list_agents")
        );
        // A different server's prefix does not strip.
        assert_eq!(strip_mcp_server_prefix("other", &namespaced), None);
        // A raw (un-namespaced) name does not strip.
        assert_eq!(strip_mcp_server_prefix("thq", "thq_list_agents"), None);
    }

    #[test]
    fn test_mcp_tool_conversion() {
        let registry = ToolRegistry::new();

        let mcp_tool = sample_mcp_tool("search");
        registry.register_mcp(mcp_tool, "search-server").unwrap();

        let internal_tools = registry.to_internal_tools();
        assert_eq!(internal_tools.len(), 1);

        let tool = &internal_tools[0];
        assert_eq!(tool.name, mcp_tool_name("search-server", "search"));
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

        // 76cbecf0: a DIFFERENT server name never conflicts — both toolsets
        // coexist (the old raw-name keying silently shadowed server-2).
        let tools = vec![
            sample_mcp_tool("new_tool"),
            sample_mcp_tool("existing"),
            sample_mcp_tool("another"),
        ];

        let count = registry.register_mcp_batch(tools, "server-2").unwrap();
        assert_eq!(count, 3);
        assert_eq!(registry.len(), 4);
        assert!(registry.contains(&mcp_tool_name("server-1", "existing")));
        assert!(registry.contains(&mcp_tool_name("server-2", "existing")));

        // The SAME server name collides on the namespaced form → skipped.
        let dup = vec![sample_mcp_tool("existing"), sample_mcp_tool("fresh")];
        let count2 = registry.register_mcp_batch(dup, "server-1").unwrap();
        assert_eq!(count2, 1); // only "fresh" registered; "existing" shadowed
        assert!(registry.contains(&mcp_tool_name("server-1", "fresh")));
    }

    #[test]
    fn test_same_service_servers_coexist() {
        // 76cbecf0 acceptance regression: two same-service servers with
        // distinct names → BOTH toolsets reachable in one registry.
        let registry = ToolRegistry::new();

        let tanbal = vec![
            sample_mcp_tool("thq_list_agent-identitys"),
            sample_mcp_tool("thq_get_agent-identity"),
        ];
        let eynak = vec![
            sample_mcp_tool("thq_list_agent-identitys"),
            sample_mcp_tool("thq_get_agent-identity"),
        ];

        assert_eq!(registry.register_mcp_batch(tanbal, "thq").unwrap(), 2);
        assert_eq!(
            registry
                .register_mcp_batch(eynak, "eynak-thq")
                .unwrap(),
            2
        );

        assert_eq!(registry.len(), 4);
        assert!(registry.contains(&mcp_tool_name("thq", "thq_list_agent-identitys")));
        assert!(registry.contains(&mcp_tool_name("eynak-thq", "thq_list_agent-identitys")));
        assert_eq!(registry.list_by_server("thq").len(), 2);
        assert_eq!(registry.list_by_server("eynak-thq").len(), 2);
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
        // 76cbecf0: stored names are namespaced — the raw form is gone.
        assert!(registry.contains(&mcp_tool_name("server-b", "b1")));
        assert!(registry.contains("b1") == false);
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

        // 76cbecf0: MCP names are namespaced — an MCP tool whose raw name
        // matches a native tool no longer collides (different namespaces).
        registry
            .register_mcp(sample_mcp_tool("native_tool"), "another-server")
            .unwrap();
        assert_eq!(registry.len(), 3);
        assert!(registry.contains("native_tool"));
        assert!(registry.contains(&mcp_tool_name("another-server", "native_tool")));

        // Conflict between two MCP tools under the SAME server name.
        let err = registry
            .register_mcp(sample_mcp_tool("mcp_tool"), "my-server")
            .unwrap_err();
        assert!(matches!(
            err,
            RegistryError::Conflict {
                existing_source: ToolSource::Mcp,
                ..
            }
        ));
    }
}

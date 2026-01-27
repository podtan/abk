//! MCP tool integration for the agent.
//!
//! This module provides functionality to load tools from MCP servers
//! and convert them to OpenAI function schemas for LLM consumption.

#[cfg(feature = "registry-mcp")]
use crate::config::McpConfig;
#[cfg(feature = "registry-mcp")]
use crate::registry::{McpClient, McpServerConfig as RegistryServerConfig, McpToolCallResult, ToolRegistry};
#[cfg(feature = "registry-mcp")]
use anyhow::Result;
#[cfg(feature = "registry-mcp")]
use std::collections::HashMap;

/// MCP tools container that can be merged with CATS tools.
#[cfg(feature = "registry-mcp")]
pub struct McpToolLoader {
    /// Registry holding the loaded MCP tools
    pub registry: ToolRegistry,
    /// Number of tools loaded
    pub tool_count: usize,
    /// Server configurations indexed by server name
    server_configs: HashMap<String, RegistryServerConfig>,
    /// HTTP client for making tool calls
    client: McpClient,
}

#[cfg(feature = "registry-mcp")]
impl McpToolLoader {
    /// Create a new MCP tool loader and fetch tools from configured servers.
    ///
    /// # Arguments
    /// * `config` - The MCP configuration from the agent config
    ///
    /// # Returns
    /// A new McpToolLoader with tools fetched from all configured servers.
    pub async fn new(config: &McpConfig) -> Result<Self> {
        let registry = ToolRegistry::new();
        let mut total_tools = 0;
        let mut server_configs = HashMap::new();
        let client = McpClient::new();

        if !config.enabled {
            return Ok(Self {
                registry,
                tool_count: 0,
                server_configs,
                client,
            });
        }

        for server in &config.servers {
            // Skip non-HTTP transports for now
            if server.transport != "http" {
                eprintln!(
                    "Warning: MCP server '{}' uses unsupported transport '{}', skipping",
                    server.name, server.transport
                );
                continue;
            }

            // Convert config to registry client config
            let mut client_config = RegistryServerConfig::new(&server.name, &server.url);

            // Add auth token if configured (with env var substitution)
            if let Some(ref token) = server.auth_token {
                let resolved_token = resolve_env_var(token);
                client_config = client_config.with_auth(resolved_token);
            }

            // Store the server config for later tool calls
            server_configs.insert(server.name.clone(), client_config.clone());

            // Fetch tools from server
            let result = if server.auto_init {
                client.fetch_tools_with_init(&client_config).await
            } else {
                client.fetch_tools(&client_config).await
            };

            match result {
                Ok(tools) => {
                    match registry.register_mcp_batch(tools, &server.name) {
                        Ok(registered) => {
                            total_tools += registered;
                            println!(
                                "✓ Loaded {} tools from MCP server '{}'",
                                registered, server.name
                            );
                        }
                        Err(e) => {
                            eprintln!(
                                "Warning: Failed to register tools from '{}': {}",
                                server.name, e
                            );
                        }
                    }
                }
                Err(e) => {
                    eprintln!(
                        "Warning: Failed to fetch tools from MCP server '{}': {}",
                        server.name, e
                    );
                }
            }
        }

        Ok(Self {
            registry,
            tool_count: total_tools,
            server_configs,
            client,
        })
    }

    /// Get all MCP tools as OpenAI function schemas.
    ///
    /// This converts the internal tool format to OpenAI's function calling format
    /// for consumption by the LLM.
    pub fn get_openai_schemas(&self) -> Vec<serde_json::Value> {
        self.registry
            .to_internal_tools()
            .into_iter()
            .map(|tool| {
                serde_json::json!({
                    "type": "function",
                    "function": {
                        "name": tool.name,
                        "description": tool.description,
                        "parameters": tool.parameters
                    }
                })
            })
            .collect()
    }

    /// Check if any tools were loaded.
    pub fn has_tools(&self) -> bool {
        self.tool_count > 0
    }

    /// Check if a tool is an MCP tool (exists in this registry).
    pub fn is_mcp_tool(&self, tool_name: &str) -> bool {
        self.registry.contains(tool_name)
    }

    /// Get the server name for a tool.
    pub fn get_tool_server(&self, tool_name: &str) -> Option<String> {
        self.registry.find(tool_name).and_then(|t| t.origin().map(String::from))
    }

    /// Execute an MCP tool by calling the remote server.
    ///
    /// # Arguments
    /// * `tool_name` - The name of the tool to execute
    /// * `arguments` - The arguments as a JSON string
    ///
    /// # Returns
    /// The tool result containing content and success status.
    pub async fn execute_tool(
        &self,
        tool_name: &str,
        arguments: &str,
    ) -> Result<McpToolExecutionResult> {
        // Find which server this tool belongs to
        let server_name = self
            .get_tool_server(tool_name)
            .ok_or_else(|| anyhow::anyhow!("Tool '{}' not found in MCP registry", tool_name))?;

        // Get the server config
        let server_config = self
            .server_configs
            .get(&server_name)
            .ok_or_else(|| anyhow::anyhow!("Server config for '{}' not found", server_name))?;

        // Parse arguments
        let args: serde_json::Value = serde_json::from_str(arguments)
            .unwrap_or_else(|_| serde_json::json!({}));

        // Call the tool
        let result = self
            .client
            .call_tool(server_config, tool_name, args)
            .await
            .map_err(|e| anyhow::anyhow!("MCP tool call failed: {}", e))?;

        Ok(McpToolExecutionResult {
            content: result.content,
            success: !result.is_error,
        })
    }
}

/// Result from executing an MCP tool.
#[cfg(feature = "registry-mcp")]
pub struct McpToolExecutionResult {
    /// The text content of the result.
    pub content: String,
    /// Whether the execution was successful.
    pub success: bool,
}

/// Resolve environment variable references in a string.
///
/// Supports patterns like `${VAR_NAME}` and replaces them with
/// the corresponding environment variable value.
#[cfg(feature = "registry-mcp")]
fn resolve_env_var(value: &str) -> String {
    if value.starts_with("${") && value.ends_with('}') {
        let var_name = &value[2..value.len() - 1];
        std::env::var(var_name).unwrap_or_else(|_| value.to_string())
    } else {
        value.to_string()
    }
}

#[cfg(all(test, feature = "registry-mcp"))]
mod tests {
    use super::*;

    #[test]
    fn test_resolve_env_var() {
        // Set a test env var
        std::env::set_var("TEST_MCP_VAR", "test_value");

        assert_eq!(resolve_env_var("${TEST_MCP_VAR}"), "test_value");
        assert_eq!(resolve_env_var("plain_value"), "plain_value");
        assert_eq!(
            resolve_env_var("${NONEXISTENT_VAR}"),
            "${NONEXISTENT_VAR}"
        );

        std::env::remove_var("TEST_MCP_VAR");
    }

    #[tokio::test]
    async fn test_mcp_loader_disabled() {
        let config = McpConfig {
            enabled: false,
            timeout_seconds: 30,
            servers: vec![],
        };

        let loader = McpToolLoader::new(&config).await.unwrap();
        assert_eq!(loader.tool_count, 0);
        assert!(!loader.has_tools());
    }
}

//! MCP Tool Source - wraps MCP server connections.
//!
//! This module provides `McpToolSource` which implements `ToolSourceProvider`
//! by connecting to MCP (Model Context Protocol) servers for remote tool execution.

use std::collections::HashMap;

use async_trait::async_trait;

use super::{McpClient, McpServerConfig};
use super::{ToolDescriptor, ToolSourceProvider};
use crate::registry::provider::ToolResult as ProviderResult;

/// Convert McpInputSchema to a JSON schema Value.
fn schema_to_json(schema: &umf::McpInputSchema) -> serde_json::Value {
    let mut map = serde_json::Map::new();
    map.insert("type".to_string(), serde_json::json!(schema.schema_type));
    
    if let Some(ref props) = schema.properties {
        map.insert("properties".to_string(), props.clone());
    }
    
    if !schema.required.is_empty() {
        map.insert("required".to_string(), serde_json::json!(schema.required));
    }
    
    if let Some(ref additional) = schema.additional {
        if let serde_json::Value::Object(additional_map) = additional {
            for (k, v) in additional_map {
                map.insert(k.clone(), v.clone());
            }
        }
    }
    
    serde_json::Value::Object(map)
}

/// A tool source that connects to MCP servers.
///
/// This provides tools from MCP servers for remote execution. The source
/// fetches tool schemas from the server and executes tools via JSON-RPC.
///
/// # Example
///
/// ```rust,ignore
/// use abk::registry::{McpToolSource, McpServerConfig, ToolSourceProvider};
///
/// let config = McpServerConfig::new("pdt", "http://127.0.0.1:8000/pdt")
///     .with_auth("${PDT_TOKEN}");
///
/// let source = McpToolSource::new(config, true).await?;
///
/// // Get all tool descriptors
/// let tools = source.tool_descriptors();
///
/// // Execute a tool
/// let result = source.execute("pdt_pdt_searchAssets", json!({"q": "test"})).await?;
/// ```
pub struct McpToolSource {
    /// Source identifier (e.g., "mcp-pdt")
    name: String,
    /// Server configuration
    config: McpServerConfig,
    /// HTTP client for MCP communication
    client: McpClient,
    /// Cached tool descriptors
    descriptors: Vec<ToolDescriptor>,
    /// Tool name to index map for fast lookup
    tool_index: HashMap<String, usize>,
}

impl McpToolSource {
    /// Create a new MCP tool source.
    ///
    /// This connects to the MCP server and fetches available tools.
    ///
    /// # Arguments
    /// * `config` - The MCP server configuration
    /// * `auto_init` - Whether to send initialize/initialized messages first
    ///
    /// # Returns
    /// A new McpToolSource with tools fetched from the server.
    ///
    /// # Errors
    /// Returns an error if the server is unreachable or returns invalid data.
    pub async fn new(config: McpServerConfig, auto_init: bool) -> anyhow::Result<Self> {
        let client = McpClient::new();
        let name = format!("mcp-{}", config.name);

        // Fetch tools from server
        let mcp_tools = if auto_init {
            client.fetch_tools_with_init(&config).await?
        } else {
            client.fetch_tools(&config).await?
        };

        // Convert to descriptors
        let descriptors: Vec<ToolDescriptor> = mcp_tools
            .iter()
            .map(|t| {
                ToolDescriptor::new(
                    &t.name,
                    t.description.as_deref().unwrap_or(&t.name),
                    schema_to_json(&t.input_schema),
                    &name,
                )
            })
            .collect();

        // Build index
        let tool_index: HashMap<String, usize> = descriptors
            .iter()
            .enumerate()
            .map(|(i, d)| (d.name.clone(), i))
            .collect();

        Ok(Self {
            name,
            config,
            client,
            descriptors,
            tool_index,
        })
    }

    /// Create an MCP tool source from pre-fetched tools.
    ///
    /// This is useful when tools are already available (e.g., from a previous fetch).
    pub fn from_tools(
        config: McpServerConfig,
        tools: Vec<umf::McpTool>,
    ) -> Self {
        let client = McpClient::new();
        let name = format!("mcp-{}", config.name);

        let descriptors: Vec<ToolDescriptor> = tools
            .iter()
            .map(|t| {
                ToolDescriptor::new(
                    &t.name,
                    t.description.as_deref().unwrap_or(&t.name),
                    schema_to_json(&t.input_schema),
                    &name,
                )
            })
            .collect();

        let tool_index: HashMap<String, usize> = descriptors
            .iter()
            .enumerate()
            .map(|(i, d)| (d.name.clone(), i))
            .collect();

        Self {
            name,
            config,
            client,
            descriptors,
            tool_index,
        }
    }

    /// Refresh tools from the server.
    ///
    /// This re-fetches the tool list from the MCP server.
    pub async fn refresh(&mut self, auto_init: bool) -> anyhow::Result<usize> {
        let mcp_tools = if auto_init {
            self.client.fetch_tools_with_init(&self.config).await?
        } else {
            self.client.fetch_tools(&self.config).await?
        };

        let count = mcp_tools.len();

        self.descriptors = mcp_tools
            .iter()
            .map(|t| {
                ToolDescriptor::new(
                    &t.name,
                    t.description.as_deref().unwrap_or(&t.name),
                    schema_to_json(&t.input_schema),
                    &self.name,
                )
            })
            .collect();

        self.tool_index = self
            .descriptors
            .iter()
            .enumerate()
            .map(|(i, d)| (d.name.clone(), i))
            .collect();

        Ok(count)
    }

    /// Get the server configuration.
    pub fn config(&self) -> &McpServerConfig {
        &self.config
    }

    /// Get the number of tools from this source.
    pub fn tool_count(&self) -> usize {
        self.descriptors.len()
    }
}

#[async_trait]
impl ToolSourceProvider for McpToolSource {
    fn name(&self) -> &str {
        &self.name
    }

    fn tool_descriptors(&self) -> Vec<ToolDescriptor> {
        self.descriptors.clone()
    }

    async fn execute(
        &self,
        tool_name: &str,
        args: serde_json::Value,
    ) -> anyhow::Result<ProviderResult> {
        let result = self.client.call_tool(&self.config, tool_name, args).await?;

        Ok(ProviderResult {
            success: !result.is_error,
            content: result.content,
            data: Some(result.raw_content),
        })
    }

    fn has_tool(&self, name: &str) -> bool {
        self.tool_index.contains_key(name)
    }
}

/// Builder for creating MCP tool sources with configuration.
pub struct McpToolSourceBuilder {
    name: String,
    url: String,
    auth_token: Option<String>,
    auto_init: bool,
}

impl McpToolSourceBuilder {
    /// Create a new builder for an MCP tool source.
    pub fn new(name: impl Into<String>, url: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            url: url.into(),
            auth_token: None,
            auto_init: false,
        }
    }

    /// Set the authentication token.
    pub fn with_auth(mut self, token: impl Into<String>) -> Self {
        self.auth_token = Some(token.into());
        self
    }

    /// Enable automatic initialization.
    pub fn with_auto_init(mut self, enabled: bool) -> Self {
        self.auto_init = enabled;
        self
    }

    /// Build the MCP tool source.
    pub async fn build(self) -> anyhow::Result<McpToolSource> {
        let mut config = McpServerConfig::new(self.name, self.url);
        if let Some(token) = self.auth_token {
            config = config.with_auth(token);
        }

        McpToolSource::new(config, self.auto_init).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_builder_creation() {
        let builder = McpToolSourceBuilder::new("test", "http://localhost:8000");
        assert_eq!(builder.name, "test");
        assert_eq!(builder.url, "http://localhost:8000");
        assert!(builder.auth_token.is_none());
        assert!(!builder.auto_init);
    }

    #[test]
    fn test_builder_with_options() {
        let builder = McpToolSourceBuilder::new("test", "http://localhost:8000")
            .with_auth("secret")
            .with_auto_init(true);

        assert_eq!(builder.auth_token, Some("secret".to_string()));
        assert!(builder.auto_init);
    }

    #[test]
    fn test_from_tools() {
        use umf::{McpInputSchema, McpTool};

        let tools = vec![
            McpTool::from_schema(
                "test_tool".to_string(),
                "A test tool".to_string(),
                serde_json::json!({"type": "object"}),
            ),
        ];

        let config = McpServerConfig::new("test-server", "http://localhost:8000");
        let source = McpToolSource::from_tools(config, tools);

        assert_eq!(source.name(), "mcp-test-server");
        assert!(source.has_tool("test_tool"));
        assert_eq!(source.tool_count(), 1);
    }
}

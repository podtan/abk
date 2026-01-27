//! MCP Client for fetching tools from MCP servers.
//!
//! This module provides an async client to connect to MCP servers
//! and fetch available tools using the JSON-RPC protocol.

use super::{RegistryError, RegistryResult};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use umf::McpTool;

/// MCP Server configuration
#[derive(Debug, Clone)]
pub struct McpServerConfig {
    /// Server name/identifier
    pub name: String,
    /// Server base URL (e.g., "http://127.0.0.1:8000/pdt")
    pub url: String,
    /// Optional authentication token
    pub auth_token: Option<String>,
}

impl McpServerConfig {
    /// Create a new MCP server configuration.
    pub fn new(name: impl Into<String>, url: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            url: url.into(),
            auth_token: None,
        }
    }

    /// Set an authentication token.
    pub fn with_auth(mut self, token: impl Into<String>) -> Self {
        self.auth_token = Some(token.into());
        self
    }
}

/// JSON-RPC request structure
#[derive(Debug, Serialize)]
struct JsonRpcRequest {
    jsonrpc: String,
    id: u64,
    method: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    params: Option<Value>,
}

/// JSON-RPC response structure
#[derive(Debug, Deserialize)]
struct JsonRpcResponse {
    #[allow(dead_code)]
    jsonrpc: String,
    #[allow(dead_code)]
    id: Option<Value>,
    result: Option<Value>,
    error: Option<JsonRpcError>,
}

/// JSON-RPC error structure
#[derive(Debug, Deserialize)]
struct JsonRpcError {
    #[allow(dead_code)]
    code: i32,
    message: String,
    #[allow(dead_code)]
    data: Option<Value>,
}

/// MCP Tool as returned from the server
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct McpToolResponse {
    name: String,
    description: String,
    input_schema: Value,
    #[serde(default)]
    annotations: Option<McpToolAnnotationsResponse>,
}

/// MCP Tool annotations as returned from the server
#[derive(Debug, Deserialize)]
struct McpToolAnnotationsResponse {
    title: Option<String>,
    #[serde(rename = "readOnlyHint")]
    read_only_hint: Option<bool>,
    #[serde(rename = "destructiveHint")]
    destructive_hint: Option<bool>,
    #[serde(rename = "idempotentHint")]
    idempotent_hint: Option<bool>,
    #[serde(rename = "openWorldHint")]
    open_world_hint: Option<bool>,
}

/// Tools list response from MCP server
#[derive(Debug, Deserialize)]
struct ToolsListResult {
    tools: Vec<McpToolResponse>,
}

/// MCP Client for communicating with MCP servers.
pub struct McpClient {
    http_client: reqwest::Client,
}

impl Default for McpClient {
    fn default() -> Self {
        Self::new()
    }
}

impl McpClient {
    /// Create a new MCP client.
    pub fn new() -> Self {
        Self {
            http_client: reqwest::Client::new(),
        }
    }

    /// Fetch tools from an MCP server.
    ///
    /// This sends a `tools/list` JSON-RPC request to the server's
    /// message endpoint and parses the response.
    ///
    /// # Arguments
    ///
    /// * `config` - The MCP server configuration
    ///
    /// # Returns
    ///
    /// A list of `McpTool` objects from the server.
    ///
    /// # Errors
    ///
    /// Returns an error if the server is unreachable or returns invalid data.
    pub async fn fetch_tools(&self, config: &McpServerConfig) -> RegistryResult<Vec<McpTool>> {
        // Construct the message endpoint URL
        let message_url = format!("{}/message", config.url.trim_end_matches('/'));

        // Build the JSON-RPC request
        let request = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: 1,
            method: "tools/list".to_string(),
            params: Some(json!({})),
        };

        // Build HTTP request
        let mut http_request = self.http_client.post(&message_url).json(&request);

        // Add authentication if configured
        if let Some(ref token) = config.auth_token {
            http_request = http_request.header("Authorization", format!("Bearer {}", token));
        }

        // Send request
        let response = http_request
            .send()
            .await
            .map_err(|e| RegistryError::McpServerError {
                server: config.name.clone(),
                message: format!("HTTP request failed: {}", e),
            })?;

        // Check HTTP status
        if !response.status().is_success() {
            return Err(RegistryError::McpServerError {
                server: config.name.clone(),
                message: format!("HTTP {} - {}", response.status(), response.status().as_str()),
            });
        }

        // Parse JSON-RPC response
        let rpc_response: JsonRpcResponse =
            response
                .json()
                .await
                .map_err(|e| RegistryError::McpServerError {
                    server: config.name.clone(),
                    message: format!("Failed to parse JSON response: {}", e),
                })?;

        // Check for RPC error
        if let Some(error) = rpc_response.error {
            return Err(RegistryError::McpServerError {
                server: config.name.clone(),
                message: format!("MCP error: {}", error.message),
            });
        }

        // Parse result
        let result = rpc_response
            .result
            .ok_or_else(|| RegistryError::McpServerError {
                server: config.name.clone(),
                message: "No result in response".to_string(),
            })?;

        let tools_result: ToolsListResult =
            serde_json::from_value(result).map_err(|e| RegistryError::McpServerError {
                server: config.name.clone(),
                message: format!("Failed to parse tools list: {}", e),
            })?;

        // Convert to McpTool
        let tools = tools_result
            .tools
            .into_iter()
            .map(|t| {
                let mut tool = McpTool::from_schema(t.name, t.description, t.input_schema);

                // Add annotations if present
                if let Some(annotations) = t.annotations {
                    if let Some(title) = annotations.title {
                        tool = tool.with_title(title);
                    }
                    if let Some(read_only) = annotations.read_only_hint {
                        tool = tool.with_read_only_hint(read_only);
                    }
                    if let Some(destructive) = annotations.destructive_hint {
                        tool = tool.with_destructive_hint(destructive);
                    }
                    if let Some(idempotent) = annotations.idempotent_hint {
                        tool = tool.with_idempotent_hint(idempotent);
                    }
                    if let Some(open_world) = annotations.open_world_hint {
                        tool = tool.with_open_world_hint(open_world);
                    }
                }

                tool
            })
            .collect();

        Ok(tools)
    }

    /// Initialize connection with an MCP server.
    ///
    /// This sends the `initialize` and `initialized` messages to
    /// establish a proper MCP session (required by some servers).
    ///
    /// # Arguments
    ///
    /// * `config` - The MCP server configuration
    ///
    /// # Errors
    ///
    /// Returns an error if initialization fails.
    pub async fn initialize(&self, config: &McpServerConfig) -> RegistryResult<()> {
        let message_url = format!("{}/message", config.url.trim_end_matches('/'));

        // Send initialize request
        let init_request = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: 1,
            method: "initialize".to_string(),
            params: Some(json!({
                "protocolVersion": "2024-11-05",
                "capabilities": {},
                "clientInfo": {
                    "name": "abk-mcp-client",
                    "version": env!("CARGO_PKG_VERSION")
                }
            })),
        };

        let mut http_request = self.http_client.post(&message_url).json(&init_request);

        if let Some(ref token) = config.auth_token {
            http_request = http_request.header("Authorization", format!("Bearer {}", token));
        }

        let response = http_request
            .send()
            .await
            .map_err(|e| RegistryError::McpServerError {
                server: config.name.clone(),
                message: format!("Initialize request failed: {}", e),
            })?;

        if !response.status().is_success() {
            return Err(RegistryError::McpServerError {
                server: config.name.clone(),
                message: format!("Initialize failed: HTTP {}", response.status()),
            });
        }

        // Send initialized notification
        let initialized_request = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: 2,
            method: "initialized".to_string(),
            params: None,
        };

        let mut http_request = self
            .http_client
            .post(&message_url)
            .json(&initialized_request);

        if let Some(ref token) = config.auth_token {
            http_request = http_request.header("Authorization", format!("Bearer {}", token));
        }

        let _ = http_request
            .send()
            .await
            .map_err(|e| RegistryError::McpServerError {
                server: config.name.clone(),
                message: format!("Initialized notification failed: {}", e),
            })?;

        Ok(())
    }

    /// Fetch tools with automatic initialization.
    ///
    /// This is a convenience method that initializes the connection
    /// before fetching tools.
    pub async fn fetch_tools_with_init(
        &self,
        config: &McpServerConfig,
    ) -> RegistryResult<Vec<McpTool>> {
        // Initialize first (some servers require this)
        if let Err(e) = self.initialize(config).await {
            // Log but don't fail - some servers work without initialize
            eprintln!("Warning: MCP initialize failed (continuing): {}", e);
        }

        self.fetch_tools(config).await
    }

    /// Call a tool on an MCP server.
    ///
    /// This sends a `tools/call` JSON-RPC request to execute a tool.
    ///
    /// # Arguments
    ///
    /// * `config` - The MCP server configuration
    /// * `tool_name` - The name of the tool to call
    /// * `arguments` - The arguments as a JSON value
    ///
    /// # Returns
    ///
    /// The tool result as a JSON value.
    pub async fn call_tool(
        &self,
        config: &McpServerConfig,
        tool_name: &str,
        arguments: Value,
    ) -> RegistryResult<McpToolCallResult> {
        let message_url = format!("{}/message", config.url.trim_end_matches('/'));

        let request = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: 1,
            method: "tools/call".to_string(),
            params: Some(json!({
                "name": tool_name,
                "arguments": arguments
            })),
        };

        let mut http_request = self.http_client.post(&message_url).json(&request);

        if let Some(ref token) = config.auth_token {
            http_request = http_request.header("Authorization", format!("Bearer {}", token));
        }

        let response = http_request
            .send()
            .await
            .map_err(|e| RegistryError::McpServerError {
                server: config.name.clone(),
                message: format!("Tool call request failed: {}", e),
            })?;

        if !response.status().is_success() {
            return Err(RegistryError::McpServerError {
                server: config.name.clone(),
                message: format!("Tool call failed: HTTP {}", response.status()),
            });
        }

        let rpc_response: JsonRpcResponse =
            response
                .json()
                .await
                .map_err(|e| RegistryError::McpServerError {
                    server: config.name.clone(),
                    message: format!("Failed to parse tool call response: {}", e),
                })?;

        if let Some(error) = rpc_response.error {
            return Err(RegistryError::McpServerError {
                server: config.name.clone(),
                message: format!("Tool call error: {}", error.message),
            });
        }

        let result = rpc_response
            .result
            .ok_or_else(|| RegistryError::McpServerError {
                server: config.name.clone(),
                message: "No result in tool call response".to_string(),
            })?;

        // Parse the MCP tool result format
        let is_error = result
            .get("isError")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        let content = result
            .get("content")
            .cloned()
            .unwrap_or_else(|| json!([]));

        // Extract text content from the content array
        let text_content = if let Some(arr) = content.as_array() {
            arr.iter()
                .filter_map(|item| {
                    if item.get("type").and_then(|t| t.as_str()) == Some("text") {
                        item.get("text").and_then(|t| t.as_str()).map(String::from)
                    } else {
                        None
                    }
                })
                .collect::<Vec<_>>()
                .join("\n")
        } else {
            content.to_string()
        };

        Ok(McpToolCallResult {
            content: text_content,
            is_error,
            raw_content: content,
        })
    }
}

/// Result from calling an MCP tool.
#[derive(Debug, Clone)]
pub struct McpToolCallResult {
    /// The text content of the result.
    pub content: String,
    /// Whether this result represents an error.
    pub is_error: bool,
    /// The raw content array from the MCP response.
    pub raw_content: Value,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_server_config_new() {
        let config = McpServerConfig::new("test-server", "http://localhost:8000");
        assert_eq!(config.name, "test-server");
        assert_eq!(config.url, "http://localhost:8000");
        assert!(config.auth_token.is_none());
    }

    #[test]
    fn test_server_config_with_auth() {
        let config =
            McpServerConfig::new("test-server", "http://localhost:8000").with_auth("secret-token");
        assert_eq!(config.auth_token, Some("secret-token".to_string()));
    }

    #[test]
    fn test_json_rpc_request_serialization() {
        let request = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: 1,
            method: "tools/list".to_string(),
            params: Some(json!({})),
        };

        let json = serde_json::to_string(&request).unwrap();
        assert!(json.contains("\"jsonrpc\":\"2.0\""));
        assert!(json.contains("\"method\":\"tools/list\""));
    }

    #[test]
    fn test_mcp_client_creation() {
        let client = McpClient::new();
        // Just verify creation doesn't panic
        let _ = client;
    }

    #[test]
    fn test_mcp_client_default() {
        let client = McpClient::default();
        let _ = client;
    }
}

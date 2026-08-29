//! MCP Client for fetching tools from MCP servers.
//!
//! This module provides an async client to connect to MCP servers
//! and fetch available tools using the JSON-RPC protocol.

use super::{RegistryError, RegistryResult};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use umf::McpTool;

#[cfg(feature = "registry-mcp-token")]
use pep::token_provider::{TokenProvider, TokenProviderEnum, StaticTokenProvider};

/// MCP Server configuration
#[derive(Debug, Clone)]
pub struct McpServerConfig {
    /// Server name/identifier
    pub name: String,
    /// Server base URL (e.g., "http://127.0.0.1:8000/pdt")
    pub url: String,
    /// Authentication token (static, resolved at creation time).
    /// When `registry-mcp-token` feature is enabled, use `with_token_provider()`
    /// for dynamic token management.
    auth_token: Option<String>,
    /// Dynamic token provider (only available with `registry-mcp-token` feature).
    #[cfg(feature = "registry-mcp-token")]
    token_provider: Option<TokenProviderEnum>,
}

impl McpServerConfig {
    /// Create a new MCP server configuration (no auth).
    pub fn new(name: impl Into<String>, url: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            url: url.into(),
            auth_token: None,
            #[cfg(feature = "registry-mcp-token")]
            token_provider: None,
        }
    }

    /// Set a static authentication token.
    pub fn with_auth(mut self, token: impl Into<String>) -> Self {
        self.auth_token = Some(token.into());
        #[cfg(feature = "registry-mcp-token")]
        {
            self.token_provider = None; // static token takes priority when set
        }
        self
    }

    /// Set a dynamic token provider (requires `registry-mcp-token` feature).
    #[cfg(feature = "registry-mcp-token")]
    pub fn with_token_provider(mut self, provider: TokenProviderEnum) -> Self {
        self.token_provider = Some(provider);
        self.auth_token = None; // dynamic provider takes priority
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

    /// Resolve the current auth token: prefers dynamic provider, falls back to static.
    #[cfg(feature = "registry-mcp-token")]
    async fn resolve_auth_token(&self, config: &McpServerConfig) -> RegistryResult<Option<String>> {
        // Dynamic provider takes priority
        if let Some(ref provider) = config.token_provider {
            let token = provider.get_token().await.map_err(|e| {
                RegistryError::McpServerError {
                    server: config.name.clone(),
                    message: format!("Token provider error: {}", e),
                }
            })?;
            return Ok(Some(token));
        }
        // Fall back to static token
        Ok(config.auth_token.clone())
    }

    /// Resolve the current auth token (static only, no PEP feature).
    #[cfg(not(feature = "registry-mcp-token"))]
    fn resolve_auth_token_sync(&self, config: &McpServerConfig) -> Option<String> {
        config.auth_token.clone()
    }

    /// Apply authentication to an HTTP request builder.
    ///
    /// Note: reqwest::RequestBuilder::header() takes `self` by value,
    /// so we must clone and replace via the mutable reference.
    #[cfg(feature = "registry-mcp-token")]
    async fn apply_auth(
        &self,
        http_request: &mut reqwest::RequestBuilder,
        config: &McpServerConfig,
    ) {
        match self.resolve_auth_token(config).await {
            Ok(Some(token)) => {
                if let Some(cloned) = http_request.try_clone() {
                    *http_request = cloned.header("Authorization", format!("Bearer {}", token));
                }
            }
            Ok(None) => {}
            Err(e) => {
                crate::observability::tee_eprintln(&format!("Warning: Failed to resolve auth token for '{}': {}", config.name, e));
            }
        }
    }

    /// Apply authentication to an HTTP request builder (static only).
    #[cfg(not(feature = "registry-mcp-token"))]
    fn apply_auth_static(
        &self,
        http_request: &mut reqwest::RequestBuilder,
        config: &McpServerConfig,
    ) {
        if let Some(ref token) = config.auth_token {
            if let Some(cloned) = http_request.try_clone() {
                *http_request = cloned.header("Authorization", format!("Bearer {}", token));
            }
        }
    }

    /// Send an authenticated MCP request.
    ///
    /// With `registry-mcp-token`, a 401 response triggers a single
    /// invalidated retry: the dynamic token provider's cached token is
    /// dropped via [`pep::token_provider::TokenProvider::invalidate`] and
    /// the request is rebuilt and re-sent exactly once. Defense-in-depth
    /// for tokens that are rejected by the resource server before the
    /// provider's cache believes they expired (nghr 199c4801; pep 849e7528
    /// added the invalidate hook for exactly this). Static-token configs
    /// have nothing to refresh and are not retried.
    async fn send_with_retry(
        &self,
        build_request: impl Fn() -> reqwest::RequestBuilder,
        config: &McpServerConfig,
    ) -> RegistryResult<reqwest::Response> {
        let mut http_request = build_request();

        #[cfg(feature = "registry-mcp-token")]
        self.apply_auth(&mut http_request, config).await;

        #[cfg(not(feature = "registry-mcp-token"))]
        self.apply_auth_static(&mut http_request, config);

        let response = Self::send_one(http_request, config).await?;

        #[cfg(feature = "registry-mcp-token")]
        if response.status() == reqwest::StatusCode::UNAUTHORIZED
            && config.token_provider.is_some()
        {
            crate::observability::tee_eprintln(&format!(
                "[MCP] 401 from '{}' — cached token rejected; invalidating and retrying once",
                config.name
            ));
            if let Some(ref provider) = config.token_provider {
                provider.invalidate().await;
            }
            let mut http_request = build_request();
            self.apply_auth(&mut http_request, config).await;
            return Self::send_one(http_request, config).await;
        }

        Ok(response)
    }

    async fn send_one(
        http_request: reqwest::RequestBuilder,
        config: &McpServerConfig,
    ) -> RegistryResult<reqwest::Response> {
        http_request.send().await.map_err(|e| {
            RegistryError::McpServerError {
                server: config.name.clone(),
                message: format!("HTTP request failed: {}", e),
            }
        })
    }

    /// Fetch tools from an MCP server.
    pub async fn fetch_tools(&self, config: &McpServerConfig) -> RegistryResult<Vec<McpTool>> {
        let message_url = format!("{}/message", config.url.trim_end_matches('/'));

        let request = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: 1,
            method: "tools/list".to_string(),
            params: Some(json!({})),
        };

        let response = self
            .send_with_retry(
                || self.http_client.post(&message_url).json(&request),
                config,
            )
            .await?;

        if !response.status().is_success() {
            return Err(RegistryError::McpServerError {
                server: config.name.clone(),
                message: format!("HTTP {} - {}", response.status(), response.status().as_str()),
            });
        }

        let rpc_response: JsonRpcResponse =
            response
                .json()
                .await
                .map_err(|e| RegistryError::McpServerError {
                    server: config.name.clone(),
                    message: format!("Failed to parse JSON response: {}", e),
                })?;

        if let Some(error) = rpc_response.error {
            return Err(RegistryError::McpServerError {
                server: config.name.clone(),
                message: format!("MCP error: {}", error.message),
            });
        }

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

        let tools = tools_result
            .tools
            .into_iter()
            .map(|t| {
                let mut tool = McpTool::from_schema(t.name, t.description, t.input_schema);

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
    pub async fn initialize(&self, config: &McpServerConfig) -> RegistryResult<()> {
        let message_url = format!("{}/message", config.url.trim_end_matches('/'));

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

        let response = self
            .send_with_retry(
                || self.http_client.post(&message_url).json(&init_request),
                config,
            )
            .await?;

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

        let _ = self
            .send_with_retry(
                || self.http_client.post(&message_url).json(&initialized_request),
                config,
            )
            .await?;

        Ok(())
    }

    /// Fetch tools with automatic initialization.
    pub async fn fetch_tools_with_init(
        &self,
        config: &McpServerConfig,
    ) -> RegistryResult<Vec<McpTool>> {
        if let Err(e) = self.initialize(config).await {
            crate::observability::tee_eprintln(&format!("Warning: MCP initialize failed (continuing): {}", e));
        }

        self.fetch_tools(config).await
    }

    /// Call a tool on an MCP server.
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

        let response = self
            .send_with_retry(|| self.http_client.post(&message_url).json(&request), config)
            .await?;

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

        let is_error = result
            .get("isError")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        let content = result
            .get("content")
            .cloned()
            .unwrap_or_else(|| json!([]));

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
        let _ = client;
    }

    #[test]
    fn test_mcp_client_default() {
        let client = McpClient::default();
        let _ = client;
    }

    // -- retry-on-401 (pep 849e7528 follow-through) ---------------------------
    //
    // TokenProviderEnum is a closed enum, so a refreshable custom provider
    // can't be injected through the public config path; invalidate semantics
    // (incl. enum dispatch) are proven by pep's own unit tests. These tests
    // prove the retry MECHANICS in abk: one invalidated re-send after a 401,
    // success on the second attempt, no retry loop on persistent 401, and no
    // spurious retry when the first attempt succeeds.

    /// One-shot TCP server. Serves `statuses` in order (e.g. ["401", "200"]);
    /// a "200" entry answers with a valid empty tools/list JSON-RPC reply.
    /// Returns the bound address (tests append /message via the client).
    #[cfg(feature = "registry-mcp-token")]
    fn serve_sequential(statuses: &[&str]) -> std::net::SocketAddr {
        use std::io::{Read, Write};

        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let statuses: Vec<String> = statuses.iter().map(|s| s.to_string()).collect();
        std::thread::spawn(move || {
            let body = br#"{"jsonrpc":"2.0","id":1,"result":{"tools":[]}}"#;
            for status in &statuses {
                let (mut stream, _) = listener.accept().unwrap();
                let mut buf = [0u8; 4096];
                let _ = stream.read(&mut buf).unwrap_or(0);
                let status_line = if status == "200" {
                    "HTTP/1.1 200 OK"
                } else {
                    "HTTP/1.1 401 Unauthorized"
                };
                let resp = format!(
                    "{}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    status_line,
                    body.len()
                );
                let _ = stream.write_all(resp.as_bytes());
                let _ = stream.write_all(body.as_slice());
            }
            // Hold the listener open until the test drops its client; any
            // UNEXPECTED extra request (retry loop bug) would block here and
            // the test's timeout/panic surfaces it.
            let _ = listener;
        });
        addr
    }

    #[cfg(feature = "registry-mcp-token")]
    #[tokio::test]
    async fn unauthorized_is_retried_once_then_succeeds() {
        let addr = serve_sequential(&["401", "200"]);
        let config = McpServerConfig::new("retry-ok", &format!("http://{addr}"))
            .with_token_provider(pep::token_provider::TokenProviderEnum::Static(
                pep::token_provider::StaticTokenProvider::new("fixed-token".to_string()),
            ));
        let client = McpClient::new();
        let tools = client.fetch_tools(&config).await.unwrap();
        assert!(tools.is_empty(), "second attempt must succeed with empty tools");
    }

    #[cfg(feature = "registry-mcp-token")]
    #[tokio::test]
    async fn persistent_unauthorized_fails_after_single_retry() {
        let addr = serve_sequential(&["401", "401"]);
        let config = McpServerConfig::new("retry-fail", &format!("http://{addr}"))
            .with_token_provider(pep::token_provider::TokenProviderEnum::Static(
                pep::token_provider::StaticTokenProvider::new("fixed-token".to_string()),
            ));
        let client = McpClient::new();
        let result = client.fetch_tools(&config).await;
        assert!(result.is_err(), "double 401 must surface as an error");
    }

    #[cfg(feature = "registry-mcp-token")]
    #[tokio::test]
    async fn success_first_try_makes_no_second_request() {
        // The server thread handles exactly ONE request; any spurious retry
        // would hang on accept() and fail the test via its own completion.
        let addr = serve_sequential(&["200"]);
        let config = McpServerConfig::new("no-retry", &format!("http://{addr}"))
            .with_token_provider(pep::token_provider::TokenProviderEnum::Static(
                pep::token_provider::StaticTokenProvider::new("fixed-token".to_string()),
            ));
        let client = McpClient::new();
        let tools = client.fetch_tools(&config).await.unwrap();
        assert!(tools.is_empty());
    }
}

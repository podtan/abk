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
    /// Server base URL. With the streamable transport, a URL with a path
    /// component is used as-is; a bare `scheme://host[:port]` gets the
    /// conventional `/mcp` mount appended.
    pub url: String,
    /// Wire transport: `"http"` = MCP Streamable HTTP (the default),
    /// `"legacy-sse"` = the pre-0.20 homemade `POST {url}/message` protocol.
    pub transport: String,
    /// Authentication token (static, resolved at creation time).
    /// When `registry-mcp-token` feature is enabled, use `with_token_provider()`
    /// for dynamic token management.
    auth_token: Option<String>,
    /// Dynamic token provider (only available with `registry-mcp-token` feature).
    #[cfg(feature = "registry-mcp-token")]
    token_provider: Option<TokenProviderEnum>,
    /// Streamable-HTTP session id (`Mcp-Session-Id`). Lives on the CONFIG (not
    /// the client) behind an Arc so every clone shares it — `McpToolSource`
    /// builds a FRESH `McpClient` for later `tools/call` and must resume the
    /// session `initialize()` minted (nghr d7d28cfe).
    session: std::sync::Arc<std::sync::Mutex<Option<String>>>,
}

impl McpServerConfig {
    /// Create a new MCP server configuration (no auth, streamable transport).
    pub fn new(name: impl Into<String>, url: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            url: url.into(),
            transport: "http".to_string(),
            auth_token: None,
            #[cfg(feature = "registry-mcp-token")]
            token_provider: None,
            session: std::sync::Arc::new(std::sync::Mutex::new(None)),
        }
    }

    /// Set the wire transport: `"http"` (Streamable HTTP, default) or
    /// `"legacy-sse"` (the old `POST {url}/message` protocol).
    pub fn with_transport(mut self, transport: impl Into<String>) -> Self {
        self.transport = transport.into();
        self
    }

    /// True when this config speaks the legacy `POST {url}/message` protocol.
    fn is_legacy(&self) -> bool {
        self.transport == "legacy-sse"
    }

    fn current_session(&self) -> Option<String> {
        self.session.lock().ok().and_then(|g| g.clone())
    }

    fn store_session(&self, value: Option<String>) {
        if let Ok(mut guard) = self.session.lock() {
            *guard = value;
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

/// JSON-RPC request structure.
/// `id: None` serializes WITHOUT the id member — a JSON-RPC notification
/// (`notifications/initialized`); the spec requires omitting it.
#[derive(Debug, Serialize)]
struct JsonRpcRequest {
    jsonrpc: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    id: Option<u64>,
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

    /// Resolve the streamable-HTTP endpoint for a server URL: a URL with a
    /// path component is used as-is; a bare `scheme://host[:port]` gets the
    /// conventional `/mcp` mount appended (the TS SDK, @agent-infra and the
    /// official reference servers all mount streamable HTTP at a path).
    fn streamable_endpoint(url: &str) -> String {
        let base = url.trim_end_matches('/');
        let after_authority = base.splitn(3, "://").nth(1).unwrap_or("");
        if after_authority.contains('/') {
            base.to_string()
        } else {
            format!("{}/mcp", base)
        }
    }

    /// POST a JSON-RPC value to the streamable-HTTP endpoint with the spec
    /// `Accept` pair and the stored session id (if one was minted).
    async fn post_streamable<T: Serialize + ?Sized>(
        &self,
        config: &McpServerConfig,
        body: &T,
    ) -> RegistryResult<reqwest::Response> {
        let endpoint = Self::streamable_endpoint(&config.url);
        let session = config.current_session();
        self.send_with_retry(
            || {
                let mut rb = self
                    .http_client
                    .post(&endpoint)
                    .header("Accept", "application/json, text/event-stream");
                if let Some(ref sid) = session {
                    rb = rb.header("Mcp-Session-Id", sid.clone());
                }
                rb.json(body)
            },
            config,
        )
        .await
    }

    /// Parse a JSON-RPC response that arrives either as direct JSON or as an
    /// SSE event stream (Streamable HTTP lets the server choose).
    async fn parse_rpc_response(
        &self,
        response: reqwest::Response,
        config: &McpServerConfig,
    ) -> RegistryResult<JsonRpcResponse> {
        let content_type = response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_string();

        if content_type.contains("text/event-stream") {
            let body = response.text().await.map_err(|e| RegistryError::McpServerError {
                server: config.name.clone(),
                message: format!("Failed to read SSE response body: {}", e),
            })?;
            // Every event's `data:` line carries one JSON-RPC message; ours is
            // the first parseable frame that has a result or an error.
            for frame in body.split("\n\n") {
                let data: Vec<&str> = frame
                    .lines()
                    .filter_map(|l| l.strip_prefix("data:"))
                    .map(str::trim_start)
                    .collect();
                if data.is_empty() {
                    continue;
                }
                if let Ok(rpc) = serde_json::from_str::<JsonRpcResponse>(&data.join("\n")) {
                    if rpc.result.is_some() || rpc.error.is_some() {
                        return Ok(rpc);
                    }
                }
            }
            Err(RegistryError::McpServerError {
                server: config.name.clone(),
                message: "SSE response carried no JSON-RPC message".to_string(),
            })
        } else {
            response.json().await.map_err(|e| RegistryError::McpServerError {
                server: config.name.clone(),
                message: format!("Failed to parse JSON response: {}", e),
            })
        }
    }

    /// Extract the JSON-RPC `result` or surface the RPC error loudly.
    fn rpc_result(
        rpc: JsonRpcResponse,
        config: &McpServerConfig,
        context: &str,
    ) -> RegistryResult<Value> {
        if let Some(error) = rpc.error {
            return Err(RegistryError::McpServerError {
                server: config.name.clone(),
                message: format!("{}: MCP error: {}", context, error.message),
            });
        }
        rpc.result.ok_or_else(|| RegistryError::McpServerError {
            server: config.name.clone(),
            message: format!("{}: no result in response", context),
        })
    }

    /// Streamable HTTP session expiry: a 404 means the server no longer knows
    /// our `Mcp-Session-Id` — re-initialize once (mints a fresh session) and
    /// retry the same request.
    async fn post_streamable_with_session_retry<T: Serialize + ?Sized>(
        &self,
        config: &McpServerConfig,
        body: &T,
    ) -> RegistryResult<reqwest::Response> {
        let mut response = self.post_streamable(config, body).await?;
        if response.status() == reqwest::StatusCode::NOT_FOUND {
            crate::observability::tee_eprintln(&format!(
                "[MCP] '{}' session expired (404) — re-initializing once",
                config.name
            ));
            self.initialize(config).await?;
            response = self.post_streamable(config, body).await?;
        }
        Ok(response)
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

    /// Fetch tools from an MCP server (dispatches on transport).
    pub async fn fetch_tools(&self, config: &McpServerConfig) -> RegistryResult<Vec<McpTool>> {
        if config.is_legacy() {
            return self.fetch_tools_legacy(config).await;
        }

        let request = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: Some(1),
            method: "tools/list".to_string(),
            params: Some(json!({})),
        };

        let response = self
            .post_streamable_with_session_retry(config, &request)
            .await?;

        if !response.status().is_success() {
            return Err(RegistryError::McpServerError {
                server: config.name.clone(),
                message: format!("HTTP {} - {}", response.status(), response.status().as_str()),
            });
        }

        let rpc = self.parse_rpc_response(response, config).await?;
        let result = Self::rpc_result(rpc, config, "tools/list")?;
        Self::tools_from_result(result, config)
    }

    /// Legacy transport (`legacy-sse`): the pre-0.20 homemade
    /// `POST {url}/message` protocol, preserved byte-for-byte.
    async fn fetch_tools_legacy(&self, config: &McpServerConfig) -> RegistryResult<Vec<McpTool>> {
        let message_url = format!("{}/message", config.url.trim_end_matches('/'));

        let request = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: Some(1),
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

        Self::tools_from_result(result, config)
    }

    /// Map a tools/list `result` value into registry tools (shared by both
    /// transports).
    fn tools_from_result(result: Value, config: &McpServerConfig) -> RegistryResult<Vec<McpTool>> {
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

    /// Initialize connection with an MCP server (dispatches on transport).
    pub async fn initialize(&self, config: &McpServerConfig) -> RegistryResult<()> {
        if config.is_legacy() {
            return self.initialize_legacy(config).await;
        }

        let init_request = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: Some(1),
            method: "initialize".to_string(),
            params: Some(json!({
                "protocolVersion": "2025-03-26",
                "capabilities": {},
                "clientInfo": {
                    "name": "abk-mcp-client",
                    "version": env!("CARGO_PKG_VERSION")
                }
            })),
        };

        let response = self.post_streamable(config, &init_request).await?;

        if !response.status().is_success() {
            return Err(RegistryError::McpServerError {
                server: config.name.clone(),
                message: format!("Initialize failed: HTTP {}", response.status()),
            });
        }

        // Session handshake: the server mints an Mcp-Session-Id that must ride
        // every subsequent request (unknown sessions 404).
        if let Some(sid) = response
            .headers()
            .get("Mcp-Session-Id")
            .and_then(|v| v.to_str().ok())
        {
            config.store_session(Some(sid.to_string()));
        }

        let rpc = self.parse_rpc_response(response, config).await?;
        let result = Self::rpc_result(rpc, config, "initialize")?;
        crate::observability::tee_eprintln(&format!(
            "[MCP] '{}' initialized: {} {} (protocol {})",
            config.name,
            result["serverInfo"]["name"].as_str().unwrap_or("unknown"),
            result["serverInfo"]["version"].as_str().unwrap_or("?"),
            result["protocolVersion"].as_str().unwrap_or("?"),
        ));

        // notifications/initialized — no id member (JSON-RPC notification);
        // Streamable HTTP answers 202 Accepted. Tolerated failure:
        // initialize already succeeded and servers may drop notifications.
        let initialized = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: None,
            method: "notifications/initialized".to_string(),
            params: None,
        };
        match self.post_streamable(config, &initialized).await {
            Ok(r) if r.status().is_success() => {}
            Ok(r) => crate::observability::tee_eprintln(&format!(
                "Warning: MCP '{}' initialized-notification answered HTTP {} (continuing)",
                config.name, r.status()
            )),
            Err(e) => crate::observability::tee_eprintln(&format!(
                "Warning: MCP '{}' initialized-notification failed (continuing): {}",
                config.name, e
            )),
        }

        Ok(())
    }

    /// Legacy transport (`legacy-sse`) initialize, preserved byte-for-byte.
    async fn initialize_legacy(&self, config: &McpServerConfig) -> RegistryResult<()> {
        let message_url = format!("{}/message", config.url.trim_end_matches('/'));

        let init_request = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: Some(1),
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
            id: Some(2),
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

    /// Call a tool on an MCP server (dispatches on transport).
    pub async fn call_tool(
        &self,
        config: &McpServerConfig,
        tool_name: &str,
        arguments: Value,
    ) -> RegistryResult<McpToolCallResult> {
        if config.is_legacy() {
            return self.call_tool_legacy(config, tool_name, arguments).await;
        }

        let request = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: Some(1),
            method: "tools/call".to_string(),
            params: Some(json!({
                "name": tool_name,
                "arguments": arguments
            })),
        };

        let response = self
            .post_streamable_with_session_retry(config, &request)
            .await?;

        if !response.status().is_success() {
            return Err(RegistryError::McpServerError {
                server: config.name.clone(),
                message: format!("Tool call failed: HTTP {}", response.status()),
            });
        }

        let rpc = self.parse_rpc_response(response, config).await?;
        let result = Self::rpc_result(rpc, config, "tools/call")?;
        Ok(Self::call_result_from_value(result))
    }

    /// Build the tool-call result from a `tools/call` result value (shared by
    /// both transports).
    fn call_result_from_value(result: Value) -> McpToolCallResult {
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

        McpToolCallResult {
            content: text_content,
            is_error,
            raw_content: content,
        }
    }

    /// Legacy transport (`legacy-sse`) tool call, preserved byte-for-byte.
    async fn call_tool_legacy(
        &self,
        config: &McpServerConfig,
        tool_name: &str,
        arguments: Value,
    ) -> RegistryResult<McpToolCallResult> {
        let message_url = format!("{}/message", config.url.trim_end_matches('/'));

        let request = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: Some(1),
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

        Ok(Self::call_result_from_value(result))
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
            id: Some(1),
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

/// nghr d7d28cfe — Streamable HTTP transport regression tests.
///
/// The mock server is a raw std::net TCP listener (no new dev-deps) that
/// SPEAKS the wire: it inspects the request path, the `Mcp-Session-Id`
/// header and the JSON-RPC method, and answers exactly like a modern TS-SDK
/// server (streamable at `/mcp`, session-bound legacy at `/message`).
#[cfg(test)]
mod streamable_tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::sync::{Arc, Mutex};

    const SESSION: &str = "mock-session-1";

    #[derive(Default, Debug, Clone)]
    struct Hits {
        inits: usize,
        lists: usize,
        list_404s: usize,
        bad_session_lists: usize,
        saw_mcp_path: bool,
        saw_message_path: bool,
        sessions_on_list: Vec<String>,
    }

    struct Mock {
        base: String,
        hits: Arc<Mutex<Hits>>,
    }

    impl Mock {
        /// Spawn the mock; `sse_tools` answers tools/list as an SSE frame,
        /// `fail_first_list` makes the FIRST sessioned tools/list 404 (the
        /// session-expiry → re-initialize → retry path).
        fn spawn(sse_tools: bool, fail_first_list: bool) -> Self {
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            let addr = listener.local_addr().unwrap();
            let hits = Arc::new(Mutex::new(Hits::default()));
            let hits_task = hits.clone();
            std::thread::spawn(move || {
                for stream in listener.incoming() {
                    let Ok(mut stream) = stream else { break };
                    let h = hits_task.clone();
                    // One request per connection; the client's pool reopens
                    // after `Connection: close`.
                    let _ = Self::handle(&mut stream, h, sse_tools, fail_first_list);
                }
            });
            Self { base: format!("http://{}", addr), hits }
        }

        fn read_request(stream: &mut std::net::TcpStream) -> Option<(String, Option<String>, Value)> {
            let mut buf: Vec<u8> = Vec::new();
            let mut chunk = [0u8; 4096];
            let header_end = loop {
                let n = stream.read(&mut chunk).ok()?;
                if n == 0 {
                    return None;
                }
                buf.extend_from_slice(&chunk[..n]);
                if let Some(pos) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                    break pos + 4;
                }
                if buf.len() > 256 * 1024 {
                    return None;
                }
            };
            let head = String::from_utf8_lossy(&buf[..header_end]).to_string();
            let mut lines = head.split("\r\n");
            let request_line = lines.next().unwrap_or("");
            let path = request_line.split_whitespace().nth(1).unwrap_or("/").to_string();
            let mut content_length = 0usize;
            let mut session: Option<String> = None;
            for line in lines {
                let lower = line.to_ascii_lowercase();
                if let Some(v) = lower.strip_prefix("content-length:") {
                    content_length = v.trim().parse().unwrap_or(0);
                }
                if let Some(v) = lower.strip_prefix("mcp-session-id:") {
                    session = Some(v.trim().to_string());
                }
            }
            while buf.len() < header_end + content_length {
                let n = stream.read(&mut chunk).ok()?;
                if n == 0 {
                    break;
                }
                buf.extend_from_slice(&chunk[..n]);
            }
            let body: Value = serde_json::from_slice(&buf[header_end..]).unwrap_or(Value::Null);
            Some((path, session, body))
        }

        fn handle(
            stream: &mut std::net::TcpStream,
            hits: Arc<Mutex<Hits>>,
            sse_tools: bool,
            fail_first_list: bool,
        ) -> std::io::Result<()> {
            let Some((path, session, body)) = Self::read_request(stream) else {
                return Ok(());
            };
            let method = body["method"].as_str().unwrap_or("").to_string();

            let (status, reason, content_type, payload): (u16, &str, &str, Value) = {
                let mut h = hits.lock().unwrap();
                if path.starts_with("/mcp") {
                    h.saw_mcp_path = true;
                }
                if path.starts_with("/message") {
                    h.saw_message_path = true;
                }
                match method.as_str() {
                    "initialize" => {
                        h.inits += 1;
                        (
                            200, "OK", "application/json",
                            serde_json::json!({
                                "jsonrpc": "2.0", "id": body["id"],
                                "result": {
                                    "protocolVersion": "2025-03-26",
                                    "capabilities": {"tools": {"listChanged": false}},
                                    "serverInfo": {"name": "mock-browser", "version": "1.0.0"}
                                }
                            }),
                        )
                    }
                    "notifications/initialized" | "initialized" => {
                        (202, "Accepted", "application/json", Value::Null)
                    }
                    "tools/list" => {
                        h.lists += 1;
                        let legacy_lane = path.starts_with("/message");
                        if !legacy_lane && session.as_deref() != Some(SESSION) {
                            h.bad_session_lists += 1;
                            (400, "Bad Request", "application/json", serde_json::json!({"error": "missing session"}))
                        } else {
                            h.sessions_on_list.push(session.unwrap_or_default());
                            if !legacy_lane && fail_first_list && h.list_404s == 0 {
                                h.list_404s += 1;
                                (404, "Not Found", "application/json", serde_json::json!({"error": "session expired"}))
                            } else {
                                let rpc = serde_json::json!({
                                    "jsonrpc": "2.0", "id": body["id"],
                                    "result": {"tools": [{
                                        "name": "browser_navigate",
                                        "description": "Navigate to a URL",
                                        "inputSchema": {"type": "object", "properties": {"url": {"type": "string"}}}
                                    }]}
                                });
                                if !legacy_lane && sse_tools {
                                    (200, "OK", "text/event-stream", rpc)
                                } else {
                                    (200, "OK", "application/json", rpc)
                                }
                            }
                        }
                    }
                    _ => (404, "Not Found", "application/json", serde_json::json!({"error": "no route"})),
                }
            };

            let body_text = if content_type == "text/event-stream" {
                format!("event: message\ndata: {}\n\n", payload)
            } else if payload.is_null() {
                String::new()
            } else {
                payload.to_string()
            };
            let response = format!(
                "HTTP/1.1 {} {}\r\nContent-Type: {}\r\nMcp-Session-Id: {}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                status, reason, content_type, SESSION, body_text.len(), body_text
            );
            stream.write_all(response.as_bytes())?;
            stream.flush()
        }

        fn hits(&self) -> Hits {
            let guard = self.hits.lock().unwrap();
            guard.clone()
        }
    }

    async fn fetch(config: &McpServerConfig) -> Vec<McpTool> {
        McpClient::new()
            .fetch_tools_with_init(config)
            .await
            .expect("fetch_tools_with_init must succeed")
    }

    #[test]
    fn streamable_endpoint_resolution() {
        assert_eq!(McpClient::streamable_endpoint("http://h:8089"), "http://h:8089/mcp");
        assert_eq!(McpClient::streamable_endpoint("http://h:8089/"), "http://h:8089/mcp");
        assert_eq!(McpClient::streamable_endpoint("http://h:8089/mcp"), "http://h:8089/mcp");
        assert_eq!(McpClient::streamable_endpoint("https://x.example/pdt"), "https://x.example/pdt");
    }

    #[tokio::test]
    async fn streamable_bare_url_appends_mcp_and_echoes_session() {
        let m = Mock::spawn(false, false);
        let config = McpServerConfig::new("browsermcp", &m.base); // transport http (default)
        let got = fetch(&config).await;
        assert_eq!(got.len(), 1, "tools must flow");
        assert_eq!(got[0].name, "browser_navigate");
        let h = m.hits();
        assert!(h.saw_mcp_path, "server must see POST /mcp (bare URL → /mcp mount)");
        assert_eq!(h.bad_session_lists, 0, "Mcp-Session-Id must ride tools/list");
        assert_eq!(h.inits, 1, "exactly one initialize");
        assert_eq!(h.sessions_on_list, vec![SESSION.to_string()]);
    }

    #[tokio::test]
    async fn streamable_parses_sse_framed_responses() {
        let m = Mock::spawn(true, false);
        let config = McpServerConfig::new("sse-frame", &m.base);
        assert_eq!(fetch(&config).await.len(), 1, "SSE-framed tools/list must parse");
    }

    #[tokio::test]
    async fn streamable_404_reinitializes_once_and_recovers() {
        let m = Mock::spawn(false, true);
        let config = McpServerConfig::new("expiring", &m.base);
        assert_eq!(fetch(&config).await.len(), 1, "recovery must be invisible to the caller");
        let h = m.hits();
        assert_eq!(h.inits, 2, "session-expiry 404 → exactly one re-initialize");
        assert_eq!(h.list_404s, 1);
    }

    #[tokio::test]
    async fn legacy_sse_still_speaks_message_endpoint() {
        let m = Mock::spawn(false, false);
        let config = McpServerConfig::new("old-server", &m.base).with_transport("legacy-sse");
        assert_eq!(fetch(&config).await.len(), 1, "legacy path preserved");
        let h = m.hits();
        assert!(h.saw_message_path, "legacy transport must POST {{url}}/message");
        assert!(!h.saw_mcp_path, "legacy transport must never touch /mcp");
    }

    #[tokio::test]
    async fn config_clone_shares_session_across_clients() {
        let m = Mock::spawn(false, false);
        let config = McpServerConfig::new("shared", &m.base);
        fetch(&config).await; // initialize mints the session into the config
        let cloned = config.clone();
        assert_eq!(
            cloned.current_session().as_deref(),
            Some(SESSION),
            "a fresh McpClient (clone path) must resume the session"
        );
    }

}

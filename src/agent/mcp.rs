//! MCP tool integration for the agent.
//!
//! This module provides functionality to load tools from MCP servers
//! and convert them to OpenAI function schemas for LLM consumption.

#[cfg(feature = "registry-mcp")]
use crate::config::{McpConfig, McpCredentialConfig};
#[cfg(feature = "registry-mcp")]
use crate::registry::{McpClient, McpServerConfig as RegistryServerConfig, ToolRegistry};
#[cfg(feature = "registry-mcp")]
use anyhow::Result;
#[cfg(feature = "registry-mcp")]
use std::collections::HashMap;

/// Status of a single MCP server connection after initialization.
#[cfg(feature = "registry-mcp")]
#[derive(Debug, Clone)]
pub struct McpServerInfo {
    /// Server name from config
    pub name: String,
    /// Whether the server connected successfully
    pub connected: bool,
    /// Number of tools loaded from this server
    pub tool_count: usize,
    /// Error message if the server failed to connect
    pub error: Option<String>,
}

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
    /// Per-server connection status collected during `new()`.
    /// Used by the agent to emit `OutputEvent::McpServerStatus` events
    /// through the output sink.
    pub server_statuses: Vec<McpServerInfo>,
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
        Self::with_token_store(config, None).await
    }

    /// Create a new MCP tool loader with an optional token store.
    ///
    /// When `token_store` is `Some`, interactive/web credential flows use
    /// it instead of creating a `FileTokenStore`. This enables per-user
    /// token isolation in multi-user deployments.
    ///
    /// # Arguments
    /// * `config` - The MCP configuration from the agent config
    /// * `token_store` - Optional shared token store for credential isolation
    #[cfg(feature = "registry-mcp-token")]
    pub async fn with_token_store(
        config: &McpConfig,
        token_store: Option<std::sync::Arc<dyn pep::token_store::TokenStore>>,
    ) -> Result<Self> {
        Self::inner_new(config, token_store).await
    }

    /// Internal constructor (no token store).
    #[cfg(not(feature = "registry-mcp-token"))]
    async fn inner_new(config: &McpConfig) -> Result<Self> {
        Self::create(config).await
    }

    /// Internal constructor (with optional token store).
    #[cfg(feature = "registry-mcp-token")]
    async fn inner_new(
        config: &McpConfig,
        token_store: Option<std::sync::Arc<dyn pep::token_store::TokenStore>>,
    ) -> Result<Self> {
        Self::create(config, token_store).await
    }

    /// Shared construction logic.
    #[cfg(not(feature = "registry-mcp-token"))]
    async fn create(config: &McpConfig) -> Result<Self> {
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
                server_statuses: Vec::new(),
            });
        }

        let mut server_statuses = Vec::new();

        for server in &config.servers {
            // nghr d7d28cfe: "http" = MCP Streamable HTTP (default, modern
            // servers); "legacy-sse" = the pre-0.20 homemade {url}/message
            // protocol. Anything else still fails loud.
            if server.transport != "http" && server.transport != "legacy-sse" {
                crate::observability::tee_eprintln(&format!(
                    "Warning: MCP server '{}' uses unsupported transport '{}', skipping",
                    server.name, server.transport
                ));
                continue;
            }

            let client_config = build_registry_config(
                &server.name,
                &server.url,
                &server.transport,
                server.auth_token.as_deref(),
                server.credentials.as_deref(),
                &config.credentials,
            )
            .await;

            server_configs.insert(server.name.clone(), client_config.clone());

            let result = if server.auto_init {
                client.fetch_tools_with_init(&client_config).await
            } else {
                client.fetch_tools(&client_config).await
            };

            match result {
                Ok(tools) => {
                    // 76cbecf0: namespaced names for the collision check —
                    // computed before the batch consumes the tools vec.
                    let namespaced: Vec<String> = tools
                        .iter()
                        .map(|t| crate::registry::mcp_tool_name(&server.name, &t.name))
                        .collect();
                    match registry.register_mcp_batch(tools, &server.name) {
                        Ok(registered) => {
                            // 76cbecf0: a tool-name collision at startup warns
                            // LOUD, naming the shadowed server — never silent
                            // shadowing. With namespacing this only fires when
                            // two configured servers share the same name.
                            if registered < namespaced.len() {
                                let shadowed: Vec<String> = namespaced
                                    .iter()
                                    .filter(|n| !registry.contains(n))
                                    .cloned()
                                    .collect();
                                crate::observability::tee_eprintln(&format!(
                                    "Warning: MCP tool name collision — {} tool(s) from server '{}' are SHADOWED (unreachable this session): {}. Another configured server already registered these namespaced names; give the servers distinct names.",
                                    shadowed.len(), server.name, shadowed.join(", ")
                                ));
                            }
                            total_tools += registered;
                            crate::observability::tee_println(&format!(
                                "✓ Loaded {} tools from MCP server '{}'",
                                registered, server.name
                            ));
                            server_statuses.push(McpServerInfo {
                                name: server.name.clone(),
                                connected: true,
                                tool_count: registered,
                                error: None,
                            });
                        }
                        Err(e) => {
                            let err_msg = format!("Failed to register tools: {}", e);
                            crate::observability::tee_eprintln(&format!(
                                "✗ MCP server '{}': {}",
                                server.name, err_msg
                            ));
                            server_statuses.push(McpServerInfo {
                                name: server.name.clone(),
                                connected: false,
                                tool_count: 0,
                                error: Some(err_msg),
                            });
                        }
                    }
                }
                Err(e) => {
                    crate::observability::tee_eprintln(&format!(
                        "✗ Failed to connect to MCP server '{}': {}",
                        server.name, e
                    ));
                    server_statuses.push(McpServerInfo {
                        name: server.name.clone(),
                        connected: false,
                        tool_count: 0,
                        error: Some(format!("{}", e)),
                    });
                }
            }
        }

        Ok(Self {
            registry,
            tool_count: total_tools,
            server_configs,
            client,
            server_statuses,
        })
    }

    /// Shared construction logic with optional token store.
    #[cfg(feature = "registry-mcp-token")]
    async fn create(
        config: &McpConfig,
        token_store: Option<std::sync::Arc<dyn pep::token_store::TokenStore>>,
    ) -> Result<Self> {
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
                server_statuses: Vec::new(),
            });
        }

        let mut server_statuses = Vec::new();

        for server in &config.servers {
            // nghr d7d28cfe: "http" = MCP Streamable HTTP (default, modern
            // servers); "legacy-sse" = the pre-0.20 homemade {url}/message
            // protocol. Anything else still fails loud.
            if server.transport != "http" && server.transport != "legacy-sse" {
                crate::observability::tee_eprintln(&format!(
                    "Warning: MCP server '{}' uses unsupported transport '{}', skipping",
                    server.name, server.transport
                ));
                continue;
            }

            // Build registry config from server config + credentials
            let client_config = build_registry_config(
                &server.name,
                &server.url,
                &server.transport,
                server.auth_token.as_deref(),
                server.credentials.as_deref(),
                &config.credentials,
                token_store.clone(),
            )
            .await;

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
                    // 76cbecf0: namespaced names for the collision check —
                    // computed before the batch consumes the tools vec.
                    let namespaced: Vec<String> = tools
                        .iter()
                        .map(|t| crate::registry::mcp_tool_name(&server.name, &t.name))
                        .collect();
                    match registry.register_mcp_batch(tools, &server.name) {
                        Ok(registered) => {
                            // 76cbecf0: a tool-name collision at startup warns
                            // LOUD, naming the shadowed server — never silent
                            // shadowing. With namespacing this only fires when
                            // two configured servers share the same name.
                            if registered < namespaced.len() {
                                let shadowed: Vec<String> = namespaced
                                    .iter()
                                    .filter(|n| !registry.contains(n))
                                    .cloned()
                                    .collect();
                                crate::observability::tee_eprintln(&format!(
                                    "Warning: MCP tool name collision — {} tool(s) from server '{}' are SHADOWED (unreachable this session): {}. Another configured server already registered these namespaced names; give the servers distinct names.",
                                    shadowed.len(), server.name, shadowed.join(", ")
                                ));
                            }
                            total_tools += registered;
                            crate::observability::tee_println(&format!(
                                "✓ Loaded {} tools from MCP server '{}'",
                                registered, server.name
                            ));
                            server_statuses.push(McpServerInfo {
                                name: server.name.clone(),
                                connected: true,
                                tool_count: registered,
                                error: None,
                            });
                        }
                        Err(e) => {
                            let err_msg = format!("Failed to register tools: {}", e);
                            crate::observability::tee_eprintln(&format!(
                                "Warning: Failed to register tools from '{}': {}",
                                server.name, e
                            ));
                            server_statuses.push(McpServerInfo {
                                name: server.name.clone(),
                                connected: false,
                                tool_count: 0,
                                error: Some(err_msg),
                            });
                        }
                    }
                }
                Err(e) => {
                    let err_msg = format!("{}", e);
                    crate::observability::tee_eprintln(&format!(
                        "Warning: Failed to fetch tools from MCP server '{}': {}",
                        server.name, e
                    ));
                    server_statuses.push(McpServerInfo {
                        name: server.name.clone(),
                        connected: false,
                        tool_count: 0,
                        error: Some(err_msg),
                    });
                }
            }
        }

        Ok(Self {
            registry,
            tool_count: total_tools,
            server_configs,
            client,
            server_statuses,
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
        self.registry
            .find(tool_name)
            .and_then(|t| t.origin().map(String::from))
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

        // 76cbecf0: the registry stores the namespaced form
        // (mcp__<server>__<tool>); the remote server expects the RAW name.
        let raw_tool_name =
            crate::registry::strip_mcp_server_prefix(&server_name, tool_name).unwrap_or(tool_name);

        // Parse arguments
        let args: serde_json::Value =
            serde_json::from_str(arguments).unwrap_or_else(|_| serde_json::json!({}));

        // Call the tool
        let result = self
            .client
            .call_tool(server_config, raw_tool_name, args)
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

// ---------------------------------------------------------------------------
// Credential resolution
// ---------------------------------------------------------------------------

/// Build a `RegistryServerConfig` from server config fields and the credentials registry.
///
/// Priority:
/// 1. `credentials` reference (if found in registry):
///    - `service-account`: attaches a lazily-exchanging
///      [`pep::token_provider::ServiceAccountTokenProvider`] — the RFC 8693
///      exchange happens on first use and the short-lived access token is
///      proactively refreshed (never a static `auth_token`); requires the
///      `registry-mcp-token` feature.
///    - `static`: resolves env vars and stores as `auth_token`.
/// 2. `auth_token` → static token
/// 3. neither → no auth
///
/// When `token_store` is `Some`, interactive/web/web-interactive credential
/// flows use it instead of creating a `FileTokenStore` from `ABK_AGENT_NAME`.
/// This enables per-user token isolation in multi-user deployments.
#[cfg(feature = "registry-mcp")]
async fn build_registry_config(
    name: &str,
    url: &str,
    transport: &str,
    auth_token: Option<&str>,
    credentials_ref: Option<&str>,
    credentials_map: &HashMap<String, McpCredentialConfig>,
    #[cfg(feature = "registry-mcp-token")] shared_store: Option<
        std::sync::Arc<dyn pep::token_store::TokenStore>,
    >,
) -> RegistryServerConfig {
    let mut config = RegistryServerConfig::new(name, url).with_transport(transport);

    // Check for named credential reference first
    if let Some(cred_name) = credentials_ref {
        if let Some(cred) = credentials_map.get(cred_name) {
            match cred {
                McpCredentialConfig::Static { token } => {
                    let resolved = resolve_env_var(token);
                    config = config.with_auth(resolved);
                }
                McpCredentialConfig::ServiceAccount {
                    service_token,
                    issuer_url,
                    client_id,
                    client_secret,
                    audience,
                    scope,
                } => {
                    let svc_token = resolve_env_var(service_token);
                    let issuer = resolve_env_var(issuer_url);
                    let cid = resolve_env_var(client_id);
                    let cs = client_secret.as_ref().map(|s| resolve_env_var(s));
                    let aud = resolve_env_var(audience);
                    let scp = scope.as_ref().map(|s| resolve_env_var(s));

                    #[cfg(feature = "registry-mcp-token")]
                    {
                        use pep::token_provider::{
                            ServiceAccountConfig, ServiceAccountTokenProvider, TokenProviderEnum,
                        };

                        // The shared registry: ONE provider (and ONE live
                        // proactive refresher) per credential identity per
                        // process — rebuild-cadence-safe (the 6121fc76 law).
                        // The warm cache also means a rebuild no longer
                        // forces a fresh RFC 8693 exchange on first use: the
                        // renewal rides the refresh_token grant (the SAME
                        // OAuth2 session) instead of churning a new session
                        // per rebuild (the fb82b866 revocation trigger).
                        let key = service_account_provider_key(
                            &svc_token,
                            &issuer,
                            &cid,
                            cs.as_deref(),
                            &aud,
                            scp.as_deref(),
                        );
                        let (provider, evicted) = shared_service_account_provider(key, || {
                            ServiceAccountTokenProvider::new(ServiceAccountConfig {
                                service_token: svc_token.clone(),
                                issuer_url: issuer.clone(),
                                client_id: cid.clone(),
                                client_secret: cs.clone(),
                                audience: aud.clone(),
                                scope: scp.clone(),
                            })
                        });
                        // Retire the evicted identity's refresher OUTSIDE the
                        // registry lock (rotation hygiene — a dead credential
                        // must not keep a 5s-retry mint loop against the IdP).
                        if let Some(old) = evicted {
                            old.stop_proactive_refresh().await;
                            crate::observability::tee_eprintln(
                                "[MCP] Evicted a rotated service-account credential's provider; its proactive refresher stopped",
                            );
                        }

                        crate::observability::tee_eprintln(
                            &format!("[MCP] Service account token provider attached for '{}' (audience={}). Token is exchanged lazily with the shared proactive refresher (one per credential identity).", name, &config.name)
                        );

                        config =
                            config.with_token_provider(TokenProviderEnum::ServiceAccount(provider));
                    }
                    #[cfg(not(feature = "registry-mcp-token"))]
                    {
                        let _ = (svc_token, issuer, cid, cs, aud, scp);
                        crate::observability::tee_eprintln(
                            &format!("Warning: Server '{}' uses service-account credentials but registry-mcp-token feature is disabled. Compile with --features registry-mcp-token", name)
                        );
                    }
                }
                McpCredentialConfig::Interactive {
                    issuer_url,
                    client_id,
                    client_secret,
                    scope,
                    redirect_port,
                } => {
                    #[cfg(feature = "registry-mcp-token")]
                    {
                        use pep::token_provider::{
                            InteractiveConfig, InteractiveTokenProvider, TokenProviderEnum,
                        };
                        use pep::token_store::{FileTokenStore, TokenStore};
                        use std::sync::Arc;

                        let token_store: Arc<dyn TokenStore> =
                            shared_store.clone().unwrap_or_else(|| {
                                let agent_name = std::env::var("ABK_AGENT_NAME")
                                    .unwrap_or_else(|_| "trustee".into());
                                Arc::new(FileTokenStore::new(&agent_name))
                            });

                        let provider = InteractiveTokenProvider::with_store(
                            InteractiveConfig {
                                issuer_url: resolve_env_var(issuer_url),
                                client_id: resolve_env_var(client_id),
                                client_secret: client_secret.as_ref().map(|s| resolve_env_var(s)),
                                redirect_uri: format!(
                                    "http://localhost:{}/callback",
                                    redirect_port
                                ),
                                scope: resolve_env_var(scope),
                                credential_name: cred_name.to_string(),
                            },
                            token_store,
                        );

                        crate::observability::tee_eprintln(&format!(
                            "[MCP] Interactive token provider attached for '{}' (credential={}).",
                            name, cred_name
                        ));

                        config =
                            config.with_token_provider(TokenProviderEnum::Interactive(provider));
                    }
                    #[cfg(not(feature = "registry-mcp-token"))]
                    {
                        let _ = (issuer_url, client_id, client_secret, scope, redirect_port);
                        crate::observability::tee_eprintln(
                            &format!("Warning: Server '{}' uses interactive credentials but registry-mcp-token feature is disabled.", name)
                        );
                    }
                }
                McpCredentialConfig::WebSession { .. } => {
                    #[cfg(feature = "registry-mcp-token")]
                    {
                        use pep::token_provider::{
                            InteractiveConfig, InteractiveTokenProvider, TokenProviderEnum,
                        };
                        use pep::token_store::{FileTokenStore, TokenStore};
                        use std::sync::Arc;

                        let token_store: Arc<dyn TokenStore> =
                            shared_store.clone().unwrap_or_else(|| {
                                let agent_name = std::env::var("ABK_AGENT_NAME")
                                    .unwrap_or_else(|_| "trustee".into());
                                Arc::new(FileTokenStore::new(&agent_name))
                            });

                        // Reserved credential name — trustee-web writes the session
                        // token here before each agent command.
                        const SESSION_CRED_NAME: &str = "__web_session";

                        let provider = InteractiveTokenProvider::with_store(
                            InteractiveConfig {
                                issuer_url: String::new(),
                                client_id: String::new(),
                                client_secret: None,
                                redirect_uri: String::new(),
                                scope: String::new(),
                                credential_name: SESSION_CRED_NAME.to_string(),
                            },
                            token_store,
                        );

                        crate::observability::tee_eprintln(
                            &format!("[MCP] Web session token provider attached for '{}' (reads __web_session from token store).", name)
                        );

                        config =
                            config.with_token_provider(TokenProviderEnum::Interactive(provider));
                    }
                    #[cfg(not(feature = "registry-mcp-token"))]
                    {
                        crate::observability::tee_eprintln(
                            &format!("Warning: Server '{}' uses web-session credentials but registry-mcp-token feature is disabled.", name)
                        );
                    }
                }
                McpCredentialConfig::WebInteractive {
                    issuer_url,
                    client_id,
                    client_secret,
                    scope,
                } => {
                    #[cfg(feature = "registry-mcp-token")]
                    {
                        use pep::token_provider::{
                            InteractiveConfig, InteractiveTokenProvider, TokenProviderEnum,
                        };
                        use pep::token_store::{FileTokenStore, TokenStore};
                        use std::sync::Arc;

                        let token_store: Arc<dyn TokenStore> =
                            shared_store.clone().unwrap_or_else(|| {
                                let agent_name = std::env::var("ABK_AGENT_NAME")
                                    .unwrap_or_else(|_| "trustee".into());
                                Arc::new(FileTokenStore::new(&agent_name))
                            });

                        let provider = InteractiveTokenProvider::with_store(
                            InteractiveConfig {
                                issuer_url: resolve_env_var(issuer_url),
                                client_id: resolve_env_var(client_id),
                                client_secret: client_secret.as_ref().map(|s| resolve_env_var(s)),
                                redirect_uri: String::new(),
                                scope: resolve_env_var(scope),
                                credential_name: cred_name.to_string(),
                            },
                            token_store,
                        );

                        crate::observability::tee_eprintln(
                            &format!("[MCP] Web interactive token provider attached for '{}' (credential={}). Login via trustee-web UI.", name, cred_name)
                        );

                        config =
                            config.with_token_provider(TokenProviderEnum::Interactive(provider));
                    }
                    #[cfg(not(feature = "registry-mcp-token"))]
                    {
                        let _ = (issuer_url, client_id, client_secret, scope);
                        crate::observability::tee_eprintln(
                            &format!("Warning: Server '{}' uses web-interactive credentials but registry-mcp-token feature is disabled.", name)
                        );
                    }
                }
            }
            return config;
        } else {
            crate::observability::tee_eprintln(
                &format!("Warning: Server '{}' references credential '{}' but it was not found in [mcp.credentials]", name, cred_name)
            );
        }
    }

    // Fall back to static auth_token
    if let Some(token) = auth_token {
        let resolved = resolve_env_var(token);
        config = config.with_auth(resolved);
    }

    config
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

// ---------------------------------------------------------------------------
// Shared service-account provider registry (the trustee TokenManager leg 1)
// ---------------------------------------------------------------------------
//
// WHY THIS EXISTS (ruling c903191f s3, fix (2) promoted; paydar delegation
// 2026-11-25 23:47Z): the trustee's MCP credential layer is a REBUILD-CADENCE
// caller — `McpToolLoader::new` runs on every loader rebuild (the trustee
// rebuilds per user on `MCP_TOOLSET_MAX_AGE`, ~600s). Before this registry,
// each rebuild constructed a FRESH `ServiceAccountTokenProvider` with an
// EMPTY cache and NEVER spawned the proactive refresher (the
// `registry/factory.rs` one-shot path spawns it; this path never did — the
// "automatic refresh" comment below was a lying comment). Two defects, one
// root:
//
// 1. NO proactive refresh on the trustee's MCP path (delegation leg 1:
//    "re-mint at TTL minus a margin (>=120 s) while the session is live" —
//    the refresher is exactly that leg; pep's effective margin is
//    BUFFER 120s + horizon 90-105s ≈ T-210s real, satisfying >=120s).
// 2. PER-REBUILD SESSION CHURN: a fresh provider's first `get_token()` runs
//    a fresh RFC 8693 exchange — a NEW OAuth2 session every rebuild per
//    user per server (~6/hour/user/server). That churn is the fb82b866
//    mid-TTL revocation trigger itself (pep 0.5.17: "a fresh RFC 8693
//    exchange minted a NEW OAuth2 session on every renewal — dozens of
//    sessions per account per day, fleet-wide — which is the likely trigger
//    of the mid-TTL revocations"). A warm, shared cache renews via the
//    `refresh_token` grant — the SAME session, no churn.
//
// Leg 2 (remint-on-401, "re-mint once and retry the same call in-line") is
// `registry/client.rs::send_with_retry` — already wired on every HTTP MCP
// path (invalidate → fresh exchange → retry exactly once).
//
// THE LIFECYCLE LAW (the 6121fc76 trap): a rebuild-cadence caller must NOT
// `spawn_proactive_refresh()` per build — every spawned task holds a clone
// of its provider (Arc internals) and lives forever, so per-build spawning
// accumulates immortal refresher tasks (6/hour/user/server) AND immortal
// IdP mint loops. The registry makes the provider a PROCESS-WIDE SINGLETON
// per credential identity: the first build spawns the ONE refresher; every
// later rebuild clones the same instance (clones share the cache and the
// refresher slot — pep 0.5.17 handle-based lifecycle). The map is bounded
// (`REGISTRY_CAP`): the OLDEST-inserted identity is evicted and its
// refresher explicitly stopped (awaited OUTSIDE the sync lock), so a
// credential rotation eventually retires the dead credential's task instead
// of leaking a 5s-retry mint loop against the IdP forever.
//
// KEY DISCIPLINE: the key is sha256 over the RESOLVED credential identity
// (service_token + issuer + client_id + client_secret + audience + scope)
// — never the raw values (the secrets-never-in-code law; pep's
// `token_fingerprint` precedent). Per-user credentials resolve to distinct
// tokens before this layer (the trustee's `resolve_user_config` substitutes
// ${VAR} per user and fails loud on leftovers), so the key is naturally
// per-user without carrying user identifiers.

/// Registry bound: distinct credential identities this process will hold.
/// Live fleet shape: ~2 credentials per live user × live users + rotations —
/// tens, not thousands. Eviction is rotation-hygiene, not steady-state.
#[cfg(feature = "registry-mcp-token")]
const SERVICE_ACCOUNT_REGISTRY_CAP: usize = 64;

#[cfg(feature = "registry-mcp-token")]
static SERVICE_ACCOUNT_PROVIDERS: std::sync::OnceLock<
    std::sync::Mutex<(
        std::collections::HashMap<[u8; 32], pep::token_provider::ServiceAccountTokenProvider>,
        std::collections::VecDeque<[u8; 32]>,
    )>,
> = std::sync::OnceLock::new();

/// Fingerprint of a resolved service-account credential identity. Order-fixed
/// with 0x00 separators (no ambiguous concatenation); raw values never leave
/// this function.
#[cfg(feature = "registry-mcp-token")]
fn service_account_provider_key(
    service_token: &str,
    issuer_url: &str,
    client_id: &str,
    client_secret: Option<&str>,
    audience: &str,
    scope: Option<&str>,
) -> [u8; 32] {
    use sha2::{Digest, Sha256};
    let mut h = Sha256::new();
    for part in [
        service_token,
        issuer_url,
        client_id,
        client_secret.unwrap_or(""),
        audience,
        scope.unwrap_or(""),
    ] {
        h.update(part.as_bytes());
        h.update([0x00]);
    }
    h.finalize().into()
}

/// Get-or-create the process-wide provider for `key`. The ctor runs at most
/// once per identity (single sync critical section — no await under the
/// lock, so no double-spawn race); the refresher spawns exactly once per
/// identity, on the miss path. Returns the provider (a clone sharing the
/// cache + refresher slot) and, when the insert evicted an older identity,
/// that evicted provider so the CALLER can `stop_proactive_refresh().await`
/// it outside the lock.
#[cfg(feature = "registry-mcp-token")]
fn shared_service_account_provider(
    key: [u8; 32],
    ctor: impl Fn() -> pep::token_provider::ServiceAccountTokenProvider,
) -> (
    pep::token_provider::ServiceAccountTokenProvider,
    Option<pep::token_provider::ServiceAccountTokenProvider>,
) {
    let lock = SERVICE_ACCOUNT_PROVIDERS.get_or_init(|| {
        std::sync::Mutex::new((
            std::collections::HashMap::new(),
            std::collections::VecDeque::new(),
        ))
    });
    let mut reg = lock
        .lock()
        .expect("service-account provider registry poisoned");
    let (map, order) = &mut *reg;
    if let Some(existing) = map.get(&key) {
        return (existing.clone(), None);
    }
    let provider = ctor();
    // ONE immortal task per credential identity, spawned HERE only — the
    // rebuild-cadence-safe spawn point (the factory.rs one-shot comment
    // generalizes: rebuild callers must not spawn per build).
    provider.spawn_proactive_refresh();
    let mut evicted = None;
    if map.len() >= SERVICE_ACCOUNT_REGISTRY_CAP {
        if let Some(oldest) = order.pop_front() {
            if let Some(old) = map.remove(&oldest) {
                evicted = Some(old);
            }
        }
    }
    order.push_back(key);
    map.insert(key, provider.clone());
    (provider, evicted)
}

/// Test probe: how many distinct credential identities are registered.
#[cfg(all(test, feature = "registry-mcp-token"))]
fn service_account_provider_registry_len() -> usize {
    SERVICE_ACCOUNT_PROVIDERS
        .get()
        .map(|l| l.lock().expect("registry poisoned").0.len())
        .unwrap_or(0)
}

#[cfg(all(test, feature = "registry-mcp"))]
mod tests {
    use super::*;

    #[test]
    fn test_resolve_env_var() {
        std::env::set_var("TEST_MCP_VAR", "test_value");

        assert_eq!(resolve_env_var("${TEST_MCP_VAR}"), "test_value");
        assert_eq!(resolve_env_var("plain_value"), "plain_value");
        assert_eq!(resolve_env_var("${NONEXISTENT_VAR}"), "${NONEXISTENT_VAR}");

        std::env::remove_var("TEST_MCP_VAR");
    }

    #[tokio::test]
    async fn test_mcp_loader_disabled() {
        use crate::config::McpConfig;
        let config = McpConfig {
            enabled: false,
            timeout_seconds: 30,
            credentials: HashMap::new(),
            servers: vec![],
        };

        let loader = McpToolLoader::new(&config).await.unwrap();
        assert_eq!(loader.tool_count, 0);
        assert!(!loader.has_tools());
    }

    // -------------------------------------------------------------------
    // Shared service-account provider registry (ruling c903191f s3 leg 1)
    // -------------------------------------------------------------------
    #[cfg(feature = "registry-mcp-token")]
    mod token_registry {
        use super::*;

        /// Serializes the registry tests against EACH OTHER: the registry is
        /// process-wide and cargo runs tests in parallel threads — the exact
        /// count asserts below are only deterministic under a test-local
        /// mutex. (Other abk tests never touch the registry: the factory
        /// path wires providers without it.)
        static REGISTRY_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

        #[test]
        fn provider_key_is_stable_per_identity_and_distinct_across_credentials() {
            let a = service_account_provider_key(
                "tok",
                "https://idp/",
                "pdt-api",
                None,
                "pdt-api",
                Some("openid"),
            );
            let a2 = service_account_provider_key(
                "tok",
                "https://idp/",
                "pdt-api",
                None,
                "pdt-api",
                Some("openid"),
            );
            assert_eq!(a, a2, "same identity → same key");

            // Every identity field participates: change one → different key.
            assert_ne!(
                a,
                service_account_provider_key(
                    "tok2",
                    "https://idp/",
                    "pdt-api",
                    None,
                    "pdt-api",
                    Some("openid")
                ),
                "service_token participates"
            );
            assert_ne!(
                a,
                service_account_provider_key(
                    "tok",
                    "https://idp2/",
                    "pdt-api",
                    None,
                    "pdt-api",
                    Some("openid")
                ),
                "issuer participates"
            );
            assert_ne!(
                a,
                service_account_provider_key(
                    "tok",
                    "https://idp/",
                    "other",
                    None,
                    "pdt-api",
                    Some("openid")
                ),
                "client_id participates"
            );
            assert_ne!(
                a,
                service_account_provider_key(
                    "tok",
                    "https://idp/",
                    "pdt-api",
                    Some("sec"),
                    "pdt-api",
                    Some("openid")
                ),
                "client_secret participates"
            );
            assert_ne!(
                a,
                service_account_provider_key(
                    "tok",
                    "https://idp/",
                    "pdt-api",
                    None,
                    "other-aud",
                    Some("openid")
                ),
                "audience participates"
            );
            assert_ne!(
                a,
                service_account_provider_key(
                    "tok",
                    "https://idp/",
                    "pdt-api",
                    None,
                    "pdt-api",
                    Some("openid groups")
                ),
                "scope participates"
            );

            // Separator discipline: ("ab","c") must never equal ("a","bc").
            assert_ne!(
                service_account_provider_key("ab", "c", "", None, "", None),
                service_account_provider_key("a", "bc", "", None, "", None),
                "0x00 separators prevent concatenation collisions"
            );
        }

        /// The rebuild-cadence law: the same credential identity constructs the
        /// provider EXACTLY ONCE per process — every later build clones the same
        /// instance (shared cache + refresher slot), never a second refresher.
        #[tokio::test]
        async fn shared_provider_constructs_once_per_identity() {
            let _serial = REGISTRY_TEST_LOCK.lock().expect("test lock poisoned");
            let key = service_account_provider_key(
                "shared-once-test-token",
                "https://shared-once.invalid/",
                "pdt-api",
                None,
                "pdt-api",
                Some("openid"),
            );
            let ctor_count = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
            let mk = || {
                let c = ctor_count.clone();
                move || {
                    c.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                    let cfg = pep::token_provider::ServiceAccountConfig {
                        service_token: "shared-once-test-token".into(),
                        issuer_url: "https://shared-once.invalid/".into(),
                        client_id: "pdt-api".into(),
                        client_secret: None,
                        audience: "pdt-api".into(),
                        scope: Some("openid".into()),
                    };
                    pep::token_provider::ServiceAccountTokenProvider::new(cfg)
                }
            };
            let counter = mk();
            let (p1, ev1) = shared_service_account_provider(key, &counter);
            let (p2, ev2) = shared_service_account_provider(key, &counter);
            let (p3, ev3) = shared_service_account_provider(key, &counter);
            assert_eq!(
                ctor_count.load(std::sync::atomic::Ordering::SeqCst),
                1,
                "ctor runs exactly once per identity"
            );
            // The FIRST call may evict when the registry is already at cap
            // (sibling tests fill it — the designed bound); a HIT path never
            // evicts (no insert, no eviction).
            if let Some(old) = &ev1 {
                old.stop_proactive_refresh().await;
            }
            assert!(ev2.is_none() && ev3.is_none(), "a HIT path never evicts");
            // Clones share the provider identity (Arc internals): Debug shows the
            // same audience, and the refresher slot is shared — spawning through
            // p2 would abort p1's task (pep handle-based lifecycle), which is the
            // designed one-refresher-per-identity invariant.
            let _ = (&p1, &p2, &p3);
        }

        /// Registry bound: inserting beyond the cap evicts the OLDEST identity
        /// (its refresher stopped by the caller) and stays bounded; a re-request
        /// of an evicted identity constructs afresh.
        #[tokio::test]
        async fn registry_cap_evicts_oldest_and_stays_bounded() {
            let _serial = REGISTRY_TEST_LOCK.lock().expect("test lock poisoned");
            // Unique prefix so this test never collides with another test's
            // identities in the process-wide registry.
            let tag = format!("cap-test-{}", std::process::id());
            let mk_key = |i: usize| {
                service_account_provider_key(
                    &format!("{tag}-tok-{i}"),
                    "https://cap.invalid/",
                    "pdt-api",
                    None,
                    "pdt-api",
                    Some("openid"),
                )
            };
            let mk_ctor = |tok: String| {
                move || {
                    let cfg = pep::token_provider::ServiceAccountConfig {
                        service_token: tok.clone(),
                        issuer_url: "https://cap.invalid/".into(),
                        client_id: "pdt-api".into(),
                        client_secret: None,
                        audience: "pdt-api".into(),
                        scope: Some("openid".into()),
                    };
                    pep::token_provider::ServiceAccountTokenProvider::new(cfg)
                }
            };

            let before = service_account_provider_registry_len();
            // Fill past the cap with fresh identities.
            let mut last_evicted = None;
            for i in 0..=(SERVICE_ACCOUNT_REGISTRY_CAP - before.min(SERVICE_ACCOUNT_REGISTRY_CAP)) {
                let key = mk_key(i);
                let (_p, ev) =
                    shared_service_account_provider(key, mk_ctor(format!("{tag}-tok-{i}")));
                if let Some(old) = ev {
                    old.stop_proactive_refresh().await;
                    last_evicted = Some(i);
                }
            }
            let after_fills = service_account_provider_registry_len();
            assert!(
                after_fills <= SERVICE_ACCOUNT_REGISTRY_CAP,
                "registry stays bounded (len={after_fills}, cap={SERVICE_ACCOUNT_REGISTRY_CAP})"
            );
            // The oldest identity (i=0) was evicted at some point...
            assert!(
                last_evicted.is_some(),
                "at least one eviction happened when filling past the cap"
            );
            // ...and re-requesting it constructs afresh (it is no longer
            // held). At cap the re-insert may itself evict the current
            // oldest — that is the designed bound, not a failure; the
            // invariant under test is boundedness, not eviction absence.
            let (p_again, ev_again) =
                shared_service_account_provider(mk_key(0), mk_ctor(format!("{tag}-tok-0")));
            if let Some(old) = ev_again {
                old.stop_proactive_refresh().await;
            }
            drop(p_again);
            let final_len = service_account_provider_registry_len();
            assert!(
                final_len <= SERVICE_ACCOUNT_REGISTRY_CAP,
                "bounded after re-insert too"
            );
        }

        /// The wiring proof: a service-account credential in the loader config
        /// attaches a token provider AND registers the shared identity (the
        /// proactive refresher is live — delegation leg 1 on the trustee path).
        #[tokio::test]
        async fn build_registry_config_wires_shared_provider_for_service_account() {
            let _serial = REGISTRY_TEST_LOCK.lock().expect("test lock poisoned");
            use crate::config::McpCredentialConfig;
            let tag = format!("wire-test-{}", std::process::id());
            let mut credentials = HashMap::new();
            credentials.insert(
                "svc".to_string(),
                McpCredentialConfig::ServiceAccount {
                    service_token: format!("{tag}-pat"),
                    issuer_url: "https://wire.invalid/oauth2/openid/pdt-api".to_string(),
                    client_id: "pdt-api".to_string(),
                    client_secret: None,
                    audience: "pdt-api".to_string(),
                    scope: Some("openid groups profile".to_string()),
                },
            );
            let before = service_account_provider_registry_len();
            let config = build_registry_config(
                "wire-svc",
                "https://wire.invalid/mcp",
                "http",
                None,
                Some("svc"),
                &credentials,
                None,
            )
            .await;
            assert!(
                config.token_provider.is_some(),
                "service-account credential attaches a token provider"
            );
            // Containment proof (cap-independent — at cap the insert evicts,
            // so a count assert would race the bound): the identity built by
            // the loader IS the registered one — a direct registry request
            // with the same key HITS (the ctor must never run).
            let key = service_account_provider_key(
                &format!("{tag}-pat"),
                "https://wire.invalid/oauth2/openid/pdt-api",
                "pdt-api",
                None,
                "pdt-api",
                Some("openid groups profile"),
            );
            let (shared, ev) = shared_service_account_provider(key, || {
                panic!("identity must already be registered — the ctor must not run")
            });
            if let Some(old) = ev {
                old.stop_proactive_refresh().await;
            }
            drop(shared);
            // A second build of the SAME credential reuses the identity —
            // the registry does not grow (the rebuild-cadence law).
            let config2 = build_registry_config(
                "wire-svc",
                "https://wire.invalid/mcp",
                "http",
                None,
                Some("svc"),
                &credentials,
                None,
            )
            .await;
            assert!(config2.token_provider.is_some());
            let after = service_account_provider_registry_len();
            assert!(
                after <= before + 1,
                "second build reuses the shared provider — the registry never grows per rebuild \
                 (len {before} → {after})"
            );
        }
    }
}

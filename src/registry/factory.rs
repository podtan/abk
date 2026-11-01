//! Factory for creating UnifiedRegistry from configuration.
//!
//! This module provides a factory function that creates a `UnifiedRegistry`
//! from the configuration's `[[tool_sources]]` section.

use std::collections::HashMap;
use std::sync::Arc;

use crate::config::{McpCredentialConfig, ToolSourceConfig};
use super::{BoxedToolSource, UnifiedRegistry, NativeToolSource, McpToolSource, McpServerConfig};

/// Build a UnifiedRegistry from configuration.
///
/// This function reads the `[[tool_sources]]` configuration and creates
/// the appropriate tool source providers, then aggregates them into
/// a unified registry.
///
/// # Arguments
/// * `configs` - List of tool source configurations
/// * `credentials` - Named credential definitions (`[mcp.credentials.*]`) —
///   the dynamic token path. A server's `credentials` reference resolves
///   against this map; a `service-account` credential attaches a
///   [`pep::token_provider::ServiceAccountTokenProvider`] (RFC 8693
///   exchange, inline refresh at the T-90s horizon, single-flight mint)
///   and spawns the proactive refresher ONCE at this one-shot build.
/// * `open_window_size` - Window size for native file reading tools
///
/// # Returns
/// A UnifiedRegistry with all configured tool sources, or an error.
///
/// # Example
///
/// ```rust,ignore
/// use abk::registry::build_registry_from_config;
/// use abk::config::Configuration;
///
/// let config = Configuration::default();
/// let registry = build_registry_from_config(&config.tool_sources, &config.mcp.credentials, 2000).await?;
///
/// // Get all tools
/// let tools = registry.all_schemas();
/// ```
pub async fn build_registry_from_config(
    configs: &[ToolSourceConfig],
    credentials: &HashMap<String, McpCredentialConfig>,
    open_window_size: usize,
) -> anyhow::Result<UnifiedRegistry> {
    let mut registry = UnifiedRegistry::new();

    for config in configs {
        let source = build_source_from_config(config, credentials, open_window_size).await?;
        registry.add_source(source);
    }

    // If no sources configured, add default native source
    if registry.source_count() == 0 {
        let default_source = Arc::new(NativeToolSource::new("opencode", open_window_size));
        registry.add_source(default_source);
    }

    Ok(registry)
}

/// Build a single tool source from configuration.
///
/// Credential priority (the agent/mcp.rs `build_registry_config` model):
/// 1. `credentials` reference resolved against the map:
///    - `static` → env-resolved static token (`with_auth`)
///    - `service-account` → [`pep::token_provider::ServiceAccountTokenProvider`]
///      (`with_token_provider`) + the proactive refresher spawned ONCE —
///      the dynamic token path (issue 6121fc76: the Mcp arm previously
///      wired ONLY the static `auth_token`, so the entire dynamic path
///      was dead code here)
/// 2. `auth_token` → static token
/// 3. neither → no auth
async fn build_source_from_config(
    config: &ToolSourceConfig,
    credentials: &HashMap<String, McpCredentialConfig>,
    open_window_size: usize,
) -> anyhow::Result<BoxedToolSource> {
    match config {
        ToolSourceConfig::Native { toolset } => {
            let source = NativeToolSource::new(toolset, open_window_size);
            Ok(Arc::new(source))
        }
        ToolSourceConfig::Mcp { name, url, auth_token, credentials: credentials_ref, auto_init } => {
            let server_config = wire_server_auth(
                name,
                url,
                auth_token.as_deref(),
                credentials_ref.as_deref(),
                credentials,
            );
            let source = McpToolSource::new(server_config, *auto_init).await?;
            Ok(Arc::new(source))
        }
    }
}

/// Wire a server's auth from the credentials map + the static fallback.
///
/// Credential priority (the agent/mcp.rs `build_registry_config` model):
/// 1. `credentials` reference resolved against the map:
///    - `static` → env-resolved static token (`with_auth`)
///    - `service-account` → [`pep::token_provider::ServiceAccountTokenProvider`]
///      (`with_token_provider`) + the proactive refresher spawned ONCE —
///      the dynamic token path (issue 6121fc76: the Mcp arm previously
///      wired ONLY the static `auth_token`, so the entire dynamic path
///      was dead code here)
/// 2. `auth_token` → static token
/// 3. neither → no auth
///
/// Extracted from the Mcp arm so the WIRING is testable without
/// `McpToolSource::new` (whose blocking-socket design cannot run inside
/// an async test — the same reason the factory itself has no async
/// production caller).
fn wire_server_auth(
    name: &str,
    url: &str,
    auth_token: Option<&str>,
    credentials_ref: Option<&str>,
    credentials: &HashMap<String, McpCredentialConfig>,
) -> McpServerConfig {
    let mut server_config = McpServerConfig::new(name, url);
    let mut auth_wired = false;

    // Named credential reference first (the dynamic token path).
    if let Some(cred_name) = credentials_ref {
        if let Some(cred) = credentials.get(cred_name) {
            match cred {
                McpCredentialConfig::Static { token } => {
                    let resolved = resolve_env_var(token);
                    server_config = server_config.with_auth(resolved);
                    auth_wired = true;
                }
                McpCredentialConfig::ServiceAccount {
                    service_token,
                    issuer_url,
                    client_id,
                    client_secret,
                    audience,
                    scope,
                } => {
                    #[cfg(feature = "registry-mcp-token")]
                    {
                        use pep::token_provider::{
                            ServiceAccountConfig, ServiceAccountTokenProvider,
                            TokenProviderEnum,
                        };
                        let provider = ServiceAccountTokenProvider::new(ServiceAccountConfig {
                            service_token: resolve_env_var(service_token),
                            issuer_url: resolve_env_var(issuer_url),
                            client_id: resolve_env_var(client_id),
                            client_secret: client_secret.as_ref().map(|s| resolve_env_var(s)),
                            audience: resolve_env_var(audience),
                            scope: scope.as_ref().map(|s| resolve_env_var(s)),
                        });
                        // The proactive refresher: ONE immortal task per
                        // provider — safe at this ONE-SHOT build (the
                        // factory runs once at startup; a rebuild-cadence
                        // caller must NOT spawn per build — immortal-task
                        // accumulation).
                        provider.spawn_proactive_refresh();
                        crate::observability::tee_eprintln(&format!(
                            "[MCP] Service account token provider attached for '{}' (audience={}). Token will be exchanged lazily with automatic refresh.",
                            name, name
                        ));
                        server_config = server_config
                            .with_token_provider(TokenProviderEnum::ServiceAccount(provider));
                        auth_wired = true;
                    }
                    #[cfg(not(feature = "registry-mcp-token"))]
                    {
                        let _ = (service_token, issuer_url, client_id, client_secret, audience, scope);
                        crate::observability::tee_eprintln(&format!(
                            "Warning: Server '{}' uses service-account credentials but registry-mcp-token feature is disabled. Compile with --features registry-mcp-token",
                            name
                        ));
                    }
                }
                // Interactive/web flows need a token store + login flow —
                // the factory's one-shot build has neither; fall through
                // to the auth_token path (the agent/mcp.rs loader owns
                // those flows).
                _ => {}
            }
        }
    }

    // Static fallback: the auth_token path (unchanged).
    if !auth_wired {
        if let Some(token) = auth_token {
            let resolved = resolve_env_var(token);
            server_config = server_config.with_auth(resolved);
        }
    }

    server_config
}

/// Resolve environment variable references in a string.
///
/// Supports patterns like `${VAR_NAME}` and replaces them with
/// the corresponding environment variable value.
fn resolve_env_var(value: &str) -> String {
    if value.starts_with("${") && value.ends_with('}') {
        let var_name = &value[2..value.len() - 1];
        std::env::var(var_name).unwrap_or_else(|_| value.to_string())
    } else {
        value.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_resolve_env_var() {
        std::env::set_var("TEST_TOOL_VAR", "test_value");

        assert_eq!(resolve_env_var("${TEST_TOOL_VAR}"), "test_value");
        assert_eq!(resolve_env_var("plain_value"), "plain_value");
        assert_eq!(
            resolve_env_var("${NONEXISTENT_VAR}"),
            "${NONEXISTENT_VAR}"
        );

        std::env::remove_var("TEST_TOOL_VAR");
    }

    #[tokio::test]
    async fn test_build_registry_empty() {
        let registry = build_registry_from_config(&[], &Default::default(), 2000).await.unwrap();

        // Should have default native source
        assert_eq!(registry.source_count(), 1);
        assert!(registry.has_tool("bash"));
    }

    #[tokio::test]
    async fn test_build_registry_native() {
        let configs = vec![ToolSourceConfig::Native {
            toolset: "opencode".to_string(),
        }];

        let registry = build_registry_from_config(&configs, &Default::default(), 2000).await.unwrap();

        assert_eq!(registry.source_count(), 1);
        assert!(registry.has_tool("bash"));
        assert!(registry.has_tool("read"));
    }

    /// THE FACTORY WIRING PIN (issue 6121fc76): a `service-account`
    /// credential reference must wire the DYNAMIC token path — the Mcp arm
    /// previously wired ONLY the static `auth_token`, so the entire dynamic
    /// path was dead code here. The pin drives `wire_server_auth` against a
    /// MOCK IdP (the discovery + the token endpoint minting short-TTL
    /// tokens) and asserts: the provider attached + the proactive refresher
    /// EXCHANGED (the mint count ≥ 1 — the spawn fires immediately when the
    /// cache is empty). The static-only old code minted ZERO.
    #[tokio::test]
    #[cfg(feature = "registry-mcp-token")]
    async fn factory_service_account_credential_wires_the_dynamic_token_path() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        // ── the mock IdP on a raw TCP listener ──────────────────────────
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        listener.set_nonblocking(true).unwrap();
        let mints = Arc::new(AtomicUsize::new(0));
        let mints_clone = mints.clone();
        let server = tokio::spawn(async move {
            let listener = tokio::net::TcpListener::from_std(listener).unwrap();
            loop {
                let (mut sock, _) = match listener.accept().await {
                    Ok(s) => s,
                    Err(_) => break,
                };
                let mints = mints_clone.clone();
                tokio::spawn(async move {
                    let mut buf = vec![0u8; 8192];
                    let n = sock.read(&mut buf).await.unwrap_or(0);
                    let req = String::from_utf8_lossy(&buf[..n]).to_string();
                    let body = if req.contains("well-known") {
                        let doc = format!(
                            "{{\"issuer\":\"http://127.0.0.1:{port}\",\"jwks_uri\":\"http://127.0.0.1:{port}/jwks\",\"token_endpoint\":\"http://127.0.0.1:{port}/token\"}}"
                        );
                        format!(
                            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                            doc.len(), doc
                        )
                    } else if req.contains("POST /token") {
                        // the RFC 8693 exchange: mint a SHORT-TTL token (2s)
                        let m = mints.fetch_add(1, Ordering::SeqCst) + 1;
                        let tok = format!("mock-token-{m}");
                        let body = format!("{{\"access_token\":\"{tok}\",\"expires_in\":2,\"token_type\":\"Bearer\"}}");
                        format!(
                            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                            body.len(), body
                        )
                    } else {
                        "HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_string()
                    };
                    let _ = sock.write_all(body.as_bytes()).await;
                });
            }
        });

        // ── the wiring: a service-account credential reference ──────────
        let mut credentials = HashMap::new();
        credentials.insert(
            "svc".to_string(),
            McpCredentialConfig::ServiceAccount {
                service_token: "svc-token-1".to_string(),
                issuer_url: format!("http://127.0.0.1:{port}"),
                client_id: "pdt-api".to_string(),
                client_secret: None,
                audience: "pdt-api".to_string(),
                scope: None,
            },
        );
        let config = wire_server_auth(
            "mock",
            &format!("http://127.0.0.1:{port}/mcp"),
            None,
            Some("svc"),
            &credentials,
        );

        // THE PIN (leg 1): the provider is ATTACHED — the old code wired
        // only the static auth_token (None here) and the dynamic path was
        // dead code.
        assert!(
            config.token_provider.is_some(),
            "a service-account credential reference must attach the token \
             provider (issue 6121fc76: the factory's Mcp arm wired only the \
             static auth_token — the dynamic path was dead code)"
        );
        assert!(config.auth_token.is_none(), "no static token when the provider is wired");

        // THE PIN (leg 2): the proactive refresher EXCHANGED — the spawn
        // fires immediately when the cache is empty; the mint count ≥ 1
        // proves the dynamic path is LIVE (the old code minted ZERO).
        for _ in 0..50 {
            if mints.load(Ordering::SeqCst) >= 1 {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }
        assert!(
            mints.load(Ordering::SeqCst) >= 1,
            "the spawned proactive refresher must exchange (the mint count ≥ 1) — \
             the dynamic token path is dead code if the provider never mints"
        );

        // ── the static credential still resolves via with_auth ──────────
        let mut static_creds = HashMap::new();
        static_creds.insert(
            "st".to_string(),
            McpCredentialConfig::Static { token: "${TEST_FACTORY_TOK}".to_string() },
        );
        std::env::set_var("TEST_FACTORY_TOK", "resolved-static");
        let config = wire_server_auth("mock2", "http://127.0.0.1:1/mcp", None, Some("st"), &static_creds);
        std::env::remove_var("TEST_FACTORY_TOK");
        assert!(
            config.token_provider.is_none(),
            "a static credential must NOT attach a provider"
        );
        assert_eq!(
            config.auth_token.as_deref(),
            Some("resolved-static"),
            "a static credential resolves env vars into the static token"
        );

        // ── the auth_token fallback when no credentials reference ───────
        let config = wire_server_auth("mock3", "http://127.0.0.1:1/mcp", Some("plain-tok"), None, &credentials);
        assert!(config.token_provider.is_none());
        assert_eq!(config.auth_token.as_deref(), Some("plain-tok"));

        server.abort();
    }
}

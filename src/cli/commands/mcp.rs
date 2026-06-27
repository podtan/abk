//! MCP management command implementations.
//!
//! Provides `trustee mcp` subcommands for managing MCP server connections
//! and authentication:
//! - `mcp auth <name>` — interactive OAuth browser login (PKCE)
//! - `mcp list` — list MCP servers and their auth status
//! - `mcp add <name>` — register a new MCP server
//! - `mcp logout <name>` — remove stored OAuth credentials
//! - `mcp debug <name>` — diagnostics for MCP connection and auth

use clap::ArgMatches;

use crate::cli::adapters::CommandContext;
use crate::cli::error::{CliError, CliResult};

/// Entry point for the `mcp` command.
///
/// Dispatches to subcommand handlers based on the subcommand name.
pub async fn mcp_command<C: CommandContext>(
    ctx: &C,
    matches: &ArgMatches,
) -> CliResult<()> {
    match matches.subcommand() {
        Some(("auth", sub)) => mcp_auth(ctx, sub).await,
        Some(("list", sub)) => mcp_list(ctx, sub).await,
        Some(("add", sub)) => mcp_add(ctx, sub).await,
        Some(("logout", sub)) => mcp_logout(ctx, sub).await,
        Some(("debug", sub)) => mcp_debug(ctx, sub).await,
        _ => {
            ctx.log_info("Usage: trustee mcp <auth|list|add|logout|debug>");
            ctx.log_info("");
            ctx.log_info("  auth <name>    Authenticate with an MCP credential via browser login");
            ctx.log_info("  list           List MCP servers and their authentication status");
            ctx.log_info("  add <name>     Register a new MCP server");
            ctx.log_info("  logout <name>  Remove stored OAuth credentials");
            ctx.log_info("  debug <name>   Debug MCP connection and authentication");
            Ok(())
        }
    }
}

// ---------------------------------------------------------------------------
// mcp auth <name>
// ---------------------------------------------------------------------------

/// Interactive OAuth browser login flow.
///
/// Uses PEP's CallbackServer + OidcClient to perform PKCE-based authentication.
/// Tokens are stored via FileTokenStore for later use by InteractiveTokenProvider.
#[cfg(feature = "registry-mcp-token")]
async fn mcp_auth<C: CommandContext>(ctx: &C, matches: &ArgMatches) -> CliResult<()> {
    let cred_name = matches
        .get_one::<String>("name")
        .cloned()
        .ok_or_else(|| CliError::ValidationError("Missing credential name".to_string()))?;

    // Find credential in config
    let mcp_config = &ctx.config().mcp;
    let cred = mcp_config
        .credentials
        .get(&cred_name)
        .ok_or_else(|| {
            CliError::ConfigError(format!(
                "Credential '{}' not found in [mcp.credentials]",
                cred_name
            ))
        })?;

    // Verify it's interactive type
    let crate::config::McpCredentialConfig::Interactive {
        issuer_url,
        client_id,
        client_secret,
        scope,
        redirect_port,
    } = cred
    else {
        return Err(CliError::ValidationError(format!(
            "Credential '{}' is not interactive type. Use type = \"interactive\" in config.",
            cred_name
        )));
    };

    let issuer_url = resolve_env_var(issuer_url);
    let client_id = resolve_env_var(client_id);
    let client_secret = client_secret.as_ref().map(|s| resolve_env_var(s));
    let scope = resolve_env_var(scope);
    let redirect_port = *redirect_port;

    // Generate PKCE pair
    let oidc_client = pep::OidcClient::new();
    let verifier = pep::oidc_client::OidcClient::generate_code_verifier();
    let challenge = pep::oidc_client::OidcClient::generate_code_challenge(&verifier);
    let state = pep::oidc_client::OidcClient::generate_state();

    // Start callback server
    let callback = pep::CallbackServer::new(redirect_port);

    // Build OidcClientConfig for the auth flow
    use pep::oidc::types::OidcClientConfig;
    let client_config = OidcClientConfig {
        issuer_url: issuer_url.clone(),
        client_id: client_id.clone(),
        client_secret: client_secret.clone(),
        redirect_uri: callback.redirect_uri(),
        scope: scope.clone(),
        code_challenge_method: "S256".to_string(),
    };

    // Build authorization URL
    ctx.log_info("Building authorization URL...");
    let auth_url = oidc_client
        .build_authorization_url(&client_config, &state, Some(&challenge))
        .await
        .map_err(|e| CliError::ConfigError(format!("Failed to build auth URL: {}", e)))?;

    // Open browser
    ctx.log_info("Opening browser for authentication...");
    ctx.log_info(&format!(
        "   If browser doesn't open, visit:\n   {}",
        auth_url
    ));
    open_browser(&auth_url);

    // Wait for callback
    ctx.log_info("   Waiting for authentication callback...");
    let auth_code = callback
        .wait_for_code()
        .await
        .map_err(|e| CliError::ConfigError(format!("OAuth callback failed: {}", e)))?;

    // CSRF check
    if let Some(ref received_state) = auth_code.state {
        if received_state != &state {
            return Err(CliError::ValidationError(
                "State mismatch — possible CSRF attack".to_string(),
            ));
        }
    }

    // Exchange code for tokens
    ctx.log_info("Exchanging authorization code for tokens...");
    let tokens = oidc_client
        .exchange_code_for_tokens(&client_config, &auth_code.code, Some(&verifier))
        .await
        .map_err(|e| CliError::ConfigError(format!("Token exchange failed: {}", e)))?;

    // Store tokens
    let agent_name = std::env::var("ABK_AGENT_NAME").unwrap_or_else(|_| "trustee".into());
    let token_store = pep::FileTokenStore::new(&agent_name);

    use pep::token_store::StoredToken;

    // Compute expires_at as RFC-3339 from expires_in seconds
    let expires_at = {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let expires_epoch = now + tokens.expires_in.unwrap_or(900);
        epoch_to_rfc3339_local(expires_epoch)
    };

    let stored = StoredToken::new(
        &tokens.access_token,
        tokens.refresh_token.clone(),
        &tokens.token_type,
        &expires_at,
        tokens.scope.clone(),
    );

    token_store
        .save(&cred_name, &stored)
        .map_err(|e| CliError::ConfigError(format!("Failed to store token: {}", e)))?;

    ctx.log_success(&format!("Authentication successful for '{}'", cred_name));
    ctx.log_info(&format!("Token expires at: {}", stored.expires_at));
    ctx.log_info("Token will be automatically refreshed when needed.");

    Ok(())
}

#[cfg(not(feature = "registry-mcp-token"))]
async fn mcp_auth<C: CommandContext>(ctx: &C, _matches: &ArgMatches) -> CliResult<()> {
    ctx.log_error("MCP auth requires the 'registry-mcp-token' feature to be enabled.")?;
    Ok(())
}

// ---------------------------------------------------------------------------
// mcp list
// ---------------------------------------------------------------------------

/// List all configured MCP servers and their authentication status.
async fn mcp_list<C: CommandContext>(ctx: &C, matches: &ArgMatches) -> CliResult<()> {
    let verbose = matches.try_get_flag("verbose").unwrap_or(false);
    let mcp_config = &ctx.config().mcp;

    if !mcp_config.enabled {
        ctx.log_info("MCP is disabled in configuration.");
        return Ok(());
    }

    if mcp_config.servers.is_empty() {
        ctx.log_info("No MCP servers configured.");
        return Ok(());
    }

    // Print header
    println!(
        "\n{:<12} {:<35} {:<22} {:<16} {}",
        "NAME", "URL", "CREDENTIAL", "AUTH MODE", "STATUS"
    );
    println!("{}", "─".repeat(100));

    for server in &mcp_config.servers {
        let (auth_mode, credential_name, status) =
            if let Some(cred_ref) = &server.credentials {
                if let Some(cred) = mcp_config.credentials.get(cred_ref) {
                    match cred {
                        crate::config::McpCredentialConfig::Static { .. } => {
                            ("static", cred_ref.as_str(), "configured")
                        }
                        crate::config::McpCredentialConfig::ServiceAccount { .. } => {
                            ("service-account", cred_ref.as_str(), "auto-refresh")
                        }
                        crate::config::McpCredentialConfig::Interactive { .. } => {
                            // Check if token file exists and is valid
                            #[cfg(feature = "registry-mcp-token")]
                            {
                                let agent_name = std::env::var("ABK_AGENT_NAME")
                                    .unwrap_or_else(|_| "trustee".into());
                                let token_store = pep::FileTokenStore::new(&agent_name);
                                match token_store.load(cred_ref) {
                                    Ok(Some(token)) => {
                                        if token.is_expired() {
                                            ("interactive", cred_ref.as_str(), "expired (will refresh)")
                                        } else {
                                            ("interactive", cred_ref.as_str(), "authenticated")
                                        }
                                    }
                                    _ => ("interactive", cred_ref.as_str(), "not authenticated"),
                                }
                            }
                            #[cfg(not(feature = "registry-mcp-token"))]
                            {
                                ("interactive", cred_ref.as_str(), "(registry-mcp-token disabled)")
                            }
                        }
                    }
                } else {
                    ("?", cred_ref.as_str(), "credential not found")
                }
            } else if server.auth_token.is_some() {
                ("static", "-", "configured")
            } else {
                ("none", "-", "no auth")
            };

        println!(
            "{:<12} {:<35} {:<22} {:<16} {}",
            server.name, server.url, credential_name, auth_mode, status
        );
    }

    if verbose {
        ctx.log_info(&format!("\nTimeout: {}s", mcp_config.timeout_seconds));
        ctx.log_info(&format!("Total servers: {}", mcp_config.servers.len()));
        ctx.log_info(&format!(
            "Credentials defined: {}",
            mcp_config.credentials.len()
        ));
    }

    println!();
    Ok(())
}

// ---------------------------------------------------------------------------
// mcp logout <name>
// ---------------------------------------------------------------------------

/// Remove stored OAuth credentials for a credential.
async fn mcp_logout<C: CommandContext>(ctx: &C, matches: &ArgMatches) -> CliResult<()> {
    let cred_name = matches
        .get_one::<String>("name")
        .cloned()
        .ok_or_else(|| CliError::ValidationError("Missing credential name".to_string()))?;

    // Verify credential exists in config
    let mcp_config = &ctx.config().mcp;
    if !mcp_config.credentials.contains_key(&cred_name) {
        return Err(CliError::ConfigError(format!(
            "Credential '{}' not found in [mcp.credentials]",
            cred_name
        )));
    }

    // Delete token file
    #[cfg(feature = "registry-mcp-token")]
    {
        let agent_name = std::env::var("ABK_AGENT_NAME").unwrap_or_else(|_| "trustee".into());
        let token_store = pep::FileTokenStore::new(&agent_name);

        match token_store.load(&cred_name) {
            Ok(Some(_)) => {
                token_store
                    .delete(&cred_name)
                    .map_err(|e| CliError::ConfigError(format!("Failed to delete token: {}", e)))?;
                ctx.log_success(&format!("Credentials removed for '{}'", cred_name));
                ctx.log_info("You will need to run `trustee mcp auth` to authenticate again.");
            }
            Ok(None) => {
                ctx.log_info(&format!(
                    "No stored credentials found for '{}'. Nothing to remove.",
                    cred_name
                ));
            }
            Err(e) => {
                return Err(CliError::ConfigError(format!(
                    "Error checking credentials: {}",
                    e
                )));
            }
        }
    }

    #[cfg(not(feature = "registry-mcp-token"))]
    {
        ctx.log_error("MCP logout requires the 'registry-mcp-token' feature to be enabled.")?;
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// mcp add <name>
// ---------------------------------------------------------------------------

/// Register a new MCP server.
///
/// Appends TOML configuration to the user's config file.
async fn mcp_add<C: CommandContext>(ctx: &C, matches: &ArgMatches) -> CliResult<()> {
    let name = matches
        .get_one::<String>("name")
        .cloned()
        .ok_or_else(|| CliError::ValidationError("Missing server name".to_string()))?;

    let url = matches
        .get_one::<String>("url")
        .cloned()
        .ok_or_else(|| CliError::ValidationError("Missing server URL".to_string()))?;

    let auth_mode = matches
        .get_one::<String>("auth_mode")
        .cloned()
        .unwrap_or_else(|| "interactive".to_string());

    // Read existing config file
    let config_path = ctx.config_path()?;
    let config_content = std::fs::read_to_string(&config_path)
        .map_err(|e| CliError::ConfigError(format!("Cannot read config: {}", e)))?;

    // Check for duplicate
    if config_content.contains(&format!("name = \"{}\"", name)) {
        return Err(CliError::ValidationError(format!(
            "Server '{}' may already exist in config. Check {}.",
            name,
            config_path.display()
        )));
    }

    // Build the new TOML section based on auth mode
    let new_section = match auth_mode.as_str() {
        "static" => {
            let token = prompt("Auth token (or ${ENV_VAR}): ");
            format!(
                "\n[[mcp.servers]]\nname = \"{}\"\nurl = \"{}\"\ntransport = \"http\"\nauto_init = true\nauth_token = \"{}\"\n",
                name, url, token
            )
        }
        "service-account" => {
            let cred_name = format!("{}_svc", name);
            let issuer = prompt("Issuer URL: ");
            let client_id = prompt("Client ID: ");
            let service_token = prompt("Service token (or ${ENV_VAR}): ");
            let audience = prompt("Audience: ");
            let scope = prompt_default("Scope", "openid profile email groups");

            format!(
                "\n[mcp.credentials.{}]\ntype = \"service-account\"\nservice_token = \"{}\"\nissuer_url = \"{}\"\nclient_id = \"{}\"\naudience = \"{}\"\nscope = \"{}\"\n\n[[mcp.servers]]\nname = \"{}\"\nurl = \"{}\"\ntransport = \"http\"\nauto_init = true\ncredentials = \"{}\"\n",
                cred_name, service_token, issuer, client_id, audience, scope,
                name, url, cred_name
            )
        }
        "interactive" => {
            let cred_name = format!("{}_interactive", name);
            let issuer = prompt("Issuer URL: ");
            let client_id = prompt("Client ID: ");
            let scope = prompt_default("Scope", "openid profile email groups");
            let redirect_port = prompt_default("Redirect port", "8765");

            format!(
                "\n[mcp.credentials.{}]\ntype = \"interactive\"\nissuer_url = \"{}\"\nclient_id = \"{}\"\nscope = \"{}\"\nredirect_port = {}\n\n[[mcp.servers]]\nname = \"{}\"\nurl = \"{}\"\ntransport = \"http\"\nauto_init = true\ncredentials = \"{}\"\n",
                cred_name, issuer, client_id, scope, redirect_port,
                name, url, cred_name
            )
        }
        _ => {
            return Err(CliError::ValidationError(format!(
                "Unknown auth mode: {}",
                auth_mode
            )))
        }
    };

    // Append to config file
    let updated = format!("{}\n{}", config_content.trim_end(), new_section);
    std::fs::write(&config_path, updated)
        .map_err(|e| CliError::ConfigError(format!("Cannot write config: {}", e)))?;

    ctx.log_success(&format!("Added MCP server '{}' to {}", name, config_path.display()));

    if auth_mode == "interactive" {
        let cred_name = format!("{}_interactive", name);
        ctx.log_info(&format!("To authenticate: trustee mcp auth {}", cred_name));
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// mcp debug <name>
// ---------------------------------------------------------------------------

/// Debug MCP connection and authentication.
async fn mcp_debug<C: CommandContext>(ctx: &C, matches: &ArgMatches) -> CliResult<()> {
    let name = matches
        .get_one::<String>("name")
        .cloned()
        .ok_or_else(|| CliError::ValidationError("Missing server name".to_string()))?;

    let mcp_config = &ctx.config().mcp;

    // Find server by name
    let server = mcp_config
        .servers
        .iter()
        .find(|s| s.name == name)
        .ok_or_else(|| {
            CliError::ConfigError(format!(
                "Server '{}' not found in [[mcp.servers]]",
                name
            ))
        })?;

    println!("\nDebugging MCP server: {}\n", name);

    // Check 1: Config
    print!("Checking config...                    ");
    println!(
        "Found (url={}, transport={})",
        server.url, server.transport
    );

    // Check 2: Credential resolution
    if let Some(cred_ref) = &server.credentials {
        print!("Checking credential reference...       ");
        match mcp_config.credentials.get(cred_ref) {
            Some(cred) => match cred {
                crate::config::McpCredentialConfig::Static { .. } => {
                    println!("Found '{}' (static)", cred_ref);
                }
                crate::config::McpCredentialConfig::ServiceAccount { .. } => {
                    println!("Found '{}' (service-account)", cred_ref);
                }
                crate::config::McpCredentialConfig::Interactive { .. } => {
                    println!("Found '{}' (interactive)", cred_ref);

                    // Check 3: Token status
                    #[cfg(feature = "registry-mcp-token")]
                    {
                        print!("Checking stored token...               ");
                        let agent_name =
                            std::env::var("ABK_AGENT_NAME").unwrap_or_else(|_| "trustee".into());
                        let token_store = pep::FileTokenStore::new(&agent_name);
                        match token_store.load(cred_ref) {
                            Ok(Some(token)) => {
                                if token.is_expired() {
                                    if token.refresh_token.is_some() {
                                        println!("Expired, but refresh token available (will auto-refresh)");
                                    } else {
                                        println!("Expired, no refresh token. Run: trustee mcp auth {}", cred_ref);
                                    }
                                } else {
                                    println!("Valid (expires at: {})", token.expires_at);
                                }
                            }
                            Ok(None) => {
                                println!("No token stored. Run: trustee mcp auth {}", cred_ref);
                            }
                            Err(e) => println!("Error loading token: {}", e),
                        }
                    }
                }
            },
            None => {
                println!("Credential '{}' not found in [mcp.credentials]", cred_ref);
            }
        }
    }

    // Check 4: Server reachability
    print!("Checking server reachability...        ");
    match check_server_reachable(&server.url).await {
        Ok(status) => println!("Reachable (HTTP {})", status),
        Err(e) => println!("Cannot reach: {}", e),
    }

    println!("\nDebug complete.\n");
    Ok(())
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Open a URL in the default browser (cross-platform).
fn open_browser(url: &str) {
    #[cfg(target_os = "linux")]
    let _ = std::process::Command::new("xdg-open").arg(url).spawn();
    #[cfg(target_os = "macos")]
    let _ = std::process::Command::new("open").arg(url).spawn();
    #[cfg(target_os = "windows")]
    let _ = std::process::Command::new("cmd")
        .args(["/C", "start", url])
        .spawn();
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

/// Prompt the user for input (reads from stdin).
fn prompt(message: &str) -> String {
    use std::io::{self, Write};
    print!("{}", message);
    let _ = io::stdout().flush();
    let mut input = String::new();
    let _ = io::stdin().read_line(&mut input);
    input.trim().to_string()
}

/// Prompt with a default value shown in brackets.
fn prompt_default(message: &str, default: &str) -> String {
    let input = prompt(&format!("{} [{}]: ", message, default));
    if input.is_empty() {
        default.to_string()
    } else {
        input
    }
}

/// Check if a server URL is reachable via HTTP GET.
async fn check_server_reachable(url: &str) -> Result<u16, String> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .map_err(|e| e.to_string())?;
    let resp = client
        .get(url)
        .send()
        .await
        .map_err(|e| e.to_string())?;
    Ok(resp.status().as_u16())
}

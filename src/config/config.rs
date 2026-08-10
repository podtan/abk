//! TOML configuration parsing and management.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

/// Installation configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstallationConfig {
    pub binary_name: String,
    pub binary_source_path: String,
    pub local_bin_path: String,
}

/// Main configuration structure
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Configuration {
    pub agent: AgentConfig,
    pub installation: Option<InstallationConfig>,
    pub logging: LoggingConfig,
    pub execution: ExecutionConfig,
    pub modes: ModesConfig,
    pub tools: ToolsConfig,
    pub search_filtering: Option<SearchFilteringConfig>,
    pub llm: Option<LlmConfig>,
    pub mcp: Option<McpConfig>,
    pub lifecycle: Option<LifecycleConfig>,
    /// Unified tool source configuration
    #[serde(default)]
    pub tool_sources: Vec<ToolSourceConfig>,
    #[cfg(feature = "checkpoint")]
    pub checkpointing: Option<crate::checkpoint::GlobalCheckpointConfig>,
    #[cfg(feature = "cli")]
    pub cli: Option<crate::cli::config::CliConfig>,
}

/// Tool source configuration for unified registry
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum ToolSourceConfig {
    /// Native cats tool source
    Native {
        /// Toolset to use: "opencode", "old", etc.
        #[serde(default = "default_toolset")]
        toolset: String,
    },
    /// MCP server tool source
    Mcp {
        /// Server name/identifier
        name: String,
        /// Server URL
        url: String,
        /// Optional authentication token (supports env var substitution)
        #[serde(default)]
        auth_token: Option<String>,
        /// Auto-initialize connection
        #[serde(default = "default_auto_init")]
        auto_init: bool,
    },
}

fn default_toolset() -> String {
    "opencode".to_string()
}

/// Lifecycle extension configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LifecycleConfig {
    /// Enable lifecycle extension (templates, task classification)
    #[serde(default = "default_lifecycle_enabled")]
    pub enabled: bool,
    /// Custom system template for simple lifecycle (used when enabled = false)
    #[serde(default)]
    pub system_template: Option<String>,
}

fn default_lifecycle_enabled() -> bool {
    true
}

/// MCP (Model Context Protocol) configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpConfig {
    /// Enable MCP tool discovery
    #[serde(default)]
    pub enabled: bool,
    /// Timeout for MCP server requests (seconds)
    #[serde(default = "default_mcp_timeout")]
    pub timeout_seconds: u64,
    /// Named credential definitions (shared across servers)
    #[serde(default)]
    pub credentials: HashMap<String, McpCredentialConfig>,
    /// List of MCP servers to connect to
    #[serde(default)]
    pub servers: Vec<McpServerConfig>,
}

fn default_mcp_timeout() -> u64 {
    30
}

impl Default for McpConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            timeout_seconds: 30,
            credentials: HashMap::new(),
            servers: vec![],
        }
    }
}

/// Configuration for a single MCP server
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpServerConfig {
    /// Server identifier (used for logging and tool namespacing)
    pub name: String,
    /// Server URL (SSE endpoint for HTTP transport)
    pub url: String,
    /// Transport type: "http" (SSE) or "stdio"
    #[serde(default = "default_transport")]
    pub transport: String,
    /// Optional static authentication token (supports env var substitution).
    /// Use `credentials` for dynamic token management.
    pub auth_token: Option<String>,
    /// Reference to a named credential in `mcp.credentials`.
    /// Takes priority over `auth_token` when both are present.
    pub credentials: Option<String>,
    /// Auto-initialize connection (send initialize/initialized messages)
    #[serde(default = "default_auto_init")]
    pub auto_init: bool,
}

/// Named credential configuration for MCP server authentication.
///
/// Supports five modes:
/// - `static`: A plain token string (same as `auth_token`, but reusable)
/// - `service-account`: Long-lived API token → RFC 8693 exchange for short-lived OIDC access tokens
/// - `interactive`: Browser-based OAuth login (PKCE) via `trustee mcp auth` CLI
/// - `web-session`: Reuse the trustee-web user's session token (injected before each command)
/// - `web-interactive`: Per-server browser login via trustee-web UI (`/auth/mcp/login`)
///
/// # Example (TOML)
///
/// ```toml
/// [mcp.credentials.kanidm_pdt]
/// type = "service-account"
/// service_token = "${PDT_SVC_TOKEN}"
/// issuer_url = "https://idm.tanbal.ir/oauth2/openid/pdt-api"
/// client_id = "pdt-api"
/// audience = "pdt-api"
/// ```
///
/// ```toml
/// [mcp.credentials.kanidm_interactive]
/// type = "interactive"
/// issuer_url = "https://idm.tanbal.ir/oauth2/openid/pdt-api"
/// client_id = "pdt-api"
/// scope = "openid profile email groups"
/// redirect_port = 8765
/// ```
///
/// ```toml
/// # C1: Reuse the logged-in user's session token
/// [mcp.credentials.session]
/// type = "web-session"
/// ```
///
/// ```toml
/// # C2: Per-server browser login via trustee-web
/// [mcp.credentials.pdt_login]
/// type = "web-interactive"
/// issuer_url = "https://idm.tanbal.ir/oauth2/openid/pdt-api"
/// client_id = "pdt-api"
/// scope = "openid profile email groups"
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum McpCredentialConfig {
    /// Static token string (resolved once at startup).
    Static {
        /// The token value (supports `${ENV_VAR}` substitution).
        token: String,
    },
    /// Service account with automatic RFC 8693 token exchange.
    ServiceAccount {
        /// Long-lived Kanidm service account API token (never expires).
        /// Supports `${ENV_VAR}` substitution.
        service_token: String,
        /// OIDC issuer URL (e.g. `https://idm.tanbal.ir/oauth2/openid/pdt-api`).
        issuer_url: String,
        /// OAuth2 client ID (e.g. `pdt-api`).
        client_id: String,
        /// OAuth2 client secret (optional, for confidential clients).
        client_secret: Option<String>,
        /// Target audience for the exchanged token (e.g. `pdt-api`).
        audience: String,
        /// Scopes to request during exchange (optional, default: `openid profile email`).
        scope: Option<String>,
    },
    /// Interactive browser-based OAuth login with PKCE.
    ///
    /// Tokens are obtained via `trustee mcp auth` and stored on disk.
    /// The `InteractiveTokenProvider` loads/refreshes them automatically.
    Interactive {
        /// OIDC issuer URL.
        issuer_url: String,
        /// OAuth2 client ID.
        client_id: String,
        /// OAuth2 client secret (optional for public clients).
        client_secret: Option<String>,
        /// OAuth2 scopes to request.
        scope: String,
        /// Local port for the OAuth callback server.
        #[serde(default = "default_redirect_port")]
        redirect_port: u16,
    },
    /// Web session token reuse (C1).
    ///
    /// Uses the logged-in user's OIDC session token from trustee-web.
    /// Trustee-web pushes the current access token into `FileTokenStore`
    /// under the reserved name `__web_session` before each agent command.
    /// The `InteractiveTokenProvider` reads it on every tool call.
    WebSession {
        /// Optional RFC 8693 token exchange if the MCP server requires
        /// a different audience than the trustee-web session token.
        #[serde(default)]
        exchange: Option<ExchangeConfig>,
    },
    /// Per-server browser login via trustee-web UI (C2).
    ///
    /// Tokens are obtained via trustee-web's `/auth/mcp/login` route
    /// and stored on disk via `FileTokenStore`. The `InteractiveTokenProvider`
    /// loads and refreshes them automatically (same as `Interactive`, but
    /// login is triggered from the web UI, not the CLI).
    WebInteractive {
        /// OIDC issuer URL.
        issuer_url: String,
        /// OAuth2 client ID.
        client_id: String,
        /// OAuth2 client secret (optional for public clients).
        client_secret: Option<String>,
        /// OAuth2 scopes to request.
        scope: String,
    },
}

/// Optional RFC 8693 token exchange configuration for `web-session` credentials.
///
/// Used when the trustee-web session token is issued by the `trustee` OIDC
/// client but the MCP server expects tokens from a different client
/// (e.g. `pdt-api`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExchangeConfig {
    /// OIDC issuer URL for the token exchange.
    pub issuer_url: String,
    /// OAuth2 client ID for the target service.
    pub client_id: String,
    /// Target audience for the exchanged token (e.g. `pdt-api`).
    pub audience: String,
    /// OAuth2 client secret (optional, for confidential clients).
    pub client_secret: Option<String>,
}

/// Default redirect port for interactive OAuth callback.
fn default_redirect_port() -> u16 {
    8765
}

fn default_transport() -> String {
    "http".to_string()
}

fn default_auto_init() -> bool {
    true
}

/// Tools configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolsConfig {
    pub open_file_window_size: Option<usize>,
    pub max_tool_result_size_bytes: Option<u64>,
    pub truncate_large_results: Option<bool>,
    /// Allowlist of tool names. `None` (absent) = all tools (backward compatible).
    /// `Some([])` = zero tools (locked down). `Some(["read", "grep"])` = only those.
    #[serde(default)]
    pub enabled_tools: Option<Vec<String>>,
    /// Denylist of tool names. Takes precedence over `enabled_tools`.
    /// e.g. `["bash"]` = all tools except bash.
    #[serde(default)]
    pub disabled_tools: Vec<String>,
}

/// LLM provider configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmConfig {
    pub endpoint: String,
    pub enable_streaming: bool,
    /// Optional utility LLM configuration for lightweight background calls
    /// (e.g., session title generation). Falls back to main provider if absent.
    #[serde(default)]
    pub utility: Option<UtilityLlmConfig>,
}

impl Default for LlmConfig {
    fn default() -> Self {
        Self {
            endpoint: "chat/completions".to_string(),
            enable_streaming: true,
            utility: None,
        }
    }
}

/// Configuration for utility LLM calls (session titles, summaries, etc.)
///
/// When present in `[llm.utility]`, these settings override the main provider's
/// defaults for lightweight background tasks. When absent, the main provider is used.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UtilityLlmConfig {
    /// Model override for utility calls (e.g., "gpt-4o-mini").
    /// None = use provider default model.
    #[serde(default)]
    pub model: Option<String>,
    /// Max tokens for utility calls. Default: 100.
    #[serde(default = "default_utility_max_tokens")]
    pub max_tokens: u32,
    /// Temperature for utility calls. Default: 0.3 (deterministic-ish).
    #[serde(default = "default_utility_temperature")]
    pub temperature: f32,
}

fn default_utility_max_tokens() -> u32 {
    1000
    }

fn default_utility_temperature() -> f32 {
    0.3
}

/// Search filtering configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchFilteringConfig {
    pub enabled: Option<bool>,
    pub exclude_dirs: Option<Vec<String>>,
    pub exclude_extensions: Option<Vec<String>>,
    pub exclude_hidden: Option<bool>,
}

impl Default for SearchFilteringConfig {
    fn default() -> Self {
        Self {
            enabled: Some(true),
            exclude_dirs: Some(vec![
                "target".to_string(),
                "node_modules".to_string(),
                "__pycache__".to_string(),
                "dist".to_string(),
                "build".to_string(),
                ".git".to_string(),
                ".svn".to_string(),
                ".hg".to_string(),
                "venv".to_string(),
                "env".to_string(),
                ".venv".to_string(),
            ]),
            exclude_extensions: Some(vec![
                "exe".to_string(),
                "dll".to_string(),
                "so".to_string(),
                "dylib".to_string(),
                "a".to_string(),
                "o".to_string(),
                "pyc".to_string(),
                "png".to_string(),
                "jpg".to_string(),
                "jpeg".to_string(),
                "gif".to_string(),
                "bmp".to_string(),
                "ico".to_string(),
                "mp3".to_string(),
                "mp4".to_string(),
                "avi".to_string(),
                "mov".to_string(),
                "wav".to_string(),
                "pdf".to_string(),
                "zip".to_string(),
                "tar".to_string(),
                "gz".to_string(),
                "rar".to_string(),
                "7z".to_string(),
            ]),
            exclude_hidden: Some(true),
        }
    }
}

/// Agent configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentConfig {
    pub name: String,
    pub version: String,
    pub default_mode: String,
    pub enable_task_classification: Option<bool>,
}

/// Logging configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoggingConfig {
    /// Directory for log files. Timestamped log files are created inside.
    /// If empty, defaults to /tmp/{ABK_AGENT_NAME}/
    #[serde(default)]
    pub log_dir: String,
    pub log_level: String,
}

/// Retry delay strategy
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RetryStrategy {
    /// Fixed delay: `retry_base_delay_seconds` between every attempt.
    Fixed,
    /// Exponential backoff: base * 2^attempt (1x, 2x, 4x, 8x, ...).
    Exponential,
    /// Linear backoff: base * (attempt+1) (1x, 2x, 3x, 4x, ...).
    Linear,
}

impl Default for RetryStrategy {
    fn default() -> Self {
        RetryStrategy::Exponential
    }
}

impl RetryStrategy {
    /// Calculate the delay for a given attempt (0-indexed).
    pub fn delay_secs(&self, attempt: u32, base: u64) -> u64 {
        match self {
            RetryStrategy::Fixed => base,
            RetryStrategy::Exponential => base.saturating_mul(2u64.saturating_pow(attempt)),
            RetryStrategy::Linear => base.saturating_mul(attempt as u64 + 1),
        }
    }
}

/// Execution configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionConfig {
    pub max_iterations: u32,
    pub timeout_seconds: u64,
    pub max_retries: u32,
    pub max_tokens: u32,
    pub max_history: u32,
    pub request_interval_seconds: u64,
    pub enable_dangerous_command_validation: bool,
    /// Base delay in seconds between retries (default: 1).
    #[serde(default = "default_retry_base_delay")]
    pub retry_base_delay_seconds: u64,
    /// Delay strategy for retries: fixed, exponential, or linear.
    #[serde(default)]
    pub retry_strategy: RetryStrategy,
}

fn default_retry_base_delay() -> u64 {
    1
}

/// Modes configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModesConfig {
    pub confirm: ModeConfig,
    pub yolo: ModeConfig,
    pub human: ModeConfig,
}

/// Individual mode configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModeConfig {
    pub description: String,
    pub auto_execute: bool,
}

/// Loads and manages TOML configuration.
#[derive(Debug)]
pub struct ConfigurationLoader {
    pub config_path: PathBuf,
    pub config: Configuration,
    #[allow(dead_code)]
    template_base: Option<PathBuf>,
    #[allow(dead_code)]
    _log_base: Option<PathBuf>,
}

impl ConfigurationLoader {
    /// Initialize configuration loader from a config file path.
    ///
    /// # Arguments
    /// * `config_path` - Path to TOML config file. If None, uses default config.
    pub fn new(config_path: Option<&Path>) -> Result<Self> {
        let config_path = config_path
            .map(|p| p.to_path_buf())
            .unwrap_or_else(|| PathBuf::from("config/agent.toml"));

        let config = if config_path.exists() {
            Self::load_config(&config_path)?
        } else {
            Self::get_default_config()
        };

        Ok(Self {
            config_path,
            config,
            template_base: None,
            _log_base: None,
        })
    }

    /// Create a configuration loader from a pre-parsed Configuration.
    ///
    /// This avoids reading any files from disk. Use this when the caller
    /// has already loaded and merged the configuration (e.g., via figment).
    pub fn from_config(config: Configuration) -> Self {
        let agent_name = &config.agent.name;
        // Construct a synthetic config path for compatibility
        let home = crate::get_home_dir().unwrap_or_else(|_| ".".to_string());
        let config_path = PathBuf::from(home)
            .join(format!(".{}", agent_name))
            .join("config")
            .join(format!("{}.toml", agent_name));

        Self {
            config_path,
            config,
            template_base: None,
            _log_base: None,
        }
    }

    /// Load configuration from TOML file.
    fn load_config(path: &Path) -> Result<Configuration> {
        let content = fs::read_to_string(path)
            .with_context(|| format!("Failed to read config file: {}", path.display()))?;

        toml::from_str(&content)
            .with_context(|| format!("Failed to parse TOML config: {}", path.display()))
    }

    /// Get default configuration.
    pub fn get_default_config() -> Configuration {
        Configuration {
            agent: AgentConfig {
                name: "NO_AGENT_NAME".to_string(),
                version: "0.1.0".to_string(),
                default_mode: "confirm".to_string(),
                enable_task_classification: Some(false), // Default to false for backward compatibility
            },
            installation: Some(InstallationConfig {
                binary_name: "agent".to_string(),
                binary_source_path: "target/release/agent".to_string(),
                local_bin_path: "~/.local/bin".to_string(),
            }),
            // Templates removed - now handled by lifecycle WASM plugins
            logging: LoggingConfig {
                log_dir: String::new(),  // Empty string - Logger will use default /tmp/{ABK_AGENT_NAME}/
                log_level: "INFO".to_string(),
            },
            execution: ExecutionConfig {
                timeout_seconds: 120,
                max_retries: 3,
                max_tokens: 4000,
                max_history: 100,
                enable_dangerous_command_validation: true,
                max_iterations: 100,
                request_interval_seconds: 0,
                retry_base_delay_seconds: 1,
                retry_strategy: RetryStrategy::Exponential,
            },
            tools: ToolsConfig {
                open_file_window_size: Some(1000),
                max_tool_result_size_bytes: Some(256000),
                truncate_large_results: Some(true),
                enabled_tools: None,
                disabled_tools: vec![],
            },
            search_filtering: Some(SearchFilteringConfig::default()),
            llm: Some(LlmConfig::default()),
            modes: ModesConfig {
                confirm: ModeConfig {
                    description: "Agent proposes actions and asks for confirmation".to_string(),
                    auto_execute: false,
                },
                yolo: ModeConfig {
                    description: "Actions run immediately without confirmation".to_string(),
                    auto_execute: true,
                },
                human: ModeConfig {
                    description: "Human enters commands directly".to_string(),
                    auto_execute: false,
                },
            },
            mcp: None,
            lifecycle: None,
            tool_sources: vec![ToolSourceConfig::Native {
                toolset: "opencode".to_string(),
            }],
            #[cfg(feature = "checkpoint")]
            checkpointing: None,
            #[cfg(feature = "cli")]
            cli: None,
        }
    }

    /// Get configuration value by dot-notation key.
    pub fn get_string(&self, key: &str) -> Option<String> {
        match key {
            "agent.name" => Some(self.config.agent.name.clone()),
            "agent.version" => Some(self.config.agent.version.clone()),
            "agent.default_mode" => Some(self.config.agent.default_mode.clone()),
            "agent.enable_task_classification" => Some(
                self.config
                    .agent
                    .enable_task_classification
                    .unwrap_or(false)
                    .to_string(),
            ),
            // Template configuration removed - now handled by lifecycle WASM plugins
            "templates.system_template" => None,
            "templates.system_classification_template" => None,
            "templates.bug_fix_template" => None,
            "templates.fallback_template" => None,
            "templates.feature_template" => None,
            "templates.maintenance_template" => None,
            "templates.query_template" => None,
            "templates.action_observation_template" => None,
            "templates.format_error_template" => None,
            "logging.log_dir" => Some(self.config.logging.log_dir.clone()),
            "logging.log_level" => Some(self.config.logging.log_level.clone()),
            "execution.retry_strategy" => Some(
                serde_json::to_string(&self.config.execution.retry_strategy)
                    .unwrap_or_else(|_| "\"exponential\"".to_string())
                    .trim_matches('"')
                    .to_string(),
            ),
            "lifecycle.enabled" => Some(
                self.config
                    .lifecycle
                    .as_ref()
                    .map(|l| l.enabled)
                    .unwrap_or(true)
                    .to_string(),
            ),
            "lifecycle.system_template" => self
                .config
                .lifecycle
                .as_ref()
                .and_then(|l| l.system_template.clone()),
            "llm.endpoint" => Some(
                self.config
                    .llm
                    .as_ref()
                    .map(|c| c.endpoint.clone())
                    .unwrap_or_else(|| "chat/completions".to_string()),
            ),
            _ => None,
        }
    }

    /// Get numeric configuration value.
    pub fn get_u64(&self, key: &str) -> Option<u64> {
        match key {
            "execution.timeout_seconds" => Some(self.config.execution.timeout_seconds),
            "execution.max_retries" => Some(self.config.execution.max_retries as u64),
            "execution.max_tokens" => Some(self.config.execution.max_tokens as u64),
            "execution.max_history" => Some(self.config.execution.max_history as u64),
            "execution.max_iterations" => Some(self.config.execution.max_iterations as u64),
            "execution.request_interval_seconds" => {
                Some(self.config.execution.request_interval_seconds)
            }
            "execution.retry_base_delay_seconds" => {
                Some(self.config.execution.retry_base_delay_seconds)
            }
            "tools.max_tool_result_size_bytes" => self.config.tools.max_tool_result_size_bytes,
            _ => None,
        }
    }

    /// Get boolean configuration value.
    pub fn get_bool(&self, key: &str) -> Option<bool> {
        match key {
            "agent.enable_task_classification" => Some(
                self.config
                    .agent
                    .enable_task_classification
                    .unwrap_or(false),
            ),
            "tools.truncate_large_results" => self.config.tools.truncate_large_results,
            "lifecycle.enabled" => Some(
                self.config
                    .lifecycle
                    .as_ref()
                    .map(|l| l.enabled)
                    .unwrap_or(false),
            ),
            "llm.enable_streaming" => Some(
                self.config
                    .llm
                    .as_ref()
                    .map(|c| c.enable_streaming)
                    .unwrap_or(true),
            ),
            _ => None,
        }
    }

    // Template-related methods removed - templates are now handled by lifecycle WASM plugins

    /// Get LLM endpoint configuration.
    pub fn get_llm_endpoint(&self) -> String {
        self.config
            .llm
            .as_ref()
            .map(|c| c.endpoint.clone())
            .unwrap_or_else(|| "chat/completions".to_string())
    }

    /// Get LLM streaming enablement configuration.
    pub fn get_llm_streaming_enabled(&self) -> bool {
        self.config
            .llm
            .as_ref()
            .map(|c| c.enable_streaming)
            .unwrap_or(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = ConfigurationLoader::get_default_config();
        assert_eq!(config.agent.name, "NO_AGENT_NAME");
        assert_eq!(config.agent.default_mode, "confirm");
        assert_eq!(config.execution.timeout_seconds, 120);
        assert_eq!(config.execution.max_tokens, 4000);
        assert_eq!(config.execution.max_history, 100);
        assert_eq!(config.execution.request_interval_seconds, 0);
        assert!(!config.modes.confirm.auto_execute);
        assert!(config.modes.yolo.auto_execute);

        // Test LLM defaults
        assert!(config.llm.is_some());
        let llm = config.llm.unwrap();
        assert_eq!(llm.endpoint, "chat/completions");
        assert!(llm.enable_streaming);
    }

    #[test]
    fn test_get_methods() {
        let loader = ConfigurationLoader::new(None).unwrap();
        assert_eq!(
            loader.get_string("agent.name"),
            Some("NO_AGENT_NAME".to_string())
        );
        assert_eq!(loader.get_u64("execution.timeout_seconds"), Some(120));
        assert_eq!(loader.get_u64("execution.max_tokens"), Some(4000));
        assert_eq!(loader.get_u64("execution.max_history"), Some(100));
        assert_eq!(
            loader.get_u64("execution.request_interval_seconds"),
            Some(0)
        );
        assert!(loader.get_template_path("system_template").is_ok());

        // Test LLM getter methods
        assert_eq!(loader.get_llm_endpoint(), "chat/completions");
        assert!(loader.get_llm_streaming_enabled());
    }

    #[test]
    fn test_llm_config_from_toml() {
        use std::fs;
        use tempfile::NamedTempFile;

        let toml_content = r#"
[agent]
name = "test"
version = "0.1.0"
default_mode = "confirm"

[templates]
system_template = "templates/system.md"
system_classification_template = "templates/system_classification.md"
bug_fix_template = "templates/task/bug_fix.md"
fallback_template = "templates/task/fallback.md"
feature_template = "templates/task/feature.md"
maintenance_template = "templates/task/maintenance.md"
query_template = "templates/task/query.md"
action_observation_template = "templates/action_observation.md"
format_error_template = "templates/format_error.md"

[logging]
log_dir = ""
log_level = "INFO"

[execution]
timeout_seconds = 120
max_retries = 3
max_tokens = 4000
max_history = 100
enable_dangerous_command_validation = true
max_iterations = 100
request_interval_seconds = 0

[modes.confirm]
description = "Test confirm mode"
auto_execute = false

[modes.yolo]
description = "Test yolo mode"
auto_execute = true

[modes.human]
description = "Test human mode"
auto_execute = false

[tools]
open_file_window_size = 1000

[llm]
endpoint = "responses"
enable_streaming = true
"#;

        let temp_file = NamedTempFile::new().unwrap();
        fs::write(temp_file.path(), toml_content).unwrap();

        let loader = ConfigurationLoader::new(Some(temp_file.path())).unwrap();
        assert_eq!(loader.get_llm_endpoint(), "responses");
        assert!(loader.get_llm_streaming_enabled());
    }

    #[test]
    fn test_streaming_configuration_integration() {
        use std::fs;
        use tempfile::NamedTempFile;

        // Test with streaming enabled by default behavior
        let toml_default = r#"
[agent]
name = "test"
version = "0.1.0"
default_mode = "confirm"

[templates]
system_template = "templates/system.md"
system_classification_template = "templates/system_classification.md"
bug_fix_template = "templates/task/bug_fix.md"
fallback_template = "templates/task/fallback.md"
feature_template = "templates/task/feature.md"
maintenance_template = "templates/task/maintenance.md"
query_template = "templates/task/query.md"
action_observation_template = "templates/action_observation.md"
format_error_template = "templates/format_error.md"

[logging]
log_dir = ""
log_level = "INFO"

[execution]
timeout_seconds = 120
max_retries = 3
max_tokens = 4000
max_history = 100
enable_dangerous_command_validation = true
max_iterations = 100
request_interval_seconds = 0

[modes.confirm]
description = "Test confirm mode"
auto_execute = false

[modes.yolo]
description = "Test yolo mode"
auto_execute = true

[modes.human]
description = "Test human mode"
auto_execute = false

[tools]
open_file_window_size = 1000
"#;

        let temp_file_default = NamedTempFile::new().unwrap();
        fs::write(temp_file_default.path(), toml_default).unwrap();

        let loader_default = ConfigurationLoader::new(Some(temp_file_default.path())).unwrap();
        // Should use defaults when llm section is missing - streaming should be enabled by default now
        assert_eq!(loader_default.get_llm_endpoint(), "chat/completions");
        assert!(loader_default.get_llm_streaming_enabled());

        // Test with streaming enabled
        let toml_streaming = r#"
[agent]
name = "test"
version = "0.1.0"
default_mode = "confirm"

[templates]
system_template = "templates/system.md"
system_classification_template = "templates/system.md"
bug_fix_template = "templates/task/bug_fix.md"
fallback_template = "templates/task/fallback.md"
feature_template = "templates/task/feature.md"
maintenance_template = "templates/task/maintenance.md"
query_template = "templates/task/query.md"
action_observation_template = "templates/action_observation.md"
format_error_template = "templates/format_error.md"

[logging]
log_dir = ""
log_level = "INFO"

[execution]
timeout_seconds = 120
max_retries = 3
max_tokens = 4000
max_history = 100
enable_dangerous_command_validation = true
max_iterations = 100
request_interval_seconds = 0

[modes.confirm]
description = "Test confirm mode"
auto_execute = false

[modes.yolo]
description = "Test yolo mode"
auto_execute = true

[modes.human]
description = "Test human mode"
auto_execute = false

[tools]
open_file_window_size = 1000

[llm]
endpoint = "chat/completions"
enable_streaming = true
"#;

        let temp_file_streaming = NamedTempFile::new().unwrap();
        fs::write(temp_file_streaming.path(), toml_streaming).unwrap();

        let loader_streaming = ConfigurationLoader::new(Some(temp_file_streaming.path())).unwrap();
        assert_eq!(loader_streaming.get_llm_endpoint(), "chat/completions");
        assert!(loader_streaming.get_llm_streaming_enabled());
    }

    #[test]
    fn test_tools_config_from_toml() {
        use std::fs;
        use tempfile::NamedTempFile;

        let toml_content = r#"
[agent]
name = "test"
version = "0.1.0"
default_mode = "confirm"

[templates]
system_template = "templates/system.md"
system_classification_template = "templates/system.md"
bug_fix_template = "templates/task/bug_fix.md"
fallback_template = "templates/task/fallback.md"
feature_template = "templates/task/feature.md"
maintenance_template = "templates/task/maintenance.md"
query_template = "templates/task/query.md"
action_observation_template = "templates/action_observation.md"
format_error_template = "templates/format_error.md"

[logging]
log_dir = ""
log_level = "INFO"

[execution]
timeout_seconds = 120
max_retries = 3
max_tokens = 4000
max_history = 100
enable_dangerous_command_validation = true
max_iterations = 100
request_interval_seconds = 0

[modes.confirm]
description = "Test confirm mode"
auto_execute = false

[modes.yolo]
description = "Test yolo mode"
auto_execute = true

[modes.human]
description = "Test human mode"
auto_execute = false

[tools]
open_file_window_size = 1000
max_tool_result_size_bytes = 256000
truncate_large_results = true

[llm]
endpoint = "chat/completions"
enable_streaming = false
"#;

        let temp_file = NamedTempFile::new().unwrap();
        fs::write(temp_file.path(), toml_content).unwrap();

        let loader = ConfigurationLoader::new(Some(temp_file.path())).unwrap();
        assert_eq!(
            loader.get_u64("tools.max_tool_result_size_bytes"),
            Some(256000)
        );
        assert_eq!(loader.get_bool("tools.truncate_large_results"), Some(true));
    }

    #[test]
    fn test_tool_filtering_config_from_toml() {
        use std::fs;
        use tempfile::NamedTempFile;

        let toml_content = r#"
[agent]
name = "test"
version = "0.1.0"
default_mode = "confirm"

[logging]
log_dir = ""
log_level = "INFO"

[execution]
timeout_seconds = 120
max_retries = 3
max_tokens = 4000
max_history = 100
enable_dangerous_command_validation = true
max_iterations = 100
request_interval_seconds = 0

[modes.confirm]
description = "Test confirm mode"
auto_execute = false

[modes.yolo]
description = "Test yolo mode"
auto_execute = true

[modes.human]
description = "Test human mode"
auto_execute = false

[tools]
open_file_window_size = 1000
enabled_tools = ["read", "grep", "list"]
disabled_tools = ["bash"]
"#;

        let temp_file = NamedTempFile::new().unwrap();
        fs::write(temp_file.path(), toml_content).unwrap();

        let loader = ConfigurationLoader::new(Some(temp_file.path())).unwrap();

        // enabled_tools parsed correctly
        assert_eq!(
            loader.config.tools.enabled_tools,
            Some(vec![
                "read".to_string(),
                "grep".to_string(),
                "list".to_string()
            ])
        );

        // disabled_tools parsed correctly
        assert_eq!(
            loader.config.tools.disabled_tools,
            vec!["bash".to_string()]
        );
    }

    #[test]
    fn test_tool_filtering_defaults() {
        let config = ConfigurationLoader::get_default_config();
        // Default: enabled_tools = None (all tools), disabled_tools = empty
        assert!(config.tools.enabled_tools.is_none());
        assert!(config.tools.disabled_tools.is_empty());
    }

    #[test]
    fn test_tool_filtering_empty_allowlist() {
        use std::fs;
        use tempfile::NamedTempFile;

        let toml_content = r#"
[agent]
name = "test"
version = "0.1.0"
default_mode = "confirm"

[logging]
log_dir = ""
log_level = "INFO"

[execution]
timeout_seconds = 120
max_retries = 3
max_tokens = 4000
max_history = 100
enable_dangerous_command_validation = true
max_iterations = 100
request_interval_seconds = 0

[modes.confirm]
description = "Test"
auto_execute = false

[modes.yolo]
description = "Test"
auto_execute = true

[modes.human]
description = "Test"
auto_execute = false

[tools]
enabled_tools = []
"#;

        let temp_file = NamedTempFile::new().unwrap();
        fs::write(temp_file.path(), toml_content).unwrap();

        let loader = ConfigurationLoader::new(Some(temp_file.path())).unwrap();

        // Empty allowlist = zero tools (locked down)
        assert_eq!(loader.config.tools.enabled_tools, Some(vec![]));
    }
}

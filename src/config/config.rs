//! TOML configuration parsing and management.

use anyhow::{Context, Result};
use chrono::Utc;
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
    #[cfg(feature = "checkpoint")]
    pub checkpointing: Option<crate::checkpoint::GlobalCheckpointConfig>,
    #[cfg(feature = "cli")]
    pub cli: Option<crate::cli::config::CliConfig>,
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
    /// Optional authentication token (supports env var substitution)
    pub auth_token: Option<String>,
    /// Auto-initialize connection (send initialize/initialized messages)
    #[serde(default = "default_auto_init")]
    pub auto_init: bool,
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
}

/// LLM provider configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmConfig {
    pub endpoint: String,
    pub enable_streaming: bool,
}

impl Default for LlmConfig {
    fn default() -> Self {
        Self {
            endpoint: "chat/completions".to_string(),
            enable_streaming: true,
        }
    }
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
    pub log_file: String,
    pub log_level: String,
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
    template_base: Option<PathBuf>,
    _log_base: Option<PathBuf>,
}

impl ConfigurationLoader {
    /// Initialize configuration loader.
    ///
    /// # Arguments
    /// * `config_path` - Path to TOML config file. If None, uses default config.
    pub fn new(config_path: Option<&Path>) -> Result<Self> {
        Self::new_with_bases(config_path, None, None)
    }

    /// Create a configuration loader from a pre-parsed Configuration.
    ///
    /// This avoids reading any files from disk. Use this when the caller
    /// has already loaded and merged the configuration (e.g., via figment).
    pub fn from_config(config: Configuration) -> Self {
        let agent_name = &config.agent.name;
        // Construct a synthetic config path for compatibility
        let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
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

    /// Initialize configuration loader with custom base paths.
    ///
    /// # Arguments
    /// * `config_path` - Path to TOML config file. If None, uses default config.
    /// * `template_base` - Base path for templates. If None, uses paths from config.
    /// * `log_base` - Base path for logs. If None, uses path from config.
    pub fn new_with_bases(
        config_path: Option<&Path>,
        template_base: Option<&Path>,
        log_base: Option<&Path>,
    ) -> Result<Self> {
        let config_path = config_path
            .map(|p| p.to_path_buf())
            .unwrap_or_else(|| PathBuf::from("config/agent.toml"));

        let mut config = if config_path.exists() {
            Self::load_config(&config_path)?
        } else {
            Self::get_default_config()
        };

        // template_base parameter kept for backward compatibility but templates
        // are now handled by lifecycle WASM plugins
        let _ = template_base; // Suppress unused variable warning

        if let Some(log_base) = log_base {
            // Use agent name from config for log directory
            let agent_name = &config.agent.name;
            let log_dir = log_base.join(agent_name);
            fs::create_dir_all(&log_dir).with_context(|| {
                format!("Failed to create log directory: {}", log_dir.display())
            })?;
            let filename = format!(
                "{}_{}_{}.md",
                agent_name,
                Utc::now().timestamp_millis(),
                std::process::id()
            );
            config.logging.log_file = log_dir.join(filename).to_string_lossy().to_string();
        }

        Ok(Self {
            config_path,
            config,
            template_base: template_base.map(|p| p.to_path_buf()),
            _log_base: log_base.map(|p| p.to_path_buf()),
        })
    }

    /// Load configuration from TOML file.
    fn load_config(path: &Path) -> Result<Configuration> {
        let content = fs::read_to_string(path)
            .with_context(|| format!("Failed to read config file: {}", path.display()))?;

        toml::from_str(&content)
            .with_context(|| format!("Failed to parse TOML config: {}", path.display()))
    }

    /// Get default configuration.
    fn get_default_config() -> Configuration {
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
                log_file: std::env::temp_dir()
                    .join("agent")
                    .join(format!(
                        "agent_{}_{}.md",
                        Utc::now().timestamp_millis(),
                        std::process::id()
                    ))
                    .to_string_lossy()
                    .to_string(),
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
            },
            tools: ToolsConfig {
                open_file_window_size: Some(1000),
                max_tool_result_size_bytes: Some(256000),
                truncate_large_results: Some(true),
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
            "logging.log_file" => Some(self.config.logging.log_file.clone()),
            "logging.log_level" => Some(self.config.logging.log_level.clone()),
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
log_file = "/tmp/test.md"
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
log_file = "/tmp/test.md"
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
log_file = "/tmp/test.md"
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
log_file = "/tmp/test.md"
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
}

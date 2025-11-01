//! TOML configuration parsing and management.

use anyhow::{Context, Result};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

/// Main configuration structure
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Configuration {
    pub agent: AgentConfig,
    pub templates: TemplateConfig,
    pub logging: LoggingConfig,
    pub execution: ExecutionConfig,
    pub modes: ModesConfig,
    pub tools: ToolsConfig,
    pub search_filtering: Option<SearchFilteringConfig>,
    pub llm: Option<LlmConfig>,
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

/// Template configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TemplateConfig {
    pub system_template: String,
    pub system_classification_template: String,
    pub bug_fix_template: String,
    pub fallback_template: String,
    pub feature_template: String,
    pub maintenance_template: String,
    pub query_template: String,
    pub action_observation_template: String,
    pub format_error_template: String,
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
            .unwrap_or_else(|| PathBuf::from("config/simpaticoder.toml"));

        let mut config = if config_path.exists() {
            Self::load_config(&config_path)?
        } else {
            Self::get_default_config()
        };

        // Update paths if base paths are provided
        if let Some(template_base) = template_base {
            config.templates.system_template = template_base
                .join("system.md")
                .to_string_lossy()
                .to_string();
            // Use distinct classification template when template base is provided
            config.templates.system_classification_template = template_base
                .join("system_classification.md")
                .to_string_lossy()
                .to_string();
            config.templates.bug_fix_template = template_base
                .join("task/bug_fix.md")
                .to_string_lossy()
                .to_string();
            config.templates.fallback_template = template_base
                .join("task/fallback.md")
                .to_string_lossy()
                .to_string();
            config.templates.feature_template = template_base
                .join("task/feature.md")
                .to_string_lossy()
                .to_string();
            config.templates.maintenance_template = template_base
                .join("task/maintenance.md")
                .to_string_lossy()
                .to_string();
            config.templates.query_template = template_base
                .join("task/query.md")
                .to_string_lossy()
                .to_string();
            config.templates.action_observation_template = template_base
                .join("action_observation.md")
                .to_string_lossy()
                .to_string();
            config.templates.format_error_template = template_base
                .join("format_error.md")
                .to_string_lossy()
                .to_string();
        }

        if let Some(log_base) = log_base {
            let simp_dir = log_base.join("simpaticoder");
            fs::create_dir_all(&simp_dir).with_context(|| {
                format!("Failed to create log directory: {}", simp_dir.display())
            })?;
            let filename = format!(
                "simpaticoder_{}_{}.md",
                Utc::now().timestamp_millis(),
                std::process::id()
            );
            config.logging.log_file = simp_dir.join(filename).to_string_lossy().to_string();
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
                name: "simpaticoder".to_string(),
                version: "0.1.0".to_string(),
                default_mode: "confirm".to_string(),
                enable_task_classification: Some(false), // Default to false for backward compatibility
            },
            // Templates are now loaded from lifecycle WASM plugin
            // These paths are kept for backward compatibility but not used
            templates: TemplateConfig {
                system_template: "lifecycle:system".to_string(),
                system_classification_template: "lifecycle:system_classification".to_string(),
                bug_fix_template: "lifecycle:task/bug_fix".to_string(),
                fallback_template: "lifecycle:task/fallback".to_string(),
                feature_template: "lifecycle:task/feature".to_string(),
                maintenance_template: "lifecycle:task/maintenance".to_string(),
                query_template: "lifecycle:task/query".to_string(),
                action_observation_template: "lifecycle:action_observation".to_string(),
                format_error_template: "lifecycle:format_error".to_string(),
            },
            logging: LoggingConfig {
                log_file: std::env::temp_dir()
                    .join("simpaticoder")
                    .join(format!(
                        "simpaticoder_{}_{}.md",
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
            "templates.system_template" => Some(self.config.templates.system_template.clone()),
            "templates.system_classification_template" => {
                Some(self.config.templates.system_classification_template.clone())
            }
            "templates.bug_fix_template" => Some(self.config.templates.bug_fix_template.clone()),
            "templates.fallback_template" => Some(self.config.templates.fallback_template.clone()),
            "templates.feature_template" => Some(self.config.templates.feature_template.clone()),
            "templates.maintenance_template" => {
                Some(self.config.templates.maintenance_template.clone())
            }
            "templates.query_template" => Some(self.config.templates.query_template.clone()),
            "templates.action_observation_template" => {
                Some(self.config.templates.action_observation_template.clone())
            }
            "templates.format_error_template" => {
                Some(self.config.templates.format_error_template.clone())
            }
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

    /// Get path to template file.
    pub fn get_template_path(&self, template_name: &str) -> Result<PathBuf> {
        let template_path = match template_name {
            "system_template" => &self.config.templates.system_template,
            "system_classification_template" => {
                &self.config.templates.system_classification_template
            }
            "bug_fix_template" => &self.config.templates.bug_fix_template,
            "fallback_template" => &self.config.templates.fallback_template,
            "feature_template" => &self.config.templates.feature_template,
            "maintenance_template" => &self.config.templates.maintenance_template,
            "query_template" => &self.config.templates.query_template,
            "action_observation_template" => &self.config.templates.action_observation_template,
            "format_error_template" => &self.config.templates.format_error_template,
            _ => {
                return Err(anyhow::anyhow!(
                    "Template '{}' not found in configuration",
                    template_name
                ))
            }
        };

        Ok(PathBuf::from(template_path))
    }

    /// Get path to task-specific template file.
    pub fn get_task_template_path(&self, task_type: &str) -> Result<PathBuf> {
        let template_name = match task_type {
            "bug_fix" => "bug_fix_template",
            "feature" => "feature_template",
            "maintenance" => "maintenance_template",
            "query" => "query_template",
            _ => "fallback_template",
        };

        self.get_template_path(template_name)
    }

    /// Get all template paths.
    pub fn get_all_template_paths(&self) -> HashMap<String, PathBuf> {
        let mut templates = HashMap::new();
        templates.insert(
            "system_template".to_string(),
            PathBuf::from(&self.config.templates.system_template),
        );
        templates.insert(
            "system_classification_template".to_string(),
            PathBuf::from(&self.config.templates.system_classification_template),
        );
        templates.insert(
            "bug_fix_template".to_string(),
            PathBuf::from(&self.config.templates.bug_fix_template),
        );
        templates.insert(
            "fallback_template".to_string(),
            PathBuf::from(&self.config.templates.fallback_template),
        );
        templates.insert(
            "feature_template".to_string(),
            PathBuf::from(&self.config.templates.feature_template),
        );
        templates.insert(
            "maintenance_template".to_string(),
            PathBuf::from(&self.config.templates.maintenance_template),
        );
        templates.insert(
            "query_template".to_string(),
            PathBuf::from(&self.config.templates.query_template),
        );
        templates.insert(
            "action_observation_template".to_string(),
            PathBuf::from(&self.config.templates.action_observation_template),
        );
        templates.insert(
            "format_error_template".to_string(),
            PathBuf::from(&self.config.templates.format_error_template),
        );
        templates
    }

    /// Get the template base path.
    pub fn get_template_base(&self) -> Option<&Path> {
        self.template_base.as_deref()
    }

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
        assert_eq!(config.agent.name, "simpaticoder");
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
            Some("simpaticoder".to_string())
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

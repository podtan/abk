//! Configuration-driven CLI system
//!
//! This module provides a dynamic CLI builder that creates clap commands
//! from configuration, allowing different agent projects to define their
//! CLI structure through config files instead of hardcoded Rust code.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Build-time information for version display
///
/// This struct carries compile-time metadata (git SHA, build date, etc.)
/// that downstream consumers can populate via their own `build.rs` scripts.
#[derive(Debug, Clone, Default)]
pub struct BuildInfo {
    /// Short git commit SHA (e.g., "a1b2c3d")
    pub git_sha: Option<String>,
    /// Build date/time (e.g., "2026-02-05")
    pub build_date: Option<String>,
    /// Rust compiler version used for the build
    pub rustc_version: Option<String>,
    /// Build profile (e.g., "release" or "debug")
    pub build_profile: Option<String>,
}

impl BuildInfo {
    /// Create a new BuildInfo with all fields populated
    pub fn new(
        git_sha: Option<&str>,
        build_date: Option<&str>,
        rustc_version: Option<&str>,
        build_profile: Option<&str>,
    ) -> Self {
        Self {
            git_sha: git_sha.map(|s| s.to_string()),
            build_date: build_date.map(|s| s.to_string()),
            rustc_version: rustc_version.map(|s| s.to_string()),
            build_profile: build_profile.map(|s| s.to_string()),
        }
    }
}

/// Top-level CLI configuration
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct CliConfig {
    /// CLI application name
    pub name: String,
    /// CLI description/about text
    pub about: String,
    /// CLI version
    pub version: String,
    /// Default command when no subcommand provided
    #[serde(default = "default_command")]
    pub default_command: String,
    /// Which commands are enabled
    #[serde(default = "default_enabled_commands")]
    pub enabled_commands: Vec<String>,
    /// Show progress indicators
    #[serde(default = "default_true")]
    pub show_progress: bool,
    /// Interactive mode
    #[serde(default = "default_true")]
    pub interactive_mode: bool,
    /// Require confirmation for destructive operations
    #[serde(default = "default_true")]
    pub require_confirmation: bool,
    /// Command definitions
    pub commands: HashMap<String, CommandConfig>,
    /// Build-time information (not serialized, set programmatically)
    #[serde(skip)]
    pub build_info: Option<BuildInfo>,
}

fn default_command() -> String { "run".to_string() }
fn default_enabled_commands() -> Vec<String> {
    vec!["run".into(), "init".into(), "config".into(), "version".into()]
}
fn default_true() -> bool { true }

/// Configuration for a single CLI command
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct CommandConfig {
    /// Command description
    pub description: String,
    /// Command arguments
    #[serde(default)]
    pub args: Vec<ArgConfig>,
    /// Whether this command is enabled
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// Subcommands (for nested command structures)
    #[serde(default)]
    pub subcommands: Option<HashMap<String, CommandConfig>>,
}

/// Configuration for a command argument
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ArgConfig {
    /// Argument name
    pub name: String,
    /// Argument description/help text
    pub help: String,
    /// Argument type
    #[serde(default)]
    pub arg_type: ArgType,
    /// Short flag (single character)
    pub short: Option<char>,
    /// Long flag
    pub long: Option<String>,
    /// Whether argument is required
    #[serde(default)]
    pub required: bool,
    /// Default value
    pub default: Option<String>,
    /// For string args: whether to accept multiple values
    #[serde(default)]
    pub multiple: bool,
    /// For string args: whether this is a trailing argument
    #[serde(default)]
    pub trailing: bool,
    /// For choice args: allowed values
    pub choices: Option<Vec<String>>,
}

/// Supported argument types
#[derive(Debug, Clone, Deserialize, Serialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum ArgType {
    #[default]
    String,
    Path,
    Bool,
    Integer,
    Choice,
}

impl CliConfig {
    /// Load CLI config from agent configuration
    pub fn from_agent_config(config: &crate::config::Configuration) -> Self {
        // If CLI config is provided in the agent config, use it
        if let Some(cli_config) = &config.cli {
            return cli_config.clone();
        }

        // Otherwise, fall back to hardcoded defaults
        let mut cli_config = CliConfig {
            name: config.agent.name.clone(),
            about: "A terminal-first software engineering agent".to_string(),
            version: config.agent.version.clone(),
            default_command: "run".to_string(),
            enabled_commands: vec!["run".into(), "init".into(), "version".into()],
            show_progress: true,
            interactive_mode: true,
            require_confirmation: true,
            commands: HashMap::new(),
            build_info: None,
        };

        // Add basic commands - we'll expand this as we migrate
        cli_config.commands.insert("version".to_string(), CommandConfig {
            description: "Show version information".to_string(),
            args: vec![],
            enabled: true,
            subcommands: None,
        });

        cli_config.commands.insert("run".to_string(), CommandConfig {
            description: format!("Run {} with the specified task", config.agent.name),
            args: vec![
                ArgConfig {
                    name: "task".to_string(),
                    help: "Description of the task to perform".to_string(),
                    arg_type: ArgType::String,
                    short: None,
                    long: None,
                    required: true,
                    default: None,
                    multiple: true,
                    trailing: true,
                    choices: None,
                },
                ArgConfig {
                    name: "config".to_string(),
                    help: "Path to TOML config file".to_string(),
                    arg_type: ArgType::Path,
                    short: Some('c'),
                    long: Some("config".to_string()),
                    required: false,
                    default: None,
                    multiple: false,
                    trailing: false,
                    choices: None,
                },
                ArgConfig {
                    name: "yolo".to_string(),
                    help: "Enable YOLO mode (no confirmation)".to_string(),
                    arg_type: ArgType::Bool,
                    short: Some('y'),
                    long: Some("yolo".to_string()),
                    required: false,
                    default: Some("false".to_string()),
                    multiple: false,
                    trailing: false,
                    choices: None,
                },
            ],
            enabled: true,
            subcommands: None,
        });

        cli_config
    }

    /// Set build-time information for version display
    ///
    /// This method allows downstream consumers to attach compile-time 
    /// metadata (git SHA, build date, etc.) that will be shown in the 
    /// version command output.
    ///
    /// # Example
    /// ```rust,ignore
    /// let cli_config = CliConfig::from_agent_config(&config)
    ///     .with_build_info(BuildInfo::new(
    ///         option_env!("GIT_SHA"),
    ///         option_env!("BUILD_DATE"),
    ///         option_env!("RUSTC_VERSION"),
    ///         option_env!("BUILD_PROFILE"),
    ///     ));
    /// ```
    pub fn with_build_info(mut self, build_info: BuildInfo) -> Self {
        self.build_info = Some(build_info);
        self
    }

    /// Add extension commands to the CLI config when extension feature is enabled
    #[cfg(feature = "extension")]
    pub fn with_extension_commands(mut self) -> Self {
        // Add extension to enabled commands if not already present
        if !self.enabled_commands.contains(&"extension".to_string()) {
            self.enabled_commands.push("extension".to_string());
        }

        // Add extension command configuration if not already present
        if !self.commands.contains_key("extension") {
            self.commands.insert("extension".to_string(), CommandConfig {
                description: "Manage extensions".to_string(),
                args: vec![
                    ArgConfig {
                        name: "list".to_string(),
                        help: "List installed extensions".to_string(),
                        arg_type: ArgType::Bool,
                        short: Some('l'),
                        long: Some("list".to_string()),
                        required: false,
                        default: None,
                        multiple: false,
                        trailing: false,
                        choices: None,
                    },
                    ArgConfig {
                        name: "install".to_string(),
                        help: "Install extension from path".to_string(),
                        arg_type: ArgType::String,
                        short: Some('i'),
                        long: Some("install".to_string()),
                        required: false,
                        default: None,
                        multiple: false,
                        trailing: false,
                        choices: None,
                    },
                    ArgConfig {
                        name: "remove".to_string(),
                        help: "Remove extension by name".to_string(),
                        arg_type: ArgType::String,
                        short: Some('r'),
                        long: Some("remove".to_string()),
                        required: false,
                        default: None,
                        multiple: false,
                        trailing: false,
                        choices: None,
                    },
                    ArgConfig {
                        name: "info".to_string(),
                        help: "Show extension info".to_string(),
                        arg_type: ArgType::String,
                        short: None,
                        long: Some("info".to_string()),
                        required: false,
                        default: None,
                        multiple: false,
                        trailing: false,
                        choices: None,
                    },
                ],
                enabled: true,
                subcommands: None,
            });
        }

        self
    }
}
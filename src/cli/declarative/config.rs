//! Configuration schema for declarative CLI
//!
//! Defines the TOML/YAML/JSON structure for CLI specifications.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

/// Root configuration for a declarative CLI
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CliConfig {
    /// Application metadata
    pub app: AppConfig,
    
    /// Adapter configuration for auto-instantiation
    #[serde(default)]
    pub adapters: AdapterConfig,
    
    /// Global arguments (apply to all commands)
    #[serde(default)]
    pub global_args: Vec<ArgumentConfig>,
    
    /// Top-level commands
    #[serde(default)]
    pub commands: Vec<CommandConfig>,
    
    /// Schema version (for future migrations)
    #[serde(default = "default_version")]
    pub version: String,
}

fn default_version() -> String {
    "1".to_string()
}

/// Application metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    /// Application name
    pub name: String,
    
    /// Application version
    pub version: String,
    
    /// Author(s)
    #[serde(default)]
    pub author: Option<String>,
    
    /// About text / description
    #[serde(default)]
    pub about: Option<String>,
}

/// Adapter configuration for auto-instantiation
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AdapterConfig {
    /// Context adapter type (e.g., "SimpatioderContext")
    pub context: Option<String>,
    
    /// Checkpoint access adapter
    pub checkpoint: Option<String>,
    
    /// Restoration access adapter
    pub restoration: Option<String>,
    
    /// Storage access adapter  
    pub storage: Option<String>,
    
    /// Custom adapters
    #[serde(flatten)]
    pub custom: HashMap<String, String>,
}

/// Command configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommandConfig {
    /// Command name
    pub name: String,
    
    /// About text / description
    #[serde(default)]
    pub about: Option<String>,
    
    /// Command aliases
    #[serde(default)]
    pub aliases: Vec<String>,
    
    /// ABK command to route to (e.g., "sessions::list")
    pub abk_command: Option<String>,
    
    /// Special handler (e.g., "agent::run" for custom logic)
    pub special_handler: Option<String>,
    
    /// Built-in handler (echo, version, help)
    pub builtin: Option<String>,
    
    /// Executable template - shell command with placeholders (e.g., "binary {arg1} --flag={arg2}")
    /// This allows fully config-driven execution without requiring Rust code
    pub exec_template: Option<String>,
    
    /// Arguments for this command
    #[serde(default)]
    pub args: Vec<ArgumentConfig>,
    
    /// Subcommands
    #[serde(default)]
    pub subcommands: Vec<CommandConfig>,
    
    /// Whether this command is hidden
    #[serde(default)]
    pub hidden: bool,
    
    /// Deprecation message
    pub deprecated: Option<String>,
}

/// Argument configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArgumentConfig {
    /// Argument name (used in code)
    pub name: String,
    
    /// Short flag (e.g., 'v' for -v)
    pub short: Option<char>,
    
    /// Long flag (e.g., "verbose" for --verbose)
    pub long: Option<String>,
    
    /// Help text
    #[serde(default)]
    pub help: Option<String>,
    
    /// Argument type: bool, string, int, float, path, etc.
    #[serde(rename = "type")]
    pub arg_type: String,
    
    /// Whether this is a positional argument
    #[serde(default)]
    pub positional: bool,
    
    /// Whether this argument is required
    #[serde(default)]
    pub required: bool,
    
    /// Default value (as string, will be parsed)
    pub default: Option<String>,
    
    /// Environment variable to read from
    pub env: Option<String>,
    
    /// Whether this is a trailing var arg (Vec<String>)
    #[serde(default)]
    pub trailing_var_arg: bool,
    
    /// Multiple values allowed
    #[serde(default)]
    pub multiple: bool,
    
    /// Value delimiter for multiple values
    pub value_delimiter: Option<char>,
    
    /// Possible values (enum validation)
    #[serde(default)]
    pub possible_values: Vec<String>,
    
    /// Value name for help text
    pub value_name: Option<String>,
}

impl CliConfig {
    /// Load config from TOML file
    pub fn from_toml_file(path: impl AsRef<std::path::Path>) -> super::error::DeclarativeResult<Self> {
        let content = std::fs::read_to_string(path.as_ref())
            .map_err(|e| super::error::DeclarativeError::config(format!("Failed to read config file: {}", e)))?;
        let config: CliConfig = toml::from_str(&content)
            .map_err(|e| super::error::DeclarativeError::config(format!("Failed to parse TOML: {}", e)))?;
        Ok(config)
    }
    
    /// Load config from YAML file
    #[cfg(feature = "yaml")]
    pub fn from_yaml_file(path: impl AsRef<std::path::Path>) -> super::error::DeclarativeResult<Self> {
        let content = std::fs::read_to_string(path.as_ref())
            .map_err(|e| super::error::DeclarativeError::config(format!("Failed to read config file: {}", e)))?;
        let config: CliConfig = serde_yaml::from_str(&content)
            .map_err(|e| super::error::DeclarativeError::config(format!("Failed to parse YAML: {}", e)))?;
        Ok(config)
    }
    
    /// Load config from JSON file
    pub fn from_json_file(path: impl AsRef<std::path::Path>) -> super::error::DeclarativeResult<Self> {
        let content = std::fs::read_to_string(path.as_ref())
            .map_err(|e| super::error::DeclarativeError::config(format!("Failed to read config file: {}", e)))?;
        let config: CliConfig = serde_json::from_str(&content)
            .map_err(|e| super::error::DeclarativeError::config(format!("Failed to parse JSON: {}", e)))?;
        Ok(config)
    }
    
    /// Auto-detect format and load config
    pub fn from_file(path: impl AsRef<std::path::Path>) -> super::error::DeclarativeResult<Self> {
        let path = path.as_ref();
        match path.extension().and_then(|s| s.to_str()) {
            Some("toml") => Self::from_toml_file(path),
            #[cfg(feature = "yaml")]
            Some("yaml") | Some("yml") => Self::from_yaml_file(path),
            Some("json") => Self::from_json_file(path),
            _ => {
                // Try TOML first as default
                Self::from_toml_file(path)
                    .or_else(|_| {
                        #[cfg(feature = "yaml")]
                        { Self::from_yaml_file(path) }
                        #[cfg(not(feature = "yaml"))]
                        { Err(super::error::DeclarativeError::config("YAML support not enabled")) }
                    })
                    .or_else(|_| Self::from_json_file(path))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_parse_minimal_config() {
        let toml = r#"
            [app]
            name = "test"
            version = "1.0.0"
        "#;
        
        let config: CliConfig = toml::from_str(toml).unwrap();
        assert_eq!(config.app.name, "test");
        assert_eq!(config.app.version, "1.0.0");
        assert_eq!(config.version, "1");
    }
    
    #[test]
    fn test_parse_with_adapters() {
        let toml = r#"
            [app]
            name = "test"
            version = "1.0.0"
            
            [adapters]
            context = "MyContext"
            checkpoint = "MyCheckpointAccess"
        "#;
        
        let config: CliConfig = toml::from_str(toml).unwrap();
        assert_eq!(config.adapters.context, Some("MyContext".to_string()));
        assert_eq!(config.adapters.checkpoint, Some("MyCheckpointAccess".to_string()));
    }
    
    #[test]
    fn test_parse_command_with_abk_routing() {
        let toml = r#"
            [app]
            name = "test"
            version = "1.0.0"
            
            [[commands]]
            name = "sessions"
            about = "Manage sessions"
            
            [[commands.subcommands]]
            name = "list"
            abk_command = "sessions::list"
        "#;
        
        let config: CliConfig = toml::from_str(toml).unwrap();
        assert_eq!(config.commands.len(), 1);
        assert_eq!(config.commands[0].name, "sessions");
        assert_eq!(config.commands[0].subcommands[0].abk_command, Some("sessions::list".to_string()));
    }
}

//! CLI executor - the main DeclarativeCli type
//!
//! This is the entry point for the declarative CLI framework.
//! Usage: `DeclarativeCli::from_file("config.toml")?.execute().await`

use super::config::CliConfig;
use super::builder::CliBuilder;
use super::router::{CommandRouter, CommandHandler, ExecutionContext, AdapterRegistry};
use super::adapters::AdapterFactory;
use super::error::{DeclarativeError, DeclarativeResult};
use std::path::Path;

/// Main declarative CLI type
pub struct DeclarativeCli {
    config: CliConfig,
    router: CommandRouter,
    adapters: AdapterRegistry,
}

impl DeclarativeCli {
    /// Create from configuration
    pub fn new(config: CliConfig) -> DeclarativeResult<Self> {
        // Validate adapter config
        AdapterFactory::validate_config(&config.adapters)?;
        
        // Create router
        let router = CommandRouter::new();
        
        // Create adapter registry (initially empty, will be populated by app)
        let adapters = AdapterFactory::create_adapters(&config.adapters)?;
        
        Ok(Self {
            config,
            router,
            adapters,
        })
    }
    
    /// Create from TOML file
    pub fn from_toml_file(path: impl AsRef<Path>) -> DeclarativeResult<Self> {
        let config = CliConfig::from_toml_file(path)?;
        Self::new(config)
    }
    
    /// Create from YAML file
    #[cfg(feature = "yaml")]
    pub fn from_yaml_file(path: impl AsRef<Path>) -> DeclarativeResult<Self> {
        let config = CliConfig::from_yaml_file(path)?;
        Self::new(config)
    }
    
    /// Create from JSON file
    pub fn from_json_file(path: impl AsRef<Path>) -> DeclarativeResult<Self> {
        let config = CliConfig::from_json_file(path)?;
        Self::new(config)
    }
    
    /// Auto-detect format and create from file
    pub fn from_file(path: impl AsRef<Path>) -> DeclarativeResult<Self> {
        let config = CliConfig::from_file(path)?;
        Self::new(config)
    }
    
    /// Set adapter registry (for external adapter injection)
    pub fn with_adapters(mut self, adapters: AdapterRegistry) -> Self {
        self.adapters = adapters;
        self
    }
    
    /// Get a reference to the adapter registry
    pub fn adapters(&self) -> &AdapterRegistry {
        &self.adapters
    }
    
    /// Get a mutable reference to the adapter registry
    pub fn adapters_mut(&mut self) -> &mut AdapterRegistry {
        &mut self.adapters
    }
    
    /// Parse command-line arguments and return execution context
    pub fn parse(&self) -> DeclarativeResult<Option<ExecutionContext>> {
        self.parse_from(std::env::args())
    }
    
    /// Parse from custom args iterator
    pub fn parse_from<I, T>(&self, args: I) -> DeclarativeResult<Option<ExecutionContext>>
    where
        I: IntoIterator<Item = T>,
        T: Into<std::ffi::OsString> + Clone,
    {
        // Build clap CLI
        let builder = CliBuilder::new(self.config.clone());
        let app = builder.build()?;
        
        // Parse arguments
        let matches = app.try_get_matches_from(args)
            .map_err(|e| DeclarativeError::execution(format!("Failed to parse arguments: {}", e)))?;
        
        // Find which command was invoked and route to handler
        let context = self.find_command_handler(&matches, &self.config.commands)?;
        
        Ok(context)
    }
    
    /// Find the handler for the invoked command
    fn find_command_handler(
        &self,
        matches: &clap::ArgMatches,
        commands: &[super::config::CommandConfig],
    ) -> DeclarativeResult<Option<ExecutionContext>> {
        // Check if a subcommand was invoked
        if let Some((subcommand_name, sub_matches)) = matches.subcommand() {
            // Find the command config
            let cmd_config = commands
                .iter()
                .find(|c| c.name == subcommand_name || c.aliases.contains(&subcommand_name.to_string()))
                .ok_or_else(|| DeclarativeError::routing(format!("Command not found: {}", subcommand_name)))?;
            
            // If it has subcommands, recurse
            if !cmd_config.subcommands.is_empty() {
                return self.find_command_handler(sub_matches, &cmd_config.subcommands);
            }
            
            // Route to handler
            let handler = self.router.route(
                cmd_config.abk_command.as_deref(),
                cmd_config.special_handler.as_deref(),
                cmd_config.builtin.as_deref(),
                cmd_config.exec_template.as_deref(),
            )?;
            
            Ok(Some(ExecutionContext {
                matches: sub_matches.clone(),
                handler,
                adapters: self.adapters.clone(),
            }))
        } else {
            // No subcommand - could be a flag-only invocation (--version, --help)
            // or an error
            Ok(None)
        }
    }
    
    /// Execute the CLI (parse args and run handler)
    ///
    /// This is a placeholder for now. In the full implementation,
    /// this would:
    /// 1. Parse arguments
    /// 2. Route to the appropriate handler
    /// 3. Call the ABK adapter command or special handler
    /// 4. Return the result
    ///
    /// For simpaticoder, the actual execution will be done by
    /// the consuming application which knows how to call ABK commands
    /// and special handlers like Agent::run().
    pub async fn execute(&self) -> DeclarativeResult<()> {
        let context = self.parse()?;
        
        if let Some(ctx) = context {
            match ctx.handler {
                CommandHandler::AbkCommand { category, command } => {
                    return Err(DeclarativeError::execution(format!(
                        "ABK command execution not implemented in framework: {}::{}. \
                        The consuming application must implement this.",
                        category, command
                    )));
                }
                CommandHandler::SpecialHandler { handler } => {
                    return Err(DeclarativeError::execution(format!(
                        "Special handler execution not implemented in framework: {}. \
                        The consuming application must implement this.",
                        handler
                    )));
                }
                CommandHandler::Builtin { action } => {
                    self.execute_builtin(&action, &ctx.matches)?;
                }
                CommandHandler::ExecTemplate { template } => {
                    self.execute_template(&template, &ctx.matches).await?;
                }
            }
        }
        
        Ok(())
    }
    
    /// Execute a builtin command
    fn execute_builtin(&self, action: &str, matches: &clap::ArgMatches) -> DeclarativeResult<()> {
        match action {
            "echo" => {
                if let Some(message) = matches.get_one::<String>("message") {
                    println!("{}", message);
                }
                Ok(())
            }
            "version" => {
                println!("{} {}", self.config.app.name, self.config.app.version);
                Ok(())
            }
            _ => Err(DeclarativeError::execution(format!("Unknown builtin: {}", action))),
        }
    }
    
    /// Execute a shell command from template with argument substitution
    ///
    /// Templates use {arg_name} syntax for substitution. Example:
    /// - Template: "my-binary {task} --config={config}"
    /// - With task="hello" and config="/path/to/config"
    /// - Executes: my-binary hello --config=/path/to/config
    async fn execute_template(&self, template: &str, matches: &clap::ArgMatches) -> DeclarativeResult<()> {
        use std::process::Command;
        
        // Substitute all {arg_name} placeholders
        let mut command_str = template.to_string();
        
        // Get all argument IDs from matches
        for id in matches.ids() {
            let arg_name = id.as_str();
            let placeholder = format!("{{{}}}", arg_name);
            
            if let Some(value) = matches.get_one::<String>(arg_name) {
                command_str = command_str.replace(&placeholder, value);
            } else if let Some(values) = matches.get_many::<String>(arg_name) {
                // Multiple values - join with spaces
                let joined = values.map(|s| s.as_str()).collect::<Vec<_>>().join(" ");
                command_str = command_str.replace(&placeholder, &joined);
            } else if let Some(path) = matches.get_one::<std::path::PathBuf>(arg_name) {
                command_str = command_str.replace(&placeholder, path.to_str().unwrap_or(""));
            } else if matches.get_flag(arg_name) {
                // Boolean flag - replace with flag name if true, otherwise remove
                if matches.get_flag(arg_name) {
                    command_str = command_str.replace(&placeholder, &format!("--{}", arg_name));
                } else {
                    command_str = command_str.replace(&placeholder, "");
                }
            }
        }
        
        // Clean up any remaining placeholders (optional arguments that weren't provided)
        // Simple approach: remove any {...} patterns along with adjacent whitespace
        while let Some(start) = command_str.find('{') {
            if let Some(end) = command_str[start..].find('}') {
                let end = start + end + 1;
                // Remove the placeholder and normalize whitespace
                command_str.replace_range(start..end, "");
                command_str = command_str.trim().to_string();
            } else {
                break;
            }
        }
        
        // Trim and normalize whitespace
        command_str = command_str.split_whitespace().collect::<Vec<_>>().join(" ");
        
        // Parse command into program and args
        let parts: Vec<&str> = command_str.split_whitespace().collect();
        if parts.is_empty() {
            return Err(DeclarativeError::execution("Empty command after substitution".to_string()));
        }
        
        let program = parts[0];
        let args = &parts[1..];
        
        // Execute the command
        let output = Command::new(program)
            .args(args)
            .output()
            .map_err(|e| DeclarativeError::execution(format!("Failed to execute command '{}': {}", program, e)))?;
        
        // Print stdout/stderr
        if !output.stdout.is_empty() {
            print!("{}", String::from_utf8_lossy(&output.stdout));
        }
        if !output.stderr.is_empty() {
            eprint!("{}", String::from_utf8_lossy(&output.stderr));
        }
        
        // Check exit status
        if !output.status.success() {
            return Err(DeclarativeError::execution(format!(
                "Command failed with exit code: {}",
                output.status.code().unwrap_or(-1)
            )));
        }
        
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::declarative::config::{AppConfig, AdapterConfig, CommandConfig};
    
    #[test]
    fn test_create_from_config() {
        let config = CliConfig {
            app: AppConfig {
                name: "test".to_string(),
                version: "1.0.0".to_string(),
                author: None,
                about: None,
            },
            adapters: AdapterConfig::default(),
            global_args: vec![],
            commands: vec![],
            version: "1".to_string(),
        };
        
        let cli = DeclarativeCli::new(config).unwrap();
        assert_eq!(cli.config.app.name, "test");
    }
    
    #[test]
    fn test_parse_with_command() {
        let config = CliConfig {
            app: AppConfig {
                name: "test".to_string(),
                version: "1.0.0".to_string(),
                author: None,
                about: None,
            },
            adapters: AdapterConfig::default(),
            global_args: vec![],
            commands: vec![
                CommandConfig {
                    name: "echo".to_string(),
                    about: None,
                    aliases: vec![],
                    abk_command: None,
                    special_handler: None,
                    builtin: Some("echo".to_string()),
                    exec_template: None,
                    args: vec![],
                    subcommands: vec![],
                    hidden: false,
                    deprecated: None,
                }
            ],
            version: "1".to_string(),
        };
        
        let cli = DeclarativeCli::new(config).unwrap();
        let context = cli.parse_from(vec!["test", "echo"]).unwrap();
        assert!(context.is_some());
    }
}

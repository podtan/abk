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
use std::sync::Arc;
use std::collections::HashMap;

// Type alias for command handler callbacks
type CommandHandlerFn = Arc<dyn Fn(&clap::ArgMatches) -> std::pin::Pin<Box<dyn std::future::Future<Output = DeclarativeResult<()>> + Send>> + Send + Sync>;

/// Main declarative CLI type
pub struct DeclarativeCli {
    config: CliConfig,
    router: CommandRouter,
    adapters: AdapterRegistry,
    
    // Handler registry for special handlers and ABK commands
    special_handlers: HashMap<String, CommandHandlerFn>,
    abk_handlers: HashMap<String, CommandHandlerFn>,
}

impl DeclarativeCli {
    /// Create new CLI from config
    pub fn new(config: CliConfig) -> DeclarativeResult<Self> {
        let router = CommandRouter::new();
        let adapters = AdapterRegistry::new();
        
        Ok(Self {
            config,
            router,
            adapters,
            special_handlers: HashMap::new(),
            abk_handlers: HashMap::new(),
        })
    }
    
    /// Register a special handler
    pub fn register_special_handler<F, Fut>(mut self, name: impl Into<String>, handler: F) -> Self 
    where
        F: Fn(&clap::ArgMatches) -> Fut + Send + Sync + 'static,
        Fut: std::future::Future<Output = DeclarativeResult<()>> + Send + 'static,
    {
        let handler = Arc::new(move |matches: &clap::ArgMatches| {
            Box::pin(handler(matches)) as std::pin::Pin<Box<dyn std::future::Future<Output = DeclarativeResult<()>> + Send>>
        });
        self.special_handlers.insert(name.into(), handler);
        self
    }
    
    /// Register an ABK command handler
    pub fn register_abk_handler<F, Fut>(mut self, name: impl Into<String>, handler: F) -> Self 
    where
        F: Fn(&clap::ArgMatches) -> Fut + Send + Sync + 'static,
        Fut: std::future::Future<Output = DeclarativeResult<()>> + Send + 'static,
    {
        let handler = Arc::new(move |matches: &clap::ArgMatches| {
            Box::pin(handler(matches)) as std::pin::Pin<Box<dyn std::future::Future<Output = DeclarativeResult<()>> + Send>>
        });
        self.abk_handlers.insert(name.into(), handler);
        self
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
                    #[cfg(feature = "agent")]
                    {
                        // Execute ABK commands using the agent feature
                        if category == "sessions" {
                            return self.execute_sessions_command(&command, &ctx.matches).await;
                        }
                    }
                    
                    // Call registered ABK handler if available
                    let handler_name = format!("{}::{}", category, command);
                    if let Some(handler) = self.abk_handlers.get(&handler_name) {
                        return handler(&ctx.matches).await;
                    }
                    
                    return Err(DeclarativeError::execution(format!(
                        "ABK command execution not implemented: {}::{}",
                        category, command
                    )));
                }
                CommandHandler::SpecialHandler { handler } => {
                    #[cfg(feature = "agent")]
                    {
                        // Execute agent-specific special handlers
                        match handler.as_str() {
                            "agent_run" => return self.execute_agent_run(&ctx.matches).await,
                            "init" => return self.execute_init(&ctx.matches).await,
                            "resume" => return self.execute_resume(&ctx.matches).await,
                            _ => {}
                        }
                    }
                    
                    // Call registered special handler if available
                    if let Some(handler_fn) = self.special_handlers.get(handler.as_str()) {
                        return handler_fn(&ctx.matches).await;
                    }
                    
                    return Err(DeclarativeError::execution(format!(
                        "Special handler not implemented: {}",
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
        let stdout_str = String::from_utf8_lossy(&output.stdout);
        let stderr_str = String::from_utf8_lossy(&output.stderr);
        
        if !output.stderr.is_empty() {
            eprint!("{}", stderr_str);
        }
        
        // Check exit status
        if !output.status.success() {
            return Err(DeclarativeError::execution(format!(
                "Command failed with exit code: {}",
                output.status.code().unwrap_or(-1)
            )));
        }
        
        // Try to parse stdout as JSON action
        if let Ok(action) = serde_json::from_str::<serde_json::Value>(&stdout_str) {
            if let Some(action_type) = action.get("action").and_then(|v| v.as_str()) {
                // Execute the action based on type
                #[cfg(feature = "agent")]
                {
                    return self.execute_action(action_type, &action, matches).await;
                }
            }
        }
        
        // If not JSON action, just print stdout
        if !output.stdout.is_empty() {
            print!("{}", stdout_str);
        }
        
        Ok(())
    }
    
    /// Execute an action from JSON response
    #[cfg(feature = "agent")]
    async fn execute_action(
        &self,
        action_type: &str,
        action_data: &serde_json::Value,
        _matches: &clap::ArgMatches
    ) -> DeclarativeResult<()> {
        match action_type {
            "agent_start_session" => {
                // Extract data from JSON
                let task = action_data.get("task")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| DeclarativeError::execution("Missing 'task' in action".to_string()))?;
                
                // TODO: Create and run agent
                // This requires simpaticoder to provide agent factory
                println!("Would start agent session with task: {}", task);
                Ok(())
            }
            _ => {
                Err(DeclarativeError::execution(format!("Unknown action type: {}", action_type)))
            }
        }
    }
    
    /// Execute agent run command (requires agent feature)
    #[cfg(feature = "agent")]
    async fn execute_agent_run(&self, matches: &clap::ArgMatches) -> DeclarativeResult<()> {
        use crate::agent::{Agent, AgentMode};
        use std::path::PathBuf;
        
        // Get agent config
        let agent_config = self.config.agent.as_ref()
            .ok_or_else(|| DeclarativeError::config("No [agent] section in CLI config"))?;
        
        // Extract task
        let task: Vec<String> = matches
            .get_many::<String>("task")
            .map(|vals| vals.map(|s| s.clone()).collect())
            .unwrap_or_default();
        let task = task.join(" ");
        
        if task.is_empty() {
            return Err(DeclarativeError::execution("No task specified".to_string()));
        }
        
        // Get paths
        let _config_path = matches.get_one::<PathBuf>("config")
            .map(|p| p.as_path())
            .or_else(|| agent_config.config_path.as_ref().map(|s| std::path::Path::new(s)));
        let _env_path = matches.get_one::<PathBuf>("env")
            .map(|p| p.as_path())
            .or_else(|| agent_config.env_path.as_ref().map(|s| std::path::Path::new(s)));
        
        // Determine mode
        let _mode = if matches.get_flag("yolo") {
            Some(AgentMode::Yolo)
        } else if let Some(mode_str) = matches.get_one::<String>("mode") {
            Some(mode_str.parse().unwrap_or(AgentMode::Confirm))
        } else if let Some(default_mode) = &agent_config.default_mode {
            Some(default_mode.parse().unwrap_or(AgentMode::Confirm))
        } else {
            Some(AgentMode::Confirm)
        };
        
        // This requires the application to register a factory that creates the agent
        // For simpaticoder, this should call simpaticoder::agent_factory::create_agent_with_bases
        
        Err(DeclarativeError::execution(
            "Agent construction requires application integration. \
            Simpaticoder must call ABK with agent factory.".to_string()
        ))
    }
    
    /// Execute init command (requires agent feature)  
    #[cfg(feature = "agent")]
    async fn execute_init(&self, _matches: &clap::ArgMatches) -> DeclarativeResult<()> {
        Err(DeclarativeError::execution(
            "Init command not implemented in ABK framework".to_string()
        ))
    }
    
    /// Execute resume command (requires agent feature)
    #[cfg(feature = "agent")]
    async fn execute_resume(&self, _matches: &clap::ArgMatches) -> DeclarativeResult<()> {
        Err(DeclarativeError::execution(
            "Resume command not implemented in ABK framework".to_string()
        ))
    }
    
    /// Execute sessions command (requires agent feature)
    #[cfg(feature = "agent")]
    async fn execute_sessions_command(&self, subcommand: &str, _matches: &clap::ArgMatches) -> DeclarativeResult<()> {
        Err(DeclarativeError::execution(
            format!("Sessions command '{}' not implemented in ABK framework", subcommand)
        ))
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
            agent: None,
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
            agent: None,
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

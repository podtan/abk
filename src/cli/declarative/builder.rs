//! CLI builder - converts CliConfig to clap Command

use super::config::{CliConfig, CommandConfig, ArgumentConfig};
use super::error::{DeclarativeError, DeclarativeResult};
use clap::{Arg, Command, ArgAction, ValueHint};

/// Builds a clap Command from declarative configuration
pub struct CliBuilder {
    config: CliConfig,
}

impl CliBuilder {
    /// Create a new builder from configuration
    pub fn new(config: CliConfig) -> Self {
        Self { config }
    }
    
    /// Build the clap Command
    pub fn build(self) -> DeclarativeResult<Command> {
        let app_name: &'static str = Box::leak(self.config.app.name.into_boxed_str());
        let app_version: &'static str = Box::leak(self.config.app.version.into_boxed_str());
        let mut app = Command::new(app_name).version(app_version);
        
        if let Some(author) = self.config.app.author {
            let author_str: &'static str = Box::leak(author.into_boxed_str());
            app = app.author(author_str);
        }
        
        if let Some(about) = self.config.app.about {
            let about_str: &'static str = Box::leak(about.into_boxed_str());
            app = app.about(about_str);
        }
        
        // Add global arguments
        for arg_config in &self.config.global_args {
            app = app.arg(Self::build_arg(arg_config)?);
        }
        
        // Add commands
        for cmd_config in &self.config.commands {
            app = app.subcommand(Self::build_command(cmd_config)?);
        }
        
        Ok(app)
    }
    
    /// Build a command (including subcommands)
    fn build_command(config: &CommandConfig) -> DeclarativeResult<Command> {
        let cmd_name: &'static str = Box::leak(config.name.clone().into_boxed_str());
        let mut cmd = Command::new(cmd_name);
        
        if let Some(about) = &config.about {
            let about_str: &'static str = Box::leak(about.clone().into_boxed_str());
            cmd = cmd.about(about_str);
        }
        
        for alias in &config.aliases {
            let alias_str: &'static str = Box::leak(alias.clone().into_boxed_str());
            cmd = cmd.alias(alias_str);
        }
        
        if config.hidden {
            cmd = cmd.hide(true);
        }
        
        // Add deprecation note if present
        if let Some(deprecated) = &config.deprecated {
            cmd = cmd.after_help(format!("⚠️  DEPRECATED: {}", deprecated));
        }
        
        // Add arguments
        for arg_config in &config.args {
            cmd = cmd.arg(Self::build_arg(arg_config)?);
        }
        
        // Add subcommands recursively
        for sub_config in &config.subcommands {
            cmd = cmd.subcommand(Self::build_command(sub_config)?);
        }
        
        Ok(cmd)
    }
    
    /// Build a single argument
    fn build_arg(config: &ArgumentConfig) -> DeclarativeResult<Arg> {
        let arg_name: &'static str = Box::leak(config.name.clone().into_boxed_str());
        let mut arg = if config.positional {
            Arg::new(arg_name)
        } else {
            let mut a = Arg::new(arg_name);
            
            if let Some(short) = config.short {
                a = a.short(short);
            }
            
            if let Some(long) = &config.long {
                let long_str: &'static str = Box::leak(long.clone().into_boxed_str());
                a = a.long(long_str);
            }
            
            a
        };
        
        if let Some(help) = &config.help {
            let help_str: &'static str = Box::leak(help.clone().into_boxed_str());
            arg = arg.help(help_str);
        }
        
        if let Some(value_name) = &config.value_name {
            let value_name_str: &'static str = Box::leak(value_name.clone().into_boxed_str());
            arg = arg.value_name(value_name_str);
        }
        
        // Set argument action based on type
        match config.arg_type.as_str() {
            "bool" | "boolean" => {
                arg = arg.action(ArgAction::SetTrue);
            }
            "count" => {
                arg = arg.action(ArgAction::Count);
            }
            _ => {
                // For other types, use Set or Append
                if config.multiple || config.trailing_var_arg {
                    arg = arg.action(ArgAction::Append);
                } else {
                    arg = arg.action(ArgAction::Set);
                }
            }
        }
        
        // Required/optional
        if config.required {
            arg = arg.required(true);
        }
        
        // Default value
        if let Some(default) = &config.default {
            let default_str: &'static str = Box::leak(default.clone().into_boxed_str());
            arg = arg.default_value(default_str);
        }
        
        // Environment variable (not available in clap 4.x without cfg)
        // Skipping env support for now
        // if let Some(env) = &config.env {
        //     arg = arg.env(env);
        // }
        
        // Trailing var arg
        if config.trailing_var_arg {
            arg = arg.num_args(1..);
            arg = arg.trailing_var_arg(true);
        }
        
        // Multiple values
        if config.multiple {
            if let Some(delimiter) = config.value_delimiter {
                arg = arg.value_delimiter(delimiter);
            }
        }
        
        // Possible values (enum validation)
        if !config.possible_values.is_empty() {
            let static_values: Vec<&'static str> = config.possible_values
                .iter()
                .map(|s| Box::leak(s.clone().into_boxed_str()) as &'static str)
                .collect();
            arg = arg.value_parser(static_values);
        }
        
        // Value hint based on type
        arg = match config.arg_type.as_str() {
            "path" | "file" => arg.value_hint(ValueHint::FilePath),
            "dir" | "directory" => arg.value_hint(ValueHint::DirPath),
            "command" => arg.value_hint(ValueHint::CommandName),
            "url" => arg.value_hint(ValueHint::Url),
            _ => arg,
        };
        
        Ok(arg)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::declarative::config::{AppConfig, AdapterConfig};
    
    #[test]
    fn test_build_minimal_cli() {
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
        
        let builder = CliBuilder::new(config);
        let app = builder.build().unwrap();
        
        assert_eq!(app.get_name(), "test");
        assert_eq!(app.get_version(), Some("1.0.0"));
    }
    
    #[test]
    fn test_build_with_command() {
        let config = CliConfig {
            app: AppConfig {
                name: "test".to_string(),
                version: "1.0.0".to_string(),
                author: None,
                about: Some("Test CLI".to_string()),
            },
            adapters: AdapterConfig::default(),
            global_args: vec![],
            commands: vec![
                CommandConfig {
                    name: "run".to_string(),
                    about: Some("Run command".to_string()),
                    aliases: vec![],
                    abk_command: None,
                    special_handler: Some("agent::run".to_string()),
                    builtin: None,
                    args: vec![],
                    subcommands: vec![],
                    hidden: false,
                    deprecated: None,
                }
            ],
            version: "1".to_string(),
        };
        
        let builder = CliBuilder::new(config);
        let app = builder.build().unwrap();
        
        assert_eq!(app.get_subcommands().count(), 1);
        let run_cmd = app.find_subcommand("run").unwrap();
        assert_eq!(run_cmd.get_name(), "run");
    }
    
    #[test]
    fn test_build_arg_bool() {
        let arg_config = ArgumentConfig {
            name: "verbose".to_string(),
            short: Some('v'),
            long: Some("verbose".to_string()),
            help: Some("Verbose output".to_string()),
            arg_type: "bool".to_string(),
            positional: false,
            required: false,
            default: None,
            env: None,
            trailing_var_arg: false,
            multiple: false,
            value_delimiter: None,
            possible_values: vec![],
            value_name: None,
        };
        
        let arg = CliBuilder::build_arg(&arg_config).unwrap();
        assert_eq!(arg.get_id(), "verbose");
        assert_eq!(arg.get_short(), Some('v'));
        assert_eq!(arg.get_long(), Some("verbose"));
    }
    
    #[test]
    fn test_build_arg_path() {
        let arg_config = ArgumentConfig {
            name: "config".to_string(),
            short: Some('c'),
            long: Some("config".to_string()),
            help: Some("Config file".to_string()),
            arg_type: "path".to_string(),
            positional: false,
            required: true,
            default: None,
            env: Some("CONFIG_FILE".to_string()),
            trailing_var_arg: false,
            multiple: false,
            value_delimiter: None,
            possible_values: vec![],
            value_name: Some("FILE".to_string()),
        };
        
        let arg = CliBuilder::build_arg(&arg_config).unwrap();
        assert_eq!(arg.get_id(), "config");
        assert!(arg.is_required_set());
        assert_eq!(arg.get_value_names(), Some(vec!["FILE".into()].as_slice()));
    }
}

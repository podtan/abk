//! Configuration-driven CLI runner
//!
//! This module provides functionality to dynamically build and run
//! a CLI based on configuration, eliminating the need for hardcoded
//! CLI definitions in individual agent projects.

use clap::{Arg, ArgMatches, Command};
use std::path::PathBuf;

use crate::cli::config::{ArgType, CliConfig};
use crate::cli::error::{CliError, CliResult};
use crate::cli::adapters::CommandContext;

/// Build a clap Command from configuration
pub fn build_command_from_config(cmd_name: &str, cmd_config: &crate::cli::config::CommandConfig) -> Command {
    let cmd_name_static = Box::leak(cmd_name.to_string().into_boxed_str()) as &'static str;
    let description_static = Box::leak(cmd_config.description.to_string().into_boxed_str()) as &'static str;
    let mut command = Command::new(cmd_name_static)
        .about(description_static);

    for arg_config in &cmd_config.args {
        let arg_name_static = Box::leak(arg_config.name.to_string().into_boxed_str()) as &'static str;
        let arg_help_static = Box::leak(arg_config.help.to_string().into_boxed_str()) as &'static str;
        let mut arg = Arg::new(arg_name_static)
            .help(arg_help_static);

        // Set argument type and properties
        match arg_config.arg_type {
            ArgType::String => {
                arg = arg.value_parser(clap::value_parser!(String));
                if arg_config.multiple {
                    arg = arg.num_args(1..);
                }
                if arg_config.trailing {
                    arg = arg.trailing_var_arg(true);
                }
            }
            ArgType::Path => {
                arg = arg.value_parser(clap::value_parser!(PathBuf));
            }
            ArgType::Bool => {
                arg = arg.value_parser(clap::value_parser!(bool));
            }
            ArgType::Integer => {
                arg = arg.value_parser(clap::value_parser!(i64));
            }
            ArgType::Choice => {
                if let Some(choices) = &arg_config.choices {
                    let possible_values: Vec<&'static str> = choices.iter().map(|s| {
                        Box::leak(s.to_string().into_boxed_str()) as &'static str
                    }).collect();
                    arg = arg.value_parser(possible_values);
                }
            }
        }

        // Add flags
        if let Some(short) = arg_config.short {
            arg = arg.short(short);
        }
        if let Some(long) = &arg_config.long {
            let long_static = Box::leak(long.to_string().into_boxed_str()) as &'static str;
            arg = arg.long(long_static);
        }

        // Set requirements and defaults
        if arg_config.required {
            arg = arg.required(true);
        }
        if let Some(default) = &arg_config.default {
            let default_static = Box::leak(default.to_string().into_boxed_str()) as &'static str;
            arg = arg.default_value(default_static);
        }

        command = command.arg(arg);
    }

    // Add subcommands if any
    if let Some(subcommands) = &cmd_config.subcommands {
        for (sub_name, sub_config) in subcommands {
            let sub_command = build_command_from_config(sub_name, sub_config);
            command = command.subcommand(sub_command);
        }
    }

    command
}

/// Build the complete CLI application from configuration
pub fn build_cli_from_config(config: &CliConfig) -> Command {
    let name_static = Box::leak(config.name.to_string().into_boxed_str()) as &'static str;
    let about_static = Box::leak(config.about.to_string().into_boxed_str()) as &'static str;
    let version_static = Box::leak(config.version.to_string().into_boxed_str()) as &'static str;
    let mut app = Command::new(name_static)
        .about(about_static)
        .version(version_static);

    // Add enabled commands
    for cmd_name in &config.enabled_commands {
        if let Some(cmd_config) = config.commands.get(cmd_name) {
            if cmd_config.enabled {
                let command = build_command_from_config(cmd_name, cmd_config);
                app = app.subcommand(command);
            }
        }
    }

    app
}

/// Main CLI runner that parses arguments and routes to command handlers
pub async fn run_configured_cli<C: CommandContext>(
    ctx: &C,
    config: &CliConfig,
) -> CliResult<()> {
    let app = build_cli_from_config(config);
    let matches = app.get_matches();

    // Handle subcommands
    match matches.subcommand() {
        Some(("version", _)) => {
            println!("{} {}", config.name, config.version);
            Ok(())
        }
        Some(("run", sub_matches)) => {
            run_command(ctx, sub_matches).await
        }
        Some((cmd, _)) => {
            Err(CliError::UnknownCommand(cmd.to_string()))
        }
        None => {
            // No subcommand provided - use default
            match config.default_command.as_str() {
                "run" => {
                    // For run command, we need to handle the case where
                    // arguments are passed directly without "run" subcommand
                    let task_args: Vec<String> = std::env::args().skip(1).collect();
                    if task_args.is_empty() {
                        return run_config_check(ctx).await;
                    }
                    // Simulate run command with all args as task
                    run_command_with_args(ctx, &task_args.join(" ")).await
                }
                _ => Err(CliError::UnknownCommand(config.default_command.clone())),
            }
        }
    }
}

/// Handle the run command
async fn run_command<C: CommandContext>(ctx: &C, matches: &ArgMatches) -> CliResult<()> {
    let task = matches
        .get_many::<String>("task")
        .map(|vals| vals.map(|s| s.as_str()).collect::<Vec<_>>().join(" "))
        .unwrap_or_default();

    if task.is_empty() {
        return Err(CliError::ValidationError("Task description is required".to_string()));
    }

    let _config_path = matches.get_one::<PathBuf>("config").cloned();
    let yolo = matches.get_flag("yolo");

    // TODO: Implement actual run logic using adapters
    ctx.log_info(&format!("Running task: {}", task));
    if yolo {
        ctx.log_info("YOLO mode enabled - no confirmations");
    }

    Ok(())
}

/// Handle run command when called without subcommand
async fn run_command_with_args<C: CommandContext>(ctx: &C, task: &str) -> CliResult<()> {
    if task.is_empty() {
        return run_config_check(ctx).await;
    }

    ctx.log_info(&format!("Running task: {}", task));
    // TODO: Implement actual run logic
    Ok(())
}

/// Handle config check (default when no args provided)
async fn run_config_check<C: CommandContext>(ctx: &C) -> CliResult<()> {
    ctx.log_info("Running configuration check...");
    // TODO: Implement config validation
    Ok(())
}
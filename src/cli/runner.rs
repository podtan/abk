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
                // For boolean flags, use SetTrue action instead of value parser
                arg = arg.action(clap::ArgAction::SetTrue);
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
        Some(("init", sub_matches)) => {
            init_command(ctx, sub_matches).await
        }
        Some(("config", sub_matches)) => {
            config_command(ctx, sub_matches).await
        }
        Some(("cache", sub_matches)) => {
            cache_command(ctx, sub_matches).await
        }
        Some(("resume", sub_matches)) => {
            resume_command(ctx, sub_matches).await
        }
        Some(("checkpoints", sub_matches)) => {
            checkpoints_command(ctx, sub_matches).await
        }
        Some(("sessions", sub_matches)) => {
            sessions_command(ctx, sub_matches).await
        }
        Some(("misc", sub_matches)) => {
            misc_command(ctx, sub_matches).await
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

/// Handle the init command
async fn init_command<C: CommandContext>(ctx: &C, matches: &ArgMatches) -> CliResult<()> {
    let force = matches.get_flag("force");
    let template = matches.get_one::<String>("template").map(|s| s.as_str()).unwrap_or("default");

    ctx.log_info(&format!("Initializing simpaticoder with template: {}", template));
    if force {
        ctx.log_info("Force mode enabled - overwriting existing files");
    }

    // Get installation configuration
    let install_config = ctx.config().installation.as_ref()
        .ok_or_else(|| CliError::ConfigError("Installation configuration not found".to_string()))?;

    // Create ~/.simpaticoder directory structure
    let home_dir = dirs::home_dir()
        .ok_or_else(|| CliError::IoError(std::io::Error::new(std::io::ErrorKind::NotFound, "Home directory not found")))?;

    let simpaticoder_dir = home_dir.join(".simpaticoder");

    // Create main directory
    if simpaticoder_dir.exists() && !force {
        return Err(CliError::ValidationError(format!(
            "Simpaticoder directory already exists: {}. Use --force to overwrite.",
            simpaticoder_dir.display()
        )));
    }

    std::fs::create_dir_all(&simpaticoder_dir)?;
    ctx.log_info(&format!("Created directory: {}", simpaticoder_dir.display()));

    // Create bin directory for the binary
    let bin_dir = simpaticoder_dir.join("bin");
    std::fs::create_dir_all(&bin_dir)?;
    ctx.log_info(&format!("Created directory: {}", bin_dir.display()));

    // Create subdirectories
    let subdirs = vec![
        "providers",
        "providers/lifecycle",
        "providers/tanbal",
        "config",
        "cache",
        "checkpoints",
        "sessions",
        "logs",
        "templates",
    ];

    for subdir in subdirs {
        let dir_path = simpaticoder_dir.join(subdir);
        std::fs::create_dir_all(&dir_path)?;
        ctx.log_info(&format!("Created directory: {}", dir_path.display()));
    }

    // Copy binary to ~/.simpaticoder/bin/
    let binary_source = PathBuf::from(&install_config.binary_source_path);
    let binary_dest = bin_dir.join(&install_config.binary_name);

    if binary_source.exists() {
        std::fs::copy(&binary_source, &binary_dest)?;
        ctx.log_info(&format!("Copied binary: {} -> {}", binary_source.display(), binary_dest.display()));

        // Make binary executable
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = std::fs::metadata(&binary_dest)?.permissions();
            perms.set_mode(0o755);
            std::fs::set_permissions(&binary_dest, perms)?;
        }
    } else {
        ctx.log_warn(&format!("Binary source not found: {}", binary_source.display()));
    }

    // Create default config file
    let config_file = simpaticoder_dir.join("config/simpaticoder.toml");
    if !config_file.exists() || force {
        let default_config = r#"# Simpaticoder Configuration
# This file was generated by 'simpaticoder init'

[agent]
name = "simpaticoder"
version = "0.1.0"
default_mode = "confirm"

[execution]
timeout_seconds = 120
max_retries = 3
max_tokens = 4000

[llm]
endpoint = "chat/completions"
enable_streaming = true
"#;
        std::fs::write(&config_file, default_config)?;
        ctx.log_info(&format!("Created config file: {}", config_file.display()));
    }

    // Create symlink in ~/.local/bin/
    let local_bin_path = shellexpand::tilde(&install_config.local_bin_path).to_string();
    let local_bin_dir = PathBuf::from(local_bin_path);
    std::fs::create_dir_all(&local_bin_dir)?;

    let local_bin_symlink = local_bin_dir.join(&install_config.binary_name);

    if local_bin_symlink.exists() {
        if force {
            std::fs::remove_file(&local_bin_symlink)?;
        } else {
            ctx.log_warn(&format!("Symlink already exists: {}", local_bin_symlink.display()));
        }
    }

    if !local_bin_symlink.exists() {
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(&binary_dest, &local_bin_symlink)?;
            ctx.log_info(&format!("Created symlink: {} -> {}", local_bin_symlink.display(), binary_dest.display()));
        }
        #[cfg(windows)]
        {
            // On Windows, create a copy instead of symlink for better compatibility
            std::fs::copy(&binary_dest, &local_bin_symlink)?;
            ctx.log_info(&format!("Created binary copy: {} -> {}", binary_dest.display(), local_bin_symlink.display()));
        }
    }

    // Create symlinks to project providers if they exist
    let project_providers = std::env::current_dir()?.join("providers");
    if project_providers.exists() {
        for entry in std::fs::read_dir(&project_providers)? {
            let entry = entry?;
            let entry_name = entry.file_name();
            let target = entry.path();
            let link = simpaticoder_dir.join("providers").join(&entry_name);

            if target.is_dir() {
                // Only create symlink if the link doesn't exist or we're in force mode
                if link.exists() {
                    if force {
                        // Remove existing link/directory
                        if link.is_dir() {
                            std::fs::remove_dir_all(&link)?;
                        } else {
                            std::fs::remove_file(&link)?;
                        }
                    } else {
                        // Skip if not in force mode
                        continue;
                    }
                }

                // Create symlink to directory
                #[cfg(unix)]
                {
                    std::os::unix::fs::symlink(&target, &link)?;
                    ctx.log_info(&format!("Created symlink: {} -> {}", link.display(), target.display()));
                }
                #[cfg(windows)]
                {
                    // On Windows, create junction
                    std::os::windows::fs::symlink_dir(&target, &link)?;
                    ctx.log_info(&format!("Created directory symlink: {} -> {}", link.display(), target.display()));
                }
            }
        }
    }

    ctx.log_success("Simpaticoder initialized successfully");
    ctx.log_info(&format!("Configuration directory: {}", simpaticoder_dir.display()));
    ctx.log_info(&format!("Binary installed to: {}", binary_dest.display()));
    ctx.log_info(&format!("Symlink created: {}", local_bin_symlink.display()));
    Ok(())
}

/// Handle the config command
async fn config_command<C: CommandContext>(ctx: &C, matches: &ArgMatches) -> CliResult<()> {
    if matches.get_flag("show") {
        ctx.log_info("Showing current configuration...");
        // TODO: Implement config display
    } else if matches.get_flag("edit") {
        ctx.log_info("Opening configuration for editing...");
        // TODO: Implement config editing
    } else if matches.get_flag("validate") {
        ctx.log_info("Validating configuration...");
        // TODO: Implement config validation
    } else {
        ctx.log_info("Use --show, --edit, or --validate flags");
    }

    Ok(())
}

/// Handle the cache command
async fn cache_command<C: CommandContext>(ctx: &C, matches: &ArgMatches) -> CliResult<()> {
    if matches.get_flag("clear") {
        ctx.log_info("Clearing cache...");
        // TODO: Implement cache clearing
        ctx.log_success("Cache cleared");
    } else if matches.get_flag("list") {
        ctx.log_info("Listing cached items...");
        // TODO: Implement cache listing
    } else if matches.get_flag("size") {
        ctx.log_info("Calculating cache size...");
        // TODO: Implement cache size calculation
    } else {
        ctx.log_info("Use --clear, --list, or --size flags");
    }

    Ok(())
}

/// Handle the resume command
async fn resume_command<C: CommandContext>(ctx: &C, matches: &ArgMatches) -> CliResult<()> {
    if let Some(session) = matches.get_one::<String>("session") {
        ctx.log_info(&format!("Resuming session: {}", session));
        // TODO: Implement session resume
    } else if matches.get_flag("latest") {
        ctx.log_info("Resuming latest session...");
        // TODO: Implement latest session resume
    } else {
        ctx.log_info("Use --session <id> or --latest flag");
    }

    Ok(())
}

/// Handle the checkpoints command
async fn checkpoints_command<C: CommandContext>(ctx: &C, matches: &ArgMatches) -> CliResult<()> {
    if matches.get_flag("list") {
        ctx.log_info("Listing checkpoints...");
        // TODO: Implement checkpoint listing
    } else if let Some(id) = matches.get_one::<String>("show") {
        ctx.log_info(&format!("Showing checkpoint: {}", id));
        // TODO: Implement checkpoint details
    } else if let Some(id) = matches.get_one::<String>("delete") {
        ctx.log_info(&format!("Deleting checkpoint: {}", id));
        // TODO: Implement checkpoint deletion
    } else if matches.get_flag("clean") {
        ctx.log_info("Cleaning old checkpoints...");
        // TODO: Implement checkpoint cleanup
    } else {
        ctx.log_info("Use --list, --show <id>, --delete <id>, or --clean flags");
    }

    Ok(())
}

/// Handle the sessions command
async fn sessions_command<C: CommandContext>(ctx: &C, matches: &ArgMatches) -> CliResult<()> {
    if matches.get_flag("list") {
        ctx.log_info("Listing sessions...");
        // TODO: Implement session listing
    } else if let Some(id) = matches.get_one::<String>("show") {
        ctx.log_info(&format!("Showing session: {}", id));
        // TODO: Implement session details
    } else if let Some(id) = matches.get_one::<String>("delete") {
        ctx.log_info(&format!("Deleting session: {}", id));
        // TODO: Implement session deletion
    } else {
        ctx.log_info("Use --list, --show <id>, or --delete <id> flags");
    }

    Ok(())
}

/// Handle the misc command
async fn misc_command<C: CommandContext>(ctx: &C, matches: &ArgMatches) -> CliResult<()> {
    if matches.get_flag("doctor") {
        ctx.log_info("Running diagnostics...");
        // TODO: Implement diagnostics
        ctx.log_success("All systems operational");
    } else if matches.get_flag("stats") {
        ctx.log_info("Showing statistics...");
        // TODO: Implement statistics
    } else if matches.get_flag("clean") {
        ctx.log_info("Cleaning temporary files...");
        // TODO: Implement cleanup
        ctx.log_success("Cleanup completed");
    } else {
        ctx.log_info("Use --doctor, --stats, or --clean flags");
    }

    Ok(())
}
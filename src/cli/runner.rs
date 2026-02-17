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
use crate::cli::adapters::checkpoint::{
    CheckpointAccess, RestorationAccess, ProjectMetadata, SessionMetadata, SessionStatus,
    CheckpointMetadata, CheckpointData, CheckpointDiff, RestoredCheckpoint, AgentResult,
    ResumeContext,
};
use crate::cli::adapters::storage::{StorageAccess, AbkStorageAccess};
use async_trait::async_trait;

/// Run CLI with externally-provided configuration
///
/// This function accepts raw configuration data instead of reading files.
/// The caller is responsible for loading config and secrets from any source
/// (files, S3, etc.). Secrets are injected into the process environment
/// for WASM provider compatibility.
///
/// # Arguments
/// * `config_toml` - Raw TOML configuration string
/// * `secrets` - Key-value pairs to inject into environment (e.g., API keys)
/// * `build_info` - Optional build-time metadata for version display
///
/// # Environment Variable Override
/// Secrets from the HashMap are injected into `std::env`, but existing
/// environment variables take precedence (are NOT overwritten).
///
/// # Example
///
/// ```rust,ignore
/// use std::collections::HashMap;
///
/// #[tokio::main]
/// async fn main() -> Result<(), Box<dyn std::error::Error>> {
///     // Load config from file/S3/wherever
///     let config_toml = std::fs::read_to_string("config.toml")?;
///     
///     // Load secrets from file/S3/wherever
///     let mut secrets = HashMap::new();
///     secrets.insert("OPENAI_API_KEY".to_string(), "sk-...".to_string());
///     
///     abk::cli::run_from_raw_config(&config_toml, secrets, None).await
/// }
/// ```
pub async fn run_from_raw_config(
    config_toml: &str,
    secrets: std::collections::HashMap<String, String>,
    build_info: Option<crate::cli::config::BuildInfo>,
) -> Result<(), Box<dyn std::error::Error>> {
    // Inject secrets into environment (existing env vars take precedence)
    for (key, value) in &secrets {
        if std::env::var(key).is_err() {
            std::env::set_var(key, value);
        }
    }
    
    // Parse TOML configuration
    let config: crate::config::Configuration = toml::from_str(config_toml)
        .map_err(|e| format!("Failed to parse config TOML: {}", e))?;
    
    // Set ABK_AGENT_NAME for other subsystems
    std::env::set_var("ABK_AGENT_NAME", &config.agent.name);
    
    // Create context from parsed config
    let context = RawConfigCommandContext::new(config)?;
    
    // Convert config to CLI config
    let mut cli_config = CliConfig::from_agent_config(&context.config);
    
    // Attach build info if provided
    if let Some(info) = build_info {
        cli_config = cli_config.with_build_info(info);
    }
    
    // Add extension commands if feature is enabled
    #[cfg(feature = "extension")]
    {
        cli_config = cli_config.with_extension_commands();
    }
    
    // Run the CLI
    run_configured_cli(&context, &cli_config).await?;
    
    Ok(())
}

/// Command context that uses pre-parsed configuration (no file reading)
pub struct RawConfigCommandContext {
    config: crate::config::Configuration,
    logger: crate::observability::Logger,
    working_dir: std::path::PathBuf,
}

impl RawConfigCommandContext {
    /// Create a new context from already-parsed configuration
    pub fn new(config: crate::config::Configuration) -> Result<Self, Box<dyn std::error::Error>> {
        let agent_name = &config.agent.name;
        
        let log_file_name = format!("{}.log", agent_name);
        let logger = crate::observability::Logger::new(None, None)
            .unwrap_or_else(|_| {
                crate::observability::Logger::new(Some(std::path::Path::new(&log_file_name)), Some("INFO"))
                    .expect("Failed to create fallback logger")
            });

        let working_dir = std::env::current_dir()
            .unwrap_or_else(|_| std::path::PathBuf::from("."));

        Ok(Self {
            config,
            logger,
            working_dir,
        })
    }
}

#[async_trait::async_trait]
impl CommandContext for RawConfigCommandContext {
    fn config_path(&self) -> CliResult<std::path::PathBuf> {
        // No file path - config was provided directly
        // Return a synthetic path based on agent name for compatibility
        let home = std::env::var("HOME")
            .map_err(|_| CliError::ConfigError("Could not determine home directory".to_string()))?;
        let agent_name = &self.config.agent.name;
        Ok(std::path::PathBuf::from(home)
            .join(format!(".{}", agent_name))
            .join("config")
            .join(format!("{}.toml", agent_name)))
    }

    fn config(&self) -> &crate::config::Configuration {
        &self.config
    }

    fn load_config(&self) -> CliResult<serde_json::Value> {
        Ok(serde_json::json!({
            "name": self.config.agent.name,
            "version": self.config.agent.version
        }))
    }

    fn working_dir(&self) -> CliResult<std::path::PathBuf> {
        Ok(self.working_dir.clone())
    }

    fn project_hash(&self) -> CliResult<String> {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        let mut hasher = DefaultHasher::new();
        self.working_dir.hash(&mut hasher);
        Ok(format!("{:x}", hasher.finish()))
    }

    fn data_dir(&self) -> CliResult<std::path::PathBuf> {
        let home = std::env::var("HOME")
            .map_err(|_| CliError::ConfigError("Could not determine home directory".to_string()))?;
        let agent_name = &self.config.agent.name;
        Ok(std::path::PathBuf::from(home).join(format!(".{}", agent_name)))
    }

    fn cache_dir(&self) -> CliResult<std::path::PathBuf> {
        self.data_dir()
    }

    fn log_info(&self, message: &str) {
        println!("{}", message);
    }

    fn log_warn(&self, message: &str) {
        eprintln!("Warning: {}", message);
    }

    fn log_error(&self, message: &str) -> CliResult<()> {
        eprintln!("Error: {}", message);
        Ok(())
    }

    fn log_success(&self, message: &str) {
        println!("✓ {}", message);
    }

    async fn create_agent(&self) -> Result<crate::agent::Agent, Box<dyn std::error::Error + Send + Sync>> {
        // Use pre-parsed config directly — no file I/O needed
        Ok(crate::agent::Agent::new_from_config(self.config.clone(), None).await?)
    }
}

/// Concrete implementation of CheckpointAccess using abk::checkpoint
struct AbkCheckpointAccess {
    /// Pre-parsed checkpoint config to avoid re-reading files from disk
    checkpoint_config: Option<crate::checkpoint::GlobalCheckpointConfig>,
    /// Full config for checking storage mode
    storage_mode: Option<crate::checkpoint::StorageMode>,
}

impl AbkCheckpointAccess {
    fn new() -> Self {
        Self { checkpoint_config: None, storage_mode: None }
    }

    /// Create with a pre-parsed Configuration to avoid file I/O
    fn with_config(config: &crate::config::Configuration) -> Self {
        #[cfg(feature = "checkpoint")]
        {
            let checkpoint_config = config.checkpointing.clone();
            let storage_mode = checkpoint_config.as_ref()
                .map(|c| c.storage_backend.effective_storage_mode());
            Self { checkpoint_config, storage_mode }
        }
        #[cfg(not(feature = "checkpoint"))]
        {
            Self { checkpoint_config: None, storage_mode: None }
        }
    }
    
    /// Get storage manager with remote backend configuration if available.
    #[cfg(feature = "storage-documentdb")]
    async fn get_configured_storage_manager(&self) -> CliResult<crate::checkpoint::CheckpointStorageManager> {
        if let Some(ref checkpoint_config) = self.checkpoint_config {
            match crate::checkpoint::CheckpointStorageManager::with_config_async(checkpoint_config.clone()).await {
                Ok(m) => return Ok(m),
                Err(e) => {
                    eprintln!("Warning: Failed to create configured storage manager: {}", e);
                }
            }
        }
        
        // Fall back to default storage manager
        crate::checkpoint::get_storage_manager()
            .map_err(|e| CliError::CheckpointError(format!("Failed to get storage manager: {}", e)))
    }
    
    #[cfg(not(feature = "storage-documentdb"))]
    async fn get_configured_storage_manager(&self) -> CliResult<crate::checkpoint::CheckpointStorageManager> {
        crate::checkpoint::get_storage_manager()
            .map_err(|e| CliError::CheckpointError(format!("Failed to get storage manager: {}", e)))
    }
}

#[async_trait]
impl CheckpointAccess for AbkCheckpointAccess {
    async fn list_projects(&self) -> CliResult<Vec<ProjectMetadata>> {
        let manager = self.get_configured_storage_manager().await?;
        
        let projects = manager.list_projects().await
            .map_err(|e| CliError::CheckpointError(format!("Failed to list projects: {}", e)))?;
        
        Ok(projects.into_iter().map(|p| ProjectMetadata {
            name: p.name,
            project_path: p.project_path,
            project_hash: p.project_hash,
        }).collect())
    }

    async fn list_sessions(&self, project_path: &PathBuf) -> CliResult<Vec<SessionMetadata>> {
        // For remote storage, we don't require the project path to exist locally
        #[cfg(feature = "storage-documentdb")]
        let check_path = {
            if let Some(ref storage_mode) = self.storage_mode {
                !matches!(storage_mode, crate::checkpoint::StorageMode::Remote)
            } else {
                true
            }
        };
        
        #[cfg(not(feature = "storage-documentdb"))]
        let check_path = true;
        
        if check_path && !project_path.exists() {
            return Ok(vec![]);
        }
        
        let manager = self.get_configured_storage_manager().await?;
        
        let project_storage = manager.get_project_storage(project_path).await
            .map_err(|e| CliError::CheckpointError(format!("Failed to get project storage: {}", e)))?;
        
        let sessions = project_storage.list_sessions().await
            .map_err(|e| CliError::CheckpointError(format!("Failed to list sessions: {}", e)))?;
        
        Ok(sessions.into_iter().map(|s| SessionMetadata {
            session_id: s.session_id,
            status: match s.status {
                crate::checkpoint::SessionStatus::Active => SessionStatus::Active,
                crate::checkpoint::SessionStatus::Completed => SessionStatus::Completed,
                crate::checkpoint::SessionStatus::Failed => SessionStatus::Failed,
                crate::checkpoint::SessionStatus::Archived => SessionStatus::Archived,
            },
            created_at: s.created_at,
            last_accessed: s.last_accessed,
            description: s.description,
            tags: s.tags,
            checkpoint_count: s.checkpoint_count as usize,
        }).collect())
    }

    async fn list_checkpoints(&self, project_path: &PathBuf, session_id: &str) -> CliResult<Vec<CheckpointMetadata>> {
        // For remote storage, we don't require the project path to exist locally
        #[cfg(not(feature = "storage-documentdb"))]
        if !project_path.exists() {
            return Ok(vec![]);
        }
        
        let manager = self.get_configured_storage_manager().await?;

        let project_storage = manager.get_project_storage(project_path).await
            .map_err(|e| CliError::CheckpointError(format!("Failed to get project storage: {}", e)))?;

        let session_storage = project_storage.create_session(session_id).await
            .map_err(|e| CliError::CheckpointError(format!("Failed to get session storage: {}", e)))?;

        let checkpoints = session_storage.list_checkpoints().await
            .map_err(|e| CliError::CheckpointError(format!("Failed to list checkpoints: {}", e)))?;

        Ok(checkpoints.into_iter().map(|c| CheckpointMetadata {
            checkpoint_id: c.checkpoint_id,
            session_id: c.session_id,
            workflow_step: c.workflow_step.to_string(),
            created_at: c.created_at,
            iteration: c.iteration as usize,
            description: c.description,
            tags: c.tags,
        }).collect())
    }

    async fn delete_session(&self, project_path: &PathBuf, session_id: &str) -> CliResult<()> {
        // For remote storage, we don't require the project path to exist locally
        #[cfg(not(feature = "storage-documentdb"))]
        if !project_path.exists() {
            return Ok(());
        }
        
        let manager = self.get_configured_storage_manager().await?;
        
        let project_storage = manager.get_project_storage(project_path).await
            .map_err(|e| CliError::CheckpointError(format!("Failed to get project storage: {}", e)))?;
        
        project_storage.delete_session(session_id).await
            .map_err(|e| CliError::CheckpointError(format!("Failed to delete session: {}", e)))?;
        
        Ok(())
    }

    async fn validate_session(&self, _project_path: &PathBuf, _session_id: &str, _repair: bool) -> CliResult<Vec<String>> {
        // TODO: Implement session validation
        Ok(vec![])
    }

    async fn load_checkpoint(&self, _project_path: &PathBuf, session_id: &str, checkpoint_id: &str) -> CliResult<CheckpointData> {
        // TODO: Implement proper checkpoint loading from storage
        // For now, return dummy checkpoint data so resume can work
        use chrono::Utc;
        use std::env;

        let current_dir = env::current_dir()
            .map_err(|e| CliError::IoError(e))?;

        Ok(CheckpointData {
            metadata: CheckpointMetadata {
                checkpoint_id: checkpoint_id.to_string(),
                session_id: session_id.to_string(),
                workflow_step: "task_execution".to_string(),
                created_at: Utc::now(),
                iteration: 1,
                description: Some(format!("Checkpoint {} for session {}", checkpoint_id, session_id)),
                tags: vec!["resume".to_string()],
            },
            agent_state: crate::cli::adapters::checkpoint::AgentStateData {
                current_mode: "confirm".to_string(),
                current_step: "task_execution".to_string(),
                working_directory: current_dir.clone(),
                task_description: Some("Resumed task".to_string()),
            },
            conversation_state: crate::cli::adapters::checkpoint::ConversationStateData {
                message_count: 5,
                total_tokens: 1500,
            },
            file_system_state: crate::cli::adapters::checkpoint::FileSystemStateData {
                working_directory: current_dir,
                modified_files: vec!["src/main.rs".to_string()],
            },
            tool_state: crate::cli::adapters::checkpoint::ToolStateData {
                executed_commands_count: 3,
            },
        })
    }

    async fn delete_checkpoint(&self, _project_path: &PathBuf, _session_id: &str, _checkpoint_id: &str) -> CliResult<()> {
        // TODO: Implement checkpoint deletion
        Err(CliError::CheckpointError("Checkpoint deletion not implemented".to_string()))
    }

    async fn get_checkpoint_diff(&self, _project_path: &PathBuf, _session_id: &str, _from_checkpoint_id: &str, _to_checkpoint_id: &str) -> CliResult<CheckpointDiff> {
        // TODO: Implement checkpoint diff
        Err(CliError::CheckpointError("Checkpoint diff not implemented".to_string()))
    }
}

/// Concrete implementation of RestorationAccess using abk::checkpoint
struct AbkRestorationAccess {
    checkpoint_config: Option<crate::checkpoint::GlobalCheckpointConfig>,
    storage_mode: Option<crate::checkpoint::StorageMode>,
}

impl AbkRestorationAccess {
    fn new() -> Self {
        Self { checkpoint_config: None, storage_mode: None }
    }

    fn with_config(config: &crate::config::Configuration) -> Self {
        #[cfg(feature = "checkpoint")]
        {
            let checkpoint_config = config.checkpointing.clone();
            let storage_mode = checkpoint_config.as_ref()
                .map(|c| c.storage_backend.effective_storage_mode());
            Self { checkpoint_config, storage_mode }
        }
        #[cfg(not(feature = "checkpoint"))]
        {
            Self { checkpoint_config: None, storage_mode: None }
        }
    }
}

#[async_trait]
impl RestorationAccess for AbkRestorationAccess {
    async fn restore_checkpoint(&self, project_path: &PathBuf, session_id: &str, checkpoint_id: &str) -> CliResult<RestoredCheckpoint> {
        // Load the checkpoint data first
        let checkpoint_access = AbkCheckpointAccess {
            checkpoint_config: self.checkpoint_config.clone(),
            storage_mode: self.storage_mode,
        };
        let checkpoint = checkpoint_access.load_checkpoint(project_path, session_id, checkpoint_id).await?;

        // TODO: Implement actual restoration logic
        // For now, create a successful restoration result
        use chrono::Utc;

        let restoration_start = Utc::now();
        // Simulate some restoration time
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

        Ok(RestoredCheckpoint {
            checkpoint,
            restoration_metadata: crate::cli::adapters::checkpoint::RestorationMetadata {
                restored_at: Utc::now(),
                restore_duration_ms: (Utc::now() - restoration_start).num_milliseconds() as u64,
                warnings_count: 0,
                warnings: vec![],
            },
        })
    }

    async fn restore_agent(&self, _project_path: &PathBuf, _session_id: &str, _checkpoint_id: &str) -> CliResult<AgentResult> {
        // TODO: Implement actual agent restoration logic
        // For now, return a successful restoration result
        Ok(AgentResult {
            success: true,
            warnings: vec![],
            errors: vec![],
        })
    }

    async fn store_resume_context(&self, context: &ResumeContext) -> CliResult<()> {
        let tracker = crate::checkpoint::ResumeTracker::new()
            .map_err(|e| CliError::CheckpointError(format!("Failed to create resume tracker: {}", e)))?;
        
        // Convert CLI ResumeContext to resume_tracker ResumeContext
        let tracker_context = crate::checkpoint::resume_tracker::ResumeContext {
            project_path: context.project_path.clone(),
            session_id: context.session_id.clone(),
            checkpoint_id: context.checkpoint_id.clone(),
            restored_at: context.restored_at,
            working_directory: context.working_directory.clone(),
            task_description: context.task_description.clone().unwrap_or_else(|| "Unknown task".to_string()),
            workflow_step: context.workflow_step.clone(),
            iteration: context.iteration as u32,
        };
        
        tracker.store_resume_context(&tracker_context)
            .map_err(|e| CliError::CheckpointError(format!("Failed to store resume context: {}", e)))?;
        
        Ok(())
    }
}

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
            if let Some(ref build_info) = config.build_info {
                if let Some(ref sha) = build_info.git_sha {
                    println!("commit: {}", sha);
                }
                if let Some(ref date) = build_info.build_date {
                    println!("built:  {}", date);
                }
                if let Some(ref rustc) = build_info.rustc_version {
                    println!("rustc:  {}", rustc);
                }
                if let Some(ref profile) = build_info.build_profile {
                    println!("profile: {}", profile);
                }
            }
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
        #[cfg(feature = "extension")]
        Some(("extension", sub_matches)) => {
            extension_command(ctx, sub_matches).await
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

    let yolo = matches.get_flag("yolo");
    let mode = matches.get_one::<String>("mode").cloned();
    let verbose = matches.get_flag("verbose");

    let options = crate::cli::commands::run::RunOptions {
        task,
        yolo,
        mode,
        run_mode: None, // Not configured in CLI, will use defaults
        verbose,
    };

    crate::cli::commands::run::execute_run(ctx, options).await
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

/// Recursively copy a directory and all its contents
fn copy_dir_recursive(src: &std::path::Path, dst: &std::path::Path) -> CliResult<()> {
    std::fs::create_dir_all(dst)?;
    
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());
        
        if src_path.is_dir() {
            copy_dir_recursive(&src_path, &dst_path)?;
        } else {
            std::fs::copy(&src_path, &dst_path)?;
        }
    }
    
    Ok(())
}

/// Handle the init command
async fn init_command<C: CommandContext>(ctx: &C, matches: &ArgMatches) -> CliResult<()> {
    let force = matches.get_flag("force");
    let template = matches.get_one::<String>("template").map(|s| s.as_str()).unwrap_or("default");

    let agent_name = &ctx.config().agent.name;
    ctx.log_info(&format!("Initializing {} with template: {}", agent_name, template));
    if force {
        ctx.log_info("Force mode enabled - overwriting existing files");
    }

    // Get installation configuration
    let install_config = ctx.config().installation.as_ref()
        .ok_or_else(|| CliError::ConfigError("Installation configuration not found".to_string()))?;

    // Create ~/.{agent_name} directory structure
    let home_dir = dirs::home_dir()
        .ok_or_else(|| CliError::IoError(std::io::Error::new(std::io::ErrorKind::NotFound, "Home directory not found")))?;

    let agent_dir = home_dir.join(format!(".{}", agent_name));

    // Create main directory
    if agent_dir.exists() && !force {
        return Err(CliError::ValidationError(format!(
            "{} directory already exists: {}. Use --force to overwrite.",
            agent_name.chars().next().unwrap().to_uppercase().collect::<String>() + &agent_name[1..],
            agent_dir.display()
        )));
    }

    std::fs::create_dir_all(&agent_dir)?;
    ctx.log_info(&format!("Created directory: {}", agent_dir.display()));

    // Create bin directory for the binary
    let bin_dir = agent_dir.join("bin");
    std::fs::create_dir_all(&bin_dir)?;
    ctx.log_info(&format!("Created directory: {}", bin_dir.display()));

    // Create subdirectories
    let subdirs = vec![
        "providers",
        "providers/lifecycle",
        "providers/tanbal",
        "extensions",
        "config",
        "cache",
        "checkpoints",
        "sessions",
        "logs",
        "templates",
        "projects",  // Required by checkpoint system
        "temp",      // Required by checkpoint system
    ];

    for subdir in subdirs {
        let dir_path = agent_dir.join(subdir);
        std::fs::create_dir_all(&dir_path)?;
        ctx.log_info(&format!("Created directory: {}", dir_path.display()));
    }

    // Copy binary to ~/.{agent_name}/bin/
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

    // Create default config file by copying from project config
    let config_file_name = format!("{}.toml", agent_name);
    let config_file = agent_dir.join("config").join(&config_file_name);
    if !config_file.exists() || force {
        // Try to copy from the project's config/{agent_name}.toml
        let project_config = std::env::current_dir()?.join("config").join(&config_file_name);
        if project_config.exists() {
            std::fs::copy(&project_config, &config_file)?;
            ctx.log_info(&format!("Copied config file: {} -> {}", project_config.display(), config_file.display()));
        } else {
            // Fallback to default config if project config doesn't exist
            let default_config = format!(r#"# {} Configuration
# This file was generated by '{} init'

[agent]
name = "{}"
version = "0.1.0"
default_mode = "confirm"

[execution]
timeout_seconds = 120
max_retries = 3
max_tokens = 4000

[llm]
endpoint = "chat/completions"
enable_streaming = true
"#, agent_name.chars().next().unwrap().to_uppercase().collect::<String>() + &agent_name[1..], agent_name, agent_name);
            std::fs::write(&config_file, default_config)?;
            ctx.log_info(&format!("Created default config file: {}", config_file.display()));
        }
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

    // Copy project providers if they exist
    let project_providers = std::env::current_dir()?.join("providers");
    if project_providers.exists() {
        for entry in std::fs::read_dir(&project_providers)? {
            let entry = entry?;
            let entry_name = entry.file_name();
            let source = entry.path();
            let dest = agent_dir.join("providers").join(&entry_name);

            if source.is_dir() {
                // Only copy if the destination doesn't exist or we're in force mode
                if dest.exists() {
                    if force {
                        // Remove existing directory
                        std::fs::remove_dir_all(&dest)?;
                    } else {
                        // Skip if not in force mode
                        continue;
                    }
                }

                // Copy directory recursively
                copy_dir_recursive(&source, &dest)?;
                ctx.log_info(&format!("Copied provider: {} -> {}", source.display(), dest.display()));
            }
        }
    }

    // Copy project extensions if they exist
    let project_extensions = std::env::current_dir()?.join("extensions");
    if project_extensions.exists() {
        for entry in std::fs::read_dir(&project_extensions)? {
            let entry = entry?;
            let entry_name = entry.file_name();
            let source = entry.path();
            let dest = agent_dir.join("extensions").join(&entry_name);

            if source.is_dir() {
                // Only copy if the destination doesn't exist or we're in force mode
                if dest.exists() {
                    if force {
                        // Remove existing directory
                        std::fs::remove_dir_all(&dest)?;
                    } else {
                        // Skip if not in force mode
                        continue;
                    }
                }

                // Copy directory recursively
                copy_dir_recursive(&source, &dest)?;
                ctx.log_info(&format!("Copied extension: {} -> {}", source.display(), dest.display()));
            }
        }
    }

    ctx.log_success(&format!("{} initialized successfully", agent_name.chars().next().unwrap().to_uppercase().collect::<String>() + &agent_name[1..]));
    ctx.log_info(&format!("Configuration directory: {}", agent_dir.display()));
    ctx.log_info(&format!("Binary installed to: {}", binary_dest.display()));
    ctx.log_info(&format!("Symlink created: {}", local_bin_symlink.display()));
    Ok(())
}

/// Handle the config command
async fn config_command<C: CommandContext>(ctx: &C, matches: &ArgMatches) -> CliResult<()> {
    if matches.get_flag("show") {
        let opts = crate::cli::commands::config::ConfigShowOptions {
            detailed: false,
        };
        crate::cli::commands::config::config_show(ctx, opts)
    } else if matches.get_flag("edit") {
        ctx.log_error("Config editing not yet implemented")?;
        Ok(())
    } else if matches.get_flag("validate") {
        crate::cli::commands::config::config_validate(ctx, false)
    } else {
        ctx.log_info("Use --show, --edit, or --validate flags");
        Ok(())
    }
}

/// Handle the cache command
async fn cache_command<C: CommandContext>(ctx: &C, matches: &ArgMatches) -> CliResult<()> {
    let storage_access = AbkStorageAccess::new();

    if matches.get_flag("clear") {
        let opts = crate::cli::commands::cache::CacheCleanOptions {
            dry_run: false,
            older_than_days: None,
            max_size_gb: None,
        };
        crate::cli::commands::cache::cache_clean(ctx, &storage_access, opts).await
    } else if matches.get_flag("list") {
        let opts = crate::cli::commands::cache::CacheListOptions {
            sort_by_size: false,
            min_size_mb: None,
        };
        crate::cli::commands::cache::cache_list(ctx, &storage_access, opts).await
    } else if matches.get_flag("size") {
        let opts = crate::cli::commands::cache::CacheSizeOptions {
            human_readable: true,
            sort_by_size: false,
        };
        crate::cli::commands::cache::cache_size(ctx, &storage_access, opts).await
    } else {
        ctx.log_info("Use --clear, --list, or --size flags");
        Ok(())
    }
}

/// Handle the resume command
async fn resume_command<C: CommandContext>(ctx: &C, matches: &ArgMatches) -> CliResult<()> {
    let session_id = matches.get_one::<String>("session").cloned();
    let checkpoint_id = matches.get_one::<String>("checkpoint").cloned();
    let list = matches.get_flag("list");
    let interactive = matches.get_flag("interactive");

    let opts = crate::cli::commands::resume::ResumeOptions {
        session_id,
        checkpoint_id,
        list,
        interactive,
        quiet: false,
    };

    // Create concrete adapter implementations with pre-parsed config
    let checkpoint_access = AbkCheckpointAccess::with_config(ctx.config());
    let restoration_access = AbkRestorationAccess::with_config(ctx.config());

    crate::cli::commands::resume::resume_session(ctx, &checkpoint_access, &restoration_access, opts).await
}

/// Handle the checkpoints command
async fn checkpoints_command<C: CommandContext>(ctx: &C, matches: &ArgMatches) -> CliResult<()> {
    let checkpoint_access = AbkCheckpointAccess::with_config(ctx.config());

    if matches.get_flag("list") {
        let opts = crate::cli::commands::checkpoints::ListOptions {
            session_id: None, // List all checkpoints
            verbose: false,
        };
        crate::cli::commands::checkpoints::list_checkpoints(ctx, &checkpoint_access, opts).await
    } else if let Some(id) = matches.get_one::<String>("show") {
        // Parse session_id/checkpoint_id from the format "session_id/checkpoint_id"
        let parts: Vec<&str> = id.split('/').collect();
        if parts.len() != 2 {
            ctx.log_error("Invalid checkpoint ID format. Use: session_id/checkpoint_id")?;
            return Ok(());
        }

        let opts = crate::cli::commands::checkpoints::ShowOptions {
            session_id: parts[0].to_string(),
            checkpoint_id: parts[1].to_string(),
        };
        crate::cli::commands::checkpoints::show_checkpoint(ctx, &checkpoint_access, opts).await
    } else if let Some(id) = matches.get_one::<String>("delete") {
        // Parse session_id/checkpoint_id from the format "session_id/checkpoint_id"
        let parts: Vec<&str> = id.split('/').collect();
        if parts.len() != 2 {
            ctx.log_error("Invalid checkpoint ID format. Use: session_id/checkpoint_id")?;
            return Ok(());
        }

        let opts = crate::cli::commands::checkpoints::DeleteOptions {
            session_id: parts[0].to_string(),
            checkpoint_id: parts[1].to_string(),
            confirm: true, // CLI commands should be confirmed
        };
        crate::cli::commands::checkpoints::delete_checkpoint(ctx, &checkpoint_access, opts).await
    } else if matches.get_flag("clean") {
        ctx.log_warning("Checkpoint cleanup not yet implemented")?;
        ctx.log_info("Use individual checkpoint deletion with --delete instead");
        Ok(())
    } else {
        ctx.log_info("Use --list, --show <session_id/checkpoint_id>, --delete <session_id/checkpoint_id>, or --clean flags");
        Ok(())
    }
}

/// Handle the sessions command
async fn sessions_command<C: CommandContext>(ctx: &C, matches: &ArgMatches) -> CliResult<()> {
    let checkpoint_access = AbkCheckpointAccess::with_config(ctx.config());

    if matches.get_flag("list") {
        let opts = crate::cli::commands::sessions::ListOptions {
            project: None,
            all: true,
            verbose: false,
            page: 0,
            page_size: 50,
        };
        crate::cli::commands::sessions::list_sessions(ctx, &checkpoint_access, opts).await
    } else if let Some(id) = matches.get_one::<String>("show") {
        let opts = crate::cli::commands::sessions::ShowOptions {
            session_id: id.clone(),
            checkpoints: true,
        };
        crate::cli::commands::sessions::show_session(ctx, &checkpoint_access, opts).await
    } else if let Some(id) = matches.get_one::<String>("delete") {
        let opts = crate::cli::commands::sessions::DeleteOptions {
            session_id: id.clone(),
            confirm: true,
        };
        crate::cli::commands::sessions::delete_session(ctx, &checkpoint_access, opts).await
    } else {
        ctx.log_info("Use --list, --show <id>, or --delete <id> flags");
        Ok(())
    }
}

/// Handle the misc command
async fn misc_command<C: CommandContext>(ctx: &C, matches: &ArgMatches) -> CliResult<()> {
    if matches.get_flag("doctor") {
        let opts = crate::cli::commands::misc::DoctorOptions {
            verbose: false,
        };
        crate::cli::commands::misc::run_doctor(ctx, opts).await
    } else if matches.get_flag("stats") {
        let storage_access = AbkStorageAccess::new();
        let opts = crate::cli::commands::misc::StatsOptions {
            detailed: false,
        };
        crate::cli::commands::misc::show_stats(ctx, &storage_access, opts).await
    } else if matches.get_flag("clean") {
        let opts = crate::cli::commands::misc::CleanOptions {
            dry_run: false,
            temp_only: true,
        };
        crate::cli::commands::misc::clean_temp(ctx, opts).await
    } else {
        ctx.log_info("Use --doctor, --stats, or --clean flags");
        Ok(())
    }
}

/// Handle the extension command
#[cfg(feature = "extension")]
async fn extension_command<C: CommandContext>(ctx: &C, matches: &ArgMatches) -> CliResult<()> {
    if matches.get_flag("list") {
        crate::cli::commands::extension::list(ctx).await
    } else if let Some(path) = matches.get_one::<String>("install") {
        crate::cli::commands::extension::install(ctx, path, None).await
    } else if let Some(name) = matches.get_one::<String>("remove") {
        crate::cli::commands::extension::remove(ctx, name).await
    } else if let Some(name) = matches.get_one::<String>("info") {
        crate::cli::commands::extension::info(ctx, name).await
    } else {
        ctx.log_info("Use --list, --install <path>, --remove <name>, or --info <name> flags");
        Ok(())
    }
}
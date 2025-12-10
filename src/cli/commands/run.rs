//! Run command - execute agent workflows
//!
//! Provides reusable agent execution logic for CLI commands

use crate::cli::error::{CliError, CliResult};
use crate::cli::adapters::CommandContext;
use crate::orchestration::AgentContext;
use crate::agent::AgentMode;
use std::path::PathBuf;

/// Options for running an agent
#[derive(Debug, Clone)]
pub struct RunOptions {
    pub task: String,
    pub config_path: Option<PathBuf>,
    pub env_path: Option<PathBuf>,
    pub yolo: bool,
    pub mode: Option<String>,
    pub run_mode: Option<String>,
    pub verbose: bool,
}

/// Execute an agent workflow
pub async fn execute_run<C: CommandContext>(
    ctx: &C,
    options: RunOptions,
) -> CliResult<()> {
    use std::env;
    use std::fs;

    let RunOptions { task, config_path, env_path, yolo, mode, run_mode, verbose } = options;

    // Determine run mode (global or local)
    let run_mode = run_mode.unwrap_or_else(|| "global".to_string());

    // Determine paths based on run mode
    let (config_path, env_path, template_base, log_base) = match run_mode.as_str() {
        "global" => {
            // Get agent name from context config
            let agent_name = &ctx.config().agent.name;
            
            // Get home directory
            let home_dir = env::var("HOME").map_err(|_| {
                CliError::ConfigError("Could not determine home directory from HOME environment variable".to_string())
            })?;
            let home_path = PathBuf::from(home_dir);
            let share_dir = home_path.join(format!(".{}", agent_name));

            // Global paths
            let config_file_name = format!("{}.toml", agent_name);
            let config_path = share_dir.join("config").join(&config_file_name);
            let env_path = share_dir.join(".env");
            let template_base = share_dir.join("templates");

            // Create logs directory in current directory for global mode
            let log_base = PathBuf::from("/tmp");
            fs::create_dir_all(&log_base)
                .map_err(|e| CliError::IoError(e))?;

            (
                Some(config_path),
                Some(env_path),
                Some(template_base),
                Some(log_base),
            )
        }
        "local" => {
            // Local paths (existing behavior)
            (config_path, env_path, None, None)
        }
        _ => {
            return Err(CliError::ValidationError(format!(
                "Invalid run mode: {}. Use 'local' or 'global'",
                run_mode
            )));
        }
    };

    // Determine agent mode
    let agent_mode = if yolo {
        AgentMode::Yolo
    } else if let Some(mode_str) = mode {
        mode_str.parse()
            .map_err(|_| CliError::ValidationError(format!("Invalid mode: {}", mode_str)))?
    } else {
        AgentMode::Confirm
    };

    // Initialize agent with determined paths
    ctx.log_info(&format!(
        "Initializing agent in {} mode (run mode: {})...",
        agent_mode, run_mode
    ));

    let mut agent = crate::agent::Agent::new_with_bases(
        config_path.as_deref(),
        env_path.as_deref(),
        Some(agent_mode),
        template_base.as_deref(),
        log_base.as_deref(),
    )
    .map_err(|e| CliError::ExecutionError(format!("Failed to create agent: {}", e)))?;
    
    // Initialize remote checkpoint backend if configured
    #[cfg(feature = "storage-documentdb")]
    {
        if let Err(e) = agent.initialize_remote_checkpoint_backend(config_path.as_deref()).await {
            ctx.log_info(&format!("Note: Remote checkpoint backend not initialized: {}", e));
        }
    }

    // Check for resume context before starting new session
    let current_dir = std::env::current_dir()
        .map_err(|e| CliError::IoError(e))?;
    
    let resume_tracker = crate::checkpoint::ResumeTracker::new()
        .map_err(|e| CliError::CheckpointError(format!("Failed to create resume tracker: {}", e)))?;
    
    let resume_context = resume_tracker.get_resume_context_for_project(&current_dir)
        .map_err(|e| CliError::CheckpointError(format!("Failed to check resume context: {}", e)))?;

    let result = if let Some(context) = resume_context {
        // Resume from checkpoint instead of starting new session
        ctx.log_info(&format!("Found resumed session: {} (checkpoint: {})", 
            context.session_id, context.checkpoint_id));
        ctx.log_info("Continuing from restored checkpoint...");
        
        let resume_result = agent.resume_from_checkpoint(
            &context.project_path,
            &context.session_id,
            &context.checkpoint_id,
        )
        .await
        .map_err(|e| CliError::ExecutionError(format!("Failed to resume from checkpoint: {}", e)))?;
        
        // Clear the resume context after successful use
        resume_tracker.clear_resume_context()
            .map_err(|e| CliError::CheckpointError(format!("Failed to clear resume context: {}", e)))?;
        
        // Add the new task as a user message to the restored conversation
        agent.chat_formatter_mut().add_user_message(task.clone(), None);
        
        // Create a new checkpoint with the updated conversation (including new task)
        if agent.should_checkpoint() {
            if let Err(e) = agent.create_workflow_checkpoint(agent.current_iteration()).await {
                ctx.log_error(&format!("Failed to create checkpoint with new task: {}", e))?;
            } else {
                ctx.log_info("Created new checkpoint with updated conversation including new task");
            }
        }
        
        resume_result
    } else {
        // Start new session
        ctx.log_info(&format!("Starting new session: {}", task));
        agent.start_session(&task, None)
            .await
            .map_err(|e| CliError::ExecutionError(format!("Failed to start session: {}", e)))?
    };

    if verbose {
        ctx.log_info(&result);
    }

    // Run the workflow - use streaming approach if enabled
    ctx.log_info("Starting workflow execution...");
    let streaming_enabled = agent.is_streaming_enabled();
    let max_iterations = ctx.config().execution.max_iterations;

    let workflow_result = if streaming_enabled {
        ctx.log_info("🚀 Using streaming workflow");
        crate::orchestration::run_workflow_streaming(&mut agent, max_iterations).await
    } else {
        ctx.log_info("📞 Using traditional iterative workflow");
        crate::orchestration::run_workflow(&mut agent, max_iterations).await
    };

    match workflow_result {
        Ok(completion_reason) => {
            ctx.log_success(&format!("Workflow completed: {}", completion_reason));
            if verbose {
                ctx.log_info(&format!("Mode: {}", agent.current_mode()));
                ctx.log_info(&format!(
                    "Working directory: {}",
                    agent.executor().working_dir().display()
                ));
            }
            Ok(())
        }
        Err(e) => {
            ctx.log_error(&format!("Task failed: {}", e))?;
            Err(CliError::ExecutionError(format!("Agent execution failed: {}", e)))
        }
    }
}
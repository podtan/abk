//! Run command - execute agent workflows
//!
//! Provides reusable agent execution logic for CLI commands

use crate::cli::error::{CliError, CliResult};
use crate::cli::adapters::CommandContext;
use crate::cli::ResumeInfo;
use crate::cli::TaskResult;
use crate::orchestration::AgentContext;
use crate::agent::AgentMode;

/// Options for running an agent
pub struct RunOptions {
    pub task: String,
    pub yolo: bool,
    pub mode: Option<String>,
    pub run_mode: Option<String>,
    pub verbose: bool,
    /// Optional custom output sink (e.g., TuiSink for TUI mode).
    /// When `Some`, the agent's output sink is set to this value, overriding
    /// the default NoopSink behavior in TUI mode.
    pub output_sink: Option<std::sync::Arc<dyn crate::orchestration::output::OutputSink>>,
    /// Optional resume info for TUI session continuity.
    /// When `Some`, the agent resumes from the specified checkpoint instead of
    /// starting a new session. The CLI flow (last_resume.json) is skipped.
    pub resume_info: Option<ResumeInfo>,
}

/// Execute an agent workflow
pub async fn execute_run<C: CommandContext>(
    ctx: &C,
    options: RunOptions,
) -> CliResult<TaskResult> {
    let RunOptions { task, yolo, mode, run_mode, verbose, output_sink, resume_info } = options;

    // Determine run mode (global or local)
    let run_mode = run_mode.unwrap_or_else(|| "global".to_string());

    // Validate run mode
    match run_mode.as_str() {
        "global" | "local" => {}
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

    let mut agent = crate::agent::Agent::new_from_config(
        ctx.config().clone(),
        Some(agent_mode),
    )
    .await
    .map_err(|e| CliError::ExecutionError(format!("Failed to create agent: {}", e)))?;
    
    // Initialize remote checkpoint backend if configured
    #[cfg(feature = "storage-documentdb")]
    {
        if let Err(e) = agent.initialize_remote_checkpoint_backend(ctx.config()).await {
            ctx.log_info(&format!("Note: Remote checkpoint backend not initialized: {}", e));
        }
    }

    // Check for resume context before starting new session
    let current_dir = std::env::current_dir()
        .map_err(|e| CliError::IoError(e))?;
    
    // Set the working directory for all tools (fixes issue where tools use wrong directory)
    agent.set_working_directory(current_dir.clone());

    // Set the output sink for the agent:
    // - If a custom output_sink was provided (e.g., TuiSink from TUI mode), use it.
    // - If in TUI mode without a custom sink, use NoopSink to prevent println! from
    //   corrupting the ratatui alternate screen buffer.
    // - Otherwise, keep the default StdoutSink.
    if let Some(sink) = output_sink {
        agent.set_output_sink(sink);
    } else if crate::observability::is_tui_mode() {
        agent.set_output_sink(crate::orchestration::output::noop_sink());
    }

    // Check for resume context — from TUI parameter OR from last_resume.json
    let resume_context = if let Some(ref info) = resume_info {
        // TUI provided resume info directly (no file needed)
        ctx.log_info(&format!(
            "TUI resume info: session={}, checkpoint={}",
            info.session_id, info.checkpoint_id
        ));
        Some(crate::checkpoint::resume_tracker::ResumeContext {
            project_path: current_dir.clone(),
            session_id: info.session_id.clone(),
            checkpoint_id: info.checkpoint_id.clone(),
            restored_at: chrono::Utc::now(),
            working_directory: current_dir.clone(),
            task_description: String::new(),
            workflow_step: "Continue".to_string(),
            iteration: info.iteration,
        })
    } else {
        // CLI mode: check for last_resume.json
        let resume_tracker = crate::checkpoint::ResumeTracker::new()
            .map_err(|e| CliError::CheckpointError(format!("Failed to create resume tracker: {}", e)))?;
        
        resume_tracker.get_resume_context_for_project(&current_dir)
            .map_err(|e| CliError::CheckpointError(format!("Failed to check resume context: {}", e)))?
    };

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
        
        // Clear the resume context only if we read from file (not from TUI)
        if resume_info.is_none() {
            let resume_tracker = crate::checkpoint::ResumeTracker::new()
                .map_err(|e| CliError::CheckpointError(format!("Failed to create resume tracker: {}", e)))?;
            resume_tracker.clear_resume_context()
                .map_err(|e| CliError::CheckpointError(format!("Failed to clear resume context: {}", e)))?;
        }
        
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

    // Create final checkpoint and extract resume info for session continuity
    let final_resume_info = agent.create_final_checkpoint_and_get_resume_info().await;

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
            Ok(TaskResult { success: true, error: None, resume_info: final_resume_info })
        }
        Err(e) => {
            ctx.log_error(&format!("Task failed: {:#}", e))?;
            Ok(TaskResult { success: false, error: Some(format!("{:#}", e)), resume_info: final_resume_info })
        }
    }
}
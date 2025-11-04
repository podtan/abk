//! Resume command for ABK
//!
//! Provides session discovery, selection, and checkpoint restoration.

use crate::cli::adapters::checkpoint::{CheckpointAccess, RestorationAccess, ResumeContext};
use crate::cli::adapters::context::CommandContext;
use crate::cli::error::CliResult;
use std::path::{Path, PathBuf};

/// Resume session info for display
#[derive(Debug, Clone)]
pub struct ResumeSessionInfo {
    pub session_id: String,
    pub project_name: String,
    pub project_path: PathBuf,
    pub checkpoint_count: usize,
    pub last_accessed: chrono::DateTime<chrono::Utc>,
    pub description: Option<String>,
    pub is_current_project: bool,
}

/// Resume options
#[derive(Debug, Clone)]
pub struct ResumeOptions {
    pub session_id: Option<String>,
    pub checkpoint_id: Option<String>,
    pub list: bool,
    pub interactive: bool,
}

/// Resume a session
pub async fn resume_session<C, A, R>(
    ctx: &C,
    checkpoint_access: &A,
    restoration_access: &R,
    opts: ResumeOptions,
) -> CliResult<()>
where
    C: CommandContext + ?Sized,
    A: CheckpointAccess + ?Sized,
    R: RestorationAccess + ?Sized,
{
    let current_dir = ctx.working_dir()?;

    // Enhanced session discovery
    if opts.list || opts.session_id.is_none() || opts.interactive {
        let sessions_info = discover_resume_sessions(ctx, checkpoint_access, &current_dir).await?;

        if sessions_info.is_empty() {
            ctx.log_warning("No sessions available for resume.");
            return Ok(());
        }

        display_resume_candidates(ctx, &sessions_info);

        if opts.session_id.is_none() && !opts.interactive {
            return Ok(());
        }

        // Interactive session selection
        if opts.interactive {
            if let Some((selected_session, selected_project_path)) =
                interactive_session_selection(ctx, &sessions_info)?
            {
                return execute_resume(
                    ctx,
                    checkpoint_access,
                    restoration_access,
                    &selected_project_path,
                    &selected_session,
                    opts.checkpoint_id,
                )
                .await;
            } else {
                ctx.log_warning("Resume cancelled by user.");
                return Ok(());
            }
        }
    }

    if let Some(session_id) = opts.session_id {
        // Find session across projects
        let projects = checkpoint_access.list_projects().await?;
        let mut project_path = None;

        // Try current project first
        for project_metadata in &projects {
            if current_dir.starts_with(&project_metadata.project_path)
                || project_metadata.project_path == current_dir
            {
                let sessions = checkpoint_access.list_sessions(&project_metadata.project_path).await?;

                if sessions.iter().any(|s| s.session_id == session_id) {
                    project_path = Some(project_metadata.project_path.clone());
                    break;
                }
            }
        }

        // If not found in current project, search all projects
        if project_path.is_none() {
            for project_metadata in projects {
                let sessions = checkpoint_access.list_sessions(&project_metadata.project_path).await?;

                if sessions.iter().any(|s| s.session_id == session_id) {
                    project_path = Some(project_metadata.project_path.clone());
                    break;
                }
            }
        }

        let project_path = match project_path {
            Some(path) => path,
            None => {
                ctx.log_error(&format!("Session '{}' not found", session_id));
                ctx.log_info("Use 'simpaticoder resume --list' to see available sessions");
                return Ok(());
            }
        };

        return execute_resume(
            ctx,
            checkpoint_access,
            restoration_access,
            &project_path,
            &session_id,
            opts.checkpoint_id,
        )
        .await;
    }

    Ok(())
}

// Helper functions

async fn discover_resume_sessions<C, A>(
    ctx: &C,
    checkpoint_access: &A,
    current_dir: &Path,
) -> CliResult<Vec<ResumeSessionInfo>>
where
    C: CommandContext + ?Sized,
    A: CheckpointAccess + ?Sized,
{
    let mut sessions_info = Vec::new();
    let projects = checkpoint_access.list_projects().await?;

    for project_metadata in projects {
        let sessions = checkpoint_access.list_sessions(&project_metadata.project_path).await?;

        let is_current_project = current_dir.starts_with(&project_metadata.project_path)
            || project_metadata.project_path == *current_dir;

        for session_meta in sessions {
            // Only include sessions with checkpoints
            if session_meta.checkpoint_count > 0 {
                sessions_info.push(ResumeSessionInfo {
                    session_id: session_meta.session_id,
                    project_name: ctx.format_project_name(&project_metadata.project_path)?,
                    project_path: project_metadata.project_path.clone(),
                    checkpoint_count: session_meta.checkpoint_count,
                    last_accessed: session_meta.last_accessed,
                    description: session_meta.description,
                    is_current_project,
                });
            }
        }
    }

    // Sort by last accessed (most recent first), prioritizing current project
    sessions_info.sort_by(|a, b| match (a.is_current_project, b.is_current_project) {
        (true, false) => std::cmp::Ordering::Less,
        (false, true) => std::cmp::Ordering::Greater,
        _ => b.last_accessed.cmp(&a.last_accessed),
    });

    Ok(sessions_info)
}

fn display_resume_candidates<C: CommandContext + ?Sized>(
    ctx: &C,
    sessions_info: &[ResumeSessionInfo],
) -> CliResult<()> {
    ctx.log_info("📋 Available Sessions for Resume");
    ctx.log_info("");

    let mut current_project_sessions = Vec::new();
    let mut other_project_sessions = Vec::new();

    for session in sessions_info {
        if session.is_current_project {
            current_project_sessions.push(session);
        } else {
            other_project_sessions.push(session);
        }
    }

    // Display current project sessions first
    if !current_project_sessions.is_empty() {
        ctx.log_info("🎯 Current Project");
        for (i, session) in current_project_sessions.iter().enumerate() {
            display_session_info(ctx, i + 1, session);
        }
        ctx.log_info("");
    }

    // Display other project sessions
    if !other_project_sessions.is_empty() {
        ctx.log_info("📁 Other Projects");
        let start_index = current_project_sessions.len();
        for (i, session) in other_project_sessions.iter().enumerate() {
            display_session_info(ctx, start_index + i + 1, session);
        }
    }

    Ok(())
}

fn display_session_info<C: CommandContext + ?Sized>(
    ctx: &C,
    index: usize,
    session: &ResumeSessionInfo,
) -> CliResult<()> {
    let session_id_display = if session.session_id.len() > 25 {
        format!("{}...", &session.session_id[..22])
    } else {
        session.session_id.clone()
    };

    let time_ago = format_time_ago(session.last_accessed);

    ctx.log_info(&format!(
        "  {}. {} {} checkpoints",
        index, session_id_display, session.checkpoint_count
    ));

    let desc = if let Some(desc) = &session.description {
        truncate_with_ellipsis(desc, 40)
    } else {
        "No description".to_string()
    };

    ctx.log_info(&format!(
        "     {} • {} • {}",
        session.project_name, time_ago, desc
    ));

    Ok(())
}

fn interactive_session_selection<C: CommandContext + ?Sized>(
    ctx: &C,
    sessions_info: &[ResumeSessionInfo],
) -> CliResult<Option<(String, PathBuf)>> {
    ctx.log_info("\nSelect a session to resume:");
    ctx.log_info("Enter the session number (or 'q' to quit): ");

    let input = ctx.read_line("→ ")?;
    let input = input.trim();

    if input.eq_ignore_ascii_case("q") || input.eq_ignore_ascii_case("quit") {
        return Ok(None);
    }

    match input.parse::<usize>() {
        Ok(index) if index > 0 && index <= sessions_info.len() => {
            let selected_session = &sessions_info[index - 1];
            Ok(Some((
                selected_session.session_id.clone(),
                selected_session.project_path.clone(),
            )))
        }
        _ => {
            ctx.log_error("Invalid selection. Please enter a valid session number.");
            Ok(None)
        }
    }
}

async fn execute_resume<C, A, R>(
    ctx: &C,
    checkpoint_access: &A,
    restoration_access: &R,
    project_path: &Path,
    session_id: &str,
    checkpoint: Option<String>,
) -> CliResult<()>
where
    C: CommandContext + ?Sized,
    A: CheckpointAccess + ?Sized,
    R: RestorationAccess + ?Sized,
{
    ctx.log_info(&format!("🔄 Resume Session: {}", session_id));

    if let Some(ref checkpoint_id) = checkpoint {
        ctx.log_info(&format!("  From checkpoint: {}", checkpoint_id));
    }

    // Find the session
    let sessions = checkpoint_access.list_sessions(&project_path.to_path_buf()).await?;

    let session_meta = match sessions.iter().find(|s| s.session_id == session_id) {
        Some(session) => session,
        None => {
            ctx.log_error(&format!("Session '{}' not found", session_id));
            return Ok(());
        }
    };

    // Determine which checkpoint to restore from
    let checkpoint_id = if let Some(cp_id) = checkpoint {
        cp_id
    } else {
        // Use the latest checkpoint
        let checkpoints = checkpoint_access
            .list_checkpoints(&project_path.to_path_buf(), session_id)
            .await?;

        if checkpoints.is_empty() {
            ctx.log_error("No checkpoints found in session");
            return Ok(());
        }

        // Find the most recent checkpoint
        let latest_checkpoint = checkpoints.iter().max_by_key(|cp| cp.created_at);
        match latest_checkpoint {
            Some(cp) => cp.checkpoint_id.clone(),
            None => {
                ctx.log_error("Could not determine latest checkpoint");
                return Ok(());
            }
        }
    };

    ctx.log_info(&format!("  Restoring from checkpoint: {}", checkpoint_id));

    // Restore the checkpoint
    match restoration_access
        .restore_checkpoint(&project_path.to_path_buf(), session_id, &checkpoint_id)
        .await
    {
        Ok(restored_checkpoint) => {
            ctx.log_success("Checkpoint restored successfully");

            // Display restoration summary
            let metadata = &restored_checkpoint.restoration_metadata;
            ctx.log_info(&format!(
                "  Restoration completed in {}ms",
                metadata.restore_duration_ms
            ));

            if metadata.warnings_count > 0 {
                ctx.log_warning(&format!("{} warnings during restoration", metadata.warnings_count));
            }

            // Try to perform agent restoration
            match restoration_access
                .restore_agent(&project_path.to_path_buf(), session_id, &checkpoint_id)
                .await
            {
                Ok(agent_result) => {
                    if agent_result.success {
                        ctx.log_success("Agent state restored successfully");
                        if !agent_result.warnings.is_empty() {
                            ctx.log_info("  Warnings:");
                            for warning in &agent_result.warnings {
                                ctx.log_warning(&format!("    - {}", warning));
                            }
                        }
                    } else {
                        ctx.log_warning("Agent restoration completed with errors:");
                        for error in &agent_result.errors {
                            ctx.log_error(&format!("    - {}", error));
                        }
                    }
                }
                Err(e) => {
                    ctx.log_warning(&format!("Agent restoration failed: {}", e));
                }
            }

            // Store resume context
            let resume_context = ResumeContext {
                project_path: project_path.to_path_buf(),
                session_id: session_id.to_string(),
                checkpoint_id: checkpoint_id.clone(),
                restored_at: chrono::Utc::now(),
                working_directory: restored_checkpoint.checkpoint.agent_state.working_directory.clone(),
                task_description: restored_checkpoint.checkpoint.agent_state.task_description.clone(),
                workflow_step: restored_checkpoint.checkpoint.metadata.workflow_step.clone(),
                iteration: restored_checkpoint.checkpoint.metadata.iteration,
            };

            if let Err(e) = restoration_access.store_resume_context(&resume_context).await {
                ctx.log_warning(&format!("Failed to store resume context: {}", e));
            } else {
                ctx.log_success("Resume context stored for future agent sessions");
            }

            // Display restored checkpoint information
            let checkpoint = &restored_checkpoint.checkpoint;
            ctx.log_info("\n📋 Restored Session Information");
            ctx.log_info(&format!("  Session ID: {}", session_meta.session_id));
            ctx.log_info(&format!("  Checkpoint: {}", checkpoint.metadata.checkpoint_id));
            ctx.log_info(&format!("  Workflow Step: {}", checkpoint.metadata.workflow_step));
            ctx.log_info(&format!("  Iteration: {}", checkpoint.metadata.iteration));
            ctx.log_info(&format!(
                "  Working Directory: {}",
                checkpoint.agent_state.working_directory.display()
            ));

            // Next steps guidance
            ctx.log_info("\n🎯 Next Steps");
            ctx.log_info("  The checkpoint has been restored. You can now:");
            ctx.log_info("  • Continue with: simpaticoder run \"continue the task\"");
            ctx.log_info("  • Or create a new agent to continue from checkpoint context");
            ctx.log_info(&format!(
                "  • Check status with: simpaticoder checkpoints list --session {}",
                session_id
            ));
            ctx.log_info(&format!(
                "  • View session details: simpaticoder sessions show {}",
                session_id
            ));

            // Store checkpoint info for potential agent continuation
            ctx.log_info("\n💡 Resume Information");
            ctx.log_info(&format!("  Project Path: {}", project_path.display()));
            ctx.log_info(&format!("  Session ID: {}", session_id));
            ctx.log_info(&format!("  Checkpoint ID: {}", checkpoint_id));
            ctx.log_info("  Use these details if you need to manually create an agent with restored context.");
        }
        Err(e) => {
            ctx.log_error(&format!("Failed to restore checkpoint: {}", e));
        }
    }

    Ok(())
}

// Utility functions

fn format_time_ago(datetime: chrono::DateTime<chrono::Utc>) -> String {
    let now = chrono::Utc::now();
    let duration = now.signed_duration_since(datetime);

    if duration.num_seconds() < 60 {
        "just now".to_string()
    } else if duration.num_minutes() < 60 {
        format!("{} min ago", duration.num_minutes())
    } else if duration.num_hours() < 24 {
        format!("{} hours ago", duration.num_hours())
    } else if duration.num_days() < 7 {
        format!("{} days ago", duration.num_days())
    } else if duration.num_weeks() < 4 {
        format!("{} weeks ago", duration.num_weeks())
    } else {
        format!("{} months ago", duration.num_days() / 30)
    }
}

fn truncate_with_ellipsis(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        s.to_string()
    } else {
        format!("{}...", &s[..max_len.saturating_sub(3)])
    }
}

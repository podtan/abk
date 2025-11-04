//! Checkpoint management commands for ABK
//!
//! Provides commands to list, show, delete, diff, and export checkpoints.

use crate::cli::adapters::checkpoint::{CheckpointAccess, CheckpointData, CheckpointDiff};
use crate::cli::adapters::context::CommandContext;
use crate::cli::error::CliResult;
use std::path::PathBuf;

/// List checkpoints options
#[derive(Debug, Clone)]
pub struct ListOptions {
    pub session_id: Option<String>,
    pub verbose: bool,
}

/// Show checkpoint options
#[derive(Debug, Clone)]
pub struct ShowOptions {
    pub session_id: String,
    pub checkpoint_id: String,
}

/// Delete checkpoint options
#[derive(Debug, Clone)]
pub struct DeleteOptions {
    pub session_id: String,
    pub checkpoint_id: String,
    pub confirm: bool,
}

/// Diff checkpoints options
#[derive(Debug, Clone)]
pub struct DiffOptions {
    pub session_id: String,
    pub from_checkpoint_id: String,
    pub to_checkpoint_id: String,
}

/// Export checkpoint options
#[derive(Debug, Clone)]
pub struct ExportOptions {
    pub session_id: String,
    pub checkpoint_id: String,
    pub output_path: PathBuf,
}

/// List checkpoints
pub async fn list_checkpoints<C, A>(
    ctx: &C,
    checkpoint_access: &A,
    opts: ListOptions,
) -> CliResult<()>
where
    C: CommandContext + ?Sized,
    A: CheckpointAccess + ?Sized,
{
    ctx.log_info("📋 Checkpoints List");

    let projects = checkpoint_access.list_projects().await?;

    if let Some(session_id) = &opts.session_id {
        // List checkpoints for a specific session
        let mut session_found = false;

        for project_metadata in projects {
            let sessions = checkpoint_access.list_sessions(&project_metadata.project_path).await?;

            if sessions.iter().any(|s| s.session_id == *session_id) {
                session_found = true;
                let checkpoints = checkpoint_access
                    .list_checkpoints(&project_metadata.project_path, session_id)
                    .await?;

                ctx.log_info(&format!(
                    "Session: {} ({})",
                    session_id,
                    project_metadata.project_path.display()
                ))?;

                if checkpoints.is_empty() {
                    ctx.log_info("  No checkpoints found.");
                } else {
                    for checkpoint in checkpoints {
                        ctx.log_info(&format!(
                            "  {} - {} ({})",
                            checkpoint.checkpoint_id,
                            checkpoint.workflow_step,
                            checkpoint.created_at.format("%Y-%m-%d %H:%M:%S")
                        ))?;

                        if opts.verbose {
                            if let Some(description) = &checkpoint.description {
                                ctx.log_info(&format!("    Description: {}", description))?;
                            }
                        }
                    }
                }
                break;
            }
        }

        if !session_found {
            ctx.log_error(&format!("Session '{}' not found", session_id))?;
        }
    } else {
        // List all checkpoints across all sessions
        if projects.is_empty() {
            ctx.log_info("No projects found.");
            return Ok(());
        }

        for project_metadata in projects {
            let sessions = checkpoint_access.list_sessions(&project_metadata.project_path).await?;

            if !sessions.is_empty() {
                ctx.log_info(&format!("Project: {}", project_metadata.project_path.display()))?;

                for session in sessions {
                    let checkpoints = checkpoint_access
                        .list_checkpoints(&project_metadata.project_path, &session.session_id)
                        .await?;

                    if !checkpoints.is_empty() {
                        ctx.log_info(&format!(
                            "  Session: {} ({} checkpoints)",
                            session.session_id,
                            checkpoints.len()
                        ))?;
                    }
                }
            }
        }
    }

    Ok(())
}

/// Show checkpoint details
pub async fn show_checkpoint<C, A>(
    ctx: &C,
    checkpoint_access: &A,
    opts: ShowOptions,
) -> CliResult<()>
where
    C: CommandContext + ?Sized,
    A: CheckpointAccess + ?Sized,
{
    ctx.log_info(&format!(
        "📄 Checkpoint Details: {}/{}",
        opts.session_id, opts.checkpoint_id
    ))?;

    // Find the session and checkpoint
    let projects = checkpoint_access.list_projects().await?;
    let mut checkpoint_found = false;

    for project_metadata in projects {
        let sessions = checkpoint_access.list_sessions(&project_metadata.project_path).await?;

        if sessions.iter().any(|s| s.session_id == opts.session_id) {
            match checkpoint_access
                .load_checkpoint(&project_metadata.project_path, &opts.session_id, &opts.checkpoint_id)
                .await
            {
                Ok(checkpoint) => {
                    checkpoint_found = true;
                    display_checkpoint_details(ctx, &project_metadata.project_path, &opts.session_id, &checkpoint)?;
                    break;
                }
                Err(_) => continue, // Try next project
            }
        }
    }

    if !checkpoint_found {
        ctx.log_error(&format!(
            "Checkpoint '{}/{}' not found",
            opts.session_id, opts.checkpoint_id
        ))?;
    }

    Ok(())
}

/// Delete a checkpoint
pub async fn delete_checkpoint<C, A>(
    ctx: &C,
    checkpoint_access: &A,
    opts: DeleteOptions,
) -> CliResult<()>
where
    C: CommandContext + ?Sized,
    A: CheckpointAccess + ?Sized,
{
    ctx.log_info(&format!(
        "🗑️  Delete Checkpoint: {}/{}",
        opts.session_id, opts.checkpoint_id
    ))?;

    // Find and delete the checkpoint
    let projects = checkpoint_access.list_projects().await?;
    let mut checkpoint_deleted = false;

    for project_metadata in projects {
        let sessions = checkpoint_access.list_sessions(&project_metadata.project_path).await?;

        if sessions.iter().any(|s| s.session_id == opts.session_id) {
            match checkpoint_access
                .delete_checkpoint(&project_metadata.project_path, &opts.session_id, &opts.checkpoint_id)
                .await
            {
                Ok(()) => {
                    checkpoint_deleted = true;
                    ctx.log_success(&format!(
                        "Checkpoint '{}/{}' deleted successfully",
                        opts.session_id, opts.checkpoint_id
                    ))?;
                    break;
                }
                Err(e) => {
                    ctx.log_error(&format!(
                        "Failed to delete checkpoint '{}/{}': {}",
                        opts.session_id, opts.checkpoint_id, e
                    ))?;
                    return Err(e);
                }
            }
        }
    }

    if !checkpoint_deleted {
        ctx.log_error(&format!(
            "Checkpoint '{}/{}' not found",
            opts.session_id, opts.checkpoint_id
        ))?;
    }

    Ok(())
}

/// Diff two checkpoints
pub async fn diff_checkpoints<C, A>(
    ctx: &C,
    checkpoint_access: &A,
    opts: DiffOptions,
) -> CliResult<()>
where
    C: CommandContext + ?Sized,
    A: CheckpointAccess + ?Sized,
{
    ctx.log_info(&format!(
        "🔍 Compare Checkpoints: {}/{} -> {}/{}",
        opts.session_id, opts.from_checkpoint_id, opts.session_id, opts.to_checkpoint_id
    ))?;

    // Find the checkpoints and get diff
    let projects = checkpoint_access.list_projects().await?;

    for project_metadata in projects {
        let sessions = checkpoint_access.list_sessions(&project_metadata.project_path).await?;

        if sessions.iter().any(|s| s.session_id == opts.session_id) {
            match checkpoint_access
                .get_checkpoint_diff(
                    &project_metadata.project_path,
                    &opts.session_id,
                    &opts.from_checkpoint_id,
                    &opts.to_checkpoint_id,
                )
                .await
            {
                Ok(diff) => {
                    display_checkpoint_diff(ctx, &diff)?;
                    return Ok(());
                }
                Err(e) => {
                    ctx.log_error(&format!("Failed to compute diff: {}", e))?;
                    return Err(e);
                }
            }
        }
    }

    ctx.log_error(&format!("Session '{}' not found", opts.session_id))?;
    Ok(())
}

/// Export checkpoint
pub async fn export_checkpoint<C, A>(
    ctx: &C,
    _checkpoint_access: &A,
    opts: ExportOptions,
) -> CliResult<()>
where
    C: CommandContext + ?Sized,
    A: CheckpointAccess + ?Sized,
{
    ctx.log_info(&format!(
        "📤 Export Checkpoint: {}/{} -> {}",
        opts.session_id,
        opts.checkpoint_id,
        opts.output_path.display()
    ))?;

    ctx.log_warning("Checkpoint export command not yet implemented");
    Ok(())
}

// Helper functions

fn display_checkpoint_details<C: CommandContext + ?Sized>(
    ctx: &C,
    project_path: &PathBuf,
    session_id: &str,
    checkpoint: &CheckpointData,
) -> CliResult<()> {
    ctx.log_info(&format!("  Project: {}", project_path.display()))?;
    ctx.log_info(&format!("  Session: {}", session_id))?;
    ctx.log_info(&format!("  Checkpoint ID: {}", checkpoint.metadata.checkpoint_id))?;
    ctx.log_info(&format!("  Workflow Step: {}", checkpoint.metadata.workflow_step))?;
    ctx.log_info(&format!(
        "  Created: {}",
        checkpoint.metadata.created_at.format("%Y-%m-%d %H:%M:%S UTC")
    ))?;

    if let Some(description) = &checkpoint.metadata.description {
        ctx.log_info(&format!("  Description: {}", description))?;
    }

    ctx.log_info(&format!("  Tags: {}", checkpoint.metadata.tags.join(", ")))?;

    // Agent state info
    ctx.log_info(&format!("  Agent Mode: {}", checkpoint.agent_state.current_mode))?;
    ctx.log_info(&format!("  Current Step: {}", checkpoint.agent_state.current_step))?;

    // Conversation info
    ctx.log_info(&format!("  Messages: {}", checkpoint.conversation_state.message_count))?;
    ctx.log_info(&format!("  Token Count: {}", checkpoint.conversation_state.total_tokens))?;

    // File system info
    ctx.log_info(&format!(
        "  Working Directory: {}",
        checkpoint.file_system_state.working_directory.display()
    ))?;
    ctx.log_info(&format!("  Modified Files: {}", checkpoint.file_system_state.modified_files.len()))?;

    Ok(())
}

fn display_checkpoint_diff<C: CommandContext + ?Sized>(
    ctx: &C,
    diff: &CheckpointDiff,
) -> CliResult<()> {
    ctx.log_info("\n📊 Checkpoint Comparison");

    // Metadata
    ctx.log_info("\n🏷️  Metadata:");
    ctx.log_info(&format!("  From: {}", diff.from_checkpoint_id))?;
    ctx.log_info(&format!("  To:   {}", diff.to_checkpoint_id))?;
    ctx.log_info(&format!("  Time difference: {} seconds", diff.time_difference_seconds.abs()))?;

    // Agent state
    ctx.log_info("\n🤖 Agent State:");
    if diff.mode_changed {
        ctx.log_info(&format!("  Mode: {} → {}", diff.mode_from, diff.mode_to))?;
    } else {
        ctx.log_info(&format!("  Mode: {} (unchanged)", diff.mode_from))?;
    }

    if diff.step_changed {
        ctx.log_info(&format!("  Step: {} → {}", diff.step_from, diff.step_to))?;
    } else {
        ctx.log_info(&format!("  Step: {} (unchanged)", diff.step_from))?;
    }

    // Conversation
    ctx.log_info("\n💬 Conversation:");
    if diff.messages_diff != 0 {
        ctx.log_info(&format!("  Messages: {:+} messages", diff.messages_diff))?;
    } else {
        ctx.log_info("  Messages: (unchanged)")?;
    }

    if diff.tokens_diff != 0 {
        ctx.log_info(&format!("  Tokens: {:+} tokens", diff.tokens_diff))?;
    } else {
        ctx.log_info("  Tokens: (unchanged)")?;
    }

    // File system
    ctx.log_info("\n📁 File System:");
    if diff.files_diff != 0 {
        ctx.log_info(&format!("  Modified files: {:+} files", diff.files_diff))?;
    } else {
        ctx.log_info("  Modified files: (unchanged)")?;
    }

    if diff.working_directory_changed {
        ctx.log_info("  Working directory: changed");
    } else {
        ctx.log_info("  Working directory: (unchanged)")?;
    }

    // Tool state
    ctx.log_info("\n🔧 Tool State:");
    if diff.commands_diff != 0 {
        ctx.log_info(&format!("  Commands executed: {:+} commands", diff.commands_diff))?;
    } else {
        ctx.log_info("  Commands executed: (unchanged)")?;
    }

    Ok(())
}

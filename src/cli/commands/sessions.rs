//! Sessions command - checkpoint session management
//!
//! Provides reusable session management logic for agents with checkpoint systems

use crate::cli::error::{CliError, CliResult};
use crate::cli::adapters::{CommandContext, CheckpointAccess, SessionMetadata};
use std::path::PathBuf;

/// Options for listing sessions
#[derive(Debug, Clone)]
pub struct ListOptions {
    pub project: Option<String>,
    pub all: bool,
    pub verbose: bool,
    pub page: usize,
    pub page_size: usize,
}

/// Options for showing session details
#[derive(Debug, Clone)]
pub struct ShowOptions {
    pub session_id: String,
    pub checkpoints: bool,
}

/// Options for deleting a session
#[derive(Debug, Clone)]
pub struct DeleteOptions {
    pub session_id: String,
    pub confirm: bool,
}

/// Options for exporting a session
#[derive(Debug, Clone)]
pub struct ExportOptions {
    pub session_id: String,
    pub output_path: PathBuf,
    pub include_checkpoints: bool,
}

/// Options for importing a session
#[derive(Debug, Clone)]
pub struct ImportOptions {
    pub input_path: PathBuf,
    pub project_path: Option<PathBuf>,
}

/// Options for cleaning old sessions
#[derive(Debug, Clone)]
pub struct CleanOptions {
    pub older_than_days: Option<usize>,
    pub dry_run: bool,
}

/// Options for validating sessions
#[derive(Debug, Clone)]
pub struct ValidateOptions {
    pub session_id: Option<String>,
    pub repair: bool,
    pub verbose: bool,
}

/// List all sessions with pagination
pub async fn list_sessions<C, A>(
    ctx: &C,
    checkpoint_access: &A,
    opts: ListOptions,
) -> CliResult<()>
where
    C: CommandContext,
    A: CheckpointAccess,
{
    ctx.log_info("📋 Sessions List");

    let effective_page_size = opts.page_size.min(100);
    if opts.page_size > 100 {
        ctx.log_warn("⚠️  Page size limited to 100 for performance");
    }

    let projects = checkpoint_access.list_projects().await?;
    
    if projects.is_empty() {
        ctx.log_warn("No sessions found.");
        return Ok(());
    }

    let terminal_width = ctx.terminal_width()?;
    let start_index = opts.page * effective_page_size;

    // Collect all sessions from all projects
    let mut all_sessions = Vec::new();
    
    for project_metadata in projects {
        if !opts.all && opts.project.is_some() {
            if let Some(filter_path) = &opts.project {
                if !project_metadata.project_path.to_string_lossy().contains(filter_path) {
                    continue;
                }
            }
        }

        let sessions = checkpoint_access
            .list_sessions(&project_metadata.project_path)
            .await?;

        for session in sessions {
            all_sessions.push((project_metadata.clone(), session));
        }
    }

    let total_count = all_sessions.len();
    
    if total_count == 0 {
        ctx.log_warn("No sessions found.");
        return Ok(());
    }

    // Apply pagination
    let end_index = (start_index + effective_page_size).min(total_count);
    let page_sessions = if start_index < total_count {
        &all_sessions[start_index..end_index]
    } else {
        &[]
    };

    if start_index >= total_count {
        ctx.log_warn("No sessions found on this page.");
        ctx.log_info(&format!(
            "Total sessions: {}, Page: {}, Page size: {}",
            total_count, opts.page, effective_page_size
        ));
        return Ok(());
    }

    // Display pagination info
    let total_pages = (total_count + effective_page_size - 1) / effective_page_size;
    ctx.log_info(&format!(
        "Page {} of {} (showing {} of {} sessions)",
        opts.page + 1,
        total_pages,
        page_sessions.len(),
        total_count
    ));

    // Display sessions
    for (project_meta, session) in page_sessions {
        let project_name = ctx.format_project_name(&project_meta.project_path)?;
        let session_json = serde_json::to_value(session)
            .map_err(|e| CliError::SerializationError(e.to_string()))?;
        let session_info = ctx.format_session_entry(&session_json, &project_name, terminal_width)?;
        ctx.log_info(&session_info);
        
        if opts.verbose {
            ctx.log_info(&format!("  Tags: {}", session.tags.join(", ")));
            if let Some(desc) = &session.description {
                ctx.log_info(&format!("  Description: {}", desc));
            }
        }
    }

    // Show navigation hint
    if opts.page + 1 < total_pages {
        ctx.log_info(&format!(
            "💡 Use --page {} to see next page",
            opts.page + 1
        ));
    }

    Ok(())
}

/// Show detailed information about a specific session
pub async fn show_session<C, A>(
    ctx: &C,
    checkpoint_access: &A,
    opts: ShowOptions,
) -> CliResult<()>
where
    C: CommandContext,
    A: CheckpointAccess,
{
    ctx.log_info(&format!("📄 Session Details: {}", opts.session_id));

    let projects = checkpoint_access.list_projects().await?;
    let mut session_found = false;

    for project_metadata in projects {
        let sessions = checkpoint_access
            .list_sessions(&project_metadata.project_path)
            .await?;

        if let Some(session) = sessions.iter().find(|s| s.session_id == opts.session_id) {
            session_found = true;

            ctx.log_info(&format!("  Project: {}", project_metadata.project_path.display()));
            ctx.log_info(&format!("  Session ID: {}", session.session_id));
            ctx.log_info(&format!("  Status: {:?}", session.status));
            ctx.log_info(&format!(
                "  Created: {}",
                session.created_at.format("%Y-%m-%d %H:%M:%S UTC")
            ));
            ctx.log_info(&format!(
                "  Last Accessed: {}",
                session.last_accessed.format("%Y-%m-%d %H:%M:%S UTC")
            ));

            if let Some(description) = &session.description {
                ctx.log_info(&format!("  Description: {}", description));
            }

            ctx.log_info(&format!("  Tags: {}", session.tags.join(", ")));

            // List checkpoints for this session
            let checkpoints = checkpoint_access
                .list_checkpoints(&project_metadata.project_path, &opts.session_id)
                .await?;
            
            ctx.log_info(&format!("  Checkpoints: {}", checkpoints.len()));

            for checkpoint in checkpoints.iter().take(5) {
                ctx.log_info(&format!(
                    "    - {} ({}) - {}",
                    checkpoint.checkpoint_id,
                    checkpoint.workflow_step,
                    checkpoint.created_at.format("%H:%M:%S")
                ));
            }

            if checkpoints.len() > 5 {
                ctx.log_info(&format!("    ... and {} more checkpoints", checkpoints.len() - 5));
            }

            break;
        }
    }

    if !session_found {
        return Err(CliError::NotFound(format!("Session '{}' not found", opts.session_id)));
    }

    Ok(())
}

/// Delete a session and all its checkpoints
pub async fn delete_session<C, A>(
    ctx: &C,
    checkpoint_access: &A,
    opts: DeleteOptions,
) -> CliResult<()>
where
    C: CommandContext,
    A: CheckpointAccess,
{
    ctx.log_info(&format!("🗑️  Delete Session: {}", opts.session_id));

    if !opts.confirm {
        ctx.log_warn("⚠️  This will permanently delete the session and all its checkpoints.");
        let confirmed = ctx.confirm("Are you sure you want to continue?")?;
        
        if !confirmed {
            ctx.log_warn("Operation cancelled.");
            return Ok(());
        }
    }

    // Find the session
    let projects = checkpoint_access.list_projects().await?;
    
    if projects.is_empty() {
        ctx.log_warn("No projects found. There are no sessions to delete.");
        return Ok(());
    }

    let mut session_found = false;
    let mut project_path = None;
    let mut session_details: Option<SessionMetadata> = None;

    for project_metadata in &projects {
        let sessions = checkpoint_access
            .list_sessions(&project_metadata.project_path)
            .await?;

        if let Some(session) = sessions.iter().find(|s| s.session_id == opts.session_id) {
            session_found = true;
            project_path = Some(project_metadata.project_path.clone());
            session_details = Some(session.clone());
            break;
        }
    }

    if !session_found {
        ctx.log_error(&format!("Session '{}' not found", opts.session_id));
        ctx.log_info("💡 Available sessions:");
        
        for project_metadata in projects {
            let sessions = checkpoint_access
                .list_sessions(&project_metadata.project_path)
                .await?;
            
            if !sessions.is_empty() {
                ctx.log_info(&format!("   Project: {}", project_metadata.name));
                for session in sessions.iter().take(3) {
                    ctx.log_info(&format!("     • {}", session.session_id));
                }
                if sessions.len() > 3 {
                    ctx.log_info(&format!("     ... and {} more", sessions.len() - 3));
                }
            }
        }
        
        return Err(CliError::NotFound(format!("Session '{}' not found", opts.session_id)));
    }

    // Delete the session
    if let Some(path) = project_path {
        if let Some(session) = session_details {
            ctx.log_info("  📊 Session Details:");
            ctx.log_info(&format!("     Created: {}", session.created_at.format("%Y-%m-%d %H:%M:%S")));
            ctx.log_info(&format!("     Checkpoints: {}", session.checkpoint_count));
        }

        checkpoint_access
            .delete_session(&path, &opts.session_id)
            .await?;
        
        ctx.log_success(&format!("Session '{}' deleted successfully", opts.session_id));
        ctx.log_info("  ℹ️ All associated checkpoints have been removed");
    }

    Ok(())
}

/// Export a session to a file
pub async fn export_session<C, A>(
    ctx: &C,
    checkpoint_access: &A,
    opts: ExportOptions,
) -> CliResult<()>
where
    C: CommandContext,
    A: CheckpointAccess,
{
    ctx.log_info(&format!(
        "📤 Export Session: {} -> {}",
        opts.session_id,
        opts.output_path.display()
    ));

    let projects = checkpoint_access.list_projects().await?;
    let mut session_found = false;

    for project_metadata in projects {
        let sessions = checkpoint_access
            .list_sessions(&project_metadata.project_path)
            .await?;

        if let Some(session) = sessions.iter().find(|s| s.session_id == opts.session_id) {
            session_found = true;

            ctx.log_info(&format!("  Found session in project: {}", project_metadata.name));
            ctx.log_info(&format!("  Project path: {}", project_metadata.project_path.display()));

            // Get checkpoints if requested
            let checkpoints = if opts.include_checkpoints {
                checkpoint_access
                    .list_checkpoints(&project_metadata.project_path, &opts.session_id)
                    .await?
            } else {
                vec![]
            };

            // Create export data
            let export_data = serde_json::json!({
                "version": "1.0",
                "export_date": chrono::Utc::now().to_rfc3339(),
                "session": session,
                "project": {
                    "name": project_metadata.name,
                    "path": project_metadata.project_path,
                    "hash": project_metadata.project_hash
                },
                "checkpoints_metadata": checkpoints,
                "checkpoint_count": checkpoints.len()
            });

            ctx.log_info(&format!("  Exporting {} checkpoints metadata", checkpoints.len()));

            // Write export file
            let export_json = serde_json::to_string_pretty(&export_data)
                .map_err(|e| CliError::SerializationError(e.to_string()))?;
            
            std::fs::write(&opts.output_path, export_json)
                .map_err(|e| CliError::IoError(e))?;

            ctx.log_success(&format!("Session exported successfully to: {}", opts.output_path.display()));

            // Show file size
            if let Ok(metadata) = std::fs::metadata(&opts.output_path) {
                let size = ctx.format_bytes(metadata.len())?;
                ctx.log_info(&format!("  Export file size: {}", size));
            }

            break;
        }
    }

    if !session_found {
        return Err(CliError::NotFound(format!("Session '{}' not found", opts.session_id)));
    }

    Ok(())
}

/// Import a session from a file
pub async fn import_session<C, A>(
    ctx: &C,
    _checkpoint_access: &A,
    opts: ImportOptions,
) -> CliResult<()>
where
    C: CommandContext,
    A: CheckpointAccess,
{
    ctx.log_info(&format!("📥 Import Session: {}", opts.input_path.display()));

    // Read and parse export file
    let import_content = std::fs::read_to_string(&opts.input_path)
        .map_err(|e| CliError::IoError(e))?;

    let import_data: serde_json::Value = serde_json::from_str(&import_content)
        .map_err(|e| CliError::SerializationError(e.to_string()))?;

    // Validate import format
    let version = import_data
        .get("version")
        .and_then(|v| v.as_str())
        .ok_or_else(|| CliError::ValidationError("Invalid import file: missing version".to_string()))?;

    if version != "1.0" {
        return Err(CliError::ValidationError(format!(
            "Unsupported import file version: {}",
            version
        )));
    }

    // Extract session information
    let _session_json = import_data
        .get("session")
        .ok_or_else(|| CliError::ValidationError("Invalid import file: missing session data".to_string()))?;

    let project_json = import_data
        .get("project")
        .ok_or_else(|| CliError::ValidationError("Invalid import file: missing project data".to_string()))?;

    let checkpoints_count = import_data
        .get("checkpoint_count")
        .and_then(|c| c.as_u64())
        .unwrap_or(0);

    ctx.log_info(&format!("  Import file version: {}", version));
    ctx.log_info(&format!(
        "  Export date: {}",
        import_data
            .get("export_date")
            .and_then(|d| d.as_str())
            .unwrap_or("Unknown")
    ));
    ctx.log_info(&format!(
        "  Project: {}",
        project_json
            .get("name")
            .and_then(|n| n.as_str())
            .unwrap_or("Unknown")
    ));
    ctx.log_info(&format!("  Checkpoints: {}", checkpoints_count));

    ctx.log_success("Session import validation completed successfully");
    ctx.log_warn("Note: Full import functionality requires additional project integration");
    ctx.log_info("      This would create a new session with the imported metadata.");

    Ok(())
}

/// Clean old sessions
pub async fn clean_sessions<C, A>(
    ctx: &C,
    checkpoint_access: &A,
    opts: CleanOptions,
) -> CliResult<()>
where
    C: CommandContext,
    A: CheckpointAccess,
{
    ctx.log_info("🧹 Clean Sessions");

    if opts.dry_run {
        ctx.log_warn("Running in dry-run mode - no actual deletion will occur");
    }

    let days = opts.older_than_days.unwrap_or(30);
    ctx.log_info(&format!("Cleaning sessions older than {} days...", days));

    let projects = checkpoint_access.list_projects().await?;
    let mut total_deleted = 0;
    let mut total_sessions = 0;

    let cutoff_date = chrono::Utc::now() - chrono::Duration::days(days as i64);

    for project_metadata in projects {
        let sessions = checkpoint_access
            .list_sessions(&project_metadata.project_path)
            .await?;

        ctx.log_info(&format!("\nProject: {}", project_metadata.name));

        let mut project_deleted = 0;

        for session_meta in sessions {
            total_sessions += 1;

            if session_meta.created_at < cutoff_date {
                if opts.dry_run {
                    ctx.log_info(&format!(
                        "  Would delete: {} (created {})",
                        session_meta.session_id,
                        session_meta.created_at.format("%Y-%m-%d %H:%M")
                    ));
                } else {
                    match checkpoint_access
                        .delete_session(&project_metadata.project_path, &session_meta.session_id)
                        .await
                    {
                        Ok(_) => {
                            ctx.log_success(&format!(
                                "  Deleted: {} (created {})",
                                session_meta.session_id,
                                session_meta.created_at.format("%Y-%m-%d %H:%M")
                            ));
                        }
                        Err(e) => {
                            ctx.log_error(&format!(
                                "  Failed to delete session {}: {}",
                                session_meta.session_id, e
                            ));
                        }
                    }
                }
                project_deleted += 1;
                total_deleted += 1;
            }
        }

        if project_deleted == 0 {
            ctx.log_info("  → No old sessions to clean");
        } else {
            ctx.log_info(&format!("  → {} sessions processed", project_deleted));
        }
    }

    ctx.log_info("");
    if opts.dry_run {
        ctx.log_info("📊 Summary (dry run):");
        ctx.log_info(&format!("  Total sessions scanned: {}", total_sessions));
        ctx.log_info(&format!("  Sessions that would be deleted: {}", total_deleted));
    } else {
        ctx.log_info("📊 Summary:");
        ctx.log_info(&format!("  Total sessions scanned: {}", total_sessions));
        ctx.log_success(&format!("  Sessions deleted: {}", total_deleted));
    }

    Ok(())
}

/// Validate and optionally repair session metadata
pub async fn validate_sessions<C, A>(
    ctx: &C,
    checkpoint_access: &A,
    opts: ValidateOptions,
) -> CliResult<()>
where
    C: CommandContext,
    A: CheckpointAccess,
{
    ctx.log_info("🔍 Validate Session Metadata");

    let projects = checkpoint_access.list_projects().await?;
    let mut total_validated = 0;
    let mut total_repaired = 0;
    let mut validation_issues = Vec::new();

    for project_metadata in projects {
        let sessions = checkpoint_access
            .list_sessions(&project_metadata.project_path)
            .await?;

        for session in sessions {
            // Skip if we're only validating a specific session
            if let Some(ref target_session) = opts.session_id {
                if session.session_id != *target_session {
                    continue;
                }
            }

            total_validated += 1;

            if opts.verbose {
                ctx.log_info(&format!("\n  Validating session: {}", session.session_id));
                ctx.log_info(&format!("    Project: {}", project_metadata.name));
                ctx.log_info(&format!(
                    "    Created: {}",
                    session.created_at.format("%Y-%m-%d %H:%M:%S")
                ));
            }

            // Validate and repair if requested
            let repair_actions = checkpoint_access
                .validate_session(&project_metadata.project_path, &session.session_id, opts.repair)
                .await?;

            if !repair_actions.is_empty() {
                total_repaired += 1;

                for action in &repair_actions {
                    validation_issues.push(format!("Session {}: {}", session.session_id, action));
                }

                if opts.verbose {
                    for action in &repair_actions {
                        ctx.log_success(&format!("    🔧 {}", action));
                    }
                } else if !opts.repair {
                    ctx.log_warn(&format!(
                        "  Session {} has {} issues (use --repair to fix)",
                        session.session_id,
                        repair_actions.len()
                    ));
                }
            }

            if opts.verbose && repair_actions.is_empty() {
                ctx.log_success("    ✓ Session is valid");
            }
        }
    }

    ctx.log_info("");
    ctx.log_info("📊 Validation Summary:");
    ctx.log_info(&format!("  Sessions validated: {}", total_validated));

    if validation_issues.is_empty() {
        ctx.log_success("  ✓ All sessions are valid");
    } else {
        ctx.log_warn(&format!("  Issues found: {}", validation_issues.len()));
        if opts.repair {
            ctx.log_success(&format!("  Sessions repaired: {}", total_repaired));
        } else {
            ctx.log_info("  💡 Use --repair to fix detected issues");
        }

        if !opts.verbose && !validation_issues.is_empty() {
            ctx.log_info("\n  Issues summary:");
            for (i, issue) in validation_issues.iter().enumerate() {
                if i < 5 {
                    ctx.log_info(&format!("    • {}", issue));
                } else if i == 5 {
                    ctx.log_info(&format!(
                        "    • ... and {} more (use --verbose for details)",
                        validation_issues.len() - 5
                    ));
                    break;
                }
            }
        }

        if !opts.repair {
            return Err(CliError::ValidationError(format!(
                "{} validation issues found",
                validation_issues.len()
            )));
        }
    }

    Ok(())
}

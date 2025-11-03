//! Shared utility functions for CLI commands

use colored::*;
use std::fs;
use std::io::Write;
use std::path::PathBuf;
use anyhow::Result;

/// Display a user-friendly error message with suggestions
pub fn display_error_with_suggestions<E: std::fmt::Display>(error: &E, context: &str, app_name: Option<&str>) {
    let app = app_name.unwrap_or("agent");
    
    eprintln!("{} {}", "❌ Error:".red().bold(), context);
    eprintln!("   {}", error.to_string().red());

    // Provide contextual suggestions based on error type
    let error_str = error.to_string().to_lowercase();
    if error_str.contains("no such file or directory") {
        eprintln!("{}", "💡 Suggestions:".blue());
        eprintln!("   • Check that the file or directory path is correct");
        eprintln!("   • Run '{} sessions list' to see available sessions", app);
        eprintln!("   • Use '{} init' to initialize a new project", app);
    } else if error_str.contains("permission denied") {
        eprintln!("{}", "💡 Suggestions:".blue());
        eprintln!("   • Check file permissions");
        eprintln!("   • Try running with appropriate user permissions");
        eprintln!("   • Ensure the ~/.{} directory is writable", app);
    } else if error_str.contains("session") && error_str.contains("not found") {
        eprintln!("{}", "💡 Suggestions:".blue());
        eprintln!("   • Run '{} sessions list' to see available sessions", app);
        eprintln!("   • Check the session ID spelling");
        eprintln!("   • Use '{} sessions validate' to check for issues", app);
    } else if error_str.contains("connection") || error_str.contains("network") {
        eprintln!("{}", "💡 Suggestions:".blue());
        eprintln!("   • Check your internet connection");
        eprintln!("   • Verify API endpoints are accessible");
        eprintln!("   • Try again in a few moments");
    }
}

/// Display progress for long-running operations
pub fn show_progress_spinner(message: &str) -> ProgressIndicator {
    ProgressIndicator::new(message)
}

/// Simple progress indicator for CLI operations
pub struct ProgressIndicator {
    #[allow(dead_code)]
    message: String,
}

impl ProgressIndicator {
    pub fn new(message: &str) -> Self {
        print!("{} {}...", "🔄".blue(), message);
        std::io::stdout().flush().unwrap();
        Self {
            message: message.to_string(),
        }
    }

    pub fn complete(&self, success: bool) {
        if success {
            println!(" {}", "✓".green());
        } else {
            println!(" {}", "✗".red());
        }
    }

    pub fn update(&self, new_message: &str) {
        print!("\r{} {}...", "🔄".blue(), new_message);
        std::io::stdout().flush().unwrap();
    }
}

/// Print a styled panel with title and content
pub fn print_panel(title: &str, content: &str, color: &str) {
    let colored_title = match color {
        "blue" => title.blue().bold(),
        "green" => title.green().bold(),
        "yellow" => title.yellow().bold(),
        "red" => title.red().bold(),
        "cyan" => title.cyan().bold(),
        _ => title.white().bold(),
    };

    println!("┌─────────────────────────────────────────┐");
    println!("│ {}                    │", colored_title);
    println!("│                                         │");
    for line in content.lines() {
        println!("│ {:<39} │", line);
    }
    println!("└─────────────────────────────────────────┘");
}

/// Recursively copy a directory
pub fn copy_dir_recursive(src: &PathBuf, dst: &PathBuf) -> Result<()> {
    if !src.exists() {
        return Ok(());
    }

    fs::create_dir_all(dst)?;

    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let entry_path = entry.path();
        let file_name = entry_path.file_name().unwrap();
        let dest_path = dst.join(file_name);

        if entry_path.is_dir() {
            copy_dir_recursive(&entry_path, &dest_path)?;
        } else {
            fs::copy(&entry_path, &dest_path)?;
        }
    }

    Ok(())
}

/// Get terminal width (defaults to 80 if detection fails)
#[cfg(feature = "cli")]
pub fn get_terminal_width() -> usize {
    // Try to get terminal width using ANSI escape codes
    // This is a simple implementation; more sophisticated detection
    // could use platform-specific APIs or terminal-query crates
    80 // Default fallback
}

/// Truncate text with ellipsis if it exceeds max length
pub fn truncate_with_ellipsis(text: &str, max_length: usize) -> String {
    if text.len() <= max_length {
        text.to_string()
    } else {
        format!("{}...", &text[..max_length.saturating_sub(3)])
    }
}

/// Format bytes with optional human-readable output
pub fn format_bytes(bytes: u64, human_readable: bool) -> String {
    if !human_readable {
        return bytes.to_string();
    }

    const KB: u64 = 1024;
    const MB: u64 = KB * 1024;
    const GB: u64 = MB * 1024;

    if bytes >= GB {
        format!("{:.2} GB", bytes as f64 / GB as f64)
    } else if bytes >= MB {
        format!("{:.2} MB", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{:.2} KB", bytes as f64 / KB as f64)
    } else {
        format!("{} B", bytes)
    }
}

/// Format timestamp as "X time ago"
#[cfg(feature = "cli")]
pub fn format_time_ago(timestamp: chrono::DateTime<chrono::Utc>) -> String {
    let now = chrono::Utc::now();
    let duration = now.signed_duration_since(timestamp);

    if duration.num_days() > 0 {
        format!("{} days ago", duration.num_days())
    } else if duration.num_hours() > 0 {
        format!("{} hours ago", duration.num_hours())
    } else if duration.num_minutes() > 0 {
        format!("{} minutes ago", duration.num_minutes())
    } else {
        "just now".to_string()
    }
}

/// Confirm user action with yes/no prompt
pub fn confirm_migration(old_path: &std::path::Path, new_path: &std::path::Path) -> Result<bool> {
    use std::io::{self, Write};
    print!(
        "Migrate from {} to {}? [y/N]: ",
        old_path.display(),
        new_path.display()
    );
    io::stdout().flush().unwrap();

    let mut input = String::new();
    io::stdin().read_line(&mut input)?;
    Ok(input.trim().to_lowercase() == "y")
}

/// Migrate files from old location to new location
pub fn migrate_from_old_location(old_path: &std::path::Path, new_path: &std::path::Path) -> Result<()> {
    use std::fs;
    
    println!("Migrating from {} to {}...", old_path.display(), new_path.display());
    copy_dir_recursive(&old_path.to_path_buf(), &new_path.to_path_buf())?;
    
    println!("Migration complete. Removing old installation...");
    fs::remove_dir_all(old_path)?;
    println!("Old installation removed.");
    Ok(())
}

/// Format project name with optional path display
pub fn format_project_name(
    project_name: &str,
    project_path: &std::path::Path,
    max_width: usize,
) -> String {
    use std::ffi::OsStr;
    // Try to canonicalize the project path. If that fails, try resolving it relative to the current directory.
    let display_path = match project_path.canonicalize() {
        Ok(p) => p,
        Err(_) => match std::env::current_dir() {
            Ok(cwd) => cwd
                .join(project_path)
                .canonicalize()
                .unwrap_or_else(|_| project_path.to_path_buf()),
            Err(_) => project_path.to_path_buf(),
        },
    };

    // If the original project_path is just "." (or empty), avoid showing "." — just show the project name.
    let show_path = if project_path.as_os_str() == OsStr::new(".") {
        None
    } else {
        Some(display_path.display().to_string())
    };

    let full_display = match show_path {
        Some(p) if !p.is_empty() => format!("{} ({})", project_name, p),
        _ => project_name.to_string(),
    };

    truncate_with_ellipsis(&full_display, max_width)
}

/// Format a session entry for display in listings
#[cfg(feature = "cli")]
pub fn format_session_entry(
    session_id: &str,
    checkpoint_count: usize,
    status: &str,
    status_color: &str,
    timestamp: &str,
    description: Option<&str>,
    verbose: bool,
    terminal_width: usize,
) -> Vec<String> {
    use colored::*;

    let mut lines = Vec::new();

    // Main session line with better formatting
    let session_id_colored = session_id.color(status_color);

    if verbose {
        let main_line = format!(
            "  {} │ {:3} ckpts │ {:>9} │ {}",
            session_id_colored,
            checkpoint_count,
            status.color(status_color),
            timestamp
        );
        lines.push(main_line);

        if let Some(desc) = description {
            let max_desc_width = terminal_width.saturating_sub(4); // Account for indent
            let truncated_desc = truncate_with_ellipsis(desc, max_desc_width);
            lines.push(format!("    {}", truncated_desc.dimmed()));
        }
    } else {
        let main_line = format!(
            "  {} │ {:3} checkpoints │ {}",
            session_id_colored, checkpoint_count, timestamp
        );
        lines.push(main_line);
    }

    lines
}

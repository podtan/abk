//! CommandContext adapter trait
//!
//! Provides access to configuration, logging, and filesystem operations
//! without coupling to specific implementations.

use crate::cli::error::CliResult;
use std::path::{Path, PathBuf};

/// Provides context for CLI command execution
///
/// This trait allows CLI commands to access configuration, logging,
/// and filesystem operations without depending on concrete types.
///
/// # Example
///
/// ```rust,ignore
/// use abk::cli::CommandContext;
/// use std::path::{Path, PathBuf};
///
/// struct MyContext {
///     config_path: PathBuf,
///     working_dir: PathBuf,
/// }
///
/// impl CommandContext for MyContext {
///     fn config_path(&self) -> CliResult<PathBuf> {
///         Ok(self.config_path.clone())
///     }
///
///     fn working_dir(&self) -> CliResult<PathBuf> {
///         Ok(self.working_dir.clone())
///     }
///
///     fn log_info(&self, message: &str) {
///         println!("[INFO] {}", message);
///     }
///
///     // ... implement remaining methods
/// }
/// ```
pub trait CommandContext {
    /// Get the path to the configuration file
    fn config_path(&self) -> CliResult<PathBuf>;

    /// Get the configuration object
    fn config(&self) -> &crate::config::Configuration;

    /// Load configuration as a typed object
    ///
    /// Returns the raw config value that can be deserialized by the caller
    fn load_config(&self) -> CliResult<serde_json::Value>;

    /// Get the current working directory
    fn working_dir(&self) -> CliResult<PathBuf>;

    /// Calculate hash for project identification
    ///
    /// Used for session/checkpoint storage paths
    fn project_hash(&self) -> CliResult<String>;

    /// Get the data directory for storing agent data
    ///
    /// Typically ~/.simpaticoder or equivalent
    fn data_dir(&self) -> CliResult<PathBuf>;

    /// Get the home directory
    ///
    /// Typically $HOME or equivalent
    fn home_dir(&self) -> CliResult<PathBuf> {
        std::env::var("HOME")
            .map(PathBuf::from)
            .map_err(|_| crate::cli::error::CliError::ConfigError(
                "Could not determine home directory".to_string()
            ))
    }

    /// Get the cache directory
    ///
    /// Used for temporary files and cached data
    fn cache_dir(&self) -> CliResult<PathBuf>;

    /// Log an informational message
    fn log_info(&self, message: &str);

    /// Log a warning message
    fn log_warn(&self, message: &str);

    /// Log an error message
    fn log_error(&self, message: &str) -> CliResult<()> {
        self.log_warn(message);
        Ok(())
    }

    /// Log a warning message (convenience wrapper)
    fn log_warning(&self, message: &str) -> CliResult<()> {
        self.log_warn(message);
        Ok(())
    }

    /// Log a success message
    fn log_success(&self, message: &str);

    /// Get terminal width for formatting
    fn terminal_width(&self) -> CliResult<usize> {
        Ok(80) // Default fallback
    }

    /// Format bytes for human-readable display
    fn format_bytes(&self, bytes: u64) -> CliResult<String> {
        const UNITS: &[&str] = &["B", "KB", "MB", "GB", "TB"];
        let mut size = bytes as f64;
        let mut unit_index = 0;
        
        while size >= 1024.0 && unit_index < UNITS.len() - 1 {
            size /= 1024.0;
            unit_index += 1;
        }
        
        Ok(if unit_index == 0 {
            format!("{} {}", bytes, UNITS[0])
        } else {
            format!("{:.2} {}", size, UNITS[unit_index])
        })
    }

    /// Format project name for display
    fn format_project_name(&self, project_path: &Path) -> CliResult<String> {
        Ok(project_path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("unknown")
            .to_string())
    }

    /// Format session entry for display
    fn format_session_entry(
        &self,
        session: &serde_json::Value,
        project_name: &str,
        _terminal_width: usize,
    ) -> CliResult<String> {
        let session_id = session.get("session_id")
            .and_then(|s| s.as_str())
            .unwrap_or("unknown");
        let status = session.get("status")
            .and_then(|s| s.as_str())
            .unwrap_or("unknown");
        let created = session.get("created_at")
            .and_then(|c| c.as_str())
            .unwrap_or("unknown");
        
        Ok(format!(
            "  {} | {} | {} | {}",
            session_id, project_name, status, created
        ))
    }

    /// Prompt user for confirmation
    fn confirm(&self, _prompt: &str) -> CliResult<bool> {
        // Default implementation: assume no confirmation
        // Real implementations should prompt the user
        Ok(false)
    }

    /// Check if a path exists
    fn path_exists(&self, path: &Path) -> bool {
        path.exists()
    }

    /// Read a line from stdin with prompt
    fn read_line(&self, prompt: &str) -> CliResult<String> {
        use std::io::{self, Write};
        print!("{}", prompt);
        io::stdout().flush()?;
        
        let mut input = String::new();
        io::stdin().read_line(&mut input)?;
        Ok(input)
    }

    /// Create an agent instance for executing tasks
    fn create_agent(&self) -> Result<crate::agent::Agent, Box<dyn std::error::Error>>;
}

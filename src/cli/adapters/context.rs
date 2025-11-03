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

    /// Get the cache directory
    ///
    /// Used for temporary files and cached data
    fn cache_dir(&self) -> CliResult<PathBuf>;

    /// Log an informational message
    fn log_info(&self, message: &str);

    /// Log a warning message
    fn log_warn(&self, message: &str);

    /// Log an error message
    fn log_error(&self, message: &str);

    /// Check if a path exists
    fn path_exists(&self, path: &Path) -> bool {
        path.exists()
    }
}

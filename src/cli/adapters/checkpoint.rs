//! CheckpointAccess adapter trait
//!
//! Provides access to checkpoint and session management operations.

use crate::cli::error::CliResult;
use async_trait::async_trait;
use std::path::PathBuf;

/// Information about a checkpoint
#[derive(Debug, Clone)]
pub struct CheckpointInfo {
    /// Unique identifier for the checkpoint
    pub id: String,
    /// Session ID this checkpoint belongs to
    pub session_id: String,
    /// Iteration number
    pub iteration: usize,
    /// Step within iteration
    pub step: String,
    /// Timestamp when created
    pub timestamp: String,
    /// Size in bytes
    pub size: u64,
    /// Path to checkpoint file
    pub path: PathBuf,
}

/// Information about a session
#[derive(Debug, Clone)]
pub struct SessionInfo {
    /// Unique session identifier
    pub id: String,
    /// Task description
    pub task: String,
    /// Session status
    pub status: String,
    /// Timestamp when created
    pub created_at: String,
    /// Timestamp when last modified
    pub updated_at: String,
    /// Number of iterations
    pub iterations: usize,
    /// Number of checkpoints
    pub checkpoint_count: usize,
    /// Path to session directory
    pub path: PathBuf,
}

/// Checkpoint data for export/import
#[derive(Debug, Clone)]
pub struct CheckpointData {
    /// Checkpoint metadata
    pub info: CheckpointInfo,
    /// Serialized checkpoint content
    pub content: String,
}

/// Provides access to checkpoint and session management
///
/// This trait wraps the existing `abk::checkpoint` functionality
/// through an async interface suitable for CLI commands.
///
/// # Example
///
/// ```rust,ignore
/// use abk::cli::CheckpointAccess;
/// use async_trait::async_trait;
///
/// struct MyCheckpointAdapter {
///     // ... fields
/// }
///
/// #[async_trait]
/// impl CheckpointAccess for MyCheckpointAdapter {
///     async fn list_sessions(&self) -> CliResult<Vec<SessionInfo>> {
///         // Implementation using abk::checkpoint
///         Ok(vec![])
///     }
///
///     // ... implement remaining methods
/// }
/// ```
#[async_trait]
pub trait CheckpointAccess: Send + Sync {
    /// List all sessions
    async fn list_sessions(&self) -> CliResult<Vec<SessionInfo>>;

    /// Get detailed information about a specific session
    async fn get_session(&self, session_id: &str) -> CliResult<SessionInfo>;

    /// List checkpoints for a session
    async fn list_checkpoints(&self, session_id: &str) -> CliResult<Vec<CheckpointInfo>>;

    /// Get detailed information about a specific checkpoint
    async fn get_checkpoint(&self, checkpoint_id: &str) -> CliResult<CheckpointInfo>;

    /// Load checkpoint data
    async fn load_checkpoint(&self, checkpoint_id: &str) -> CliResult<CheckpointData>;

    /// Export checkpoint to a file or string
    async fn export_checkpoint(&self, checkpoint_id: &str, destination: Option<&PathBuf>) -> CliResult<PathBuf>;

    /// Delete a checkpoint
    async fn delete_checkpoint(&self, checkpoint_id: &str) -> CliResult<()>;

    /// Delete an entire session
    async fn delete_session(&self, session_id: &str) -> CliResult<()>;
}

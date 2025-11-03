//! CheckpointAccess adapter trait
//!
//! Provides access to checkpoint and session management operations.

use crate::cli::error::CliResult;
use async_trait::async_trait;
use std::path::PathBuf;
use serde::{Deserialize, Serialize};

/// Project metadata information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectMetadata {
    pub name: String,
    pub project_path: PathBuf,
    pub project_hash: String,
}

/// Session metadata information (compatible with checkpoint module)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionMetadata {
    pub session_id: String,
    pub status: SessionStatus,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub last_accessed: chrono::DateTime<chrono::Utc>,
    pub description: Option<String>,
    pub tags: Vec<String>,
    pub checkpoint_count: usize,
}

/// Session status
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum SessionStatus {
    Active,
    Completed,
    Failed,
    Archived,
}

/// Checkpoint metadata information (compatible with checkpoint module)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CheckpointMetadata {
    pub checkpoint_id: String,
    pub session_id: String,
    pub workflow_step: String,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub iteration: usize,
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
    /// List all projects with checkpoint data
    async fn list_projects(&self) -> CliResult<Vec<ProjectMetadata>>;

    /// List sessions for a specific project
    async fn list_sessions(&self, project_path: &PathBuf) -> CliResult<Vec<SessionMetadata>>;

    /// List checkpoints for a session in a project
    async fn list_checkpoints(&self, project_path: &PathBuf, session_id: &str) -> CliResult<Vec<CheckpointMetadata>>;

    /// Delete a session from a project
    async fn delete_session(&self, project_path: &PathBuf, session_id: &str) -> CliResult<()>;

    /// Validate and optionally repair a session
    /// Returns list of actions taken if repair=true, or issues found if repair=false
    async fn validate_session(&self, project_path: &PathBuf, session_id: &str, repair: bool) -> CliResult<Vec<String>>;
}

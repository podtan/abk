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
    pub description: Option<String>,
    pub tags: Vec<String>,
}

/// Full checkpoint data for detailed operations
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CheckpointData {
    pub metadata: CheckpointMetadata,
    pub agent_state: AgentStateData,
    pub conversation_state: ConversationStateData,
    pub file_system_state: FileSystemStateData,
    pub tool_state: ToolStateData,
}

/// Agent state data
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentStateData {
    pub current_mode: String,
    pub current_step: String,
    pub working_directory: PathBuf,
    pub task_description: Option<String>,
}

/// Conversation state data
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConversationStateData {
    pub message_count: usize,
    pub total_tokens: usize,
}

/// File system state data
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileSystemStateData {
    pub working_directory: PathBuf,
    pub modified_files: Vec<String>,
}

/// Tool state data
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolStateData {
    pub executed_commands_count: usize,
}

/// Restored checkpoint result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RestoredCheckpoint {
    pub checkpoint: CheckpointData,
    pub restoration_metadata: RestorationMetadata,
}

/// Restoration metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RestorationMetadata {
    pub restored_at: chrono::DateTime<chrono::Utc>,
    pub restore_duration_ms: u64,
    pub warnings_count: usize,
    pub warnings: Vec<String>,
}

/// Agent restoration result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentResult {
    pub success: bool,
    pub warnings: Vec<String>,
    pub errors: Vec<String>,
}

/// Resume context for agent continuation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResumeContext {
    pub project_path: PathBuf,
    pub session_id: String,
    pub checkpoint_id: String,
    pub restored_at: chrono::DateTime<chrono::Utc>,
    pub working_directory: PathBuf,
    pub task_description: Option<String>,
    pub workflow_step: String,
    pub iteration: usize,
}

/// Checkpoint diff result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CheckpointDiff {
    pub from_checkpoint_id: String,
    pub to_checkpoint_id: String,
    pub time_difference_seconds: i64,
    pub mode_changed: bool,
    pub mode_from: String,
    pub mode_to: String,
    pub step_changed: bool,
    pub step_from: String,
    pub step_to: String,
    pub messages_diff: i32,
    pub tokens_diff: i32,
    pub files_diff: i32,
    pub commands_diff: i32,
    pub working_directory_changed: bool,
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

    /// Load full checkpoint data
    async fn load_checkpoint(&self, project_path: &PathBuf, session_id: &str, checkpoint_id: &str) -> CliResult<CheckpointData>;

    /// Delete a specific checkpoint
    async fn delete_checkpoint(&self, project_path: &PathBuf, session_id: &str, checkpoint_id: &str) -> CliResult<()>;

    /// Get diff between two checkpoints
    async fn get_checkpoint_diff(&self, project_path: &PathBuf, session_id: &str, from_checkpoint_id: &str, to_checkpoint_id: &str) -> CliResult<CheckpointDiff>;
}

/// Provides checkpoint restoration capabilities
///
/// This trait handles the complex restoration logic for resuming sessions.
/// Implementers should wrap their existing restoration infrastructure.
#[async_trait]
pub trait RestorationAccess: Send + Sync {
    /// Restore a checkpoint to disk
    async fn restore_checkpoint(&self, project_path: &PathBuf, session_id: &str, checkpoint_id: &str) -> CliResult<RestoredCheckpoint>;

    /// Restore agent state from checkpoint
    async fn restore_agent(&self, project_path: &PathBuf, session_id: &str, checkpoint_id: &str) -> CliResult<AgentResult>;

    /// Store resume context for future agent sessions
    async fn store_resume_context(&self, context: &ResumeContext) -> CliResult<()>;
}

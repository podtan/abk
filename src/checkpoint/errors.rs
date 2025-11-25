//! Error types for the checkpoint system

use std::path::PathBuf;
use thiserror::Error;

/// Result type for checkpoint operations
pub type CheckpointResult<T> = Result<T, CheckpointError>;

/// Comprehensive error types for checkpoint operations
#[derive(Error, Debug)]
pub enum CheckpointError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    #[error("Configuration error: {message}")]
    Config { message: String },

    #[error("Storage error: {message}")]
    Storage { message: String },

    #[error("Project not found: {path}")]
    ProjectNotFound { path: PathBuf },

    #[error("Session not found: {session_id}")]
    SessionNotFound { session_id: String },

    #[error("Checkpoint not found: {checkpoint_id} in session {session_id}")]
    CheckpointNotFound {
        checkpoint_id: String,
        session_id: String,
    },

    #[error("Project hash collision detected for paths: {path1} and {path2}")]
    HashCollision { path1: PathBuf, path2: PathBuf },

    #[error("Permission denied: {path}")]
    PermissionDenied { path: PathBuf },

    #[error("Storage quota exceeded: {current_size} bytes > {max_size} bytes")]
    StorageQuotaExceeded { current_size: u64, max_size: u64 },

    #[error("Checkpoint version mismatch: expected {expected}, found {found}")]
    VersionMismatch { expected: String, found: String },

    #[error("Corrupted checkpoint data: {message}")]
    CorruptedData { message: String },

    #[error("Invalid checkpoint ID format: {checkpoint_id}")]
    InvalidCheckpointId { checkpoint_id: String },

    #[error("Invalid session ID format: {session_id}")]
    InvalidSessionId { session_id: String },

    #[error("Atomic operation failed: {operation}")]
    AtomicOperationFailed { operation: String },

    #[error("Git operation failed: {message}")]
    GitError { message: String },

    #[error("Retention policy violation: {message}")]
    RetentionPolicyViolation { message: String },

    #[error("Migration error: {message}")]
    Migration { message: String },

    #[error("Restoration error: {message}")]
    Restoration { message: String },

    #[error("Validation error: {message}")]
    Validation { message: String },

    #[error("Other error: {message}")]
    Other { message: String },
}

impl CheckpointError {
    /// Create a config error
    pub fn config<S: Into<String>>(message: S) -> Self {
        Self::Config {
            message: message.into(),
        }
    }

    /// Create a storage error
    pub fn storage<S: Into<String>>(message: S) -> Self {
        Self::Storage {
            message: message.into(),
        }
    }

    /// Create a corrupted data error
    pub fn corrupted<S: Into<String>>(message: S) -> Self {
        Self::CorruptedData {
            message: message.into(),
        }
    }

    /// Create a validation error
    pub fn validation<S: Into<String>>(message: S) -> Self {
        Self::Validation {
            message: message.into(),
        }
    }

    /// Create a restoration error
    pub fn restoration<S: Into<String>>(message: S) -> Self {
        Self::Restoration {
            message: message.into(),
        }
    }

    /// Create a not found error (generic)
    pub fn not_found<S: Into<String>>(message: S) -> Self {
        Self::Other {
            message: message.into(),
        }
    }

    /// Create a git error
    pub fn git<S: Into<String>>(message: S) -> Self {
        Self::GitError {
            message: message.into(),
        }
    }

    /// Create an "other" error for generic error cases
    pub fn other<S: Into<String>>(message: S) -> Self {
        Self::Other {
            message: message.into(),
        }
    }

    /// Check if this error is recoverable (user can potentially fix it)
    pub fn is_recoverable(&self) -> bool {
        matches!(
            self,
            CheckpointError::Config { .. }
                | CheckpointError::ProjectNotFound { .. }
                | CheckpointError::SessionNotFound { .. }
                | CheckpointError::CheckpointNotFound { .. }
                | CheckpointError::PermissionDenied { .. }
                | CheckpointError::InvalidCheckpointId { .. }
                | CheckpointError::InvalidSessionId { .. }
                | CheckpointError::Validation { .. }
        )
    }

    /// Get a user-friendly error message with recovery suggestions
    pub fn user_friendly_message(&self) -> String {
        // Try to get agent name from environment, fallback to generic "agent"
        let agent_name = std::env::var("ABK_AGENT_NAME").unwrap_or_else(|_| "agent".to_string());
        
        match self {
            CheckpointError::Config { message } => {
                format!(
                    "Configuration error: {}. Please check your {} configuration.",
                    message, agent_name
                )
            }
            CheckpointError::ProjectNotFound { path } => {
                format!("Project not found at {}. Make sure you're running {} in a valid project directory.", path.display(), agent_name)
            }
            CheckpointError::SessionNotFound { session_id } => {
                format!("Session '{}' not found. Use '{} sessions list' to see available sessions.", session_id, agent_name)
            }
            CheckpointError::CheckpointNotFound {
                checkpoint_id,
                session_id,
            } => {
                format!("Checkpoint '{}' not found in session '{}'. Use '{} checkpoints list --session {}' to see available checkpoints.", checkpoint_id, session_id, agent_name, session_id)
            }
            CheckpointError::PermissionDenied { path } => {
                format!(
                    "Permission denied accessing {}. Please check file permissions.",
                    path.display()
                )
            }
            CheckpointError::StorageQuotaExceeded {
                current_size,
                max_size,
            } => {
                format!("Storage quota exceeded ({} bytes > {} bytes). Use '{} cache clean' to free up space.", current_size, max_size, agent_name)
            }
            CheckpointError::CorruptedData { message } => {
                format!("Corrupted checkpoint data: {}. You may need to delete and recreate this checkpoint.", message)
            }
            _ => self.to_string(),
        }
    }
}

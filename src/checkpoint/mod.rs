//! Agent Checkpoint System
//!
//! Comprehensive checkpoint and session management for software engineering agents.
//!
//! This crate provides:
//! - Session persistence and restoration
//! - Checkpoint storage with compression
//! - Retention policies and cleanup
//! - Project isolation via hash-based directories
//! - Atomic file operations
//!
//! All data is stored centrally in `~/.simpaticoder/` to avoid project directory pollution.
//!
//! ## Usage
//!
//! ```rust,no_run
//! use abk::checkpoint::{get_storage_manager, CheckpointResult};
//! use std::path::Path;
//!
//! async fn example() -> CheckpointResult<()> {
//!     let manager = get_storage_manager()?;
//!     let project_path = Path::new(".");
//!     let project_storage = manager.get_project_storage(project_path).await?;
//!     Ok(())
//! }
//! ```

pub mod atomic;
pub mod cleanup;
pub mod config;
pub mod errors;
pub mod models;
pub mod restoration;
pub mod resume_tracker;
pub mod size_calc;
pub mod storage;

// Re-export key types for convenience
pub use atomic::{AtomicFileWriter, AtomicOps, FileLock};
pub use cleanup::CleanupManager;
pub use config::{
    CleanupReport, ConfigMigrator, GlobalCheckpointConfig, MigrationReport,
    ProjectCheckpointConfig, ProjectConfigManager, ProjectStats, RetentionPolicy, SessionStats,
    StorageStats,
};
pub use errors::{CheckpointError, CheckpointResult};
pub use models::{
    AgentStateSnapshot, Checkpoint, CheckpointMetadata, CheckpointSummary, ConversationSnapshot,
    EnvironmentSnapshot, FileSystemSnapshot, ProjectHash, SessionMetadata, SessionStatus,
    ToolStateSnapshot,
};
pub use restoration::{
    CheckpointRestoration, RestorationMetadata, RestorationResult, RestoredCheckpoint,
    ValidationIssue, ValidationResults, ValidationSeverity,
};
pub use resume_tracker::{ResumeContext, ResumeTracker};
pub use size_calc::{SizeCategory, SizeInfo, SizeUtils, StorageSizeCalculator};
pub use storage::{CheckpointStorageManager, ProjectStorage, SessionStorage};

/// Initialize the checkpoint system
pub fn initialize() -> CheckpointResult<()> {
    // Create the global ~/.simpaticoder directory structure
    storage::ensure_global_storage_directories()?;
    Ok(())
}

/// Get the global checkpoint storage manager
pub fn get_storage_manager() -> CheckpointResult<CheckpointStorageManager> {
    CheckpointStorageManager::new()
}

/// Cleanup expired checkpoint data across all projects
pub async fn cleanup_expired_data() -> CheckpointResult<u32> {
    let manager = get_storage_manager()?;
    manager.cleanup_expired_data().await
}

/// Calculate total storage usage across all projects  
pub async fn calculate_total_storage_usage() -> CheckpointResult<u64> {
    let manager = get_storage_manager()?;
    let stats = manager.calculate_storage_usage().await?;
    Ok(stats.total_size)
}

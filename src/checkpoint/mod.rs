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
//! All data is stored centrally in `~/.{agent_name}/` to avoid project directory pollution.
//!
//! ## V2 Storage Format
//!
//! The v2 module provides a new split-file checkpoint format:
//! - `{NNN}_metadata.json` - Checkpoint metadata (small, queryable)
//! - `{NNN}_agent.json` - Agent state snapshot
//! - `{NNN}_conversation.json` - Conversation events
//! - `events.jsonl` - Append-only event log
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

pub mod agent_context;
pub mod atomic;
pub mod backend;
pub mod cleanup;
pub mod config;
pub mod errors;
pub mod models;
pub mod restoration;
pub mod resume_tracker;
pub mod session_manager;
pub mod size_calc;
pub mod storage;
pub mod utils;
pub mod v2;

// Re-export key types for convenience
pub use agent_context::AgentContext;
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
pub use session_manager::SessionManager;
pub use size_calc::{SizeCategory, SizeInfo, SizeUtils, StorageSizeCalculator};
pub use storage::{CheckpointStorageManager, ProjectStorage, SessionStorage};

// Backend re-exports for storage abstraction
pub use backend::{
    FileStorageBackend, ListOptions, ListResult, StorageBackend, StorageBackendBuilder,
    StorageBackendExt, StorageError, StorageItemMeta, StorageResult,
};

// V2 re-exports for split-file checkpoint format
pub use v2::{
    // Schemas
    AgentStateV2, CheckpointMetadataV2, CheckpointRefs, CheckpointsIndex,
    ConversationFileV2, SessionMetadataV2, SessionStatusV2, WorkflowStepV2,
    CHECKPOINT_VERSION_V2,
    // Storage
    ProjectStorageV2, SessionStorageV2,
    // Events
    EventEnvelope, EventType, EventsLog,
};

/// Initialize the checkpoint system
pub fn initialize() -> CheckpointResult<()> {
    // Create the global ~/.{agent_name} directory structure
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

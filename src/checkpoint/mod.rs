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
//! ## Storage Format (append-only)
//!
//! A session's durable state lives in a small set of append-only logs and
//! indexes — one shared "mainline" conversation log per session, plus a
//! per-checkpoint agent-state log. There are no per-checkpoint copies of the
//! conversation on the mainline path.
//!
//! **Local layout** (`projects/{project_hash}/sessions/{session_id}/`):
//! - `session_metadata.json` — session metadata (write-once, at session creation)
//! - `conversation.jsonl` — append-only conversation log, one
//!   [`crate::checkpoint::models::ChatMessage`] per line, shared by all
//!   checkpoints of the session (the mainline)
//! - `agent_state.jsonl` — append-only log, one agent-state entry per
//!   checkpoint iteration
//! - `checkpoints/checkpoints.json` — index of all `CheckpointMetadata`
//!   entries, each recording a `cursor_seq` into the conversation log
//! - `checkpoints/{checkpoint_id}_conversation.json` — full conversation
//!   snapshot, written only for diverged (forked) branches that cannot be
//!   safely appended to the shared mainline log
//!
//! **Remote layout** (when a storage backend such as DocumentDB is
//! configured) mirrors the same shape under path-like keys
//! (`projects/{project_hash}/...`):
//! - `messages/{seq:05}.json` — one document per conversation message (O(N)
//!   total documents, one per message)
//! - `state/{seq:05}.json` — one document per agent-state log entry
//! - `checkpoints/checkpoints.json` — the checkpoint index (with cursors)
//! - `metadata.json` — session metadata
//!
//! The DocumentDB backend stores these path-keyed documents in a single
//! collection, with the path as the document `_id`.
//!
//! **Fork semantics:** a checkpoint resumed from a non-latest checkpoint and
//! continued from there is a diverged branch; it is written as a full
//! `{checkpoint_id}_conversation.json` snapshot with `cursor_seq = 0` instead
//! of being appended to the shared log. Older sessions written in per-checkpoint
//! split-file formats remain readable via fallback loading.
//!
//! **Storage modes:** `local` (files only), `remote` (backend only), and
//! `mirror` (both — local files are the authoritative shared log).
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
pub mod agent_state_log;
pub mod atomic;
pub mod backend;
pub mod cleanup;
pub mod config;
pub mod conversation_log;
pub mod errors;
pub mod models;
pub mod restoration;
pub mod resume_tracker;
pub mod session_manager;
pub mod size_calc;
pub mod storage;
pub mod utils;

// Re-export key types for convenience
pub use agent_context::AgentContext;
pub use atomic::{AtomicFileWriter, AtomicOps, FileLock};
pub use cleanup::CleanupManager;
pub use config::{
    CleanupReport, ConfigMigrator, GlobalCheckpointConfig, MigrationReport,
    ProjectCheckpointConfig, ProjectConfigManager, ProjectStats, RetentionPolicy, SessionStats,
    StorageBackendConfig, StorageBackendType, StorageMode, StorageStats,
};
pub use errors::{CheckpointError, CheckpointResult};
pub use models::{
    AgentStateSnapshot, Checkpoint, CheckpointMetadata, CheckpointSummary, ConversationSnapshot,
    EnvironmentSnapshot, FileSystemSnapshot, SessionConstants, SessionMetadata, SessionStatus,
    ToolStateSnapshot, project_id_from_path,
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
pub use conversation_log::{ConversationLog, ConversationLogEntry, CONVERSATION_LOG_FILENAME};
pub use agent_state_log::{AgentStateEntry, AgentStateLog, AGENT_STATE_LOG_FILENAME};

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

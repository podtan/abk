//! StorageAccess adapter trait
//!
//! Provides access to cache and storage management operations.

use crate::cli::error::CliResult;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Storage usage statistics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageStats {
    pub total_size: u64,
    pub project_count: usize,
    pub session_count: usize,
    pub checkpoint_count: usize,
    pub projects: Vec<ProjectStorageStats>,
}

/// Per-project storage statistics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectStorageStats {
    pub project_path: PathBuf,
    pub size_bytes: u64,
    pub session_count: usize,
    pub checkpoint_count: usize,
}

/// Provides access to storage and cache management
///
/// This trait wraps storage-related operations for cache commands.
///
/// # Example
///
/// ```rust,ignore
/// use abk::cli::StorageAccess;
/// use async_trait::async_trait;
///
/// struct MyStorageAdapter {
///     // ... fields
/// }
///
/// #[async_trait]
/// impl StorageAccess for MyStorageAdapter {
///     async fn calculate_storage_usage(&self) -> CliResult<StorageStats> {
///         // Implementation
///         Ok(StorageStats {
///             total_size: 0,
///             project_count: 0,
///             session_count: 0,
///             checkpoint_count: 0,
///             projects: vec![],
///         })
///     }
///
///     async fn cleanup_expired_data(&self) -> CliResult<usize> {
///         // Implementation
///         Ok(0)
///     }
/// }
/// ```
#[async_trait]
pub trait StorageAccess: Send + Sync {
    /// Calculate storage usage across all projects
    async fn calculate_storage_usage(&self) -> CliResult<StorageStats>;

    /// Clean up expired or old data
    /// Returns the number of items cleaned up
    async fn cleanup_expired_data(&self) -> CliResult<usize>;
}

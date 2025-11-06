//! Storage access adapter for CLI commands

use crate::cli::error::{CliError, CliResult};
use async_trait::async_trait;
use std::path::PathBuf;

/// Statistics for a single project's storage
#[derive(Debug, Clone)]
pub struct ProjectStorageStats {
    pub project_path: PathBuf,
    pub size_bytes: u64,
    pub session_count: usize,
    pub checkpoint_count: usize,
}

/// Overall storage statistics
#[derive(Debug, Clone)]
pub struct StorageStats {
    pub total_size: u64,
    pub project_count: usize,
    pub session_count: usize,
    pub checkpoint_count: usize,
    pub projects: Vec<ProjectStorageStats>,
}

/// Trait for storage access operations
#[async_trait]
pub trait StorageAccess {
    async fn calculate_storage_usage(&self) -> CliResult<StorageStats>;
    async fn cleanup_expired_data(&self) -> CliResult<u32>;
}

/// Concrete implementation using abk::checkpoint
pub struct AbkStorageAccess;

impl AbkStorageAccess {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl StorageAccess for AbkStorageAccess {
    async fn calculate_storage_usage(&self) -> CliResult<StorageStats> {
        let manager = crate::checkpoint::get_storage_manager()
            .map_err(|e| CliError::CheckpointError(format!("Failed to get storage manager: {}", e)))?;
        
        let projects = manager.list_projects().await
            .map_err(|e| CliError::CheckpointError(format!("Failed to list projects: {}", e)))?;
        
        let mut total_size = 0u64;
        let mut total_sessions = 0usize;
        let mut total_checkpoints = 0usize;
        let mut project_stats = Vec::new();
        
        for project in projects {
            let project_storage = manager.get_project_storage(&project.project_path).await
                .map_err(|e| CliError::CheckpointError(format!("Failed to get project storage: {}", e)))?;
            
            let sessions = project_storage.list_sessions().await
                .map_err(|e| CliError::CheckpointError(format!("Failed to list sessions: {}", e)))?;
            
            let mut project_size = 0u64;
            let mut checkpoint_count = 0usize;
            
            for session in &sessions {
                let session_storage = project_storage.create_session(&session.session_id).await
                    .map_err(|e| CliError::CheckpointError(format!("Failed to get session storage: {}", e)))?;
                
                let checkpoints = session_storage.list_checkpoints().await
                    .map_err(|e| CliError::CheckpointError(format!("Failed to list checkpoints: {}", e)))?;
                
                checkpoint_count += checkpoints.len();
                
                // Estimate size based on checkpoint count (rough approximation)
                project_size += (checkpoints.len() as u64) * 1024 * 1024; // 1MB per checkpoint estimate
            }
            
            total_size += project_size;
            total_sessions += sessions.len();
            total_checkpoints += checkpoint_count;
            
            project_stats.push(ProjectStorageStats {
                project_path: project.project_path,
                size_bytes: project_size,
                session_count: sessions.len(),
                checkpoint_count,
            });
        }
        
        Ok(StorageStats {
            total_size,
            project_count: project_stats.len(),
            session_count: total_sessions,
            checkpoint_count: total_checkpoints,
            projects: project_stats,
        })
    }
    
    async fn cleanup_expired_data(&self) -> CliResult<u32> {
        // TODO: Implement actual cleanup logic
        // For now, return 0 (no cleanup performed)
        Ok(0)
    }
}

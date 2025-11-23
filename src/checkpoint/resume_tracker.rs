//! Resume context tracking for checkpoint restoration
//!
//! This module provides functionality to track when checkpoints have been restored
//! so that subsequent agent sessions can automatically use the restored context
//! instead of starting fresh.

use super::{CheckpointError, CheckpointResult};
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Information about a restored checkpoint context
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResumeContext {
    pub project_path: PathBuf,
    pub session_id: String,
    pub checkpoint_id: String,
    pub restored_at: DateTime<Utc>,
    pub working_directory: PathBuf,
    pub task_description: String,
    pub workflow_step: String,
    pub iteration: u32,
}

/// Manages tracking of restored checkpoint contexts
pub struct ResumeTracker {
    tracker_file: PathBuf,
}

impl ResumeTracker {
    /// Create a new resume tracker
    pub fn new() -> CheckpointResult<Self> {
        let agent_name = std::env::var("ABK_AGENT_NAME").unwrap_or_else(|_| "simpaticoder".to_string());
        let dir_name = format!(".{}", agent_name);
        
        let home_dir = std::env::var("HOME")
            .map_err(|_| CheckpointError::storage("Could not determine home directory"))?;
        let tracker_file = PathBuf::from(home_dir)
            .join(&dir_name)
            .join("last_resume.json");

        // Ensure parent directory exists
        if let Some(parent) = tracker_file.parent() {
            std::fs::create_dir_all(parent).map_err(|e| {
                CheckpointError::storage(format!(
                    "Failed to create resume tracker directory: {}",
                    e
                ))
            })?;
        }

        Ok(Self { tracker_file })
    }

    /// Store information about a restored checkpoint
    pub fn store_resume_context(&self, context: &ResumeContext) -> CheckpointResult<()> {
        let json = serde_json::to_string_pretty(context).map_err(|e| {
            CheckpointError::storage(format!("Failed to serialize resume context: {}", e))
        })?;

        std::fs::write(&self.tracker_file, json).map_err(|e| {
            CheckpointError::storage(format!("Failed to write resume context: {}", e))
        })?;

        Ok(())
    }

    /// Check if there's a valid resume context for the given project path
    pub fn get_resume_context_for_project(
        &self,
        project_path: &Path,
    ) -> CheckpointResult<Option<ResumeContext>> {
        if !self.tracker_file.exists() {
            return Ok(None);
        }

        let content = std::fs::read_to_string(&self.tracker_file).map_err(|e| {
            CheckpointError::storage(format!("Failed to read resume context: {}", e))
        })?;

        let context: ResumeContext = serde_json::from_str(&content).map_err(|e| {
            CheckpointError::storage(format!("Failed to parse resume context: {}", e))
        })?;

        // Check if this context is for the current project
        let current_project = project_path
            .canonicalize()
            .unwrap_or_else(|_| project_path.to_path_buf());
        let context_project = context
            .project_path
            .canonicalize()
            .unwrap_or_else(|_| context.project_path.clone());

        if current_project != context_project {
            return Ok(None);
        }

        // Check if the context is still fresh (within 1 hour)
        let now = Utc::now();
        let age = now.signed_duration_since(context.restored_at);
        if age > Duration::hours(1) {
            // Context is too old, clean it up
            self.clear_resume_context()?;
            return Ok(None);
        }

        Ok(Some(context))
    }

    /// Clear the resume context (typically after successful use)
    pub fn clear_resume_context(&self) -> CheckpointResult<()> {
        if self.tracker_file.exists() {
            std::fs::remove_file(&self.tracker_file).map_err(|e| {
                CheckpointError::storage(format!("Failed to clear resume context: {}", e))
            })?;
        }
        Ok(())
    }

    /// Check if there's any resume context available (for any project)
    pub fn has_any_resume_context(&self) -> bool {
        self.tracker_file.exists()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_resume_tracker_basic_operations() {
        let temp_dir = TempDir::new().unwrap();
        std::env::set_var("HOME", temp_dir.path().to_str().unwrap());

        let tracker = ResumeTracker::new().unwrap();

        let context = ResumeContext {
            project_path: PathBuf::from("/test/project"),
            session_id: "test_session".to_string(),
            checkpoint_id: "001_analyze".to_string(),
            restored_at: Utc::now(),
            working_directory: PathBuf::from("/test/project"),
            task_description: "Test task".to_string(),
            workflow_step: "Analyze".to_string(),
            iteration: 1,
        };

        // Store context
        tracker.store_resume_context(&context).unwrap();
        assert!(tracker.has_any_resume_context());

        // Retrieve context for same project
        let retrieved = tracker
            .get_resume_context_for_project(&PathBuf::from("/test/project"))
            .unwrap();
        assert!(retrieved.is_some());
        assert_eq!(retrieved.as_ref().unwrap().session_id, "test_session");

        // Clear context
        tracker.clear_resume_context().unwrap();
        assert!(!tracker.has_any_resume_context());
    }

    #[test]
    fn test_resume_context_expiration() {
        let temp_dir = TempDir::new().unwrap();
        std::env::set_var("HOME", temp_dir.path().to_str().unwrap());

        let tracker = ResumeTracker::new().unwrap();

        let old_context = ResumeContext {
            project_path: PathBuf::from("/test/project"),
            session_id: "test_session".to_string(),
            checkpoint_id: "001_analyze".to_string(),
            restored_at: Utc::now() - Duration::hours(2), // 2 hours ago
            working_directory: PathBuf::from("/test/project"),
            task_description: "Test task".to_string(),
            workflow_step: "Analyze".to_string(),
            iteration: 1,
        };

        // Store old context
        tracker.store_resume_context(&old_context).unwrap();

        // Should return None due to expiration
        let retrieved = tracker
            .get_resume_context_for_project(&PathBuf::from("/test/project"))
            .unwrap();
        assert!(retrieved.is_none());

        // Context should be automatically cleared
        assert!(!tracker.has_any_resume_context());
    }
}

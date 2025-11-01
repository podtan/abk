//! Cleanup operations for checkpoint storage management

use super::{
    CheckpointResult, CleanupReport, RetentionPolicy, SessionMetadata, SessionStatus,
    StorageSizeCalculator, AtomicOps, SizeUtils,
};
use chrono::{Duration, Utc};
use std::path::{Path, PathBuf};
use tokio::fs;

/// Cleanup manager for checkpoint storage
pub struct CleanupManager {
    storage_root: PathBuf,
    size_calculator: StorageSizeCalculator,
    dry_run: bool,
    verbose: bool,
}

impl CleanupManager {
    /// Create a new cleanup manager
    pub fn new(storage_root: PathBuf, dry_run: bool, verbose: bool) -> Self {
        Self {
            storage_root,
            size_calculator: StorageSizeCalculator::new(300), // 5 minute cache TTL
            dry_run,
            verbose,
        }
    }

    /// Run comprehensive cleanup based on retention policy
    pub async fn run_cleanup(
        &mut self,
        retention: &RetentionPolicy,
    ) -> CheckpointResult<CleanupReport> {
        let start_time = std::time::Instant::now();
        let mut deleted_sessions = 0u32;
        let mut deleted_checkpoints = 0u32;
        let mut freed_bytes = 0u64;
        let mut errors = Vec::new();

        if self.verbose {
            println!("🧹 Starting cleanup with retention policy");
            if self.dry_run {
                println!("   Running in dry-run mode - no actual deletion");
            }
        }

        // 1. Clean up expired sessions
        match self.cleanup_expired_sessions(retention).await {
            Ok((sessions, checkpoints, bytes)) => {
                deleted_sessions += sessions;
                deleted_checkpoints += checkpoints;
                freed_bytes += bytes;

                if self.verbose {
                    println!(
                        "   Cleaned {} expired sessions ({} checkpoints, {})",
                        sessions,
                        checkpoints,
                        format_bytes(bytes)
                    );
                }
            }
            Err(e) => errors.push(format!("Failed to clean expired sessions: {}", e)),
        }

        // 2. Enforce storage quotas if configured
        if let Some(max_size_gb) = retention.max_total_size_gb {
            match self
                .enforce_storage_quota(max_size_gb as u64 * 1024 * 1024 * 1024)
                .await
            {
                Ok((sessions, checkpoints, bytes)) => {
                    deleted_sessions += sessions;
                    deleted_checkpoints += checkpoints;
                    freed_bytes += bytes;

                    if self.verbose && sessions > 0 {
                        println!(
                            "   Quota cleanup: {} sessions ({} checkpoints, {})",
                            sessions,
                            checkpoints,
                            format_bytes(bytes)
                        );
                    }
                }
                Err(e) => errors.push(format!("Failed to enforce storage quota: {}", e)),
            }
        }

        // 3. Clean up empty directories
        match self.cleanup_empty_directories().await {
            Ok(dirs_removed) => {
                if self.verbose && dirs_removed > 0 {
                    println!("   Removed {} empty directories", dirs_removed);
                }
            }
            Err(e) => errors.push(format!("Failed to clean empty directories: {}", e)),
        }

        // 4. Clean up temporary and orphaned files
        match self.cleanup_temporary_files().await {
            Ok(temp_bytes) => {
                freed_bytes += temp_bytes;
                if self.verbose && temp_bytes > 0 {
                    println!(
                        "   Cleaned up {} of temporary files",
                        format_bytes(temp_bytes)
                    );
                }
            }
            Err(e) => errors.push(format!("Failed to clean temporary files: {}", e)),
        }

        let duration = start_time.elapsed();

        Ok(CleanupReport {
            deleted_sessions,
            deleted_checkpoints,
            freed_bytes,
            duration: Duration::from_std(duration).unwrap(),
            errors,
        })
    }

    /// Clean up expired sessions based on retention policy
    async fn cleanup_expired_sessions(
        &mut self,
        retention: &RetentionPolicy,
    ) -> CheckpointResult<(u32, u32, u64)> {
        let mut deleted_sessions = 0u32;
        let mut deleted_checkpoints = 0u32;
        let mut freed_bytes = 0u64;

        let projects_dir = self.storage_root.join("projects");
        if !projects_dir.exists() {
            return Ok((0, 0, 0));
        }

        let mut entries = fs::read_dir(&projects_dir).await?;
        while let Some(project_entry) = entries.next_entry().await? {
            if !project_entry.file_type().await?.is_dir() {
                continue;
            }

            let sessions_dir = project_entry.path().join("sessions");
            if !sessions_dir.exists() {
                continue;
            }

            let mut session_entries = fs::read_dir(&sessions_dir).await?;
            while let Some(session_entry) = session_entries.next_entry().await? {
                if !session_entry.file_type().await?.is_dir() {
                    continue;
                }

                let session_path = session_entry.path();
                let should_delete = match self.should_delete_session(&session_path, retention).await
                {
                    Ok(should_delete) => should_delete,
                    Err(_) => continue, // Skip sessions with errors
                };

                if should_delete {
                    let session_size = self
                        .calculate_session_size(&session_path)
                        .await
                        .unwrap_or(0);
                    let checkpoint_count = self.count_checkpoints(&session_path).await.unwrap_or(0);

                    if !self.dry_run {
                        if let Err(e) = fs::remove_dir_all(&session_path).await {
                            if self.verbose {
                                println!(
                                    "   Warning: Failed to delete {}: {}",
                                    session_path.display(),
                                    e
                                );
                            }
                            continue;
                        }
                    }

                    deleted_sessions += 1;
                    deleted_checkpoints += checkpoint_count;
                    freed_bytes += session_size;

                    if self.verbose {
                        println!(
                            "   {} session: {} ({} checkpoints, {})",
                            if self.dry_run {
                                "Would delete"
                            } else {
                                "Deleted"
                            },
                            session_entry.file_name().to_string_lossy(),
                            checkpoint_count,
                            format_bytes(session_size)
                        );
                    }
                }
            }
        }

        Ok((deleted_sessions, deleted_checkpoints, freed_bytes))
    }

    /// Determine if a session should be deleted based on retention policy
    async fn should_delete_session(
        &self,
        session_path: &Path,
        retention: &RetentionPolicy,
    ) -> CheckpointResult<bool> {
        let metadata_path = session_path.join("metadata.json");
        if !metadata_path.exists() {
            // Delete sessions without metadata (corrupted/orphaned)
            return Ok(true);
        }

        let metadata: SessionMetadata = AtomicOps::read_json(&metadata_path)?;

        // Never delete active sessions if configured
        if retention.preserve_active_sessions && matches!(metadata.status, SessionStatus::Active) {
            return Ok(false);
        }

        // Never delete tagged sessions if configured
        if retention.preserve_tagged && !metadata.tags.is_empty() {
            return Ok(false);
        }

        // Check age limit
        if let Some(max_age_days) = retention.max_age_days {
            let age = Utc::now().signed_duration_since(metadata.created_at);
            if age.num_days() > max_age_days as i64 {
                return Ok(true);
            }
        }

        Ok(false)
    }

    /// Enforce storage quota by deleting oldest sessions
    async fn enforce_storage_quota(&mut self, max_bytes: u64) -> CheckpointResult<(u32, u32, u64)> {
        let total_size = self.calculate_total_storage_size().await?;

        if total_size <= max_bytes {
            return Ok((0, 0, 0)); // No cleanup needed
        }

        let bytes_to_free = total_size - max_bytes;
        let mut sessions_to_delete = self.get_sessions_by_age().await?;

        // Sort by age (oldest first) but preserve tagged sessions
        sessions_to_delete.sort_by_key(|s| s.1.created_at);

        let mut deleted_sessions = 0u32;
        let mut deleted_checkpoints = 0u32;
        let mut freed_bytes = 0u64;

        for (session_path, metadata) in sessions_to_delete {
            if freed_bytes >= bytes_to_free {
                break;
            }

            // Skip tagged sessions
            if !metadata.tags.is_empty() {
                continue;
            }

            // Skip active sessions
            if matches!(metadata.status, SessionStatus::Active) {
                continue;
            }

            let session_size = self
                .calculate_session_size(&session_path)
                .await
                .unwrap_or(0);
            let checkpoint_count = self.count_checkpoints(&session_path).await.unwrap_or(0);

            if !self.dry_run {
                fs::remove_dir_all(&session_path).await?;
            }

            deleted_sessions += 1;
            deleted_checkpoints += checkpoint_count;
            freed_bytes += session_size;

            if self.verbose {
                println!(
                    "   {} session for quota: {} ({} checkpoints, {})",
                    if self.dry_run {
                        "Would delete"
                    } else {
                        "Deleted"
                    },
                    session_path
                        .file_name()
                        .unwrap_or_default()
                        .to_string_lossy(),
                    checkpoint_count,
                    format_bytes(session_size)
                );
            }
        }

        Ok((deleted_sessions, deleted_checkpoints, freed_bytes))
    }

    /// Clean up empty directories
    async fn cleanup_empty_directories(&self) -> CheckpointResult<u32> {
        let mut removed_count = 0u32;
        self.cleanup_empty_dirs_recursive(&self.storage_root, &mut removed_count)
            .await?;
        Ok(removed_count)
    }

    /// Recursively clean up empty directories
    fn cleanup_empty_dirs_recursive<'a>(
        &'a self,
        dir_path: &'a Path,
        removed_count: &'a mut u32,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = CheckpointResult<()>> + Send + 'a>>
    {
        Box::pin(async move {
            if !dir_path.exists() {
                return Ok(());
            }

            let mut entries = fs::read_dir(dir_path).await?;
            let mut _has_entries = false;
            let mut subdirs = Vec::new();

            while let Some(entry) = entries.next_entry().await? {
                _has_entries = true;

                if entry.file_type().await?.is_dir() {
                    subdirs.push(entry.path());
                }
            }

            // Process subdirectories first
            for subdir in subdirs {
                self.cleanup_empty_dirs_recursive(&subdir, removed_count)
                    .await?;
            }

            // Check if directory is now empty (after subdirectory cleanup)
            let mut entries_check = fs::read_dir(dir_path).await?;
            let is_empty = entries_check.next_entry().await?.is_none();

            if is_empty && dir_path != self.storage_root {
                // Don't delete the root storage directory
                if !self.dry_run {
                    fs::remove_dir(dir_path).await?;
                }
                *removed_count += 1;

                if self.verbose {
                    println!(
                        "   {} empty directory: {}",
                        if self.dry_run {
                            "Would remove"
                        } else {
                            "Removed"
                        },
                        dir_path.display()
                    );
                }
            }

            Ok(())
        })
    }

    /// Clean up temporary and orphaned files
    async fn cleanup_temporary_files(&self) -> CheckpointResult<u64> {
        let mut freed_bytes = 0u64;

        // Patterns for temporary files
        let temp_patterns = [
            "*.tmp",
            "*.temp",
            "*.lock",
            "*.backup",
            ".DS_Store",
            "Thumbs.db",
        ];

        self.cleanup_temp_files_recursive(&self.storage_root, &temp_patterns, &mut freed_bytes)
            .await?;
        Ok(freed_bytes)
    }

    /// Recursively clean up temporary files
    fn cleanup_temp_files_recursive<'a>(
        &'a self,
        dir_path: &'a Path,
        patterns: &'a [&'a str],
        freed_bytes: &'a mut u64,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = CheckpointResult<()>> + Send + 'a>>
    {
        Box::pin(async move {
            if !dir_path.exists() {
                return Ok(());
            }

            let mut entries = fs::read_dir(dir_path).await?;

            while let Some(entry) = entries.next_entry().await? {
                let entry_path = entry.path();
                let entry_type = entry.file_type().await?;

                if entry_type.is_dir() {
                    self.cleanup_temp_files_recursive(&entry_path, patterns, freed_bytes)
                        .await?;
                } else if entry_type.is_file() {
                    let filename = entry_path
                        .file_name()
                        .and_then(|n| n.to_str())
                        .unwrap_or("");

                    let should_delete = patterns.iter().any(|pattern| {
                        if pattern.starts_with('*') {
                            let suffix = &pattern[1..];
                            filename.ends_with(suffix)
                        } else {
                            filename == *pattern
                        }
                    });

                    if should_delete {
                        let file_size = entry.metadata().await?.len();

                        if !self.dry_run {
                            if let Err(e) = fs::remove_file(&entry_path).await {
                                if self.verbose {
                                    println!(
                                        "   Warning: Failed to delete temp file {}: {}",
                                        entry_path.display(),
                                        e
                                    );
                                }
                                continue;
                            }
                        }

                        *freed_bytes += file_size;

                        if self.verbose {
                            println!(
                                "   {} temp file: {} ({})",
                                if self.dry_run {
                                    "Would delete"
                                } else {
                                    "Deleted"
                                },
                                filename,
                                format_bytes(file_size)
                            );
                        }
                    }
                }
            }

            Ok(())
        })
    }

    /// Calculate total storage size
    async fn calculate_total_storage_size(&mut self) -> CheckpointResult<u64> {
        let size_info = self
            .size_calculator
            .calculate_size(&self.storage_root)
            .await?;
        Ok(size_info.size_bytes)
    }

    /// Calculate size of a specific session
    async fn calculate_session_size(&mut self, session_path: &Path) -> CheckpointResult<u64> {
        let size_info = self.size_calculator.calculate_size(session_path).await?;
        Ok(size_info.size_bytes)
    }

    /// Count checkpoints in a session
    async fn count_checkpoints(&self, session_path: &Path) -> CheckpointResult<u32> {
        let mut count = 0u32;
        let mut entries = fs::read_dir(session_path).await?;

        while let Some(entry) = entries.next_entry().await? {
            if entry.file_type().await?.is_file() {
                let filename = entry.file_name();
                if let Some(name) = filename.to_str() {
                    if name.ends_with(".json")
                        && name != "metadata.json"
                        && name != "checkpoints.json"
                    {
                        count += 1;
                    }
                }
            }
        }

        Ok(count)
    }

    /// Get all sessions sorted by age
    async fn get_sessions_by_age(&self) -> CheckpointResult<Vec<(PathBuf, SessionMetadata)>> {
        let mut sessions = Vec::new();
        let projects_dir = self.storage_root.join("projects");

        if !projects_dir.exists() {
            return Ok(sessions);
        }

        let mut entries = fs::read_dir(&projects_dir).await?;
        while let Some(project_entry) = entries.next_entry().await? {
            if !project_entry.file_type().await?.is_dir() {
                continue;
            }

            let sessions_dir = project_entry.path().join("sessions");
            if !sessions_dir.exists() {
                continue;
            }

            let mut session_entries = fs::read_dir(&sessions_dir).await?;
            while let Some(session_entry) = session_entries.next_entry().await? {
                if !session_entry.file_type().await?.is_dir() {
                    continue;
                }

                let session_path = session_entry.path();
                let metadata_path = session_path.join("metadata.json");

                if let Ok(metadata) =
                    AtomicOps::read_json::<SessionMetadata>(&metadata_path)
                {
                    sessions.push((session_path, metadata));
                }
            }
        }

        Ok(sessions)
    }
}

/// Format bytes for display
fn format_bytes(bytes: u64) -> String {
    SizeUtils::format_bytes(bytes, false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;
    use tokio::fs;

    #[tokio::test]
    async fn test_cleanup_empty_directories() {
        let temp_dir = tempdir().unwrap();
        let storage_path = temp_dir.path().to_path_buf();

        // Create some empty directories
        let empty_dir1 = storage_path.join("empty1");
        let empty_dir2 = storage_path.join("empty2");
        fs::create_dir_all(&empty_dir1).await.unwrap();
        fs::create_dir_all(&empty_dir2).await.unwrap();

        // Create a directory with files
        let filled_dir = storage_path.join("filled");
        fs::create_dir_all(&filled_dir).await.unwrap();
        fs::write(filled_dir.join("file.txt"), "content")
            .await
            .unwrap();

        let cleanup_manager = CleanupManager::new(storage_path, false, false);
        let removed_count = cleanup_manager.cleanup_empty_directories().await.unwrap();

        assert_eq!(removed_count, 2);
        assert!(!empty_dir1.exists());
        assert!(!empty_dir2.exists());
        assert!(filled_dir.exists());
    }

    #[tokio::test]
    async fn test_cleanup_temporary_files() {
        let temp_dir = tempdir().unwrap();
        let storage_path = temp_dir.path().to_path_buf();

        // Create some temporary files
        fs::write(storage_path.join("test.tmp"), "temp content")
            .await
            .unwrap();
        fs::write(storage_path.join("backup.backup"), "backup content")
            .await
            .unwrap();
        fs::write(storage_path.join("normal.txt"), "normal content")
            .await
            .unwrap();

        let cleanup_manager = CleanupManager::new(storage_path.clone(), false, false);
        let freed_bytes = cleanup_manager.cleanup_temporary_files().await.unwrap();

        // Check that some bytes were freed (should be at least 24 bytes)
        assert!(
            freed_bytes >= 24,
            "Expected at least 24 bytes to be freed, got {}",
            freed_bytes
        );
        assert!(!storage_path.join("test.tmp").exists());
        assert!(!storage_path.join("backup.backup").exists());
        assert!(storage_path.join("normal.txt").exists());
    }
}

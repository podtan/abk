//! Atomic file operations for safe checkpoint data handling

use super::{CheckpointError, CheckpointResult};
use std::fs;
use std::path::{Path, PathBuf};
use uuid::Uuid;

/// Atomic file writer that uses write-temp-rename pattern
pub struct AtomicFileWriter {
    target_path: PathBuf,
    temp_path: PathBuf,
}

impl AtomicFileWriter {
    /// Create a new atomic file writer for the target path
    pub fn new(target_path: &Path) -> CheckpointResult<Self> {
        let temp_path = Self::generate_temp_path(target_path)?;

        Ok(AtomicFileWriter {
            target_path: target_path.to_path_buf(),
            temp_path,
        })
    }

    /// Write content to the file atomically
    pub fn write_content(&self, content: &str) -> CheckpointResult<()> {
        // Ensure parent directory exists
        if let Some(parent) = self.target_path.parent() {
            fs::create_dir_all(parent)?;
        }

        // Write to temporary file
        fs::write(&self.temp_path, content)?;

        // Atomic rename
        self.commit()?;

        Ok(())
    }

    /// Write JSON data to the file atomically
    pub fn write_json<T: serde::Serialize>(&self, data: &T) -> CheckpointResult<()> {
        let content = serde_json::to_string_pretty(data)?;
        self.write_content(&content)
    }

    /// Commit the write by renaming temp file to target
    pub fn commit(&self) -> CheckpointResult<()> {
        fs::rename(&self.temp_path, &self.target_path).map_err(|e| {
            CheckpointError::storage(format!("Failed to commit atomic write: {}", e))
        })?;
        Ok(())
    }

    /// Abort the write by deleting the temp file
    pub fn abort(&self) -> CheckpointResult<()> {
        if self.temp_path.exists() {
            fs::remove_file(&self.temp_path)?;
        }
        Ok(())
    }

    /// Generate a unique temporary file path
    fn generate_temp_path(target: &Path) -> CheckpointResult<PathBuf> {
        let parent = target.parent().ok_or_else(|| {
            CheckpointError::storage("Target path has no parent directory".to_string())
        })?;

        let filename = target
            .file_name()
            .ok_or_else(|| CheckpointError::storage("Target path has no filename".to_string()))?;

        let temp_name = format!(
            "{}.tmp.{}",
            filename.to_string_lossy(),
            Uuid::new_v4().to_string()
        );

        Ok(parent.join(temp_name))
    }
}

impl Drop for AtomicFileWriter {
    fn drop(&mut self) {
        // Clean up temp file if it still exists
        let _ = self.abort();
    }
}

/// Atomic file operations utility functions
pub struct AtomicOps;

impl AtomicOps {
    /// Atomically write string content to a file
    pub fn write_file(path: &Path, content: &str) -> CheckpointResult<()> {
        let writer = AtomicFileWriter::new(path)?;
        writer.write_content(content)
    }

    /// Atomically write JSON data to a file
    pub fn write_json<T: serde::Serialize>(path: &Path, data: &T) -> CheckpointResult<()> {
        let writer = AtomicFileWriter::new(path)?;
        writer.write_json(data)
    }

    /// Atomically read JSON data from a file
    pub fn read_json<T: serde::de::DeserializeOwned>(path: &Path) -> CheckpointResult<T> {
        let content = fs::read_to_string(path)?;
        let data = serde_json::from_str(&content)?;
        Ok(data)
    }

    /// Check if a file can be safely read/written (basic locking check)
    pub fn is_file_accessible(path: &Path) -> bool {
        if !path.exists() {
            return true; // New files are always accessible
        }

        // Try to open for reading to check if file is locked
        match fs::File::open(path) {
            Ok(_) => true,
            Err(_) => false,
        }
    }

    /// Create a backup of a file before modification
    pub fn create_backup(path: &Path) -> CheckpointResult<PathBuf> {
        if !path.exists() {
            return Err(CheckpointError::storage(
                "File does not exist for backup".to_string(),
            ));
        }

        let backup_path = Self::generate_backup_path(path)?;
        fs::copy(path, &backup_path)?;

        Ok(backup_path)
    }

    /// Restore a file from backup
    pub fn restore_backup(backup_path: &Path, target_path: &Path) -> CheckpointResult<()> {
        if !backup_path.exists() {
            return Err(CheckpointError::storage(
                "Backup file does not exist".to_string(),
            ));
        }

        fs::copy(backup_path, target_path)?;
        fs::remove_file(backup_path)?; // Clean up backup

        Ok(())
    }

    /// Generate backup file path
    fn generate_backup_path(original: &Path) -> CheckpointResult<PathBuf> {
        let parent = original.parent().ok_or_else(|| {
            CheckpointError::storage("Original path has no parent directory".to_string())
        })?;

        let filename = original
            .file_name()
            .ok_or_else(|| CheckpointError::storage("Original path has no filename".to_string()))?;

        let backup_name = format!(
            "{}.backup.{}",
            filename.to_string_lossy(),
            chrono::Utc::now().format("%Y%m%d_%H%M%S")
        );

        Ok(parent.join(backup_name))
    }
}

/// File lock for concurrent access protection
pub struct FileLock {
    _lock_file: PathBuf,
}

impl FileLock {
    /// Try to acquire a lock for a file
    pub fn try_acquire(target_path: &Path) -> CheckpointResult<Option<FileLock>> {
        let lock_path = Self::get_lock_path(target_path);

        // Try to create lock file exclusively (fails if already exists)
        match fs::OpenOptions::new()
            .write(true)
            .create_new(true) // This will fail if file already exists
            .open(&lock_path)
        {
            Ok(mut file) => {
                use std::io::Write;
                let _ = file.write_all(std::process::id().to_string().as_bytes());
                Ok(Some(FileLock {
                    _lock_file: lock_path,
                }))
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => Ok(None),
            Err(e) => Err(CheckpointError::from(e)),
        }
    }

    /// Wait for lock with timeout
    pub fn wait_for_lock(target_path: &Path, timeout_seconds: u64) -> CheckpointResult<FileLock> {
        let start_time = std::time::Instant::now();

        loop {
            if let Some(lock) = Self::try_acquire(target_path)? {
                return Ok(lock);
            }

            if start_time.elapsed().as_secs() > timeout_seconds {
                return Err(CheckpointError::storage(format!(
                    "Failed to acquire file lock within {} seconds",
                    timeout_seconds
                )));
            }

            std::thread::sleep(std::time::Duration::from_millis(100));
        }
    }

    /// Get the lock file path for a target file
    fn get_lock_path(target_path: &Path) -> PathBuf {
        let parent = target_path.parent().unwrap_or_else(|| Path::new("."));
        let filename = target_path.file_name().unwrap_or_default();
        parent.join(format!("{}.lock", filename.to_string_lossy()))
    }
}

impl Drop for FileLock {
    fn drop(&mut self) {
        // Remove lock file when dropped
        let _ = fs::remove_file(&self._lock_file);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_atomic_write_success() {
        let temp_dir = tempdir().unwrap();
        let target_path = temp_dir.path().join("test.json");

        let data = serde_json::json!({"test": "data"});
        AtomicOps::write_json(&target_path, &data).unwrap();

        assert!(target_path.exists());
        let read_data: serde_json::Value = AtomicOps::read_json(&target_path).unwrap();
        assert_eq!(data, read_data);
    }

    #[test]
    fn test_file_lock() {
        let temp_dir = tempdir().unwrap();
        let target_path = temp_dir.path().join("test.txt");

        let _lock1 = FileLock::try_acquire(&target_path).unwrap().unwrap();
        let lock2 = FileLock::try_acquire(&target_path).unwrap();

        assert!(lock2.is_none()); // Second lock should fail
    }

    #[test]
    fn test_backup_restore() {
        let temp_dir = tempdir().unwrap();
        let target_path = temp_dir.path().join("test.txt");

        // Create original file
        fs::write(&target_path, "original content").unwrap();

        // Create backup
        let backup_path = AtomicOps::create_backup(&target_path).unwrap();
        assert!(backup_path.exists());

        // Modify original
        fs::write(&target_path, "modified content").unwrap();

        // Restore from backup
        AtomicOps::restore_backup(&backup_path, &target_path).unwrap();

        let restored_content = fs::read_to_string(&target_path).unwrap();
        assert_eq!(restored_content, "original content");
        assert!(!backup_path.exists()); // Backup should be cleaned up
    }
}

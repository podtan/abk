//! File System Storage Backend
//!
//! Default implementation using local filesystem for checkpoint storage.

use async_trait::async_trait;
use std::path::{Path, PathBuf};
use tokio::fs;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use super::traits::{
    ListOptions, ListResult, StorageBackend, StorageError, StorageItemMeta, StorageResult,
};

/// File system storage backend
///
/// Stores checkpoint data on the local filesystem.
/// Keys are translated to file paths relative to the base path.
pub struct FileStorageBackend {
    base_path: PathBuf,
}

impl FileStorageBackend {
    /// Create a new file storage backend
    ///
    /// # Arguments
    /// * `base_path` - Base directory for all storage operations
    pub fn new<P: AsRef<Path>>(base_path: P) -> StorageResult<Self> {
        let base_path = base_path.as_ref().to_path_buf();

        // Create base directory if it doesn't exist (synchronously for constructor)
        std::fs::create_dir_all(&base_path)?;

        Ok(Self { base_path })
    }

    /// Get the base path
    pub fn base_path(&self) -> &Path {
        &self.base_path
    }

    /// Convert a key to a full file path
    fn key_to_path(&self, key: &str) -> PathBuf {
        // Sanitize key to prevent directory traversal
        let sanitized = key
            .replace("..", "_")
            .replace("//", "/")
            .trim_start_matches('/')
            .to_string();
        self.base_path.join(sanitized)
    }

    /// Ensure parent directory exists
    async fn ensure_parent_dir(&self, path: &Path) -> StorageResult<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).await?;
        }
        Ok(())
    }
}

#[async_trait]
impl StorageBackend for FileStorageBackend {
    fn backend_type(&self) -> &'static str {
        "file"
    }

    async fn is_available(&self) -> bool {
        // Check if base path exists and is writable
        self.base_path.exists()
            && fs::metadata(&self.base_path)
                .await
                .map(|m| m.is_dir())
                .unwrap_or(false)
    }

    async fn write(&self, key: &str, data: &[u8]) -> StorageResult<()> {
        let path = self.key_to_path(key);
        self.ensure_parent_dir(&path).await?;

        // Write atomically using temp file + rename pattern
        let temp_path = path.with_extension("tmp");

        let mut file = fs::File::create(&temp_path).await?;
        file.write_all(data).await?;
        file.sync_all().await?;

        fs::rename(&temp_path, &path).await?;

        Ok(())
    }

    async fn read(&self, key: &str) -> StorageResult<Vec<u8>> {
        let path = self.key_to_path(key);

        if !path.exists() {
            return Err(StorageError::NotFound(key.to_string()));
        }

        let mut file = fs::File::open(&path).await?;
        let mut data = Vec::new();
        file.read_to_end(&mut data).await?;

        Ok(data)
    }

    async fn exists(&self, key: &str) -> StorageResult<bool> {
        let path = self.key_to_path(key);
        Ok(path.exists())
    }

    async fn delete(&self, key: &str) -> StorageResult<()> {
        let path = self.key_to_path(key);

        if !path.exists() {
            return Ok(()); // Idempotent delete
        }

        if path.is_dir() {
            fs::remove_dir_all(&path).await?;
        } else {
            fs::remove_file(&path).await?;
        }

        Ok(())
    }

    async fn list(&self, options: ListOptions) -> StorageResult<ListResult> {
        let mut items = Vec::new();
        let prefix = options.prefix.unwrap_or_default();
        let limit = options.limit.unwrap_or(usize::MAX);

        // Walk directory tree
        let search_path = if prefix.is_empty() {
            self.base_path.clone()
        } else {
            self.base_path.join(&prefix)
        };

        if !search_path.exists() {
            return Ok(ListResult {
                items: Vec::new(),
                continuation_token: None,
                has_more: false,
            });
        }

        // Collect all matching files
        let mut stack = vec![search_path.clone()];

        while let Some(dir) = stack.pop() {
            if items.len() >= limit {
                break;
            }

            let mut entries = match fs::read_dir(&dir).await {
                Ok(entries) => entries,
                Err(_) => continue,
            };

            while let Ok(Some(entry)) = entries.next_entry().await {
                if items.len() >= limit {
                    break;
                }

                let path = entry.path();
                let file_type = match entry.file_type().await {
                    Ok(ft) => ft,
                    Err(_) => continue,
                };

                if file_type.is_dir() {
                    stack.push(path);
                } else if file_type.is_file() {
                    // Convert path back to key
                    let key = path
                        .strip_prefix(&self.base_path)
                        .map(|p| p.to_string_lossy().to_string())
                        .unwrap_or_default();

                    // Check prefix filter
                    if !prefix.is_empty() && !key.starts_with(&prefix) {
                        continue;
                    }

                    let metadata = fs::metadata(&path).await?;
                    let modified = metadata
                        .modified()
                        .map(|t| {
                            t.duration_since(std::time::UNIX_EPOCH)
                                .map(|d| d.as_secs() as i64)
                                .unwrap_or(0)
                        })
                        .unwrap_or(0);

                    items.push(StorageItemMeta {
                        key,
                        size: metadata.len(),
                        modified_at: modified,
                        content_type: path
                            .extension()
                            .and_then(|ext| match ext.to_str() {
                                Some("json") => Some("application/json".to_string()),
                                Some("jsonl") => Some("application/x-ndjson".to_string()),
                                _ => None,
                            }),
                    });
                }
            }
        }

        let has_more = items.len() >= limit;

        Ok(ListResult {
            items,
            continuation_token: None, // Simple implementation without pagination
            has_more,
        })
    }

    async fn metadata(&self, key: &str) -> StorageResult<StorageItemMeta> {
        let path = self.key_to_path(key);

        if !path.exists() {
            return Err(StorageError::NotFound(key.to_string()));
        }

        let metadata = fs::metadata(&path).await?;
        let modified = metadata
            .modified()
            .map(|t| {
                t.duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_secs() as i64)
                    .unwrap_or(0)
            })
            .unwrap_or(0);

        Ok(StorageItemMeta {
            key: key.to_string(),
            size: metadata.len(),
            modified_at: modified,
            content_type: path
                .extension()
                .and_then(|ext| match ext.to_str() {
                    Some("json") => Some("application/json".to_string()),
                    Some("jsonl") => Some("application/x-ndjson".to_string()),
                    _ => None,
                }),
        })
    }

    async fn delete_many(&self, keys: &[String]) -> StorageResult<u32> {
        let mut deleted = 0;

        for key in keys {
            let path = self.key_to_path(key);
            if path.exists() {
                if path.is_dir() {
                    if fs::remove_dir_all(&path).await.is_ok() {
                        deleted += 1;
                    }
                } else if fs::remove_file(&path).await.is_ok() {
                    deleted += 1;
                }
            }
        }

        Ok(deleted)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[tokio::test]
    async fn test_file_backend_basic_operations() {
        let temp_dir = TempDir::new().unwrap();
        let backend = FileStorageBackend::new(temp_dir.path()).unwrap();

        // Test write and read
        let key = "test/data.json";
        let data = b"{\"key\": \"value\"}";

        backend.write(key, data).await.unwrap();
        assert!(backend.exists(key).await.unwrap());

        let read_data = backend.read(key).await.unwrap();
        assert_eq!(read_data, data);

        // Test metadata
        let meta = backend.metadata(key).await.unwrap();
        assert_eq!(meta.key, key);
        assert_eq!(meta.size, data.len() as u64);
        assert_eq!(meta.content_type, Some("application/json".to_string()));

        // Test delete
        backend.delete(key).await.unwrap();
        assert!(!backend.exists(key).await.unwrap());
    }

    #[tokio::test]
    async fn test_file_backend_not_found() {
        let temp_dir = TempDir::new().unwrap();
        let backend = FileStorageBackend::new(temp_dir.path()).unwrap();

        let result = backend.read("nonexistent").await;
        assert!(matches!(result, Err(StorageError::NotFound(_))));
    }

    #[tokio::test]
    async fn test_file_backend_list() {
        let temp_dir = TempDir::new().unwrap();
        let backend = FileStorageBackend::new(temp_dir.path()).unwrap();

        // Create some files
        backend.write("session1/checkpoint1.json", b"{}").await.unwrap();
        backend.write("session1/checkpoint2.json", b"{}").await.unwrap();
        backend.write("session2/checkpoint1.json", b"{}").await.unwrap();

        // List all
        let result = backend.list(ListOptions::default()).await.unwrap();
        assert_eq!(result.items.len(), 3);

        // List with prefix
        let result = backend
            .list(ListOptions {
                prefix: Some("session1".to_string()),
                ..Default::default()
            })
            .await
            .unwrap();
        assert_eq!(result.items.len(), 2);

        // List with limit
        let result = backend
            .list(ListOptions {
                limit: Some(2),
                ..Default::default()
            })
            .await
            .unwrap();
        assert!(result.items.len() <= 2);
    }

    #[tokio::test]
    async fn test_file_backend_json_extension() {
        use super::super::StorageBackendExt;
        
        let temp_dir = TempDir::new().unwrap();
        let backend = FileStorageBackend::new(temp_dir.path()).unwrap();

        #[derive(serde::Serialize, serde::Deserialize, PartialEq, Debug)]
        struct TestData {
            name: String,
            value: i32,
        }

        let data = TestData {
            name: "test".to_string(),
            value: 42,
        };

        backend.write_json("test.json", &data).await.unwrap();
        let read_data: TestData = backend.read_json("test.json").await.unwrap();

        assert_eq!(data, read_data);
    }

    #[tokio::test]
    async fn test_file_backend_atomic_write() {
        let temp_dir = TempDir::new().unwrap();
        let backend = FileStorageBackend::new(temp_dir.path()).unwrap();

        // Write data
        let key = "atomic_test.json";
        let data = b"test data";

        backend.write(key, data).await.unwrap();

        // Verify no temp files left behind
        let temp_path = temp_dir.path().join("atomic_test.tmp");
        assert!(!temp_path.exists());

        // Verify data written correctly
        let read_data = backend.read(key).await.unwrap();
        assert_eq!(read_data, data);
    }

    #[tokio::test]
    async fn test_file_backend_delete_many() {
        let temp_dir = TempDir::new().unwrap();
        let backend = FileStorageBackend::new(temp_dir.path()).unwrap();

        // Create files
        backend.write("file1.json", b"{}").await.unwrap();
        backend.write("file2.json", b"{}").await.unwrap();
        backend.write("file3.json", b"{}").await.unwrap();

        // Delete multiple
        let deleted = backend
            .delete_many(&[
                "file1.json".to_string(),
                "file2.json".to_string(),
                "nonexistent.json".to_string(),
            ])
            .await
            .unwrap();

        assert_eq!(deleted, 2);
        assert!(!backend.exists("file1.json").await.unwrap());
        assert!(!backend.exists("file2.json").await.unwrap());
        assert!(backend.exists("file3.json").await.unwrap());
    }

    #[tokio::test]
    async fn test_file_backend_is_available() {
        let temp_dir = TempDir::new().unwrap();
        let backend = FileStorageBackend::new(temp_dir.path()).unwrap();

        assert!(backend.is_available().await);
        assert_eq!(backend.backend_type(), "file");
    }

    #[tokio::test]
    async fn test_directory_traversal_prevention() {
        let temp_dir = TempDir::new().unwrap();
        let backend = FileStorageBackend::new(temp_dir.path()).unwrap();

        // Attempt directory traversal
        let malicious_key = "../../../etc/passwd";
        backend.write(malicious_key, b"test").await.unwrap();

        // File should be written within base_path, not outside
        let path = temp_dir.path().join("______etc_passwd");
        // The sanitized path should not escape the base directory
        let written_path = backend.key_to_path(malicious_key);
        assert!(written_path.starts_with(temp_dir.path()));
    }
}

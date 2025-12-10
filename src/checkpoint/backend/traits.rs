//! Storage Backend Traits
//!
//! Defines the core traits for checkpoint storage backends.

use async_trait::async_trait;
use std::path::PathBuf;

/// Result type for storage operations
pub type StorageResult<T> = Result<T, StorageError>;

/// Error types for storage operations
#[derive(Debug, thiserror::Error)]
pub enum StorageError {
    /// IO error during storage operation
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    /// Serialization error
    #[error("Serialization error: {0}")]
    Serialization(String),

    /// Deserialization error
    #[error("Deserialization error: {0}")]
    Deserialization(String),

    /// Key not found
    #[error("Key not found: {0}")]
    NotFound(String),

    /// Connection error (for remote backends)
    #[error("Connection error: {0}")]
    Connection(String),

    /// Configuration error
    #[error("Configuration error: {0}")]
    Configuration(String),

    /// Permission denied
    #[error("Permission denied: {0}")]
    PermissionDenied(String),

    /// Generic backend error
    #[error("Backend error: {0}")]
    Backend(String),
}

/// Metadata about a stored item
#[derive(Debug, Clone)]
pub struct StorageItemMeta {
    /// Key/path of the item
    pub key: String,
    /// Size in bytes
    pub size: u64,
    /// Last modified timestamp (Unix timestamp)
    pub modified_at: i64,
    /// Content type (e.g., "application/json")
    pub content_type: Option<String>,
}

/// Options for listing items
#[derive(Debug, Clone, Default)]
pub struct ListOptions {
    /// Prefix filter
    pub prefix: Option<String>,
    /// Maximum number of items to return
    pub limit: Option<usize>,
    /// Continuation token for pagination
    pub continuation_token: Option<String>,
}

/// Result of a list operation
#[derive(Debug, Clone)]
pub struct ListResult {
    /// Items found
    pub items: Vec<StorageItemMeta>,
    /// Continuation token for next page (if any)
    pub continuation_token: Option<String>,
    /// Whether there are more items
    pub has_more: bool,
}

/// Core trait for storage backends
///
/// All storage backends must implement this trait to provide
/// basic CRUD operations for checkpoint data.
#[async_trait]
pub trait StorageBackend: Send + Sync {
    /// Get the backend type name (e.g., "file", "documentdb", "s3")
    fn backend_type(&self) -> &'static str;

    /// Check if the backend is available/connected
    async fn is_available(&self) -> bool;

    /// Write raw bytes to storage
    async fn write(&self, key: &str, data: &[u8]) -> StorageResult<()>;

    /// Read raw bytes from storage
    async fn read(&self, key: &str) -> StorageResult<Vec<u8>>;

    /// Check if a key exists
    async fn exists(&self, key: &str) -> StorageResult<bool>;

    /// Delete an item from storage
    async fn delete(&self, key: &str) -> StorageResult<()>;

    /// List items in storage
    async fn list(&self, options: ListOptions) -> StorageResult<ListResult>;

    /// Get metadata for an item
    async fn metadata(&self, key: &str) -> StorageResult<StorageItemMeta>;

    /// Delete multiple items (default implementation)
    async fn delete_many(&self, keys: &[String]) -> StorageResult<u32> {
        let mut deleted = 0;
        for key in keys {
            if self.delete(key).await.is_ok() {
                deleted += 1;
            }
        }
        Ok(deleted)
    }
}

/// Extension trait for JSON operations
///
/// Provides convenient methods for serializing/deserializing JSON data.
#[async_trait]
pub trait StorageBackendExt: StorageBackend {
    /// Write a JSON-serializable value
    async fn write_json<T: serde::Serialize + Send + Sync>(
        &self,
        key: &str,
        value: &T,
    ) -> StorageResult<()> {
        let json = serde_json::to_vec_pretty(value)
            .map_err(|e| StorageError::Serialization(e.to_string()))?;
        self.write(key, &json).await
    }

    /// Read and deserialize a JSON value
    async fn read_json<T: serde::de::DeserializeOwned>(&self, key: &str) -> StorageResult<T> {
        let data = self.read(key).await?;
        serde_json::from_slice(&data).map_err(|e| StorageError::Deserialization(e.to_string()))
    }
}

// Blanket implementation for all StorageBackend implementors
impl<T: StorageBackend + ?Sized> StorageBackendExt for T {}

/// Builder for creating storage backends from configuration
pub struct StorageBackendBuilder {
    backend_type: String,
    config: std::collections::HashMap<String, String>,
}

impl StorageBackendBuilder {
    /// Create a new builder
    pub fn new(backend_type: &str) -> Self {
        Self {
            backend_type: backend_type.to_string(),
            config: std::collections::HashMap::new(),
        }
    }

    /// Add a configuration option
    pub fn with_option(mut self, key: &str, value: &str) -> Self {
        self.config.insert(key.to_string(), value.to_string());
        self
    }

    /// Set the base path (for file backend)
    pub fn with_base_path(self, path: impl Into<PathBuf>) -> Self {
        self.with_option("base_path", &path.into().to_string_lossy())
    }

    /// Build the storage backend
    pub fn build(self) -> StorageResult<Box<dyn StorageBackend>> {
        match self.backend_type.as_str() {
            "file" | "filesystem" => {
                let base_path = self
                    .config
                    .get("base_path")
                    .ok_or_else(|| StorageError::Configuration("base_path is required".into()))?;
                let backend = super::FileStorageBackend::new(base_path)?;
                Ok(Box::new(backend))
            }
            #[cfg(feature = "storage-documentdb")]
            "documentdb" | "mongodb" => {
                // DocumentDB backend would be created here
                Err(StorageError::Configuration(
                    "DocumentDB backend requires connection string".into(),
                ))
            }
            unknown => Err(StorageError::Configuration(format!(
                "Unknown backend type: {}",
                unknown
            ))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_storage_error_display() {
        let err = StorageError::NotFound("test_key".to_string());
        assert_eq!(err.to_string(), "Key not found: test_key");

        let err = StorageError::Io(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "file not found",
        ));
        assert!(err.to_string().contains("IO error"));
    }

    #[test]
    fn test_list_options_default() {
        let opts = ListOptions::default();
        assert!(opts.prefix.is_none());
        assert!(opts.limit.is_none());
        assert!(opts.continuation_token.is_none());
    }

    #[test]
    fn test_builder_file_backend() {
        let result = StorageBackendBuilder::new("file")
            .with_base_path("/tmp/test")
            .build();
        // Should succeed in creating file backend
        assert!(result.is_ok());
    }

    #[test]
    fn test_builder_unknown_backend() {
        let result = StorageBackendBuilder::new("unknown").build();
        assert!(result.is_err());
        if let Err(StorageError::Configuration(msg)) = result {
            assert!(msg.contains("Unknown backend type"));
        }
    }
}

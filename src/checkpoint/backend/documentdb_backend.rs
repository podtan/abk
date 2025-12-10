//! DocumentDB/MongoDB Storage Backend
//!
//! Implementation for cloud-native checkpoint storage using MongoDB/DocumentDB.
//! This backend is useful for distributed agents and multi-region deployments.
//!
//! ## Usage
//!
//! Enable the `storage-documentdb` feature in Cargo.toml:
//!
//! ```toml
//! abk = { version = "0.1.42", features = ["checkpoint", "storage-documentdb"] }
//! ```
//!
//! ## Configuration
//!
//! ```rust,no_run
//! use abk::checkpoint::backend::DocumentDBStorageBackend;
//!
//! async fn example() -> anyhow::Result<()> {
//!     let backend = DocumentDBStorageBackend::new(
//!         "mongodb://localhost:27017",
//!         "checkpoints_db",
//!         "checkpoint_data"
//!     ).await?;
//!     
//!     Ok(())
//! }
//! ```

use async_trait::async_trait;
use mongodb::{
    bson::{doc, Binary, Document},
    options::ClientOptions,
    Client, Collection,
};

use super::traits::{
    ListOptions, ListResult, StorageBackend, StorageError, StorageItemMeta, StorageResult,
};

/// DocumentDB/MongoDB storage backend
pub struct DocumentDBStorageBackend {
    client: Client,
    collection: Collection<Document>,
    database_name: String,
    collection_name: String,
}

impl DocumentDBStorageBackend {
    /// Create a new DocumentDB storage backend
    ///
    /// # Arguments
    /// * `connection_string` - MongoDB/DocumentDB connection string
    /// * `database` - Database name
    /// * `collection` - Collection name
    pub async fn new(
        connection_string: &str,
        database: &str,
        collection: &str,
    ) -> StorageResult<Self> {
        let client_options = ClientOptions::parse(connection_string)
            .await
            .map_err(|e| StorageError::Connection(e.to_string()))?;

        let client =
            Client::with_options(client_options).map_err(|e| StorageError::Connection(e.to_string()))?;

        let db = client.database(database);
        let coll = db.collection::<Document>(collection);

        Ok(Self {
            client,
            collection: coll,
            database_name: database.to_string(),
            collection_name: collection.to_string(),
        })
    }

    /// Get the MongoDB client (for advanced operations)
    pub fn client(&self) -> &Client {
        &self.client
    }

    /// Get the collection (for advanced operations)
    pub fn collection(&self) -> &Collection<Document> {
        &self.collection
    }
}

#[async_trait]
impl StorageBackend for DocumentDBStorageBackend {
    fn backend_type(&self) -> &'static str {
        "documentdb"
    }

    async fn is_available(&self) -> bool {
        self.client
            .database(&self.database_name)
            .run_command(doc! { "ping": 1 })
            .await
            .is_ok()
    }

    async fn write(&self, key: &str, data: &[u8]) -> StorageResult<()> {
        let now = chrono::Utc::now().timestamp();
        
        let doc = doc! {
            "_id": key,
            "data": Binary { subtype: mongodb::bson::spec::BinarySubtype::Generic, bytes: data.to_vec() },
            "size": data.len() as i64,
            "modified_at": now,
        };

        // Upsert the document
        self.collection
            .replace_one(doc! { "_id": key }, doc)
            .upsert(true)
            .await
            .map_err(|e| StorageError::Backend(e.to_string()))?;

        Ok(())
    }

    async fn read(&self, key: &str) -> StorageResult<Vec<u8>> {
        let filter = doc! { "_id": key };

        let doc = self
            .collection
            .find_one(filter)
            .await
            .map_err(|e| StorageError::Backend(e.to_string()))?
            .ok_or_else(|| StorageError::NotFound(key.to_string()))?;

        match doc.get("data") {
            Some(mongodb::bson::Bson::Binary(bin)) => Ok(bin.bytes.clone()),
            _ => Err(StorageError::Deserialization(
                "Invalid data format in document".to_string(),
            )),
        }
    }

    async fn exists(&self, key: &str) -> StorageResult<bool> {
        let filter = doc! { "_id": key };

        let count = self
            .collection
            .count_documents(filter)
            .await
            .map_err(|e| StorageError::Backend(e.to_string()))?;

        Ok(count > 0)
    }

    async fn delete(&self, key: &str) -> StorageResult<()> {
        let filter = doc! { "_id": key };

        self.collection
            .delete_one(filter)
            .await
            .map_err(|e| StorageError::Backend(e.to_string()))?;

        Ok(())
    }

    async fn list(&self, options: ListOptions) -> StorageResult<ListResult> {
        use futures_util::TryStreamExt;
        use mongodb::options::FindOptions;

        let mut filter = doc! {};

        // Add prefix filter if specified
        if let Some(prefix) = &options.prefix {
            filter.insert(
                "_id",
                doc! { "$regex": format!("^{}", regex::escape(prefix)) },
            );
        }

        // Build find options with limit if specified
        let find_options = if let Some(limit) = options.limit {
            FindOptions::builder().limit(limit as i64).build()
        } else {
            FindOptions::default()
        };

        let mut cursor = self
            .collection
            .find(filter)
            .with_options(find_options)
            .await
            .map_err(|e| StorageError::Backend(e.to_string()))?;

        let mut items = Vec::new();

        while let Some(doc) = cursor
            .try_next()
            .await
            .map_err(|e| StorageError::Backend(e.to_string()))?
        {
            if let Some(limit) = options.limit {
                if items.len() >= limit {
                    break;
                }
            }

            let key = doc
                .get_str("_id")
                .map_err(|_| StorageError::Deserialization("Missing _id field".to_string()))?
                .to_string();

            let size = doc.get_i64("size").unwrap_or(0) as u64;
            let modified_at = doc.get_i64("modified_at").unwrap_or(0);

            items.push(StorageItemMeta {
                key,
                size,
                modified_at,
                content_type: Some("application/octet-stream".to_string()),
            });
        }

        Ok(ListResult {
            items,
            continuation_token: None, // Simple implementation without cursor-based pagination
            has_more: false,
        })
    }

    async fn metadata(&self, key: &str) -> StorageResult<StorageItemMeta> {
        let filter = doc! { "_id": key };

        let doc = self
            .collection
            .find_one(filter)
            .await
            .map_err(|e| StorageError::Backend(e.to_string()))?
            .ok_or_else(|| StorageError::NotFound(key.to_string()))?;

        let size = doc.get_i64("size").unwrap_or(0) as u64;
        let modified_at = doc.get_i64("modified_at").unwrap_or(0);

        Ok(StorageItemMeta {
            key: key.to_string(),
            size,
            modified_at,
            content_type: Some("application/octet-stream".to_string()),
        })
    }

    async fn delete_many(&self, keys: &[String]) -> StorageResult<u32> {
        if keys.is_empty() {
            return Ok(0);
        }

        let filter = doc! {
            "_id": { "$in": keys }
        };

        let result = self
            .collection
            .delete_many(filter)
            .await
            .map_err(|e| StorageError::Backend(e.to_string()))?;

        Ok(result.deleted_count as u32)
    }
}

#[cfg(test)]
mod tests {
    // Tests require a running MongoDB instance
    // Run with: cargo test --features storage-documentdb -- --ignored

    use super::*;

    #[tokio::test]
    #[ignore] // Requires MongoDB instance
    async fn test_documentdb_backend_basic() {
        let backend = DocumentDBStorageBackend::new(
            "mongodb://localhost:27017",
            "test_checkpoints",
            "test_data",
        )
        .await
        .expect("Failed to connect to MongoDB");

        assert!(backend.is_available().await);
        assert_eq!(backend.backend_type(), "documentdb");
    }

    #[tokio::test]
    #[ignore] // Requires MongoDB instance
    async fn test_documentdb_crud() {
        let backend = DocumentDBStorageBackend::new(
            "mongodb://localhost:27017",
            "test_checkpoints",
            "test_data",
        )
        .await
        .expect("Failed to connect to MongoDB");

        let key = "test/key";
        let data = b"test data";

        // Write
        backend.write(key, data).await.unwrap();

        // Read
        let read_data = backend.read(key).await.unwrap();
        assert_eq!(read_data, data);

        // Exists
        assert!(backend.exists(key).await.unwrap());

        // Metadata
        let meta = backend.metadata(key).await.unwrap();
        assert_eq!(meta.key, key);
        assert_eq!(meta.size, data.len() as u64);

        // Delete
        backend.delete(key).await.unwrap();
        assert!(!backend.exists(key).await.unwrap());
    }
}

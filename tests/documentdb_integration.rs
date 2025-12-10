//! Integration test for DocumentDB backend with local container
//! 
//! Run with: cargo test --features "checkpoint,storage-documentdb" --test documentdb_integration -- --nocapture

#[cfg(feature = "storage-documentdb")]
mod tests {
    use abk::checkpoint::backend::{DocumentDBStorageBackend, StorageBackend, StorageBackendExt, ListOptions};
    
    // Connection details for local DocumentDB container
    const TEST_URL: &str = "mongodb://trustee:abk12345@localhost:10260/?tls=true&tlsAllowInvalidCertificates=true";
    const TEST_DB: &str = "test_abk_checkpoints";
    const TEST_COLLECTION: &str = "test_checkpoints";
    
    #[tokio::test]
    async fn test_documentdb_connection() {
        println!("Testing DocumentDB connection...");
        
        let backend = match DocumentDBStorageBackend::new(TEST_URL, TEST_DB, TEST_COLLECTION).await {
            Ok(b) => b,
            Err(e) => {
                eprintln!("Failed to create backend: {}", e);
                eprintln!("Make sure DocumentDB container is running:");
                eprintln!("  docker run -dt -p 10260:10260 --name documentdb-container documentdb --username trustee --password abk12345");
                panic!("Backend creation failed");
            }
        };
        
        println!("Backend created, checking availability...");
        
        if !backend.is_available().await {
            eprintln!("Backend not available. Check container is running.");
            panic!("Backend not available");
        }
        
        println!("✅ DocumentDB backend is available!");
        assert_eq!(backend.backend_type(), "documentdb");
    }
    
    #[tokio::test]
    async fn test_documentdb_crud_operations() {
        println!("Testing DocumentDB CRUD operations...");
        
        let backend = DocumentDBStorageBackend::new(TEST_URL, TEST_DB, TEST_COLLECTION)
            .await
            .expect("Failed to create backend");
        
        let test_key = "test/checkpoint_001_metadata.json";
        let test_data = b"{\"checkpoint_id\": \"001\", \"timestamp\": \"2025-12-10T00:00:00Z\"}";
        
        // Write
        println!("Writing test data...");
        backend.write(test_key, test_data).await.expect("Failed to write");
        println!("✅ Write successful");
        
        // Exists
        println!("Checking existence...");
        assert!(backend.exists(test_key).await.expect("Failed to check exists"));
        println!("✅ Key exists");
        
        // Read
        println!("Reading data...");
        let read_data = backend.read(test_key).await.expect("Failed to read");
        assert_eq!(read_data, test_data);
        println!("✅ Read successful, data matches");
        
        // Metadata
        println!("Getting metadata...");
        let meta = backend.metadata(test_key).await.expect("Failed to get metadata");
        assert_eq!(meta.key, test_key);
        assert_eq!(meta.size, test_data.len() as u64);
        println!("✅ Metadata correct: size={}", meta.size);
        
        // Delete
        println!("Deleting test data...");
        backend.delete(test_key).await.expect("Failed to delete");
        assert!(!backend.exists(test_key).await.expect("Failed to check exists after delete"));
        println!("✅ Delete successful");
    }
    
    #[tokio::test]
    async fn test_documentdb_json_operations() {
        println!("Testing DocumentDB JSON operations...");
        
        let backend = DocumentDBStorageBackend::new(TEST_URL, TEST_DB, TEST_COLLECTION)
            .await
            .expect("Failed to create backend");
        
        #[derive(serde::Serialize, serde::Deserialize, Debug, PartialEq)]
        struct TestCheckpoint {
            checkpoint_id: String,
            session_id: String,
            iteration: u32,
            workflow_step: String,
        }
        
        let checkpoint = TestCheckpoint {
            checkpoint_id: "test_cp_001".to_string(),
            session_id: "test_session".to_string(),
            iteration: 42,
            workflow_step: "execute".to_string(),
        };
        
        let key = "sessions/test_session/test_cp_001_metadata.json";
        
        // Write JSON
        println!("Writing JSON checkpoint...");
        backend.write_json(key, &checkpoint).await.expect("Failed to write JSON");
        println!("✅ JSON write successful");
        
        // Read JSON
        println!("Reading JSON checkpoint...");
        let read_checkpoint: TestCheckpoint = backend.read_json(key).await.expect("Failed to read JSON");
        assert_eq!(checkpoint, read_checkpoint);
        println!("✅ JSON read successful, data matches");
        
        // Cleanup
        backend.delete(key).await.expect("Failed to cleanup");
        println!("✅ Cleanup successful");
    }
    
    #[tokio::test]
    async fn test_documentdb_list_operations() {
        println!("Testing DocumentDB list operations...");
        
        let backend = DocumentDBStorageBackend::new(TEST_URL, TEST_DB, TEST_COLLECTION)
            .await
            .expect("Failed to create backend");
        
        // Create multiple test entries
        let prefix = "list_test/session1";
        let keys = vec![
            format!("{}/checkpoint_001.json", prefix),
            format!("{}/checkpoint_002.json", prefix),
            format!("{}/checkpoint_003.json", prefix),
        ];
        
        for key in &keys {
            backend.write(key, b"{}").await.expect("Failed to write");
        }
        println!("✅ Created {} test entries", keys.len());
        
        // List with prefix
        println!("Listing with prefix...");
        let result = backend.list(ListOptions {
            prefix: Some(prefix.to_string()),
            ..Default::default()
        }).await.expect("Failed to list");
        
        println!("Found {} items", result.items.len());
        assert_eq!(result.items.len(), keys.len());
        println!("✅ List returned correct count");
        
        // Cleanup
        let deleted = backend.delete_many(&keys).await.expect("Failed to delete many");
        assert_eq!(deleted, keys.len() as u32);
        println!("✅ Cleanup successful, deleted {} items", deleted);
    }
}

//! Integration test for DocumentDB backend with local container
//! 
//! Run with: cargo test --features "checkpoint,storage-documentdb" --test documentdb_integration -- --nocapture
//!
//! The local DocumentDB (MongoDB-compatible) container:
//!   docker run -d --name documentdb -p 10260:10260 ghcr.io/documentdb/documentdb/documentdb-local:latest
//! Credentials (see /Projects/nox/infra/documentdb.txt):
//!   user=docdb  db=docdb  (TLS + tlsAllowInvalidCertificates)

#[cfg(feature = "storage-documentdb")]
mod tests {
    use abk::checkpoint::backend::{DocumentDBStorageBackend, StorageBackend, StorageBackendExt, ListOptions};
    use abk::checkpoint::config::{StorageBackendConfig, StorageBackendType, StorageMode};
    use abk::checkpoint::{CheckpointStorageManager};
    use abk::checkpoint::models::{
        AgentStateSnapshot, ChatMessage, Checkpoint, CheckpointMetadata, ConversationSnapshot,
        ConversationStats, EnvironmentSnapshot, FileSystemSnapshot, ModelConfig, ProcessInfo,
        ResourceUsage, SystemInfo, ToolStateSnapshot, WorkflowStep,
    };
    use chrono::Utc;
    use std::collections::HashMap;
    use std::path::PathBuf;
    use tempfile::TempDir;

    // Connection details for local DocumentDB container.
    // Override via DOCUMENTDB_TEST_URL when the container uses different creds.
    fn test_url() -> String {
        std::env::var("DOCUMENTDB_TEST_URL").unwrap_or_else(|_| {
            "mongodb://docdb:1wzy8TrUHUGxSDbpcuUC@localhost:10260/?tls=true&tlsAllowInvalidCertificates=true&directConnection=true".to_string()
        })
    }
    const TEST_DB: &str = "test_abk_checkpoints";
    const TEST_COLLECTION: &str = "test_checkpoints";
    // Dedicated DB for the E2E session-level test (cleaned up after).
    const E2E_DB: &str = "test_abk_checkpoints_e2e";
    const E2E_COLLECTION: &str = "checkpoints";

    // -----------------------------------------------------------------------
    // Checkpoint builders (mirror tests/divergent_resume.rs)
    // -----------------------------------------------------------------------

    fn msg(role: &str, content: &str) -> ChatMessage {
        ChatMessage {
            role: role.to_string(),
            content: content.to_string(),
            reasoning: None,
            timestamp: Utc::now(),
            token_count: None,
            tool_calls: None,
            tool_call_id: None,
            name: None,
        }
    }

    fn snapshot(messages: Vec<ChatMessage>) -> ConversationSnapshot {
        ConversationSnapshot {
            messages,
            system_prompt: "Test system prompt".to_string(),
            context_window_size: 4096,
            model_configuration: ModelConfig {
                model_name: "test-model".to_string(),
                max_tokens: None,
                temperature: None,
                top_p: None,
                frequency_penalty: None,
                presence_penalty: None,
            },
            conversation_stats: ConversationStats {
                total_tokens: 0,
                total_messages: 0,
                estimated_cost: None,
                api_calls: 0,
            },
        }
    }

    fn agent(iteration: u32) -> AgentStateSnapshot {
        AgentStateSnapshot {
            current_mode: "confirm".to_string(),
            current_iteration: iteration,
            current_step: WorkflowStep::Analyze,
            max_iterations: 10,
            task_description: "DocumentDB E2E test".to_string(),
            configuration: HashMap::new(),
            working_directory: PathBuf::from("/test/project"),
            session_start_time: Utc::now(),
            last_activity: Utc::now(),
        }
    }

    fn checkpoint(iteration: u32, messages: Vec<ChatMessage>) -> Checkpoint {
        Checkpoint {
            metadata: CheckpointMetadata {
                checkpoint_id: format!("{:03}_analyze", iteration),
                session_id: "e2e_session".to_string(),
                project_hash: "e2e_project_hash".to_string(),
                created_at: Utc::now(),
                iteration,
                workflow_step: WorkflowStep::Analyze,
                checkpoint_version: "1.0".to_string(),
                compressed_size: 0,
                uncompressed_size: 0,
                description: None,
                tags: vec![],
                cursor_seq: 0,
                message_count: 0,
                mainline_fingerprint: None,
            },
            agent_state: agent(iteration),
            conversation_state: snapshot(messages),
            file_system_state: FileSystemSnapshot {
                working_directory: PathBuf::from("/test/project"),
                tracked_files: vec![],
                modified_files: vec![],
                git_status: None,
                file_permissions: HashMap::new(),
            },
            tool_state: ToolStateSnapshot {
                active_tools: HashMap::new(),
                executed_commands: vec![],
                tool_registry: HashMap::new(),
                execution_context: abk::checkpoint::models::ExecutionContext {
                    environment_variables: HashMap::new(),
                    working_directory: PathBuf::from("/test/project"),
                    timeout_seconds: 30,
                    max_retries: 3,
                },
            },
            environment_state: EnvironmentSnapshot {
                environment_variables: HashMap::new(),
                system_info: SystemInfo {
                    os_name: "Linux".to_string(),
                    os_version: "5.0".to_string(),
                    architecture: "x86_64".to_string(),
                    hostname: "test-host".to_string(),
                    cpu_count: 4,
                    total_memory: 8589934592,
                },
                process_info: ProcessInfo {
                    pid: 12345,
                    parent_pid: Some(1234),
                    start_time: Utc::now(),
                    command_line: vec!["agent".to_string()],
                    working_directory: PathBuf::from("/test/project"),
                },
                resource_usage: ResourceUsage {
                    cpu_usage: 0.0,
                    memory_usage: 0,
                    disk_usage: 0,
                    network_bytes_sent: 0,
                    network_bytes_received: 0,
                },
            },
        }
    }

    fn contents(c: &Checkpoint) -> Vec<String> {
        c.conversation_state.messages.iter().map(|m| m.content.clone()).collect()
    }

    #[tokio::test]
    async fn test_documentdb_connection() {
        println!("Testing DocumentDB connection...");
        
        let backend = match DocumentDBStorageBackend::new(&test_url(), TEST_DB, TEST_COLLECTION).await {
            Ok(b) => b,
            Err(e) => {
                eprintln!("Failed to create backend: {}", e);
                eprintln!("Make sure DocumentDB container is running:");
                eprintln!("  docker run -d --name documentdb -p 10260:10260 ghcr.io/documentdb/documentdb/documentdb-local:latest");
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
        
        let backend = DocumentDBStorageBackend::new(&test_url(), TEST_DB, TEST_COLLECTION)
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
        
        let backend = DocumentDBStorageBackend::new(&test_url(), TEST_DB, TEST_COLLECTION)
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
        
        let backend = DocumentDBStorageBackend::new(&test_url(), TEST_DB, TEST_COLLECTION)
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

    // -----------------------------------------------------------------------
    // E2E: session-level O(N) append-only layout against a real DocumentDB.
    //
    // Task fc4ed7ca acceptance: create session, save 5 checkpoints with
    // growing message lists → remote doc count for conversation data ≈ total
    // messages (NOT cumulative blobs); resume from checkpoint 3 returns
    // exactly messages 1..k₃.
    // -----------------------------------------------------------------------

    /// Build a backend config pointing at the E2E database (mirror mode).
    fn e2e_backend_config() -> StorageBackendConfig {
        StorageBackendConfig {
            backend_type: StorageBackendType::DocumentDB,
            storage_mode: StorageMode::Mirror,
            connection_url: Some(test_url()),
            database: Some(E2E_DB.to_string()),
            collection: E2E_COLLECTION.to_string(),
            username: None,
            password: None,
            tls_enabled: true,
            tls_allow_invalid_certs: true,
            connection_timeout_secs: 30,
        }
    }

    #[tokio::test]
    async fn test_documentdb_e2e_append_only_remote_layout() {
        println!("=== E2E: O(N) append-only remote layout ===");

        let temp = TempDir::new().unwrap();
        let backend_config = e2e_backend_config();

        let manager = CheckpointStorageManager::with_home_dir_and_backend(
            temp.path().to_path_buf(),
            "trustee",
            backend_config,
        )
        .await
        .expect("Failed to create storage manager with DocumentDB backend");

        // Clean any stale E2E data from previous runs.
        let raw = DocumentDBStorageBackend::new(&test_url(), E2E_DB, E2E_COLLECTION)
            .await
            .expect("Failed to create raw backend");
        let stale = raw
            .list(ListOptions { prefix: Some("projects/e2e_project_hash/".to_string()), ..Default::default() })
            .await
            .expect("Failed to list stale docs");
        if !stale.items.is_empty() {
            let keys: Vec<String> = stale.items.iter().map(|i| i.key.clone()).collect();
            raw.delete_many(&keys).await.expect("Failed to clean stale docs");
            println!("Cleaned {} stale E2E docs", keys.len());
        }

        // Project storage with a FIXED project id (used directly as the dir
        // name AND the remote project_hash).
        let project = manager
            .get_project_storage_with_id(
                &PathBuf::from("/test/project"),
                "e2e_project_hash",
            )
            .await
            .expect("Failed to get project storage");

        let session_id = "e2e_session";
        let mut session = project
            .create_session(session_id)
            .await
            .expect("Failed to create session");

        // Save 5 checkpoints with growing message lists (2,4,6,8,10 msgs).
        let mut expected_total: usize = 0;
        for i in 1..=5u32 {
            let mut messages = Vec::new();
            for j in 1..=i {
                messages.push(msg("user", &format!("m{}", j)));
                messages.push(msg("assistant", &format!("a{}", j)));
            }
            expected_total = messages.len();
            let cp = checkpoint(i, messages);
            session.save_checkpoint(&cp).await.expect("Failed to save checkpoint");
            println!("Saved checkpoint {:03}_analyze ({} msgs)", i, expected_total);
        }

        // ---- Assert remote layout is O(N): one message doc per message ----
        let prefix = format!("projects/e2e_project_hash/sessions/{}/messages/", session_id);
        let msgs = raw
            .list(ListOptions { prefix: Some(prefix.clone()), ..Default::default() })
            .await
            .expect("Failed to list remote message docs");
        println!("Remote message docs: {} (expected total messages: {})", msgs.items.len(), expected_total);
        assert_eq!(
            msgs.items.len(),
            expected_total,
            "remote must store ONE doc per message (O(N)), not one cumulative blob per checkpoint (O(N²))"
        );

        // ---- Assert agent-state docs: one per checkpoint ----
        let state_prefix = format!("projects/e2e_project_hash/sessions/{}/state/", session_id);
        let states = raw
            .list(ListOptions { prefix: Some(state_prefix.clone()), ..Default::default() })
            .await
            .expect("Failed to list remote state docs");
        println!("Remote state docs: {} (expected 5 checkpoints)", states.items.len());
        assert_eq!(states.items.len(), 5);

        // ---- Resume from checkpoint 3 must return exactly messages 1..k₃ ----
        // Re-open the session from the same manager (resume path).
        let session2 = project
            .create_session(session_id)
            .await
            .expect("Failed to re-open session for resume");
        let c3 = session2
            .load_checkpoint("003_analyze")
            .await
            .expect("Failed to load checkpoint 3");
        let expected3: Vec<String> = (1..=3u32)
            .flat_map(|j| vec![format!("m{}", j), format!("a{}", j)])
            .collect();
        assert_eq!(
            contents(&c3),
            expected3,
            "resume from checkpoint 3 must return exactly messages 1..k₃"
        );
        println!("✅ Resume from checkpoint 3 returned exactly {} messages", contents(&c3).len());

        // ---- Mirror consistency: local log has the same O(N) line count ----
        let local_log = temp.path().join("projects/e2e_project_hash/sessions/e2e_session/conversation.jsonl");
        let line_count = std::fs::read_to_string(&local_log)
            .expect("local conversation.jsonl must exist (mirror mode)")
            .lines()
            .filter(|l| !l.trim().is_empty())
            .count();
        println!("Local conversation.jsonl lines: {} (expected {})", line_count, expected_total);
        assert_eq!(line_count, expected_total, "mirror: local log must match remote message count");

        // Cleanup: drop the E2E project docs.
        let e2e_docs = raw
            .list(ListOptions { prefix: Some("projects/e2e_project_hash/".to_string()), ..Default::default() })
            .await
            .expect("Failed to list E2E docs for cleanup");
        let keys: Vec<String> = e2e_docs.items.iter().map(|i| i.key.clone()).collect();
        if !keys.is_empty() {
            raw.delete_many(&keys).await.expect("Failed to clean E2E docs");
        }
        println!("✅ E2E complete — O(N) remote layout verified, cleanup done");
    }
}

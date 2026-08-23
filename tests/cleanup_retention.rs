//! B4 verification: session cleanup and retention semantics for the
//! append-only shared-log checkpoint layout.
//!
//! Verifies, end-to-end on disk:
//!   1. `delete_session` removes the ENTIRE per-session directory — the
//!      shared `conversation.jsonl` / `agent_state.jsonl` logs, the
//!      `checkpoints/checkpoints.json` index, and `session_metadata.json` —
//!      leaving no orphans and no stray entries visible via `list_sessions`.
//!   2. Retention (`cleanup_old_sessions` with an age policy) deletes an
//!      expired non-active session but leaves a fresh session's shared-log
//!      cursors and checkpoint index intact — retention must never corrupt
//!      the surviving session's cursor-addressed append-only logs.
//!   3. Reported storage size reflects the amount of stored session data
//!      (grows with appended messages), not a fixed footprint.
//!
//! Run with:
//! cargo test --features "checkpoint,observability" --test cleanup_retention

use std::collections::HashMap;
use std::path::PathBuf;

use abk::checkpoint::config::RetentionPolicy;
use abk::checkpoint::models::{
    AgentStateSnapshot, ChatMessage, Checkpoint, CheckpointMetadata, ConversationSnapshot,
    ConversationStats, EnvironmentSnapshot, ExecutionContext, FileSystemSnapshot, ModelConfig,
    ProcessInfo, ResourceUsage, SessionMetadata, SessionStatus, SystemInfo, ToolStateSnapshot,
    WorkflowStep,
};
use abk::checkpoint::storage::ProjectStorage;
use abk::checkpoint::{AtomicOps, SessionStorage};
use chrono::{Duration, Utc};
use tempfile::TempDir;

// ---------------------------------------------------------------------------
// Builders
// ---------------------------------------------------------------------------

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
    let total_messages = messages.len();
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
            total_messages,
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
        task_description: "Cleanup/retention test".to_string(),
        configuration: HashMap::new(),
        working_directory: PathBuf::from("/test/project"),
        session_start_time: Utc::now(),
        last_activity: Utc::now(),
    }
}

fn checkpoint(session: &str, iteration: u32, messages: Vec<ChatMessage>) -> Checkpoint {
    Checkpoint {
        metadata: CheckpointMetadata {
            checkpoint_id: format!("{:03}_analyze", iteration),
            session_id: session.to_string(),
            project_hash: "ph1".to_string(),
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
            execution_context: ExecutionContext {
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

async fn save_all(storage: &mut SessionStorage, cks: &[Checkpoint]) {
    for ck in cks {
        storage.save_checkpoint(ck).await.unwrap();
    }
}

fn session_dir(base: &TempDir, sid: &str) -> PathBuf {
    base.path().join("projects").join("ph1").join("sessions").join(sid)
}

async fn open_local(base: &TempDir, sid: &str) -> SessionStorage {
    let dir = session_dir(base, sid);
    let metadata: SessionMetadata =
        AtomicOps::read_json(&dir.join("session_metadata.json")).unwrap();
    SessionStorage::new(dir, metadata).await.unwrap()
}

fn line_count(path: &std::path::Path) -> usize {
    std::fs::read_to_string(path)
        .map(|s| s.lines().filter(|l| !l.trim().is_empty()).count())
        .unwrap_or(0)
}

fn age_session_metadata(base: &TempDir, sid: &str, days: i64) {
    let path = session_dir(base, sid).join("session_metadata.json");
    let mut metadata: SessionMetadata = AtomicOps::read_json(&path).unwrap();
    metadata.created_at = Utc::now() - Duration::days(days);
    metadata.last_accessed = Utc::now() - Duration::days(days);
    metadata.status = SessionStatus::Completed;
    AtomicOps::write_json(&path, &metadata).unwrap();
}

fn retention_policy() -> RetentionPolicy {
    RetentionPolicy {
        max_age_days: Some(30),
        max_total_size_gb: None,
        max_sessions_per_project: None,
        cleanup_interval_hours: 24,
        enable_auto_cleanup: false,
        preserve_tagged: true,
        preserve_active_sessions: true,
    }
}

// ---------------------------------------------------------------------------
// 1. delete_session removes the whole per-session directory — no orphans
// ---------------------------------------------------------------------------

#[tokio::test]
async fn delete_session_removes_shared_logs_index_and_metadata_without_orphans() {
    let base = TempDir::new().unwrap();
    let mut manager =
        ProjectStorage::new(base.path().to_path_buf(), "ph1".to_string(), base.path().to_path_buf())
            .await
            .unwrap();

    let mut ss = manager.create_session("victim_session").await.unwrap();
    save_all(
        &mut ss,
        &[
            checkpoint("victim_session", 1, vec![msg("user", "q1"), msg("assistant", "a1")]),
            checkpoint("victim_session", 2, vec![msg("user", "q1"), msg("assistant", "a1"), msg("user", "q2"), msg("assistant", "a2")]),
        ],
    )
    .await;

    let dir = session_dir(&base, "victim_session");
    for expected in ["conversation.jsonl", "agent_state.jsonl", "session_metadata.json"] {
        assert!(dir.join(expected).exists(), "expected {} before delete", expected);
    }
    assert!(dir.join("checkpoints.json").exists());
    assert_eq!(line_count(&dir.join("conversation.jsonl")), 4);

    manager.delete_session("victim_session").await.unwrap();

    assert!(!dir.exists(), "session dir must be removed entirely");
    let sessions = manager.list_sessions().await.unwrap();
    assert!(
        sessions.iter().all(|s| s.session_id != "victim_session"),
        "deleted session must not linger in the project session list"
    );

    // Deleting a non-existent session is a no-op, not an error.
    manager.delete_session("victim_session").await.unwrap();
}

// ---------------------------------------------------------------------------
// 2. Retention deletes the expired session, never the surviving one's cursors
// ---------------------------------------------------------------------------

#[tokio::test]
async fn retention_deletes_expired_session_and_preserves_shared_log_cursors() {
    let base = TempDir::new().unwrap();
    let mut manager =
        ProjectStorage::new(base.path().to_path_buf(), "ph1".to_string(), base.path().to_path_buf())
            .await
            .unwrap();

    // Fresh session that must survive retention.
    let mut keep = manager.create_session("keep_session").await.unwrap();
    save_all(
        &mut keep,
        &[
            checkpoint("keep_session", 1, vec![msg("user", "k1"), msg("assistant", "r1")]),
            checkpoint("keep_session", 2, vec![msg("user", "k1"), msg("assistant", "r1"), msg("user", "k2"), msg("assistant", "r2")]),
        ],
    )
    .await;

    // Expired session (created 40 days ago, completed).
    let mut old = manager.create_session("old_session").await.unwrap();
    save_all(
        &mut old,
        &[
            checkpoint("old_session", 1, vec![msg("user", "o1"), msg("assistant", "p1")]),
            checkpoint("old_session", 2, vec![msg("user", "o1"), msg("assistant", "p1"), msg("user", "o2"), msg("assistant", "p2")]),
        ],
    )
    .await;
    drop(old);
    age_session_metadata(&base, "old_session", 40);

    // Re-open the manager so the (30s) sessions cache cannot serve stale
    // Active metadata for the aged session.
    let manager =
        ProjectStorage::new(base.path().to_path_buf(), "ph1".to_string(), base.path().to_path_buf())
            .await
            .unwrap();

    let deleted = manager.cleanup_old_sessions(&retention_policy()).await.unwrap();
    assert_eq!(deleted, 1, "exactly the expired session should be deleted");

    assert!(!session_dir(&base, "old_session").exists());
    let keep_dir = session_dir(&base, "keep_session");
    assert!(keep_dir.exists());

    // The surviving shared log is untouched: 4 lines, both cursors valid.
    assert_eq!(line_count(&keep_dir.join("conversation.jsonl")), 4);
    assert_eq!(line_count(&keep_dir.join("agent_state.jsonl")), 2);

    let mut reloaded = open_local(&base, "keep_session").await;
    let cp1 = reloaded.load_checkpoint("001_analyze").await.unwrap();
    let cp2 = reloaded.load_checkpoint("002_analyze").await.unwrap();
    assert_eq!(cp1.metadata.cursor_seq, 2);
    assert_eq!(cp2.metadata.cursor_seq, 4);
    assert_eq!(cp1.conversation_state.messages.len(), 2);
    assert_eq!(cp2.conversation_state.messages.len(), 4);
    assert_eq!(cp2.conversation_state.messages[3].content, "r2");

    let sessions = manager.list_sessions().await.unwrap();
    assert_eq!(sessions.len(), 1);
    assert_eq!(sessions[0].session_id, "keep_session");
}

// ---------------------------------------------------------------------------
// 3. Reported size tracks stored data volume
// ---------------------------------------------------------------------------

#[tokio::test]
async fn project_size_grows_with_appended_session_data() {
    let base = TempDir::new().unwrap();
    let mut manager =
        ProjectStorage::new(base.path().to_path_buf(), "ph1".to_string(), base.path().to_path_buf())
            .await
            .unwrap();

    let mut ss = manager.create_session("size_session").await.unwrap();
    save_all(&mut ss, &[checkpoint("size_session", 1, vec![msg("user", "small")])])
        .await;
    let small = manager.calculate_project_size().await.unwrap();
    assert!(small > 0, "project size must account for stored data");

    let big = "x".repeat(4096);
    save_all(
        &mut ss,
        &[checkpoint("size_session", 2, vec![msg("user", &big)])],
    )
    .await;
    let large = manager.calculate_project_size().await.unwrap();
    assert!(
        large > small,
        "size must grow with appended data ({} -> {})",
        small,
        large
    );
}

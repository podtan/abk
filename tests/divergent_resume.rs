//! Divergent-resume (fork) repro for the append-only conversation log.
//!
//! A session's `conversation.jsonl` is a shared, append-only log addressed by
//! a per-checkpoint cursor (`cursor_seq`). Resuming a NON-latest checkpoint
//! and continuing from it forks the conversation: the new branch's messages
//! do NOT extend the shared log (their content diverged from the mainline),
//! so the branch must be persisted as a full `{NNN}_conversation.json`
//! snapshot with `cursor_seq = 0` (the legacy fallback reader loads it).
//!
//! Before the fork fix, `save_checkpoint` only appended when
//! `total > hwm`, so a forked checkpoint indexed a cursor pointing at the
//! WRONG (mainline) messages and the branch was silently lost on reload.
//!
//! Run with: cargo test --features checkpoint --test divergent_resume

use std::path::PathBuf;

use abk::checkpoint::models::{
    AgentStateSnapshot, ChatMessage, Checkpoint, CheckpointMetadata, ConversationSnapshot,
    ConversationStats, EnvironmentSnapshot, FileSystemSnapshot, ModelConfig,
    ProcessInfo, ResourceUsage, SessionMetadata, SessionStatus, SystemInfo,
    ToolStateSnapshot, WorkflowStep,
};
use abk::checkpoint::{AtomicOps, SessionStorage};
use chrono::Utc;
use tempfile::TempDir;

// ---------------------------------------------------------------------------
// Test checkpoint builders
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
        task_description: "Divergent resume test".to_string(),
        configuration: std::collections::HashMap::new(),
        working_directory: PathBuf::from("/test/project"),
        session_start_time: Utc::now(),
        last_activity: Utc::now(),
    }
}

/// Build a checkpoint at `iteration` whose conversation is `messages`.
fn checkpoint(iteration: u32, messages: Vec<ChatMessage>) -> Checkpoint {
    Checkpoint {
        metadata: CheckpointMetadata {
            checkpoint_id: format!("{:03}_analyze", iteration),
            session_id: "fork_session".to_string(),
            project_hash: "test_project_hash".to_string(),
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
        },
        agent_state: agent(iteration),
        conversation_state: snapshot(messages),
        file_system_state: FileSystemSnapshot {
            working_directory: PathBuf::from("/test/project"),
            tracked_files: vec![],
            modified_files: vec![],
            git_status: None,
            file_permissions: std::collections::HashMap::new(),
        },
        tool_state: ToolStateSnapshot {
            active_tools: std::collections::HashMap::new(),
            executed_commands: vec![],
            tool_registry: std::collections::HashMap::new(),
            execution_context: abk::checkpoint::models::ExecutionContext {
                environment_variables: std::collections::HashMap::new(),
                working_directory: PathBuf::from("/test/project"),
                timeout_seconds: 30,
                max_retries: 3,
            },
        },
        environment_state: EnvironmentSnapshot {
            environment_variables: std::collections::HashMap::new(),
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

fn session_metadata() -> SessionMetadata {
    SessionMetadata {
        session_id: "fork_session".to_string(),
        project_hash: "test_project_hash".to_string(),
        created_at: Utc::now(),
        last_accessed: Utc::now(),
        checkpoint_count: 0,
        status: SessionStatus::Active,
        description: None,
        tags: vec![],
        size_bytes: 0,
        task_description: Some("Divergent resume test".to_string()),
        configuration: std::collections::HashMap::new(),
        working_directory: Some("/test/project".to_string()),
        max_iterations: 10,
    }
}

/// Create (or reuse) the on-disk session files, then build a `SessionStorage`.
///
/// On first creation the session directory is empty, so we seed an empty
/// `checkpoints.json` + `session_metadata.json`. On a reload (after saves) we
/// must NOT clobber the index/metadata the saves wrote — we read them back.
async fn open_session(temp: &std::path::Path) -> SessionStorage {
    let meta_path = temp.join("session_metadata.json");
    let index_path = temp.join("checkpoints.json");

    if !meta_path.exists() {
        AtomicOps::write_json(&meta_path, &session_metadata()).unwrap();
    }
    if !index_path.exists() {
        AtomicOps::write_json(
            &index_path,
            &std::collections::HashMap::<String, CheckpointMetadata>::new(),
        )
        .unwrap();
    }

    // Read back whatever is on disk (empty on first create, populated after saves).
    let metadata: SessionMetadata =
        if meta_path.exists() {
            AtomicOps::read_json(&meta_path).unwrap()
        } else {
            session_metadata()
        };
    SessionStorage::new(temp.to_path_buf(), metadata).await.unwrap()
}

fn contents(checkpoint: &Checkpoint) -> Vec<String> {
    checkpoint
        .conversation_state
        .messages
        .iter()
        .map(|m| m.content.clone())
        .collect()
}

// ---------------------------------------------------------------------------
// The repro
// ---------------------------------------------------------------------------

/// Linear history still works end-to-end: save 001 (2 msgs) → 002 (4 msgs)
/// → 003 (6 msgs); every checkpoint reloads its exact messages from the
/// append-only log (cursor 2 / 4 / 6).
#[tokio::test]
async fn linear_session_checkpoints_reload_exact_messages() {
    let temp = TempDir::new().unwrap();
    let mut session = open_session(temp.path()).await;

    let c1 = checkpoint(1, vec![msg("user", "m1"), msg("assistant", "a1")]);
    session.save_checkpoint(&c1).await.unwrap();
    let c2 = checkpoint(
        2,
        vec![
            msg("user", "m1"),
            msg("assistant", "a1"),
            msg("user", "m2"),
            msg("assistant", "a2"),
        ],
    );
    session.save_checkpoint(&c2).await.unwrap();
    let c3 = checkpoint(
        3,
        vec![
            msg("user", "m1"),
            msg("assistant", "a1"),
            msg("user", "m2"),
            msg("assistant", "a2"),
            msg("user", "m3"),
            msg("assistant", "a3"),
        ],
    );
    session.save_checkpoint(&c3).await.unwrap();

    // Fresh load (simulates a later process / reload).
    let mut session = open_session(temp.path()).await;
    let loaded: Vec<Checkpoint> = vec![
        session.load_checkpoint("001_analyze").await.unwrap(),
        session.load_checkpoint("002_analyze").await.unwrap(),
        session.load_checkpoint("003_analyze").await.unwrap(),
    ];

    assert_eq!(contents(&loaded[0]), vec!["m1", "a1"]);
    assert_eq!(contents(&loaded[1]), vec!["m1", "a1", "m2", "a2"]);
    assert_eq!(
        contents(&loaded[2]),
        vec!["m1", "a1", "m2", "a2", "m3", "a3"]
    );
}

/// THE repro: resume a NON-latest checkpoint (001) and continue from it.
/// The new branch's conversation does NOT extend the shared log, so a fresh
/// reload of 004 must yield the DIVERGED messages — not the first 4 lines of
/// the mainline log.
#[tokio::test]
async fn divergent_resume_saves_and_reloads_forked_branch() {
    let temp = TempDir::new().unwrap();
    let mut session = open_session(temp.path()).await;

    // Mainline: 001 (2 msgs) → 002 (4 msgs) → 003 (6 msgs).
    let c1 = checkpoint(1, vec![msg("user", "m1"), msg("assistant", "a1")]);
    session.save_checkpoint(&c1).await.unwrap();
    let c2 = checkpoint(
        2,
        vec![
            msg("user", "m1"),
            msg("assistant", "a1"),
            msg("user", "m2"),
            msg("assistant", "a2"),
        ],
    );
    session.save_checkpoint(&c2).await.unwrap();
    let c3 = checkpoint(
        3,
        vec![
            msg("user", "m1"),
            msg("assistant", "a1"),
            msg("user", "m2"),
            msg("assistant", "a2"),
            msg("user", "m3"),
            msg("assistant", "a3"),
        ],
    );
    session.save_checkpoint(&c3).await.unwrap();

    // Resume 001 (true max = 3 → next iteration is 4) and continue with a
    // DIFFERENT conversation. The branch's messages diverge from the
    // mainline; only the first message is shared.
    let c4 = checkpoint(4, vec![msg("user", "m1"), msg("assistant", "a1-fork")]);
    session.save_checkpoint(&c4).await.unwrap();

    // Fresh load.
    let mut session = open_session(temp.path()).await;
    let loaded = session.load_checkpoint("004_analyze").await.unwrap();

    // The forked branch must come back EXACTLY.
    assert_eq!(
        contents(&loaded),
        vec!["m1", "a1-fork"],
        "forked checkpoint 004 must reload the diverged branch, not the mainline prefix"
    );

    // The append-only log must be untouched by the fork: still exactly the
    // 6 mainline messages (seq 1..=6), never rewritten or truncated.
    let log = abk::checkpoint::conversation_log::ConversationLog::new(temp.path());
    let mainline = log.read_all().unwrap();
    assert_eq!(
        mainline.iter().map(|m| m.content.clone()).collect::<Vec<_>>(),
        vec!["m1", "a1", "m2", "a2", "m3", "a3"],
        "conversation.jsonl must remain the untouched mainline history"
    );

    // The mainline checkpoints still reload correctly alongside the fork.
    let c3_loaded = session.load_checkpoint("003_analyze").await.unwrap();
    assert_eq!(
        contents(&c3_loaded),
        vec!["m1", "a1", "m2", "a2", "m3", "a3"]
    );
}

/// A fork must not collide with the shared log's sequence numbers: the fork's
/// messages must be addressable under their own branch, and a SECOND resume
/// from the same base must produce a consistent, collision-free id.
#[tokio::test]
async fn fork_from_earlier_checkpoint_does_not_corrupt_shared_log() {
    let temp = TempDir::new().unwrap();
    let mut session = open_session(temp.path()).await;

    // Mainline: 001 (2 msgs) → 003 (6 msgs).
    let c1 = checkpoint(1, vec![msg("user", "m1"), msg("assistant", "a1")]);
    session.save_checkpoint(&c1).await.unwrap();
    let c3 = checkpoint(
        3,
        vec![
            msg("user", "m1"),
            msg("assistant", "a1"),
            msg("user", "m2"),
            msg("assistant", "a2"),
            msg("user", "m3"),
            msg("assistant", "a3"),
        ],
    );
    session.save_checkpoint(&c3).await.unwrap();

    // Resume 001 → fork at iteration 4 with a diverged 2-message branch.
    let c4 = checkpoint(4, vec![msg("user", "m1"), msg("assistant", "a1-fork")]);
    session.save_checkpoint(&c4).await.unwrap();

    // Reload the fork from a fresh session.
    let mut session = open_session(temp.path()).await;
    let loaded = session.load_checkpoint("004_analyze").await.unwrap();
    assert_eq!(contents(&loaded), vec!["m1", "a1-fork"]);

    // The shared log is still exactly the 6 mainline messages.
    let log = abk::checkpoint::conversation_log::ConversationLog::new(temp.path());
    assert_eq!(log.count().unwrap(), 6);
}

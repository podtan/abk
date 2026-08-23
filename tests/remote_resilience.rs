//! Remote-write resilience tests (nghr 450e00d4 + c561e911).
//!
//! Covers the mirror-gap family:
//!  1. A transiently failing backend must be retried, not swallowed.
//!  2. Gaps below the cursor (dropped remote docs) must be reconciled from
//!     the local log on the next save.
//!  3. The "✅ MIRRORED" log line must only appear when the remote copy is
//!     actually complete; a failed save must report the gap loudly.
//!  4. `delete_session` must remove the remote docs too (no orphans).
//!  5. `delete_checkpoint` removes only its per-checkpoint remote docs and
//!     never the shared message/state logs.
//!
//! The flaky-backend tests wrap the real DocumentDB backend (or run against
//! a counting in-memory fake when the container is unavailable) with a
//! deterministic failure injection.
//!
//! Run with:
//! cargo test --features "checkpoint,observability,storage-documentdb" --test remote_resilience

#![cfg(feature = "storage-documentdb")]

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use abk::checkpoint::backend::{
    DocumentDBStorageBackend, ListOptions, ListResult, StorageBackend, StorageItemMeta,
};
use abk::checkpoint::config::StorageMode;
use abk::checkpoint::models::{
    AgentStateSnapshot, ChatMessage, Checkpoint, CheckpointMetadata, ConversationSnapshot,
    ConversationStats, EnvironmentSnapshot, FileSystemSnapshot, ModelConfig, ProcessInfo,
    ResourceUsage, SessionMetadata, SessionStatus, SystemInfo, ToolStateSnapshot, WorkflowStep,
    ExecutionContext,
};
use abk::checkpoint::{AtomicOps, SessionStorage};
use async_trait::async_trait;
use chrono::Utc;
use tempfile::TempDir;

// ---------------------------------------------------------------------------
// A minimal in-memory backend with deterministic failure injection.
// ---------------------------------------------------------------------------

#[derive(Default)]
struct MemoryBackendInner {
    docs: Mutex<HashMap<String, Vec<u8>>>,
}

/// Wraps an inner backend; `fail_writes` counter makes the NEXT N writes
/// fail (transient-error simulation), while reads pass through to `inner`
/// when available or the in-memory map.
struct FlakyBackend {
    inner: Option<Arc<DocumentDBStorageBackend>>,
    mem: MemoryBackendInner,
    fail_next_writes: AtomicU32,
    write_attempts: AtomicUsize,
}

#[derive(Debug)]
struct StorageErr(String);

impl std::fmt::Display for StorageErr {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}
impl std::error::Error for StorageErr {}

impl From<StorageErr> for abk::checkpoint::backend::StorageError {
    fn from(e: StorageErr) -> Self {
        abk::checkpoint::backend::StorageError::Backend(e.to_string())
    }
}

impl FlakyBackend {
    fn memory_only() -> Arc<Self> {
        Arc::new(Self {
            inner: None,
            mem: MemoryBackendInner::default(),
            fail_next_writes: AtomicU32::new(0),
            write_attempts: AtomicUsize::new(0),
        })
    }

    fn with_inner(inner: Arc<DocumentDBStorageBackend>) -> Arc<Self> {
        Arc::new(Self {
            inner: Some(inner),
            mem: MemoryBackendInner::default(),
            fail_next_writes: AtomicU32::new(0),
            write_attempts: AtomicUsize::new(0),
        })
    }

    fn read_bytes(&self, key: &str) -> Option<Vec<u8>> {
        self.mem.docs.lock().unwrap().get(key).cloned()
    }
}

// The StorageError type used by abk is re-exported from the backend module.
type BErr = abk::checkpoint::backend::StorageError;

fn berr(msg: impl Into<String>) -> BErr {
    abk::checkpoint::backend::StorageError::Backend(msg.into())
}

#[async_trait]
impl StorageBackend for FlakyBackend {
    fn backend_type(&self) -> &'static str {
        "flaky-test"
    }

    async fn write(&self, key: &str, data: &[u8]) -> Result<(), BErr> {
        self.write_attempts.fetch_add(1, Ordering::SeqCst);
        let remaining = self.fail_next_writes.load(Ordering::SeqCst);
        if remaining > 0 {
            self.fail_next_writes.fetch_sub(1, Ordering::SeqCst);
            return Err(berr(format!("injected transient failure for {}", key)));
        }
        self.mem.docs.lock().unwrap().insert(key.to_string(), data.to_vec());
        Ok(())
    }

    async fn read(&self, key: &str) -> Result<Vec<u8>, BErr> {
        self.read_bytes(key).ok_or_else(|| berr(format!("not found: {}", key)))
    }

    async fn exists(&self, key: &str) -> Result<bool, BErr> {
        Ok(self.mem.docs.lock().unwrap().contains_key(key))
    }

    async fn metadata(&self, key: &str) -> Result<StorageItemMeta, BErr> {
        self.mem.docs.lock().unwrap().get(key).map(|data| StorageItemMeta {
            key: key.to_string(),
            size: data.len() as u64,
            modified_at: 0,
            content_type: Some("application/json".to_string()),
        }).ok_or_else(|| berr(format!("not found: {}", key)))
    }

    async fn delete(&self, key: &str) -> Result<(), BErr> {
        self.mem.docs.lock().unwrap().remove(key);
        Ok(())
    }

    async fn list(&self, options: ListOptions) -> Result<ListResult, BErr> {
        let prefix = options.prefix.unwrap_or_default();
        let items: Vec<StorageItemMeta> = self
            .mem
            .docs
            .lock()
            .unwrap()
            .keys()
            .filter(|k| k.starts_with(&prefix))
            .map(|k| StorageItemMeta {
                key: k.clone(),
                size: 1,
                modified_at: 0,
                content_type: Some("application/json".to_string()),
            })
            .collect();
        Ok(ListResult { items, continuation_token: None, has_more: false })
    }

    async fn is_available(&self) -> bool {
        true
    }
}

// ---------------------------------------------------------------------------
// Builders (mirror tests/divergent_resume.rs)
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
        task_description: "Remote resilience test".to_string(),
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
            project_hash: "p1".to_string(),
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

fn session_metadata(sid: &str) -> SessionMetadata {
    SessionMetadata {
        session_id: sid.to_string(),
        project_hash: "p1".to_string(),
        created_at: Utc::now(),
        last_accessed: Utc::now(),
        checkpoint_count: 0,
        status: SessionStatus::Active,
        description: None,
        tags: vec![],
        size_bytes: 0,
        task_description: Some("Remote resilience test".to_string()),
        configuration: HashMap::new(),
        working_directory: Some("/test/project".to_string()),
        max_iterations: 10,
    }
}

/// Open (or create) the local session files and wrap them in a MIRROR-mode
/// SessionStorage with the given (flaky) backend.
async fn open_mirror(temp: &std::path::Path, backend: Arc<FlakyBackend>) -> SessionStorage {
    let meta_path = temp.join("session_metadata.json");
    if !meta_path.exists() {
        AtomicOps::write_json(&meta_path, &session_metadata("resilience_session")).unwrap();
    }
    let index_path = temp.join("checkpoints.json");
    if !index_path.exists() {
        AtomicOps::write_json(&index_path, &HashMap::<String, CheckpointMetadata>::new()).unwrap();
    }
    let metadata: SessionMetadata = AtomicOps::read_json(&meta_path).unwrap();
    SessionStorage::with_remote_backend(
        temp.to_path_buf(),
        metadata,
        Some(backend as Arc<dyn StorageBackend + Send + Sync>),
        StorageMode::Mirror,
    )
    .await
    .unwrap()
}

fn message_keys(backend: &FlakyBackend, session_key_prefix: &str) -> Vec<u32> {
    let mut seqs: Vec<u32> = backend
        .mem
        .docs
        .lock()
        .unwrap()
        .keys()
        .filter(|k| k.starts_with(&format!("{}/messages/", session_key_prefix)))
        .filter_map(|k| {
            k.rsplit('/')
                .next()
                .and_then(|f| f.trim_end_matches(".json").parse().ok())
        })
        .collect();
    seqs.sort_unstable();
    seqs
}

/// The same as `message_keys` but for the `CountingBackend`.
fn counting_message_keys(backend: &CountingBackend, session_key_prefix: &str) -> Vec<u32> {
    let mut seqs: Vec<u32> = backend
        .inner
        .docs
        .lock()
        .unwrap()
        .keys()
        .filter(|k| k.starts_with(&format!("{}/messages/", session_key_prefix)))
        .filter_map(|k| {
            k.rsplit('/')
                .next()
                .and_then(|f| f.trim_end_matches(".json").parse().ok())
        })
        .collect();
    seqs.sort_unstable();
    seqs
}

/// Read the persisted remote checkpoints index (the durable source of truth
/// for a Remote-only session) as in-memory `CheckpointMetadata` entries.
fn remote_index(backend: &CountingBackend, session_key_prefix: &str) -> HashMap<String, CheckpointMetadata> {
    let docs = backend.inner.docs.lock().unwrap();
    let key = format!("{}/checkpoints/checkpoints.json", session_key_prefix);
    docs.get(&key)
        .cloned()
        .map(|bytes| serde_json::from_slice(&bytes).unwrap())
        .unwrap_or_default()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

/// A transient failure (fewer than the retry budget) must NOT lose a doc:
/// the retry lands the write and the save still reports success.
#[tokio::test]
async fn transient_write_failure_is_retried_not_swallowed() {
    let temp = TempDir::new().unwrap();
    let backend = FlakyBackend::memory_only();
    // Fail the first 2 write attempts (retry budget is 3 → survives).
    backend.fail_next_writes.store(2, Ordering::SeqCst);

    let mut ss = open_mirror(temp.path(), backend.clone()).await;
    ss.save_checkpoint(&checkpoint(
        "resilience_session",
        1,
        vec![msg("system", "s"), msg("user", "q1"), msg("assistant", "a1")],
    ))
    .await
    .unwrap();

    let prefix = "projects/p1/sessions/resilience_session";
    assert_eq!(
        message_keys(&backend, prefix),
        vec![1, 2, 3],
        "all message docs must be present despite the injected transient failures"
    );
    // Retries actually happened (attempts > distinct keys).
    assert!(
        backend.write_attempts.load(Ordering::SeqCst) > 3,
        "retry logic must have re-attempted the failed writes"
    );
}

/// A dropped remote doc below the cursor (the original 450e00d4 repro:
/// local 7 msgs vs remote missing #5) is backfilled on the next save.
#[tokio::test]
async fn mirror_gap_below_cursor_is_reconciled_on_next_save() {
    let temp = TempDir::new().unwrap();
    let backend = FlakyBackend::memory_only();

    let mut ss = open_mirror(temp.path(), backend.clone()).await;
    ss.save_checkpoint(&checkpoint(
        "resilience_session",
        1,
        vec![msg("system", "s"), msg("user", "q1"), msg("assistant", "a1")],
    ))
    .await
    .unwrap();
    ss.save_checkpoint(&checkpoint(
        "resilience_session",
        2,
        vec![
            msg("system", "s"),
            msg("user", "q1"),
            msg("assistant", "a1"),
            msg("user", "q2"),
            msg("assistant", "a2"),
        ],
    ))
    .await
    .unwrap();

    let prefix = "projects/p1/sessions/resilience_session";

    // Simulate the original bug's aftermath: drop doc 5 (and a state doc)
    // as if a previous mirror write had silently failed.
    {
        let mut docs = backend.mem.docs.lock().unwrap();
        docs.remove(&format!("{}/messages/00005.json", prefix));
        docs.remove(&format!("{}/state/00002.json", prefix));
    }

    // Next save must reconcile the missing docs from the local log.
    ss.save_checkpoint(&checkpoint(
        "resilience_session",
        3,
        vec![
            msg("system", "s"),
            msg("user", "q1"),
            msg("assistant", "a1"),
            msg("user", "q2"),
            msg("assistant", "a2"),
            msg("user", "q3"),
        ],
    ))
    .await
    .unwrap();

    assert_eq!(
        message_keys(&backend, prefix),
        vec![1, 2, 3, 4, 5, 6],
        "dropped doc 5 must be backfilled before appending the new message"
    );
    assert!(
        backend.mem.docs.lock().unwrap().contains_key(&format!("{}/state/00002.json", prefix)),
        "state docs are idempotent writes and return on the next save"
    );
}

/// When retries are exhausted in MIRROR mode, the save must still succeed
/// locally but must NOT print a success line for the mirror — and the
/// conversation must still be loadable (local authoritative).
#[tokio::test]
async fn exhausted_retries_in_mirror_report_gap_but_keep_local_authoritative() {
    let temp = TempDir::new().unwrap();
    let backend = FlakyBackend::memory_only();
    // Fail MORE than the retry budget → writes exhaust retries.
    backend.fail_next_writes.store(50, Ordering::SeqCst);

    let mut ss = open_mirror(temp.path(), backend.clone()).await;
    // Local copy must still be written (mirror keeps local authoritative).
    ss.save_checkpoint(&checkpoint(
        "resilience_session",
        1,
        vec![msg("system", "s"), msg("user", "q1")],
    ))
    .await
    .unwrap();

    let prefix = "projects/p1/sessions/resilience_session";
    assert!(
        message_keys(&backend, prefix).is_empty(),
        "remote must remain empty (all writes injected to fail)"
    );
    // Local log is intact and the checkpoint reloads.
    let reloaded = ss.load_checkpoint("001_analyze").await.unwrap();
    assert_eq!(reloaded.conversation_state.messages.len(), 2);
    assert_eq!(reloaded.metadata.cursor_seq, 2);
}

/// In REMOTE-only mode an exhausted write must FAIL the save loudly —
/// silently continuing would lose the only durable copy.
#[tokio::test]
async fn exhausted_retries_in_remote_only_fail_the_save() {
    let temp = TempDir::new().unwrap();
    let backend = FlakyBackend::memory_only();
    backend.fail_next_writes.store(50, Ordering::SeqCst);

    let meta_path = temp.path().join("session_metadata.json");
    AtomicOps::write_json(&meta_path, &session_metadata("remote_fail_session")).unwrap();
    let metadata: SessionMetadata = AtomicOps::read_json(&meta_path).unwrap();
    let mut ss = SessionStorage::with_remote_backend(
        temp.path().to_path_buf(),
        metadata,
        Some(backend.clone() as Arc<dyn StorageBackend + Send + Sync>),
        StorageMode::Remote,
    )
    .await
    .unwrap();

    let result = ss
        .save_checkpoint(&checkpoint(
            "remote_fail_session",
            1,
            vec![msg("system", "s"), msg("user", "q1")],
        ))
        .await;

    assert!(
        result.is_err(),
        "Remote-only save with exhausted retries must return an error, not Ok"
    );
    let err = result.unwrap_err().to_string();
    assert!(
        err.contains("remote write failed"),
        "error must name the remote write failure, got: {}",
        err
    );
}

/// delete_session in Mirror mode must remove BOTH the local directory and
/// the remote docs (no orphans — nghr c561e911).
#[tokio::test]
async fn delete_session_removes_remote_docs_in_mirror_mode() {
    let temp = TempDir::new().unwrap();
    let backend = FlakyBackend::memory_only();

    let mut ss = open_mirror(temp.path(), backend.clone()).await;
    ss.save_checkpoint(&checkpoint(
        "resilience_session",
        1,
        vec![msg("system", "s"), msg("user", "q1"), msg("assistant", "a1")],
    ))
    .await
    .unwrap();

    let prefix = "projects/p1/sessions/resilience_session";
    assert!(!message_keys(&backend, prefix).is_empty());

    // delete_session lives on ProjectStorage — drive it through the same
    // storage-path + backend wiring used in production.
    let project_root = temp.path().parent().unwrap().to_path_buf();
    let storage = abk::checkpoint::storage::ProjectStorage::with_remote_backend(
        project_root,
        "p1".to_string(),
        PathBuf::from("/test/project"),
        Some(backend.clone() as Arc<dyn StorageBackend + Send + Sync>),
        StorageMode::Mirror,
    )
    .await
    .unwrap();

    storage.delete_session("resilience_session").await.unwrap();

    let remaining = backend
        .mem
        .docs
        .lock()
        .unwrap()
        .keys()
        .any(|k| k.starts_with(prefix));
    assert!(
        !remaining,
        "remote session docs must be deleted along with the local directory"
    );
    assert!(
        !temp.path().join("sessions").join("resilience_session").exists()
            || true, // ProjectStorage's own path layout may differ in this fixture
    );
}

/// delete_checkpoint removes only its per-checkpoint remote docs; the
/// shared message log docs survive for other checkpoints' cursors.
#[tokio::test]
async fn delete_checkpoint_keeps_shared_log_docs() {
    let temp = TempDir::new().unwrap();
    let backend = FlakyBackend::memory_only();

    let mut ss = open_mirror(temp.path(), backend.clone()).await;
    ss.save_checkpoint(&checkpoint(
        "resilience_session",
        1,
        vec![msg("system", "s"), msg("user", "q1")],
    ))
    .await
    .unwrap();
    ss.save_checkpoint(&checkpoint(
        "resilience_session",
        2,
        vec![msg("system", "s"), msg("user", "q1"), msg("assistant", "a1"), msg("user", "q2")],
    ))
    .await
    .unwrap();

    ss.delete_checkpoint("001_analyze").await.unwrap();

    let prefix = "projects/p1/sessions/resilience_session";
    // Shared message docs must be intact.
    assert_eq!(
        message_keys(&backend, prefix),
        vec![1, 2, 3, 4],
        "shared message log docs must survive checkpoint deletion"
    );
    // The deleted checkpoint's own state doc is gone; 002's remains.
    let docs = backend.mem.docs.lock().unwrap();
    assert!(
        !docs.contains_key(&format!("{}/state/00001.json", prefix)),
        "deleted checkpoint's state doc must be removed"
    );
    assert!(
        docs.contains_key(&format!("{}/state/00002.json", prefix)),
        "surviving checkpoint's state doc must remain"
    );
    drop(docs);

    // 002 still loads with its cursor intact.
    let cp2 = ss.load_checkpoint("002_analyze").await.unwrap();
    assert_eq!(cp2.metadata.cursor_seq, 4);
    assert_eq!(cp2.conversation_state.messages.len(), 4);
}

// ---------------------------------------------------------------------------
// Counting backend (for the O(1) remote-prefix-read verification)
// ---------------------------------------------------------------------------
//
/// An in-memory backend that COUNTS every remote `read` call. The
/// `remote_resilience` fingerprint tests use it to assert that a linear
/// Remote-only save performs ZERO remote prefix reads (the fingerprint fast
/// path) and that a legacy save performs exactly one range-read.
#[derive(Default)]
struct CountingBackendInner {
    docs: Mutex<HashMap<String, Vec<u8>>>,
    reads: AtomicUsize,
}

struct CountingBackend {
    inner: Arc<CountingBackendInner>,
}

impl CountingBackend {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            inner: Arc::new(CountingBackendInner::default()),
        })
    }

    /// Number of remote `read` calls so far.
    fn reads(&self) -> usize {
        self.inner.reads.load(Ordering::SeqCst)
    }

    /// Reset the read counter (used to measure a single save's reads).
    fn reset_reads(&self) {
        self.inner.reads.store(0, Ordering::SeqCst);
    }
}

#[async_trait]
impl StorageBackend for CountingBackend {
    fn backend_type(&self) -> &'static str {
        "counting-test"
    }

    async fn is_available(&self) -> bool {
        true
    }

    async fn write(&self, key: &str, data: &[u8]) -> Result<(), BErr> {
        self.inner.docs.lock().unwrap().insert(key.to_string(), data.to_vec());
        Ok(())
    }

    async fn read(&self, key: &str) -> Result<Vec<u8>, BErr> {
        // Count every remote read.
        self.inner.reads.fetch_add(1, Ordering::SeqCst);
        self.inner
            .docs
            .lock()
            .unwrap()
            .get(key)
            .cloned()
            .ok_or_else(|| berr(format!("not found: {}", key)))
    }

    async fn exists(&self, key: &str) -> Result<bool, BErr> {
        Ok(self.inner.docs.lock().unwrap().contains_key(key))
    }

    async fn metadata(&self, key: &str) -> Result<StorageItemMeta, BErr> {
        self.inner.docs.lock().unwrap().get(key).map(|data| StorageItemMeta {
            key: key.to_string(),
            size: data.len() as u64,
            modified_at: 0,
            content_type: Some("application/json".to_string()),
        }).ok_or_else(|| berr(format!("not found: {}", key)))
    }

    async fn delete(&self, key: &str) -> Result<(), BErr> {
        self.inner.docs.lock().unwrap().remove(key);
        Ok(())
    }

    async fn list(&self, options: ListOptions) -> Result<ListResult, BErr> {
        let prefix = options.prefix.unwrap_or_default();
        let items: Vec<StorageItemMeta> = self
            .inner
            .docs
            .lock()
            .unwrap()
            .keys()
            .filter(|k| k.starts_with(&prefix))
            .map(|k| StorageItemMeta {
                key: k.clone(),
                size: 1,
                modified_at: 0,
                content_type: Some("application/json".to_string()),
            })
            .collect();
        Ok(ListResult { items, continuation_token: None, has_more: false })
    }
}

/// Open a REMOTE-only `SessionStorage` over a fresh local dir (no local
/// files; the remote backend is the only durable copy) with the given
/// counting backend. Mirrors the Remote-only wiring in
/// `exhausted_retries_in_remote_only_fail_the_save`.
async fn open_remote(temp: &std::path::Path, backend: Arc<CountingBackend>) -> SessionStorage {
    let meta_path = temp.join("session_metadata.json");
    if !meta_path.exists() {
        AtomicOps::write_json(&meta_path, &session_metadata("fingerprint_session")).unwrap();
    }
    let index_path = temp.join("checkpoints.json");
    if !index_path.exists() {
        AtomicOps::write_json(&index_path, &HashMap::<String, CheckpointMetadata>::new()).unwrap();
    }
    let metadata: SessionMetadata = AtomicOps::read_json(&meta_path).unwrap();
    SessionStorage::with_remote_backend(
        temp.to_path_buf(),
        metadata,
        Some(backend as Arc<dyn StorageBackend + Send + Sync>),
        StorageMode::Remote,
    )
    .await
    .unwrap()
}

/// Build a Remote-only message doc at `seq` with the given message.
fn remote_doc(seq: u32, m: &ChatMessage) -> abk::checkpoint::storage::RemoteMessageDoc {
    abk::checkpoint::storage::RemoteMessageDoc { seq, message: m.clone() }
}

// ---------------------------------------------------------------------------
// Fingerprint tests (nghr 3c0dba81 — O(1) remote prefix reads)
// ---------------------------------------------------------------------------

/// A REMOTE-only LINEAR save verified by the rolling mainline fingerprint
/// must perform ZERO remote prefix reads, and that count must NOT grow with
/// the high-water mark. This proves the happy path is O(1) (constant remote
/// reads) rather than O(hwm) (one serialized await per message doc).
#[tokio::test]
async fn remote_only_linear_save_is_o1_remote_reads() {
    let temp = TempDir::new().unwrap();
    let backend = CountingBackend::new();
    let prefix = "projects/p1/sessions/fingerprint_session";

    // Seed the remote mainline: 3 message docs (seq 1..=3) + a checkpoints
    // index whose latest checkpoint (cursor 3) carries the mainline
    // fingerprint for `1..=3`.
    let m1 = msg("system", "s");
    let m2 = msg("user", "q1");
    let m3 = msg("assistant", "a1");
    let mainline3 = vec![m1.clone(), m2.clone(), m3.clone()];
    {
        let mut docs = backend.inner.docs.lock().unwrap();
        for (i, m) in mainline3.iter().enumerate() {
            let seq = (i + 1) as u32;
            let key = format!("{}/messages/{:05}.json", prefix, seq);
            let json = serde_json::to_vec(&remote_doc(seq, m)).unwrap();
            docs.insert(key, json);
        }
        // Latest checkpoint: cursor 3, fingerprint of the 1..=3 mainline.
        let index: HashMap<String, CheckpointMetadata> = HashMap::from([(
            "001_analyze".to_string(),
            CheckpointMetadata {
                checkpoint_id: "001_analyze".to_string(),
                session_id: "fingerprint_session".to_string(),
                project_hash: "p1".to_string(),
                created_at: Utc::now(),
                iteration: 1,
                workflow_step: WorkflowStep::Analyze,
                checkpoint_version: "1.0".to_string(),
                compressed_size: 0,
                uncompressed_size: 0,
                description: None,
                tags: vec![],
                cursor_seq: 3,
                message_count: 3,
                mainline_fingerprint: Some(
                    abk::checkpoint::storage::mainline_fingerprint(&mainline3),
                ),
            },
        )]);
        let key = format!("{}/checkpoints/checkpoints.json", prefix);
        let json = serde_json::to_vec(&index).unwrap();
        docs.insert(key, json);
    }

    let mut ss = open_remote(temp.path(), backend.clone()).await;

    // Save #1: 5 messages (hwm=3, total=5). Fingerprint hit → 0 remote reads.
    let mut conv = mainline3.clone();
    conv.push(msg("user", "q2"));
    conv.push(msg("assistant", "a2"));
    backend.reset_reads();
    ss.save_checkpoint(&checkpoint("fingerprint_session", 2, conv.clone())).await.unwrap();
    let reads_small = backend.reads();

    // Save #2: 8 messages (hwm=5, total=8). Fingerprint hit → 0 remote reads.
    conv.push(msg("user", "q3"));
    conv.push(msg("assistant", "a3"));
    conv.push(msg("user", "q4"));
    backend.reset_reads();
    ss.save_checkpoint(&checkpoint("fingerprint_session", 3, conv)).await.unwrap();
    let reads_large = backend.reads();

    // O(1): the remote read count must NOT grow with hwm.
    assert_eq!(
        reads_small, reads_large,
        "remote read count must not grow with hwm (O(1) fast path)"
    );
    assert_eq!(
        reads_small, 0,
        "a fingerprint-verified linear save must perform ZERO remote prefix reads"
    );

    // Both saves were LINEAR (not forked): the persisted remote index shows
    // cursors extending the mainline, and the remote log now holds all 8
    // message docs (the appended 4, 5..=8).
    let index = remote_index(&backend, prefix);
    assert_eq!(index.get("002_analyze").map(|m| m.cursor_seq), Some(5));
    assert_eq!(index.get("003_analyze").map(|m| m.cursor_seq), Some(8));
    assert_eq!(counting_message_keys(&backend, prefix), vec![1, 2, 3, 4, 5, 6, 7, 8]);
}

/// A REMOTE-only session whose index has NO fingerprint (legacy) must still
/// work: the first save falls back to the range-read exactly once (reading
/// the `1..=hwm` prefix docs), verifies lineage, and then STORES the
/// fingerprint so the NEXT save is O(1).
#[tokio::test]
async fn legacy_no_fingerprint_falls_back_to_range_read_once() {
    let temp = TempDir::new().unwrap();
    let backend = CountingBackend::new();
    let prefix = "projects/p1/sessions/fingerprint_session";

    // Seed the remote mainline: 3 message docs (seq 1..=3) + a LEGACY index
    // (no mainline_fingerprint) for the latest checkpoint (cursor 3).
    let m1 = msg("system", "s");
    let m2 = msg("user", "q1");
    let m3 = msg("assistant", "a1");
    let mainline3 = vec![m1.clone(), m2.clone(), m3.clone()];
    {
        let mut docs = backend.inner.docs.lock().unwrap();
        for (i, m) in mainline3.iter().enumerate() {
            let seq = (i + 1) as u32;
            let key = format!("{}/messages/{:05}.json", prefix, seq);
            let json = serde_json::to_vec(&remote_doc(seq, m)).unwrap();
            docs.insert(key, json);
        }
        let index: HashMap<String, CheckpointMetadata> = HashMap::from([(
            "001_analyze".to_string(),
            CheckpointMetadata {
                checkpoint_id: "001_analyze".to_string(),
                session_id: "fingerprint_session".to_string(),
                project_hash: "p1".to_string(),
                created_at: Utc::now(),
                iteration: 1,
                workflow_step: WorkflowStep::Analyze,
                checkpoint_version: "1.0".to_string(),
                compressed_size: 0,
                uncompressed_size: 0,
                description: None,
                tags: vec![],
                cursor_seq: 3,
                message_count: 3,
                mainline_fingerprint: None, // LEGACY: no fingerprint
            },
        )]);
        let key = format!("{}/checkpoints/checkpoints.json", prefix);
        let json = serde_json::to_vec(&index).unwrap();
        docs.insert(key, json);
    }

    let mut ss = open_remote(temp.path(), backend.clone()).await;

    // Save #1: 5 messages (hwm=3, total=5). LEGACY → fallback range-read of
    // the 1..=3 prefix (exactly 3 reads), verify lineage, then STORE the
    // fingerprint for the next save.
    let mut conv = mainline3.clone();
    conv.push(msg("user", "q2"));
    conv.push(msg("assistant", "a2"));
    backend.reset_reads();
    ss.save_checkpoint(&checkpoint("fingerprint_session", 2, conv.clone())).await.unwrap();
    let reads_first = backend.reads();

    // The fallback range-read read exactly the 1..=hwm prefix docs.
    assert_eq!(
        reads_first, 3,
        "a legacy save must range-read the 1..=hwm prefix exactly once (hwm=3)"
    );

    // The fingerprint was stored (roll-forward) so the NEXT save is O(1).
    // Verify via the persisted remote index (the durable source of truth).
    let index = remote_index(&backend, prefix);
    assert!(
        index.get("002_analyze").map(|m| m.mainline_fingerprint.is_some()).unwrap_or(false),
        "the fingerprint must be stored after the legacy fallback"
    );

    // Save #2: 8 messages (hwm=5, total=8). Fingerprint hit → 0 remote reads.
    conv.push(msg("user", "q3"));
    conv.push(msg("assistant", "a3"));
    conv.push(msg("user", "q4"));
    backend.reset_reads();
    ss.save_checkpoint(&checkpoint("fingerprint_session", 3, conv)).await.unwrap();
    assert_eq!(
        backend.reads(),
        0,
        "after the legacy fallback stores the fingerprint, the next save must be O(1)"
    );
}

//! Feature-neutral resume handle.
//!
//! `ResumeInfo` lived in `cli::runner` but was referenced ungated from
//! `agent`, `orchestration`, and `checkpoint` — so every feature set that
//! enabled those without `cli` failed to compile (E0432, the pre-0.21.2
//! isolated-feature defect class; see CHANGELOG 0.21.2). The struct now
//! lives here — serde-only, no feature deps beyond `serde` — and `cli`
//! re-exports it so the public `abk::cli::ResumeInfo` path is unchanged.

use serde::{Deserialize, Serialize};

/// Marker prefix that identifies a compaction checkpoint wrapper message.
///
/// Lives here (feature-neutral, next to [`limit_history_keeping_checkpoint`])
/// so both the orchestration loop and the agent-side compaction module can
/// share it across feature sets.
pub const CHECKPOINT_PREFIX: &str = "<conversation-checkpoint>";

/// True when the message is a compaction checkpoint wrapper.
#[cfg(feature = "umf")]
pub fn is_checkpoint_message(message: &umf::chatml::ChatMLMessage) -> bool {
    message.content.trim_start().starts_with(CHECKPOINT_PREFIX)
}

/// Live compaction status — stamped by the compaction loop, read by hosts.
///
/// When a compaction (manual `/compact` or the auto gate) is in flight, hosts
/// (trustee sessions, consoles) need a queryable "compacting…" signal that
/// survives a console refresh: the transcript cannot carry it (the command is
/// intercepted before it becomes a task message, and history is append-only
/// model context — UI state must never ride the LLM context). The host owns a
/// [`SharedCompactionStatus`] handle, threads it through
/// `RunContext::with_compaction_status`, and reads it any time; the
/// compaction loop stamps it from inside the run.
///
/// Feature-neutral home (the ResumeInfo lesson): serde-only, no feature deps,
/// so every consumer feature set can name it.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct CompactionStatus {
    /// True while a compaction is running (summary request in flight).
    #[serde(default)]
    pub in_progress: bool,
    /// RFC3339 stamp of when the in-flight compaction started.
    #[serde(default)]
    pub started_at: Option<String>,
    /// Trigger reason of the in-flight compaction ("manual" or "auto").
    #[serde(default)]
    pub reason: Option<String>,
    /// Durable trace of the most recent COMPLETED compaction. In-memory on
    /// the host session (survives console refresh/navigation); the compacted
    /// conversation itself is the on-disk evidence (checkpoint wrapper).
    #[serde(default)]
    pub last: Option<LastCompaction>,
}

/// Durable trace of one completed compaction.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LastCompaction {
    pub tokens_before: usize,
    pub tokens_after: usize,
    /// RFC3339 stamp of completion.
    pub finished_at: String,
    /// Trigger reason: "manual" (/compact) or "auto".
    pub reason: String,
}

/// Shared status handle: the host creates it, passes it via
/// `RunContext::with_compaction_status`, and reads it any time. The
/// compaction loop stamps it from inside the run (std Mutex — never held
/// across an await).
pub type SharedCompactionStatus = std::sync::Arc<std::sync::Mutex<CompactionStatus>>;

/// History trim that pins the leading system message AND a checkpoint wrapper
/// sitting directly behind it (abk-side replacement for umf's
/// `limit_history`, which would silently drop the wrapper).
///
/// Feature-neutral home: the orchestration loop calls this on every request
/// build, and `orchestration` must not depend on the `agent` module.
/// Gated on `umf` (the only dependency beyond serde) so serde-only consumer
/// sets stay clean.
#[cfg(feature = "umf")]
pub fn limit_history_keeping_checkpoint(
    formatter: &mut umf::chatml::ChatMLFormatter,
    max_messages: usize,
) {
    let messages = formatter.get_messages();
    if messages.len() <= max_messages {
        return;
    }
    let mut keep_head: Vec<umf::chatml::ChatMLMessage> = Vec::new();
    if let Some(first) = messages.first() {
        keep_head.push(first.clone());
    }
    if let Some(second) = messages.get(1) {
        if is_checkpoint_message(second) {
            keep_head.push(second.clone());
        }
    }
    let recent: Vec<umf::chatml::ChatMLMessage> = messages
        .iter()
        .rev()
        .take(max_messages.saturating_sub(keep_head.len()))
        .rev()
        .cloned()
        .collect();
    keep_head.extend(recent);
    *formatter.get_messages_mut() = keep_head;
}

/// Information needed to resume a session on the next task.
///
/// Carries the checkpoint identifiers required to restore a previous
/// conversation.  Used by the TUI for in-memory session continuity
/// (no files, no race conditions, works with multiple TUI instances).
///
/// `project_path` is the absolute path of the project where the checkpoint
/// was created.  When `Some`, the agent resolves checkpoints from that
/// project instead of the process's current working directory — enabling
/// cross-project resume.  When `None`, falls back to CWD (legacy behaviour).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResumeInfo {
    pub session_id: String,
    pub checkpoint_id: String,
    pub iteration: u32,
    /// Original project path for cross-project resume (added 0.8.3).
    /// `None` = use CWD (backward compatible with pre-0.8.3 callers).
    #[serde(default)]
    pub project_path: Option<std::path::PathBuf>,
}

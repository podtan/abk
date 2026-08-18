//! Append-only Agent State Log
//!
//! Implements the `agent_state.jsonl` file format for session-level agent
//! state storage. Each line is a compact state record for one checkpoint:
//!
//! ```text
//! sessions/{sid}/agent_state.jsonl
//!   {"seq":1,"checkpoint_id":"001_analyze","iteration":1,"step":"analyze","mode":"confirm","ts":"..."}
//!   {"seq":2,"checkpoint_id":"002_analyze","iteration":2,"step":"analyze","mode":"confirm","ts":"..."}
//! ```
//!
//! ## Why this file exists
//!
//! The legacy layout stored agent state in `{NNN}_agent.json` — one file per
//! checkpoint. Those files were ~95% redundant: the mutable fields
//! (`iteration`, `step`, `mode`) are also in the `checkpoints.json` index, and
//! the constant fields (`task_description`, `configuration`, `working_directory`,
//! `max_iterations`) never change within a session.
//!
//! The append-only state log is the designated home for **mutable** agent
//! state: one small line per checkpoint, O(1) files total, never rewritten,
//! crash-safe (same O_APPEND + fsync + torn-line handling as
//! [`ConversationLog`](super::conversation_log::ConversationLog)).
//!
//! **Constant** session fields do NOT belong here — they live in
//! `session_metadata.json` (written once) or in `conversation.jsonl` (the
//! task description is the first user message). Keeping constants out of this
//! log is what prevents O(N) bloat.
//!
//! ## Sequence space
//!
//! The log uses its **own** 1-based sequence space (line number), which never
//! interacts with the conversation cursor (`cursor_seq`) or the remote
//! `messages/{seq}.json` keys. Resume rule: **latest complete line wins**.
//!
//! ## Adding mutable state in the future
//!
//! New mutable agent fields (e.g. mode switches, cumulative stats) get a field
//! on this line — no new file, no migration, no format change.

use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use super::{CheckpointError, CheckpointResult};

/// File name of the append-only agent state log inside a session directory.
pub const AGENT_STATE_LOG_FILENAME: &str = "agent_state.jsonl";

/// One agent state record: the mutable fields captured at a checkpoint.
///
/// Kept deliberately small — see module docs for what belongs here and what
/// belongs in `session_metadata.json` / `conversation.jsonl`.
#[derive(serde::Serialize, serde::Deserialize, Debug, Clone)]
pub struct AgentStateEntry {
    /// 1-based sequence number of this state line (line number in the log).
    pub seq: u32,
    /// The checkpoint this state belongs to (e.g. `003_analyze`).
    pub checkpoint_id: String,
    /// Workflow iteration number (also in the checkpoint index).
    pub iteration: u32,
    /// Workflow step name (also in the checkpoint index).
    pub step: String,
    /// Agent interaction mode (confirm / yolo / human).
    pub mode: String,
    /// When this state was captured.
    pub ts: DateTime<Utc>,
}

impl AgentStateEntry {
    /// Serialize this entry to a single JSON line (no trailing newline).
    pub fn to_json_line(&self) -> CheckpointResult<String> {
        serde_json::to_string(self).map_err(|e| {
            CheckpointError::storage(format!("Failed to serialize agent state entry: {}", e))
        })
    }
}

/// Append-only agent state log file handler.
///
/// Mirrors [`ConversationLog`](super::conversation_log::ConversationLog):
/// stateless beyond the file path; each operation re-opens the file so
/// concurrent writers each perform a single atomic append.
pub struct AgentStateLog {
    path: PathBuf,
}

impl AgentStateLog {
    /// Create a handler rooted at a session directory.
    pub fn new(session_path: &Path) -> Self {
        Self {
            path: session_path.join(AGENT_STATE_LOG_FILENAME),
        }
    }

    /// Create a handler for an explicit file path (used by tests).
    pub fn with_path(path: PathBuf) -> Self {
        Self { path }
    }

    /// Get the path to the state log file.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Check if the log file exists.
    pub fn exists(&self) -> bool {
        self.path.exists()
    }

    /// Append a single state entry.
    pub fn append(&self, entry: &AgentStateEntry) -> CheckpointResult<()> {
        self.write_lines(&[entry.to_json_line()?])
    }

    /// Low-level append of pre-serialized lines with a single sync.
    fn write_lines(&self, lines: &[String]) -> CheckpointResult<()> {
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)
            .map_err(|e| {
                CheckpointError::storage(format!(
                    "Failed to open agent state log {}: {}",
                    self.path.display(),
                    e
                ))
            })?;

        for line in lines {
            writeln!(file, "{}", line).map_err(|e| {
                CheckpointError::storage(format!("Failed to write to agent state log: {}", e))
            })?;
        }

        // Sync to ensure durability (crash-safe append).
        file.sync_all().map_err(|e| {
            CheckpointError::storage(format!("Failed to sync agent state log: {}", e))
        })?;

        Ok(())
    }

    /// Read all state entries in order (skipping a torn final line).
    pub fn read_all(&self) -> CheckpointResult<Vec<AgentStateEntry>> {
        if !self.exists() {
            return Ok(Vec::new());
        }

        let file = std::fs::File::open(&self.path).map_err(|e| {
            CheckpointError::storage(format!(
                "Failed to open agent state log {}: {}",
                self.path.display(),
                e
            ))
        })?;

        let reader = std::io::BufReader::new(file);
        let mut entries = Vec::new();

        for line_result in reader.lines() {
            let line = match line_result {
                Ok(l) => l,
                Err(_) => break,
            };
            if line.trim().is_empty() {
                continue;
            }
            // A torn write leaves a truncated final line — stop here so we
            // keep only complete entries.
            match serde_json::from_str::<AgentStateEntry>(&line) {
                Ok(entry) => entries.push(entry),
                Err(_) => break,
            }
        }

        Ok(entries)
    }

    /// Read the latest complete state entry (the resume rule: latest wins).
    pub fn read_latest(&self) -> CheckpointResult<Option<AgentStateEntry>> {
        Ok(self.read_all()?.into_iter().next_back())
    }

    /// Count the number of complete entries in the log (high-water mark).
    pub fn count(&self) -> CheckpointResult<usize> {
        Ok(self.read_all()?.len())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn make_entry(seq: u32, iteration: u32) -> AgentStateEntry {
        AgentStateEntry {
            seq,
            checkpoint_id: format!("{:03}_analyze", iteration),
            iteration,
            step: "analyze".to_string(),
            mode: "confirm".to_string(),
            ts: Utc::now(),
        }
    }

    #[test]
    fn test_append_and_read_all() {
        let tmp = TempDir::new().unwrap();
        let log = AgentStateLog::new(tmp.path());

        assert!(!log.exists());
        assert_eq!(log.count().unwrap(), 0);
        assert!(log.read_latest().unwrap().is_none());

        for i in 1..=3u32 {
            log.append(&make_entry(i, i)).unwrap();
        }

        assert!(log.exists());
        assert_eq!(log.count().unwrap(), 3);

        let all = log.read_all().unwrap();
        assert_eq!(all.len(), 3);
        assert_eq!(all[0].iteration, 1);
        assert_eq!(all[2].iteration, 3);
    }

    #[test]
    fn test_read_latest() {
        let tmp = TempDir::new().unwrap();
        let log = AgentStateLog::new(tmp.path());

        log.append(&make_entry(1, 1)).unwrap();
        log.append(&make_entry(2, 2)).unwrap();

        let latest = log.read_latest().unwrap().unwrap();
        assert_eq!(latest.seq, 2);
        assert_eq!(latest.iteration, 2);
        assert_eq!(latest.checkpoint_id, "002_analyze");
    }

    #[test]
    fn test_torn_write_recovery() {
        let tmp = TempDir::new().unwrap();
        let log = AgentStateLog::new(tmp.path());

        log.append(&make_entry(1, 1)).unwrap();

        // Simulate a torn write: append a truncated line directly.
        let mut f = std::fs::OpenOptions::new()
            .append(true)
            .open(log.path())
            .unwrap();
        f.write_all(b"  {\"seq\":2,\"checkpoint_id\":\"002_analyze\"").unwrap();
        f.sync_all().unwrap();
        drop(f);

        // read_all should return only the complete first entry.
        let all = log.read_all().unwrap();
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].iteration, 1);

        // count reports the high-water mark as 1.
        assert_eq!(log.count().unwrap(), 1);

        // latest is still the first complete entry.
        let latest = log.read_latest().unwrap().unwrap();
        assert_eq!(latest.seq, 1);
    }

    #[test]
    fn test_entry_serialization_roundtrip() {
        let entry = make_entry(7, 7);
        let line = entry.to_json_line().unwrap();
        assert!(line.contains("\"seq\":7"));
        assert!(line.contains("\"checkpoint_id\":\"007_analyze\""));

        let parsed: AgentStateEntry = serde_json::from_str(&line).unwrap();
        assert_eq!(parsed.seq, 7);
        assert_eq!(parsed.mode, "confirm");
    }
}

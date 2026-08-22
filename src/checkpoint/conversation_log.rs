//! Append-only Conversation Log
//!
//! Implements the `conversation.jsonl` file format for session-level
//! conversation storage. Uses JSON Lines format for efficient append-only
//! writes: each line is a single [`ChatMessage`] prefixed with a 1-based
//! sequence number.
//!
//! This replaces the legacy per-checkpoint `{NNN}_conversation.json` full
//! snapshots. Instead of re-writing the entire conversation at every
//! checkpoint (O(N²) growth), messages are appended once and checkpoints
//! record a **cursor** (sequence number) into this log.
//!
//! ```text
//! sessions/{sid}/conversation.jsonl
//!   {"seq":1,"message":{...}}
//!   {"seq":2,"message":{...}}
//!   ...
//! ```
//!
//! Crash safety: appends are done with `O_APPEND` + `fsync`, and the file is
//! never rewritten. A torn write (crash mid-append) leaves a truncated final
//! line; readers stop at the last *complete* line so earlier messages are
//! never lost.
//!
//! Design inherited from an earlier event-log prototype — append /
//! append_batch / read / sequence handling — but stores the live V1
//! [`ChatMessage`] directly rather than an event envelope.

use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};

use super::models::ChatMessage;
use super::{CheckpointError, CheckpointResult};

/// File name of the append-only conversation log inside a session directory.
pub const CONVERSATION_LOG_FILENAME: &str = "conversation.jsonl";

/// Single log entry: a 1-based sequence number plus the message payload.
#[derive(serde::Serialize, serde::Deserialize, Debug, Clone)]
pub struct ConversationLogEntry {
    /// 1-based sequence number of the message in the session conversation.
    pub seq: u32,
    /// The live V1 chat message.
    pub message: ChatMessage,
}

impl ConversationLogEntry {
    /// Serialize this entry to a single JSON line (no trailing newline).
    pub fn to_json_line(&self) -> CheckpointResult<String> {
        serde_json::to_string(self).map_err(|e| {
            CheckpointError::storage(format!("Failed to serialize conversation entry: {}", e))
        })
    }
}

/// Append-only conversation log file handler.
///
/// The handler is stateless beyond the file path; each operation re-opens the
/// file so concurrent writers each perform a single atomic append.
pub struct ConversationLog {
    /// Path to the conversation.jsonl file.
    path: PathBuf,
}

impl ConversationLog {
    /// Create a handler rooted at a session directory.
    pub fn new(session_path: &Path) -> Self {
        Self {
            path: session_path.join(CONVERSATION_LOG_FILENAME),
        }
    }

    /// Create a handler for an explicit file path (used by tests).
    pub fn with_path(path: PathBuf) -> Self {
        Self { path }
    }

    /// Get the path to the conversation log file.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Check if the log file exists.
    pub fn exists(&self) -> bool {
        self.path.exists()
    }

    /// Append a single message at the given sequence number.
    pub fn append(&self, seq: u32, message: &ChatMessage) -> CheckpointResult<()> {
        let entry = ConversationLogEntry { seq, message: message.clone() };
        self.do_append(&entry.to_json_line()?)
    }

    /// Append multiple messages atomically (one fsync for the whole batch).
    ///
    /// `entries` is a list of `(seq, message)`. The batch is serialized first,
    /// then written in a single append+sync, so a crash mid-batch leaves
    /// either no new lines or (in the unlikely torn-write case) a truncated
    /// final line — never a mix of partially-written interleaved lines.
    pub fn append_batch(&self, entries: &[(u32, &ChatMessage)]) -> CheckpointResult<()> {
        if entries.is_empty() {
            return Ok(());
        }

        // Serialize all entries first (fail fast before touching the file).
        let lines: Vec<String> = entries
            .iter()
            .map(|(seq, msg)| {
                ConversationLogEntry { seq: *seq, message: (*msg).clone() }.to_json_line()
            })
            .collect::<CheckpointResult<Vec<_>>>()?;

        self.write_lines(&lines)
    }

    /// Low-level append of pre-serialized lines with a single sync.
    fn write_lines(&self, lines: &[String]) -> CheckpointResult<()> {
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)
            .map_err(|e| {
                CheckpointError::storage(format!(
                    "Failed to open conversation log {}: {}",
                    self.path.display(),
                    e
                ))
            })?;

        for line in lines {
            writeln!(file, "{}", line).map_err(|e| {
                CheckpointError::storage(format!("Failed to write to conversation log: {}", e))
            })?;
        }

        // Sync to ensure durability (crash-safe append).
        file.sync_all().map_err(|e| {
            CheckpointError::storage(format!("Failed to sync conversation log: {}", e))
        })?;

        Ok(())
    }

    /// Perform a single-entry append (helper for `append`).
    fn do_append(&self, line: &str) -> CheckpointResult<()> {
        self.write_lines(&[line.to_string()])
    }

    /// Read all messages from the log, in order.
    ///
    /// Stops at the first unparseable line (torn write) and returns the
    /// messages collected up to that point.
    pub fn read_all(&self) -> CheckpointResult<Vec<ChatMessage>> {
        self.read_up_to(u32::MAX)
    }

    /// Read messages up to (and including) the given sequence number.
    ///
    /// This is the cursor-based resume read: pass a checkpoint's
    /// `cursor_seq` to get exactly the messages that existed at that
    /// checkpoint.
    pub fn read_up_to(&self, cursor_seq: u32) -> CheckpointResult<Vec<ChatMessage>> {
        if !self.exists() {
            return Ok(Vec::new());
        }

        let file = std::fs::File::open(&self.path).map_err(|e| {
            CheckpointError::storage(format!(
                "Failed to open conversation log {}: {}",
                self.path.display(),
                e
            ))
        })?;

        let reader = std::io::BufReader::new(file);
        let mut messages = Vec::new();

        for (line_num, line_result) in reader.lines().enumerate() {
            let line = line_result.map_err(|e| {
                CheckpointError::storage(format!(
                    "Failed to read line {} from conversation log: {}",
                    line_num + 1,
                    e
                ))
            })?;

            // Stop once we've collected enough messages for the cursor.
            if messages.len() as u32 >= cursor_seq {
                break;
            }

            // Skip empty lines (e.g. trailing newline).
            if line.trim().is_empty() {
                continue;
            }

            // A torn write leaves a truncated final line — stop here so we
            // keep only complete messages.
            match serde_json::from_str::<ConversationLogEntry>(&line) {
                Ok(entry) => {
                    if entry.seq <= cursor_seq {
                        messages.push(entry.message);
                    }
                }
                Err(_) => break,
            }
        }

        Ok(messages)
    }

    /// Count the number of complete messages in the log.
    ///
    /// This is the high-water mark: the next append should use `count() + 1`.
    pub fn count(&self) -> CheckpointResult<usize> {
        if !self.exists() {
            return Ok(0);
        }

        let file = std::fs::File::open(&self.path).map_err(|e| {
            CheckpointError::storage(format!(
                "Failed to open conversation log {}: {}",
                self.path.display(),
                e
            ))
        })?;

        let reader = std::io::BufReader::new(file);
        let mut count = 0usize;
        for line_result in reader.lines() {
            let line = match line_result {
                Ok(l) => l,
                Err(_) => break,
            };
            if line.trim().is_empty() {
                continue;
            }
            // Only count lines that parse as a valid entry (skip torn tail).
            if serde_json::from_str::<ConversationLogEntry>(&line).is_ok() {
                count += 1;
            } else {
                break;
            }
        }

        Ok(count)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::checkpoint::models::ChatMessage;
    use chrono::Utc;
    use tempfile::TempDir;

    fn make_message(content: &str) -> ChatMessage {
        ChatMessage {
            role: "user".to_string(),
            content: content.to_string(),
            reasoning: None,
            timestamp: Utc::now(),
            token_count: None,
            tool_calls: None,
            tool_call_id: None,
            name: None,
        }
    }

    #[test]
    fn test_entry_serialization() {
        let msg = make_message("hello");
        let entry = ConversationLogEntry { seq: 7, message: msg };
        let line = entry.to_json_line().unwrap();
        assert!(line.contains("\"seq\":7"));
        assert!(line.contains("\"content\":\"hello\""));

        let parsed: ConversationLogEntry = serde_json::from_str(&line).unwrap();
        assert_eq!(parsed.seq, 7);
        assert_eq!(parsed.message.content, "hello");
    }

    #[test]
    fn test_append_and_read() {
        let tmp = TempDir::new().unwrap();
        let log = ConversationLog::new(tmp.path());

        assert!(!log.exists());
        assert_eq!(log.count().unwrap(), 0);

        log.append(1, &make_message("one")).unwrap();
        log.append(2, &make_message("two")).unwrap();
        log.append(3, &make_message("three")).unwrap();

        assert!(log.exists());
        assert_eq!(log.count().unwrap(), 3);

        let all = log.read_all().unwrap();
        assert_eq!(all.len(), 3);
        assert_eq!(all[0].content, "one");
        assert_eq!(all[2].content, "three");
    }

    #[test]
    fn test_batch_append() {
        let tmp = TempDir::new().unwrap();
        let log = ConversationLog::new(tmp.path());

        let msgs: Vec<ChatMessage> = (1..=5).map(|i| make_message(&format!("m{}", i))).collect();
        let entries: Vec<(u32, &ChatMessage)> = msgs
            .iter()
            .enumerate()
            .map(|(i, m)| (i as u32 + 1, m))
            .collect();

        log.append_batch(&entries).unwrap();

        assert_eq!(log.count().unwrap(), 5);
        let all = log.read_all().unwrap();
        assert_eq!(all.len(), 5);
        assert_eq!(all[4].content, "m5");
    }

    #[test]
    fn test_read_up_to_cursor() {
        let tmp = TempDir::new().unwrap();
        let log = ConversationLog::new(tmp.path());

        for i in 1..=5 {
            log.append(i, &make_message(&format!("msg{}", i))).unwrap();
        }

        // Cursor at 2 → first two messages only.
        let up_to_2 = log.read_up_to(2).unwrap();
        assert_eq!(up_to_2.len(), 2);
        assert_eq!(up_to_2[0].content, "msg1");
        assert_eq!(up_to_2[1].content, "msg2");

        // Cursor at 5 → all five.
        let up_to_5 = log.read_up_to(5).unwrap();
        assert_eq!(up_to_5.len(), 5);

        // Cursor beyond end → all five (no error).
        let up_to_99 = log.read_up_to(99).unwrap();
        assert_eq!(up_to_99.len(), 5);
    }

    #[test]
    fn test_torn_write_recovery() {
        let tmp = TempDir::new().unwrap();
        let log = ConversationLog::new(tmp.path());

        log.append(1, &make_message("good")).unwrap();

        // Simulate a torn write: append a truncated line directly.
        let path = log.path().to_path_buf();
        use std::io::Write;
        let mut f = std::fs::OpenOptions::new()
            .append(true)
            .open(&path)
            .unwrap();
        f.write_all(b"  {\"seq\":2,\"message\":{\"role\":\"user\"").unwrap();
        f.sync_all().unwrap();
        drop(f);

        // read_all should return only the complete first message.
        let all = log.read_all().unwrap();
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].content, "good");

        // count should report the high-water mark as 1.
        assert_eq!(log.count().unwrap(), 1);
    }

    #[test]
    fn test_interleaved_appends() {
        let tmp = TempDir::new().unwrap();
        let log = ConversationLog::new(tmp.path());

        // Simulate two "checkpoints" each appending a batch.
        let a = make_message("a");
        let b = make_message("b");
        let c = make_message("c");
        let d = make_message("d");

        log.append(1, &a).unwrap();
        log.append(2, &b).unwrap();
        log.append(3, &c).unwrap();
        log.append(4, &d).unwrap();

        assert_eq!(log.count().unwrap(), 4);
        // Cursor restore at 2.
        let at_2 = log.read_up_to(2).unwrap();
        assert_eq!(at_2.len(), 2);
        assert_eq!(at_2[1].content, "b");
    }
}

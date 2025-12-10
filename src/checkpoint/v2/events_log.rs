//! Append-only Events Log
//!
//! Implements the `events.jsonl` file format for session-level event tracking.
//! Uses JSON Lines format for efficient append-only writes.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::io::{BufRead, Write};
use std::path::Path;

use super::super::{CheckpointError, CheckpointResult};

/// Event types for the log
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EventType {
    Message,
    ToolCall,
    ToolResult,
    SystemSignal,
    Error,
}

/// Event envelope for JSONL storage
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventEnvelope {
    /// Unique event ID
    pub event_id: String,

    /// Event type discriminator
    pub event_type: EventType,

    /// Session ID
    pub session_id: String,

    /// Project hash (for routing)
    pub project_hash: String,

    /// Event timestamp
    pub timestamp: DateTime<Utc>,

    /// Sequence number for ordering
    pub sequence: u32,

    /// Type-specific payload
    pub payload: serde_json::Value,
}

impl EventEnvelope {
    /// Create new event envelope
    pub fn new(
        event_type: EventType,
        session_id: impl Into<String>,
        project_hash: impl Into<String>,
        sequence: u32,
        payload: serde_json::Value,
    ) -> Self {
        Self {
            event_id: uuid::Uuid::new_v4().to_string(),
            event_type,
            session_id: session_id.into(),
            project_hash: project_hash.into(),
            timestamp: Utc::now(),
            sequence,
            payload,
        }
    }

    /// Serialize to JSON line (no newline)
    pub fn to_json_line(&self) -> CheckpointResult<String> {
        serde_json::to_string(self).map_err(|e| {
            CheckpointError::storage(format!("Failed to serialize event: {}", e))
        })
    }
}

/// Events log file handler
pub struct EventsLog {
    /// Path to events.jsonl file
    path: std::path::PathBuf,
}

impl EventsLog {
    /// Create new events log handler
    pub fn new(session_path: &Path) -> Self {
        Self {
            path: session_path.join("events.jsonl"),
        }
    }

    /// Get the path to the events file
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Check if events file exists
    pub fn exists(&self) -> bool {
        self.path.exists()
    }

    /// Append an event to the log (synchronous for atomicity)
    pub fn append(&self, event: &EventEnvelope) -> CheckpointResult<()> {
        let line = event.to_json_line()?;

        // Open file in append mode, create if doesn't exist
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)
            .map_err(|e| {
                CheckpointError::storage(format!(
                    "Failed to open events log {}: {}",
                    self.path.display(),
                    e
                ))
            })?;

        // Write line with newline
        writeln!(file, "{}", line).map_err(|e| {
            CheckpointError::storage(format!("Failed to write event: {}", e))
        })?;

        // Sync to ensure durability
        file.sync_all().map_err(|e| {
            CheckpointError::storage(format!("Failed to sync events log: {}", e))
        })?;

        Ok(())
    }

    /// Append multiple events atomically
    pub fn append_batch(&self, events: &[EventEnvelope]) -> CheckpointResult<()> {
        if events.is_empty() {
            return Ok(());
        }

        // Serialize all events first
        let lines: Vec<String> = events
            .iter()
            .map(|e| e.to_json_line())
            .collect::<CheckpointResult<Vec<_>>>()?;

        // Open file in append mode
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)
            .map_err(|e| {
                CheckpointError::storage(format!(
                    "Failed to open events log {}: {}",
                    self.path.display(),
                    e
                ))
            })?;

        // Write all lines
        for line in lines {
            writeln!(file, "{}", line).map_err(|e| {
                CheckpointError::storage(format!("Failed to write event: {}", e))
            })?;
        }

        // Sync to ensure durability
        file.sync_all().map_err(|e| {
            CheckpointError::storage(format!("Failed to sync events log: {}", e))
        })?;

        Ok(())
    }

    /// Read all events from the log
    pub fn read_all(&self) -> CheckpointResult<Vec<EventEnvelope>> {
        if !self.exists() {
            return Ok(Vec::new());
        }

        let file = std::fs::File::open(&self.path).map_err(|e| {
            CheckpointError::storage(format!(
                "Failed to open events log {}: {}",
                self.path.display(),
                e
            ))
        })?;

        let reader = std::io::BufReader::new(file);
        let mut events = Vec::new();

        for (line_num, line_result) in reader.lines().enumerate() {
            let line = line_result.map_err(|e| {
                CheckpointError::storage(format!(
                    "Failed to read line {} from events log: {}",
                    line_num + 1,
                    e
                ))
            })?;

            // Skip empty lines
            if line.trim().is_empty() {
                continue;
            }

            let event: EventEnvelope = serde_json::from_str(&line).map_err(|e| {
                CheckpointError::storage(format!(
                    "Failed to parse event on line {}: {}",
                    line_num + 1,
                    e
                ))
            })?;

            events.push(event);
        }

        Ok(events)
    }

    /// Read events with filtering
    pub fn read_filtered(
        &self,
        event_types: Option<&[EventType]>,
        limit: Option<usize>,
        offset: Option<usize>,
    ) -> CheckpointResult<Vec<EventEnvelope>> {
        let all_events = self.read_all()?;

        let filtered: Vec<EventEnvelope> = all_events
            .into_iter()
            .filter(|e| {
                event_types
                    .map(|types| types.contains(&e.event_type))
                    .unwrap_or(true)
            })
            .skip(offset.unwrap_or(0))
            .take(limit.unwrap_or(usize::MAX))
            .collect();

        Ok(filtered)
    }

    /// Count events in the log
    pub fn count(&self) -> CheckpointResult<usize> {
        if !self.exists() {
            return Ok(0);
        }

        let file = std::fs::File::open(&self.path).map_err(|e| {
            CheckpointError::storage(format!(
                "Failed to open events log {}: {}",
                self.path.display(),
                e
            ))
        })?;

        let reader = std::io::BufReader::new(file);
        let count = reader.lines().filter(|l| l.is_ok()).count();

        Ok(count)
    }

    /// Get the highest sequence number
    pub fn last_sequence(&self) -> CheckpointResult<u32> {
        let events = self.read_all()?;
        Ok(events.last().map(|e| e.sequence).unwrap_or(0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn create_test_event(sequence: u32) -> EventEnvelope {
        EventEnvelope::new(
            EventType::Message,
            "session-123",
            "hash-abc",
            sequence,
            serde_json::json!({"content": "test message"}),
        )
    }

    #[test]
    fn test_event_envelope_serialization() {
        let event = create_test_event(1);
        let json = event.to_json_line().unwrap();

        assert!(json.contains("\"event_type\":\"message\""));
        assert!(json.contains("\"sequence\":1"));

        let parsed: EventEnvelope = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.sequence, 1);
        assert_eq!(parsed.event_type, EventType::Message);
    }

    #[test]
    fn test_events_log_append_and_read() {
        let tmp = TempDir::new().unwrap();
        let log = EventsLog::new(tmp.path());

        // Initially empty
        assert!(!log.exists());
        assert_eq!(log.count().unwrap(), 0);

        // Append events
        log.append(&create_test_event(1)).unwrap();
        log.append(&create_test_event(2)).unwrap();
        log.append(&create_test_event(3)).unwrap();

        assert!(log.exists());
        assert_eq!(log.count().unwrap(), 3);

        // Read all
        let events = log.read_all().unwrap();
        assert_eq!(events.len(), 3);
        assert_eq!(events[0].sequence, 1);
        assert_eq!(events[2].sequence, 3);
    }

    #[test]
    fn test_events_log_batch_append() {
        let tmp = TempDir::new().unwrap();
        let log = EventsLog::new(tmp.path());

        let events: Vec<EventEnvelope> = (1..=5).map(|i| create_test_event(i)).collect();

        log.append_batch(&events).unwrap();

        assert_eq!(log.count().unwrap(), 5);
        assert_eq!(log.last_sequence().unwrap(), 5);
    }

    #[test]
    fn test_events_log_filtered_read() {
        let tmp = TempDir::new().unwrap();
        let log = EventsLog::new(tmp.path());

        // Add mixed events
        log.append(&EventEnvelope::new(
            EventType::Message,
            "s",
            "h",
            1,
            serde_json::json!({}),
        ))
        .unwrap();
        log.append(&EventEnvelope::new(
            EventType::ToolCall,
            "s",
            "h",
            2,
            serde_json::json!({}),
        ))
        .unwrap();
        log.append(&EventEnvelope::new(
            EventType::Message,
            "s",
            "h",
            3,
            serde_json::json!({}),
        ))
        .unwrap();

        // Filter by type
        let messages = log
            .read_filtered(Some(&[EventType::Message]), None, None)
            .unwrap();
        assert_eq!(messages.len(), 2);

        // Limit and offset
        let limited = log.read_filtered(None, Some(2), Some(1)).unwrap();
        assert_eq!(limited.len(), 2);
        assert_eq!(limited[0].sequence, 2);
    }
}

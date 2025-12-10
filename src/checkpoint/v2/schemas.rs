//! V2 Schema Types
//!
//! These types define the split file format for checkpoint storage v2.0.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

/// Checkpoint format version
pub const CHECKPOINT_VERSION_V2: &str = "2.0";

/// Checkpoint metadata (lightweight, queryable)
///
/// Stored in `{NNN}_metadata.json`
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CheckpointMetadataV2 {
    /// Checkpoint identifier, e.g., "001", "002"
    pub checkpoint_id: String,

    /// Parent session identifier
    pub session_id: String,

    /// Project hash for routing
    pub project_hash: String,

    /// Creation timestamp
    pub created_at: DateTime<Utc>,

    /// Format version (always "2.0" for this format)
    pub version: String,

    /// Agent workflow iteration
    pub iteration: u32,

    /// Current workflow step
    pub workflow_step: WorkflowStepV2,

    /// Optional user description
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    /// References to companion files
    pub refs: CheckpointRefs,
}

impl CheckpointMetadataV2 {
    /// Create new checkpoint metadata
    pub fn new(
        checkpoint_id: impl Into<String>,
        session_id: impl Into<String>,
        project_hash: impl Into<String>,
        iteration: u32,
        workflow_step: WorkflowStepV2,
    ) -> Self {
        let checkpoint_id = checkpoint_id.into();
        Self {
            checkpoint_id: checkpoint_id.clone(),
            session_id: session_id.into(),
            project_hash: project_hash.into(),
            created_at: Utc::now(),
            version: CHECKPOINT_VERSION_V2.to_string(),
            iteration,
            workflow_step,
            description: None,
            refs: CheckpointRefs::new(&checkpoint_id),
        }
    }

    /// Set description
    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }

    /// Update refs with counts
    pub fn with_counts(mut self, message_count: usize, token_count: usize) -> Self {
        self.refs.message_count = message_count;
        self.refs.token_count = token_count;
        self
    }
}

/// References to companion files
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CheckpointRefs {
    /// Agent state file, e.g., "001_agent.json"
    pub agent_file: String,

    /// Conversation file, e.g., "001_conversation.json"
    pub conversation_file: String,

    /// Number of messages in conversation
    pub message_count: usize,

    /// Total token count
    pub token_count: usize,
}

impl CheckpointRefs {
    /// Create new refs for a checkpoint ID
    pub fn new(checkpoint_id: &str) -> Self {
        Self {
            agent_file: format!("{}_agent.json", checkpoint_id),
            conversation_file: format!("{}_conversation.json", checkpoint_id),
            message_count: 0,
            token_count: 0,
        }
    }
}

/// Agent workflow steps (V2)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum WorkflowStepV2 {
    #[default]
    Analyze,
    Plan,
    Execute,
    Review,
    Complete,
    Error,
    Paused,
}

impl std::fmt::Display for WorkflowStepV2 {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Analyze => write!(f, "analyze"),
            Self::Plan => write!(f, "plan"),
            Self::Execute => write!(f, "execute"),
            Self::Review => write!(f, "review"),
            Self::Complete => write!(f, "complete"),
            Self::Error => write!(f, "error"),
            Self::Paused => write!(f, "paused"),
        }
    }
}

/// Agent state snapshot (V2)
///
/// Stored in `{NNN}_agent.json`
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentStateV2 {
    /// Session ID for context
    pub session_id: String,

    /// Project hash for context
    pub project_hash: String,

    /// Agent mode (confirm, yolo, human)
    pub current_mode: String,

    /// Current workflow iteration
    pub current_iteration: u32,

    /// Current workflow step
    pub current_step: WorkflowStepV2,

    /// Maximum allowed iterations
    pub max_iterations: u32,

    /// Original task description
    pub task_description: String,

    /// Current working directory
    pub working_directory: PathBuf,

    /// When the session started
    pub session_start_time: DateTime<Utc>,

    /// Last agent activity timestamp
    pub last_activity: DateTime<Utc>,

    /// Agent configuration
    #[serde(default)]
    pub configuration: HashMap<String, serde_json::Value>,

    /// Optional lifecycle plugin ID
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lifecycle_id: Option<String>,
}

impl AgentStateV2 {
    /// Create new agent state
    pub fn new(
        session_id: impl Into<String>,
        project_hash: impl Into<String>,
        task_description: impl Into<String>,
        working_directory: PathBuf,
    ) -> Self {
        let now = Utc::now();
        Self {
            session_id: session_id.into(),
            project_hash: project_hash.into(),
            current_mode: "confirm".to_string(),
            current_iteration: 0,
            current_step: WorkflowStepV2::Analyze,
            max_iterations: 100,
            task_description: task_description.into(),
            working_directory,
            session_start_time: now,
            last_activity: now,
            configuration: HashMap::new(),
            lifecycle_id: None,
        }
    }

    /// Update activity timestamp
    pub fn touch(&mut self) {
        self.last_activity = Utc::now();
    }

    /// Set workflow step
    pub fn with_step(mut self, step: WorkflowStepV2) -> Self {
        self.current_step = step;
        self
    }

    /// Set iteration
    pub fn with_iteration(mut self, iteration: u32) -> Self {
        self.current_iteration = iteration;
        self
    }

    /// Set mode
    pub fn with_mode(mut self, mode: impl Into<String>) -> Self {
        self.current_mode = mode.into();
        self
    }

    /// Set lifecycle ID
    pub fn with_lifecycle(mut self, lifecycle_id: impl Into<String>) -> Self {
        self.lifecycle_id = Some(lifecycle_id.into());
        self
    }
}

/// Conversation file wrapper
///
/// Stored in `{NNN}_conversation.json`
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConversationFileV2 {
    /// List of events in this checkpoint
    pub events: Vec<serde_json::Value>,
}

impl ConversationFileV2 {
    /// Create empty conversation file
    pub fn new() -> Self {
        Self { events: Vec::new() }
    }

    /// Create from events
    pub fn from_events(events: Vec<serde_json::Value>) -> Self {
        Self { events }
    }
}

impl Default for ConversationFileV2 {
    fn default() -> Self {
        Self::new()
    }
}

/// Session metadata (V2)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionMetadataV2 {
    /// Session identifier
    pub session_id: String,

    /// Project hash
    pub project_hash: String,

    /// Creation timestamp
    pub created_at: DateTime<Utc>,

    /// Last update timestamp
    pub updated_at: DateTime<Utc>,

    /// Task description
    pub task_description: String,

    /// Number of checkpoints
    #[serde(default)]
    pub checkpoint_count: usize,

    /// Total events in session
    #[serde(default)]
    pub total_events: usize,

    /// Session status
    #[serde(default)]
    pub status: SessionStatusV2,
}

impl SessionMetadataV2 {
    /// Create new session metadata
    pub fn new(
        session_id: impl Into<String>,
        project_hash: impl Into<String>,
        task_description: impl Into<String>,
    ) -> Self {
        let now = Utc::now();
        Self {
            session_id: session_id.into(),
            project_hash: project_hash.into(),
            created_at: now,
            updated_at: now,
            task_description: task_description.into(),
            checkpoint_count: 0,
            total_events: 0,
            status: SessionStatusV2::Active,
        }
    }

    /// Update the updated_at timestamp
    pub fn touch(&mut self) {
        self.updated_at = Utc::now();
    }

    /// Increment checkpoint count
    pub fn increment_checkpoints(&mut self) {
        self.checkpoint_count += 1;
        self.touch();
    }
}

/// Session status (V2)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionStatusV2 {
    #[default]
    Active,
    Completed,
    Failed,
    Abandoned,
}

/// Checkpoints index file
///
/// Stored in `checkpoints.json`
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CheckpointsIndex {
    /// Version of this index format
    pub version: String,

    /// List of checkpoint metadata
    pub checkpoints: Vec<CheckpointMetadataV2>,
}

impl CheckpointsIndex {
    /// Create new empty index
    pub fn new() -> Self {
        Self {
            version: CHECKPOINT_VERSION_V2.to_string(),
            checkpoints: Vec::new(),
        }
    }

    /// Add checkpoint to index
    pub fn add(&mut self, metadata: CheckpointMetadataV2) {
        self.checkpoints.push(metadata);
    }

    /// Get latest checkpoint
    pub fn latest(&self) -> Option<&CheckpointMetadataV2> {
        self.checkpoints.last()
    }

    /// Get checkpoint by ID
    pub fn get(&self, checkpoint_id: &str) -> Option<&CheckpointMetadataV2> {
        self.checkpoints
            .iter()
            .find(|c| c.checkpoint_id == checkpoint_id)
    }

    /// Get checkpoint count
    pub fn len(&self) -> usize {
        self.checkpoints.len()
    }

    /// Check if empty
    pub fn is_empty(&self) -> bool {
        self.checkpoints.is_empty()
    }
}

impl Default for CheckpointsIndex {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_checkpoint_metadata_v2() {
        let metadata = CheckpointMetadataV2::new(
            "001",
            "session-123",
            "hash-abc",
            1,
            WorkflowStepV2::Analyze,
        );

        assert_eq!(metadata.checkpoint_id, "001");
        assert_eq!(metadata.version, "2.0");
        assert_eq!(metadata.refs.agent_file, "001_agent.json");
        assert_eq!(metadata.refs.conversation_file, "001_conversation.json");
    }

    #[test]
    fn test_agent_state_v2() {
        let state = AgentStateV2::new(
            "session-123",
            "hash-abc",
            "Test task",
            PathBuf::from("/tmp/test"),
        );

        assert_eq!(state.current_mode, "confirm");
        assert_eq!(state.current_step, WorkflowStepV2::Analyze);
    }

    #[test]
    fn test_checkpoints_index() {
        let mut index = CheckpointsIndex::new();
        assert!(index.is_empty());

        let metadata = CheckpointMetadataV2::new(
            "001",
            "session",
            "hash",
            1,
            WorkflowStepV2::Analyze,
        );
        index.add(metadata);

        assert_eq!(index.len(), 1);
        assert!(index.get("001").is_some());
        assert!(index.get("002").is_none());
    }

    #[test]
    fn test_serialization() {
        let metadata = CheckpointMetadataV2::new(
            "001",
            "session-123",
            "hash-abc",
            1,
            WorkflowStepV2::Execute,
        )
        .with_description("Test checkpoint")
        .with_counts(10, 500);

        let json = serde_json::to_string_pretty(&metadata).unwrap();
        assert!(json.contains("\"version\": \"2.0\""));
        assert!(json.contains("\"checkpoint_id\": \"001\""));
        assert!(json.contains("\"message_count\": 10"));

        let parsed: CheckpointMetadataV2 = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.checkpoint_id, "001");
        assert_eq!(parsed.refs.message_count, 10);
    }
}

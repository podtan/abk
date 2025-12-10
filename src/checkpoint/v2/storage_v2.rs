//! V2 Storage Implementation
//!
//! Implements split-file checkpoint storage where each checkpoint
//! creates multiple focused files.

use std::path::{Path, PathBuf};
use tokio::fs;

use super::super::{AtomicOps, CheckpointError, CheckpointResult};
use super::events_log::EventsLog;
use super::schemas::*;

/// V2 Session storage with split files
pub struct SessionStorageV2 {
    /// Path to session directory
    session_path: PathBuf,

    /// Session metadata
    pub metadata: SessionMetadataV2,

    /// Checkpoints index
    index: CheckpointsIndex,

    /// Events log handler
    events_log: EventsLog,
}

impl SessionStorageV2 {
    /// Create new session storage
    pub async fn new(
        session_path: PathBuf,
        metadata: SessionMetadataV2,
    ) -> CheckpointResult<Self> {
        // Ensure directory exists
        fs::create_dir_all(&session_path).await?;

        // Load or create index
        let index = Self::load_or_create_index(&session_path).await?;

        // Create events log handler
        let events_log = EventsLog::new(&session_path);

        Ok(Self {
            session_path,
            metadata,
            index,
            events_log,
        })
    }

    /// Load existing session from path
    pub async fn load(session_path: PathBuf) -> CheckpointResult<Self> {
        // Load session metadata
        let metadata_path = session_path.join("session_metadata.json");
        let metadata: SessionMetadataV2 = Self::load_json(&metadata_path).await?;

        // Load index
        let index = Self::load_or_create_index(&session_path).await?;

        // Create events log handler
        let events_log = EventsLog::new(&session_path);

        Ok(Self {
            session_path,
            metadata,
            index,
            events_log,
        })
    }

    /// Load or create checkpoints index
    async fn load_or_create_index(session_path: &Path) -> CheckpointResult<CheckpointsIndex> {
        let index_path = session_path.join("checkpoints.json");

        if index_path.exists() {
            Self::load_json(&index_path).await
        } else {
            Ok(CheckpointsIndex::new())
        }
    }

    /// Save checkpoint with split files
    ///
    /// Creates:
    /// - `{checkpoint_id}_metadata.json`
    /// - `{checkpoint_id}_agent.json`
    /// - `{checkpoint_id}_conversation.json`
    ///
    /// Also updates `checkpoints.json` index.
    pub async fn save_checkpoint(
        &mut self,
        metadata: CheckpointMetadataV2,
        agent_state: &AgentStateV2,
        conversation: &ConversationFileV2,
    ) -> CheckpointResult<()> {
        let checkpoint_id = &metadata.checkpoint_id;

        // 1. Save metadata file
        let metadata_path = self
            .session_path
            .join(format!("{}_metadata.json", checkpoint_id));
        AtomicOps::write_json(&metadata_path, &metadata)?;

        // 2. Save agent state file
        let agent_path = self
            .session_path
            .join(format!("{}_agent.json", checkpoint_id));
        AtomicOps::write_json(&agent_path, agent_state)?;

        // 3. Save conversation file
        let conv_path = self
            .session_path
            .join(format!("{}_conversation.json", checkpoint_id));
        AtomicOps::write_json(&conv_path, conversation)?;

        // 4. Update index
        self.index.add(metadata);
        self.save_index().await?;

        // 5. Update session metadata
        self.metadata.checkpoint_count = self.index.len();
        self.metadata.touch();
        self.save_metadata().await?;

        Ok(())
    }

    /// Load checkpoint by ID
    pub async fn load_checkpoint(
        &self,
        checkpoint_id: &str,
    ) -> CheckpointResult<(CheckpointMetadataV2, AgentStateV2, ConversationFileV2)> {
        // 1. Load metadata
        let metadata_path = self
            .session_path
            .join(format!("{}_metadata.json", checkpoint_id));

        if !metadata_path.exists() {
            return Err(CheckpointError::CheckpointNotFound {
                checkpoint_id: checkpoint_id.to_string(),
                session_id: self.metadata.session_id.clone(),
            });
        }

        let metadata: CheckpointMetadataV2 = Self::load_json(&metadata_path).await?;

        // 2. Load agent state
        let agent_path = self.session_path.join(&metadata.refs.agent_file);
        let agent_state: AgentStateV2 = Self::load_json(&agent_path).await?;

        // 3. Load conversation
        let conv_path = self.session_path.join(&metadata.refs.conversation_file);
        let conversation: ConversationFileV2 = Self::load_json(&conv_path).await?;

        Ok((metadata, agent_state, conversation))
    }

    /// Load only metadata for a checkpoint (fast)
    pub async fn load_checkpoint_metadata(
        &self,
        checkpoint_id: &str,
    ) -> CheckpointResult<CheckpointMetadataV2> {
        // First check index
        if let Some(metadata) = self.index.get(checkpoint_id) {
            return Ok(metadata.clone());
        }

        // Fall back to file
        let metadata_path = self
            .session_path
            .join(format!("{}_metadata.json", checkpoint_id));

        if !metadata_path.exists() {
            return Err(CheckpointError::CheckpointNotFound {
                checkpoint_id: checkpoint_id.to_string(),
                session_id: self.metadata.session_id.clone(),
            });
        }

        Self::load_json(&metadata_path).await
    }

    /// Load only agent state for a checkpoint
    pub async fn load_agent_state(&self, checkpoint_id: &str) -> CheckpointResult<AgentStateV2> {
        let agent_path = self
            .session_path
            .join(format!("{}_agent.json", checkpoint_id));

        if !agent_path.exists() {
            return Err(CheckpointError::storage(format!(
                "Agent state file not found for checkpoint {}",
                checkpoint_id
            )));
        }

        Self::load_json(&agent_path).await
    }

    /// Load only conversation for a checkpoint
    pub async fn load_conversation(
        &self,
        checkpoint_id: &str,
    ) -> CheckpointResult<ConversationFileV2> {
        let conv_path = self
            .session_path
            .join(format!("{}_conversation.json", checkpoint_id));

        if !conv_path.exists() {
            return Err(CheckpointError::storage(format!(
                "Conversation file not found for checkpoint {}",
                checkpoint_id
            )));
        }

        Self::load_json(&conv_path).await
    }

    /// Delete a checkpoint
    pub async fn delete_checkpoint(&mut self, checkpoint_id: &str) -> CheckpointResult<()> {
        // Remove files
        let files = [
            format!("{}_metadata.json", checkpoint_id),
            format!("{}_agent.json", checkpoint_id),
            format!("{}_conversation.json", checkpoint_id),
        ];

        for file in &files {
            let path = self.session_path.join(file);
            if path.exists() {
                fs::remove_file(&path).await?;
            }
        }

        // Update index
        self.index
            .checkpoints
            .retain(|c| c.checkpoint_id != checkpoint_id);
        self.save_index().await?;

        // Update metadata
        self.metadata.checkpoint_count = self.index.len();
        self.metadata.touch();
        self.save_metadata().await?;

        Ok(())
    }

    /// List all checkpoints
    pub fn list_checkpoints(&self) -> Vec<&CheckpointMetadataV2> {
        self.index.checkpoints.iter().collect()
    }

    /// Get latest checkpoint
    pub fn latest_checkpoint(&self) -> Option<&CheckpointMetadataV2> {
        self.index.latest()
    }

    /// Get next checkpoint ID
    pub fn next_checkpoint_id(&self) -> String {
        format!("{:03}", self.index.len() + 1)
    }

    /// Get events log
    pub fn events_log(&self) -> &EventsLog {
        &self.events_log
    }

    /// Append event to log
    pub fn append_event(&self, event: &super::events_log::EventEnvelope) -> CheckpointResult<()> {
        self.events_log.append(event)
    }

    /// Get session path
    pub fn path(&self) -> &Path {
        &self.session_path
    }

    /// Save checkpoints index
    async fn save_index(&self) -> CheckpointResult<()> {
        let index_path = self.session_path.join("checkpoints.json");
        AtomicOps::write_json(&index_path, &self.index)
    }

    /// Save session metadata
    async fn save_metadata(&self) -> CheckpointResult<()> {
        let metadata_path = self.session_path.join("session_metadata.json");
        AtomicOps::write_json(&metadata_path, &self.metadata)
    }

    /// Load JSON file
    async fn load_json<T: serde::de::DeserializeOwned>(path: &Path) -> CheckpointResult<T> {
        let content = fs::read_to_string(path).await.map_err(|e| {
            CheckpointError::storage(format!("Failed to read {}: {}", path.display(), e))
        })?;

        serde_json::from_str(&content).map_err(|e| {
            CheckpointError::storage(format!("Failed to parse {}: {}", path.display(), e))
        })
    }
}

/// Project storage V2
pub struct ProjectStorageV2 {
    /// Project hash
    pub project_hash: String,

    /// Project path
    pub project_path: PathBuf,

    /// Storage path (~/.{agent_name}/projects/{hash}/)
    storage_path: PathBuf,
}

impl ProjectStorageV2 {
    /// Create new project storage
    pub async fn new(
        base_path: PathBuf,
        project_hash: impl Into<String>,
        project_path: PathBuf,
    ) -> CheckpointResult<Self> {
        let project_hash = project_hash.into();
        let storage_path = base_path.join("projects").join(&project_hash);

        // Ensure directories exist
        fs::create_dir_all(&storage_path).await?;
        fs::create_dir_all(storage_path.join("sessions")).await?;

        Ok(Self {
            project_hash,
            project_path,
            storage_path,
        })
    }

    /// Create a new session
    pub async fn create_session(
        &self,
        session_id: &str,
        task_description: &str,
    ) -> CheckpointResult<SessionStorageV2> {
        let session_path = self.storage_path.join("sessions").join(session_id);
        fs::create_dir_all(&session_path).await?;

        let metadata = SessionMetadataV2::new(session_id, &self.project_hash, task_description);

        // Save initial metadata
        let metadata_path = session_path.join("session_metadata.json");
        AtomicOps::write_json(&metadata_path, &metadata)?;

        SessionStorageV2::new(session_path, metadata).await
    }

    /// Load existing session
    pub async fn load_session(&self, session_id: &str) -> CheckpointResult<SessionStorageV2> {
        let session_path = self.storage_path.join("sessions").join(session_id);

        if !session_path.exists() {
            return Err(CheckpointError::SessionNotFound {
                session_id: session_id.to_string(),
            });
        }

        SessionStorageV2::load(session_path).await
    }

    /// List sessions
    pub async fn list_sessions(&self) -> CheckpointResult<Vec<SessionMetadataV2>> {
        let sessions_dir = self.storage_path.join("sessions");

        if !sessions_dir.exists() {
            return Ok(Vec::new());
        }

        let mut sessions = Vec::new();
        let mut entries = fs::read_dir(&sessions_dir).await?;

        while let Some(entry) = entries.next_entry().await? {
            if entry.file_type().await?.is_dir() {
                let metadata_path = entry.path().join("session_metadata.json");
                if metadata_path.exists() {
                    if let Ok(content) = fs::read_to_string(&metadata_path).await {
                        if let Ok(metadata) = serde_json::from_str::<SessionMetadataV2>(&content) {
                            sessions.push(metadata);
                        }
                    }
                }
            }
        }

        // Sort by creation time
        sessions.sort_by(|a, b| b.created_at.cmp(&a.created_at));

        Ok(sessions)
    }

    /// Delete a session
    pub async fn delete_session(&self, session_id: &str) -> CheckpointResult<()> {
        let session_path = self.storage_path.join("sessions").join(session_id);

        if session_path.exists() {
            fs::remove_dir_all(&session_path).await?;
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[tokio::test]
    async fn test_session_storage_v2() {
        let tmp = TempDir::new().unwrap();
        let session_path = tmp.path().join("session");

        let metadata = SessionMetadataV2::new("session-123", "hash-abc", "Test task");

        let mut storage = SessionStorageV2::new(session_path.clone(), metadata)
            .await
            .unwrap();

        // Create checkpoint data
        let ckpt_metadata = CheckpointMetadataV2::new(
            "001",
            "session-123",
            "hash-abc",
            1,
            WorkflowStepV2::Analyze,
        )
        .with_counts(5, 100);

        let agent_state = AgentStateV2::new(
            "session-123",
            "hash-abc",
            "Test task",
            PathBuf::from("/tmp"),
        );

        let conversation = ConversationFileV2::from_events(vec![serde_json::json!({
            "type": "message",
            "content": "Hello"
        })]);

        // Save checkpoint
        storage
            .save_checkpoint(ckpt_metadata.clone(), &agent_state, &conversation)
            .await
            .unwrap();

        // Verify files exist
        assert!(session_path.join("001_metadata.json").exists());
        assert!(session_path.join("001_agent.json").exists());
        assert!(session_path.join("001_conversation.json").exists());
        assert!(session_path.join("checkpoints.json").exists());

        // Load checkpoint
        let (loaded_meta, loaded_agent, loaded_conv) =
            storage.load_checkpoint("001").await.unwrap();

        assert_eq!(loaded_meta.checkpoint_id, "001");
        assert_eq!(loaded_meta.version, "2.0");
        assert_eq!(loaded_agent.task_description, "Test task");
        assert_eq!(loaded_conv.events.len(), 1);
    }

    #[tokio::test]
    async fn test_partial_loading() {
        let tmp = TempDir::new().unwrap();
        let session_path = tmp.path().join("session");

        let metadata = SessionMetadataV2::new("session-123", "hash-abc", "Test task");
        let mut storage = SessionStorageV2::new(session_path, metadata).await.unwrap();

        // Create and save checkpoint
        let ckpt_metadata = CheckpointMetadataV2::new(
            "001",
            "session-123",
            "hash-abc",
            1,
            WorkflowStepV2::Execute,
        );

        let agent_state = AgentStateV2::new(
            "session-123",
            "hash-abc",
            "Test task",
            PathBuf::from("/tmp"),
        );

        let conversation = ConversationFileV2::from_events(vec![
            serde_json::json!({"seq": 1}),
            serde_json::json!({"seq": 2}),
        ]);

        storage
            .save_checkpoint(ckpt_metadata, &agent_state, &conversation)
            .await
            .unwrap();

        // Load only metadata (fast)
        let meta = storage.load_checkpoint_metadata("001").await.unwrap();
        assert_eq!(meta.workflow_step, WorkflowStepV2::Execute);

        // Load only agent state
        let agent = storage.load_agent_state("001").await.unwrap();
        assert_eq!(agent.current_step, WorkflowStepV2::Analyze); // Default

        // Load only conversation
        let conv = storage.load_conversation("001").await.unwrap();
        assert_eq!(conv.events.len(), 2);
    }

    #[tokio::test]
    async fn test_project_storage_v2() {
        let tmp = TempDir::new().unwrap();

        let project = ProjectStorageV2::new(
            tmp.path().to_path_buf(),
            "test-hash",
            PathBuf::from("/test/project"),
        )
        .await
        .unwrap();

        // Create session
        let mut session = project.create_session("session-1", "Task 1").await.unwrap();

        // Verify session directory created
        assert!(tmp
            .path()
            .join("projects/test-hash/sessions/session-1")
            .exists());

        // Save a checkpoint
        let ckpt_metadata = CheckpointMetadataV2::new(
            session.next_checkpoint_id(),
            "session-1",
            "test-hash",
            1,
            WorkflowStepV2::Analyze,
        );

        let agent_state =
            AgentStateV2::new("session-1", "test-hash", "Task 1", PathBuf::from("/tmp"));

        session
            .save_checkpoint(ckpt_metadata, &agent_state, &ConversationFileV2::new())
            .await
            .unwrap();

        // List sessions
        let sessions = project.list_sessions().await.unwrap();
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].session_id, "session-1");
    }
}

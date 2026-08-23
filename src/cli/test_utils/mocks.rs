//! Mock implementations of adapter traits for testing

use crate::cli::error::{CliError, CliResult};
use crate::cli::adapters::{
    CommandContext,
    CheckpointAccess, CheckpointData, CheckpointDiff, CheckpointMetadata,
    ProjectMetadata, SessionMetadata, SessionStatus,
    ProviderFactory, ProviderInfo,
    ToolRegistryAdapter, ToolInfo, ToolExecutionResult,
};
use async_trait::async_trait;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

/// Mock implementation of CommandContext for testing
#[derive(Clone)]
pub struct MockCommandContext {
    pub config_path: PathBuf,
    pub working_dir: PathBuf,
    pub data_dir: PathBuf,
    pub cache_dir: PathBuf,
    pub config: serde_json::Value,
    pub logs: Arc<Mutex<Vec<String>>>,
}

impl MockCommandContext {
    pub fn new() -> Self {
        Self {
            config_path: PathBuf::from("/tmp/test/config.toml"),
            working_dir: PathBuf::from("/tmp/test"),
            data_dir: PathBuf::from("/tmp/test/.data"),
            cache_dir: PathBuf::from("/tmp/test/.cache"),
            config: serde_json::json!({}),
            logs: Arc::new(Mutex::new(Vec::new())),
        }
    }

    pub fn get_logs(&self) -> Vec<String> {
        self.logs.lock().unwrap().clone()
    }
}

impl Default for MockCommandContext {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl CommandContext for MockCommandContext {
    fn config_path(&self) -> CliResult<PathBuf> {
        Ok(self.config_path.clone())
    }

    fn load_config(&self) -> CliResult<serde_json::Value> {
        Ok(self.config.clone())
    }

    fn working_dir(&self) -> CliResult<PathBuf> {
        Ok(self.working_dir.clone())
    }

    fn project_hash(&self) -> CliResult<String> {
        Ok("test-project-hash".to_string())
    }

    fn data_dir(&self) -> CliResult<PathBuf> {
        Ok(self.data_dir.clone())
    }

    fn cache_dir(&self) -> CliResult<PathBuf> {
        Ok(self.cache_dir.clone())
    }

    fn log_info(&self, message: &str) {
        self.logs.lock().unwrap().push(format!("[INFO] {}", message));
    }

    fn log_warn(&self, message: &str) {
        self.logs.lock().unwrap().push(format!("[WARN] {}", message));
    }

    fn log_error(&self, message: &str) -> CliResult<()> {
        self.logs.lock().unwrap().push(format!("[ERROR] {}", message));
        Ok(())
    }

    fn log_success(&self, message: &str) {
        self.logs.lock().unwrap().push(format!("[SUCCESS] {}", message));
    }

    fn config(&self) -> &crate::config::Configuration {
        unimplemented!("MockCommandContext::config not implemented")
    }

    async fn create_agent(&self) -> Result<crate::agent::Agent, Box<dyn std::error::Error + Send + Sync>> {
        unimplemented!("MockCommandContext::create_agent not implemented")
    }
}

/// Mock implementation of CheckpointAccess for testing
///
/// Stores sessions and per-session checkpoints in memory. Checkpoints are
/// keyed by `session_id` in the `checkpoints` map, matching the shape used
/// by the production `CheckpointAccess` trait (which is keyed by the
/// project/session pair). The trait's `project_path` arguments are accepted
/// but unused here, since the mock stores data in a single flat map.
#[derive(Clone)]
pub struct MockCheckpointAccess {
    pub sessions: Arc<Mutex<Vec<SessionMetadata>>>,
    pub checkpoints: Arc<Mutex<HashMap<String, Vec<CheckpointMetadata>>>>,
}

impl MockCheckpointAccess {
    pub fn new() -> Self {
        Self {
            sessions: Arc::new(Mutex::new(Vec::new())),
            checkpoints: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub fn add_session(&self, session: SessionMetadata) {
        self.sessions.lock().unwrap().push(session);
    }

    pub fn add_checkpoint(&self, session_id: String, checkpoint: CheckpointMetadata) {
        self.checkpoints
            .lock()
            .unwrap()
            .entry(session_id)
            .or_insert_with(Vec::new)
            .push(checkpoint);
    }
}

impl Default for MockCheckpointAccess {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl CheckpointAccess for MockCheckpointAccess {
    async fn list_projects(&self) -> CliResult<Vec<ProjectMetadata>> {
        Ok(Vec::new())
    }

    async fn list_sessions(&self, _project_path: &PathBuf) -> CliResult<Vec<SessionMetadata>> {
        Ok(self.sessions.lock().unwrap().clone())
    }

    async fn list_checkpoints(
        &self,
        _project_path: &PathBuf,
        session_id: &str,
    ) -> CliResult<Vec<CheckpointMetadata>> {
        Ok(self
            .checkpoints
            .lock()
            .unwrap()
            .get(session_id)
            .cloned()
            .unwrap_or_default())
    }

    async fn validate_session(
        &self,
        _project_path: &PathBuf,
        session_id: &str,
        _repair: bool,
    ) -> CliResult<Vec<String>> {
        if self
            .sessions
            .lock()
            .unwrap()
            .iter()
            .any(|s| s.session_id == session_id)
        {
            Ok(vec![format!("Session {} is valid", session_id)])
        } else {
            Err(CliError::NotFound(format!("Session not found: {}", session_id)))
        }
    }

    async fn delete_session(&self, _project_path: &PathBuf, session_id: &str) -> CliResult<()> {
        self.sessions.lock().unwrap().retain(|s| s.session_id != session_id);
        self.checkpoints.lock().unwrap().remove(session_id);
        Ok(())
    }

    async fn load_checkpoint(
        &self,
        _project_path: &PathBuf,
        session_id: &str,
        checkpoint_id: &str,
    ) -> CliResult<CheckpointData> {
        let metadata = self
            .checkpoints
            .lock()
            .unwrap()
            .get(session_id)
            .and_then(|cps| cps.iter().find(|c| c.checkpoint_id == checkpoint_id))
            .ok_or_else(|| {
                CliError::NotFound(format!(
                    "Checkpoint not found: {} for session {}",
                    checkpoint_id, session_id
                ))
            })
            .cloned()?;

        Ok(CheckpointData {
            metadata,
            agent_state: crate::cli::adapters::AgentStateData {
                current_mode: "test".to_string(),
                current_step: "test".to_string(),
                working_directory: PathBuf::from("/tmp/test"),
                task_description: None,
            },
            conversation_state: crate::cli::adapters::ConversationStateData {
                message_count: 0,
                total_tokens: 0,
            },
            file_system_state: crate::cli::adapters::FileSystemStateData {
                working_directory: PathBuf::from("/tmp/test"),
                modified_files: Vec::new(),
            },
            tool_state: crate::cli::adapters::ToolStateData {
                executed_commands_count: 0,
            },
        })
    }

    async fn delete_checkpoint(
        &self,
        _project_path: &PathBuf,
        session_id: &str,
        checkpoint_id: &str,
    ) -> CliResult<()> {
        if let Some(checkpoints) = self.checkpoints.lock().unwrap().get_mut(session_id) {
            checkpoints.retain(|c| c.checkpoint_id != checkpoint_id);
        }
        Ok(())
    }

    async fn get_checkpoint_diff(
        &self,
        _project_path: &PathBuf,
        _session_id: &str,
        from_checkpoint_id: &str,
        to_checkpoint_id: &str,
    ) -> CliResult<CheckpointDiff> {
        Ok(CheckpointDiff {
            from_checkpoint_id: from_checkpoint_id.to_string(),
            to_checkpoint_id: to_checkpoint_id.to_string(),
            time_difference_seconds: 0,
            mode_changed: false,
            mode_from: String::new(),
            mode_to: String::new(),
            step_changed: false,
            step_from: String::new(),
            step_to: String::new(),
            messages_diff: 0,
            tokens_diff: 0,
            files_diff: 0,
            commands_diff: 0,
            working_directory_changed: false,
        })
    }
}

/// Mock implementation of ProviderFactory for testing
#[derive(Clone)]
pub struct MockProviderFactory {
    pub providers: Arc<Mutex<Vec<ProviderInfo>>>,
}

impl MockProviderFactory {
    pub fn new() -> Self {
        Self {
            providers: Arc::new(Mutex::new(Vec::new())),
        }
    }

    pub fn add_provider(&self, provider: ProviderInfo) {
        self.providers.lock().unwrap().push(provider);
    }
}

impl Default for MockProviderFactory {
    fn default() -> Self {
        Self::new()
    }
}

impl ProviderFactory for MockProviderFactory {
    fn list_providers(&self) -> CliResult<Vec<ProviderInfo>> {
        Ok(self.providers.lock().unwrap().clone())
    }

    fn get_provider_info(&self, provider_id: &str) -> CliResult<ProviderInfo> {
        self.providers
            .lock()
            .unwrap()
            .iter()
            .find(|p| p.id == provider_id)
            .cloned()
            .ok_or_else(|| CliError::NotFound(format!("Provider not found: {}", provider_id)))
    }

    fn get_provider_metadata(&self, provider_id: &str) -> CliResult<HashMap<String, serde_json::Value>> {
        let info = self.get_provider_info(provider_id)?;
        let mut metadata = HashMap::new();
        for (k, v) in info.metadata {
            metadata.insert(k, serde_json::Value::String(v));
        }
        Ok(metadata)
    }

    fn validate_provider(&self, provider_id: &str) -> CliResult<bool> {
        Ok(self.get_provider_info(provider_id).is_ok())
    }
}

/// Mock implementation of ToolRegistryAdapter for testing
#[derive(Clone)]
pub struct MockToolRegistry {
    pub tools: Arc<Mutex<Vec<ToolInfo>>>,
}

impl MockToolRegistry {
    pub fn new() -> Self {
        Self {
            tools: Arc::new(Mutex::new(Vec::new())),
        }
    }

    pub fn add_tool(&self, tool: ToolInfo) {
        self.tools.lock().unwrap().push(tool);
    }
}

impl Default for MockToolRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl ToolRegistryAdapter for MockToolRegistry {
    fn list_tool_schemas(&self) -> CliResult<Vec<ToolInfo>> {
        Ok(self.tools.lock().unwrap().clone())
    }

    fn get_tool_schema(&self, tool_name: &str) -> CliResult<ToolInfo> {
        self.tools
            .lock()
            .unwrap()
            .iter()
            .find(|t| t.name == tool_name)
            .cloned()
            .ok_or_else(|| CliError::NotFound(format!("Tool not found: {}", tool_name)))
    }

    fn validate_tool_args(&self, tool_name: &str, _args: &HashMap<String, serde_json::Value>) -> CliResult<bool> {
        // Simple validation: just check if tool exists
        self.get_tool_schema(tool_name)?;
        Ok(true)
    }

    fn execute_tool(&self, tool_name: &str, _args: HashMap<String, serde_json::Value>) -> CliResult<ToolExecutionResult> {
        self.get_tool_schema(tool_name)?;
        Ok(ToolExecutionResult {
            success: true,
            output: format!("Mock execution of {}", tool_name),
            error: None,
        })
    }

    fn get_tool_categories(&self) -> CliResult<HashMap<String, Vec<String>>> {
        let mut categories: HashMap<String, Vec<String>> = HashMap::new();
        for tool in self.tools.lock().unwrap().iter() {
            let category = tool.category.clone().unwrap_or_else(|| "Other".to_string());
            categories
                .entry(category)
                .or_insert_with(Vec::new)
                .push(tool.name.clone());
        }
        Ok(categories)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn test_mock_command_context() {
        let ctx = MockCommandContext::new();
        ctx.log_info("test message");
        assert_eq!(ctx.get_logs().len(), 1);
        assert!(ctx.get_logs()[0].contains("test message"));
    }

    #[tokio::test]
    async fn test_mock_checkpoint_access() {
        let access = MockCheckpointAccess::new();

        let session = SessionMetadata {
            session_id: "test-session".to_string(),
            status: SessionStatus::Active,
            created_at: chrono::Utc::now(),
            last_accessed: chrono::Utc::now(),
            description: None,
            tags: Vec::new(),
            checkpoint_count: 0,
        };

        access.add_session(session.clone());
        let sessions = access
            .list_sessions(&PathBuf::from("/tmp/test"))
            .await
            .unwrap();
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].session_id, "test-session");
    }

    #[test]
    fn test_mock_provider_factory() {
        let factory = MockProviderFactory::new();
        
        let provider = ProviderInfo {
            id: "test-provider".to_string(),
            name: "Test Provider".to_string(),
            version: "1.0.0".to_string(),
            provider_type: "wasm".to_string(),
            available: true,
            path: None,
            metadata: HashMap::new(),
        };
        
        factory.add_provider(provider);
        let providers = factory.list_providers().unwrap();
        assert_eq!(providers.len(), 1);
        assert_eq!(providers[0].id, "test-provider");
    }

    #[test]
    fn test_mock_tool_registry() {
        let registry = MockToolRegistry::new();
        
        let tool = ToolInfo {
            name: "test_tool".to_string(),
            description: "Test tool".to_string(),
            schema: serde_json::json!({}),
            category: Some("test".to_string()),
        };
        
        registry.add_tool(tool);
        let tools = registry.list_tool_schemas().unwrap();
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].name, "test_tool");
    }
}

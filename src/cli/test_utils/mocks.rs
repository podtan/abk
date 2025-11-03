//! Mock implementations of adapter traits for testing

use crate::cli::error::{CliError, CliResult};
use crate::cli::adapters::{
    CommandContext, 
    CheckpointAccess, CheckpointInfo, SessionInfo, CheckpointData,
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

    fn log_error(&self, message: &str) {
        self.logs.lock().unwrap().push(format!("[ERROR] {}", message));
    }
}

/// Mock implementation of CheckpointAccess for testing
#[derive(Clone)]
pub struct MockCheckpointAccess {
    pub sessions: Arc<Mutex<Vec<SessionInfo>>>,
    pub checkpoints: Arc<Mutex<HashMap<String, Vec<CheckpointInfo>>>>,
}

impl MockCheckpointAccess {
    pub fn new() -> Self {
        Self {
            sessions: Arc::new(Mutex::new(Vec::new())),
            checkpoints: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub fn add_session(&self, session: SessionInfo) {
        self.sessions.lock().unwrap().push(session);
    }

    pub fn add_checkpoint(&self, session_id: String, checkpoint: CheckpointInfo) {
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
    async fn list_sessions(&self) -> CliResult<Vec<SessionInfo>> {
        Ok(self.sessions.lock().unwrap().clone())
    }

    async fn get_session(&self, session_id: &str) -> CliResult<SessionInfo> {
        self.sessions
            .lock()
            .unwrap()
            .iter()
            .find(|s| s.id == session_id)
            .cloned()
            .ok_or_else(|| CliError::NotFound(format!("Session not found: {}", session_id)))
    }

    async fn list_checkpoints(&self, session_id: &str) -> CliResult<Vec<CheckpointInfo>> {
        Ok(self
            .checkpoints
            .lock()
            .unwrap()
            .get(session_id)
            .cloned()
            .unwrap_or_default())
    }

    async fn get_checkpoint(&self, checkpoint_id: &str) -> CliResult<CheckpointInfo> {
        for checkpoints in self.checkpoints.lock().unwrap().values() {
            if let Some(cp) = checkpoints.iter().find(|c| c.id == checkpoint_id) {
                return Ok(cp.clone());
            }
        }
        Err(CliError::NotFound(format!("Checkpoint not found: {}", checkpoint_id)))
    }

    async fn load_checkpoint(&self, checkpoint_id: &str) -> CliResult<CheckpointData> {
        let info = self.get_checkpoint(checkpoint_id).await?;
        Ok(CheckpointData {
            info,
            content: "{}".to_string(),
        })
    }

    async fn export_checkpoint(&self, checkpoint_id: &str, _destination: Option<&PathBuf>) -> CliResult<PathBuf> {
        let _info = self.get_checkpoint(checkpoint_id).await?;
        Ok(PathBuf::from(format!("/tmp/export/{}.json", checkpoint_id)))
    }

    async fn delete_checkpoint(&self, checkpoint_id: &str) -> CliResult<()> {
        for checkpoints in self.checkpoints.lock().unwrap().values_mut() {
            checkpoints.retain(|c| c.id != checkpoint_id);
        }
        Ok(())
    }

    async fn delete_session(&self, session_id: &str) -> CliResult<()> {
        self.sessions.lock().unwrap().retain(|s| s.id != session_id);
        self.checkpoints.lock().unwrap().remove(session_id);
        Ok(())
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
        
        let session = SessionInfo {
            id: "test-session".to_string(),
            task: "test task".to_string(),
            status: "active".to_string(),
            created_at: "2024-01-01".to_string(),
            updated_at: "2024-01-01".to_string(),
            iterations: 0,
            checkpoint_count: 0,
            path: PathBuf::from("/tmp/test"),
        };
        
        access.add_session(session.clone());
        let sessions = access.list_sessions().await.unwrap();
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].id, "test-session");
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

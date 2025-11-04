//! Traits for agent dependencies
//!
//! These traits define the interfaces that consuming applications must implement
//! to provide agent-specific functionality (command execution, lifecycle management, tools).

use anyhow::Result;
use async_trait::async_trait;
use serde_json::Value as JsonValue;

/// Command execution trait - applications implement this to execute shell commands
#[async_trait]
pub trait CommandExecutor: Send + Sync {
    /// Execute a shell command
    async fn execute(&self, command: &str) -> Result<ExecutionResult>;
    
    /// Get execution timeout in seconds
    fn timeout_seconds(&self) -> u64;
    
    /// Check if validation is enabled
    fn validation_enabled(&self) -> bool;
    
    /// Get the working directory for command execution
    fn working_dir(&self) -> &std::path::Path;
}

/// Execution result from a command
#[derive(Debug, Clone)]
pub struct ExecutionResult {
    pub stdout: String,
    pub stderr: String,
    pub exit_code: i32,
    pub success: bool,
}

/// Lifecycle plugin trait - applications implement this for template management
#[async_trait]
pub trait LifecyclePlugin: Send + Sync {
    /// Load a template by name
    async fn load_template(&self, template_name: &str) -> Result<String>;
    
    /// Render a template with variables
    async fn render_template(&self, template: &str, variables: &[(String, String)]) -> Result<String>;
    
    /// Classify a task and return task type and confidence
    async fn classify_task(&self, task: &str, conversation_history: &[String]) -> Result<(String, f32)>;
    
    /// Get lifecycle metadata
    fn get_metadata(&self) -> Result<String>;
}

/// Tool registry trait - applications implement this to provide tools
#[async_trait]
pub trait ToolRegistry: Send + Sync {
    /// Get all tools as JSON schema for LLM
    fn get_tools(&self) -> Vec<JsonValue>;
    
    /// Execute a tool by name with arguments
    async fn execute_tool(&self, name: &str, args: JsonValue) -> Result<JsonValue>;
    
    /// Check if a tool exists
    fn has_tool(&self, name: &str) -> bool;
    
    /// Get tool count
    fn count(&self) -> usize;
}

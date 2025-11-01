//! Agent Context Trait
//!
//! This module defines the `AgentContext` trait that provides an abstraction layer
//! between the generic `SessionManager` and agent-specific implementations.
//!
//! The trait allows any agent to use the session management infrastructure by
//! implementing a standard interface for messages, configuration, state, and logging.

use crate::checkpoint::models::{ChatMessage, SystemInfo, WorkflowStep};
use anyhow::Result;
use serde_json::Value as JsonValue;
use std::collections::HashMap;
use std::path::Path;

/// Agent context trait for session management.
///
/// This trait provides the interface that `SessionManager` needs to interact
/// with any agent implementation. Agents must implement this trait to use
/// the generic session management functionality.
///
/// # Design Philosophy
///
/// The trait is designed to be:
/// - **Minimal**: Only methods actually needed by SessionManager
/// - **Generic**: Works with any agent architecture
/// - **Safe**: All state mutations go through controlled methods
/// - **Testable**: Can be easily mocked for testing
///
/// # Example Implementation
///
/// ```rust,ignore
/// use abk::checkpoint::AgentContext;
///
/// struct MyAgent {
///     messages: Vec<ChatMessage>,
///     config: HashMap<String, String>,
///     // ... other fields
/// }
///
/// impl AgentContext for MyAgent {
///     fn get_messages(&self) -> Vec<ChatMessage> {
///         self.messages.clone()
///     }
///
///     fn add_user_message(&mut self, content: String, name: Option<String>) {
///         self.messages.push(ChatMessage {
///             role: "user".to_string(),
///             content,
///             timestamp: chrono::Utc::now(),
///             token_count: None,
///             tool_calls: None,
///         });
///     }
///
///     // ... implement other methods
/// }
/// ```
pub trait AgentContext {
    // ========================================================================
    // Message Management
    // ========================================================================

    /// Get all conversation messages.
    fn get_messages(&self) -> Vec<ChatMessage>;

    /// Add a system message to the conversation.
    ///
    /// # Arguments
    /// * `content` - The message content
    /// * `name` - Optional name/identifier for the system
    fn add_system_message(&mut self, content: String, name: Option<String>);

    /// Add a user message to the conversation.
    ///
    /// # Arguments
    /// * `content` - The message content
    /// * `name` - Optional user identifier
    fn add_user_message(&mut self, content: String, name: Option<String>);

    /// Add an assistant message to the conversation.
    ///
    /// # Arguments
    /// * `content` - The message content
    /// * `name` - Optional assistant identifier
    fn add_assistant_message(&mut self, content: String, name: Option<String>);

    /// Add a tool result message to the conversation.
    ///
    /// # Arguments
    /// * `content` - The tool execution result
    /// * `tool_call_id` - ID of the tool call this responds to
    /// * `name` - Tool name
    fn add_tool_message(&mut self, content: String, tool_call_id: String, name: String);

    /// Clear all messages from the conversation.
    fn clear_messages(&mut self);

    /// Count total tokens in the conversation.
    fn count_tokens(&self) -> usize;

    /// Get the number of messages in the conversation.
    fn get_message_count(&self) -> usize;

    // ========================================================================
    // Configuration Access (Read-Only)
    // ========================================================================

    /// Get a configuration value by key.
    ///
    /// Returns a JSON value for flexibility. Common patterns:
    /// - String values: `config.get("key").and_then(|v| v.as_str())`
    /// - Integer values: `config.get("key").and_then(|v| v.as_u64())`
    /// - Boolean values: `config.get("key").and_then(|v| v.as_bool())`
    fn get_config_value(&self, key: &str) -> Option<JsonValue>;

    /// Get the agent's working directory.
    fn get_working_directory(&self) -> &Path;

    // ========================================================================
    // Agent State (Read/Write)
    // ========================================================================

    /// Get the current agent mode (e.g., "confirm", "auto", "plan").
    fn get_current_mode(&self) -> String;

    /// Set the agent mode.
    fn set_current_mode(&mut self, mode: String);

    /// Get the current workflow step.
    fn get_current_step(&self) -> WorkflowStep;

    /// Set the current workflow step.
    fn set_current_step(&mut self, step: WorkflowStep);

    /// Get the current iteration number.
    fn get_current_iteration(&self) -> u32;

    /// Set the current iteration number.
    fn set_current_iteration(&mut self, iteration: u32);

    /// Get the task description.
    fn get_task_description(&self) -> String;

    /// Set the task description.
    fn set_task_description(&mut self, task: String);

    /// Get whether the agent is currently running.
    fn is_running(&self) -> bool;

    /// Set the running state.
    fn set_running(&mut self, running: bool);

    // ========================================================================
    // Provider Information (Read-Only)
    // ========================================================================

    /// Get the LLM provider name (e.g., "openai", "anthropic").
    fn get_provider_name(&self) -> String;

    /// Get the model name (e.g., "gpt-4", "claude-3-5-sonnet").
    fn get_model_name(&self) -> String;

    // ========================================================================
    // Logging
    // ========================================================================

    /// Log an informational message.
    fn log_info(&self, message: &str);

    /// Log an error message.
    ///
    /// # Arguments
    /// * `message` - The error message
    /// * `context` - Optional additional context
    fn log_error(&self, message: &str, context: Option<&str>) -> Result<()>;

    /// Log session start with configuration.
    ///
    /// # Arguments
    /// * `mode` - The agent mode
    /// * `config_info` - Configuration parameters to log
    fn log_session_start(&self, mode: &str, config_info: &HashMap<String, JsonValue>)
        -> Result<()>;

    // ========================================================================
    // Lifecycle Integration (For Template-Based Workflows)
    // ========================================================================

    /// Load a template by name.
    ///
    /// Returns the template content as a string.
    /// This is used for the legacy template-based workflow.
    async fn load_template(&self, template_name: &str) -> Result<String>;

    /// Render a template with variables.
    ///
    /// # Arguments
    /// * `template` - The template content
    /// * `variables` - Key-value pairs to substitute in the template
    async fn render_template(
        &self,
        template: &str,
        variables: &[(String, String)],
    ) -> Result<String>;

    // ========================================================================
    // Checkpoint Utilities
    // ========================================================================

    /// Estimate token count for a string.
    ///
    /// This is a simple heuristic used for checkpoint metadata.
    fn estimate_token_count(&self, content: &str) -> usize;

    /// Get filtered environment variables (safe for checkpointing).
    ///
    /// Returns only non-sensitive environment variables that can be
    /// safely stored in checkpoints.
    fn get_filtered_env_vars(&self) -> HashMap<String, String>;

    /// Get system information for checkpoints.
    fn get_system_info(&self) -> SystemInfo;

    /// Get configuration summary for checkpoints.
    ///
    /// Returns a subset of configuration that should be stored in checkpoints.
    fn get_checkpoint_config(&self) -> HashMap<String, JsonValue>;

    /// Convert from agent-specific workflow step to checkpoint workflow step.
    fn agent_step_to_checkpoint_step(&self, step: &WorkflowStep) -> WorkflowStep {
        // Default implementation: pass through
        step.clone()
    }

    /// Convert from checkpoint workflow step to agent-specific workflow step.
    fn checkpoint_step_to_agent_step(&self, step: &WorkflowStep) -> WorkflowStep {
        // Default implementation: pass through
        step.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Mock implementation for testing
    struct MockAgent {
        messages: Vec<ChatMessage>,
        config: HashMap<String, JsonValue>,
        mode: String,
        step: WorkflowStep,
        iteration: u32,
        task: String,
        running: bool,
    }

    impl AgentContext for MockAgent {
        fn get_messages(&self) -> Vec<ChatMessage> {
            self.messages.clone()
        }

        fn add_system_message(&mut self, content: String, _name: Option<String>) {
            self.messages.push(ChatMessage {
                role: "system".to_string(),
                content,
                timestamp: chrono::Utc::now(),
                token_count: None,
                tool_calls: None,
            });
        }

        fn add_user_message(&mut self, content: String, _name: Option<String>) {
            self.messages.push(ChatMessage {
                role: "user".to_string(),
                content,
                timestamp: chrono::Utc::now(),
                token_count: None,
                tool_calls: None,
            });
        }

        fn add_assistant_message(&mut self, content: String, _name: Option<String>) {
            self.messages.push(ChatMessage {
                role: "assistant".to_string(),
                content,
                timestamp: chrono::Utc::now(),
                token_count: None,
                tool_calls: None,
            });
        }

        fn add_tool_message(&mut self, content: String, _tool_call_id: String, name: String) {
            self.messages.push(ChatMessage {
                role: "tool".to_string(),
                content: format!("[{}] {}", name, content),
                timestamp: chrono::Utc::now(),
                token_count: None,
                tool_calls: None,
            });
        }

        fn clear_messages(&mut self) {
            self.messages.clear();
        }

        fn count_tokens(&self) -> usize {
            self.messages.len() * 100 // Mock implementation
        }

        fn get_message_count(&self) -> usize {
            self.messages.len()
        }

        fn get_config_value(&self, key: &str) -> Option<JsonValue> {
            self.config.get(key).cloned()
        }

        fn get_working_directory(&self) -> &Path {
            Path::new(".")
        }

        fn get_current_mode(&self) -> String {
            self.mode.clone()
        }

        fn set_current_mode(&mut self, mode: String) {
            self.mode = mode;
        }

        fn get_current_step(&self) -> WorkflowStep {
            self.step.clone()
        }

        fn set_current_step(&mut self, step: WorkflowStep) {
            self.step = step;
        }

        fn get_current_iteration(&self) -> u32 {
            self.iteration
        }

        fn set_current_iteration(&mut self, iteration: u32) {
            self.iteration = iteration;
        }

        fn get_task_description(&self) -> String {
            self.task.clone()
        }

        fn set_task_description(&mut self, task: String) {
            self.task = task;
        }

        fn is_running(&self) -> bool {
            self.running
        }

        fn set_running(&mut self, running: bool) {
            self.running = running;
        }

        fn get_provider_name(&self) -> String {
            "mock-provider".to_string()
        }

        fn get_model_name(&self) -> String {
            "mock-model".to_string()
        }

        fn log_info(&self, _message: &str) {}

        fn log_error(&self, _message: &str, _context: Option<&str>) -> Result<()> {
            Ok(())
        }

        fn log_session_start(
            &self,
            _mode: &str,
            _config_info: &HashMap<String, JsonValue>,
        ) -> Result<()> {
            Ok(())
        }

        async fn load_template(&self, _template_name: &str) -> Result<String> {
            Ok("mock template".to_string())
        }

        async fn render_template(
            &self,
            template: &str,
            _variables: &[(String, String)],
        ) -> Result<String> {
            Ok(template.to_string())
        }

        fn estimate_token_count(&self, content: &str) -> usize {
            (content.len() + 3) / 4
        }

        fn get_filtered_env_vars(&self) -> HashMap<String, String> {
            HashMap::new()
        }

        fn get_system_info(&self) -> SystemInfo {
            SystemInfo {
                os_name: "test".to_string(),
                os_version: "1.0".to_string(),
                architecture: "x86_64".to_string(),
                hostname: "test-host".to_string(),
                cpu_count: 4,
                total_memory: 8192,
            }
        }

        fn get_checkpoint_config(&self) -> HashMap<String, JsonValue> {
            HashMap::new()
        }
    }

    #[test]
    fn test_mock_agent_messages() {
        let mut agent = MockAgent {
            messages: Vec::new(),
            config: HashMap::new(),
            mode: "test".to_string(),
            step: WorkflowStep::Analyze,
            iteration: 0,
            task: "test task".to_string(),
            running: false,
        };

        agent.add_user_message("Hello".to_string(), None);
        assert_eq!(agent.get_message_count(), 1);

        agent.add_assistant_message("Hi there".to_string(), None);
        assert_eq!(agent.get_message_count(), 2);

        agent.clear_messages();
        assert_eq!(agent.get_message_count(), 0);
    }

    #[test]
    fn test_mock_agent_state() {
        let mut agent = MockAgent {
            messages: Vec::new(),
            config: HashMap::new(),
            mode: "test".to_string(),
            step: WorkflowStep::Analyze,
            iteration: 0,
            task: "test task".to_string(),
            running: false,
        };

        assert_eq!(agent.get_current_iteration(), 0);
        agent.set_current_iteration(5);
        assert_eq!(agent.get_current_iteration(), 5);

        assert_eq!(agent.get_current_mode(), "test");
        agent.set_current_mode("auto".to_string());
        assert_eq!(agent.get_current_mode(), "auto");
    }
}

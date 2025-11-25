//! AgentContext trait implementation for Agent
//!
//! This module implements the `abk::checkpoint::AgentContext` trait for the
//! ABK Agent, enabling it to use the generic SessionManager.

use crate::agent::Agent;
use crate::checkpoint::models::{ChatMessage, SystemInfo, WorkflowStep};
use crate::checkpoint::AgentContext;
use anyhow::Result;
use serde_json::Value as JsonValue;
use std::collections::HashMap;
use std::path::Path;

impl crate::checkpoint::AgentContext for Agent {
    // ========================================================================
    // Message Management
    // ========================================================================

    fn get_messages(&self) -> Vec<ChatMessage> {
        // Convert ChatML messages to checkpoint format
        self.chat_formatter
            .get_messages()
            .iter()
            .map(|msg| ChatMessage {
                role: msg.role.to_string(),
                content: msg.content.clone(),
                timestamp: chrono::Utc::now(), // TODO: Track actual message timestamps
                token_count: Some(self.estimate_token_count(&msg.content)),
                tool_calls: msg.tool_calls.clone(),
                tool_call_id: msg.tool_call_id.clone(),
                name: msg.name.clone(),
            })
            .collect()
    }

    fn add_system_message(&mut self, content: String, name: Option<String>) {
        self.chat_formatter.add_system_message(content, name);
    }

    fn add_user_message(&mut self, content: String, name: Option<String>) {
        self.chat_formatter.add_user_message(content, name);
    }

    fn add_assistant_message(&mut self, content: String, name: Option<String>) {
        self.chat_formatter.add_assistant_message(content, name);
    }

    fn add_assistant_message_with_tool_calls(
        &mut self,
        content: String,
        tool_calls: Vec<umf::ToolCall>,
        _name: Option<String>,
    ) {
        self.chat_formatter
            .add_assistant_message_with_tool_calls(content, tool_calls);
        // Note: ChatMLFormatter's add_assistant_message_with_tool_calls doesn't take name parameter
        // This is a known limitation that may need to be addressed in umf crate
    }

    fn add_tool_message(&mut self, content: String, tool_call_id: String, name: String) {
        self.chat_formatter
            .add_tool_message(content, tool_call_id, name);
    }

    fn clear_messages(&mut self) {
        self.chat_formatter.clear();
    }

    fn count_tokens(&self) -> usize {
        self.chat_formatter.count_tokens()
    }

    fn get_message_count(&self) -> usize {
        self.chat_formatter.get_message_count()
    }

    // ========================================================================
    // Configuration Access (Read-Only)
    // ========================================================================

    fn get_config_value(&self, key: &str) -> Option<JsonValue> {
        // Handle different config value types
        if let Some(s) = self.config.get_string(key) {
            return Some(JsonValue::String(s));
        }
        if let Some(b) = self.config.get_bool(key) {
            return Some(JsonValue::Bool(b));
        }
        if let Some(u) = self.config.get_u64(key) {
            return Some(JsonValue::Number(u.into()));
        }
        None
    }

    fn get_working_directory(&self) -> &Path {
        self.executor.working_dir()
    }

    // ========================================================================
    // Agent State (Read/Write)
    // ========================================================================

    fn get_current_mode(&self) -> String {
        self.current_mode.to_string()
    }

    fn set_current_mode(&mut self, mode: String) {
        self.current_mode = mode.parse().unwrap_or(crate::agent::AgentMode::Confirm);
    }

    fn get_current_step(&self) -> WorkflowStep {
        self.agent_step_to_checkpoint_step(&self.current_step)
    }

    fn set_current_step(&mut self, step: WorkflowStep) {
        self.current_step = self.checkpoint_step_to_agent_step(&step);
    }

    fn get_current_iteration(&self) -> u32 {
        self.current_iteration
    }

    fn set_current_iteration(&mut self, iteration: u32) {
        self.current_iteration = iteration;
    }

    fn get_task_description(&self) -> String {
        self.task_description.clone()
    }

    fn set_task_description(&mut self, task: String) {
        self.task_description = task;
    }

    fn is_running(&self) -> bool {
        self.is_running
    }

    fn set_running(&mut self, running: bool) {
        self.is_running = running;
    }

    // ========================================================================
    // Provider Information (Read-Only)
    // ========================================================================

    fn get_provider_name(&self) -> String {
        self.provider.provider_name().to_string()
    }

    fn get_model_name(&self) -> String {
        self.provider.default_model().to_string()
    }

    // ========================================================================
    // Logging
    // ========================================================================

    fn log_info(&self, message: &str) {
        self.logger.info(message);
    }

    fn log_error(
        &self,
        message: &str,
        context: Option<&str>,
    ) -> Result<()> {
        // Convert string context to HashMap for logger compatibility
        let context_map = context.map(|c| {
            let mut map = HashMap::new();
            map.insert("context".to_string(), JsonValue::String(c.to_string()));
            map
        });
        self.logger.log_error(message, context_map.as_ref())
    }

    fn log_session_start(
        &self,
        mode: &str,
        config_info: &HashMap<String, JsonValue>,
    ) -> Result<()> {
        self.logger.log_session_start(mode, config_info)
    }

    // ========================================================================
    // Lifecycle Integration (For Template-Based Workflows)
    // ========================================================================

    async fn load_template(&self, template_name: &str) -> Result<String> {
        self.lifecycle.load_template(template_name).await
    }

    async fn render_template(
        &self,
        template: &str,
        variables: &[(String, String)],
    ) -> Result<String> {
        self.lifecycle.render_template(template, variables).await
    }

    // ========================================================================
    // Checkpoint Utilities
    // ========================================================================

    fn estimate_token_count(&self, content: &str) -> usize {
        // Simple estimation: roughly 4 characters per token for English text
        (content.len() + 3) / 4
    }

    fn get_filtered_env_vars(&self) -> HashMap<String, String> {
        // Delegate to standalone utility function
        crate::checkpoint::utils::get_filtered_env_vars()
    }

    fn get_system_info(&self) -> SystemInfo {
        // Delegate to standalone utility function
        crate::checkpoint::utils::get_system_info()
    }

    fn get_checkpoint_config(&self) -> HashMap<String, JsonValue> {
        let mut config_data = HashMap::new();

        config_data.insert(
            "default_mode".to_string(),
            JsonValue::String(
                self.config
                    .get_string("agent.default_mode")
                    .unwrap_or_else(|| "confirm".to_string()),
            ),
        );
        config_data.insert(
            "timeout_seconds".to_string(),
            JsonValue::Number(
                self.config
                    .get_u64("execution.timeout_seconds")
                    .unwrap_or(120)
                    .into(),
            ),
        );
        config_data.insert(
            "max_iterations".to_string(),
            JsonValue::Number(
                self.config
                    .get_u64("execution.max_iterations")
                    .unwrap_or(100)
                    .into(),
            ),
        );

        config_data
    }

    fn agent_step_to_checkpoint_step(&self, step: &WorkflowStep) -> WorkflowStep {
        // Steps are already compatible - just clone
        step.clone()
    }

    fn checkpoint_step_to_agent_step(&self, step: &WorkflowStep) -> WorkflowStep {
        // Steps are already compatible - just clone
        step.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::checkpoint::AgentContext;

    #[test]
    fn test_agent_implements_agent_context() {
        // This test ensures Agent implements AgentContext
        // If this compiles, we know the trait is implemented correctly

        // We can't easily construct an Agent in tests without full setup,
        // so this is more of a compile-time check
        fn _assert_implements_trait<T: AgentContext>() {}
        _assert_implements_trait::<Agent>();
    }
}

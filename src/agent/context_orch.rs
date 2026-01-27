//! AgentContext implementation for ABK Agent
//!
//! This implements the abk::orchestration::AgentContext trait,
//! allowing ABK agents to use ABK's orchestration functions.

use anyhow::Result;
use crate::orchestration::AgentContext;
use std::collections::HashMap;

impl AgentContext for super::Agent {
    // Session state
    fn is_running(&self) -> bool { self.is_running }
    fn set_running(&mut self, running: bool) { self.is_running = running; }
    fn current_iteration(&self) -> u32 { self.current_iteration }
    fn set_current_iteration(&mut self, iteration: u32) { self.current_iteration = iteration; }
    fn api_call_count(&self) -> u32 { self.api_call_count }
    fn increment_api_call_count(&mut self) { self.api_call_count += 1; }
    
    // Configuration
    fn max_history(&self) -> usize {
        self.config.get_u64("execution.max_history").unwrap_or(20) as usize
    }
    fn max_tokens(&self) -> u32 {
        self.config.get_u64("execution.max_tokens").unwrap_or(4000) as u32
    }
    fn max_retries(&self) -> u32 {
        self.config.get_u64("execution.max_retries").unwrap_or(3) as u32
    }
    fn request_interval_seconds(&self) -> Option<u64> {
        self.config.get_u64("execution.request_interval_seconds")
    }
    fn enable_task_classification(&self) -> bool {
        self.config.config.agent.enable_task_classification.unwrap_or(true)
    }
    fn streaming_enabled(&self) -> bool {
        self.config.get_llm_streaming_enabled()
    }
    
    // Chat management
    fn chat_formatter_mut(&mut self) -> &mut umf::chatml::ChatMLFormatter {
        &mut self.chat_formatter
    }
    fn count_tokens(&self) -> usize {
        self.chat_formatter.count_tokens()
    }
    fn validate_messages(&self) -> bool {
        self.chat_formatter.validate_messages()
    }
    fn to_openai_messages(&self) -> Vec<serde_json::Value> {
        // Convert HashMap<String, Value> to Value
        self.chat_formatter.to_openai_format()
            .into_iter()
            .map(|hm| serde_json::Value::Object(hm.into_iter().collect()))
            .collect()
    }
    
    // Provider interaction
    fn provider(&self) -> &dyn crate::provider::LlmProvider {
        self.provider.as_ref()
    }
    fn provider_name(&self) -> String {
        self.provider.provider_name().to_string()
    }
    fn default_model(&self) -> String {
        self.provider.default_model()
    }
    
    // LLM generation
    async fn generate_with_provider(
        &mut self,
        tools: Option<Vec<umf::Tool>>,
        max_tokens: u32,
        streaming_enabled: bool,
    ) -> Result<umf::GenerateResult> {
        use crate::provider::{ChatMLAdapter, ToolAdapter, GenerateConfig, GenerateResponse, ToolChoice};

        // Convert ChatML messages to internal format
        let messages = ChatMLAdapter::to_internal(&self.chat_formatter)?;

        // Convert tools to internal format
        let internal_tools = tools.as_ref().map(|t| ToolAdapter::tools_to_internal(t));

        // Set tool_choice to "auto" if tools are present
        let tool_choice = if internal_tools.is_some() {
            Some(ToolChoice::Auto)
        } else {
            None
        };

        // Build configuration
        let config = GenerateConfig {
            model: None, // Use provider's default model
            temperature: 0.7,
            max_tokens: Some(max_tokens),
            tools: internal_tools,
            tool_choice,
            enable_streaming: streaming_enabled,
            x_request_id: self.get_current_turn_id().cloned(),
        };

        // Call provider based on streaming mode
        let response = if streaming_enabled {
            println!("🚀 Using provider streaming with umf::StreamingAccumulator");
            
            let stream = self.provider.generate_stream(messages, &config).await?;
            
            use futures_util::StreamExt;
            let mut pinned_stream = Box::pin(stream);
            let mut accumulator = umf::StreamingAccumulator::new();

            while let Some(chunk_result) = pinned_stream.next().await {
                let chunk = chunk_result?;
                if accumulator.process_chunk(chunk) {
                    break;
                }
            }

            let accumulated = accumulator.finish();
            let collected_text = accumulated.text;
            let collected_tool_calls = accumulated.tool_calls;

            if !collected_tool_calls.is_empty() {
                if !collected_text.is_empty() {
                    self.logger.log_llm_response(&collected_text, Some(&self.provider.default_model()))?;
                }
                umf::GenerateResult::ToolCalls(collected_tool_calls)
            } else {
                umf::GenerateResult::Content(collected_text)
            }
        } else {
            let provider_response = self.provider.generate(messages, &config).await?;
            
            match provider_response {
                GenerateResponse::Content(text) => umf::GenerateResult::Content(text),
                GenerateResponse::ToolCalls(invocations) => {
                    let tool_calls = invocations
                        .into_iter()
                        .map(|inv| umf::ToolCall {
                            id: inv.id,
                            r#type: "function".to_string(),
                            function: umf::FunctionCall {
                                name: inv.name,
                                arguments: serde_json::to_string(&inv.arguments).unwrap_or_else(|_| "{}".to_string()),
                            },
                        })
                        .collect();
                    umf::GenerateResult::ToolCalls(tool_calls)
                }
            }
        };

        Ok(response)
    }
    
    // Tool execution
    async fn execute_tool_calls_structured(&mut self, tool_calls: Vec<umf::ToolCall>) 
        -> Result<Vec<crate::orchestration::agent_orchestration::ToolExecutionResult>> {
        // Call the existing method and convert types
        let results = self.execute_tool_calls_structured(tool_calls).await?;
        Ok(results.into_iter().map(|r| crate::orchestration::agent_orchestration::ToolExecutionResult {
            tool_call_id: r.tool_call_id,
            tool_name: r.tool_name,
            content: r.content,
            success: r.success,
        }).collect())
    }
    
    fn generate_assistant_content_for_tools(&self, tool_calls: &[umf::ToolCall]) -> String {
        self.generate_assistant_content_for_tools(tool_calls)
    }
    
    fn get_tool_schemas(&self) -> Vec<serde_json::Value> {
        // Start with CATS tools
        let mut schemas = self.tool_registry.get_all_schemas();
        
        // Add MCP tools if available
        #[cfg(feature = "registry-mcp")]
        if let Some(ref mcp_loader) = self.mcp_tools {
            schemas.extend(mcp_loader.get_openai_schemas());
        }
        
        schemas
    }
    
    // Lifecycle/templates
    async fn load_template(&self, name: &str) -> Result<String> {
        self.lifecycle.load_template(name).await
    }
    
    async fn render_template(&self, template: &str, variables: &[(String, String)]) -> Result<String> {
        self.lifecycle.render_template(template, variables).await
    }
    
    // Logging
    fn log_workflow_iteration(&self, iteration: u32, context: Option<&str>) -> Result<()> {
        self.logger.log_workflow_iteration(iteration, context)
    }
    
    fn log_llm_interaction(&self, _messages: &[serde_json::Value], _response: &str, _model: &str) -> Result<()> {
        // Convert serde_json::Value to HashMap if needed
        // For now, skip logging this specific call or implement conversion
        // The logger expects HashMap format, but we receive serde_json::Value
        // This is a compatibility shim
        Ok(())
    }
    
    fn log_llm_response(&self, response: &str, model: Option<&str>) -> Result<()> {
        self.logger.log_llm_response(response, model)
    }
    
    fn log_error(&self, message: &str, _context: Option<&str>) -> Result<()> {
        self.logger.log_error(message, None)
    }
    
    fn log_completion(&self, reason: &str) -> Result<()> {
        self.logger.log_completion(reason)
    }
    
    fn log_info(&self, message: &str) {
        self.logger.info(message);
    }
    
    // Error formatting  
    async fn format_error(&self, error_type: &str, message: &str, context: &HashMap<String, serde_json::Value>) -> Result<String> {
        // This is a workaround for the &self vs &mut self mismatch
        // We'll render a simple error without template rendering
        Ok(format!("{}: {} (context: {:?})", error_type, message, context))
    }
    
    // Session management
    async fn create_workflow_checkpoint(&mut self, _iteration: u32) -> Result<()> {
        let mut session_manager = self.session_manager
            .take()
            .ok_or_else(|| anyhow::anyhow!("SessionManager not initialized"))?;

        let result = session_manager.create_checkpoint(self).await;
        self.session_manager = Some(session_manager);
        result
    }
    
    fn should_checkpoint(&self) -> bool {
        // Check if checkpointing is enabled
        let enabled = self.session_manager
            .as_ref()
            .map(|sm| sm.is_checkpointing_enabled())
            .unwrap_or(false);
        
        if !enabled {
            return false;
        }
        
        // Get checkpoint interval from config (default to 1 if not set)
        let interval = self.config.get_u64("checkpointing.auto_checkpoint_interval").unwrap_or(1) as u32;
        
        // Checkpoint every N iterations (including iteration 0 for initial state)
        self.current_iteration % interval == 0
    }
    
    async fn finalize_checkpoint_session(&mut self) -> Result<()> {
        if let Some(ref mut session_storage) = self.current_session {
            if let Err(e) = session_storage.synchronize_metadata().await {
                self.logger.log_error(
                    &format!("Warning: Failed to synchronize session metadata: {}", e),
                    None,
                )?;
            }
        }
        Ok(())
    }
    
    // Classification state
    fn classification_done(&self) -> bool { self.classification_done }
    fn set_classification_done(&mut self, done: bool) { self.classification_done = done; }
    fn classified_task_type(&self) -> Option<String> { self.classified_task_type.clone() }
    fn set_classified_task_type(&mut self, task_type: Option<String>) { 
        self.classified_task_type = task_type;
    }
    fn template_sent(&self) -> bool { self.template_sent }
    fn set_template_sent(&mut self, sent: bool) { self.template_sent = sent; }
    fn initial_task_description(&self) -> &str { &self.initial_task_description }
    fn working_dir(&self) -> &std::path::Path { self.executor.working_dir() }
    
    // Conversation turn management
    fn get_current_turn_id(&self) -> Option<&String> {
        self.current_turn_id.as_ref()
    }
    
    fn start_conversation_turn(&mut self) -> String {
        let turn_id = uuid::Uuid::new_v4().to_string();
        self.current_turn_id = Some(turn_id.clone());
        self.turn_request_count = 0;
        turn_id
    }
    
    fn end_conversation_turn(&mut self) {
        self.current_turn_id = None;
        self.turn_request_count = 0;
    }
    
    // LLM helpers
    fn parse_response(&self, response: &str) -> (Option<String>, Option<String>, bool) {
        self.parse_response(response)
    }
    
    fn extract_tool_calls(&self, response: &str) -> Result<Vec<umf::ToolCall>> {
        self.extract_tool_calls(response)
    }
}

// Sophisticated Agent Session Orchestration
// Extracted from simpaticoder - refactored to be reusable for any agent
//
// This module provides advanced orchestration capabilities including:
// - Task classification workflow
// - Dynamic template system integration
// - Dual streaming modes (unified and traditional)
// - X-Request-Id conversation turn management
// - Structured error handling with templates
// - Session management with checkpointing
// - Complex tool execution with detailed results

use anyhow::{Context, Result};
use async_trait::async_trait;
use std::collections::HashMap;
use umf::GenerateResult;

/// Trait for pluggable template providers
/// Implement this to provide your own template system (e.g., lifecycle plugin, file-based, etc.)
#[async_trait]
pub trait TemplateProvider: Send + Sync {
    /// Load a template by name (e.g., "task/bug_fix", "task/feature")
    async fn load_template(&self, name: &str) -> Result<String>;
    
    /// Render a template with variables
    async fn render_template(&self, template: &str, variables: &[(String, String)]) -> Result<String>;
}

/// Trait for task classification handling
/// Implement this to detect and process task classification
#[async_trait]
pub trait ClassificationHandler: Send + Sync {
    /// Called when a classify_task tool is executed
    /// Returns the classified task type (e.g., "bug_fix", "feature")
    async fn handle_classification(&mut self, tool_result: &str) -> Result<Option<String>>;
    
    /// Check if classification is complete
    fn is_classification_done(&self) -> bool;
    
    /// Get the classified task type
    fn get_classified_type(&self) -> Option<String>;
}

/// Trait for session storage and checkpointing
/// Implement this to provide your own storage backend
#[async_trait]
pub trait SessionStorage: Send + Sync {
    /// Start a new session with task description
    async fn start_session(&mut self, task_description: &str, additional_context: Option<&str>) -> Result<String>;
    
    /// Resume session from a checkpoint
    async fn resume_from_checkpoint(&mut self, project_path: &std::path::Path, session_id: &str, checkpoint_id: &str) -> Result<String>;
    
    /// Create a checkpoint at the current iteration
    async fn create_checkpoint(&mut self, iteration: u32, messages: &[serde_json::Value]) -> Result<()>;
    
    /// Check if checkpointing is enabled
    fn is_checkpointing_enabled(&self) -> bool;
    
    /// Synchronize session metadata
    async fn synchronize_metadata(&mut self) -> Result<()>;
}

/// Trait for error formatting
/// Implement this to provide custom error message formatting
#[async_trait]
pub trait ErrorFormatter: Send + Sync {
    /// Format an error with context using templates
    async fn format_error(&self, error_type: &str, message: &str, context: &HashMap<String, serde_json::Value>) -> Result<String>;
}

/// Trait for logging orchestration events
pub trait OrchestrationLogger: Send + Sync {
    fn log_workflow_iteration(&self, iteration: u32, context: Option<&str>) -> Result<()>;
    fn log_llm_interaction(&self, messages: &[serde_json::Value], response: &str, model: &str) -> Result<()>;
    fn log_llm_response(&self, response: &str, model: Option<&str>) -> Result<()>;
    fn log_error(&self, message: &str, context: Option<&str>) -> Result<()>;
    fn log_completion(&self, reason: &str) -> Result<()>;
    fn info(&self, message: &str);
}

/// Tool execution result with success tracking
#[derive(Debug, Clone)]
pub struct ToolExecutionResult {
    pub tool_call_id: String,
    pub tool_name: String,
    pub content: String,
    pub success: bool,
}

/// Configuration for agent session
#[derive(Debug, Clone)]
pub struct SessionConfig {
    pub max_iterations: u32,
    pub max_tokens: u32,
    pub max_history: usize,
    pub max_retries: u32,
    pub enable_task_classification: bool,
    pub request_interval_seconds: Option<u64>,
    pub streaming_enabled: bool,
}

impl Default for SessionConfig {
    fn default() -> Self {
        Self {
            max_iterations: 50,
            max_tokens: 4000,
            max_history: 20,
            max_retries: 3,
            enable_task_classification: true,
            request_interval_seconds: None,
            streaming_enabled: false,
        }
    }
}

/// Execution mode for the agent
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExecutionMode {
    ToolsOnly,
    Hybrid,
    TextOnly,
}

/// Agent session orchestrator with sophisticated workflow management
pub struct AgentSession<P, T, L, C, S, E, F>
where
    P: crate::provider::LlmProvider,
    T: ToolExecutor,
    L: OrchestrationLogger,
    C: ClassificationHandler,
    S: SessionStorage,
    E: ErrorFormatter,
    F: ChatFormatter,
{
    // Core components
    provider: P,
    tool_executor: T,
    logger: L,
    chat_formatter: F,
    
    // Pluggable components
    classification_handler: C,
    session_storage: Option<S>,
    error_formatter: E,
    template_provider: Option<Box<dyn TemplateProvider>>,
    
    // Configuration
    config: SessionConfig,
    execution_mode: ExecutionMode,
    
    // Session state
    is_running: bool,
    current_iteration: u32,
    api_call_count: u32,
    
    // Classification state
    classification_done: bool,
    classified_task_type: Option<String>,
    template_sent: bool,
    initial_task_description: String,
    
    // Conversation turn tracking (X-Request-Id)
    current_turn_id: Option<String>,
    turn_request_count: u32,
}

/// Trait for tool execution
/// Implement this to execute tools in your agent
#[async_trait]
pub trait ToolExecutor: Send + Sync {
    /// Execute a batch of tool calls and return structured results
    async fn execute_tools(&mut self, tool_calls: Vec<umf::ToolCall>) -> Result<Vec<ToolExecutionResult>>;
    
    /// Get all available tool schemas
    fn get_tool_schemas(&self) -> Vec<serde_json::Value>;
    
    /// Generate assistant content based on tool calls (for conversation flow)
    fn generate_assistant_content(&self, tool_calls: &[umf::ToolCall]) -> String;
}

/// Trait for chat message formatting
pub trait ChatFormatter: Send + Sync {
    /// Convert to OpenAI format
    fn to_openai_format(&self) -> Vec<serde_json::Value>;
    
    /// Add assistant message
    fn add_assistant_message(&mut self, content: String, role: Option<String>);
    
    /// Add assistant message with tool calls
    fn add_assistant_message_with_tool_calls(&mut self, content: String, tool_calls: Vec<umf::ToolCall>);
    
    /// Add tool message
    fn add_tool_message(&mut self, content: String, tool_call_id: String, tool_name: String);
    
    /// Add user message
    fn add_user_message(&mut self, content: String, role: Option<String>);
    
    /// Limit conversation history
    fn limit_history(&mut self, max: usize);
    
    /// Validate message structure
    fn validate_messages(&self) -> bool;
    
    /// Count tokens in current context
    fn count_tokens(&self) -> usize;
}

impl<P, T, L, C, S, E, F> AgentSession<P, T, L, C, S, E, F>
where
    P: crate::provider::LlmProvider,
    T: ToolExecutor,
    L: OrchestrationLogger,
    C: ClassificationHandler,
    S: SessionStorage,
    E: ErrorFormatter,
    F: ChatFormatter,
{
    /// Create a new agent session
    pub fn new(
        provider: P,
        tool_executor: T,
        logger: L,
        chat_formatter: F,
        classification_handler: C,
        error_formatter: E,
        config: SessionConfig,
        execution_mode: ExecutionMode,
    ) -> Self {
        Self {
            provider,
            tool_executor,
            logger,
            chat_formatter,
            classification_handler,
            session_storage: None,
            error_formatter,
            template_provider: None,
            config,
            execution_mode,
            is_running: false,
            current_iteration: 1,
            api_call_count: 0,
            classification_done: false,
            classified_task_type: None,
            template_sent: false,
            initial_task_description: String::new(),
            current_turn_id: None,
            turn_request_count: 0,
        }
    }
    
    /// Set session storage (optional)
    pub fn with_session_storage(mut self, storage: S) -> Self {
        self.session_storage = Some(storage);
        self
    }
    
    /// Set template provider (optional)
    pub fn with_template_provider(mut self, provider: Box<dyn TemplateProvider>) -> Self {
        self.template_provider = Some(provider);
        self
    }
    
    /// Start a session
    pub async fn start_session(
        &mut self,
        task_description: &str,
        additional_context: Option<&str>,
    ) -> Result<String> {
        self.initial_task_description = task_description.to_string();
        self.is_running = true;
        
        if let Some(storage) = &mut self.session_storage {
            storage.start_session(task_description, additional_context).await?;
        }
        
        Ok(format!("Session started: {}", task_description))
    }
    
    /// Resume from checkpoint
    pub async fn resume_from_checkpoint(
        &mut self,
        project_path: &std::path::Path,
        session_id: &str,
        checkpoint_id: &str,
    ) -> Result<String> {
        if let Some(storage) = &mut self.session_storage {
            storage.resume_from_checkpoint(project_path, session_id, checkpoint_id).await
        } else {
            Err(anyhow::anyhow!("No session storage configured"))
        }
    }
    
    /// Stop the session
    pub async fn stop_session(&mut self, reason: &str) -> Result<String> {
        self.is_running = false;
        
        // End conversation turn
        if let Some(turn_id) = &self.current_turn_id {
            println!(
                "🔑 Ending conversation turn: {} (Total requests: {})",
                turn_id, self.turn_request_count
            );
        }
        self.end_conversation_turn();
        
        // Finalize checkpoint if active
        if let Some(storage) = &mut self.session_storage {
            if let Err(e) = storage.synchronize_metadata().await {
                self.logger.log_error(
                    &format!("Warning: Failed to synchronize session metadata: {}", e),
                    None,
                )?;
            }
        }
        
        self.logger.log_completion(reason)?;
        Ok(format!("Session completed: {}", reason))
    }
    
    /// Start a conversation turn (X-Request-Id generation)
    pub fn start_conversation_turn(&mut self) -> String {
        let turn_id = uuid::Uuid::new_v4().to_string();
        self.current_turn_id = Some(turn_id.clone());
        self.turn_request_count = 0;
        turn_id
    }
    
    /// End conversation turn
    pub fn end_conversation_turn(&mut self) {
        self.current_turn_id = None;
        self.turn_request_count = 0;
    }
    
    /// Get current turn ID
    pub fn get_current_turn_id(&self) -> Option<&String> {
        self.current_turn_id.as_ref()
    }
    
    /// Run the main workflow loop (non-streaming mode)
    pub async fn run_workflow(&mut self) -> Result<String> {
        if !self.is_running {
            return Ok("Agent session not started. Call start_session() first.".to_string());
        }

        // Ensure conversation turn exists
        if self.get_current_turn_id().is_none() {
            let turn_id = self.start_conversation_turn();
            println!(
                "🔑 Started conversation turn: {} (X-Request-Id for request grouping)",
                turn_id
            );
        }

        let max_iterations = self.config.max_iterations;
        
        for iteration in self.current_iteration..=max_iterations {
            self.current_iteration = iteration;

            // Log iteration with context
            let context_tokens = self.chat_formatter.count_tokens();
            let context_info = Some(format!("Context = {}", context_tokens));
            self.logger.log_workflow_iteration(iteration, context_info.as_deref())?;

            // Create checkpoint if enabled
            if let Some(storage) = &mut self.session_storage {
                if storage.is_checkpointing_enabled() {
                    let messages = self.chat_formatter.to_openai_format();
                    if let Err(e) = storage.create_checkpoint(iteration, &messages).await {
                        self.logger.log_error(
                            &format!("Failed to create checkpoint at iteration {}: {}", iteration, e),
                            None,
                        )?;
                    }
                }
            }

            // Limit history
            self.chat_formatter.limit_history(self.config.max_history);

            // Validate messages
            if !self.chat_formatter.validate_messages() {
                return Err(anyhow::anyhow!("Invalid message structure detected"));
            }

            // Log LLM interaction
            let messages = self.chat_formatter.to_openai_format();
            self.logger.log_llm_interaction(&messages, "", &self.provider.default_model())?;

            // Request interval throttling
            if let Some(interval) = self.config.request_interval_seconds {
                if interval > 0 {
                    tokio::time::sleep(std::time::Duration::from_secs(interval)).await;
                }
            }

            // Generate response with retry
            let response = self.generate_with_retry().await?;

            // Process response
            match response {
                GenerateResult::ToolCalls(tool_calls) => {
                    if !self.handle_tool_calls(tool_calls).await? {
                        return self.stop_session("Task completed via submit tool").await;
                    }
                }
                GenerateResult::Content(response_text) => {
                    if !self.handle_content_response(response_text).await? {
                        return self.stop_session("Task completed successfully").await;
                    }
                }
            }
        }

        self.stop_session(&format!("Maximum iterations ({}) reached", max_iterations)).await
    }

    /// Run workflow in streaming mode
    pub async fn run_workflow_streaming(&mut self) -> Result<String> {
        if !self.is_running {
            return Ok("Agent session not started. Call start_session() first.".to_string());
        }

        // Ensure conversation turn exists
        if self.get_current_turn_id().is_none() {
            let turn_id = self.start_conversation_turn();
            println!(
                "🔑 Started conversation turn: {} (X-Request-Id for request grouping)",
                turn_id
            );
        }

        if !self.config.streaming_enabled {
            return self.run_workflow().await;
        }

        println!("🚀 Starting TRUE unified streaming workflow (One API call)");

        let max_iterations = self.config.max_iterations;
        
        loop {
            // Checkpoint at start of iteration
            if let Some(storage) = &mut self.session_storage {
                if storage.is_checkpointing_enabled() {
                    let messages = self.chat_formatter.to_openai_format();
                    if let Err(e) = storage.create_checkpoint(self.current_iteration, &messages).await {
                        self.logger.log_error(
                            &format!("Failed to create checkpoint: {}", e),
                            None,
                        )?;
                    }
                }
            }

            // Get tools for streaming call
            let tools = self.get_tools_for_call();

            // Log API call
            self.api_call_count += 1;
            let context_tokens = self.chat_formatter.count_tokens();
            println!(
                "🔥 API Call {} | Context={} | TRUE Unified Streaming | Model: {} | Tools: {} | Provider: {}",
                self.api_call_count,
                context_tokens,
                self.provider.default_model(),
                tools.as_ref().map(|t| t.len()).unwrap_or(0),
                self.provider.provider_name()
            );

            // Make streaming call
            match self.generate_with_provider_internal(tools, true).await {
                Ok(result) => {
                    match result {
                        GenerateResult::ToolCalls(tool_calls) => {
                            // Handle tool calls
                            let has_submit = tool_calls.iter().any(|tc| tc.function.name.to_lowercase() == "submit");
                            
                            if !self.handle_tool_calls(tool_calls).await? || has_submit {
                                return self.stop_session("Task completed via streaming workflow").await;
                            }

                            // Send template after classification
                            self.maybe_send_template().await?;

                            if self.current_iteration >= max_iterations {
                                return self.stop_session("Streaming workflow completed after max iterations").await;
                            }

                            self.current_iteration += 1;
                        }
                        GenerateResult::Content(response_text) => {
                            self.logger.log_llm_response(&response_text, Some(&self.provider.default_model()))?;

                            // Check for completion markers
                            if response_text.to_uppercase().contains("TASK_COMPLETED") 
                                || response_text.to_uppercase().contains("COMPLETED") {
                                return self.stop_session("Task completed via streaming response").await;
                            }

                            self.chat_formatter.add_assistant_message(
                                response_text,
                                Some("assistant".to_string()),
                            );

                            if self.current_iteration >= max_iterations {
                                return self.stop_session("Streaming workflow reached max iterations").await;
                            }

                            self.current_iteration += 1;
                        }
                    }
                }
                Err(e) => {
                    // Fallback to non-streaming on streaming errors
                    self.logger.log_error(&format!("Streaming failed: {}", e), None)?;
                    
                    let fallback_tools = self.get_tools_for_call();
                    match self.generate_with_provider_internal(fallback_tools, false).await {
                        Ok(fallback_result) => {
                            match fallback_result {
                                GenerateResult::ToolCalls(tool_calls) => {
                                    self.handle_tool_calls(tool_calls).await?;
                                }
                                GenerateResult::Content(text) => {
                                    self.chat_formatter.add_assistant_message(
                                        text,
                                        Some("assistant".to_string()),
                                    );
                                }
                            }
                            self.current_iteration += 1;
                            continue;
                        }
                        Err(e2) => {
                            self.logger.log_error(&format!("Fallback failed: {}", e2), None)?;
                            return Err(anyhow::anyhow!("Streaming and fallback both failed"));
                        }
                    }
                }
            }
        }
    }

    /// Generate response with retry logic
    async fn generate_with_retry(&mut self) -> Result<GenerateResult> {
        let mut last_error = None;
        
        for attempt in 0..=self.config.max_retries {
            let tools = self.get_tools_for_call();
            
            self.api_call_count += 1;
            println!(
                "🔥 API Call {} | Iteration {} | Model: {} | Tools: {} | Provider: {}",
                self.api_call_count,
                self.current_iteration,
                self.provider.default_model(),
                tools.as_ref().map(|t| t.len()).unwrap_or(0),
                self.provider.provider_name()
            );

            match self.generate_with_provider_internal(tools, self.config.streaming_enabled).await {
                Ok(result) => return Ok(result),
                Err(e) => {
                    last_error = Some(e);
                    if attempt < self.config.max_retries {
                        let wait_time = std::time::Duration::from_secs(2u64.pow(attempt));
                        tokio::time::sleep(wait_time).await;
                    }
                }
            }
        }

        Err(last_error.unwrap_or_else(|| anyhow::anyhow!("Unknown LLM error")))
            .context("Failed to generate response after retries")
    }

    /// Generate using provider with optional streaming
    async fn generate_with_provider_internal(
        &mut self,
        tools: Option<Vec<umf::Tool>>,
        streaming: bool,
    ) -> Result<GenerateResult> {
        use crate::provider::{ToolAdapter, GenerateConfig, GenerateResponse, ToolChoice, InternalMessage};
        use umf::{MessageRole, MessageContent};

        // Convert chat messages to internal format manually
        let messages: Vec<InternalMessage> = self.chat_formatter.to_openai_format()
            .into_iter()
            .filter_map(|msg| {
                let role_str = msg.get("role")?.as_str()?;
                let role = match role_str {
                    "system" => MessageRole::System,
                    "user" => MessageRole::User,
                    "assistant" => MessageRole::Assistant,
                    "tool" => MessageRole::Tool,
                    _ => return None,
                };
                
                // Handle content
                let content = if let Some(content_val) = msg.get("content") {
                    if let Some(text) = content_val.as_str() {
                        MessageContent::Text(text.to_string())
                    } else {
                        MessageContent::Text(String::new())
                    }
                } else {
                    MessageContent::Text(String::new())
                };
                
                // Handle tool_call_id and name for tool messages
                let tool_call_id = msg.get("tool_call_id").and_then(|v| v.as_str()).map(|s| s.to_string());
                let name = msg.get("name").and_then(|v| v.as_str()).map(|s| s.to_string());
                
                Some(InternalMessage {
                    role,
                    content,
                    metadata: std::collections::HashMap::new(),
                    tool_call_id,
                    name,
                })
            })
            .collect();

        let internal_tools = tools.as_ref().map(|t| ToolAdapter::tools_to_internal(t));
        let tool_choice = if internal_tools.is_some() {
            Some(ToolChoice::Auto)
        } else {
            None
        };

        let config = GenerateConfig {
            model: None,
            temperature: 0.7,
            max_tokens: Some(self.config.max_tokens),
            tools: internal_tools,
            tool_choice,
            enable_streaming: streaming,
            x_request_id: self.get_current_turn_id().cloned(),
        };

        let response = if streaming {
            self.generate_streaming(messages, &config).await?
        } else {
            self.provider.generate(messages, &config).await?
        };

        match response {
            GenerateResponse::Content(text) => Ok(GenerateResult::Content(text)),
            GenerateResponse::ToolCalls(invocations) => {
                let tool_calls = ToolAdapter::invocations_to_tool_calls(&invocations)?;
                Ok(GenerateResult::ToolCalls(tool_calls))
            }
        }
    }

    /// Generate with streaming accumulator
    async fn generate_streaming(
        &mut self,
        messages: Vec<crate::provider::InternalMessage>,
        config: &crate::provider::GenerateConfig,
    ) -> Result<crate::provider::GenerateResponse> {
        use futures_util::StreamExt;
        
        let stream = self.provider.generate_stream(messages, config).await?;
        let mut pinned_stream = Box::pin(stream);
        let mut accumulator = umf::StreamingAccumulator::new();

        while let Some(chunk_result) = pinned_stream.next().await {
            let chunk = chunk_result?;
            if accumulator.process_chunk(chunk) {
                break;
            }
        }

        let accumulated = accumulator.finish();
        
        if !accumulated.tool_calls.is_empty() {
            if !accumulated.text.is_empty() {
                self.logger.log_llm_response(&accumulated.text, Some(&self.provider.default_model()))?;
            }
            
            let mut invocations = Vec::new();
            for tool_call in accumulated.tool_calls {
                let arguments = if tool_call.function.arguments.is_empty() {
                    serde_json::json!({})
                } else {
                    serde_json::from_str(&tool_call.function.arguments)
                        .unwrap_or_else(|_| serde_json::json!({}))
                };
                
                invocations.push(crate::provider::ToolInvocation {
                    id: tool_call.id,
                    name: tool_call.function.name,
                    arguments,
                    provider_metadata: std::collections::HashMap::new(),
                });
            }
            Ok(crate::provider::GenerateResponse::ToolCalls(invocations))
        } else {
            Ok(crate::provider::GenerateResponse::Content(accumulated.text))
        }
    }

    /// Get tools for the current call (excludes classify_task if done)
    fn get_tools_for_call(&self) -> Option<Vec<umf::Tool>> {
        if !matches!(self.execution_mode, ExecutionMode::ToolsOnly | ExecutionMode::Hybrid) {
            return None;
        }

        let mut tools: Vec<_> = self.tool_executor.get_tool_schemas()
            .into_iter()
            .map(|def| umf::Tool {
                r#type: "function".to_string(),
                function: umf::Function {
                    name: def["function"]["name"].as_str().unwrap_or("").to_string(),
                    description: def["function"]["description"].as_str().unwrap_or("").to_string(),
                    parameters: def["function"]["parameters"].clone(),
                },
            })
            .collect();

        // Remove classify_task if classification is done
        if self.classification_done && self.config.enable_task_classification {
            tools.retain(|t| t.function.name != "classify_task");
        }

        Some(tools)
    }

    /// Handle tool calls execution
    /// Returns false if session should stop
    async fn handle_tool_calls(&mut self, tool_calls: Vec<umf::ToolCall>) -> Result<bool> {
        println!(
            "🔧 API Call {} → Executing {} tools: [{}]",
            self.api_call_count,
            tool_calls.len(),
            tool_calls.iter().map(|tc| tc.function.name.as_str()).collect::<Vec<_>>().join(", ")
        );

        // Add assistant message with tool calls
        let assistant_content = self.tool_executor.generate_assistant_content(&tool_calls);
        self.chat_formatter.add_assistant_message_with_tool_calls(
            assistant_content,
            tool_calls.clone(),
        );

        // Check for submit tool (completion signal)
        let has_submit = tool_calls.iter().any(|tc| tc.function.name.to_lowercase() == "submit");

        // Execute tools
        let results = self.tool_executor.execute_tools(tool_calls).await?;
        
        println!("✅ API Call {} → Tool execution completed", self.api_call_count);

        // Add tool messages
        for result in &results {
            self.chat_formatter.add_tool_message(
                result.content.clone(),
                result.tool_call_id.clone(),
                result.tool_name.clone(),
            );
        }

        // Log results
        let summary = results.iter()
            .map(|r| format!("Tool: {}\n{}: {}", r.tool_name, 
                if r.success { "Result" } else { "Error" }, r.content))
            .collect::<Vec<_>>()
            .join("\n\n");
        self.logger.info(&format!("Tool execution results: {}", summary));

        Ok(!has_submit)
    }

    /// Handle content response
    /// Returns false if session should stop
    async fn handle_content_response(&mut self, response_text: String) -> Result<bool> {
        self.logger.log_llm_response(&response_text, Some(&self.provider.default_model()))?;

        if !response_text.trim().is_empty() {
            println!("\n{}\n", response_text);
        }

        // Check for completion markers
        if response_text.to_uppercase().contains("TASK_COMPLETED") 
            || response_text.to_uppercase().contains("COMPLETED") {
            return Ok(false);
        }

        // Error: no tools and no completion
        let mut error_context = HashMap::new();
        error_context.insert("response".to_string(), serde_json::Value::String(response_text.clone()));

        let error_msg = self.error_formatter.format_error(
            "INVALID_RESPONSE",
            "No tool calls found in response. Please use tools appropriately.",
            &error_context,
        ).await?;

        self.chat_formatter.add_user_message(error_msg, Some("system".to_string()));
        
        Ok(true)
    }

    /// Send template after classification if not already sent
    async fn maybe_send_template(&mut self) -> Result<()> {
        if !self.classification_done || self.template_sent {
            return Ok(());
        }

        if let Some(task_type) = self.classified_task_type.clone() {
            if let Some(provider) = &self.template_provider {
                let template_name = format!("task/{}", task_type);
                if let Ok(template) = provider.load_template(&template_name).await {
                    let variables = vec![
                        ("task_description".to_string(), self.initial_task_description.clone()),
                        ("task_type".to_string(), task_type),
                    ];
                    
                    if let Ok(content) = provider.render_template(&template, &variables).await {
                        self.chat_formatter.add_user_message(content, None);
                        self.template_sent = true;
                    }
                }
            }
        }

        Ok(())
    }
}

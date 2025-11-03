//! Orchestration functions for agents with integrated state
//!
//! Unlike the trait-based AgentSession, these functions work with agents
//! that have tightly coupled components (like simpaticoder's Agent).
//!
//! This provides the orchestration logic extraction without forcing
//! architectural changes on the consuming agent.

use anyhow::{Context, Result};
use umf::GenerateResult;
use std::collections::HashMap;

/// Tool execution result (re-export from tools module to avoid circular dependency)
#[derive(Debug, Clone)]
pub struct ToolExecutionResult {
    pub tool_call_id: String,
    pub tool_name: String,
    pub content: String,
    pub success: bool,
}

/// Agent context trait - minimal interface needed for orchestration
/// 
/// Instead of 8 separate traits, we have ONE trait that agents implement
pub trait AgentContext {
    // Session state
    fn is_running(&self) -> bool;
    fn set_running(&mut self, running: bool);
    fn current_iteration(&self) -> u32;
    fn set_current_iteration(&mut self, iteration: u32);
    fn api_call_count(&self) -> u32;
    fn increment_api_call_count(&mut self);
    
    // Configuration
    fn max_history(&self) -> usize;
    fn max_tokens(&self) -> u32;
    fn max_retries(&self) -> u32;
    fn request_interval_seconds(&self) -> Option<u64>;
    fn enable_task_classification(&self) -> bool;
    fn streaming_enabled(&self) -> bool;
    
    // Chat management
    fn chat_formatter_mut(&mut self) -> &mut umf::chatml::ChatMLFormatter;
    fn count_tokens(&self) -> usize;
    fn validate_messages(&self) -> bool;
    fn to_openai_messages(&self) -> Vec<serde_json::Value>;
    
    // Provider interaction
    fn provider(&self) -> &dyn crate::provider::LlmProvider;
    fn provider_name(&self) -> String;
    fn default_model(&self) -> String;
    
    // LLM generation - agents implement this to call their provider appropriately
    async fn generate_with_provider(
        &mut self,
        tools: Option<Vec<umf::Tool>>,
        max_tokens: u32,
        streaming: bool,
    ) -> Result<GenerateResult>;
    
    // Tool execution
    async fn execute_tool_calls_structured(&mut self, tool_calls: Vec<umf::ToolCall>) 
        -> Result<Vec<ToolExecutionResult>>;
    fn generate_assistant_content_for_tools(&self, tool_calls: &[umf::ToolCall]) -> String;
    fn get_tool_schemas(&self) -> Vec<serde_json::Value>;
    
    // Lifecycle/templates
    async fn load_template(&self, name: &str) -> Result<String>;
    async fn render_template(&self, template: &str, variables: &[(String, String)]) -> Result<String>;
    
    // Logging
    fn log_workflow_iteration(&self, iteration: u32, context: Option<&str>) -> Result<()>;
    fn log_llm_interaction(&self, messages: &[serde_json::Value], response: &str, model: &str) -> Result<()>;
    fn log_llm_response(&self, response: &str, model: Option<&str>) -> Result<()>;
    fn log_error(&self, message: &str, context: Option<&str>) -> Result<()>;
    fn log_completion(&self, reason: &str) -> Result<()>;
    fn log_info(&self, message: &str);
    
    // Error formatting
    async fn format_error(&self, error_type: &str, message: &str, context: &HashMap<String, serde_json::Value>) -> Result<String>;
    
    // Session management
    async fn create_workflow_checkpoint(&mut self, iteration: u32) -> Result<()>;
    fn should_checkpoint(&self) -> bool;
    async fn finalize_checkpoint_session(&mut self) -> Result<()>;
    
    // Classification state
    fn classification_done(&self) -> bool;
    fn set_classification_done(&mut self, done: bool);
    fn classified_task_type(&self) -> Option<String>;
    fn set_classified_task_type(&mut self, task_type: Option<String>);
    fn template_sent(&self) -> bool;
    fn set_template_sent(&mut self, sent: bool);
    fn initial_task_description(&self) -> &str;
    fn working_dir(&self) -> &std::path::Path;
    
    // Conversation turn management
    fn get_current_turn_id(&self) -> Option<&String>;
    fn start_conversation_turn(&mut self) -> String;
    fn end_conversation_turn(&mut self);
    
    // LLM helpers
    fn parse_response(&self, response: &str) -> (Option<String>, Option<String>, bool);
    fn extract_tool_calls(&self, response: &str) -> Result<Vec<umf::ToolCall>>;
}

/// Run non-streaming workflow - extracted from simpaticoder
pub async fn run_workflow<A: AgentContext>(agent: &mut A, max_iterations: u32) -> Result<String> {
    if !agent.is_running() {
        return Ok("Agent session not started. Call start_session() first.".to_string());
    }

    // Ensure conversation turn exists
    if agent.get_current_turn_id().is_none() {
        let turn_id = agent.start_conversation_turn();
        println!("🔑 Started conversation turn: {}", turn_id);
    }

    for iteration in agent.current_iteration()..=max_iterations {
        agent.set_current_iteration(iteration);

        // Log iteration
        let context_tokens = agent.count_tokens();
        agent.log_workflow_iteration(iteration, Some(&format!("Context = {}", context_tokens)))?;

        // Checkpoint if enabled
        if agent.should_checkpoint() {
            if let Err(e) = agent.create_workflow_checkpoint(iteration).await {
                agent.log_error(&format!("Failed to create checkpoint: {}", e), None)?;
            }
        }

        // Limit history
        let max_history = agent.max_history();
        agent.chat_formatter_mut().limit_history(max_history);

        // Validate messages
        if !agent.validate_messages() {
            return Err(anyhow::anyhow!("Invalid message structure detected"));
        }

        // Log LLM interaction
        let messages = agent.to_openai_messages();
        agent.log_llm_interaction(&messages, "", &agent.default_model())?;

        // Request interval
        if let Some(interval) = agent.request_interval_seconds() {
            if interval > 0 {
                tokio::time::sleep(std::time::Duration::from_secs(interval)).await;
            }
        }

        // Generate with retry
        let response = generate_with_retry(agent).await?;

        // Process response
        match response {
            GenerateResult::ToolCalls(tool_calls) => {
                if !handle_tool_calls(agent, tool_calls).await? {
                    return stop_session(agent, "Task completed via submit tool").await;
                }
            }
            GenerateResult::Content(response_text) => {
                if !handle_content_response(agent, response_text).await? {
                    return stop_session(agent, "Task completed successfully").await;
                }
            }
        }
    }

    stop_session(agent, &format!("Maximum iterations ({}) reached", max_iterations)).await
}

/// Run streaming workflow
pub async fn run_workflow_streaming<A: AgentContext>(agent: &mut A, max_iterations: u32) -> Result<String> {
    if !agent.is_running() {
        return Ok("Agent session not started. Call start_session() first.".to_string());
    }

    if !agent.streaming_enabled() {
        return run_workflow(agent, max_iterations).await;
    }

    println!("🚀 Starting TRUE unified streaming workflow");

    loop {
        // Checkpoint
        if agent.should_checkpoint() {
            if let Err(e) = agent.create_workflow_checkpoint(agent.current_iteration()).await {
                agent.log_error(&format!("Checkpoint failed: {}", e), None)?;
            }
        }

        // Get tools
        let tools = get_tools_for_call(agent);

        // Log API call
        agent.increment_api_call_count();
        println!(
            "🔥 API Call {} | Context={} | Streaming | Model: {} | Tools: {}",
            agent.api_call_count(),
            agent.count_tokens(),
            agent.default_model(),
            tools.as_ref().map(|t| t.len()).unwrap_or(0)
        );

        // Make streaming API call
        let max_tokens = agent.max_tokens();
        match agent.generate_with_provider(tools, max_tokens, true).await {
            Ok(result) => {
                println!("📡 Streaming API call completed successfully");
                
                // Increment iteration after successful API call
                agent.set_current_iteration(agent.current_iteration() + 1);
                
                // Check iteration limit
                if agent.current_iteration() > max_iterations {
                    return stop_session(agent, &format!("Maximum iterations ({}) reached", max_iterations)).await;
                }
                
                match result {
                    GenerateResult::ToolCalls(tool_calls) => {
                        // Check for completion
                        if handle_tool_calls(agent, tool_calls).await? {
                            continue; // Continue loop
                        } else {
                            // Stop requested (submit tool or max iterations)
                            return stop_session(agent, "Task completed via submit tool").await;
                        }
                    }
                    GenerateResult::Content(response_text) => {
                        if !handle_content_response(agent, response_text).await? {
                            return stop_session(agent, "Task completed successfully").await;
                        }
                    }
                }
            }
            Err(e) => {
                let err_msg = e.to_string();
                if err_msg.contains("Task completed") {
                    return stop_session(agent, &err_msg).await;
                } else if err_msg.contains("Maximum iterations") {
                    return stop_session(agent, &format!("Maximum iterations ({}) reached", max_iterations)).await;
                } else if err_msg.contains("API timeout") || err_msg.contains("rate limit") {
                    agent.log_info("Retrying after transient error...");
                    tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                    continue;
                }
                return Err(anyhow::anyhow!(err_msg)).context("Streaming workflow failed");
            }
        }
    }
}

/// Generate with retry logic
async fn generate_with_retry<A: AgentContext>(agent: &mut A) -> Result<GenerateResult> {
    let mut last_error = None;
    
    for attempt in 0..=agent.max_retries() {
        let tools = get_tools_for_call(agent);
        let max_tokens = agent.max_tokens();
        let streaming_enabled = agent.streaming_enabled();
        
        agent.increment_api_call_count();
        println!(
            "🔥 API Call {} | Iteration {} | Model: {} | Tools: {}",
            agent.api_call_count(),
            agent.current_iteration(),
            agent.default_model(),
            tools.as_ref().map(|t| t.len()).unwrap_or(0)
        );

        // Call the agent's generate method
        match agent.generate_with_provider(tools, max_tokens, streaming_enabled).await {
            Ok(result) => return Ok(result),
            Err(e) => {
                last_error = Some(e);
                if attempt < agent.max_retries() {
                    tokio::time::sleep(std::time::Duration::from_secs(2u64.pow(attempt))).await;
                }
            }
        }
    }

    Err(last_error.unwrap_or_else(|| anyhow::anyhow!("Unknown error")))
}

/// Handle tool calls - returns false if should stop
async fn handle_tool_calls<A: AgentContext>(agent: &mut A, tool_calls: Vec<umf::ToolCall>) -> Result<bool> {
    println!(
        "🔧 Executing {} tools: [{}]",
        tool_calls.len(),
        tool_calls.iter().map(|tc| tc.function.name.as_str()).collect::<Vec<_>>().join(", ")
    );

    // Add assistant message
    let content = agent.generate_assistant_content_for_tools(&tool_calls);
    agent.chat_formatter_mut().add_assistant_message_with_tool_calls(content, tool_calls.clone());

    // Check for submit
    let has_submit = tool_calls.iter().any(|tc| tc.function.name.to_lowercase() == "submit");

    // Execute tools
    let results = agent.execute_tool_calls_structured(tool_calls).await?;

    // Add tool messages
    for result in &results {
        agent.chat_formatter_mut().add_tool_message(
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
    agent.log_info(&format!("Tool results: {}", summary));

    Ok(!has_submit)
}

/// Handle content response - returns false if should stop
async fn handle_content_response<A: AgentContext>(agent: &mut A, response_text: String) -> Result<bool> {
    agent.log_llm_response(&response_text, Some(&agent.default_model()))?;

    if !response_text.trim().is_empty() {
        println!("\n{}\n", response_text);
    }

    // Check completion
    if response_text.to_uppercase().contains("TASK_COMPLETED") 
        || response_text.to_uppercase().contains("COMPLETED") {
        return Ok(false);
    }

    // Error: no tools and no completion
    let mut error_context = HashMap::new();
    error_context.insert("response".to_string(), serde_json::Value::String(response_text.clone()));

    let error_msg = agent.format_error(
        "INVALID_RESPONSE",
        "No tool calls found in response.",
        &error_context,
    ).await?;

    agent.chat_formatter_mut().add_user_message(error_msg, Some("system".to_string()));
    
    Ok(true)
}

/// Stop session
async fn stop_session<A: AgentContext>(agent: &mut A, reason: &str) -> Result<String> {
    agent.set_running(false);
    
    if let Some(turn_id) = agent.get_current_turn_id() {
        println!("🔑 Ending conversation turn: {}", turn_id);
    }
    agent.end_conversation_turn();
    
    agent.finalize_checkpoint_session().await?;
    agent.log_completion(reason)?;
    
    Ok(format!("Session completed: {}", reason))
}

/// Get tools for current call (exclude classify_task if done)
fn get_tools_for_call<A: AgentContext>(agent: &A) -> Option<Vec<umf::Tool>> {
    let mut tools: Vec<_> = agent.get_tool_schemas()
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

    if agent.classification_done() && agent.enable_task_classification() {
        tools.retain(|t| t.function.name != "classify_task");
    }

    Some(tools)
}

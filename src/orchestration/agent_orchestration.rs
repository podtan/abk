//! Orchestration functions for agents with integrated state
//!
//! Unlike the trait-based AgentSession, these functions work with agents
//! that have tightly coupled components (like ABK's Agent).
//!
//! This provides the orchestration logic extraction without forcing
//! architectural changes on the consuming agent.

use anyhow::{Context, Result};
use umf::GenerateResult;
use std::collections::HashMap;
use std::sync::Arc;

use super::output::{OutputEvent, OutputSink, SharedSink};

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
    
    /// Tee-print: write raw message to both stdout and log file.
    /// Use for output that should be mirrored exactly to the log file.
    fn log_tee(&self, message: &str);
    
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
    
    // Output sink for structured events (TUI/CLI/log consumers)
    fn output_sink(&self) -> &SharedSink;

    // LLM helpers
    fn parse_response(&self, response: &str) -> (Option<String>, Option<String>, bool);
    fn extract_tool_calls(&self, response: &str) -> Result<Vec<umf::ToolCall>>;
}

/// Run non-streaming workflow - reusable for any agent
pub async fn run_workflow<A: AgentContext>(agent: &mut A, max_iterations: u32) -> Result<String> {
    if !agent.is_running() {
        return Ok("Agent session not started. Call start_session() first.".to_string());
    }

    // Ensure conversation turn exists
    if agent.get_current_turn_id().is_none() {
        let turn_id = agent.start_conversation_turn();
        agent.output_sink().emit(OutputEvent::Info {
            message: format!("🔑 Started conversation turn: {}", turn_id),
        });
        agent.log_info(&format!("🔑 Started conversation turn: {}", turn_id));
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
            GenerateResult::ToolCalls { calls: tool_calls, content, reasoning } => {
                // Execute tools and continue loop
                handle_tool_calls(agent, tool_calls, content, reasoning).await?;
            }
            GenerateResult::Content { text: response_text, reasoning } => {
                // LLM finished naturally - stop the loop.
                // streaming_enabled: false because we're inside the non-streaming
                // run_workflow; LlmResponse event IS needed here.
                handle_content_response(agent, response_text, reasoning, false).await?;
                return stop_session(agent, "Task completed").await;
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

    agent.output_sink().emit(OutputEvent::WorkflowStarted {
        task_description: agent.initial_task_description().to_string(),
    });
    agent.log_info("🚀 Starting TRUE unified streaming workflow");

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
        let tool_count = tools.as_ref().map(|t| t.len()).unwrap_or(0);
        agent.output_sink().emit(OutputEvent::ApiCallStarted {
            call_number: agent.api_call_count(),
            model: agent.default_model(),
            tool_count,
            streaming: true,
        });
        agent.log_info(&format!(
            "🔥 API Call {} | Context={} | Streaming | Model: {} | Tools: {}",
            agent.api_call_count(),
            agent.count_tokens(),
            agent.default_model(),
            tool_count
        ));

        // Make streaming API call
        let max_tokens = agent.max_tokens();
        match agent.generate_with_provider(tools, max_tokens, true).await {
            Ok(result) => {
                agent.output_sink().emit(OutputEvent::Info {
                    message: "📡 Streaming API call completed successfully".to_string(),
                });
                agent.log_info("📡 Streaming API call completed successfully");
                
                // Increment iteration after successful API call
                agent.set_current_iteration(agent.current_iteration() + 1);
                
                // Check iteration limit
                if agent.current_iteration() > max_iterations {
                    return stop_session(agent, &format!("Maximum iterations ({}) reached", max_iterations)).await;
                }
                
                match result {
                    GenerateResult::ToolCalls { calls: tool_calls, content, reasoning } => {
                        // Execute tools and continue loop
                        handle_tool_calls(agent, tool_calls, content, reasoning).await?;
                        continue;
                    }
                    GenerateResult::Content { text: response_text, reasoning } => {
                        // LLM finished naturally - stop the loop.
                        // streaming_enabled: true because we're inside run_workflow_streaming;
                        // pass true so handle_content_response skips the redundant
                        // LlmResponse event (the full text was already streamed
                        // chunk-by-chunk via StreamingChunk events in generate_with_provider).
                        handle_content_response(agent, response_text, reasoning, true).await?;
                        return stop_session(agent, "Task completed").await;
                    }
                }
            }
            Err(e) => {
                let err_msg = format!("{:#}", e);
                if err_msg.contains("Task completed") {
                    return stop_session(agent, &err_msg).await;
                } else if err_msg.contains("Maximum iterations") {
                    return stop_session(agent, &format!("Maximum iterations ({}) reached", max_iterations)).await;
                } else if err_msg.contains("API timeout") || err_msg.contains("rate limit")
                    || err_msg.contains("finish_reason:") || err_msg.contains("network_error")
                    || err_msg.contains("Stream error") {
                    agent.output_sink().emit(OutputEvent::Error {
                        message: format!("Streaming failed (retryable): {}", err_msg),
                        context: None,
                    });
                    agent.log_error(&format!("Streaming failed (retryable): {}", err_msg), None)?;
                    tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                    continue;
                }
                agent.output_sink().emit(OutputEvent::Error {
                    message: format!("Streaming failed: {}", err_msg),
                    context: None,
                });
                agent.log_error(&format!("Streaming failed: {}", err_msg), None)?;
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
        let tool_count = tools.as_ref().map(|t| t.len()).unwrap_or(0);
        agent.output_sink().emit(OutputEvent::ApiCallStarted {
            call_number: agent.api_call_count(),
            model: agent.default_model(),
            tool_count,
            streaming: streaming_enabled,
        });
        agent.log_info(&format!(
            "🔥 API Call {} | Iteration {} | Model: {} | Tools: {}",
            agent.api_call_count(),
            agent.current_iteration(),
            agent.default_model(),
            tool_count
        ));

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

/// Handle tool calls - executes tools and returns Ok(()) 
async fn handle_tool_calls<A: AgentContext>(
    agent: &mut A, 
    tool_calls: Vec<umf::ToolCall>,
    content: Option<String>,
    reasoning: Option<String>,
) -> Result<()> {
    let tool_names: Vec<String> = tool_calls.iter().map(|tc| tc.function.name.clone()).collect();
    agent.output_sink().emit(OutputEvent::ToolsExecuting {
        tool_names: tool_names.clone(),
    });
    agent.log_info(&format!(
        "🔧 Executing {} tools: [{}]",
        tool_names.len(),
        tool_names.join(", ")
    ));

    // Add assistant message - use provided content or generate placeholder
    let message_content = content.unwrap_or_else(|| agent.generate_assistant_content_for_tools(&tool_calls));
    
    if let Some(reasoning_content) = reasoning {
        agent.chat_formatter_mut().add_assistant_message_with_reasoning(
            message_content,
            reasoning_content,
            Some(tool_calls.clone()),
        );
    } else {
        agent.chat_formatter_mut().add_assistant_message_with_tool_calls(message_content, tool_calls.clone());
    }

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

    // Send classification template if classification just completed
    maybe_send_template(agent).await?;

    Ok(())
}

/// Send task-specific template after classification if not already sent
async fn maybe_send_template<A: AgentContext>(agent: &mut A) -> Result<()> {
    // Only send if classification is done and template hasn't been sent yet
    if !agent.classification_done() || agent.template_sent() {
        return Ok(());
    }

    if let Some(task_type) = agent.classified_task_type() {
        let template_name = format!("task/{}", task_type);
        
        // Try to load the task-specific template
        if let Ok(template) = agent.load_template(&template_name).await {
            let variables = vec![
                ("task_description".to_string(), agent.initial_task_description().to_string()),
                ("task_type".to_string(), task_type.clone()),
                ("working_dir".to_string(), agent.working_dir().display().to_string()),
            ];
            
            // Render and add to conversation
            if let Ok(content) = agent.render_template(&template, &variables).await {
                agent.output_sink().emit(OutputEvent::Info {
                    message: format!("📋 Sending task template for: {}", task_type),
                });
                agent.log_info(&format!("📋 Sending task template for: {}", task_type));
                agent.chat_formatter_mut().add_user_message(content, None);
                agent.set_template_sent(true);
            }
        }
    }

    Ok(())
}

/// Handle content response - LLM finished naturally, add message and stop.
/// Returns Ok(()) - the caller should stop the loop.
async fn handle_content_response<A: AgentContext>(agent: &mut A, response_text: String, reasoning: Option<String>, was_streamed: bool) -> Result<()> {
    // Emit the LLM response so sinks can display it — but ONLY when the response
    // was NOT already streamed chunk-by-chunk.  In streaming mode the full text
    // has already been emitted via StreamingChunk events in generate_with_provider(),
    // so emitting LlmResponse here would print it a second time.
    if !was_streamed {
        agent.output_sink().emit(OutputEvent::LlmResponse {
            text: response_text.clone(),
            model: agent.default_model(),
        });
    }

    // Structured log entry (always written to the log file).
    agent.log_llm_response(&response_text, Some(&agent.default_model()))?;

    // Store assistant message in conversation history (for checkpoints and context)
    if let Some(reasoning_content) = reasoning {
        agent.chat_formatter_mut().add_assistant_message_with_reasoning(
            response_text.clone(),
            reasoning_content,
            None,
        );
    } else {
        agent.chat_formatter_mut().add_assistant_message(response_text.clone(), None);
    }

    // LLM finished naturally - no error, just stop
    Ok(())
}

/// Stop session
async fn stop_session<A: AgentContext>(agent: &mut A, reason: &str) -> Result<String> {
    agent.set_running(false);
    
    // Save final checkpoint with all messages including last assistant response
    if agent.should_checkpoint() {
        let final_iteration = agent.current_iteration() + 1;
        if let Err(e) = agent.create_workflow_checkpoint(final_iteration).await {
            agent.log_error(&format!("Failed to create final checkpoint: {}", e), None)?;
        }
    }
    
    if let Some(turn_id) = agent.get_current_turn_id() {
        agent.output_sink().emit(OutputEvent::Info {
            message: format!("🔑 Ending conversation turn: {}", turn_id),
        });
        agent.log_info(&format!("🔑 Ending conversation turn: {}", turn_id));
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

    // Remove classify_task if classification is done (regardless of config)
    if agent.classification_done() {
        tools.retain(|t| t.function.name != "classify_task");
    }

    Some(tools)
}

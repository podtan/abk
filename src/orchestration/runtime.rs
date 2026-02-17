//! Agent runtime core - execution loop and iteration control

use anyhow::{Context, Result};
use std::sync::Arc;
use tokio::sync::RwLock;

#[cfg(feature = "orchestration")]
use async_trait::async_trait;

#[cfg(feature = "orchestration")]
use umf::GenerateResult;

/// Workflow execution status
#[derive(Debug, Clone, PartialEq)]
pub enum WorkflowStatus {
    /// Not yet started
    Pending,
    /// Currently running
    Running,
    /// Paused (waiting for user input or external event)
    Paused,
    /// Completed successfully
    Completed,
    /// Failed with error
    Failed(String),
}

/// Agent execution result
#[derive(Debug, Clone)]
pub struct ExecutionResult {
    /// Whether execution completed successfully
    pub success: bool,
    /// Result message or error description
    pub message: String,
    /// Number of iterations executed
    pub iterations: u32,
}


/// Agent runtime configuration
#[derive(Debug, Clone)]
pub struct RuntimeConfig {
    /// Maximum iterations allowed
    pub max_iterations: u32,
    /// Whether to auto-checkpoint on iterations
    pub auto_checkpoint: bool,
    /// Checkpoint interval (iterations)
    pub checkpoint_interval: u32,
}

impl Default for RuntimeConfig {
    fn default() -> Self {
        Self {
            max_iterations: 50,
            auto_checkpoint: true,
            checkpoint_interval: 5,
        }
    }
}

/// Agent runtime state
#[derive(Debug)]
pub struct RuntimeState {
    /// Current iteration count
    pub iteration: u32,
    /// Current workflow status
    pub status: WorkflowStatus,
    /// Whether the runtime is actively running
    pub is_running: bool,
}

impl Default for RuntimeState {
    fn default() -> Self {
        Self {
            iteration: 0,
            status: WorkflowStatus::Pending,
            is_running: false,
        }
    }
}

/// Trait for LLM provider integration in orchestration
#[cfg(feature = "orchestration")]
#[async_trait]
pub trait OrchestrationProvider: Send + Sync {
    /// Generate response from LLM with tool support
    async fn generate(
        &self,
        messages: Vec<serde_json::Value>,
        tools: Option<Vec<umf::Tool>>,
        max_tokens: u32,
    ) -> Result<GenerateResult>;
    
    /// Get provider name for logging
    fn provider_name(&self) -> &str;
    
    /// Get default model name
    fn model_name(&self) -> &str;
}

/// Trait for tool execution in orchestration
#[cfg(feature = "orchestration")]
#[async_trait]
pub trait OrchestrationTools: Send + Sync {
    /// Execute a tool call and return the result
    async fn execute_tool(
        &self,
        tool_name: &str,
        tool_call_id: &str,
        arguments: serde_json::Value,
    ) -> Result<super::ToolExecutionResult>;
    
    /// Get all available tool schemas
    fn get_schemas(&self) -> Vec<umf::Tool>;
}

/// Trait for message formatting in orchestration
#[cfg(feature = "orchestration")]
pub trait OrchestrationFormatter: Send + Sync {
    /// Get messages in OpenAI format
    fn to_messages(&self) -> Vec<serde_json::Value>;
    
    /// Add assistant message with optional tool calls
    fn add_assistant_message(&mut self, content: String, tool_calls: Option<Vec<umf::ToolCall>>);
    
    /// Add tool result message
    fn add_tool_message(&mut self, content: String, tool_call_id: String, tool_name: String);
    
    /// Add user message
    fn add_user_message(&mut self, content: String);
    
    /// Count tokens in conversation
    fn count_tokens(&self) -> usize;
    
    /// Limit conversation history
    fn limit_history(&mut self, max_messages: usize);
}

/// Callback for checkpointing during orchestration
#[cfg(feature = "orchestration")]
#[async_trait]
pub trait CheckpointCallback: Send + Sync {
    /// Create a checkpoint at current state
    async fn create_checkpoint(&mut self, iteration: u32) -> Result<()>;
}

/// Core agent runtime
///
/// Manages the execution loop, iteration control, and workflow state.
/// This is the heart of the orchestration layer.
pub struct AgentRuntime {
    config: RuntimeConfig,
    state: Arc<RwLock<RuntimeState>>,
}

impl AgentRuntime {
    /// Create a new agent runtime with default configuration
    pub fn new() -> Self {
        Self::with_config(RuntimeConfig::default())
    }

    /// Create a new agent runtime with custom configuration
    pub fn with_config(config: RuntimeConfig) -> Self {
        Self {
            config,
            state: Arc::new(RwLock::new(RuntimeState::default())),
        }
    }

    /// Get current runtime state (read-only)
    pub async fn state(&self) -> RuntimeState {
        let state = self.state.read().await;
        RuntimeState {
            iteration: state.iteration,
            status: state.status.clone(),
            is_running: state.is_running,
        }
    }

    /// Run the orchestration loop
    ///
    /// This is the core method that executes the agent workflow:
    /// 1. Initialize runtime state
    /// 2. Loop up to max_iterations:
    ///    a. Create checkpoint if needed
    ///    b. Prepare messages and tools
    ///    c. Call LLM provider
    ///    d. Handle tool calls or content response
    ///    e. Check for completion
    /// 3. Return final result
    #[cfg(feature = "orchestration")]
    pub async fn run<P, T, F, C>(
        &self,
        provider: &P,
        tools: &T,
        formatter: &mut F,
        mut checkpoint: Option<&mut C>,
        max_history: usize,
    ) -> Result<ExecutionResult>
    where
        P: OrchestrationProvider,
        T: OrchestrationTools,
        F: OrchestrationFormatter,
        C: CheckpointCallback,
    {
        // Start the runtime
        self.start().await?;

        loop {
            // Increment iteration
            let iteration = self.increment_iteration().await;
            
            // Check max iterations
            if self.max_iterations_reached().await {
                self.stop(Some("Maximum iterations reached")).await?;
                return Ok(self.result().await);
            }

            // Create checkpoint if needed
            if self.should_checkpoint().await {
                if let Some(ref mut cp) = checkpoint {
                    if let Err(e) = cp.create_checkpoint(iteration).await {
                        eprintln!("Warning: Failed to create checkpoint at iteration {}: {}", iteration, e);
                    }
                }
            }

            // Limit conversation history
            formatter.limit_history(max_history);

            // Get current messages
            let messages = formatter.to_messages();
            let context_tokens = formatter.count_tokens();
            
            println!("🔥 Iteration {} | Context={} tokens | Model: {} | Provider: {}", 
                     iteration, context_tokens, provider.model_name(), provider.provider_name());

            // Prepare tools
            let available_tools = tools.get_schemas();
            let tools_option = if !available_tools.is_empty() {
                Some(available_tools)
            } else {
                None
            };

            // Generate response with retry logic
            let max_retries = 3;
            let mut response = None;
            let mut last_error = None;

            for attempt in 0..=max_retries {
                match provider.generate(messages.clone(), tools_option.clone(), 4000).await {
                    Ok(result) => {
                        response = Some(result);
                        break;
                    }
                    Err(e) => {
                        last_error = Some(e);
                        if attempt < max_retries {
                            // Exponential backoff
                            let wait_time = std::time::Duration::from_secs(2u64.pow(attempt));
                            tokio::time::sleep(wait_time).await;
                        }
                    }
                }
            }

            let response = match response {
                Some(resp) => resp,
                None => {
                    let error = last_error.unwrap_or_else(|| anyhow::anyhow!("Unknown LLM error"));
                    self.stop(Some(&error.to_string())).await?;
                    return Err(error).context("Failed to generate response from LLM after retries");
                }
            };

            // Handle response
            match response {
                GenerateResult::ToolCalls { calls: tool_calls, content } => {
                    // Log tool execution
                    let tool_names: Vec<&str> = tool_calls.iter().map(|tc| tc.function.name.as_str()).collect();
                    println!("🔧 Iteration {} → Executing {} tools: [{}]", iteration, tool_calls.len(), tool_names.join(", "));

                    // Check for completion via submit tool
                    let has_submit = tool_calls.iter().any(|tc| tc.function.name.to_lowercase() == "submit");

                    // Add assistant message with tool calls - use provided content or generate placeholder
                    let assistant_content = content.unwrap_or_else(|| format!("Executing {} tools", tool_calls.len()));
                    formatter.add_assistant_message(assistant_content, Some(tool_calls.clone()));

                    // Execute tools
                    for tool_call in tool_calls {
                        // Parse arguments from String to serde_json::Value
                        let arguments: serde_json::Value = serde_json::from_str(&tool_call.function.arguments)
                            .unwrap_or(serde_json::json!({}));
                        
                        let result = tools.execute_tool(
                            &tool_call.function.name,
                            &tool_call.id,
                            arguments,
                        ).await?;

                        // Add tool result to conversation
                        formatter.add_tool_message(
                            result.content,
                            result.tool_call_id,
                            result.tool_name,
                        );
                    }

                    println!("✅ Iteration {} → Tool execution completed", iteration);

                    // Check for completion
                    if has_submit {
                        self.stop(None).await?;
                        return Ok(self.result().await);
                    }
                }
                GenerateResult::Content(content) => {
                    // Add assistant message
                    formatter.add_assistant_message(content.clone(), None);

                    // Check for completion marker
                    if content.contains("TASK_COMPLETE") || content.contains("##DONE##") {
                        self.stop(None).await?;
                        return Ok(self.result().await);
                    }

                    // No tools and no completion - this might be an error
                    println!("⚠️ Iteration {} → No tool calls in response", iteration);
                }
            }
        }
    }

    /// Start the runtime
    pub async fn start(&self) -> Result<()> {
        let mut state = self.state.write().await;
        state.is_running = true;
        state.status = WorkflowStatus::Running;
        state.iteration = 0;
        Ok(())
    }

    /// Stop the runtime
    pub async fn stop(&self, reason: Option<&str>) -> Result<()> {
        let mut state = self.state.write().await;
        state.is_running = false;
        state.status = if let Some(err) = reason {
            WorkflowStatus::Failed(err.to_string())
        } else {
            WorkflowStatus::Completed
        };
        Ok(())
    }

    /// Increment iteration counter
    pub async fn increment_iteration(&self) -> u32 {
        let mut state = self.state.write().await;
        state.iteration += 1;
        state.iteration
    }

    /// Check if should checkpoint based on iteration count
    pub async fn should_checkpoint(&self) -> bool {
        if !self.config.auto_checkpoint {
            return false;
        }
        let state = self.state.read().await;
        state.iteration > 0 && state.iteration % self.config.checkpoint_interval == 0
    }

    /// Check if max iterations reached
    pub async fn max_iterations_reached(&self) -> bool {
        let state = self.state.read().await;
        state.iteration >= self.config.max_iterations
    }

    /// Pause the runtime
    pub async fn pause(&self) -> Result<()> {
        let mut state = self.state.write().await;
        state.status = WorkflowStatus::Paused;
        Ok(())
    }

    /// Resume the runtime
    pub async fn resume(&self) -> Result<()> {
        let mut state = self.state.write().await;
        state.status = WorkflowStatus::Running;
        state.is_running = true;
        Ok(())
    }

    /// Get execution result
    pub async fn result(&self) -> ExecutionResult {
        let state = self.state.read().await;
        match &state.status {
            WorkflowStatus::Completed => ExecutionResult {
                success: true,
                message: "Workflow completed successfully".to_string(),
                iterations: state.iteration,
            },
            WorkflowStatus::Failed(err) => ExecutionResult {
                success: false,
                message: err.clone(),
                iterations: state.iteration,
            },
            _ => ExecutionResult {
                success: false,
                message: format!("Workflow in unexpected state: {:?}", state.status),
                iterations: state.iteration,
            },
        }
    }
}

impl Default for AgentRuntime {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_runtime_lifecycle() {
        let runtime = AgentRuntime::new();
        
        // Initial state
        let state = runtime.state().await;
        assert_eq!(state.iteration, 0);
        assert_eq!(state.status, WorkflowStatus::Pending);
        assert!(!state.is_running);

        // Start
        runtime.start().await.unwrap();
        let state = runtime.state().await;
        assert!(state.is_running);
        assert_eq!(state.status, WorkflowStatus::Running);

        // Increment
        let iter = runtime.increment_iteration().await;
        assert_eq!(iter, 1);

        // Stop
        runtime.stop(None).await.unwrap();
        let state = runtime.state().await;
        assert!(!state.is_running);
        assert_eq!(state.status, WorkflowStatus::Completed);
    }

    #[tokio::test]
    async fn test_checkpoint_intervals() {
        let config = RuntimeConfig {
            auto_checkpoint: true,
            checkpoint_interval: 5,
            ..Default::default()
        };
        let runtime = AgentRuntime::with_config(config);

        runtime.start().await.unwrap();
        
        // Not at checkpoint
        for _ in 0..4 {
            runtime.increment_iteration().await;
        }
        assert!(!runtime.should_checkpoint().await);

        // At checkpoint
        runtime.increment_iteration().await; // iteration 5
        assert!(runtime.should_checkpoint().await);
    }
}

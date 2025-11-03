//! Integration test for orchestration runtime
//!
//! Tests the complete orchestration loop with mock provider and tools

use abk::orchestration::{
    AgentRuntime, RuntimeConfig, OrchestrationProvider, OrchestrationTools,
    OrchestrationFormatter, CheckpointCallback, ToolExecutionResult,
};
use anyhow::Result;
use async_trait::async_trait;
use std::sync::{Arc, Mutex};
use umf::{GenerateResult, ToolCall, FunctionCall, Tool, Function};

// Mock provider for testing
struct MockProvider {
    responses: Arc<Mutex<Vec<GenerateResult>>>,
}

impl MockProvider {
    fn new(responses: Vec<GenerateResult>) -> Self {
        Self {
            responses: Arc::new(Mutex::new(responses)),
        }
    }
}

#[async_trait]
impl OrchestrationProvider for MockProvider {
    async fn generate(
        &self,
        _messages: Vec<serde_json::Value>,
        _tools: Option<Vec<Tool>>,
        _max_tokens: u32,
    ) -> Result<GenerateResult> {
        let mut responses = self.responses.lock().unwrap();
        if responses.is_empty() {
            Ok(GenerateResult::Content("TASK_COMPLETE".to_string()))
        } else {
            Ok(responses.remove(0))
        }
    }

    fn provider_name(&self) -> &str {
        "MockProvider"
    }

    fn model_name(&self) -> &str {
        "mock-model"
    }
}

// Mock tools for testing
struct MockTools {
    results: Arc<Mutex<Vec<ToolExecutionResult>>>,
}

impl MockTools {
    fn new() -> Self {
        Self {
            results: Arc::new(Mutex::new(Vec::new())),
        }
    }
}

#[async_trait]
impl OrchestrationTools for MockTools {
    async fn execute_tool(
        &self,
        tool_name: &str,
        tool_call_id: &str,
        _arguments: serde_json::Value,
    ) -> Result<ToolExecutionResult> {
        let result = ToolExecutionResult {
            tool_call_id: tool_call_id.to_string(),
            tool_name: tool_name.to_string(),
            content: format!("Executed {}", tool_name),
            success: true,
        };
        
        self.results.lock().unwrap().push(result.clone());
        Ok(result)
    }

    fn get_schemas(&self) -> Vec<Tool> {
        vec![
            Tool {
                r#type: "function".to_string(),
                function: Function {
                    name: "test_tool".to_string(),
                    description: "A test tool".to_string(),
                    parameters: serde_json::json!({
                        "type": "object",
                        "properties": {}
                    }),
                },
            }
        ]
    }
}

// Mock formatter for testing
struct MockFormatter {
    messages: Arc<Mutex<Vec<String>>>,
}

impl MockFormatter {
    fn new() -> Self {
        Self {
            messages: Arc::new(Mutex::new(vec!["system: You are a helpful assistant".to_string()])),
        }
    }
}

impl OrchestrationFormatter for MockFormatter {
    fn to_messages(&self) -> Vec<serde_json::Value> {
        let messages = self.messages.lock().unwrap();
        messages
            .iter()
            .map(|m| serde_json::json!({"role": "user", "content": m}))
            .collect()
    }

    fn add_assistant_message(&mut self, content: String, _tool_calls: Option<Vec<ToolCall>>) {
        self.messages.lock().unwrap().push(format!("assistant: {}", content));
    }

    fn add_tool_message(&mut self, content: String, _tool_call_id: String, tool_name: String) {
        self.messages.lock().unwrap().push(format!("tool[{}]: {}", tool_name, content));
    }

    fn add_user_message(&mut self, content: String) {
        self.messages.lock().unwrap().push(format!("user: {}", content));
    }

    fn count_tokens(&self) -> usize {
        self.messages.lock().unwrap().len() * 10 // Mock token count
    }

    fn limit_history(&mut self, max_messages: usize) {
        let mut messages = self.messages.lock().unwrap();
        let current_len = messages.len();
        if current_len > max_messages {
            messages.drain(0..current_len - max_messages);
        }
    }
}

// Mock checkpoint callback
struct MockCheckpoint {
    checkpoints: Arc<Mutex<Vec<u32>>>,
}

impl MockCheckpoint {
    fn new() -> Self {
        Self {
            checkpoints: Arc::new(Mutex::new(Vec::new())),
        }
    }
}

#[async_trait]
impl CheckpointCallback for MockCheckpoint {
    async fn create_checkpoint(&mut self, iteration: u32) -> Result<()> {
        self.checkpoints.lock().unwrap().push(iteration);
        Ok(())
    }
}

#[tokio::test]
async fn test_orchestration_with_tool_calls() -> Result<()> {
    // Create mock tool call response
    let tool_call = ToolCall {
        id: "call_1".to_string(),
        r#type: "function".to_string(),
        function: FunctionCall {
            name: "test_tool".to_string(),
            arguments: "{}".to_string(),
        },
    };

    let provider = MockProvider::new(vec![
        GenerateResult::ToolCalls(vec![tool_call.clone()]),
        GenerateResult::Content("TASK_COMPLETE".to_string()),
    ]);

    let tools = MockTools::new();
    let mut formatter = MockFormatter::new();
    let mut checkpoint = MockCheckpoint::new();

    let config = RuntimeConfig {
        max_iterations: 10,
        auto_checkpoint: true,
        checkpoint_interval: 1,
    };

    let runtime = AgentRuntime::with_config(config);

    let result = runtime.run(
        &provider,
        &tools,
        &mut formatter,
        Some(&mut checkpoint),
        20,
    ).await?;

    assert!(result.success);
    assert_eq!(result.iterations, 2);

    // Verify tool was executed
    let tool_results = tools.results.lock().unwrap();
    assert_eq!(tool_results.len(), 1);
    assert_eq!(tool_results[0].tool_name, "test_tool");

    // Verify checkpoints were created
    let checkpoints = checkpoint.checkpoints.lock().unwrap();
    assert!(!checkpoints.is_empty());

    Ok(())
}

#[tokio::test]
async fn test_orchestration_max_iterations() -> Result<()> {
    // Provider that always returns content (never completes)
    let provider = MockProvider::new(vec![
        GenerateResult::Content("Still working...".to_string()),
        GenerateResult::Content("Still working...".to_string()),
        GenerateResult::Content("Still working...".to_string()),
    ]);

    let tools = MockTools::new();
    let mut formatter = MockFormatter::new();

    let config = RuntimeConfig {
        max_iterations: 3,
        auto_checkpoint: false,
        checkpoint_interval: 1,
    };

    let runtime = AgentRuntime::with_config(config);

    let result = runtime.run(
        &provider,
        &tools,
        &mut formatter,
        None::<&mut MockCheckpoint>,
        20,
    ).await?;

    assert!(!result.success, "Expected failure due to max iterations");
    assert_eq!(result.iterations, 3);
    assert!(result.message.contains("Maximum iterations"), "Expected max iterations message, got: {}", result.message);

    Ok(())
}

#[tokio::test]
async fn test_orchestration_submit_completion() -> Result<()> {
    // Submit tool call for completion
    let submit_call = ToolCall {
        id: "call_submit".to_string(),
        r#type: "function".to_string(),
        function: FunctionCall {
            name: "submit".to_string(),
            arguments: "{}".to_string(),
        },
    };

    let provider = MockProvider::new(vec![
        GenerateResult::ToolCalls(vec![submit_call]),
    ]);

    let tools = MockTools::new();
    let mut formatter = MockFormatter::new();

    let config = RuntimeConfig::default();
    let runtime = AgentRuntime::with_config(config);

    let result = runtime.run(
        &provider,
        &tools,
        &mut formatter,
        None::<&mut MockCheckpoint>,
        20,
    ).await?;

    assert!(result.success);
    assert_eq!(result.iterations, 1);

    Ok(())
}

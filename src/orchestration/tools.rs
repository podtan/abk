//! Tool invocation coordination

use anyhow::Result;
use std::collections::HashMap;

/// Tool execution result
#[derive(Debug, Clone)]
pub struct ToolExecutionResult {
    /// Unique ID for this tool call
    pub tool_call_id: String,
    /// Name of the tool that was executed
    pub tool_name: String,
    /// Tool output content
    pub content: String,
    /// Whether tool execution succeeded
    pub success: bool,
}

/// Tool invocation request
#[derive(Debug, Clone)]
pub struct ToolInvocation {
    /// Name of the tool to invoke
    pub tool_name: String,
    /// Tool arguments as JSON
    pub arguments: HashMap<String, serde_json::Value>,
}

/// Tool coordinator
///
/// Manages tool invocations, tracks executions, and handles errors.
pub struct ToolCoordinator {
    /// Count of tool invocations
    invocation_count: u32,
    /// Tool execution history
    execution_history: Vec<ToolExecutionResult>,
}

impl ToolCoordinator {
    /// Create a new tool coordinator
    pub fn new() -> Self {
        Self {
            invocation_count: 0,
            execution_history: Vec::new(),
        }
    }

    /// Record a tool invocation
    pub fn record_invocation(&mut self, result: ToolExecutionResult) {
        self.invocation_count += 1;
        self.execution_history.push(result);
    }

    /// Get total number of tool invocations
    pub fn invocation_count(&self) -> u32 {
        self.invocation_count
    }

    /// Get execution history
    pub fn execution_history(&self) -> &[ToolExecutionResult] {
        &self.execution_history
    }

    /// Get successful executions
    pub fn successful_executions(&self) -> Vec<&ToolExecutionResult> {
        self.execution_history
            .iter()
            .filter(|r| r.success)
            .collect()
    }

    /// Get failed executions
    pub fn failed_executions(&self) -> Vec<&ToolExecutionResult> {
        self.execution_history
            .iter()
            .filter(|r| !r.success)
            .collect()
    }

    /// Reset coordinator state
    pub fn reset(&mut self) {
        self.invocation_count = 0;
        self.execution_history.clear();
    }
}

impl Default for ToolCoordinator {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tool_coordinator() {
        let mut coordinator = ToolCoordinator::new();
        
        assert_eq!(coordinator.invocation_count(), 0);
        assert!(coordinator.execution_history().is_empty());

        // Record successful execution
        coordinator.record_invocation(ToolExecutionResult {
            tool_call_id: "call_1".to_string(),
            tool_name: "test_tool".to_string(),
            content: "Success".to_string(),
            success: true,
        });

        assert_eq!(coordinator.invocation_count(), 1);
        assert_eq!(coordinator.successful_executions().len(), 1);
        assert_eq!(coordinator.failed_executions().len(), 0);

        // Record failed execution
        coordinator.record_invocation(ToolExecutionResult {
            tool_call_id: "call_2".to_string(),
            tool_name: "failing_tool".to_string(),
            content: "Error".to_string(),
            success: false,
        });

        assert_eq!(coordinator.invocation_count(), 2);
        assert_eq!(coordinator.successful_executions().len(), 1);
        assert_eq!(coordinator.failed_executions().len(), 1);
    }
}

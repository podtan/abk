//! Tool execution - delegates to cats crate or MCP servers

use crate::agent::types::ToolExecutionResult;
use umf::ToolCall;
use anyhow::Result;

struct LoggerCallback<'a> {
    logger: &'a crate::observability::Logger,
}

impl<'a> cats::ExecutionCallback for LoggerCallback<'a> {
    fn on_tool_start(&mut self, _: &str, _: &str) {}
    fn on_tool_complete(&mut self, name: &str, args: &str, result: &str, success: bool) {
        let _ = self.logger.log_tool_execution(name, args, result, success);
    }
    fn on_compact_log(&mut self, json: &str) {
        let _ = self.logger.log_compact_tool_call(json);
    }
}

impl super::Agent {
    pub async fn execute_tool_calls(&mut self, tool_calls: Vec<ToolCall>) -> Result<String> {
        let mut results = Vec::new();

        for tc in &tool_calls {
            let result = self.execute_single_tool(tc).await?;
            results.push(result);
        }

        // Format results as combined string
        let mut output = String::new();
        for r in results {
            output.push_str(&format!("[{}] {}\n", r.tool_name, r.content));
        }
        Ok(output)
    }

    pub async fn execute_tool_calls_structured(&mut self, tool_calls: Vec<ToolCall>) -> Result<Vec<ToolExecutionResult>> {
        let mut results = Vec::new();

        for tc in &tool_calls {
            let result = self.execute_single_tool(tc).await?;
            results.push(result);
        }

        Ok(results)
    }

    /// Execute a single tool call, routing to MCP or CATS as appropriate.
    async fn execute_single_tool(&mut self, tc: &ToolCall) -> Result<ToolExecutionResult> {
        // Check if this is an MCP tool
        #[cfg(feature = "registry-mcp")]
        if let Some(ref mcp_tools) = self.mcp_tools {
            if mcp_tools.is_mcp_tool(&tc.function.name) {
                // Execute via MCP
                let mcp_result = mcp_tools
                    .execute_tool(&tc.function.name, &tc.function.arguments)
                    .await;

                let (content, success) = match mcp_result {
                    Ok(r) => (r.content, r.success),
                    Err(e) => (format!("MCP tool error: {}", e), false),
                };

                // Log the execution
                let _ = self.logger.log_tool_execution(
                    &tc.function.name,
                    &tc.function.arguments,
                    &content,
                    success,
                );

                return Ok(ToolExecutionResult {
                    tool_call_id: tc.id.clone(),
                    tool_name: tc.function.name.clone(),
                    content,
                    success,
                });
            }
        }

        // Execute via CATS (local tool)
        self.execute_cats_tool(tc).await
    }

    /// Execute a tool via CATS (local tool registry).
    async fn execute_cats_tool(&mut self, tc: &ToolCall) -> Result<ToolExecutionResult> {
        let mut cb = LoggerCallback { logger: &self.logger };
        let req = cats::ToolCallRequest::new(&tc.id, &tc.function.name, &tc.function.arguments);
        let cfg = cats::ResultHandlerConfig {
            max_size_bytes: self.config.get_u64("tools.max_tool_result_size_bytes").unwrap_or(256000) as usize,
            truncate_enabled: self.config.get_bool("tools.truncate_large_results").unwrap_or(true),
        };

        let cats_results = cats::execute_tool_calls_structured(
            &mut self.tool_registry,
            vec![req],
            &cfg,
            &mut cb,
        )?;

        let cr = cats_results.into_iter().next().ok_or_else(|| {
            anyhow::anyhow!("No result from CATS tool execution")
        })?;

        // Handle classify_task special case
        if cr.tool_name == "classify_task" && !self.classification_done {
            if let Ok(args) = serde_json::from_str::<serde_json::Value>(&tc.function.arguments) {
                if let Some(tt) = args.get("task_type").and_then(|v| v.as_str()) {
                    self.logger.info(&format!("Task classified as: {}", tt));
                    self.classification_done = true;
                    self.classified_task_type = Some(tt.to_string());
                }
            }
        }

        Ok(ToolExecutionResult {
            tool_call_id: cr.tool_call_id,
            tool_name: cr.tool_name,
            content: cr.content,
            success: cr.success,
        })
    }

    pub fn generate_assistant_content_for_tools(&self, tool_calls: &[ToolCall]) -> String {
        let infos: Vec<_> = tool_calls.iter().map(|tc| 
            cats::ToolCallInfo::new(&tc.function.name, &tc.function.arguments)
        ).collect();
        cats::generate_assistant_content(&infos)
    }
}

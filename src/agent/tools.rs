//! Tool execution - delegates to cats crate

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
        let mut cb = LoggerCallback { logger: &self.logger };
        let reqs: Vec<_> = tool_calls.iter().map(|tc| 
            cats::ToolCallRequest::new(&tc.id, &tc.function.name, &tc.function.arguments)
        ).collect();
        let cfg = cats::ResultHandlerConfig {
            max_size_bytes: self.config.get_u64("tools.max_tool_result_size_bytes").unwrap_or(256000) as usize,
            truncate_enabled: self.config.get_bool("tools.truncate_large_results").unwrap_or(true),
        };
        let result = cats::execute_tool_calls(&mut self.tool_registry, reqs, &cfg, &mut cb)?;
        
        // Handle classify_task
        for tc in &tool_calls {
            if tc.function.name == "classify_task" && !self.classification_done {
                if let Ok(args) = serde_json::from_str::<serde_json::Value>(&tc.function.arguments) {
                    if let Some(tt) = args.get("task_type").and_then(|v| v.as_str()) {
                        self.logger.info(&format!("Task classified as: {}", tt));
                        self.classification_done = true;
                        self.classified_task_type = Some(tt.to_string());
                    }
                }
            }
        }
        Ok(result)
    }

    pub async fn execute_tool_calls_structured(&mut self, tool_calls: Vec<ToolCall>) -> Result<Vec<ToolExecutionResult>> {
        let mut cb = LoggerCallback { logger: &self.logger };
        let reqs: Vec<_> = tool_calls.iter().map(|tc|
            cats::ToolCallRequest::new(&tc.id, &tc.function.name, &tc.function.arguments)
        ).collect();
        let cfg = cats::ResultHandlerConfig {
            max_size_bytes: self.config.get_u64("tools.max_tool_result_size_bytes").unwrap_or(256000) as usize,
            truncate_enabled: self.config.get_bool("tools.truncate_large_results").unwrap_or(true),
        };
        let cats_results = cats::execute_tool_calls_structured(&mut self.tool_registry, reqs, &cfg, &mut cb)?;
        
        // Handle classify_task & convert results
        let mut results = Vec::new();
        for (idx, cr) in cats_results.iter().enumerate() {
            if cr.tool_name == "classify_task" && !self.classification_done {
                if let Some(tc) = tool_calls.get(idx) {
                    if let Ok(args) = serde_json::from_str::<serde_json::Value>(&tc.function.arguments) {
                        if let Some(tt) = args.get("task_type").and_then(|v| v.as_str()) {
                            self.logger.info(&format!("Task classified as: {}", tt));
                            self.classification_done = true;
                            self.classified_task_type = Some(tt.to_string());
                        }
                    }
                }
            }
            results.push(ToolExecutionResult {
                tool_call_id: cr.tool_call_id.clone(),
                tool_name: cr.tool_name.clone(),
                content: cr.content.clone(),
                success: cr.success,
            });
        }
        Ok(results)
    }

    pub fn generate_assistant_content_for_tools(&self, tool_calls: &[ToolCall]) -> String {
        let infos: Vec<_> = tool_calls.iter().map(|tc| 
            cats::ToolCallInfo::new(&tc.function.name, &tc.function.arguments)
        ).collect();
        cats::generate_assistant_content(&infos)
    }
}

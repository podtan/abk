//! Logging system for agent interactions.

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde_json;
use std::collections::HashMap;
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

/// Logger for agent interactions and commands.
///
/// This logger creates markdown-formatted log files for tracking agent sessions,
/// LLM interactions, command executions, and other events.
#[derive(Debug)]
pub struct Logger {
    log_file: PathBuf,
    log_level: String,
}

impl Logger {
    /// Initialize logger.
    ///
    /// # Arguments
    /// * `log_file` - Path to log file. If None, creates a timestamped file in temp directory.
    /// * `log_level` - Logging level (defaults to "INFO").
    pub fn new(log_file: Option<&Path>, log_level: Option<&str>) -> Result<Self> {
        let log_file = match log_file {
            Some(p) => p.to_path_buf(),
            None => {
                let mut dir = std::env::temp_dir();
                dir.push("abk-logs");
                std::fs::create_dir_all(&dir).with_context(|| {
                    format!("Failed to create log directory: {}", dir.display())
                })?;
                let filename = format!(
                    "agent_{}_{}.md",
                    Utc::now().timestamp_millis(),
                    std::process::id()
                );
                dir.join(filename)
            }
        };

        let log_level = log_level.unwrap_or("INFO").to_string();

        // Ensure log directory exists
        if let Some(parent) = log_file.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("Failed to create log directory: {}", parent.display()))?;
        }

        let logger = Self {
            log_file,
            log_level,
        };

        // Initialize log file if it doesn't exist
        if !logger.log_file.exists() {
            logger.initialize_log_file()?;
        }

        Ok(logger)
    }

    /// Initialize the log file with header.
    fn initialize_log_file(&self) -> Result<()> {
        let mut file = File::create(&self.log_file)
            .with_context(|| format!("Failed to create log file: {}", self.log_file.display()))?;

        let now: DateTime<Utc> = Utc::now();

        writeln!(file, "# Agent Interaction Log\n")?;
        writeln!(file, "Log started: {}\n", now.to_rfc3339())?;
        writeln!(file, "---\n")?;

        Ok(())
    }

    /// Append content to log file.
    fn append_to_log(&self, content: &str) -> Result<()> {
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.log_file)
            .with_context(|| format!("Failed to open log file: {}", self.log_file.display()))?;

        write!(file, "{}", content).with_context(|| "Failed to write to log file")?;

        Ok(())
    }

    /// Log session start.
    ///
    /// # Arguments
    /// * `mode` - Interaction mode (confirm, yolo, human).
    /// * `config` - Configuration HashMap.
    pub fn log_session_start(
        &self,
        mode: &str,
        config: &HashMap<String, serde_json::Value>,
    ) -> Result<()> {
        let now: DateTime<Utc> = Utc::now();
        let content = format!(
            "## Session Started - {}\n\n**Mode:** {}\n**Config:** {}\n\n",
            now.to_rfc3339(),
            mode,
            serde_json::to_string_pretty(config).unwrap_or_default()
        );

        self.append_to_log(&content)?;
        println!("INFO: Session started in {} mode", mode);
        Ok(())
    }

    /// Log LLM interaction.
    ///
    /// # Arguments
    /// * `messages` - Vector of message HashMaps sent to LLM.
    /// * `response` - LLM response.
    /// * `model` - Model name used.
    pub fn log_llm_interaction(
        &self,
        messages: &[HashMap<String, serde_json::Value>],
        response: &str,
        model: &str,
    ) -> Result<()> {
        // Skip logging only if both response and messages are empty (trimmed)
        let trimmed_response = response.trim();
        let messages_empty = messages.is_empty()
            || messages.iter().all(|m| {
                let role_empty = m
                    .get("role")
                    .and_then(|r| r.as_str())
                    .unwrap_or("")
                    .is_empty();
                let content_empty = m
                    .get("content")
                    .and_then(|c| c.as_str())
                    .unwrap_or("")
                    .trim()
                    .is_empty();
                let name_empty = m
                    .get("name")
                    .and_then(|n| n.as_str())
                    .unwrap_or("")
                    .is_empty();
                role_empty && content_empty && name_empty
            });

        if trimmed_response.is_empty() && messages_empty {
            println!("DEBUG: Skipping log entry for empty LLM response");
            return Ok(());
        }

        let now: DateTime<Utc> = Utc::now();

        // Check if debug logging is enabled
        let is_debug = std::env::var("RUST_LOG")
            .map(|log_level| log_level.to_lowercase().contains("debug"))
            .unwrap_or(false);

        // Only prepare pretty messages JSON in debug mode
        let messages_json = if is_debug && !messages.is_empty() {
            serde_json::to_string_pretty(messages).unwrap_or_else(|_| {
                // Fallback: manually format messages
                let mut formatted = Vec::new();
                for (i, msg) in messages.iter().enumerate() {
                    let role = msg
                        .get("role")
                        .and_then(|r| r.as_str())
                        .unwrap_or("unknown");
                    let content = msg.get("content").and_then(|c| c.as_str()).unwrap_or("");
                    let name = msg.get("name").and_then(|n| n.as_str()).unwrap_or("");

                    let mut msg_str =
                        format!("Message {}: role={}, content={}", i + 1, role, content);
                    if !name.is_empty() {
                        msg_str.push_str(&format!(", name={}", name));
                    }
                    formatted.push(msg_str);
                }
                format!("[\n  {}\n]", formatted.join(",\n  "))
            })
        } else {
            String::new()
        };

        let content = if is_debug {
            // Debug: include pretty messages and response
            format!(
                "### LLM Interaction - {}\n\n**Model:** {}\n\n**Messages:**\n```json\n{}\n```\n\n**Response:**\n```\n{}\n```\n\n",
                now.to_rfc3339(),
                model,
                if messages_json.is_empty() { "[]" } else { &messages_json },
                response
            )
        } else {
            // Non-debug: include minimal summary only if we have messages, then response
            let messages_block = if !messages.is_empty() {
                format!("**Messages:** {} messages\n\n", messages.len())
            } else {
                String::new()
            };

            format!(
                "### LLM Interaction - {}\n\n**Model:** {}\n\n{}**Response:**\n```\n{}\n```\n\n",
                now.to_rfc3339(),
                model,
                messages_block,
                response
            )
        };

        self.append_to_log(&content)?;
        println!("INFO: LLM interaction logged");
        Ok(())
    }

    /// Log LLM response only.
    ///
    /// # Arguments
    /// * `response` - LLM response.
    /// * `model` - Model name used.
    pub fn log_llm_response(&self, response: &str, model: Option<&str>) -> Result<()> {
        // Skip logging if response is empty or contains only whitespace
        let trimmed_response = response.trim();
        if trimmed_response.is_empty() {
            println!("DEBUG: Skipping log entry for empty LLM response");
            return Ok(());
        }

        let model = model.unwrap_or("unknown");
        let now: DateTime<Utc> = Utc::now();
        let content = format!(
            "### LLM Response - {}\n\n**Model:** {}\n\n**Response:**\n```\n{}\n```\n\n",
            now.to_rfc3339(),
            model,
            response
        );

        self.append_to_log(&content)?;
        println!("INFO: LLM response logged");
        Ok(())
    }

    /// Log command execution.
    ///
    /// # Arguments
    /// * `command` - Command that was executed.
    /// * `stdout` - Command stdout.
    /// * `stderr` - Command stderr.
    /// * `return_code` - Command return code.
    /// * `mode` - Execution mode.
    pub fn log_command_execution(
        &self,
        command: &str,
        stdout: &str,
        stderr: &str,
        return_code: i32,
        mode: &str,
    ) -> Result<()> {
        let now: DateTime<Utc> = Utc::now();
        let mut content = format!(
            "### Command Execution - {}\n\n**Mode:** {}\n**Command:** `{}`\n**Return Code:** {}\n\n",
            now.to_rfc3339(),
            mode,
            command,
            return_code
        );

        if !stdout.is_empty() {
            content.push_str(&format!("**Stdout:**\n```\n{}\n```\n\n", stdout));
        }

        if !stderr.is_empty() {
            content.push_str(&format!("**Stderr:**\n```\n{}\n```\n\n", stderr));
        }

        self.append_to_log(&content)?;
        println!(
            "INFO: Command executed: {} (exit: {})",
            command, return_code
        );
        Ok(())
    }

    /// Log mode change.
    ///
    /// # Arguments
    /// * `old_mode` - Previous mode.
    /// * `new_mode` - New mode.
    pub fn log_mode_change(&self, old_mode: &str, new_mode: &str) -> Result<()> {
        let now: DateTime<Utc> = Utc::now();
        let content = format!(
            "### Mode Change - {}\n\n**From:** {}\n**To:** {}\n\n",
            now.to_rfc3339(),
            old_mode,
            new_mode
        );

        self.append_to_log(&content)?;
        println!("INFO: Mode changed from {} to {}", old_mode, new_mode);
        Ok(())
    }

    /// Log error with context.
    ///
    /// # Arguments
    /// * `error` - Error message.
    /// * `context` - Additional context information.
    pub fn log_error(
        &self,
        error: &str,
        context: Option<&HashMap<String, serde_json::Value>>,
    ) -> Result<()> {
        let now: DateTime<Utc> = Utc::now();
        let mut content = format!(
            "### Error - {}\n\n**Error:** {}\n\n",
            now.to_rfc3339(),
            error
        );

        if let Some(ctx) = context {
            content.push_str(&format!(
                "**Context:** {}\n\n",
                serde_json::to_string_pretty(ctx).unwrap_or_default()
            ));
        }

        self.append_to_log(&content)?;
        eprintln!("ERROR: {}", error);
        Ok(())
    }

    /// Log session completion.
    ///
    /// # Arguments
    /// * `reason` - Reason for completion.
    pub fn log_completion(&self, reason: &str) -> Result<()> {
        let now: DateTime<Utc> = Utc::now();
        let content = format!(
            "### Session Completed - {}\n\n**Reason:** {}\n\n---\n\n",
            now.to_rfc3339(),
            reason
        );

        self.append_to_log(&content)?;
        println!("INFO: Session completed: {}", reason);
        Ok(())
    }

    /// Log custom content.
    ///
    /// # Arguments
    /// * `title` - Log entry title.
    /// * `content` - Log content.
    /// * `level` - Log level.
    pub fn log_custom(&self, title: &str, content: &str, level: Option<&str>) -> Result<()> {
        let level = level.unwrap_or("INFO");
        let now: DateTime<Utc> = Utc::now();
        let log_content = format!("### {} - {}\n\n{}\n\n", title, now.to_rfc3339(), content);

        self.append_to_log(&log_content)?;

        match level {
            "ERROR" => eprintln!("ERROR: {}: {}", title, content),
            "WARN" => println!("WARN: {}: {}", title, content),
            _ => println!("INFO: {}: {}", title, content),
        }

        Ok(())
    }

    /// Log tool execution with detailed results.
    ///
    /// # Arguments
    /// * `tool_name` - Name of the tool executed.
    /// * `tool_args` - Arguments passed to the tool.
    /// * `result` - Tool execution result.
    /// * `success` - Whether the tool execution was successful.
    pub fn log_tool_execution(
        &self,
        tool_name: &str,
        tool_args: &str,
        result: &str,
        success: bool,
    ) -> Result<()> {
        let now: DateTime<Utc> = Utc::now();
        let status = if success { "Result" } else { "Error" };

        // Log to file with timestamp only (remove console printing to avoid duplicates)
        let content = format!(
            "### Tool Execution - {}\n\n**Tool:** {}\n**Args:** {}\n**{}:** {}\n\n",
            now.to_rfc3339(),
            tool_name,
            tool_args,
            status,
            result
        );

        self.append_to_log(&content)?;
        Ok(())
    }

    /// Log compact tool call request at INFO level.
    ///
    /// # Arguments
    /// * `tool_call_json` - Compact JSON string of the tool call wrapper.
    pub fn log_compact_tool_call(&self, tool_call_json: &str) -> Result<()> {
        // Print to stdout at INFO level (always visible)
        println!("INFO: Tool request: {}", tool_call_json);

        // Append to markdown log file for persistence
        let now: DateTime<Utc> = Utc::now();
        let content = format!(
            "### Tool Request - {}\n\n**Tool Request (compact):** {}\n\n",
            now.to_rfc3339(),
            tool_call_json
        );
        self.append_to_log(&content)?;
        Ok(())
    }

    /// Log workflow iteration start.
    ///
    /// # Arguments
    /// * `iteration` - Iteration number.
    /// * `context_info` - Optional context information (e.g., token count).
    pub fn log_workflow_iteration(&self, iteration: u32, context_info: Option<&str>) -> Result<()> {
        let info = match context_info {
            Some(ctx) => format!("Starting workflow iteration {} : {}", iteration, ctx),
            None => format!("Starting workflow iteration {}", iteration),
        };

        println!("INFO: {}", info);

        let now: DateTime<Utc> = Utc::now();
        let content = format!(
            "## Starting workflow iteration {} : {}\n\n**Timestamp:** {}\n**Context:** {}\n\n",
            iteration,
            context_info.unwrap_or("Workflow iteration"),
            now.to_rfc3339(),
            context_info.unwrap_or("No additional context")
        );

        self.append_to_log(&content)?;
        Ok(())
    }

    /// Log info message.
    pub fn info(&self, message: &str) {
        println!("INFO: {}", message);
    }

    /// Log error message.
    pub fn error(&self, message: &str) {
        eprintln!("ERROR: {}", message);
    }

    /// Get the log file path.
    pub fn log_file(&self) -> &Path {
        &self.log_file
    }

    /// Get the log level.
    pub fn log_level(&self) -> &str {
        &self.log_level
    }
}

impl Default for Logger {
    fn default() -> Self {
        Self::new(None, None).expect("Failed to create default logger")
    }
}

#[cfg(test)]
mod tests;

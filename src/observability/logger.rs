//! Logging system for agent interactions.

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde_json;
use std::collections::HashMap;
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

/// Session start timestamp - set once when the logger module is first loaded
static SESSION_START_TIMESTAMP: OnceLock<String> = OnceLock::new();

/// Global logger instance - ensures all logs go to the same file
static GLOBAL_LOGGER: OnceLock<Logger> = OnceLock::new();

/// Get or initialize the session timestamp
fn get_session_timestamp() -> &'static str {
    SESSION_START_TIMESTAMP.get_or_init(|| {
        Utc::now().format("%Y%m%d_%H%M%S").to_string()
    })
}

/// Get or initialize the global logger instance
fn get_global_logger() -> &'static Logger {
    GLOBAL_LOGGER.get_or_init(|| {
        Logger::new(None, None).expect("Failed to create global logger")
    })
}

/// Initialize the global logger with a specific Logger instance.
/// This should be called early (e.g., from agent initialization) so that
/// standalone tee_* functions write to the same log file as the agent's logger.
/// If the global logger has already been initialized, this is a no-op.
pub fn init_global_logger(logger: Logger) {
    let _ = GLOBAL_LOGGER.set(logger);
}

/// Get the current log file path from the global logger.
pub fn current_log_path() -> &'static Path {
    get_global_logger().log_file()
}

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
    /// * `log_dir` - Directory for log files. If None, creates timestamped files in /tmp/{ABK_AGENT_NAME}/.
    /// * `log_level` - Logging level (defaults to "INFO").
    pub fn new(log_dir: Option<&Path>, log_level: Option<&str>) -> Result<Self> {
        let agent_name = std::env::var("ABK_AGENT_NAME")
            .unwrap_or_else(|_| "agent".to_string());
        let timestamp = get_session_timestamp();
        let filename = format!("{}_{}.log", agent_name, timestamp);

        let log_dir = match log_dir {
            Some(d) if !d.as_os_str().is_empty() => d.to_path_buf(),
            _ => std::env::temp_dir().join(&agent_name),
        };

        std::fs::create_dir_all(&log_dir)
            .with_context(|| format!("Failed to create log directory: {}", log_dir.display()))?;

        let log_file = log_dir.join(filename);

        let log_level = log_level.unwrap_or("INFO").to_string();

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

        writeln!(file, "Agent Interaction Log")?;
        writeln!(file, "Log started: {}", now.to_rfc3339())?;
        writeln!(file, "---")?;

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

        // Append to log file for persistence
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

    /// Log info message (console + file).
    pub fn info(&self, message: &str) {
        println!("INFO: {}", message);
        let _ = self.append_to_log(&format!("INFO: {}\n", message));
    }

    /// Log error message (console + file).
    pub fn error(&self, message: &str) {
        eprintln!("ERROR: {}", message);
        let _ = self.append_to_log(&format!("ERROR: {}\n", message));
    }

    /// Tee-print: write to both stdout and log file.
    /// Use this for raw output that should be mirrored exactly.
    pub fn tee_println(&self, message: &str) {
        println!("{}", message);
        let _ = self.append_to_log(&format!("{}\n", message));
    }

    /// Tee-eprint: write to both stderr and log file.
    pub fn tee_eprintln(&self, message: &str) {
        eprintln!("{}", message);
        let _ = self.append_to_log(&format!("{}\n", message));
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


fn append_to_global_log(content: &str) {
    let logger = get_global_logger();
    let _ = logger.append_to_log(content);
}

/// Tee-print to stdout and the log file using the global logger.
/// Use this from components that don't have a Logger reference.
pub fn tee_print(message: &str) {
    print!("{}", message);
    let _ = std::io::stdout().flush();
    append_to_global_log(message);
}

/// Tee-eprint to stderr and the log file using the global logger.
/// Use this from components that don't have a Logger reference.
pub fn tee_eprint(message: &str) {
    eprint!("{}", message);
    let _ = std::io::stderr().flush();
    append_to_global_log(message);
}

/// Tee-eprintln to stderr and the log file using the global logger.
pub fn tee_eprintln(message: &str) {
    eprintln!("{}", message);
    append_to_global_log(&format!("{}\n", message));
}

#[cfg(test)]
mod tests;

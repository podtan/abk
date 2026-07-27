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

// ---------------------------------------------------------------------------
// Task-local state — replaces process-global TUI_MODE and GLOBAL_LOGGER.
//
// In multi-user deployments (e.g., trustee-web), multiple concurrent
// workflows run in separate tokio tasks. Task-local storage ensures
// each workflow has its own TUI mode flag and logger, preventing
// cross-user contamination.
// ---------------------------------------------------------------------------

tokio::task_local! {
    /// TUI mode flag for the current task.
    ///
    /// When true, console output (stdout/stderr) is suppressed in tee_*
    /// functions and Logger methods. Log file writes are unaffected.
    ///
    /// Task-local so concurrent users don't share the same flag.
    pub static TUI_MODE: bool;

    /// Logger for the current task.
    ///
    /// Replaces the old process-global `GLOBAL_LOGGER`. Each concurrent
    /// workflow gets its own logger scope via [`with_logger`].
    pub static TASK_LOGGER: Logger;
}

/// Process-global fallback logger for single-user CLI mode.
///
/// Only used when no task-local logger is set (i.e., when not running
/// inside a [`with_logger`] scope). This preserves backward compatibility
/// for CLI/TUI single-user operation.
static FALLBACK_LOGGER: OnceLock<Logger> = OnceLock::new();

/// Check whether TUI mode is active for the current task.
///
/// Returns `false` when no task-local scope is set (e.g., outside an
/// async runtime, or in single-user CLI mode without a scope).
pub fn is_tui_mode() -> bool {
    match TUI_MODE.try_get() {
        Ok(v) => v,
        Err(_) => false,
    }
}

/// **Deprecated.** Use [`with_tui_mode`] instead.
///
/// In task-local mode, this is a no-op — it cannot set the task-local
/// flag from synchronous code. The flag is set by running the workflow
/// inside a `with_tui_mode(enabled, future)` scope.
///
/// This function is kept for backward API compatibility. In debug builds
/// it logs a warning.
pub fn set_tui_mode(enabled: bool) {
    // No-op: TUI mode is now task-local. Callers should use with_tui_mode().
    if cfg!(debug_assertions) && enabled {
        eprintln!("WARN: set_tui_mode(true) is a no-op in task-local mode; use with_tui_mode()");
    }
}

/// Run a future with TUI mode enabled or disabled for the current task.
///
/// This is the task-local replacement for `set_tui_mode()`. All code
/// running inside `fut` (and any child tasks spawned within it) will
/// see the given TUI mode value from [`is_tui_mode`].
///
/// # Example
///
/// ```no_run
/// # async fn run() {
/// abk::observability::with_tui_mode(true, async {
///     // TUI mode is active here
///     assert!(abk::observability::is_tui_mode());
/// }).await;
/// // TUI mode is no longer active
/// assert!(!abk::observability::is_tui_mode());
/// # }
/// ```
pub async fn with_tui_mode<F>(enabled: bool, fut: F) -> F::Output
where
    F: std::future::Future,
{
    TUI_MODE.scope(enabled, fut).await
}

/// Run a future with a specific logger for the current task.
///
/// This replaces `init_global_logger()` for multi-user operation.
/// Each concurrent workflow should call this to scope its logger.
///
/// When no scope is set, free functions like `tee_println()` fall back
/// to the process-global [`FALLBACK_LOGGER`] (set by [`init_global_logger`]).
pub async fn with_logger<F>(logger: Logger, fut: F) -> F::Output
where
    F: std::future::Future,
{
    TASK_LOGGER.scope(logger, fut).await
}

/// Get or initialize the session timestamp
fn get_session_timestamp() -> &'static str {
    SESSION_START_TIMESTAMP.get_or_init(|| {
        Utc::now().format("%Y%m%d_%H%M%S").to_string()
    })
}

/// Resolve the logger for the current context.
///
/// Tries task-local logger first (multi-user mode), then falls back to
/// the process-global [`FALLBACK_LOGGER`] (single-user CLI mode).
///
/// Returns `None` if neither is set.
fn resolve_logger() -> Option<Logger> {
    // Try task-local logger first
    if let Ok(logger_ref) = TASK_LOGGER.try_get() {
        return Some(logger_ref.clone());
    }
    // Fall back to process-global logger (single-user CLI mode)
    FALLBACK_LOGGER.get().cloned()
}

/// **Deprecated.** Use [`with_logger`] instead.
///
/// Stores the logger in a process-global fallback. This is only used
/// when no task-local logger scope is active. In multi-user deployments,
/// each workflow should use `with_logger()` to scope its logger.
///
/// If the fallback has already been set, this is a no-op.
pub fn init_global_logger(logger: Logger) {
    let _ = FALLBACK_LOGGER.set(logger);
}

/// Get the logger for the current task, if any.
///
/// Tries task-local first, then the process-global fallback.
/// Unlike the old `get_global_logger_opt()`, this returns an owned
/// `Option<Logger>` rather than a `&'static Logger` reference.
pub fn get_global_logger_opt() -> Option<Logger> {
    resolve_logger()
}

/// Get the current log file path.
///
/// Tries task-local logger first, then falls back to the process-global
/// fallback logger. If neither is set, returns `None`.
pub fn current_log_path() -> Option<std::path::PathBuf> {
    resolve_logger().map(|l| l.log_file.to_path_buf())
}

/// Logger for agent interactions and commands.
///
/// This logger creates markdown-formatted log files for tracking agent sessions,
/// LLM interactions, command executions, and other events.
#[derive(Debug, Clone)]
pub struct Logger {
    log_file: PathBuf,
    log_level: String,
}

impl Logger {
    /// Initialize logger.
    ///
    /// # Arguments
    /// * `log_dir` - Directory for log files. If None, creates timestamped files in /tmp/{agent_name}/.
    /// * `log_level` - Logging level (defaults to "INFO").
    /// * `agent_name` - Optional agent name override. When `None`, falls back to
    ///   the `ABK_AGENT_NAME` env var, then `"agent"`.
    pub fn new(log_dir: Option<&Path>, log_level: Option<&str>) -> Result<Self> {
        Self::with_agent_name(log_dir, log_level, None)
    }

    /// Initialize logger with an explicit agent name.
    ///
    /// When `agent_name` is `Some`, it overrides the `ABK_AGENT_NAME` env var
    /// for log file naming and directory.
    pub fn with_agent_name(
        log_dir: Option<&Path>,
        log_level: Option<&str>,
        agent_name: Option<&str>,
    ) -> Result<Self> {
        let agent_name = agent_name
            .map(|s| s.to_string())
            .unwrap_or_else(|| {
                std::env::var("ABK_AGENT_NAME")
                    .unwrap_or_else(|_| "agent".to_string())
            });
        let timestamp = get_session_timestamp();
        let filename = format!("{}_{}.log", agent_name, timestamp);

        let log_dir = match log_dir {
            Some(d) if !d.as_os_str().is_empty() => d.to_path_buf(),
            _ => std::env::temp_dir().join(&agent_name),
        };

        std::fs::create_dir_all(&log_dir)
            .with_context(|| format!("Failed to create log directory: {}", log_dir.display()))?;

        // On multi-user systems, the /tmp/{agent_name}/ directory may be created
        // by one user, preventing others from writing. Set world-writable + sticky
        // bit (0o1777, same as /tmp itself) so all users can create their own log
        // files, but only the file owner can delete/rename them.
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&log_dir, std::fs::Permissions::from_mode(0o1777));
        }

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
        file.flush()?;

        Ok(())
    }

    /// Append content to log file.
    pub fn append_to_log(&self, content: &str) -> Result<()> {
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.log_file)
            .with_context(|| format!("Failed to open log file: {}", self.log_file.display()))?;

        write!(file, "{}", content).with_context(|| "Failed to write to log file")?;
        file.flush()?;

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
        // No println — session start is announced via OutputEvent::WorkflowStarted
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
        // No println here — the response is already displayed via OutputSink
        // (StreamingChunk events during streaming, or LlmResponse event otherwise).
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
        Ok(())
    }

    /// Log custom content.
    ///
    /// # Arguments
    /// * `title` - Log entry title.
    /// * `content` - Log content.
    /// * `level` - Log level.
    pub fn log_custom(&self, title: &str, content: &str, _level: Option<&str>) -> Result<()> {
        let now: DateTime<Utc> = Utc::now();
        let log_content = format!("### {} - {}\n\n{}\n\n", title, now.to_rfc3339(), content);

        self.append_to_log(&log_content)?;
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
        let _info = match context_info {
            Some(ctx) => format!("Starting workflow iteration {} : {}", iteration, ctx),
            None => format!("Starting workflow iteration {}", iteration),
        };

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
        let _ = self.append_to_log(&format!("INFO: {}\n", message));
    }

    /// Log error message (console + file).
    pub fn error(&self, message: &str) {
        let _ = self.append_to_log(&format!("ERROR: {}\n", message));
    }

    /// Tee-print: write to both stdout and log file.
    /// Use this for raw output that should be mirrored exactly.
    pub fn tee_println(&self, message: &str) {
        if !is_tui_mode() {
            println!("{}", message);
            let _ = std::io::stdout().flush();
        }
        let _ = self.append_to_log(&format!("{}\n", message));
    }

    /// Tee-eprint: write to both stderr and log file.
    pub fn tee_eprintln(&self, message: &str) {
        if !is_tui_mode() { eprintln!("{}", message); }
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


/// Append content to the current task's logger (or fallback logger).
///
/// In multi-user mode, uses the task-local logger set by [`with_logger`].
/// In single-user CLI mode, falls back to the process-global logger set
/// by [`init_global_logger`]. If neither is set, the content is silently
/// dropped.
pub fn append_to_global_log(content: &str) {
    if let Some(logger) = resolve_logger() {
        let _ = logger.append_to_log(content);
    }
}

/// Strip ANSI escape codes from a string.
/// Removes sequences like \x1b[...m (SGR), \x1b[...H (cursor), etc.
pub fn strip_ansi(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\x1b' {
            // Skip the escape sequence
            if chars.peek() == Some(&'[') {
                chars.next(); // consume '['
                // Consume until we hit a letter (the terminator)
                while let Some(&ch) = chars.peek() {
                    chars.next();
                    if ch.is_ascii_alphabetic() {
                        break;
                    }
                }
            }
        } else {
            result.push(c);
        }
    }
    result
}

/// Tee-println to stdout and the log file using the global logger.
/// Use this from components that don't have a Logger reference.
/// In TUI mode, console output is suppressed.
pub fn tee_println(message: &str) {
    if !is_tui_mode() {
        println!("{}", message);
        let _ = std::io::stdout().flush();
    }
    append_to_global_log(&format!("{}\n", strip_ansi(message)));
}

/// Tee-print to stdout and the log file using the global logger.
/// Use this from components that don't have a Logger reference.
/// ANSI escape codes are written to stdout but stripped from the log file.
/// In TUI mode, console output is suppressed.
pub fn tee_print(message: &str) {
    if !is_tui_mode() {
        print!("{}", message);
        let _ = std::io::stdout().flush();
    }
    append_to_global_log(&strip_ansi(message));
}

/// Tee-eprint to stderr and the log file using the global logger.
/// Use this from components that don't have a Logger reference.
/// ANSI escape codes are written to stderr but stripped from the log file.
/// In TUI mode, console output is suppressed.
pub fn tee_eprint(message: &str) {
    if !is_tui_mode() {
        eprint!("{}", message);
        let _ = std::io::stderr().flush();
    }
    append_to_global_log(&strip_ansi(message));
}

/// Tee-eprintln to stderr and the log file using the global logger.
/// ANSI escape codes are written to stderr but stripped from the log file.
/// In TUI mode, console output is suppressed.
pub fn tee_eprintln(message: &str) {
    if !is_tui_mode() {
        eprintln!("{}", message);
    }
    let clean = strip_ansi(message);
    append_to_global_log(&format!("{}\n", clean));
}

#[cfg(test)]
mod tests;

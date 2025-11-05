//! Command execution module with timeout handling for simpaticoder.

use anyhow::Result;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;
use tokio::process::Command as TokioCommand;
use tokio::time::timeout;

/// Execution result containing command output and metadata.
#[derive(Debug, Clone)]
pub struct ExecutionResult {
    pub stdout: String,
    pub stderr: String,
    pub return_code: i32,
    pub success: bool,
}

/// Executes bash commands with timeout and logging.
#[derive(Debug)]
pub struct CommandExecutor {
    timeout_seconds: u64,
    working_dir: PathBuf,
    last_command: Option<String>,
    enable_validation: bool,
    last_result: Option<ExecutionResult>,
}

impl CommandExecutor {
    /// Initialize command executor.
    ///
    /// # Arguments
    /// * `timeout_seconds` - Default timeout for command execution.
    /// * `working_dir` - Working directory for commands.
    pub fn new(timeout_seconds: u64, working_dir: Option<&Path>, enable_validation: bool) -> Self {
        Self {
            timeout_seconds,
            working_dir: working_dir.unwrap_or(Path::new(".")).to_path_buf(),
            last_command: None,
            enable_validation,
            last_result: None,
        }
    }

    /// Execute bash command with timeout.
    ///
    /// # Arguments
    /// * `command` - Bash command to execute.
    /// * `timeout_override` - Optional timeout override in seconds.
    ///
    /// # Returns
    /// ExecutionResult with stdout, stderr, and return code.
    pub async fn execute_command(
        &mut self,
        command: &str,
        timeout_override: Option<u64>,
    ) -> Result<ExecutionResult> {
        let timeout_secs = timeout_override.unwrap_or(self.timeout_seconds);
        self.last_command = Some(command.to_string());

        let mut cmd = TokioCommand::new("sh");
        cmd.arg("-c")
            .arg(command)
            .current_dir(&self.working_dir)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .stdin(Stdio::null());

        let timeout_duration = Duration::from_secs(timeout_secs);

        match timeout(timeout_duration, cmd.output()).await {
            Ok(Ok(output)) => {
                let stdout = String::from_utf8_lossy(&output.stdout).to_string();
                let stderr = String::from_utf8_lossy(&output.stderr).to_string();
                let return_code = output.status.code().unwrap_or(-1);
                let success = output.status.success();

                let result = ExecutionResult {
                    stdout,
                    stderr,
                    return_code,
                    success,
                };

                self.last_result = Some(result.clone());
                Ok(result)
            }
            Ok(Err(e)) => {
                let error_msg = format!("Failed to execute command: {}", e);
                let result = ExecutionResult {
                    stdout: String::new(),
                    stderr: error_msg.clone(),
                    return_code: -1,
                    success: false,
                };
                self.last_result = Some(result.clone());
                Err(anyhow::anyhow!(error_msg))
            }
            Err(_) => {
                let error_msg = format!("Command timed out after {} seconds", timeout_secs);
                let result = ExecutionResult {
                    stdout: String::new(),
                    stderr: error_msg.clone(),
                    return_code: -1,
                    success: false,
                };
                self.last_result = Some(result.clone());
                Err(anyhow::anyhow!(error_msg))
            }
        }
    }

    /// Execute command with retry logic.
    ///
    /// # Arguments
    /// * `command` - Bash command to execute.
    /// * `max_retries` - Maximum number of retry attempts.
    /// * `timeout_override` - Optional timeout override per attempt.
    ///
    /// # Returns
    /// ExecutionResult from the final attempt.
    pub async fn execute_with_retry(
        &mut self,
        command: &str,
        max_retries: u32,
        timeout_override: Option<u64>,
    ) -> Result<ExecutionResult> {
        let mut last_result = None;

        for attempt in 0..=max_retries {
            match self.execute_command(command, timeout_override).await {
                Ok(result) => {
                    if result.success {
                        return Ok(result);
                    }
                    last_result = Some(result);
                }
                Err(e) => {
                    if attempt == max_retries {
                        return Err(e);
                    }
                    // Wait before retry (exponential backoff)
                    let wait_time = Duration::from_secs(2u64.pow(attempt));
                    tokio::time::sleep(wait_time).await;
                }
            }
        }

        // Return the last failed attempt
        match last_result {
            Some(result) => Ok(result),
            None => Err(anyhow::anyhow!("No execution result available")),
        }
    }

    /// Validate command safety (basic checks).
    ///
    /// # Arguments
    /// * `command` - Command to validate.
    ///
    /// # Returns
    /// Tuple of (is_valid, error_message).
    pub fn validate_command(&self, command: &str) -> (bool, String) {
        if !self.enable_validation {
            return (true, String::new());
        }

        let command = command.trim();

        // Empty command
        if command.is_empty() {
            return (false, "Command cannot be empty".to_string());
        }

        // Dangerous commands (basic list)
        let dangerous_patterns = [
            "rm -rf /",
            "rm -rf *",
            "format ",
            "mkfs.",
            "dd if=",
            ":(){ :|:& };:", // Fork bomb
            "curl | sh",
            "wget | sh",
        ];

        let command_lower = command.to_lowercase();
        for pattern in &dangerous_patterns {
            if command_lower.contains(pattern) {
                return (
                    false,
                    format!("Potentially dangerous command detected: {}", pattern),
                );
            }
        }

        (true, String::new())
    }

    /// Get summary of last execution.
    ///
    /// # Returns
    /// HashMap with execution details.
    pub fn get_execution_summary(&self) -> HashMap<String, serde_json::Value> {
        let mut summary = HashMap::new();

        summary.insert(
            "command".to_string(),
            serde_json::Value::String(self.last_command.clone().unwrap_or_default()),
        );

        if let Some(result) = &self.last_result {
            summary.insert(
                "return_code".to_string(),
                serde_json::Value::Number(result.return_code.into()),
            );
            summary.insert(
                "stdout_lines".to_string(),
                serde_json::Value::Number(result.stdout.lines().count().into()),
            );
            summary.insert(
                "stderr_lines".to_string(),
                serde_json::Value::Number(result.stderr.lines().count().into()),
            );
            summary.insert(
                "success".to_string(),
                serde_json::Value::Bool(result.success),
            );
        } else {
            summary.insert("return_code".to_string(), serde_json::Value::Null);
            summary.insert(
                "stdout_lines".to_string(),
                serde_json::Value::Number(0.into()),
            );
            summary.insert(
                "stderr_lines".to_string(),
                serde_json::Value::Number(0.into()),
            );
            summary.insert("success".to_string(), serde_json::Value::Bool(false));
        }

        summary
    }

    /// Get the working directory.
    pub fn working_dir(&self) -> &Path {
        &self.working_dir
    }

    /// Set the working directory.
    pub fn set_working_dir(&mut self, working_dir: &Path) -> Result<()> {
        if !working_dir.exists() {
            return Err(anyhow::anyhow!(
                "Working directory does not exist: {}",
                working_dir.display()
            ));
        }
        if !working_dir.is_dir() {
            return Err(anyhow::anyhow!(
                "Path is not a directory: {}",
                working_dir.display()
            ));
        }
        self.working_dir = working_dir.to_path_buf();
        Ok(())
    }

    /// Get the last executed command.
    pub fn last_command(&self) -> Option<&String> {
        self.last_command.as_ref()
    }

    /// Get the last execution result.
    pub fn last_result(&self) -> Option<&ExecutionResult> {
        self.last_result.as_ref()
    }
}

impl Default for CommandExecutor {
    fn default() -> Self {
        Self::new(120, None, true)
    }
}

#[cfg(test)]
mod tests;

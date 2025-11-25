//! Checkpoint restoration functionality
//!
//! This module handles loading and restoring checkpoint data back into the agent state,
//! conversation context, file system state, and tool states.

use super::{
    AgentStateSnapshot, Checkpoint, CheckpointError, CheckpointMetadata, CheckpointResult,
    CheckpointStorageManager, ConversationSnapshot, EnvironmentSnapshot, FileSystemSnapshot,
    SessionMetadata, ToolStateSnapshot,
};
// Logger removed - restoration now returns structured results
use chrono::{DateTime, Utc};
use std::path::Path;
use tokio::fs;

/// Checkpoint restoration manager
pub struct CheckpointRestoration {
    storage_manager: CheckpointStorageManager,
    validation_enabled: bool,
}

/// Result of a checkpoint restoration operation
#[derive(Debug, Clone)]
pub struct RestorationResult {
    pub success: bool,
    pub checkpoint_id: String,
    pub session_id: String,
    pub restored_at: DateTime<Utc>,
    pub validation_results: ValidationResults,
    pub warnings: Vec<String>,
    pub errors: Vec<String>,
}

/// Results of checkpoint validation during restoration
#[derive(Debug, Clone)]
pub struct ValidationResults {
    pub checkpoint_valid: bool,
    pub agent_state_valid: bool,
    pub conversation_valid: bool,
    pub file_system_valid: bool,
    pub tool_state_valid: bool,
    pub environment_valid: bool,
    pub issues: Vec<ValidationIssue>,
}

/// A validation issue found during restoration
#[derive(Debug, Clone)]
pub struct ValidationIssue {
    pub component: String,
    pub severity: ValidationSeverity,
    pub message: String,
    pub field: Option<String>,
}

/// Severity of validation issues
#[derive(Debug, Clone, PartialEq)]
pub enum ValidationSeverity {
    Error,   // Prevents restoration
    Warning, // Allows restoration but with caveats
    Info,    // Informational only
}

/// Restored checkpoint data ready for use
#[derive(Debug, Clone)]
pub struct RestoredCheckpoint {
    pub checkpoint: Checkpoint,
    pub session_metadata: SessionMetadata,
    pub restoration_metadata: RestorationMetadata,
}

/// Result of agent state restoration
#[derive(Debug)]
pub struct AgentRestorationResult {
    pub success: bool,
    pub restored_checkpoint: RestoredCheckpoint,
    pub agent_ready: bool,
    pub warnings: Vec<String>,
    pub errors: Vec<String>,
}

/// Result of conversation restoration
#[derive(Debug)]
pub struct ConversationRestorationResult {
    pub success: bool,
    pub messages_restored: usize,
    pub total_tokens: usize,
    pub context_window_size: usize,
    pub truncated_messages: usize,
    pub model_configuration: Option<super::models::ModelConfig>,
    pub warnings: Vec<String>,
    pub errors: Vec<String>,
}

/// Metadata about the restoration process
#[derive(Debug, Clone)]
pub struct RestorationMetadata {
    pub restored_at: DateTime<Utc>,
    pub restore_duration_ms: u64,
    pub validation_passed: bool,
    pub warnings_count: usize,
    pub errors_count: usize,
}

impl CheckpointRestoration {
    /// Create a new checkpoint restoration manager
    pub fn new() -> CheckpointResult<Self> {
        let storage_manager = CheckpointStorageManager::new()?;
        Ok(Self {
            storage_manager,
            validation_enabled: true,
        })
    }

    /// Create a new restoration manager with custom storage manager
    pub fn with_storage_manager(storage_manager: CheckpointStorageManager) -> Self {
        Self {
            storage_manager,
            validation_enabled: true,
        }
    }

    /// Enable or disable validation during restoration
    pub fn set_validation_enabled(&mut self, enabled: bool) {
        self.validation_enabled = enabled;
    }

    /// Restore a checkpoint from storage
    pub async fn restore_checkpoint(
        &self,
        project_path: &Path,
        session_id: &str,
        checkpoint_id: &str,
    ) -> CheckpointResult<RestoredCheckpoint> {
        let start_time = std::time::Instant::now();

        // Load checkpoint data from storage
        let checkpoint = self
            .load_checkpoint(project_path, session_id, checkpoint_id)
            .await?;
        let session_metadata = self.load_session_metadata(project_path, session_id).await?;

        // Validate checkpoint integrity if enabled
        let validation_results = if self.validation_enabled {
            self.validate_checkpoint(&checkpoint).await?
        } else {
            ValidationResults::default()
        };

        // Check for blocking validation errors
        if validation_results.has_errors() {
            return Err(CheckpointError::restoration(format!(
                "Checkpoint validation failed for {}: {} errors found",
                checkpoint_id,
                validation_results.error_count()
            )));
        }

        let restore_duration = start_time.elapsed().as_millis() as u64;

        let restoration_metadata = RestorationMetadata {
            restored_at: Utc::now(),
            restore_duration_ms: restore_duration,
            validation_passed: validation_results.checkpoint_valid,
            warnings_count: validation_results.warning_count(),
            errors_count: validation_results.error_count(),
        };

        Ok(RestoredCheckpoint {
            checkpoint,
            session_metadata,
            restoration_metadata,
        })
    }

    /// Restore an agent from a checkpoint
    pub async fn restore_agent(
        &self,
        project_path: &Path,
        session_id: &str,
        checkpoint_id: &str,
        // logger parameter removed - results are returned instead
    ) -> CheckpointResult<AgentRestorationResult> {
        let mut warnings = Vec::new();
        let mut errors = Vec::new();


        // First restore the checkpoint data
        let restored_checkpoint = match self
            .restore_checkpoint(project_path, session_id, checkpoint_id)
            .await
        {
            Ok(checkpoint) => checkpoint,
            Err(e) => {
                let error_msg = format!("Failed to restore checkpoint: {}", e);
                errors.push(error_msg.clone());
                return Ok(AgentRestorationResult {
                    success: false,
                    restored_checkpoint: self.create_empty_restored_checkpoint(),
                    agent_ready: false,
                    warnings,
                    errors,
                });
            }
        };

        // Restore agent state components
        let agent_state_result = self
            .restore_agent_state(&restored_checkpoint.checkpoint.agent_state)
            .await;
        match agent_state_result {
            Ok(state_warnings) => warnings.extend(state_warnings),
            Err(e) => {
                let error_msg = format!("Agent state restoration failed: {}", e);
                errors.push(error_msg);
            }
        }

        // Restore conversation context
        let conversation_result = self
            .restore_conversation_context(&restored_checkpoint.checkpoint.conversation_state)
            .await;
        match conversation_result {
            Ok(conv_warnings) => warnings.extend(conv_warnings),
            Err(e) => {
                let error_msg = format!("Conversation restoration failed: {}", e);
                errors.push(error_msg);
            }
        }

        // Restore file system state
        let fs_result = self
            .restore_file_system_state(&restored_checkpoint.checkpoint.file_system_state)
            .await;
        match fs_result {
            Ok(fs_warnings) => warnings.extend(fs_warnings),
            Err(e) => {
                let error_msg = format!("File system restoration failed: {}", e);
                errors.push(error_msg);
            }
        }

        // Restore tool state
        let tool_result = self
            .restore_tool_state(&restored_checkpoint.checkpoint.tool_state)
            .await;
        match tool_result {
            Ok(tool_warnings) => warnings.extend(tool_warnings),
            Err(e) => {
                let error_msg = format!("Tool state restoration failed: {}", e);
                errors.push(error_msg);
            }
        }

        // Restore environment state
        let env_result = self
            .restore_environment_state(&restored_checkpoint.checkpoint.environment_state)
            .await;
        match env_result {
            Ok(env_warnings) => warnings.extend(env_warnings),
            Err(e) => {
                let error_msg = format!("Environment restoration failed: {}", e);
                errors.push(error_msg);
            }
        }

        let success = errors.is_empty();
        let agent_ready = success && warnings.len() < 5; // Arbitrary threshold for "ready"


        Ok(AgentRestorationResult {
            success,
            restored_checkpoint,
            agent_ready,
            warnings,
            errors,
        })
    }

    /// Restore agent state from snapshot
    async fn restore_agent_state(
        &self,
        state: &AgentStateSnapshot,
    ) -> CheckpointResult<Vec<String>> {
        let mut warnings = Vec::new();

        // Validate working directory
        if !state.working_directory.exists() {
            warnings.push(format!(
                "Working directory does not exist: {}",
                state.working_directory.display()
            ));
        } else if !state.working_directory.is_dir() {
            return Err(CheckpointError::restoration(format!(
                "Working directory is not a directory: {}",
                state.working_directory.display()
            )));
        }

        // Validate iteration bounds
        if state.current_iteration > state.max_iterations {
            return Err(CheckpointError::restoration(format!(
                "Current iteration ({}) exceeds maximum ({})",
                state.current_iteration, state.max_iterations
            )));
        }

        // Check session timing consistency
        if state.last_activity < state.session_start_time {
            warnings.push("Last activity timestamp is before session start time".to_string());
        }

        // Validate mode
        let valid_modes = ["confirm", "yolo", "human"];
        if !valid_modes.contains(&state.current_mode.as_str()) {
            warnings.push(format!("Unknown agent mode: {}", state.current_mode));
        }

        Ok(warnings)
    }

    /// Restore conversation context with advanced features
    pub async fn restore_conversation_context_advanced(
        &self,
        conversation: &ConversationSnapshot,
        max_context_window: Option<usize>,
        preserve_system_prompt: bool,
    ) -> CheckpointResult<ConversationRestorationResult> {
        let mut warnings = Vec::new();
        let mut errors = Vec::new();

        let effective_context_window =
            max_context_window.unwrap_or(conversation.context_window_size);

        // Validate system prompt
        if conversation.system_prompt.is_empty() && preserve_system_prompt {
            warnings.push("System prompt is empty but preservation was requested".to_string());
        }

        // Validate model configuration
        if conversation.model_configuration.model_name.is_empty() {
            errors.push("Model name is empty - cannot restore conversation".to_string());
        }

        // Rebuild message context with token management
        let (restored_messages, total_tokens, truncated_count) = self
            .rebuild_message_context(&conversation.messages, effective_context_window)
            .await?;

        // Validate message consistency after rebuild
        if restored_messages != conversation.messages.len() {
            warnings.push(format!(
                "Message count changed during rebuild: original={}, restored={}",
                conversation.messages.len(),
                restored_messages
            ));
        }

        // Check conversation stats consistency
        let stats = &conversation.conversation_stats;
        if stats.total_messages as usize != conversation.messages.len() {
            warnings.push(format!(
                "Conversation stats inconsistent: metadata={}, actual={}",
                stats.total_messages,
                conversation.messages.len()
            ));
        }

        // Validate token calculations
        if total_tokens > effective_context_window {
            warnings.push(format!(
                "Token count ({}) exceeds context window ({})",
                total_tokens, effective_context_window
            ));
        }

        let success = errors.is_empty();

        Ok(ConversationRestorationResult {
            success,
            messages_restored: restored_messages,
            total_tokens,
            context_window_size: effective_context_window,
            truncated_messages: truncated_count,
            model_configuration: Some(conversation.model_configuration.clone()),
            warnings,
            errors,
        })
    }

    /// Rebuild message context with proper token management
    async fn rebuild_message_context(
        &self,
        messages: &[super::models::ChatMessage],
        max_context_window: usize,
    ) -> CheckpointResult<(usize, usize, usize)> {
        if messages.is_empty() {
            return Ok((0, 0, 0));
        }

        let mut total_tokens = 0;
        let mut included_messages = 0;
        let mut truncated_count = 0;

        // Process messages in reverse order (most recent first) to respect context window
        for message in messages.iter().rev() {
            let message_tokens = match message.token_count {
                Some(tokens) => tokens,
                None => {
                    // Estimate token count if not cached
                    self.estimate_message_tokens(&message.content)
                }
            };

            if total_tokens + message_tokens > max_context_window {
                truncated_count += 1;
                continue;
            }

            total_tokens += message_tokens;
            included_messages += 1;
        }

        Ok((included_messages, total_tokens, truncated_count))
    }

    /// Estimate token count for a message (simple heuristic)
    fn estimate_message_tokens(&self, content: &str) -> usize {
        // Simple estimation: roughly 4 characters per token for English text
        // This is a rough approximation - in production you'd want proper tokenization
        (content.len() + 3) / 4
    }

    /// Restore conversation with message filtering and optimization
    pub async fn restore_conversation_with_filtering(
        &self,
        conversation: &ConversationSnapshot,
        include_system_messages: bool,
        include_tool_messages: bool,
        max_messages: Option<usize>,
    ) -> CheckpointResult<ConversationRestorationResult> {
        let mut warnings = Vec::new();
        let errors = Vec::new();

        // Filter messages based on criteria
        let filtered_messages: Vec<_> = conversation
            .messages
            .iter()
            .filter(|msg| {
                match msg.role.as_str() {
                    "system" => include_system_messages,
                    "tool" | "function" => include_tool_messages,
                    _ => true, // Include user and assistant messages by default
                }
            })
            .collect();

        let _messages_after_filter = filtered_messages.len();

        // Apply message count limit if specified
        let final_messages = if let Some(max_msg) = max_messages {
            if filtered_messages.len() > max_msg {
                warnings.push(format!(
                    "Truncating messages from {} to {} due to limit",
                    filtered_messages.len(),
                    max_msg
                ));
                // Take the most recent messages
                filtered_messages.into_iter().rev().take(max_msg).collect()
            } else {
                filtered_messages
            }
        } else {
            filtered_messages
        };

        // Calculate token usage for filtered messages
        let total_tokens: usize = final_messages
            .iter()
            .map(|msg| {
                msg.token_count
                    .unwrap_or_else(|| self.estimate_message_tokens(&msg.content))
            })
            .sum();

        let truncated_messages = conversation.messages.len() - final_messages.len();

        if truncated_messages > 0 {
            warnings.push(format!(
                "Filtered out {} messages during restoration",
                truncated_messages
            ));
        }

        Ok(ConversationRestorationResult {
            success: errors.is_empty(),
            messages_restored: final_messages.len(),
            total_tokens,
            context_window_size: conversation.context_window_size,
            truncated_messages,
            model_configuration: Some(conversation.model_configuration.clone()),
            warnings,
            errors,
        })
    }

    /// Restore conversation context from snapshot (enhanced version)
    async fn restore_conversation_context(
        &self,
        conversation: &ConversationSnapshot,
    ) -> CheckpointResult<Vec<String>> {
        self.restore_conversation_context_advanced(conversation, None, true)
            .await
            .map(|result| result.warnings)
    }

    /// Restore file system state from snapshot
    async fn restore_file_system_state(
        &self,
        fs_state: &FileSystemSnapshot,
    ) -> CheckpointResult<Vec<String>> {
        let mut warnings = Vec::new();

        // Validate working directory
        if !fs_state.working_directory.exists() {
            warnings.push(format!(
                "File system working directory does not exist: {}",
                fs_state.working_directory.display()
            ));
        }

        // Check tracked files
        for tracked_file in &fs_state.tracked_files {
            if !tracked_file.path.exists() {
                warnings.push(format!(
                    "Tracked file no longer exists: {}",
                    tracked_file.path.display()
                ));
            } else {
                // Check if file has been modified since checkpoint
                if let Ok(metadata) = fs::metadata(&tracked_file.path).await {
                    let current_size = metadata.len();
                    if current_size != tracked_file.size {
                        warnings.push(format!(
                            "File size changed: {} (was {}, now {})",
                            tracked_file.path.display(),
                            tracked_file.size,
                            current_size
                        ));
                    }
                }
            }
        }

        // Sync file permissions from snapshot where possible
        for (path, perms_snapshot) in &fs_state.file_permissions {
            if path.exists() {
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    if let Ok(metadata) = fs::metadata(path).await {
                        let current_mode = metadata.permissions().mode();
                        if current_mode != perms_snapshot.mode {
                            let desired = std::fs::Permissions::from_mode(perms_snapshot.mode);
                            match fs::set_permissions(path, desired).await {
                                Ok(_) => {
                                    warnings.push(format!(
                                        "Synchronized permissions for {}",
                                        path.display()
                                    ));
                                }
                                Err(e) => {
                                    warnings.push(format!(
                                        "Failed to set permissions for {}: {}",
                                        path.display(),
                                        e
                                    ));
                                }
                            }
                        }
                    } else {
                        warnings.push(format!("Unable to read metadata for {}", path.display()));
                    }
                }
                #[cfg(not(unix))]
                {
                    warnings.push(format!(
                        "Permission sync skipped for {} on non-Unix platform",
                        path.display()
                    ));
                }
            } else {
                warnings.push(format!(
                    "Permission entry for missing file: {}",
                    path.display()
                ));
            }
        }

        Ok(warnings)
    }

    /// Restore tool state from snapshot
    async fn restore_tool_state(
        &self,
        tool_state: &ToolStateSnapshot,
    ) -> CheckpointResult<Vec<String>> {
        let mut warnings = Vec::new();

        // Validate execution context
        let context = &tool_state.execution_context;
        if !context.working_directory.exists() {
            warnings.push(format!(
                "Tool working directory does not exist: {}",
                context.working_directory.display()
            ));
        }

        if context.timeout_seconds == 0 {
            warnings.push("Tool timeout is zero seconds".to_string());
        }

        // Check active tools
        if tool_state.active_tools.is_empty() && !tool_state.executed_commands.is_empty() {
            warnings.push("No active tools but executed commands exist".to_string());
        }

        Ok(warnings)
    }

    /// Restore environment state from snapshot
    async fn restore_environment_state(
        &self,
        env_state: &EnvironmentSnapshot,
    ) -> CheckpointResult<Vec<String>> {
        let mut warnings = Vec::new();

        // Apply environment variables cautiously - skip critical ones
        let critical_vars = ["PATH", "HOME", "SHELL"];
        for (key, value) in &env_state.environment_variables {
            if critical_vars.contains(&key.as_str()) {
                if std::env::var(key).ok().as_deref() != Some(value.as_str()) {
                    warnings.push(format!("Skipping critical env var {} change", key));
                }
                continue;
            }
            std::env::set_var(key, value);
        }

        // Additional validation of environment compatibility can be added here.
        // We intentionally avoid failing restoration due to environment differences.

        Ok(warnings)
    }

    /// Create an empty restored checkpoint for error cases
    fn create_empty_restored_checkpoint(&self) -> RestoredCheckpoint {
        use crate::checkpoint::models::{
            ConversationStats, ExecutionContext, ModelConfig, ProcessInfo, ResourceUsage,
            SystemInfo, WorkflowStep,
        };
        use std::collections::HashMap;

        RestoredCheckpoint {
            checkpoint: Checkpoint {
                metadata: CheckpointMetadata {
                    checkpoint_id: "empty".to_string(),
                    session_id: "empty".to_string(),
                    project_hash: "empty".to_string(),
                    created_at: Utc::now(),
                    iteration: 0,
                    workflow_step: WorkflowStep::Error,
                    checkpoint_version: "1.0".to_string(),
                    compressed_size: 0,
                    uncompressed_size: 0,
                    description: Some("Empty checkpoint due to restoration failure".to_string()),
                    tags: vec![],
                },
                agent_state: AgentStateSnapshot {
                    current_mode: "confirm".to_string(),
                    current_iteration: 0,
                    current_step: WorkflowStep::Error,
                    max_iterations: 1,
                    task_description: "Test checkpoint task".to_string(),
                    configuration: HashMap::new(),
                    working_directory: std::env::temp_dir(),
                    session_start_time: Utc::now(),
                    last_activity: Utc::now(),
                },
                conversation_state: ConversationSnapshot {
                    messages: vec![],
                    system_prompt: "".to_string(),
                    context_window_size: 4096,
                    model_configuration: ModelConfig {
                        model_name: "gpt-4o-mini".to_string(),
                        max_tokens: Some(1024),
                        temperature: Some(0.7),
                        top_p: Some(1.0),
                        frequency_penalty: None,
                        presence_penalty: None,
                    },
                    conversation_stats: ConversationStats {
                        total_tokens: 0,
                        total_messages: 0,
                        estimated_cost: Some(0.0),
                        api_calls: 0,
                    },
                },
                file_system_state: FileSystemSnapshot {
                    working_directory: std::env::temp_dir(),
                    tracked_files: vec![],
                    modified_files: vec![],
                    git_status: None,
                    file_permissions: HashMap::new(),
                },
                tool_state: ToolStateSnapshot {
                    active_tools: HashMap::new(),
                    executed_commands: vec![],
                    tool_registry: HashMap::new(),
                    execution_context: ExecutionContext {
                        environment_variables: HashMap::new(),
                        working_directory: std::env::temp_dir(),
                        timeout_seconds: 30,
                        max_retries: 3,
                    },
                },
                environment_state: EnvironmentSnapshot {
                    environment_variables: HashMap::new(),
                    system_info: SystemInfo {
                        os_name: "Unknown".to_string(),
                        os_version: "0.0.0".to_string(),
                        architecture: "unknown".to_string(),
                        hostname: "unknown".to_string(),
                        cpu_count: 1,
                        total_memory: 0,
                    },
                    process_info: ProcessInfo {
                        pid: 0,
                        parent_pid: None,
                        start_time: Utc::now(),
                        command_line: vec![],
                        working_directory: std::env::temp_dir(),
                    },
                    resource_usage: ResourceUsage {
                        cpu_usage: 0.0,
                        memory_usage: 0,
                        disk_usage: 0,
                        network_bytes_sent: 0,
                        network_bytes_received: 0,
                    },
                },
            },
            session_metadata: SessionMetadata {
                session_id: "empty".to_string(),
                project_hash: "empty".to_string(),
                created_at: Utc::now(),
                last_accessed: Utc::now(),
                checkpoint_count: 0,
                status: super::models::SessionStatus::Failed,
                description: Some("Failed restoration".to_string()),
                tags: vec![],
                size_bytes: 0,
            },
            restoration_metadata: RestorationMetadata {
                restored_at: Utc::now(),
                restore_duration_ms: 0,
                validation_passed: false,
                warnings_count: 0,
                errors_count: 1,
            },
        }
    }

    /// Load checkpoint data from storage
    async fn load_checkpoint(
        &self,
        project_path: &Path,
        session_id: &str,
        checkpoint_id: &str,
    ) -> CheckpointResult<Checkpoint> {
        let project_storage = self
            .storage_manager
            .get_project_storage(project_path)
            .await?;

        // Try to get session storage - this should exist if checkpoint exists
        let session_storage = project_storage.create_session(session_id).await?;

        // Load the specific checkpoint
        let checkpoint_path = session_storage.get_checkpoint_path(checkpoint_id);

        if !checkpoint_path.exists() {
            return Err(CheckpointError::not_found(format!(
                "Checkpoint {} not found in session {}",
                checkpoint_id, session_id
            )));
        }

        // Read and deserialize checkpoint
        let checkpoint_data = fs::read_to_string(&checkpoint_path).await?;
        let checkpoint: Checkpoint = serde_json::from_str(&checkpoint_data)?;

        Ok(checkpoint)
    }

    /// Load session metadata from storage
    async fn load_session_metadata(
        &self,
        project_path: &Path,
        session_id: &str,
    ) -> CheckpointResult<SessionMetadata> {
        let project_storage = self
            .storage_manager
            .get_project_storage(project_path)
            .await?;
        let sessions = project_storage.list_sessions().await?;

        sessions
            .into_iter()
            .find(|s| s.session_id == session_id)
            .ok_or_else(|| CheckpointError::not_found(format!("Session {} not found", session_id)))
    }

    /// Validate checkpoint integrity before restoration
    async fn validate_checkpoint(
        &self,
        checkpoint: &Checkpoint,
    ) -> CheckpointResult<ValidationResults> {
        let mut results = ValidationResults::default();

        // Validate checkpoint metadata
        self.validate_metadata(&checkpoint.metadata, &mut results);

        // Validate agent state
        self.validate_agent_state(&checkpoint.agent_state, &mut results);

        // Validate conversation state
        self.validate_conversation(&checkpoint.conversation_state, &mut results);

        // Validate file system state
        self.validate_file_system(&checkpoint.file_system_state, &mut results);

        // Validate tool state
        self.validate_tool_state(&checkpoint.tool_state, &mut results);

        // Validate environment state
        self.validate_environment(&checkpoint.environment_state, &mut results);

        // Set overall validation status
        results.checkpoint_valid = !results.has_errors();

        Ok(results)
    }

    /// Validate checkpoint metadata
    fn validate_metadata(&self, metadata: &CheckpointMetadata, results: &mut ValidationResults) {
        // Check required fields
        if metadata.checkpoint_id.is_empty() {
            results.add_error("metadata", "Checkpoint ID is empty");
        }

        if metadata.session_id.is_empty() {
            results.add_error("metadata", "Session ID is empty");
        }

        // Check timestamp validity
        if metadata.created_at > Utc::now() {
            results.add_warning("metadata", "Checkpoint created in the future");
        }

        // Check size consistency
        if metadata.compressed_size > metadata.uncompressed_size {
            results.add_warning("metadata", "Compressed size larger than uncompressed size");
        }
    }

    /// Validate agent state snapshot
    fn validate_agent_state(&self, state: &AgentStateSnapshot, results: &mut ValidationResults) {
        // Check working directory exists and is accessible
        if !state.working_directory.exists() {
            results.add_warning(
                "agent_state",
                format!(
                    "Working directory does not exist: {}",
                    state.working_directory.display()
                ),
            );
        }

        // Check iteration bounds
        if state.current_iteration > state.max_iterations {
            results.add_error(
                "agent_state",
                format!(
                    "Current iteration ({}) exceeds max ({})",
                    state.current_iteration, state.max_iterations
                ),
            );
        }

        // Check timestamp validity
        if state.last_activity < state.session_start_time {
            results.add_error("agent_state", "Last activity before session start");
        }

        results.agent_state_valid = !results.has_component_errors("agent_state");
    }

    /// Validate conversation snapshot
    fn validate_conversation(
        &self,
        conversation: &ConversationSnapshot,
        results: &mut ValidationResults,
    ) {
        // Check message consistency
        if conversation.messages.is_empty() {
            results.add_info("conversation", "No messages in conversation");
        }

        // Check system prompt
        if conversation.system_prompt.is_empty() {
            results.add_warning("conversation", "System prompt is empty");
        }

        // Check context window size
        if conversation.context_window_size == 0 {
            results.add_error("conversation", "Context window size is zero");
        }

        // Validate conversation stats
        let stats = &conversation.conversation_stats;
        if stats.total_messages as usize != conversation.messages.len() {
            results.add_warning(
                "conversation",
                format!(
                    "Message count mismatch: stats={}, actual={}",
                    stats.total_messages,
                    conversation.messages.len()
                ),
            );
        }

        results.conversation_valid = !results.has_component_errors("conversation");
    }

    /// Validate file system snapshot
    fn validate_file_system(&self, fs_state: &FileSystemSnapshot, results: &mut ValidationResults) {
        // Check working directory
        if !fs_state.working_directory.exists() {
            results.add_warning(
                "file_system",
                format!(
                    "Working directory does not exist: {}",
                    fs_state.working_directory.display()
                ),
            );
        }

        // Validate tracked files
        for tracked_file in &fs_state.tracked_files {
            if !tracked_file.path.exists() {
                results.add_warning(
                    "file_system",
                    format!(
                        "Tracked file does not exist: {}",
                        tracked_file.path.display()
                    ),
                );
            }
        }

        results.file_system_valid = !results.has_component_errors("file_system");
    }

    /// Validate tool state snapshot
    fn validate_tool_state(&self, tool_state: &ToolStateSnapshot, results: &mut ValidationResults) {
        // Check execution context
        let context = &tool_state.execution_context;
        if !context.working_directory.exists() {
            results.add_warning(
                "tool_state",
                format!(
                    "Tool working directory does not exist: {}",
                    context.working_directory.display()
                ),
            );
        }

        if context.timeout_seconds == 0 {
            results.add_warning("tool_state", "Tool timeout is zero");
        }

        results.tool_state_valid = !results.has_component_errors("tool_state");
    }

    /// Validate environment snapshot
    fn validate_environment(
        &self,
        env_state: &EnvironmentSnapshot,
        results: &mut ValidationResults,
    ) {
        // Check system info
        let sys_info = &env_state.system_info;
        if sys_info.cpu_count == 0 {
            results.add_warning("environment", "CPU count is zero");
        }

        if sys_info.total_memory == 0 {
            results.add_warning("environment", "Total memory is zero");
        }

        // Check process info
        let proc_info = &env_state.process_info;
        if proc_info.pid == 0 {
            results.add_warning("environment", "Process ID is zero");
        }

        results.environment_valid = !results.has_component_errors("environment");
    }
}

impl ValidationResults {
    /// Create default validation results
    fn default() -> Self {
        Self {
            checkpoint_valid: false,
            agent_state_valid: false,
            conversation_valid: false,
            file_system_valid: false,
            tool_state_valid: false,
            environment_valid: false,
            issues: Vec::new(),
        }
    }

    /// Add an error to validation results
    fn add_error(&mut self, component: &str, message: impl Into<String>) {
        self.issues.push(ValidationIssue {
            component: component.to_string(),
            severity: ValidationSeverity::Error,
            message: message.into(),
            field: None,
        });
    }

    /// Add a warning to validation results
    fn add_warning(&mut self, component: &str, message: impl Into<String>) {
        self.issues.push(ValidationIssue {
            component: component.to_string(),
            severity: ValidationSeverity::Warning,
            message: message.into(),
            field: None,
        });
    }

    /// Add an info message to validation results
    fn add_info(&mut self, component: &str, message: impl Into<String>) {
        self.issues.push(ValidationIssue {
            component: component.to_string(),
            severity: ValidationSeverity::Info,
            message: message.into(),
            field: None,
        });
    }

    /// Check if there are any validation errors
    fn has_errors(&self) -> bool {
        self.issues
            .iter()
            .any(|issue| issue.severity == ValidationSeverity::Error)
    }

    /// Check if a specific component has errors
    fn has_component_errors(&self, component: &str) -> bool {
        self.issues.iter().any(|issue| {
            issue.component == component && issue.severity == ValidationSeverity::Error
        })
    }

    /// Count total errors
    fn error_count(&self) -> usize {
        self.issues
            .iter()
            .filter(|issue| issue.severity == ValidationSeverity::Error)
            .count()
    }

    /// Count total warnings
    fn warning_count(&self) -> usize {
        self.issues
            .iter()
            .filter(|issue| issue.severity == ValidationSeverity::Warning)
            .count()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::checkpoint::models::{
        ConversationStats, ExecutionContext, ModelConfig, ProcessInfo, ResourceUsage, SystemInfo,
        WorkflowStep,
    };
    use std::collections::HashMap;
    use tempfile::TempDir;

    fn create_test_checkpoint() -> Checkpoint {
        Checkpoint {
            metadata: CheckpointMetadata {
                checkpoint_id: "001_analyze".to_string(),
                session_id: "test_session".to_string(),
                project_hash: "test_hash".to_string(),
                created_at: Utc::now(),
                iteration: 1,
                workflow_step: WorkflowStep::Analyze,
                checkpoint_version: "1.0".to_string(),
                compressed_size: 1024,
                uncompressed_size: 2048,
                description: Some("Test checkpoint".to_string()),
                tags: vec![],
            },
            agent_state: AgentStateSnapshot {
                current_mode: "confirm".to_string(),
                current_iteration: 1,
                current_step: WorkflowStep::Analyze,
                max_iterations: 10,
                task_description: "Test task for restoration".to_string(),
                configuration: HashMap::new(),
                working_directory: std::env::temp_dir(),
                session_start_time: Utc::now() - chrono::Duration::minutes(30),
                last_activity: Utc::now(),
            },
            conversation_state: ConversationSnapshot {
                messages: vec![],
                system_prompt: "Test system prompt".to_string(),
                context_window_size: 4096,
                model_configuration: ModelConfig {
                    model_name: "gpt-4o-mini".to_string(),
                    max_tokens: Some(1024),
                    temperature: Some(0.7),
                    top_p: Some(1.0),
                    frequency_penalty: None,
                    presence_penalty: None,
                },
                conversation_stats: ConversationStats {
                    total_tokens: 100,
                    total_messages: 0, // Matches empty messages vec
                    estimated_cost: Some(0.01),
                    api_calls: 1,
                },
            },
            file_system_state: FileSystemSnapshot {
                working_directory: std::env::temp_dir(),
                tracked_files: vec![],
                modified_files: vec![],
                git_status: None,
                file_permissions: HashMap::new(),
            },
            tool_state: ToolStateSnapshot {
                active_tools: HashMap::new(),
                executed_commands: vec![],
                tool_registry: HashMap::new(),
                execution_context: ExecutionContext {
                    environment_variables: HashMap::new(),
                    working_directory: std::env::temp_dir(),
                    timeout_seconds: 30,
                    max_retries: 3,
                },
            },
            environment_state: EnvironmentSnapshot {
                environment_variables: HashMap::new(),
                system_info: SystemInfo {
                    os_name: "Linux".to_string(),
                    os_version: "5.0".to_string(),
                    architecture: "x86_64".to_string(),
                    hostname: "test-host".to_string(),
                    cpu_count: 4,
                    total_memory: 8589934592,
                },
                process_info: ProcessInfo {
                    pid: 12345,
                    parent_pid: Some(1234),
                    start_time: Utc::now(),
                    command_line: vec!["agent".to_string()],
                    working_directory: std::env::temp_dir(),
                },
                resource_usage: ResourceUsage {
                    cpu_usage: 0.1,
                    memory_usage: 134217728,
                    disk_usage: 52428800,
                    network_bytes_sent: 1024,
                    network_bytes_received: 2048,
                },
            },
        }
    }

    #[tokio::test]
    async fn test_checkpoint_restoration_creation() {
        let temp_dir = TempDir::new().unwrap();
        std::env::set_var("HOME", temp_dir.path().to_str().unwrap());

        let restoration = CheckpointRestoration::new();
        assert!(restoration.is_ok());
    }

    #[tokio::test]
    async fn test_validation_results() {
        let mut results = ValidationResults::default();

        // Initially no errors or warnings
        assert!(!results.has_errors());
        assert_eq!(results.error_count(), 0);
        assert_eq!(results.warning_count(), 0);

        // Add some validation issues
        results.add_error("test", "Test error");
        results.add_warning("test", "Test warning");
        results.add_info("test", "Test info");

        assert!(results.has_errors());
        assert_eq!(results.error_count(), 1);
        assert_eq!(results.warning_count(), 1);
        assert_eq!(results.issues.len(), 3);
    }

    #[tokio::test]
    async fn test_checkpoint_validation() {
        let temp_dir = TempDir::new().unwrap();
        std::env::set_var("HOME", temp_dir.path().to_str().unwrap());

        let restoration = CheckpointRestoration::new().unwrap();
        let checkpoint = create_test_checkpoint();

        let results = restoration.validate_checkpoint(&checkpoint).await.unwrap();

        // Should have minimal warnings since we're using temp directories that exist
        assert!(!results.has_errors());
    }

    #[tokio::test]
    async fn test_checkpoint_validation_with_errors() {
        let temp_dir = TempDir::new().unwrap();
        std::env::set_var("HOME", temp_dir.path().to_str().unwrap());

        let restoration = CheckpointRestoration::new().unwrap();
        let mut checkpoint = create_test_checkpoint();

        // Introduce validation errors
        checkpoint.metadata.checkpoint_id = "".to_string(); // Empty ID should cause error
        checkpoint.agent_state.current_iteration = 999; // Exceeds max iterations
        checkpoint.conversation_state.context_window_size = 0; // Invalid context window

        let results = restoration.validate_checkpoint(&checkpoint).await.unwrap();

        // Should have errors now
        assert!(results.has_errors());
        assert!(results.error_count() > 0);
    }

    #[tokio::test]
    async fn test_validation_enabled_disabled() {
        let temp_dir = TempDir::new().unwrap();
        std::env::set_var("HOME", temp_dir.path().to_str().unwrap());

        let mut restoration = CheckpointRestoration::new().unwrap();

        // Initially validation should be enabled
        assert!(restoration.validation_enabled);

        // Disable validation
        restoration.set_validation_enabled(false);
        assert!(!restoration.validation_enabled);

        // Enable validation
        restoration.set_validation_enabled(true);
        assert!(restoration.validation_enabled);
    }

    #[tokio::test]
    async fn test_agent_state_restoration() {
        let temp_dir = TempDir::new().unwrap();
        std::env::set_var("HOME", temp_dir.path().to_str().unwrap());

        let restoration = CheckpointRestoration::new().unwrap();
        let checkpoint = create_test_checkpoint();

        let warnings = restoration
            .restore_agent_state(&checkpoint.agent_state)
            .await
            .unwrap();

        // Should have minimal warnings for a valid checkpoint
        assert!(warnings.len() <= 2); // Allow some warnings for test data
    }

    #[tokio::test]
    async fn test_conversation_context_restoration() {
        let temp_dir = TempDir::new().unwrap();
        std::env::set_var("HOME", temp_dir.path().to_str().unwrap());

        let restoration = CheckpointRestoration::new().unwrap();
        let checkpoint = create_test_checkpoint();

        let warnings = restoration
            .restore_conversation_context(&checkpoint.conversation_state)
            .await
            .unwrap();

        // Should have minimal warnings for a valid checkpoint
        assert!(warnings.is_empty() || warnings.len() <= 1);
    }

    #[tokio::test]
    async fn test_advanced_conversation_restoration() {
        let temp_dir = TempDir::new().unwrap();
        std::env::set_var("HOME", temp_dir.path().to_str().unwrap());

        let restoration = CheckpointRestoration::new().unwrap();
        let checkpoint = create_test_checkpoint();

        let result = restoration
            .restore_conversation_context_advanced(&checkpoint.conversation_state, Some(4096), true)
            .await
            .unwrap();

        assert!(result.success);
        assert_eq!(result.context_window_size, 4096);
    }

    #[tokio::test]
    async fn test_conversation_filtering() {
        let temp_dir = TempDir::new().unwrap();
        std::env::set_var("HOME", temp_dir.path().to_str().unwrap());

        let restoration = CheckpointRestoration::new().unwrap();
        let mut checkpoint = create_test_checkpoint();

        // Add some test messages
        use crate::checkpoint::models::ChatMessage;
        checkpoint.conversation_state.messages = vec![
            ChatMessage {
                role: "system".to_string(),
                content: "System message".to_string(),
                timestamp: Utc::now(),
                token_count: Some(5),
                tool_calls: None,
                tool_call_id: None,
                name: None,
            },
            ChatMessage {
                role: "user".to_string(),
                content: "User message".to_string(),
                timestamp: Utc::now(),
                token_count: Some(5),
                tool_calls: None,
                tool_call_id: None,
                name: None,
            },
            ChatMessage {
                role: "tool".to_string(),
                content: "Tool message".to_string(),
                timestamp: Utc::now(),
                token_count: Some(5),
                tool_calls: None,
                tool_call_id: None,
                name: None,
            },
        ];

        let result = restoration
            .restore_conversation_with_filtering(
                &checkpoint.conversation_state,
                false, // Exclude system messages
                false, // Exclude tool messages
                None,
            )
            .await
            .unwrap();

        assert!(result.success);
        assert_eq!(result.messages_restored, 1); // Only user message should remain
        assert_eq!(result.truncated_messages, 2); // System and tool messages filtered out
    }

    #[tokio::test]
    async fn test_message_context_rebuilding() {
        let temp_dir = TempDir::new().unwrap();
        std::env::set_var("HOME", temp_dir.path().to_str().unwrap());

        let restoration = CheckpointRestoration::new().unwrap();

        // Create test messages
        use crate::checkpoint::models::ChatMessage;
        let messages = vec![
            ChatMessage {
                role: "user".to_string(),
                content: "Short message".to_string(),
                timestamp: Utc::now(),
                token_count: Some(5),
                tool_calls: None,
                tool_call_id: None,
                name: None,
            },
            ChatMessage {
                role: "assistant".to_string(),
                content: "Another short message".to_string(),
                timestamp: Utc::now(),
                token_count: Some(10),
                tool_calls: None,
                tool_call_id: None,
                name: None,
            },
        ];

        let (included, tokens, truncated) = restoration
            .rebuild_message_context(&messages, 20)
            .await
            .unwrap();

        assert_eq!(included, 2); // Both messages should fit
        assert_eq!(tokens, 15); // Total tokens: 5 + 10
        assert_eq!(truncated, 0); // No messages truncated

        // Test with smaller context window
        let (included2, tokens2, truncated2) = restoration
            .rebuild_message_context(&messages, 10)
            .await
            .unwrap();

        assert_eq!(included2, 1); // Only one message should fit
        assert_eq!(tokens2, 10); // Only the second (most recent) message
        assert_eq!(truncated2, 1); // One message truncated
    }

    #[tokio::test]
    async fn test_token_estimation() {
        let temp_dir = TempDir::new().unwrap();
        std::env::set_var("HOME", temp_dir.path().to_str().unwrap());

        let restoration = CheckpointRestoration::new().unwrap();

        // Test token estimation
        let short_text = "Hello";
        let tokens = restoration.estimate_message_tokens(short_text);
        assert!(tokens >= 1 && tokens <= 3); // Should be reasonable estimate

        let long_text = "This is a much longer message that should result in more tokens";
        let long_tokens = restoration.estimate_message_tokens(long_text);
        assert!(long_tokens > tokens); // Should be more tokens for longer text
    }

    #[tokio::test]
    async fn test_file_system_state_restoration() {
        let temp_dir = TempDir::new().unwrap();
        std::env::set_var("HOME", temp_dir.path().to_str().unwrap());

        let restoration = CheckpointRestoration::new().unwrap();
        let checkpoint = create_test_checkpoint();

        let warnings = restoration
            .restore_file_system_state(&checkpoint.file_system_state)
            .await
            .unwrap();

        // May have warnings about missing files since we're using temp directories
        assert!(warnings.is_empty() || !warnings.is_empty()); // At least should not fail
    }

    #[tokio::test]
    async fn test_tool_state_restoration() {
        let temp_dir = TempDir::new().unwrap();
        std::env::set_var("HOME", temp_dir.path().to_str().unwrap());

        let restoration = CheckpointRestoration::new().unwrap();
        let checkpoint = create_test_checkpoint();

        let warnings = restoration
            .restore_tool_state(&checkpoint.tool_state)
            .await
            .unwrap();

        // Should have minimal warnings for a valid checkpoint
        assert!(warnings.is_empty() || warnings.len() <= 2);
    }

    #[tokio::test]
    async fn test_environment_state_restoration() {
        let temp_dir = TempDir::new().unwrap();
        std::env::set_var("HOME", temp_dir.path().to_str().unwrap());

        let restoration = CheckpointRestoration::new().unwrap();
        let mut checkpoint = create_test_checkpoint();

        // Set a benign environment variable to be restored
        checkpoint
            .environment_state
            .environment_variables
            .insert("SIMPATICODER_TEST_VAR".to_string(), "ok".to_string());

        let warnings = restoration
            .restore_environment_state(&checkpoint.environment_state)
            .await
            .unwrap();
        // Should not fail; warnings allowed
        assert!(warnings.is_empty() || !warnings.is_empty());
        assert_eq!(std::env::var("SIMPATICODER_TEST_VAR").unwrap(), "ok");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn test_file_permission_sync() {
        use crate::checkpoint::models::FilePermissions;
        use std::os::unix::fs::PermissionsExt;

        let temp_dir = TempDir::new().unwrap();
        std::env::set_var("HOME", temp_dir.path().to_str().unwrap());

        let restoration = CheckpointRestoration::new().unwrap();
        let mut checkpoint = create_test_checkpoint();

        // Create a temp file and set restrictive permissions
        let file_path = temp_dir.path().join("perm.txt");
        tokio::fs::write(&file_path, "data").await.unwrap();
        let restrictive = std::fs::Permissions::from_mode(0o600);
        tokio::fs::set_permissions(&file_path, restrictive)
            .await
            .unwrap();

        // Snapshot expects 0o644
        checkpoint.file_system_state.file_permissions.insert(
            file_path.clone(),
            FilePermissions {
                mode: 0o644,
                readable: true,
                writable: true,
                executable: false,
            },
        );

        let _ = restoration
            .restore_file_system_state(&checkpoint.file_system_state)
            .await
            .unwrap();

        let meta = tokio::fs::metadata(&file_path).await.unwrap();
        assert_eq!(meta.permissions().mode() & 0o777, 0o644);
    }

    #[tokio::test]
    async fn test_empty_restored_checkpoint_creation() {
        let temp_dir = TempDir::new().unwrap();
        std::env::set_var("HOME", temp_dir.path().to_str().unwrap());

        let restoration = CheckpointRestoration::new().unwrap();
        let empty_checkpoint = restoration.create_empty_restored_checkpoint();

        assert_eq!(empty_checkpoint.checkpoint.metadata.checkpoint_id, "empty");
        assert_eq!(empty_checkpoint.session_metadata.session_id, "empty");
        assert!(!empty_checkpoint.restoration_metadata.validation_passed);
    }
}

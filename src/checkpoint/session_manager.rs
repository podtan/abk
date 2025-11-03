//! Session Manager
//!
//! Generic session management that works with any agent implementing `AgentContext`.
//!
//! The `SessionManager` handles:
//! - Session lifecycle (start, resume, checkpoint creation)
//! - Checkpoint integration
//! - Resume tracking
//! - Task classification workflows
//!
//! # Example
//!
//! ```rust,ignore
//! use abk::checkpoint::{SessionManager, AgentContext};
//!
//! // Create session manager
//! let mut session_manager = SessionManager::new(true)?;
//!
//! // Start a session with your agent
//! session_manager.start_session(&mut my_agent, "Fix the bug in parser.rs", None).await?;
//!
//! // Create checkpoints periodically
//! if session_manager.should_create_checkpoint(my_agent.get_current_iteration()) {
//!     session_manager.create_checkpoint(&my_agent).await?;
//! }
//! ```

use crate::checkpoint::{
    AgentContext, CheckpointStorageManager, ResumeTracker, SessionStorage,
};
use crate::checkpoint::models::{
    AgentStateSnapshot, Checkpoint, CheckpointMetadata, ConversationSnapshot,
    ConversationStats, EnvironmentSnapshot, ExecutionContext, FileSystemSnapshot, ModelConfig,
    ProcessInfo, ResourceUsage, ToolStateSnapshot,
};
use anyhow::{Context, Result};
use std::path::Path;

/// Generic session manager for agent sessions.
///
/// Manages session lifecycle, checkpointing, and resume capabilities for any
/// agent that implements the `AgentContext` trait.
pub struct SessionManager {
    /// Checkpoint storage manager (optional)
    storage_manager: Option<CheckpointStorageManager>,

    /// Current active session (if any)
    current_session: Option<SessionStorage>,

    /// Current iteration counter
    current_iteration: u32,

    /// Whether checkpointing is enabled
    checkpointing_enabled: bool,

    // Classification workflow state (for unified classification workflow)
    /// Whether task classification has been completed
    classification_done: bool,

    /// The classified task type (if classification is done)
    classified_task_type: Option<String>,

    /// Whether the task template has been sent
    template_sent: bool,

    /// Initial task description (before classification)
    initial_task_description: String,

    /// Initial additional context (before classification)
    initial_additional_context: Option<String>,
}

impl SessionManager {
    /// Create a new session manager.
    ///
    /// # Arguments
    /// * `checkpointing_enabled` - Whether to enable checkpoint saving
    ///
    /// # Returns
    /// A new `SessionManager` instance, or an error if initialization fails.
    pub fn new(checkpointing_enabled: bool) -> Result<Self> {
        let storage_manager = if checkpointing_enabled {
            Some(CheckpointStorageManager::new()?)
        } else {
            None
        };

        Ok(Self {
            storage_manager,
            current_session: None,
            current_iteration: 0,
            checkpointing_enabled,
            classification_done: false,
            classified_task_type: None,
            template_sent: false,
            initial_task_description: String::new(),
            initial_additional_context: None,
        })
    }

    /// Start a new agent session.
    ///
    /// This method handles:
    /// - Checking for resumed sessions
    /// - Initializing checkpoint sessions
    /// - Setting up classification workflows
    /// - Starting legacy template workflows
    ///
    /// # Arguments
    /// * `context` - The agent context
    /// * `task_description` - The task to perform
    /// * `additional_context` - Optional additional context for the task
    ///
    /// # Returns
    /// A message describing the session start, or an error.
    pub async fn start_session<C: AgentContext>(
        &mut self,
        context: &mut C,
        task_description: &str,
        additional_context: Option<&str>,
    ) -> Result<String> {
        context.set_task_description(task_description.to_string());
        context.set_running(true);

        // Check if there's a restored checkpoint context that we should use
        if let Ok(tracker) = ResumeTracker::new() {
            let current_dir = context.get_working_directory().to_path_buf();
            if let Ok(Some(resume_context)) = tracker.get_resume_context_for_project(&current_dir) {
                context.log_info(&format!(
                    "Found restored checkpoint context for session: {}",
                    resume_context.session_id
                ));

                // Clear the resume context since we're using it
                let _ = tracker.clear_resume_context();

                // Restore iteration number from resume context first
                self.current_iteration = resume_context.iteration;
                context.set_current_iteration(self.current_iteration);

                // Try to resume from the checkpoint
                let resume_result = self
                    .resume_from_checkpoint(
                        context,
                        &resume_context.project_path,
                        &resume_context.session_id,
                        &resume_context.checkpoint_id,
                    )
                    .await;

                // Even if resume fails, continue with new task
                match resume_result {
                    Ok(_) => {
                        context.log_info("Successfully resumed from checkpoint");
                    }
                    Err(e) => {
                        context.log_error(
                            &format!(
                                "Failed to resume from checkpoint: {}, continuing with new task",
                                e
                            ),
                            None,
                        )?;
                    }
                }

                // Add the new task description as a user message
                context.add_user_message(task_description.to_string(), None);

                // Increment iteration counter
                self.current_iteration += 1;
                context.set_current_iteration(self.current_iteration);

                // Create a new checkpoint with the new task if enabled
                if self.checkpointing_enabled && self.current_session.is_some() {
                    if let Err(e) = self.create_checkpoint(context).await {
                        context.log_error(
                            &format!(
                                "Failed to create checkpoint with new task at iteration {}: {}",
                                self.current_iteration, e
                            ),
                            None,
                        )?;
                    } else {
                        context.log_info(&format!(
                            "Created new checkpoint at iteration {} with new task",
                            self.current_iteration
                        ));
                    }
                }

                return Ok(format!(
                    "Session resumed and continued with new task. Current iteration: {}. Task: {}",
                    self.current_iteration,
                    context.get_task_description()
                ));
            }
        }

        // Log session start
        let mut config_info = std::collections::HashMap::new();
        config_info.insert(
            "mode".to_string(),
            serde_json::Value::String(context.get_current_mode()),
        );
        if let Some(timeout) = context.get_config_value("execution.timeout_seconds") {
            config_info.insert("timeout".to_string(), timeout);
        }
        config_info.insert(
            "working_dir".to_string(),
            serde_json::Value::String(
                context.get_working_directory().display().to_string()
            ),
        );

        // Display streaming mode if configured
        let streaming_enabled = context
            .get_config_value("llm.enable_streaming")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        let streaming_status = if streaming_enabled {
            "🚀 STREAMING ENABLED"
        } else {
            "📞 NON-STREAMING MODE"
        };

        let endpoint = context
            .get_config_value("llm.endpoint")
            .and_then(|v| v.as_str().map(|s| s.to_string()))
            .unwrap_or_else(|| "unknown".to_string());

        println!(
            "🔧 Configuration: {} | Endpoint: {} | Provider: {} | Model: {}",
            streaming_status,
            &endpoint,
            context.get_provider_name(),
            context.get_model_name()
        );

        context.log_session_start(&context.get_current_mode(), &config_info)?;

        // Initialize checkpoint session if enabled
        if self.checkpointing_enabled {
            if let Some(ref _checkpoint_manager) = self.storage_manager {
                match self.create_checkpoint_session(context, task_description).await {
                    Ok(session_storage) => {
                        self.current_session = Some(session_storage);
                        context.log_info("Checkpoint session initialized successfully");
                    }
                    Err(e) => {
                        context.log_error(
                            &format!("Failed to create checkpoint session: {}", e),
                            None,
                        )?;
                        // Continue without checkpoints
                    }
                }
            }
        }

        // Check if task classification is enabled
        let enable_classification = context
            .get_config_value("agent.enable_task_classification")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        if enable_classification {
            // New workflow: Task classification + modular templates
            self.start_session_with_classification(context, task_description, additional_context)
                .await
        } else {
            // Original workflow: Direct task template usage
            self.start_session_legacy(context, task_description, additional_context)
                .await
        }
    }

    /// Start session with unified classification and task execution.
    ///
    /// This is a streaming-friendly workflow that combines classification
    /// and task execution in one conversation.
    async fn start_session_with_classification<C: AgentContext>(
        &mut self,
        context: &mut C,
        task_description: &str,
        additional_context: Option<&str>,
    ) -> Result<String> {
        context.log_info("Starting unified classification and task execution workflow...");

        // Store initial task info for later use
        self.initial_task_description = task_description.to_string();
        self.initial_additional_context = additional_context.map(|s| s.to_string());
        self.classification_done = false;
        self.classified_task_type = None;
        self.template_sent = false;

        // Load system template
        let execution_system_content = context
            .load_template("system")
            .await
            .context("Failed to load system template")?;

        // Prepare initial message with task description + optional context
        let initial_message = format!(
            "Task to classify and execute: {}\n\n{}",
            task_description,
            additional_context
                .map(|ctx| format!("Additional context: {}", ctx))
                .unwrap_or_default()
        );

        // Initialize chat with system message and classification+task request
        context.clear_messages();
        context.add_system_message(execution_system_content, Some("simpaticoder".to_string()));
        context.add_user_message(initial_message, Some("user".to_string()));

        Ok(format!(
            "Session started in {} mode with unified classification workflow: {}",
            context.get_current_mode(),
            task_description
        ))
    }

    /// Start session with original workflow (backward compatibility).
    async fn start_session_legacy<C: AgentContext>(
        &mut self,
        context: &mut C,
        task_description: &str,
        additional_context: Option<&str>,
    ) -> Result<String> {
        // Load system template
        let system_template = context
            .load_template("system")
            .await
            .context("Failed to load system template")?;

        let system_variables = vec![(
            "working_dir".to_string(),
            context.get_working_directory().display().to_string(),
        )];

        let system_content = context
            .render_template(&system_template, &system_variables)
            .await
            .context("Failed to render system template")?;

        // Load task template
        let task_template = context
            .load_template("task/fallback")
            .await
            .context("Failed to load task template")?;

        let task_variables = vec![
            ("task_description".to_string(), task_description.to_string()),
            (
                "task_context".to_string(),
                additional_context.unwrap_or("").to_string(),
            ),
            (
                "working_dir".to_string(),
                context.get_working_directory().display().to_string(),
            ),
        ];

        let task_content = context
            .render_template(&task_template, &task_variables)
            .await
            .context("Failed to render task template")?;

        // Initialize chat with system and task messages
        context.clear_messages();
        context.add_system_message(system_content, Some("simpaticoder".to_string()));
        context.add_user_message(task_content, Some("user".to_string()));

        Ok(format!(
            "Session started in {} mode. Task: {}",
            context.get_current_mode(),
            task_description
        ))
    }

    /// Resume session from a checkpoint, restoring conversation history.
    ///
    /// # Arguments
    /// * `context` - The agent context
    /// * `project_path` - Path to the project
    /// * `session_id` - Session ID to resume
    /// * `checkpoint_id` - Checkpoint ID within the session
    ///
    /// # Returns
    /// A message describing the resume, or an error.
    pub async fn resume_from_checkpoint<C: AgentContext>(
        &mut self,
        context: &mut C,
        project_path: &Path,
        session_id: &str,
        checkpoint_id: &str,
    ) -> Result<String> {
        context.set_running(true);

        // Initialize checkpoint restoration
        let restoration = crate::checkpoint::CheckpointRestoration::new()?;

        // Load the checkpoint
        let restored_checkpoint = restoration
            .restore_checkpoint(project_path, session_id, checkpoint_id)
            .await?;

        let checkpoint = &restored_checkpoint.checkpoint;

        // Restore agent state
        context.set_current_mode(checkpoint.agent_state.current_mode.clone());
        context.set_current_step(
            context.checkpoint_step_to_agent_step(&checkpoint.agent_state.current_step),
        );
        self.current_iteration = checkpoint.agent_state.current_iteration;
        context.set_current_iteration(self.current_iteration);
        context.set_task_description(checkpoint.agent_state.task_description.clone());

        // Clear current conversation and restore from checkpoint
        context.clear_messages();

        // Convert checkpoint messages back to agent format
        for msg in &checkpoint.conversation_state.messages {
            match msg.role.as_str() {
                "system" => {
                    context.add_system_message(msg.content.clone(), msg.name.clone());
                }
                "user" => {
                    context.add_user_message(msg.content.clone(), msg.name.clone());
                }
                "assistant" => {
                    // Check if this assistant message has tool_calls
                    if let Some(ref tool_calls) = msg.tool_calls {
                        context.add_assistant_message_with_tool_calls(
                            msg.content.clone(),
                            tool_calls.clone(),
                            msg.name.clone(),
                        );
                    } else {
                        context.add_assistant_message(msg.content.clone(), msg.name.clone());
                    }
                }
                "tool" => {
                    // Use the tool_call_id from the message, or fallback to a generic one
                    let tool_call_id = msg.tool_call_id.clone().unwrap_or_else(|| {
                        context.log_info("Tool message missing tool_call_id, using generic ID");
                        "restored_tool_call".to_string()
                    });
                    let tool_name = msg.name.clone().unwrap_or_else(|| "tool".to_string());
                    context.add_tool_message(
                        msg.content.clone(),
                        tool_call_id,
                        tool_name,
                    );
                }
                _ => {
                    // fallback to user message
                    context.add_user_message(msg.content.clone(), msg.name.clone());
                }
            }
        }

        // Initialize checkpoint session with existing session if enabled
        if self.checkpointing_enabled {
            if let Some(ref checkpoint_manager) = self.storage_manager {
                let project_storage = checkpoint_manager.get_project_storage(project_path).await?;
                match project_storage.create_session(session_id).await {
                    Ok(session_storage) => {
                        self.current_session = Some(session_storage);
                        context.log_info("Resumed checkpoint session");
                    }
                    Err(e) => {
                        context.log_error(
                            &format!("Failed to restore checkpoint session: {}", e),
                            None,
                        )?;
                        // Continue without checkpoints
                    }
                }
            }
        }

        Ok(format!(
            "Session resumed from checkpoint {} in {} mode. Task: {}",
            checkpoint_id,
            context.get_current_mode(),
            context.get_task_description()
        ))
    }

    /// Create a checkpoint session for the current task.
    async fn create_checkpoint_session<C: AgentContext>(
        &self,
        context: &C,
        task_description: &str,
    ) -> Result<SessionStorage> {
        let checkpoint_manager = self
            .storage_manager
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("Checkpoint manager not initialized"))?;

        // Get project storage for current working directory
        let current_dir = context.get_working_directory();
        let project_storage = checkpoint_manager.get_project_storage(current_dir).await?;

        // Generate unique session ID based on task and timestamp
        let timestamp = chrono::Utc::now().format("%Y_%m_%d_%H_%M");
        let task_slug = task_description
            .chars()
            .take(30)
            .filter(|c| c.is_alphanumeric() || *c == ' ')
            .collect::<String>()
            .split_whitespace()
            .take(3)
            .collect::<Vec<&str>>()
            .join("_")
            .to_lowercase();
        let session_id = format!("session_{}_{}", timestamp, task_slug);

        // Create the session
        let session_storage = project_storage.create_session(&session_id).await?;

        Ok(session_storage)
    }

    /// Create a checkpoint at the current workflow step.
    ///
    /// # Arguments
    /// * `context` - The agent context
    ///
    /// # Returns
    /// Ok(()) if checkpoint was created successfully, or an error.
    pub async fn create_checkpoint<C: AgentContext>(&mut self, context: &C) -> Result<()> {
        let iteration = context.get_current_iteration();

        // Increment checkpoint counter to ensure unique IDs
        self.current_iteration += 1;
        
        // Generate checkpoint ID based on checkpoint counter (not iteration!)
        // This ensures each checkpoint has a unique ID even if iteration doesn't change
        let checkpoint_id = format!("{:03}_{}", self.current_iteration, context.get_current_step());

        // Build the checkpoint data
        let checkpoint = self.build_checkpoint(context, &checkpoint_id, iteration).await?;

        // Save the checkpoint
        if let Some(ref mut session_storage) = self.current_session {
            session_storage.save_checkpoint(&checkpoint).await?;
        } else {
            return Err(anyhow::anyhow!("No active session for checkpointing"));
        }

        Ok(())
    }

    /// Build a complete checkpoint with all state data.
    async fn build_checkpoint<C: AgentContext>(
        &self,
        context: &C,
        checkpoint_id: &str,
        iteration: u32,
    ) -> Result<Checkpoint> {
        // Create checkpoint metadata
        let _session = self
            .current_session
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("No active session"))?;

        // Placeholder values - these would come from session
        let session_id = format!("session_{}", checkpoint_id);
        let project_hash = "unknown_project".to_string();

        let metadata = CheckpointMetadata {
            checkpoint_id: checkpoint_id.to_string(),
            session_id,
            project_hash,
            created_at: chrono::Utc::now(),
            iteration,
            workflow_step: context.get_current_step(),
            checkpoint_version: "1.0".to_string(),
            compressed_size: 0,
            uncompressed_size: 0,
            description: Some(format!(
                "Checkpoint at {} step, iteration {}",
                context.get_current_step(),
                iteration
            )),
            tags: vec![
                context.get_current_step().to_string(),
                format!("iter_{}", iteration),
            ],
        };

        // Capture agent state
        let agent_state = AgentStateSnapshot {
            current_mode: context.get_current_mode(),
            current_iteration: iteration,
            current_step: context.get_current_step(),
            max_iterations: 100, // TODO: get from config
            task_description: context.get_task_description(),
            configuration: context.get_checkpoint_config(),
            working_directory: context.get_working_directory().to_path_buf(),
            session_start_time: chrono::Utc::now(), // TODO: track session start time
            last_activity: chrono::Utc::now(),
        };

        // Capture conversation state
        let conversation_state = ConversationSnapshot {
            messages: context.get_messages(),
            system_prompt: "Simpaticoder System".to_string(),
            context_window_size: context.count_tokens(),
            model_configuration: ModelConfig {
                model_name: context.get_model_name(),
                max_tokens: Some(4000),
                temperature: Some(0.7),
                top_p: None,
                frequency_penalty: None,
                presence_penalty: None,
            },
            conversation_stats: ConversationStats {
                total_tokens: context.count_tokens(),
                total_messages: context.get_message_count(),
                estimated_cost: None,
                api_calls: 0, // TODO: track API calls
            },
        };

        // Capture environment snapshot
        let environment_state = EnvironmentSnapshot {
            environment_variables: context.get_filtered_env_vars(),
            system_info: context.get_system_info(),
            process_info: ProcessInfo {
                pid: std::process::id(),
                parent_pid: None,
                start_time: chrono::Utc::now(),
                command_line: vec![],
                working_directory: context.get_working_directory().to_path_buf(),
            },
            resource_usage: ResourceUsage {
                cpu_usage: 0.0,
                memory_usage: 0,
                disk_usage: 0,
                network_bytes_sent: 0,
                network_bytes_received: 0,
            },
        };

        // Create file system snapshot (placeholder)
        let filesystem_state = FileSystemSnapshot {
            working_directory: context.get_working_directory().to_path_buf(),
            tracked_files: vec![],
            modified_files: vec![],
            git_status: None,
            file_permissions: std::collections::HashMap::new(),
        };

        // Create tool state snapshot (placeholder)
        let tool_state = ToolStateSnapshot {
            active_tools: std::collections::HashMap::new(),
            executed_commands: vec![],
            tool_registry: std::collections::HashMap::new(),
            execution_context: ExecutionContext {
                environment_variables: context.get_filtered_env_vars(),
                working_directory: context.get_working_directory().to_path_buf(),
                timeout_seconds: context
                    .get_config_value("execution.timeout_seconds")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(120),
                max_retries: context
                    .get_config_value("execution.max_retries")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(3) as u32,
            },
        };

        Ok(Checkpoint {
            metadata,
            agent_state,
            conversation_state,
            file_system_state: filesystem_state,
            tool_state,
            environment_state,
        })
    }

    /// Check if a checkpoint should be created at the current iteration.
    ///
    /// # Arguments
    /// * `iteration` - Current iteration number
    ///
    /// # Returns
    /// true if a checkpoint should be created, false otherwise.
    pub fn should_create_checkpoint(&self, iteration: u32) -> bool {
        if !self.checkpointing_enabled || self.current_session.is_none() {
            return false;
        }

        // Create checkpoint every 5 iterations (configurable)
        // TODO: Make this configurable via checkpoint config
        iteration % 5 == 0
    }

    /// Get whether checkpointing is enabled.
    pub fn is_checkpointing_enabled(&self) -> bool {
        self.checkpointing_enabled
    }

    /// Get the current iteration number.
    pub fn get_current_iteration(&self) -> u32 {
        self.current_iteration
    }

    /// Set the current iteration number.
    pub fn set_current_iteration(&mut self, iteration: u32) {
        self.current_iteration = iteration;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Tests will be added in the next phase
}

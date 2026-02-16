//! Main agent class for ABK.

use umf::chatml::ChatMLFormatter;
use crate::checkpoint::{CheckpointStorageManager, SessionStorage};
use crate::executor::{CommandExecutor, ExecutionResult};
use crate::lifecycle::LifecyclePlugin;
use crate::config::{ConfigurationLoader, EnvironmentLoader};
use crate::observability::Logger;
use crate::provider::{ProviderFactory, LlmProvider};
use cats::{create_tool_registry_with_open_window_size, ToolRegistry};
use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde_json;

use std::collections::HashMap;
use std::path::{Path, PathBuf};

pub mod types;
pub use types::{AgentMode, ExecutionMode, ToolExecutionResult, WorkflowStep};

// Phase 2: checkpoint helpers module
pub mod checkpoint_utils;

// AgentContext implementation for abk integration
pub mod context_impl;

// AgentContext implementation for abk orchestration
pub mod context_orch;

// Phase 3: LLM parsing helpers module
pub mod llm;

// Phase 5: Tools conversion & execution helpers module
pub mod tools;

// Session orchestration - sophisticated workflow management
pub mod session;

// MCP tool integration (requires registry-mcp feature)
#[cfg(feature = "registry-mcp")]
pub mod mcp;
#[cfg(feature = "registry-mcp")]
pub use mcp::{McpToolLoader, McpToolExecutionResult};

/// Main ABK agent structure.
#[allow(dead_code)]
pub struct Agent {
    env: EnvironmentLoader,
    config: ConfigurationLoader,
    chat_formatter: ChatMLFormatter,
    lifecycle: LifecyclePlugin,
    provider: Box<dyn LlmProvider>, // Provider-based interface
    executor: CommandExecutor,
    logger: Logger,
    current_mode: AgentMode,
    current_step: WorkflowStep,
    current_iteration: u32,
    api_call_count: u32, // Track API calls
    task_description: String,
    completion_marker: String,
    is_running: bool,
    tool_registry: ToolRegistry,
    execution_mode: ExecutionMode,
    // MCP tools loaded from external servers
    #[cfg(feature = "registry-mcp")]
    mcp_tools: Option<McpToolLoader>,
    // Session management (replaces checkpoint_storage_manager, current_session, and classification state)
    // Wrapped in Option to allow taking ownership during delegation calls
    session_manager: Option<crate::checkpoint::SessionManager>,

    // TEMPORARY: These fields are deprecated and will be removed in Phase 3.4
    // They exist only for backward compatibility during migration
    #[allow(dead_code)]
    checkpoint_storage_manager: Option<CheckpointStorageManager>,
    #[allow(dead_code)]
    current_session: Option<SessionStorage>,
    #[allow(dead_code)]
    checkpointing_enabled: bool,
    #[allow(dead_code)]
    classification_done: bool,
    #[allow(dead_code)]
    classified_task_type: Option<String>,
    #[allow(dead_code)]
    template_sent: bool,
    #[allow(dead_code)]
    initial_task_description: String,
    #[allow(dead_code)]
    initial_additional_context: Option<String>,

    // Conversation turn management for X-Request-Id (like VS Code Copilot)
    current_turn_id: Option<String>,
    turn_request_count: u32,
}

impl Agent {
    /// Initialize the agent.
    ///
    /// # Arguments
    /// * `config_path` - Path to TOML config file.
    /// * `env_file` - Path to .env file.
    /// * `mode` - Initial interaction mode.
    /// * `template_base` - Base path for templates (optional).
    /// * `log_base` - Base path for logs (optional).
    pub async fn new(
        config_path: Option<&Path>,
        env_file: Option<&Path>,
        mode: Option<AgentMode>,
    ) -> Result<Self> {
        Self::new_with_bases(config_path, env_file, mode, None, None).await
    }

    /// Initialize the agent from a pre-parsed Configuration.
    ///
    /// This avoids reading any config files from disk. Use this when the caller
    /// has already loaded and merged the configuration (e.g., via figment layered config).
    pub async fn new_from_config(
        config: crate::config::Configuration,
        mode: Option<AgentMode>,
    ) -> Result<Self> {
        let env = EnvironmentLoader::new(None);
        let config_loader = ConfigurationLoader::from_config(config);

        let chat_formatter = ChatMLFormatter::new();

        let lifecycle = crate::lifecycle::find_lifecycle_plugin().await
            .context("Failed to load lifecycle plugin")?;

        let provider = ProviderFactory::create(&env).await
            .context("Failed to create LLM provider")?;

        let timeout_seconds = config_loader.get_u64("execution.timeout_seconds").unwrap_or(120);
        let enable_validation = config_loader
            .get_bool("execution.enable_dangerous_command_validation")
            .unwrap_or(true);
        let executor =
            CommandExecutor::new(timeout_seconds, Some(Path::new(".")), enable_validation);

        let log_file_path = config_loader.get_string("logging.log_file").map(PathBuf::from);
        let log_level = config_loader.get_string("logging.log_level");
        let logger = Logger::new(log_file_path.as_deref(), log_level.as_deref())?;

        let default_mode_str = config_loader
            .get_string("agent.default_mode")
            .unwrap_or_else(|| "confirm".to_string());
        let default_mode = default_mode_str.parse().unwrap_or(AgentMode::Confirm);
        let current_mode = mode.unwrap_or(default_mode);

        // Read checkpointing.enabled directly from the already-parsed config (no file I/O)
        let checkpointing_enabled = {
            // Serialize config to toml::Value to check checkpointing section
            let config_value = toml::Value::try_from(&config_loader.config)
                .ok()
                .and_then(|v| v.get("checkpointing")
                    .and_then(|c| c.get("enabled"))
                    .and_then(|e| e.as_bool()));
            config_value.unwrap_or(false)
        };

        let session_manager = Some(crate::checkpoint::SessionManager::new(checkpointing_enabled)
            .context("Failed to initialize session manager")?);

        #[cfg(feature = "registry-mcp")]
        let mcp_tools = {
            if let Some(ref mcp_config) = config_loader.config.mcp {
                if mcp_config.enabled {
                    match McpToolLoader::new(mcp_config).await {
                        Ok(loader) => {
                            if loader.has_tools() {
                                Some(loader)
                            } else {
                                None
                            }
                        }
                        Err(e) => {
                            eprintln!("Warning: Failed to load MCP tools: {}", e);
                            None
                        }
                    }
                } else {
                    None
                }
            } else {
                None
            }
        };

        let open_window_size = config_loader.config.tools.open_file_window_size;
        Ok(Self {
            env,
            config: config_loader,
            chat_formatter,
            lifecycle,
            provider,
            executor,
            logger,
            current_mode,
            current_step: WorkflowStep::Analyze,
            current_iteration: 1,
            api_call_count: 0,
            task_description: String::new(),
            completion_marker: "TASK_COMPLETED".to_string(),
            is_running: false,
            tool_registry: create_tool_registry_with_open_window_size(open_window_size),
            execution_mode: ExecutionMode::Hybrid,
            #[cfg(feature = "registry-mcp")]
            mcp_tools,
            session_manager,
            checkpoint_storage_manager: None,
            current_session: None,
            checkpointing_enabled,
            classification_done: false,
            classified_task_type: None,
            template_sent: false,
            initial_task_description: String::new(),
            initial_additional_context: None,
            current_turn_id: None,
            turn_request_count: 0,
        })
    }

    /// Initialize the agent with custom base paths.
    ///
    /// # Arguments
    /// * `config_path` - Path to TOML config file.
    /// * `env_file` - Path to .env file.
    /// * `mode` - Initial interaction mode.
    /// * `template_base` - Base path for templates (optional).
    /// * `log_base` - Base path for logs (optional).
    pub async fn new_with_bases(
        config_path: Option<&Path>,
        env_file: Option<&Path>,
        mode: Option<AgentMode>,
        template_base: Option<&Path>,
        log_base: Option<&Path>,
    ) -> Result<Self> {
        // Load environment and configuration
        let env = EnvironmentLoader::new(env_file);
        let config = ConfigurationLoader::new_with_bases(config_path, template_base, log_base)?;

        // Initialize components
        let chat_formatter = ChatMLFormatter::new();

        // Load lifecycle extension (uses new extension system)
        let lifecycle = crate::lifecycle::find_lifecycle_plugin().await
            .context("Failed to load lifecycle plugin")?;

        // Create provider using factory (new provider-based architecture)
        let provider = ProviderFactory::create(&env).await
            .context("Failed to create LLM provider")?;

        let timeout_seconds = config.get_u64("execution.timeout_seconds").unwrap_or(120);
        let enable_validation = config
            .get_bool("execution.enable_dangerous_command_validation")
            .unwrap_or(true);
        let executor =
            CommandExecutor::new(timeout_seconds, Some(Path::new(".")), enable_validation);

        let log_file_path = config.get_string("logging.log_file").map(PathBuf::from);
        let log_level = config.get_string("logging.log_level");
        let logger = Logger::new(log_file_path.as_deref(), log_level.as_deref())?;

        // Agent state
        let default_mode_str = config
            .get_string("agent.default_mode")
            .unwrap_or_else(|| "confirm".to_string());
        let default_mode = default_mode_str.parse().unwrap_or(AgentMode::Confirm);
        let current_mode = mode.unwrap_or(default_mode);
        // Capture tools-specific settings before moving `config` into the struct below

        // Environment validation: WASM providers read their own env vars via WASI.
        // Host only validates LLM_PROVIDER selection.

        // Initialize session manager with checkpoint support
        // Load checkpoint configuration directly from TOML
        let checkpointing_enabled = {
            let agent_name = std::env::var("ABK_AGENT_NAME").unwrap_or_else(|_| "NO_AGENT_NAME".to_string());
            let home_dir = std::env::var("HOME")
                .map(PathBuf::from)
                .unwrap_or_else(|_| PathBuf::from("."));
            let config_path = home_dir.join(format!(".{}/config/{}.toml", agent_name, agent_name));

            if config_path.exists() {
                match std::fs::read_to_string(&config_path) {
                    Ok(content) => match toml::from_str::<toml::Value>(&content) {
                        Ok(config) => config
                            .get("checkpointing")
                            .and_then(|c| c.get("enabled"))
                            .and_then(|e| e.as_bool())
                            .unwrap_or(false),
                        Err(_) => false,
                    },
                    Err(_) => false,
                }
            } else {
                false
            }
        };

        // Initialize session manager (replaces checkpoint_storage_manager)
        let session_manager = Some(crate::checkpoint::SessionManager::new(checkpointing_enabled)
            .context("Failed to initialize session manager")?);

        // Load MCP tools if configured
        #[cfg(feature = "registry-mcp")]
        let mcp_tools = {
            if let Some(ref mcp_config) = config.config.mcp {
                if mcp_config.enabled {
                    match McpToolLoader::new(mcp_config).await {
                        Ok(loader) => {
                            if loader.has_tools() {
                                Some(loader)
                            } else {
                                None
                            }
                        }
                        Err(e) => {
                            eprintln!("Warning: Failed to load MCP tools: {}", e);
                            None
                        }
                    }
                } else {
                    None
                }
            } else {
                None
            }
        };

        let open_window_size = config.config.tools.open_file_window_size;
        Ok(Self {
            env,
            config,
            chat_formatter,
            lifecycle,
            provider, // Provider-based architecture
            executor,
            logger,
            current_mode,
            current_step: WorkflowStep::Analyze,
            current_iteration: 1,
            api_call_count: 0, // Initialize API call counter
            task_description: String::new(),
            completion_marker: "TASK_COMPLETED".to_string(),
            is_running: false,
            tool_registry: create_tool_registry_with_open_window_size(open_window_size),
            execution_mode: ExecutionMode::Hybrid,
            #[cfg(feature = "registry-mcp")]
            mcp_tools,
            session_manager,
            // TEMPORARY: Initialize deprecated fields for backward compatibility
            checkpoint_storage_manager: None,
            current_session: None,
            checkpointing_enabled,
            classification_done: false,
            classified_task_type: None,
            template_sent: false,
            initial_task_description: String::new(),
            initial_additional_context: None,
            // Initialize conversation turn management for X-Request-Id
            current_turn_id: None,
            turn_request_count: 0,
        })
    }

    /// Set the interaction mode.
    ///
    /// # Arguments
    /// * `mode` - New interaction mode.
    pub fn set_mode(&mut self, mode: AgentMode) -> Result<()> {
        let old_mode = self.current_mode.to_string();
        self.current_mode = mode.clone();
        self.logger.log_mode_change(&old_mode, &mode.to_string())?;
        Ok(())
    }
    
    /// Initialize the remote storage backend for checkpoints.
    ///
    /// This method should be called after agent creation to enable remote
    /// storage backends like DocumentDB. It loads the checkpoint config
    /// from the agent's config file and initializes the backend connection.
    ///
    /// # Arguments
    /// * `config_path` - Path to the TOML config file containing checkpoint settings
    ///
    /// # Returns
    /// Ok(()) if successful, or an error if backend initialization fails.
    #[cfg(feature = "storage-documentdb")]
    pub async fn initialize_remote_checkpoint_backend(&mut self, config_path: Option<&Path>) -> Result<()> {
        use crate::checkpoint::GlobalCheckpointConfig;
        
        // Load checkpoint config from TOML
        let config_path = config_path.map(|p| p.to_path_buf()).unwrap_or_else(|| {
            let agent_name = std::env::var("ABK_AGENT_NAME").unwrap_or_else(|_| "trustee".to_string());
            let home_dir = std::env::var("HOME")
                .map(PathBuf::from)
                .unwrap_or_else(|_| PathBuf::from("."));
            home_dir.join(format!(".{}/config/{}.toml", agent_name, agent_name))
        });
        
        if !config_path.exists() {
            return Ok(()); // No config file, skip remote backend
        }
        
        let content = std::fs::read_to_string(&config_path)
            .with_context(|| format!("Failed to read config: {}", config_path.display()))?;
            
        let config_value: toml::Value = toml::from_str(&content)
            .with_context(|| format!("Failed to parse TOML: {}", config_path.display()))?;
            
        // Check if checkpointing section exists
        let checkpointing = match config_value.get("checkpointing") {
            Some(c) => c,
            None => return Ok(()), // No checkpointing config
        };
        
        // Re-serialize just the checkpointing section and parse with defaults
        let checkpointing_toml = toml::to_string(checkpointing)
            .with_context(|| "Failed to serialize checkpointing config")?;
        
        let checkpoint_config: GlobalCheckpointConfig = toml::from_str(&checkpointing_toml)
            .with_context(|| "Failed to parse checkpoint config")?;
        
        // Initialize the remote backend in session manager
        if let Some(ref mut session_manager) = self.session_manager {
            session_manager.initialize_remote_backend(checkpoint_config).await
                .context("Failed to initialize remote checkpoint backend")?;
        }
        
        Ok(())
    }

    // ========================================================================
    // SessionManager accessors (for backward compatibility)
    // ========================================================================
    // TODO: Remove these once old session.rs is fully replaced

    /// Check if checkpointing is enabled
    pub(crate) fn checkpointing_enabled(&self) -> bool {
        self.session_manager
            .as_ref()
            .map(|sm| sm.is_checkpointing_enabled())
            .unwrap_or(false)
    }

    /// Check if current session exists
    pub(crate) fn has_current_session(&self) -> bool {
        // This information is internal to SessionManager
        // For now, just return checkpointing_enabled as approximation
        self.session_manager
            .as_ref()
            .map(|sm| sm.is_checkpointing_enabled())
            .unwrap_or(false)
    }

    /// Execute tool calls and return structured results for proper OpenAI API tool message handling.
    /// This method returns individual tool results that can be sent as separate tool messages.

    /// Check if tool result is too large and handle accordingly

    pub async fn format_action_observation(
        &mut self,
        _command: &str,
        result: &ExecutionResult,
    ) -> Result<String> {
        // Load action_observation template from lifecycle plugin
        let template = self
            .lifecycle
            .load_template("action_observation")
            .await
            .context("Failed to load action_observation template")?;

        // Prepare variables
        let variables = vec![
            ("stdout".to_string(), result.stdout.clone()),
            ("stderr".to_string(), result.stderr.clone()),
            (
                "success".to_string(),
                if result.success {
                    "SUCCESS".to_string()
                } else {
                    "FAILED".to_string()
                },
            ),
            ("return_code".to_string(), result.return_code.to_string()),
        ];

        // Render template
        self.lifecycle
            .render_template(&template, &variables)
            .await
            .context("Failed to render action_observation template")
    }

    /// Format error message.
    ///
    /// # Arguments
    /// * `error_type` - Type of error.
    /// * `error_message` - Error message.
    /// * `context` - Error context.
    ///
    /// # Returns
    /// Formatted error string.
    pub async fn format_error(
        &mut self,
        error_type: &str,
        error_message: &str,
        context: &HashMap<String, serde_json::Value>,
    ) -> Result<String> {
        // Load format_error template from lifecycle plugin
        let template = self
            .lifecycle
            .load_template("format_error")
            .await
            .context("Failed to load format_error template")?;

        let now: DateTime<Utc> = Utc::now();
        let command = context
            .get("command")
            .and_then(|v| v.as_str())
            .unwrap_or("Unknown");

        // Prepare variables
        let variables = vec![
            ("error_type".to_string(), error_type.to_string()),
            ("error_message".to_string(), error_message.to_string()),
            ("timestamp".to_string(), now.to_rfc3339()),
            ("command".to_string(), command.to_string()),
            (
                "working_dir".to_string(),
                self.executor.working_dir().display().to_string(),
            ),
            ("execution_mode".to_string(), self.current_mode.to_string()),
        ];

        // Render template
        self.lifecycle
            .render_template(&template, &variables)
            .await
            .context("Failed to render format_error template")
    }

    /// Get the current mode.
    pub fn current_mode(&self) -> &AgentMode {
        &self.current_mode
    }

    /// Check if streaming is enabled in configuration.
    pub fn is_streaming_enabled(&self) -> bool {
        self.config.get_llm_streaming_enabled()
    }

    /// Get the current iteration number.
    pub fn current_iteration(&self) -> u32 {
        self.current_iteration
    }

    /// Get the tool registry.
    pub fn get_tool_registry(&self) -> &ToolRegistry {
        &self.tool_registry
    }

    /// Get the executor.
    pub fn executor(&self) -> &CommandExecutor {
        &self.executor
    }

    /// Get the logger.
    pub fn logger(&self) -> &Logger {
        &self.logger
    }

    /// Generate meaningful assistant content based on actual tool calls
    /// This replaces the generic "I'll execute the requested tools." with specific descriptions
    /// like professional AI assistants (Zed, VS Code Copilot) do

    /// Get the current conversation turn ID for X-Request-Id header
    pub fn get_current_turn_id(&self) -> Option<&String> {
        self.current_turn_id.as_ref()
    }

    /// Increment the request count for the current turn (for logging/debugging)
    pub fn increment_turn_request_count(&mut self) {
        self.turn_request_count += 1;
    }

    /// Start a new conversation turn and return the turn ID.
    pub fn start_conversation_turn(&mut self) -> String {
        let turn_id = uuid::Uuid::new_v4().to_string();
        self.current_turn_id = Some(turn_id.clone());
        self.turn_request_count = 0;
        turn_id
    }

    /// End the current conversation turn.
    pub fn end_conversation_turn(&mut self) {
        self.current_turn_id = None;
        self.turn_request_count = 0;
    }
}

/// Drop implementation to ensure proper cleanup
impl Drop for Agent {
    fn drop(&mut self) {
        if self.current_session.is_some() {
            // Note: We can't do async operations in Drop, but we can at least log
            eprintln!("Warning: Agent dropped with active checkpoint session. Consider calling finalize_checkpoint_session() before dropping.");
        }
    }
}

//! Configuration management for agent-based systems.
//!
//! This module provides configuration loading through TOML files and
//! environment variable management via `.env` files.
//!
//! # Example
//!
//! ```no_run
//! use abk::config::{ConfigurationLoader, EnvironmentLoader};
//! use std::path::Path;
//!
//! // Load environment variables
//! let env = EnvironmentLoader::new(None);
//!
//! // Load configuration from TOML
//! let config_loader = ConfigurationLoader::new(Some(Path::new("config/agent.toml"))).unwrap();
//! let config = &config_loader.config;
//!
//! // Access configuration
//! println!("Max iterations: {}", config.execution.max_iterations);
//! println!("LLM provider: {:?}", env.llm_provider());
//! ```

pub mod config;
pub mod environment;

// Re-export main types for convenience
pub use self::config::{
    AgentConfig, Configuration, ConfigurationLoader, ExecutionConfig,
    LlmConfig, LoggingConfig, ModeConfig, ModesConfig, SearchFilteringConfig, TemplateConfig,
    ToolsConfig,
};
pub use self::environment::EnvironmentLoader;

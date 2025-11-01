//! Agent Builder Kit (ABK) - Modular utilities for building LLM agents
//!
//! ABK provides a set of feature-gated modules for building LLM-based agents:
//!
//! - **`config`** - Configuration and environment loading
//! - **`observability`** - Structured logging, metrics, and tracing
//! - **`cli`** - CLI display utilities and formatting helpers
//!
//! # Features
//!
//! Enable the features you need in your `Cargo.toml`:
//!
//! ```toml
//! [dependencies]
//! abk = { version = "0.1", features = ["config"] }
//! # Or enable multiple features:
//! abk = { version = "0.1", features = ["config", "observability"] }
//! # Or enable everything:
//! abk = { version = "0.1", features = ["all"] }
//! ```
//!
//! # Example: Using the config feature
//!
//! ```no_run
//! use abk::config::{ConfigurationLoader, EnvironmentLoader};
//! use std::path::Path;
//!
//! // Load environment variables
//! let env = EnvironmentLoader::new(None);
//!
//! // Load configuration from TOML
//! let config_loader = ConfigurationLoader::new(Some(Path::new("config/simpaticoder.toml"))).unwrap();
//! let config = &config_loader.config;
//!
//! // Access configuration
//! println!("Max iterations: {}", config.execution.max_iterations);
//! println!("LLM provider: {:?}", env.llm_provider());
//! ```

#![warn(missing_docs)]

/// Configuration management (enabled with the `config` feature)
#[cfg(feature = "config")]
pub mod config;

/// Observability utilities (enabled with the `observability` feature)
#[cfg(feature = "observability")]
pub mod observability;

/// CLI display utilities (enabled with the `cli` feature)
#[cfg(feature = "cli")]
pub mod cli;

/// Prelude module for convenient imports
pub mod prelude {
    #[cfg(feature = "config")]
    pub use crate::config::{Configuration, ConfigurationLoader, EnvironmentLoader};
}

//! Observability utilities for agent-based systems.
//!
//! This module provides structured logging, metrics, and tracing capabilities
//! for LLM agents and related systems.
//!
//! # Example
//!
//! ```no_run
//! use abk::observability::Logger;
//! use std::collections::HashMap;
//!
//! // Create a logger
//! let logger = Logger::new(None, Some("DEBUG")).unwrap();
//!
//! // Log a session start
//! let config = HashMap::new();
//! logger.log_session_start("auto", &config).unwrap();
//!
//! // Log an LLM interaction
//! let messages = vec![];
//! logger.log_llm_interaction(&messages, "Hello, world!", "gpt-4").unwrap();
//!
//! // Log completion
//! logger.log_completion("Task completed").unwrap();
//! ```

pub mod logger;

// Re-export main types for convenience
pub use logger::Logger;

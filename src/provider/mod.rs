//! LLM Provider abstraction module
//!
//! This module provides provider traits, WASM-based provider implementation,
//! and utilities for working with different LLM providers.

pub mod traits;
pub mod factory;
pub mod types;
pub mod adapters;
pub mod wasm;

// Re-export main types
pub use traits::{LlmProvider, GenerateResponse, ToolInvocation, StreamingResponse};
pub use factory::ProviderFactory;
pub use types::{InternalMessage, GenerateConfig, InternalToolDefinition};
pub use adapters::{ChatMLAdapter, ToolAdapter};

// Re-export streaming types from umf
pub use umf::StreamChunk;

// Re-export tool types from umf
pub use umf::{ToolCall, FunctionCall, Function, Tool};

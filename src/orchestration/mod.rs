//! Agent orchestration - runtime core for LLM-based agents
//!
//! This module provides the core orchestration logic for running LLM agents:
//! - Task execution loop with iteration control
//! - Tool invocation coordination
//! - Streaming response handling
//! - Session workflow management
//!
//! The orchestration layer sits between the LLM provider and the application,
//! coordinating conversations, tool calls, and checkpointing.
//!
//! ## Three orchestration approaches:
//!
//! ### 1. Simple Orchestration (AgentRuntime)
//! For basic agents with straightforward workflows:
//! - Basic iteration loop
//! - Simple tool execution
//! - Auto-checkpointing
//!
//! ### 2. Trait-Based Orchestration (AgentSession) - DEPRECATED
//! Too complex, requires 8 separate trait implementations.
//! Use agent_orchestration instead.
//!
//! ### 3. Context-Based Orchestration (agent_orchestration) - RECOMMENDED
//! For agents with integrated state (like simpaticoder):
//! - Single AgentContext trait to implement
//! - Standalone functions (run_workflow, run_workflow_streaming)
//! - Works with tightly coupled components

pub mod runtime;
pub mod workflow;
pub mod tools;
pub mod agent_session;  // Deprecated - use agent_orchestration
pub mod agent_orchestration;

// Re-export main types
pub use runtime::{
    AgentRuntime, RuntimeConfig, RuntimeState, ExecutionResult, WorkflowStatus,
    OrchestrationProvider, OrchestrationTools, OrchestrationFormatter, CheckpointCallback,
};
pub use workflow::{WorkflowCoordinator, WorkflowStep, ExecutionMode, AgentMode};
pub use tools::{ToolCoordinator, ToolExecutionResult, ToolInvocation};

// Re-export context-based orchestration (RECOMMENDED)
pub use agent_orchestration::{
    AgentContext,
    run_workflow,
    run_workflow_streaming,
};

// Re-export sophisticated session types (DEPRECATED)
pub use agent_session::{
    AgentSession, SessionConfig, TemplateProvider, ClassificationHandler,
    SessionStorage, ErrorFormatter, OrchestrationLogger, ToolExecutor, ChatFormatter,
    ToolExecutionResult as SessionToolResult,
};


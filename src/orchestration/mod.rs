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
//! ## Orchestration approach
//!
//! Context-Based Orchestration (agent_orchestration) - RECOMMENDED
//! For agents with integrated state (like ABK's Agent):
//! - Single AgentContext trait to implement
//! - Standalone functions (run_workflow, run_workflow_streaming)
//! - Works with tightly coupled components

pub mod workflow;
pub mod tools;
pub mod agent_orchestration;
pub mod output;  // OutputSink foundation (Workstream A)

// Re-export main types
pub use workflow::{WorkflowCoordinator, WorkflowStep, ExecutionMode, AgentMode};
pub use tools::{ToolCoordinator, ToolExecutionResult, ToolInvocation};

// Re-export context-based orchestration (RECOMMENDED)
pub use agent_orchestration::{
    AgentContext,
    run_workflow,
    run_workflow_streaming,
};

// Re-export output sink types
pub use output::{OutputEvent, OutputSink, StdoutSink, NoopSink, SharedSink};
//! Structured output events and sink abstraction for agent orchestration.
//!
//! This module provides a decoupled output mechanism that allows consumers
//! (TUI, CLI, web UI, etc.) to receive agent events without parsing stdout.

/// Structured output events emitted during agent orchestration.
///
/// Each variant represents a significant event in the agent lifecycle.
/// Consumers can pattern-match on these to drive their UI or logging.
#[derive(Debug, Clone)]
pub enum OutputEvent {
    /// Agent workflow has started
    WorkflowStarted {
        task_description: String,
    },

    /// A new iteration of the agent loop has started
    IterationStarted {
        iteration: u32,
        context_tokens: usize,
    },

    /// Agent workflow has completed
    WorkflowCompleted {
        reason: String,
        iterations: u32,
    },

    /// An API call to the LLM provider is about to be made
    ApiCallStarted {
        call_number: u32,
        model: String,
        tool_count: usize,
        streaming: bool,
    },

    /// Received a full LLM response (non-streaming or accumulated)
    LlmResponse {
        text: String,
        model: String,
    },

    /// A streaming chunk has arrived
    StreamingChunk {
        delta: String,
    },

    /// Tools are being executed
    ToolsExecuting {
        tool_names: Vec<String>,
    },

    /// A single tool execution has completed
    ToolCompleted {
        tool_name: String,
        success: bool,
        content: String,
    },

    /// An error occurred
    Error {
        message: String,
        context: Option<String>,
    },

    /// General informational message (supersedes raw log_info / println)
    Info {
        message: String,
    },
}

impl std::fmt::Display for OutputEvent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::WorkflowStarted { task_description } => {
                write!(f, "🚀 Workflow started: {}", task_description)
            }
            Self::IterationStarted { iteration, context_tokens } => {
                write!(f, "📡 Iteration {} | Context = {} tokens", iteration, context_tokens)
            }
            Self::WorkflowCompleted { reason, iterations } => {
                write!(f, "✅ Workflow completed after {} iterations: {}", iterations, reason)
            }
            Self::ApiCallStarted { call_number, model, tool_count, streaming } => {
                let mode = if *streaming { "Streaming" } else { "Non-streaming" };
                write!(f, "🔥 API Call {} | {} | Model: {} | Tools: {}", call_number, mode, model, tool_count)
            }
            Self::LlmResponse { text, model } => {
                write!(f, "📡 LLM Response ({}):\n{}", model, text)
            }
            Self::StreamingChunk { delta } => {
                write!(f, "{}", delta)
            }
            Self::ToolsExecuting { tool_names } => {
                write!(f, "🔧 Executing {} tools: [{}]", tool_names.len(), tool_names.join(", "))
            }
            Self::ToolCompleted { tool_name, success, content } => {
                let status = if *success { "Result" } else { "Error" };
                write!(f, "Tool: {}\n{}: {}", tool_name, status, content)
            }
            Self::Error { message, context } => {
                if let Some(ctx) = context {
                    write!(f, "❌ Error: {} — {}", message, ctx)
                } else {
                    write!(f, "❌ Error: {}", message)
                }
            }
            Self::Info { message } => {
                write!(f, "{}", message)
            }
        }
    }
}

use std::sync::Arc;

/// Trait for receiving structured output events from the orchestration layer.
///
/// Implementations can route events to stdout, a TUI channel, a log file,
/// a WebSocket, or any other destination.
///
/// # Thread Safety
///
/// The trait requires `Send + Sync` so that a single sink can be shared
/// across async tasks (e.g. the workflow loop and a TUI event loop).
pub trait OutputSink: Send + Sync {
    /// Emit a single structured output event.
    fn emit(&self, event: OutputEvent);
}

// ---------------------------------------------------------------------------
// Concrete sink implementations
// ---------------------------------------------------------------------------

/// A sink that writes human-readable event text to stdout.
///
/// This is the default sink for CLI / non-TUI mode and preserves full
/// backward compatibility with the previous `println!`-based output.
#[derive(Debug, Clone, Default)]
pub struct StdoutSink;

impl StdoutSink {
    /// Create a new stdout sink.
    pub fn new() -> Self {
        Self
    }
}

impl OutputSink for StdoutSink {
    fn emit(&self, event: OutputEvent) {
        use std::io::Write;
        match &event {
            // StreamingChunk deltas must NOT append a newline — each delta is a
            // small fragment (sometimes a single word or token) that should be
            // printed inline so the response flows naturally on one line.
            OutputEvent::StreamingChunk { delta } => {
                print!("{}", delta);
                let _ = std::io::stdout().flush();
            }
            _ => println!("{}", event),
        }
    }
}

/// A sink that discards all events silently.
///
/// Useful for TUI mode where console output is managed by ratatui and
/// raw `println!` calls would corrupt the alternate screen buffer.
#[derive(Debug, Clone, Default)]
pub struct NoopSink;

impl NoopSink {
    /// Create a new no-op sink.
    pub fn new() -> Self {
        Self
    }
}

impl OutputSink for NoopSink {
    fn emit(&self, _event: OutputEvent) {
        // intentionally silent
    }
}

/// Convenience alias for a shared, clonable sink handle.
pub type SharedSink = Arc<dyn OutputSink>;

/// Helper to build a shared `StdoutSink`.
pub fn stdout_sink() -> SharedSink {
    Arc::new(StdoutSink::new())
}

/// Helper to build a shared `NoopSink`.
pub fn noop_sink() -> SharedSink {
    Arc::new(NoopSink::new())
}

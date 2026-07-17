//! Native Rust OpenAI provider — tools conversion utilities.

use crate::provider::types::tools::{InternalToolDefinition, ToolChoice};
use serde_json::{json, Value};

/// Convert `InternalToolDefinition` array → OpenAI tools JSON array.
pub fn tools_to_openai(tools: &[InternalToolDefinition]) -> Vec<Value> {
    tools
        .iter()
        .map(|t| {
            json!({
                "type": "function",
                "function": {
                    "name": t.name,
                    "description": t.description,
                    "parameters": t.parameters,
                }
            })
        })
        .collect()
}

/// Convert `ToolChoice` → OpenAI tool_choice value.
pub fn tool_choice_to_openai(choice: &ToolChoice) -> Value {
    match choice {
        ToolChoice::Auto => json!("auto"),
        ToolChoice::Required => json!("required"),
        ToolChoice::None => json!("none"),
        ToolChoice::Specific { name } => json!({
            "type": "function",
            "function": { "name": name }
        }),
    }
}

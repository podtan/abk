//! Native Rust OpenAI provider — tools conversion utilities.

use crate::provider::types::tools::{InternalToolDefinition, ToolChoice};
use serde_json::{json, Value};

/// Convert `InternalToolDefinition` array → OpenAI tools JSON array.
///
/// The output is **sorted by tool name** to guarantee a byte-identical
/// `tools` array across runs and processes. Tools reach this point from
/// multiple sources (cats native tools via `HashMap` iteration, MCP servers,
/// extension registries); without a deterministic sort the array order is
/// per-process random, which changes the first system-prompt tokens and
/// defeats LLM prefix-cache reuse (nghr issue 1494b6fe).
pub fn tools_to_openai(tools: &[InternalToolDefinition]) -> Vec<Value> {
    let mut sorted: Vec<&InternalToolDefinition> = tools.iter().collect();
    sorted.sort_by(|a, b| a.name.cmp(&b.name));
    sorted
        .into_iter()
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

#[cfg(test)]
mod tests {
    use super::*;

    fn def(name: &str) -> InternalToolDefinition {
        InternalToolDefinition::new(
            name,
            format!("Tool: {}", name),
            serde_json::json!({"type": "object"}),
        )
    }

    #[test]
    fn test_tools_to_openai_sorted_deterministic() {
        // Regression test for nghr issue 1494b6fe: the tools array must be
        // byte-identical across runs/processes, regardless of the order the
        // definitions arrive in (they arrive in HashMap iteration order from
        // cats' native tool registry, which is per-process random).
        let shuffled = vec![
            def("websearch"),
            def("bash"),
            def("multiedit"),
            def("grep"),
            def("list"),
            def("glob"),
            def("read"),
            def("write"),
            def("edit"),
            def("todoread"),
            def("webfetch"),
        ];

        let out = tools_to_openai(&shuffled);
        let names: Vec<&str> = out
            .iter()
            .map(|t| t["function"]["name"].as_str().unwrap())
            .collect();

        assert_eq!(
            names,
            vec![
                "bash",
                "edit",
                "glob",
                "grep",
                "list",
                "multiedit",
                "read",
                "todoread",
                "webfetch",
                "websearch",
                "write"
            ]
        );

        // Any input order yields the same output.
        let mut reversed = shuffled.clone();
        reversed.reverse();
        assert_eq!(tools_to_openai(&reversed), out);
    }

    #[test]
    fn test_tool_choice_roundtrip() {
        assert_eq!(tool_choice_to_openai(&ToolChoice::Auto), json!("auto"));
        assert_eq!(
            tool_choice_to_openai(&ToolChoice::Required),
            json!("required")
        );
        assert_eq!(tool_choice_to_openai(&ToolChoice::None), json!("none"));
        assert_eq!(
            tool_choice_to_openai(&ToolChoice::Specific {
                name: "bash".to_string()
            }),
            json!({"type": "function", "function": {"name": "bash"}})
        );
    }
}

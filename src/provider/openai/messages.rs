//! Native Rust OpenAI provider — message conversion utilities.

use crate::provider::types::InternalMessage;
use serde_json::{json, Value};

/// Convert `InternalMessage` array → OpenAI chat-completions JSON array.
pub fn messages_to_openai(messages: &[InternalMessage]) -> Vec<Value> {
    let mut result = Vec::new();

    for msg in messages {
        match msg.role {
            umf::MessageRole::System => {
                if let Some(text) = msg.text() {
                    result.push(json!({
                        "role": "system",
                        "content": text,
                    }));
                }
            }

            umf::MessageRole::User => {
                if let Some(text) = msg.text() {
                    result.push(json!({
                        "role": "user",
                        "content": text,
                    }));
                } else if let Some(blocks) = msg.blocks() {
                    // Multi-content: extract text parts
                    let parts: Vec<Value> = blocks
                        .iter()
                        .filter_map(|b| b.as_text().map(|t| json!({"type": "text", "text": t})))
                        .collect();
                    if !parts.is_empty() {
                        result.push(json!({
                            "role": "user",
                            "content": parts,
                        }));
                    }
                }
            }

            umf::MessageRole::Assistant => {
                let mut entry = json!({"role": "assistant"});
                let mut tool_calls = Vec::new();

                // Extract text content
                if let Some(text) = msg.text() {
                    entry["content"] = json!(text);
                } else if let Some(blocks) = msg.blocks() {
                    let mut text_parts = Vec::new();
                    for block in blocks {
                        match block {
                            umf::ContentBlock::Text { text } => {
                                text_parts.push(text.clone());
                            }
                            umf::ContentBlock::ToolUse { id, name, input } => {
                                tool_calls.push(json!({
                                    "id": id,
                                    "type": "function",
                                    "function": {
                                        "name": name,
                                        "arguments": serde_json::to_string(input).unwrap_or_default(),
                                    }
                                }));
                            }
                            _ => {}
                        }
                    }
                    if !text_parts.is_empty() {
                        entry["content"] = json!(text_parts.join("\n"));
                    }
                }

                if !tool_calls.is_empty() {
                    entry["tool_calls"] = json!(tool_calls);
                }

                result.push(entry);
            }

            umf::MessageRole::Tool => {
                let content = msg
                    .text()
                    .map(|t| t.to_string())
                    .unwrap_or_default();

                result.push(json!({
                    "role": "tool",
                    "tool_call_id": msg.tool_call_id.clone().unwrap_or_default(),
                    "content": content,
                }));
            }
        }
    }

    result
}

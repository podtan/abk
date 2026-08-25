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

                // Reasoning/thinking content (thinking models). Sent verbatim so
                // the rendered <think> block round-trips byte-for-byte — required
                // for prefix-cache reuse across turns (nghr 1494b6fe follow-up).
                if let Some(reasoning) = &msg.reasoning {
                    if !reasoning.is_empty() {
                        entry["reasoning_content"] = json!(reasoning);
                    }
                }

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
                // Tool result content can be in Text or Blocks form.
                // ChatMLAdapter wraps it as Blocks(vec![ToolResult{...}]).
                let content = if let Some(text) = msg.text() {
                    text.to_string()
                } else if let Some(blocks) = msg.blocks() {
                    // Extract content from ToolResult blocks
                    blocks
                        .iter()
                        .filter_map(|b| match b {
                            umf::ContentBlock::ToolResult { content, .. } => Some(content.clone()),
                            umf::ContentBlock::Text { text } => Some(text.clone()),
                            _ => None,
                        })
                        .collect::<Vec<_>>()
                        .join("\n")
                } else {
                    String::new()
                };

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

#[cfg(test)]
mod tests {
    use super::*;
    use umf::InternalMessage;

    #[test]
    fn test_assistant_reasoning_content_serialized() {
        // Regression test (nghr 1494b6fe follow-up): assistant reasoning must be
        // emitted as `reasoning_content` so the engine's rendered <think> block
        // round-trips and the prefix cache hits on the next turn.
        let msgs = vec![
            InternalMessage::system("You are helpful"),
            InternalMessage::user("hi"),
            InternalMessage::assistant_with_reasoning("Hello!", "user greeted me"),
            InternalMessage::user("and now?"),
        ];

        let out = messages_to_openai(&msgs);
        assert_eq!(out[2]["role"], "assistant");
        assert_eq!(out[2]["reasoning_content"], "user greeted me");
        assert_eq!(out[2]["content"], "Hello!");
        // Non-assistant messages must not carry the field.
        assert!(out[0].get("reasoning_content").is_none());
        assert!(out[1].get("reasoning_content").is_none());
        // Empty reasoning is omitted entirely.
        let plain = messages_to_openai(&[InternalMessage::assistant("just text")]);
        assert!(plain[0].get("reasoning_content").is_none());
    }

    #[test]
    fn test_assistant_tools_plus_reasoning_serialized() {
        let blocks = vec![
            umf::ContentBlock::text("Running ls"),
            umf::ContentBlock::tool_use("call_1", "bash", serde_json::json!({"command": "ls"})),
        ];
        let msg = InternalMessage {
            role: umf::MessageRole::Assistant,
            content: umf::MessageContent::Blocks(blocks),
            reasoning: Some("need a listing".to_string()),
            metadata: std::collections::HashMap::new(),
            tool_call_id: None,
            name: None,
        };

        let out = messages_to_openai(&[msg]);
        assert_eq!(out[0]["reasoning_content"], "need a listing");
        assert_eq!(out[0]["content"], "Running ls");
        assert_eq!(out[0]["tool_calls"][0]["function"]["name"], "bash");
    }
}

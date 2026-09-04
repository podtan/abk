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
                // Multimodal user turns carry Image blocks; render them as
                // OpenAI `image_url` content parts with `data:` URLs.
                // Text-only messages keep the plain string content form.
                let has_image = msg
                    .blocks()
                    .map(|blocks| blocks.iter().any(|b| b.as_image().is_some()))
                    .unwrap_or(false);

                if has_image {
                    let parts: Vec<Value> = msg
                        .blocks()
                        .unwrap()
                        .iter()
                        .filter_map(|b| match b {
                            umf::ContentBlock::Text { text } => {
                                Some(json!({"type": "text", "text": text}))
                            }
                            umf::ContentBlock::Image {
                                source:
                                    umf::ImageSource::Base64 {
                                        media_type,
                                        data,
                                    },
                            } => Some(json!({
                                "type": "image_url",
                                "image_url": {
                                    "url": format!("data:{};base64,{}", media_type, data),
                                }
                            })),
                            _ => None,
                        })
                        .collect();
                    if !parts.is_empty() {
                        result.push(json!({
                            "role": "user",
                            "content": parts,
                        }));
                    }
                } else if let Some(text) = msg.text() {
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

#[test]
fn test_user_image_blocks_serialize_as_image_url_parts() {
    // Wire format (nghr 02ce6d5e / task 95b19c56): user turns with Image
    // blocks must serialize text+image parts in order, images as
    // {"type":"image_url","image_url":{"url":"data:{mime};base64,{data}"}}.
    let blocks = vec![
        umf::ContentBlock::text("describe this"),
        umf::ContentBlock::image(umf::ImageSource::Base64 {
            media_type: "image/jpeg".to_string(),
            data: "QUJD".to_string(),
        }),
    ];
    let msg = InternalMessage {
        role: umf::MessageRole::User,
        content: umf::MessageContent::Blocks(blocks),
        reasoning: None,
        metadata: std::collections::HashMap::new(),
        tool_call_id: None,
        name: None,
    };

    let out = messages_to_openai(&[msg]);
    assert_eq!(out[0]["role"], "user");
    let parts = out[0]["content"].as_array().unwrap();
    assert_eq!(parts.len(), 2);
    assert_eq!(parts[0]["type"], "text");
    assert_eq!(parts[0]["text"], "describe this");
    assert_eq!(parts[1]["type"], "image_url");
    assert_eq!(
        parts[1]["image_url"]["url"],
        "data:image/jpeg;base64,QUJD"
    );
}

#[test]
fn test_user_image_only_turn_serializes_image_part_without_text() {
    // Image-only turn (empty content dropped, no text part emitted).
    let blocks = vec![umf::ContentBlock::image(umf::ImageSource::Base64 {
        media_type: "image/png".to_string(),
        data: "iVBOR".to_string(),
    })];
    let msg = InternalMessage {
        role: umf::MessageRole::User,
        content: umf::MessageContent::Blocks(blocks),
        reasoning: None,
        metadata: std::collections::HashMap::new(),
        tool_call_id: None,
        name: None,
    };

    let out = messages_to_openai(&[msg]);
    let parts = out[0]["content"].as_array().unwrap();
    assert_eq!(parts.len(), 1);
    assert_eq!(parts[0]["type"], "image_url");
    assert_eq!(parts[0]["image_url"]["url"], "data:image/png;base64,iVBOR");
}

#[test]
fn test_user_plain_text_unchanged_string_content() {
    // Regression guard: text-only user messages must keep the plain string
    // content form (no parts array).
    let out = messages_to_openai(&[InternalMessage::user("plain")]);
    assert_eq!(out[0]["content"], "plain");
    assert!(out[0]["content"].is_string());
}

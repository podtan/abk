//! ChatML to internal message format adapter.
//!
//! This module provides conversion between ChatML messages (used internally
//! by ABK) and the provider-agnostic internal message format.

use crate::provider::types::internal::{
    ContentBlock, InternalMessage, MessageContent, MessageRole,
};
use crate::provider::ToolCall;
use anyhow::Result;
use umf::chatml::{ChatMLFormatter, ChatMLMessage, MessageRole as ChatMLRole};

/// Adapter for converting between ChatML and internal message formats
pub struct ChatMLAdapter;

impl ChatMLAdapter {
    /// Convert ChatML formatter messages to internal message format
    ///
    /// # Arguments
    /// * `formatter` - ChatML formatter containing conversation history
    ///
    /// # Returns
    /// Vector of internal messages
    pub fn to_internal(formatter: &ChatMLFormatter) -> Result<Vec<InternalMessage>> {
        let mut internal_messages = Vec::new();

        for chatml_msg in formatter.get_messages() {
            let internal_msg = Self::message_to_internal(chatml_msg)?;
            internal_messages.push(internal_msg);
        }

        Ok(internal_messages)
    }

    /// Convert a single ChatML message to internal format
    fn message_to_internal(msg: &ChatMLMessage) -> Result<InternalMessage> {
        let role = Self::convert_role(&msg.role);

        // Multimodal sidecar: ChatML images map to Image blocks so provider
        // adapters can serialize them (e.g. OpenAI image_url parts).
        if !msg.images.is_empty() {
            let mut blocks = Vec::new();

            // Text part first (may be empty for image-only turns)
            if !msg.content.is_empty() {
                blocks.push(ContentBlock::text(&msg.content));
            }

            for image in &msg.images {
                blocks.push(ContentBlock::image(
                    umf::ImageSource::Base64 {
                        media_type: image.mime.clone(),
                        data: image.data.clone(),
                    },
                ));
            }

            let mut metadata = std::collections::HashMap::new();
            if let Some(ref name) = msg.name {
                metadata.insert("name".to_string(), name.clone());
            }

            return Ok(InternalMessage {
                role,
                content: MessageContent::Blocks(blocks),
                reasoning: msg.reasoning_content.clone(),
                metadata,
                tool_call_id: None,
                name: None,
            });
        }

        // If message has tool_calls, create blocks content
        if let Some(ref tool_calls) = msg.tool_calls {
            let mut blocks = Vec::new();

            // Add text content block if present
            if !msg.content.is_empty() {
                blocks.push(ContentBlock::text(&msg.content));
            }

            // Add tool call blocks
            for tool_call in tool_calls {
                let input: serde_json::Value = serde_json::from_str(&tool_call.function.arguments)
                    .unwrap_or(serde_json::Value::Object(serde_json::Map::new()));

                blocks.push(ContentBlock::tool_use(
                    &tool_call.id,
                    &tool_call.function.name,
                    input,
                ));
            }

            let mut metadata = std::collections::HashMap::new();
            if let Some(ref name) = msg.name {
                metadata.insert("name".to_string(), name.clone());
            }

            Ok(InternalMessage {
                role,
                content: MessageContent::Blocks(blocks),
                reasoning: msg.reasoning_content.clone(),
                metadata,
                tool_call_id: None,
                name: None,
            })
        } else if let Some(ref tool_call_id) = msg.tool_call_id {
            // This is a tool result message
            let blocks = vec![ContentBlock::tool_result(tool_call_id, &msg.content)];

            let tool_name = msg.name.clone().unwrap_or_else(|| "unknown".to_string());

            Ok(InternalMessage {
                role,
                content: MessageContent::Blocks(blocks),
                reasoning: None,
                metadata: std::collections::HashMap::new(),
                tool_call_id: Some(tool_call_id.clone()),
                name: Some(tool_name),
            })
        } else {
            // Simple text message
            let mut metadata = std::collections::HashMap::new();
            if let Some(ref name) = msg.name {
                metadata.insert("name".to_string(), name.clone());
            }

            Ok(InternalMessage {
                role,
                content: MessageContent::Text(msg.content.clone()),
                reasoning: msg.reasoning_content.clone(),
                metadata,
                tool_call_id: None,
                name: None,
            })
        }
    }

    /// Convert ChatML role to internal role
    fn convert_role(role: &ChatMLRole) -> MessageRole {
        match role {
            ChatMLRole::System => MessageRole::System,
            ChatMLRole::User => MessageRole::User,
            ChatMLRole::Assistant => MessageRole::Assistant,
            ChatMLRole::Tool => MessageRole::Tool,
        }
    }

    /// Convert internal messages back to ChatML format (for backward compatibility)
    ///
    /// # Arguments
    /// * `messages` - Internal messages to convert
    ///
    /// # Returns
    /// Vector of ChatML messages
    pub fn from_internal(messages: &[InternalMessage]) -> Result<Vec<ChatMLMessage>> {
        let mut chatml_messages = Vec::new();

        for msg in messages {
            let chatml_msg = Self::internal_to_message(msg)?;
            chatml_messages.push(chatml_msg);
        }

        Ok(chatml_messages)
    }

    /// Convert a single internal message to ChatML format
    fn internal_to_message(msg: &InternalMessage) -> Result<ChatMLMessage> {
        let role = Self::convert_internal_role(&msg.role);
        let name = msg.metadata.get("name").cloned();

        match &msg.content {
            MessageContent::Text(text) => {
                let mut m = ChatMLMessage::new(role, text.clone(), name);
                m.reasoning_content = msg.reasoning.clone();
                Ok(m)
            }
            MessageContent::Blocks(blocks) => {
                // Extract tool calls and tool results from blocks
                let mut text_parts = Vec::new();
                let mut tool_calls = Vec::new();
                let mut tool_call_id = None;
                let mut images: Vec<umf::chatml::ImageAttachment> = Vec::new();

                for block in blocks {
                    match block {
                        ContentBlock::Text { text } => {
                            text_parts.push(text.clone());
                        }
                        ContentBlock::ToolUse { id, name, input } => {
                            let arguments = serde_json::to_string(input)?;
                            tool_calls.push(ToolCall {
                                id: id.clone(),
                                r#type: "function".to_string(),
                                function: crate::provider::FunctionCall {
                                    name: name.clone(),
                                    arguments,
                                },
                            });
                        }
                        ContentBlock::ToolResult {
                            tool_use_id,
                            content,
                        } => {
                            tool_call_id = Some(tool_use_id.clone());
                            text_parts.push(content.clone());
                        }
                        ContentBlock::Image { source } => {
                            // Multimodal: preserve images on the ChatML sidecar
                            // (base64 form only; URL sources degrade: dropped here).
                            if let umf::ImageSource::Base64 { media_type, data } = source {
                                images.push(umf::chatml::ImageAttachment::new(
                                    media_type.clone(),
                                    data.clone(),
                                ));
                            }
                        }
                    }
                }

                let content = text_parts.join("\n");

                // Create appropriate ChatML message based on what we found
                if !tool_calls.is_empty() {
                    let mut m = ChatMLMessage::new_assistant_with_tool_calls(content, tool_calls);
                    m.reasoning_content = msg.reasoning.clone();
                    m.images = images;
                    Ok(m)
                } else if let Some(tid) = tool_call_id {
                    let tool_name = msg
                        .metadata
                        .get("tool_name")
                        .cloned()
                        .unwrap_or_else(|| "unknown".to_string());
                    let mut m = ChatMLMessage::new_tool(content, tid, tool_name);
                    m.images = images;
                    Ok(m)
                } else {
                    let mut m = ChatMLMessage::new(role, content, name);
                    m.images = images;
                    Ok(m)
                }
            }
        }
    }

    /// Convert internal role to ChatML role
    fn convert_internal_role(role: &MessageRole) -> ChatMLRole {
        match role {
            MessageRole::System => ChatMLRole::System,
            MessageRole::User => ChatMLRole::User,
            MessageRole::Assistant => ChatMLRole::Assistant,
            MessageRole::Tool => ChatMLRole::Tool,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::{FunctionCall, ToolCall};

    #[test]
    fn test_simple_message_conversion() {
        let chatml_msg = ChatMLMessage::new(ChatMLRole::User, "Hello, world!".to_string(), None);

        let internal_msg = ChatMLAdapter::message_to_internal(&chatml_msg).unwrap();
        assert_eq!(internal_msg.role, MessageRole::User);
        assert_eq!(internal_msg.text(), Some("Hello, world!"));
    }

    #[test]
    fn test_tool_call_message_conversion() {
        let tool_call = ToolCall {
            id: "call_123".to_string(),
            r#type: "function".to_string(),
            function: FunctionCall {
                name: "get_weather".to_string(),
                arguments: r#"{"location":"SF"}"#.to_string(),
            },
        };

        let chatml_msg = ChatMLMessage::new_assistant_with_tool_calls(
            "Let me check the weather".to_string(),
            vec![tool_call],
        );

        let internal_msg = ChatMLAdapter::message_to_internal(&chatml_msg).unwrap();
        assert_eq!(internal_msg.role, MessageRole::Assistant);

        if let MessageContent::Blocks(blocks) = &internal_msg.content {
            assert_eq!(blocks.len(), 2); // text + tool_use
            assert!(matches!(blocks[0], ContentBlock::Text { .. }));
            assert!(matches!(blocks[1], ContentBlock::ToolUse { .. }));
        } else {
            panic!("Expected blocks content");
        }
    }

    #[test]
    fn test_tool_result_message_conversion() {
        let chatml_msg = ChatMLMessage::new_tool(
            "72°F, sunny".to_string(),
            "call_123".to_string(),
            "get_weather".to_string(),
        );

        let internal_msg = ChatMLAdapter::message_to_internal(&chatml_msg).unwrap();
        assert_eq!(internal_msg.role, MessageRole::Tool);
        assert_eq!(internal_msg.tool_call_id, Some("call_123".to_string()));
        assert_eq!(internal_msg.name, Some("get_weather".to_string()));
    }

    #[test]
    fn test_round_trip_conversion() {
        let mut formatter = ChatMLFormatter::new();
        formatter.add_system_message("You are helpful".to_string(), None);
        formatter.add_user_message("Hello".to_string(), None);
        formatter.add_assistant_message("Hi there!".to_string(), None);

        let internal = ChatMLAdapter::to_internal(&formatter).unwrap();
        let back_to_chatml = ChatMLAdapter::from_internal(&internal).unwrap();

        let original = formatter.get_messages();
        assert_eq!(original.len(), back_to_chatml.len());
        for (orig, converted) in original.iter().zip(back_to_chatml.iter()) {
            assert_eq!(orig.role, converted.role);
            assert_eq!(orig.content, converted.content);
        }
    }

    #[test]
    fn test_reasoning_round_trip_preserved() {
        // Regression test (nghr 1494b6fe follow-up): assistant reasoning_content
        // must survive ChatML -> InternalMessage -> request serialization. The
        // engine renders <think> blocks from reasoning_content; dropping it
        // changes the rendered prompt and defeats prefix-cache reuse across turns.
        let mut formatter = ChatMLFormatter::new();
        formatter.add_system_message("You are helpful".to_string(), None);
        formatter.add_user_message("Hello".to_string(), None);
        formatter.add_assistant_message_with_reasoning(
            "Done.".to_string(),
            "I should greet the user politely.".to_string(),
            None,
        );

        let internal = ChatMLAdapter::to_internal(&formatter).unwrap();
        assert_eq!(
            internal[2].reasoning.as_deref(),
            Some("I should greet the user politely.")
        );

        // And back to ChatML must keep it too.
        let back = ChatMLAdapter::from_internal(&internal).unwrap();
        assert_eq!(
            back[2].reasoning_content.as_deref(),
            Some("I should greet the user politely.")
        );
    }

    #[test]
    fn test_reasoning_with_tool_calls_preserved() {
        let tool_call = ToolCall {
            id: "call_1".to_string(),
            r#type: "function".to_string(),
            function: FunctionCall {
                name: "bash".to_string(),
                arguments: r#"{"command":"ls"}"#.to_string(),
            },
        };

        let mut formatter = ChatMLFormatter::new();
        formatter.add_assistant_message_with_reasoning(
            "Running ls.".to_string(),
            "User wants a listing.".to_string(),
            Some(vec![tool_call]),
        );

        let internal = ChatMLAdapter::to_internal(&formatter).unwrap();
        assert_eq!(
            internal[0].reasoning.as_deref(),
            Some("User wants a listing.")
        );
        assert!(internal[0].blocks().is_some());
    }
}

#[test]
fn test_chatml_images_sidecar_maps_to_image_blocks() {
    // Sidecar → Internal: images become ordered [Text?, Image...] blocks so
    // provider adapters can serialize them (nghr 02ce6d5e / task 95b19c56).
    let mut msg = ChatMLMessage::new(ChatMLRole::User, "describe this".to_string(), None);
    msg.images.push(
        umf::chatml::ImageAttachment::new("image/jpeg", "QUJD").with_filename("photo.jpg"),
    );

    let internal = ChatMLAdapter::message_to_internal(&msg).unwrap();
    let blocks = internal.blocks().unwrap();
    assert_eq!(blocks.len(), 2);
    assert_eq!(blocks[0].as_text(), Some("describe this"));
    match blocks[1].as_image().unwrap() {
        umf::ImageSource::Base64 { media_type, data } => {
            assert_eq!(media_type, "image/jpeg");
            assert_eq!(data, "QUJD");
        }
        other => panic!("expected base64 image source, got {:?}", other),
    }
}

#[test]
fn test_chatml_image_only_sidecar_maps_without_text_block() {
    let mut msg = ChatMLMessage::new(ChatMLRole::User, String::new(), None);
    msg.images.push(umf::chatml::ImageAttachment::new("image/png", "iVBOR"));

    let internal = ChatMLAdapter::message_to_internal(&msg).unwrap();
    let blocks = internal.blocks().unwrap();
    assert_eq!(blocks.len(), 1);
    assert!(blocks[0].as_image().is_some());
}

#[test]
fn test_internal_image_blocks_roundtrip_into_chatml_sidecar() {
    // Internal → ChatML (from_internal): base64 Image blocks must land on the
    // ChatMLMessage.images sidecar instead of being dropped.
    let blocks = vec![
        umf::ContentBlock::text("look at this"),
        umf::ContentBlock::image(umf::ImageSource::Base64 {
            media_type: "image/webp".to_string(),
            data: "UklGRg".to_string(),
        }),
    ];
    let internal = InternalMessage {
        role: MessageRole::User,
        content: MessageContent::Blocks(blocks),
        reasoning: None,
        metadata: std::collections::HashMap::new(),
        tool_call_id: None,
        name: None,
    };

    let chatml_msgs = ChatMLAdapter::from_internal(&[internal]).unwrap();
    assert_eq!(chatml_msgs[0].content, "look at this");
    assert_eq!(chatml_msgs[0].images.len(), 1);
    assert_eq!(chatml_msgs[0].images[0].mime, "image/webp");
    assert_eq!(chatml_msgs[0].images[0].data, "UklGRg");
}

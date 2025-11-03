//! ChatML to internal message format adapter.
//!
//! This module provides conversion between ChatML messages (used internally
//! by simpaticoder) and the provider-agnostic internal message format.

use umf::chatml::{ChatMLFormatter, ChatMLMessage, MessageRole as ChatMLRole};
use crate::provider::types::internal::{
    ContentBlock, InternalMessage, MessageContent, MessageRole,
};
use crate::provider::ToolCall;
use anyhow::Result;

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
                Ok(ChatMLMessage::new(role, text.clone(), name))
            }
            MessageContent::Blocks(blocks) => {
                // Extract tool calls and tool results from blocks
                let mut text_parts = Vec::new();
                let mut tool_calls = Vec::new();
                let mut tool_call_id = None;

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
                                    arguments 
                                },
                            });
                        }
                        ContentBlock::ToolResult { tool_use_id, content } => {
                            tool_call_id = Some(tool_use_id.clone());
                            text_parts.push(content.clone());
                        }
                        ContentBlock::Image { .. } => {
                            // Images not supported in ChatML yet, skip
                        }
                    }
                }

                let content = text_parts.join("\n");

                // Create appropriate ChatML message based on what we found
                if !tool_calls.is_empty() {
                    Ok(ChatMLMessage::new_assistant_with_tool_calls(content, tool_calls))
                } else if let Some(tid) = tool_call_id {
                    let tool_name = msg.metadata.get("tool_name")
                        .cloned()
                        .unwrap_or_else(|| "unknown".to_string());
                    Ok(ChatMLMessage::new_tool(content, tid, tool_name))
                } else {
                    Ok(ChatMLMessage::new(role, content, name))
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
        let chatml_msg = ChatMLMessage::new(
            ChatMLRole::User,
            "Hello, world!".to_string(),
            None,
        );

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
        assert_eq!(
            internal_msg.tool_call_id,
            Some("call_123".to_string())
        );
        assert_eq!(
            internal_msg.name,
            Some("get_weather".to_string())
        );
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
}

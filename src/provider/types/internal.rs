//! Internal message format for provider-agnostic message representation.
//!
//! This module re-exports the Universal Message Format (UMF) types.
//! UMF is now a standalone crate that can be used independently.

// Re-export all UMF types
pub use umf::{
    InternalMessage, MessageRole, MessageContent, ContentBlock, ImageSource,
};

// Keep tests here for backward compatibility
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_message_creation() {
        let msg = InternalMessage::system("You are a helpful assistant");
        assert_eq!(msg.role, MessageRole::System);
        assert_eq!(msg.text(), Some("You are a helpful assistant"));

        let msg = InternalMessage::user("Hello");
        assert_eq!(msg.role, MessageRole::User);
        assert_eq!(msg.text(), Some("Hello"));

        let msg = InternalMessage::assistant("Hi there!");
        assert_eq!(msg.role, MessageRole::Assistant);
        assert_eq!(msg.text(), Some("Hi there!"));
    }

    #[test]
    fn test_content_blocks() {
        let block = ContentBlock::text("Hello world");
        assert_eq!(block.as_text(), Some("Hello world"));

        let block = ContentBlock::tool_use("tool_123", "get_weather", serde_json::json!({"location": "SF"}));
        let (id, name, input) = block.as_tool_use().unwrap();
        assert_eq!(id, "tool_123");
        assert_eq!(name, "get_weather");
        assert_eq!(input["location"], "SF");

        let block = ContentBlock::tool_result("tool_123", "72°F, sunny");
        let (tool_use_id, content) = block.as_tool_result().unwrap();
        assert_eq!(tool_use_id, "tool_123");
        assert_eq!(content, "72°F, sunny");
    }

    #[test]
    fn test_message_serialization() {
        let msg = InternalMessage::user("Test message");
        let json = serde_json::to_string(&msg).unwrap();
        let deserialized: InternalMessage = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.role, MessageRole::User);
        assert_eq!(deserialized.text(), Some("Test message"));
    }

    #[test]
    fn test_role_string_conversion() {
        assert_eq!(MessageRole::System.as_str(), "system");
        assert_eq!(MessageRole::User.as_str(), "user");
        assert_eq!(MessageRole::Assistant.as_str(), "assistant");
        assert_eq!(MessageRole::Tool.as_str(), "tool");
    }

    #[test]
    fn test_text_block_matches_spec() {
        let block = ContentBlock::text("Hello world");
        let json = serde_json::to_value(&block).unwrap();

        // Verify exact structure: {"type":"text","text":"Hello world"}
        assert_eq!(json["type"], "text");
        assert_eq!(json["text"], "Hello world");

        // Verify exactly 2 fields
        let obj = json.as_object().unwrap();
        assert_eq!(obj.len(), 2);
    }

    #[test]
    fn test_tool_use_block_matches_spec() {
        let block = ContentBlock::tool_use("call_123", "search", serde_json::json!({"query": "weather"}));
        let json = serde_json::to_value(&block).unwrap();

        // Verify exact structure
        assert_eq!(json["type"], "tool_use");
        assert_eq!(json["id"], "call_123");
        assert_eq!(json["name"], "search");
        assert_eq!(json["input"]["query"], "weather");

        // Verify exactly 4 fields
        let obj = json.as_object().unwrap();
        assert_eq!(obj.len(), 4);
    }

    #[test]
    fn test_tool_result_block_matches_spec() {
        let block = ContentBlock::tool_result("call_123", "Result text");
        let json = serde_json::to_value(&block).unwrap();

        // Verify exact structure
        assert_eq!(json["type"], "tool_result");
        assert_eq!(json["tool_use_id"], "call_123");
        assert_eq!(json["content"], "Result text");

        // Verify exactly 3 fields
        let obj = json.as_object().unwrap();
        assert_eq!(obj.len(), 3);
    }

    #[test]
    fn test_message_with_tool_call_id() {
        let msg = InternalMessage::tool_result("call_123", "search", "Weather is sunny");
        let json = serde_json::to_value(&msg).unwrap();

        // Verify tool_call_id and name are at top level
        assert_eq!(json["role"], "tool");
        assert_eq!(json["tool_call_id"], "call_123");
        assert_eq!(json["name"], "search");
        assert_eq!(json["content"], "Weather is sunny");
    }

    #[test]
    fn test_full_message_roundtrip() {
        let blocks = vec![
            ContentBlock::text("I'll search for you"),
            ContentBlock::tool_use("call_123", "search", serde_json::json!({"q": "test"})),
        ];

        let msg = InternalMessage {
            role: MessageRole::Assistant,
            content: MessageContent::Blocks(blocks),
            metadata: std::collections::HashMap::new(),
            tool_call_id: None,
            name: None,
        };

        let json = serde_json::to_string(&msg).unwrap();
        let deserialized: InternalMessage = serde_json::from_str(&json).unwrap();

        // Verify structure is preserved
        assert_eq!(deserialized.role, MessageRole::Assistant);
        if let MessageContent::Blocks(blocks) = deserialized.content {
            assert_eq!(blocks.len(), 2);
            assert!(matches!(blocks[0], ContentBlock::Text { .. }));
            assert!(matches!(blocks[1], ContentBlock::ToolUse { .. }));
        } else {
            panic!("Expected blocks content");
        }
    }

    #[test]
    fn test_spec_compliance_full_example() {
        // Recreate Example 4 from universal_message_format.md
        let blocks = vec![
            ContentBlock::text("I'll help you search"),
            ContentBlock::tool_use("call_abc123", "search", serde_json::json!({"query": "weather"})),
        ];

        let msg = InternalMessage {
            role: MessageRole::Assistant,
            content: MessageContent::Blocks(blocks),
            metadata: std::collections::HashMap::new(),
            tool_call_id: None,
            name: None,
        };

        let json = serde_json::to_value(&msg).unwrap();

        // Verify structure matches spec
        assert_eq!(json["role"], "assistant");

        let content = json["content"].as_array().unwrap();
        assert_eq!(content.len(), 2);

        // First block: text
        assert_eq!(content[0]["type"], "text");
        assert_eq!(content[0]["text"], "I'll help you search");

        // Second block: tool_use
        assert_eq!(content[1]["type"], "tool_use");
        assert_eq!(content[1]["id"], "call_abc123");
        assert_eq!(content[1]["name"], "search");
        assert_eq!(content[1]["input"]["query"], "weather");
    }

    #[test]
    fn test_wasm_provider_can_parse() {
        // Verify that serialized messages can be parsed as raw JSON with expected structure
        let msg = InternalMessage::tool_result("call_123", "search", "Result");
        let json_str = serde_json::to_string(&msg).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json_str).unwrap();

        // WASM provider expects these fields at top level
        assert_eq!(parsed["role"].as_str(), Some("tool"));
        assert_eq!(parsed["tool_call_id"].as_str(), Some("call_123"));
        assert_eq!(parsed["name"].as_str(), Some("search"));
        assert_eq!(parsed["content"].as_str(), Some("Result"));
    }
}

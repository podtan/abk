//! Native Rust OpenAI provider — SSE streaming parser.

use crate::provider::StreamChunk;

/// Parse a single SSE event payload (the `data: …` line content) into zero or more `StreamChunk`s.
///
/// Returns `None` if the event is a keep-alive comment or has no usable delta.
/// Returns `Some(Vec::new())` if the JSON parsed but had no relevant content.
pub fn parse_sse_event(data: &str) -> Option<Vec<StreamChunk>> {
    let trimmed = data.trim();

    // [DONE] sentinel
    if trimmed == "[DONE]" {
        return Some(vec![StreamChunk::Done]);
    }

    // Try to parse as JSON
    let json: serde_json::Value = serde_json::from_str(trimmed).ok()?;

    let delta = json.get("choices")
        .and_then(|c| c.get(0))
        .and_then(|c| c.get("delta"))?;

    let mut chunks = Vec::new();

    // Reasoning content (thinking models).
    // Providers drift on the key: `reasoning_content` (z.ai/DeepSeek/OpenRouter)
    // vs `reasoning` (vLLM reasoning-parser). Accept both (nghr 5fc21214).
    if let Some(reasoning) = delta
        .get("reasoning_content")
        .or_else(|| delta.get("reasoning"))
        .and_then(|v| v.as_str())
    {
        if !reasoning.is_empty() {
            chunks.push(StreamChunk::Reasoning(reasoning.to_string()));
        }
    }

    // Normal text content
    if let Some(content) = delta.get("content").and_then(|v| v.as_str()) {
        if !content.is_empty() {
            chunks.push(StreamChunk::Text(content.to_string()));
        }
    }

    // Tool call deltas
    if let Some(tool_calls) = delta.get("tool_calls").and_then(|v| v.as_array()) {
        for tc in tool_calls {
            let index = tc
                .get("index")
                .and_then(|v| v.as_u64())
                .unwrap_or(0) as usize;

            let function = tc.get("function").unwrap_or(&serde_json::Value::Null);

            let id = tc
                .get("id")
                .and_then(|v| v.as_str())
                .filter(|s| !s.is_empty())
                .map(|s| s.to_string());

            let name = function
                .get("name")
                .and_then(|v| v.as_str())
                .filter(|s| !s.is_empty())
                .map(|s| s.to_string());

            let arguments_delta = function
                .get("arguments")
                .and_then(|v| v.as_str())
                .filter(|s| !s.is_empty())
                .map(|s| s.to_string());

            chunks.push(StreamChunk::ToolCallDelta {
                index,
                id,
                name,
                arguments_delta,
            });
        }
    }

    // Check finish_reason for Done
    let finish_reason = json
        .get("choices")
        .and_then(|c| c.get(0))
        .and_then(|c| c.get("finish_reason"))
        .and_then(|v| v.as_str());

    if let Some(reason) = finish_reason {
        if !reason.is_empty() {
            chunks.push(StreamChunk::Done);
        }
    }

    if chunks.is_empty() {
        return Some(Vec::new());
    }

    Some(chunks)
}

#[cfg(test)]
mod tests {
    use super::*;

    // vLLM >=0.30 reasoning-parser shape (nghr 5fc21214): thinking rides `delta.reasoning`.
    #[test]
    fn test_sse_reasoning_key_vllm() {
        let data = r#"{"choices":[{"delta":{"reasoning":"checking the bat and ball puzzle"}}]}"#;
        let chunks = parse_sse_event(data).expect("event should parse");
        assert_eq!(chunks.len(), 1);
        assert!(matches!(&chunks[0], StreamChunk::Reasoning(t) if t == "checking the bat and ball puzzle"));
    }

    // z.ai/DeepSeek/OpenRouter shape: `delta.reasoning_content` (de-facto standard) — must not regress.
    #[test]
    fn test_sse_reasoning_content_key_zai() {
        let data = r#"{"choices":[{"delta":{"reasoning_content":"thinking"}}]}"#;
        let chunks = parse_sse_event(data).expect("event should parse");
        assert_eq!(chunks.len(), 1);
        assert!(matches!(&chunks[0], StreamChunk::Reasoning(t) if t == "thinking"));
    }

    // Precedence: `reasoning_content` wins when a provider serves both.
    #[test]
    fn test_sse_reasoning_content_takes_precedence() {
        let data = r#"{"choices":[{"delta":{"reasoning_content":"canonical","reasoning":"alias"}}]}"#;
        let chunks = parse_sse_event(data).expect("event should parse");
        assert_eq!(chunks.len(), 1);
        assert!(matches!(&chunks[0], StreamChunk::Reasoning(t) if t == "canonical"));
    }

    // Empty reasoning string stays silent (existing behavior preserved for both keys).
    #[test]
    fn test_sse_empty_reasoning_emits_nothing() {
        let data = r#"{"choices":[{"delta":{"reasoning":""}}]}"#;
        let chunks = parse_sse_event(data).expect("event should parse");
        assert!(chunks.is_empty());
    }
}

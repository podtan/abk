//! Native Rust OpenAI provider — response parsing.

use crate::provider::traits::{GenerateResponse, ToolInvocation};
use anyhow::{Context, Result};
use serde_json::Value;

/// Parse a non-streaming OpenAI chat-completions response into `GenerateResponse`.
pub fn parse_response(response_body: &str) -> Result<GenerateResponse> {
    let json: Value =
        serde_json::from_str(response_body).context("Failed to parse OpenAI response JSON")?;

    let choice = json
        .get("choices")
        .and_then(|c| c.get(0))
        .context("No choices in OpenAI response")?;

    let message = choice
        .get("message")
        .context("No message in first choice")?;

    // Check for reasoning content (thinking models).
    // No OpenAI-spec field exists; providers drift: z.ai/DeepSeek/OpenRouter serve
    // `reasoning_content` (de-facto standard), vLLM >=0.30 reasoning-parser serves
    // `reasoning`. Accept both — first key wins (nghr 5fc21214).
    let reasoning = message
        .get("reasoning_content")
        .or_else(|| message.get("reasoning"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    // Check for tool calls
    let tool_calls = message.get("tool_calls");

    if let Some(tool_calls) = tool_calls {
        if !tool_calls.is_null() {
            let calls: Vec<ToolInvocation> = tool_calls
                .as_array()
                .context("tool_calls is not an array")?
                .iter()
                .map(|tc| {
                    let id = tc
                        .get("id")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();
                    let function = tc.get("function").unwrap_or(&Value::Null);
                    let name = function
                        .get("name")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();
                    let args_str = function
                        .get("arguments")
                        .and_then(|v| v.as_str())
                        .unwrap_or("{}");
                    let arguments = serde_json::from_str(args_str).unwrap_or_default();

                    ToolInvocation {
                        id,
                        name,
                        arguments,
                        provider_metadata: std::collections::HashMap::new(),
                    }
                })
                .collect();

            if !calls.is_empty() {
                return Ok(GenerateResponse::ToolCalls { calls, reasoning });
            }
        }
    }

    // Normal content response
    let text = message
        .get("content")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    Ok(GenerateResponse::Content { text, reasoning })
}

#[cfg(test)]
mod tests {
    use super::*;

    // vLLM >=0.30 reasoning-parser shape (nghr 5fc21214): `message.reasoning`.
    #[test]
    fn test_response_reasoning_key_vllm() {
        let body = r#"{"choices":[{"message":{"content":"The ball costs $0.05","reasoning":"bat+ball = 1.10, so..."}}]}"#;
        let resp = parse_response(body).expect("should parse");
        match resp {
            GenerateResponse::Content { text, reasoning } => {
                assert_eq!(text, "The ball costs $0.05");
                assert_eq!(reasoning.as_deref(), Some("bat+ball = 1.10, so..."));
            }
            other => panic!("expected Content, got {:?}", other),
        }
    }

    // z.ai/DeepSeek/OpenRouter shape: `message.reasoning_content` — must not regress.
    #[test]
    fn test_response_reasoning_content_key_zai() {
        let body = r#"{"choices":[{"message":{"content":"ok","reasoning_content":"hmm"}}]}"#;
        let resp = parse_response(body).expect("should parse");
        match resp {
            GenerateResponse::Content { text, reasoning } => {
                assert_eq!(text, "ok");
                assert_eq!(reasoning.as_deref(), Some("hmm"));
            }
            other => panic!("expected Content, got {:?}", other),
        }
    }

    // Precedence: `reasoning_content` wins when a provider serves both.
    #[test]
    fn test_response_reasoning_content_takes_precedence() {
        let body = r#"{"choices":[{"message":{"content":"ok","reasoning_content":"canonical","reasoning":"alias"}}]}"#;
        let resp = parse_response(body).expect("should parse");
        match resp {
            GenerateResponse::Content { reasoning, .. } => {
                assert_eq!(reasoning.as_deref(), Some("canonical"));
            }
            other => panic!("expected Content, got {:?}", other),
        }
    }

    // Neither key present → None (plain models unaffected).
    #[test]
    fn test_response_no_reasoning() {
        let body = r#"{"choices":[{"message":{"content":"plain"}}]}"#;
        let resp = parse_response(body).expect("should parse");
        match resp {
            GenerateResponse::Content { reasoning, .. } => assert!(reasoning.is_none()),
            other => panic!("expected Content, got {:?}", other),
        }
    }
}

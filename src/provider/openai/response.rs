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

    // Check for reasoning content (thinking models)
    let reasoning = message
        .get("reasoning_content")
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

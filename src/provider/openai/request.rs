//! Native Rust OpenAI provider — request body builder.

use serde_json::{json, Value};

/// Build the OpenAI chat-completions request body.
pub fn build_request_body(
    messages: &[Value],
    model: &str,
    stream: bool,
    temperature: Option<f32>,
    max_tokens: Option<u32>,
    tools: Option<&[Value]>,
    tool_choice: Option<&Value>,
) -> Value {
    let mut body = json!({
        "model": model,
        "messages": messages,
        "stream": stream,
    });

    if let Some(temp) = temperature {
        body["temperature"] = json!(temp);
    }

    if let Some(max) = max_tokens {
        body["max_tokens"] = json!(max);
    }

    if let Some(tools) = tools {
        if !tools.is_empty() {
            body["tools"] = json!(tools);
        }
    }

    if let Some(tc) = tool_choice {
        body["tool_choice"] = tc.clone();
    }

    body
}

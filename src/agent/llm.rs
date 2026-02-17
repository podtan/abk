use anyhow::Result;
use regex::Regex;

use umf::{FunctionCall, ToolCall};

impl super::Agent {
    /// Parse task classification from LLM response.
    ///
    /// # Arguments
    /// * `response` - LLM response containing task classification.
    ///
    /// # Returns
    /// Task type string.
    pub fn parse_task_classification(&self, response: &str) -> Result<String> {
        // Look for TASK_CLASSIFICATION: pattern
        let classification_re = Regex::new(r"TASK_CLASSIFICATION:\s*(\w+)").unwrap();
        if let Some(caps) = classification_re.captures(response) {
            if let Some(task_type) = caps.get(1) {
                let classification = task_type.as_str().to_lowercase();
                // Validate classification is one of our known types
                match classification.as_str() {
                    "bug_fix" | "feature" | "maintenance" | "query" => return Ok(classification),
                    _ => {
                        self.logger.log_error(
                            &format!("Unknown task classification: {}", classification),
                            None,
                        )?;
                    }
                }
            }
        }

        // If no valid classification found, use fallback
        Ok("fallback".to_string())
    }

    /// Parse agent response for THOUGHT and completion status.
    ///
    /// # Arguments
    /// * `response` - Agent response text.
    ///
    /// # Returns
    /// Parse LLM response for thought and completion status.
    /// Returns tuple of (thought, is_completion). Command extraction removed as tools should be used.
    pub fn parse_response(&self, response: &str) -> (Option<String>, Option<String>, bool) {
        // Extract THOUGHT section
        let thought_regex = Regex::new(r"THOUGHT:\s*(.*?)(?:\n\n|\n```|$)").unwrap();
        let thought = thought_regex
            .captures(response)
            .and_then(|cap| cap.get(1))
            .map(|m| m.as_str().trim().to_string());

        // Check for completion markers
        let completion_markers = [
            "TASK_COMPLETED", // New primary completion marker (underscore)
            "TASK COMPLETED", // Alternative (with space)
            "IMPLEMENTATION COMPLETE",
            "SOLUTION VERIFIED",
            "ALL TESTS PASSING",
            "TASK FINISHED",
            "✓ COMPLETE",
            "COMPLETE_TASK_AND_SUBMIT_FINAL_OUTPUT", // Legacy marker from tests
        ];

        // Basic completion detection via known textual markers
        let mut is_completion = completion_markers
            .iter()
            .any(|marker| response.to_uppercase().contains(&marker.to_uppercase()));

        // Additionally, treat an explicit call to the `submit` tool as a completion signal.
        // This supports the new pattern where LLMs call the submit tool instead of echoing TASK_COMPLETED.
        if !is_completion {
            if let Ok(tool_calls) = self.extract_tool_calls(response) {
                if tool_calls
                    .iter()
                    .any(|tc| tc.function.name.to_lowercase() == "submit")
                {
                    is_completion = true;
                }
            }

            // Fallback: if extract_tool_calls failed to parse JSON tool call syntax,
            // do a simple case-insensitive regex check for '"name"\\s*:\\s*"submit"' in the raw response.
            if !is_completion {
                if let Ok(submit_re) = Regex::new(r#"(?i)"name"\s*:\s*"submit""#) {
                    if submit_re.is_match(response) {
                        is_completion = true;
                    }
                }
            }
        }

        // Command extraction removed - tools should be used exclusively
        (thought, None, is_completion)
    }

    pub fn extract_tool_calls(&self, response: &str) -> Result<Vec<ToolCall>> {
        // Try to parse tool calls from JSON format in the response
        // Use proper JSON parsing instead of regex to handle nested braces
        let mut tool_calls = Vec::new();

        // First, try to find JSON objects that look like tool calls
        let json_objects = self.extract_json_objects(response)?;

        for json_str in json_objects {
            if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&json_str) {
                if let Some(obj) = parsed.as_object() {
                    // Check if this looks like a tool call
                    if let (Some(name), Some(arguments)) = (obj.get("name"), obj.get("arguments")) {
                        if let Some(name_str) = name.as_str() {
                            let args_str = if arguments.is_object() {
                                serde_json::to_string(arguments)
                                    .unwrap_or_else(|_| "{}".to_string())
                            } else {
                                arguments.to_string()
                            };

                            tool_calls.push(ToolCall {
                                id: format!("call_{}", tool_calls.len()),
                                r#type: "function".to_string(),
                                function: FunctionCall {
                                    name: name_str.to_string(),
                                    arguments: args_str,
                                },
                            });
                        }
                    }
                }
            }
        }

        // If no JSON tool calls found, try to parse simple command syntax
        if tool_calls.is_empty() {
            for line in response.lines() {
                let trimmed = line.trim();

                // Handle simple command formats
                if trimmed == "scroll_down" {
                    tool_calls.push(ToolCall {
                        id: format!("call_{}", tool_calls.len()),
                        r#type: "function".to_string(),
                        function: FunctionCall {
                            name: "scroll_down".to_string(),
                            arguments: "{}".to_string(),
                        },
                    });
                    break;
                } else if trimmed == "scroll_up" {
                    tool_calls.push(ToolCall {
                        id: format!("call_{}", tool_calls.len()),
                        r#type: "function".to_string(),
                        function: FunctionCall {
                            name: "scroll_up".to_string(),
                            arguments: "{}".to_string(),
                        },
                    });
                    break;
                } else if let Ok(re) = Regex::new(r"^goto\s+(\d+)") {
                    if let Some(caps) = re.captures(trimmed) {
                        let line_num = caps.get(1).unwrap().as_str();
                        tool_calls.push(ToolCall {
                            id: format!("call_{}", tool_calls.len()),
                            r#type: "function".to_string(),
                            function: FunctionCall {
                                name: "goto".to_string(),
                                arguments: format!(r#"{{"line_number": {}}}"#, line_num),
                            },
                        });
                        break;
                    }
                }

                // Handle legacy syntax with parentheses
                if trimmed.contains("scroll_down()") {
                    tool_calls.push(ToolCall {
                        id: format!("call_{}", tool_calls.len()),
                        r#type: "function".to_string(),
                        function: FunctionCall {
                            name: "scroll_down".to_string(),
                            arguments: "{}".to_string(),
                        },
                    });
                    break;
                } else if trimmed.contains("scroll_up()") {
                    tool_calls.push(ToolCall {
                        id: format!("call_{}", tool_calls.len()),
                        r#type: "function".to_string(),
                        function: FunctionCall {
                            name: "scroll_up".to_string(),
                            arguments: "{}".to_string(),
                        },
                    });
                    break;
                } else if let Ok(re) = Regex::new(r"goto\s*\(\s*(\d+)\s*\)") {
                    if let Some(caps) = re.captures(trimmed) {
                        let line_num = caps.get(1).unwrap().as_str();
                        tool_calls.push(ToolCall {
                            id: format!("call_{}", tool_calls.len()),
                            r#type: "function".to_string(),
                            function: FunctionCall {
                                name: "goto".to_string(),
                                arguments: format!(r#"{{"line_number": {}}}"#, line_num),
                            },
                        });
                        break;
                    }
                }
            }
        }

        Ok(tool_calls)
    }

    /// Extract JSON objects from response text using proper brace counting
    fn extract_json_objects(&self, text: &str) -> Result<Vec<String>> {
        let mut json_objects = Vec::new();
        let chars: Vec<char> = text.chars().collect();
        let mut i = 0;

        while i < chars.len() {
            // Look for opening brace
            if chars[i] == '{' {
                let mut brace_count = 1;
                let mut in_string = false;
                let mut escaped = false;
                let start = i;
                i += 1;

                // Find the matching closing brace
                while i < chars.len() && brace_count > 0 {
                    let current_char = chars[i];

                    if escaped {
                        escaped = false;
                    } else if current_char == '\\' && in_string {
                        escaped = true;
                    } else if current_char == '"' {
                        in_string = !in_string;
                    } else if !in_string {
                        if current_char == '{' {
                            brace_count += 1;
                        } else if current_char == '}' {
                            brace_count -= 1;
                        }
                    }

                    i += 1;
                }

                // If we found a complete JSON object, extract it
                if brace_count == 0 {
                    let json_str: String = chars[start..i].iter().collect();

                    // Quick validation: check if it contains both "name" and "arguments" keys
                    if json_str.contains("\"name\"") && json_str.contains("\"arguments\"") {
                        json_objects.push(json_str);
                    }
                }
            } else {
                i += 1;
            }
        }

        Ok(json_objects)
    }
}

#[cfg(test)]
mod tests {
    use super::super::Agent;
    use crate::test_utils;
    use crate::config::ConfigurationLoader;

    #[tokio::test]
    async fn parse_task_classification_recognizes_known_types() {
        let _guard = test_utils::setup_env();
        let config = ConfigurationLoader::get_default_config();
        let agent = Agent::new_from_config(config, None).await.unwrap();

        let response = "TASK_CLASSIFICATION: bug_fix";
        assert_eq!(
            agent.parse_task_classification(response).unwrap(),
            "bug_fix"
        );

        let response = "TASK_CLASSIFICATION: unknown";
        assert_eq!(
            agent.parse_task_classification(response).unwrap(),
            "fallback"
        );
    }

    #[tokio::test]
    async fn parse_response_detects_submit_completion() {
        let _guard = test_utils::setup_env();
        let config = ConfigurationLoader::get_default_config();
        let agent = Agent::new_from_config(config, None).await.unwrap();

        let response = r#"THOUGHT: Task complete

{"name": "submit", "arguments": {}}"#;
        let (_thought, _command, is_completion) = agent.parse_response(response);
        assert!(is_completion);
    }

    #[tokio::test]
    async fn extract_json_objects_handles_multiple_objects() {
        let _guard = test_utils::setup_env();
        let config = ConfigurationLoader::get_default_config();
        let agent = Agent::new_from_config(config, None).await.unwrap();

        let response = r#"
        Some text
        {"name": "run_command", "arguments": {"command": "ls -la"}}
        more text
        {"name": "submit", "arguments": {}}
        "#;

        let objects = agent.extract_json_objects(response).unwrap();
        assert_eq!(objects.len(), 2);
        assert!(objects[0].contains("\"run_command\""));
        assert!(objects[1].contains("\"submit\""));
    }

    #[tokio::test]
    async fn extract_tool_calls_parses_json_tool_calls() {
        let _guard = test_utils::setup_env();
        let config = ConfigurationLoader::get_default_config();
        let agent = Agent::new_from_config(config, None).await.unwrap();

        let response = r#"{"name": "run_command", "arguments": {"command": "ls -la"}}
{"name": "submit", "arguments": {}}"#;
        let tool_calls = agent.extract_tool_calls(response).unwrap();

        assert_eq!(tool_calls.len(), 2);
        assert_eq!(tool_calls[0].function.name, "run_command");
        assert_eq!(tool_calls[1].function.name, "submit");
    }
}

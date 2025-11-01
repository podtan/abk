use super::*;
use std::collections::HashMap;
use tempfile::tempdir;

#[test]
fn test_logger_creation() {
    let temp_dir = tempdir().unwrap();
    let log_path = temp_dir.path().join("test.log");

    let logger = Logger::new(Some(&log_path), Some("DEBUG"));
    assert!(logger.is_ok());

    let logger = logger.unwrap();
    assert_eq!(logger.log_file(), &log_path);
    assert_eq!(logger.log_level(), "DEBUG");
}

#[test]
fn test_log_file_creation() {
    let temp_dir = tempdir().unwrap();
    let log_path = temp_dir.path().join("logs").join("test.md");

    let _logger = Logger::new(Some(&log_path), None).unwrap();
    assert!(log_path.exists());

    let content = std::fs::read_to_string(&log_path).unwrap();
    assert!(content.contains("# Agent Interaction Log"));
    assert!(content.contains("Log started:"));
}

#[test]
fn test_log_operations() {
    let temp_dir = tempdir().unwrap();
    let log_path = temp_dir.path().join("test.md");
    let logger = Logger::new(Some(&log_path), None).unwrap();

    // Test session start
    let mut config = HashMap::new();
    config.insert(
        "mode".to_string(),
        serde_json::Value::String("test".to_string()),
    );

    assert!(logger.log_session_start("confirm", &config).is_ok());

    // Test command execution
    assert!(logger
        .log_command_execution(
            "ls -la",
            "total 0\ndrwx 2 user user 4096 Jan 1 00:00 .\n",
            "",
            0,
            "confirm"
        )
        .is_ok());

    // Test completion
    assert!(logger.log_completion("Task completed successfully").is_ok());

    // Verify file contains expected content
    let content = std::fs::read_to_string(&log_path).unwrap();
    assert!(content.contains("Session Started"));
    assert!(content.contains("Command Execution"));
    assert!(content.contains("ls -la"));
    assert!(content.contains("Session Completed"));
}

#[test]
fn test_log_llm_interaction_debug_controls_messages() {
    let temp_dir = tempdir().unwrap();
    let log_path = temp_dir.path().join("test.md");
    let logger = Logger::new(Some(&log_path), None).unwrap();

    // Create sample messages
    let mut messages = Vec::new();
    let mut message = HashMap::new();
    message.insert(
        "role".to_string(),
        serde_json::Value::String("user".to_string()),
    );
    message.insert(
        "content".to_string(),
        serde_json::Value::String("Test message".to_string()),
    );
    messages.push(message);

    // Test with debug mode OFF (default)
    std::env::remove_var("RUST_LOG");
    assert!(logger
        .log_llm_interaction(&messages, "Test response", "gpt-4")
        .is_ok());

    // Check log content for non-debug mode
    let content = std::fs::read_to_string(&log_path).unwrap();
    assert!(content.contains("**Messages:** 1 messages")); // Should have summary
    assert!(!content.contains("```json")); // Should NOT have JSON block
    assert!(content.contains("Test response"));

    // Clear the log for next test
    std::fs::write(
        &log_path,
        "# Agent Interaction Log\n\nLog started: Test\n\n",
    )
    .unwrap();

    // Test with debug mode ON
    std::env::set_var("RUST_LOG", "debug");
    assert!(logger
        .log_llm_interaction(&messages, "Test response debug", "gpt-4")
        .is_ok());

    // Check log content for debug mode
    let content = std::fs::read_to_string(&log_path).unwrap();
    assert!(content.contains("```json")); // Should have JSON block in debug
    assert!(content.contains("\"role\": \"user\"")); // Should have actual JSON
    assert!(content.contains("Test response debug"));

    // Clean up
    std::env::remove_var("RUST_LOG");
}

#[test]
fn test_log_compact_tool_call() {
    let temp_dir = tempdir().unwrap();
    let log_path = temp_dir.path().join("test.md");
    let logger = Logger::new(Some(&log_path), None).unwrap();

    let tool_call_json = r#"{"name":"run_command","arguments":{"command":"docker ps"}}"#;

    assert!(logger.log_compact_tool_call(tool_call_json).is_ok());

    // Check log content
    let content = std::fs::read_to_string(&log_path).unwrap();
    assert!(content.contains("### Tool Request"));
    assert!(content.contains(
        r#"**Tool Request (compact):** {"name":"run_command","arguments":{"command":"docker ps"}}"#
    ));
}

use super::*;

#[tokio::test]
async fn test_simple_command() {
    let mut executor = CommandExecutor::default();
    let result = executor
        .execute_command("echo 'Hello World'", None)
        .await
        .unwrap();

    assert!(result.success);
    assert_eq!(result.return_code, 0);
    assert!(result.stdout.contains("Hello World"));
    assert!(result.stderr.is_empty());
}

#[test]
fn test_command_validation() {
    let executor = CommandExecutor::default();

    // Valid command
    let (valid, _) = executor.validate_command("ls -la");
    assert!(valid);

    // Empty command
    let (valid, msg) = executor.validate_command("");
    assert!(!valid);
    assert!(msg.contains("empty"));

    // Dangerous command
    let (valid, msg) = executor.validate_command("rm -rf /");
    assert!(!valid);
    assert!(msg.contains("dangerous"));
}

#[tokio::test]
async fn test_command_timeout() {
    let mut executor = CommandExecutor::new(1, None, true); // 1 second timeout
    let result = executor.execute_command("sleep 2", None).await;

    // Should timeout
    assert!(result.is_err());
}

#[tokio::test]
async fn test_working_directory() {
    let mut executor = CommandExecutor::default();
    let result = executor.execute_command("pwd", None).await.unwrap();

    assert!(result.success);
    // The output should contain some path
    assert!(!result.stdout.trim().is_empty());
}

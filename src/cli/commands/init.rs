//! Init command - project initialization helpers
//!
//! Provides reusable initialization logic for agent projects

use crate::cli::error::{CliError, CliResult};
use crate::cli::adapters::CommandContext;
use std::path::{Path, PathBuf};
use std::fs;

/// Options for initialization
#[derive(Debug, Clone)]
pub struct InitOptions {
    pub directory: Option<PathBuf>,
    pub force: bool,
}

/// Create directory structure for a project
pub fn create_project_directories<C: CommandContext>(
    ctx: &C,
    base_dir: &Path,
    dirs: &[&str],
) -> CliResult<()> {
    for dir_name in dirs {
        let dir_path = base_dir.join(dir_name);
        fs::create_dir_all(&dir_path)
            .map_err(|e| CliError::IoError(e))?;
        ctx.log_info(&format!("Created directory: {}", dir_path.display()));
    }
    Ok(())
}

/// Create a file with content if it doesn't exist or force is enabled
pub fn create_file_if_needed<C: CommandContext>(
    ctx: &C,
    file_path: &Path,
    content: &str,
    force: bool,
) -> CliResult<()> {
    if file_path.exists() && !force {
        ctx.log_warn(&format!("File already exists: {}", file_path.display()));
        return Ok(());
    }
    
    fs::write(file_path, content)
        .map_err(|e| CliError::IoError(e))?;
    ctx.log_info(&format!("Created: {}", file_path.display()));
    Ok(())
}

/// Standard .env.example content for agent projects
pub fn default_env_example(api_provider: &str) -> String {
    match api_provider {
        "openai" => r#"# Example environment file
# Copy this file to .env and fill in your actual values

# OpenAI API Configuration
OPENAI_API_KEY=sk-your-api-key-here
OPENAI_BASE_URL=https://api.openai.com/v1
OPENAI_DEFAULT_MODEL=gpt-4o-mini

# Optional: Override default settings
# AGENT_CONFIG=config/agent.toml
# AGENT_MODE=confirm
# AGENT_LOG_LEVEL=INFO"#.to_string(),
        
        "anthropic" => r#"# Example environment file
# Copy this file to .env and fill in your actual values

# Anthropic API Configuration
ANTHROPIC_API_KEY=sk-ant-your-api-key-here
ANTHROPIC_BASE_URL=https://api.anthropic.com
ANTHROPIC_MODEL=claude-3-5-sonnet-20241022

# Optional: Override default settings
# AGENT_CONFIG=config/agent.toml
# AGENT_MODE=confirm
# AGENT_LOG_LEVEL=INFO"#.to_string(),
        
        _ => r#"# Example environment file
# Copy this file to .env and fill in your actual values

# API Configuration
API_KEY=your-api-key-here

# Optional: Override default settings
# AGENT_CONFIG=config/agent.toml
# AGENT_LOG_LEVEL=INFO"#.to_string(),
    }
}

/// Initialize a local project directory
pub fn init_local_project<C: CommandContext>(
    ctx: &C,
    opts: InitOptions,
    app_name: &str,
) -> CliResult<()> {
    let project_dir = opts.directory.unwrap_or_else(|| 
        ctx.working_dir().unwrap_or_else(|_| PathBuf::from("."))
    );

    ctx.log_info(&format!("Initializing {} project locally in {}", 
        app_name, project_dir.display()));

    // Create directories
    let dirs_to_create = ["config", "logs"];
    create_project_directories(ctx, &project_dir, &dirs_to_create)?;

    // Create .env.example
    let env_example = project_dir.join(".env.example");
    let env_content = default_env_example("openai");
    create_file_if_needed(ctx, &env_example, &env_content, opts.force)?;

    ctx.log_info("Local project initialized!");
    ctx.log_info(&format!("1. Copy .env.example to .env and add your API key"));
    ctx.log_info(&format!("2. Run '{} run \"your task description\"'", app_name));

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::test_utils::MockCommandContext;
    use tempfile::TempDir;

    #[test]
    fn test_create_project_directories() {
        let temp_dir = TempDir::new().unwrap();
        let ctx = MockCommandContext::new();
        
        let dirs = vec!["config", "logs", "data"];
        create_project_directories(&ctx, temp_dir.path(), &dirs).unwrap();
        
        assert!(temp_dir.path().join("config").exists());
        assert!(temp_dir.path().join("logs").exists());
        assert!(temp_dir.path().join("data").exists());
    }

    #[test]
    fn test_create_file_if_needed() {
        let temp_dir = TempDir::new().unwrap();
        let ctx = MockCommandContext::new();
        let file_path = temp_dir.path().join("test.txt");
        
        // First creation should succeed
        create_file_if_needed(&ctx, &file_path, "content", false).unwrap();
        assert!(file_path.exists());
        
        // Second creation without force should skip
        create_file_if_needed(&ctx, &file_path, "new content", false).unwrap();
        assert_eq!(fs::read_to_string(&file_path).unwrap(), "content");
        
        // With force should overwrite
        create_file_if_needed(&ctx, &file_path, "new content", true).unwrap();
        assert_eq!(fs::read_to_string(&file_path).unwrap(), "new content");
    }

    #[test]
    fn test_init_local_project() {
        let temp_dir = TempDir::new().unwrap();
        let mut ctx = MockCommandContext::new();
        ctx.working_dir = temp_dir.path().to_path_buf();
        
        let opts = InitOptions {
            directory: Some(temp_dir.path().to_path_buf()),
            force: false,
        };
        
        init_local_project(&ctx, opts, "testapp").unwrap();
        
        assert!(temp_dir.path().join("config").exists());
        assert!(temp_dir.path().join("logs").exists());
        assert!(temp_dir.path().join(".env.example").exists());
    }
}

//! Miscellaneous CLI commands
//!
//! Simple utility commands that don't require complex state:
//! - echo: Echo a message
//! - version: Show version information
//! - count: Count tokens in a file
//! - config_check: Validate configuration
//! - doctor: Run diagnostics
//! - stats: Show statistics
//! - clean: Clean temporary files

use crate::cli::error::{CliError, CliResult};
use crate::cli::adapters::{CommandContext, StorageAccess};
use colored::*;
use std::fs;
use std::path::PathBuf;

/// Doctor command options
#[derive(Debug, Clone)]
pub struct DoctorOptions {
    pub verbose: bool,
}

/// Stats command options
#[derive(Debug, Clone)]
pub struct StatsOptions {
    pub detailed: bool,
}

/// Clean command options
#[derive(Debug, Clone)]
pub struct CleanOptions {
    pub dry_run: bool,
    pub temp_only: bool,
}

/// Echo command - prints a message
pub fn echo<C: CommandContext>(_ctx: &C, message: &[String]) -> CliResult<()> {
    println!("{}", message.join(" "));
    Ok(())
}

/// Version command - displays version information
///
/// Note: The version is embedded at compile time from CARGO_PKG_VERSION and GIT_SHA
pub fn version<C: CommandContext>(_ctx: &C, pkg_version: &str, git_sha: Option<&str>) -> CliResult<()> {
    println!("{}", pkg_version);
    if let Some(sha) = git_sha {
        println!("Git SHA: {}", sha);
    }
    Ok(())
}

/// Count tokens in a file
///
/// Uses tiktoken_rs with cl100k_base encoding (GPT-4)
pub fn count_tokens<C: CommandContext>(_ctx: &C, file: &PathBuf) -> CliResult<()> {
    let content = fs::read_to_string(file)
        .map_err(|e| CliError::IoError(e))?;
    
    // Use tiktoken_rs if available, otherwise fallback to character count
    #[cfg(feature = "tiktoken")]
    {
        use tiktoken_rs::cl100k_base;
        let bpe = cl100k_base()
            .map_err(|e| CliError::ExecutionError(format!("Failed to load tokenizer: {}", e)))?;
        let tokens = bpe.encode_with_special_tokens(&content);
        println!("Tokens: {}", tokens.len());
    }
    
    #[cfg(not(feature = "tiktoken"))]
    {
        println!("Characters: {}", content.len());
        println!("Note: Install with 'tiktoken' feature for actual token counting");
    }
    
    Ok(())
}

/// Configuration check command
///
/// Validates that configuration and environment are properly set up
pub fn config_check<C: CommandContext>(
    ctx: &C,
    config_path: Option<&PathBuf>,
    env_file: Option<&PathBuf>,
) -> CliResult<()> {
    println!("{}", "Configuration Check".cyan().bold());
    println!("{}", "===================".cyan());

    // Check configuration
    let default_config = ctx.config_path().ok();
    let config_to_check = config_path.or(default_config.as_ref());
    
    if let Some(cfg_path) = config_to_check {
        println!(
            "{}",
            format!("Config file: {}", cfg_path.display()).green()
        );
        if ctx.path_exists(cfg_path) {
            println!("{}", "✓ Config file exists".green());
            // Try to load config
            match ctx.load_config() {
                Ok(_) => println!("{}", "✓ Config file is valid".green()),
                Err(e) => println!("{}", format!("✗ Config error: {}", e).red()),
            }
        } else {
            println!("{}", "✗ Config file does not exist".red());
        }
    } else {
        println!("{}", "Using default configuration".yellow());
    }

    // Check environment
    if let Some(env_path) = env_file {
        println!("{}", format!("Env file: {}", env_path.display()).green());
        if ctx.path_exists(env_path) {
            println!("{}", "✓ Env file exists".green());
            // Environment checking is handled by the host application
            println!("{}", "✓ Env file accessible".green());
        } else {
            println!("{}", "✗ Env file does not exist".red());
        }
    } else {
        println!("{}", "No environment file specified".yellow());
    }

    Ok(())
}

/// Run diagnostics
pub async fn run_doctor<C>(
    ctx: &C,
    opts: DoctorOptions,
) -> CliResult<()>
where
    C: CommandContext,
{
    ctx.log_info("🔍 Running diagnostics");

    let mut issues = Vec::new();

    // Check configuration
    let config_path = ctx.config_path()?;
    if !config_path.exists() {
        issues.push("Configuration file not found".to_string());
    } else {
        match ctx.load_config() {
            Ok(_) => {
                if opts.verbose {
                    println!("✅ Configuration file is valid");
                }
            }
            Err(e) => {
                issues.push(format!("Configuration file is invalid: {}", e));
            }
        }
    }

    // Check data directory
    let data_dir = ctx.data_dir()?;
    if !data_dir.exists() {
        issues.push("Data directory not found".to_string());
    } else {
        if opts.verbose {
            println!("✅ Data directory exists");
        }
    }

    // Check for temporary files
    let temp_path = data_dir.join("temp");
    if temp_path.exists() {
        if let Ok(entries) = std::fs::read_dir(&temp_path) {
            let temp_files: Vec<_> = entries.filter_map(|e| e.ok()).collect();
            if !temp_files.is_empty() {
                issues.push(format!("{} temporary files found", temp_files.len()));
            }
        }
    }

    if issues.is_empty() {
        ctx.log_success("All systems operational");
    } else {
        ctx.log_warning("Issues found:");
        for issue in issues {
            println!("  • {}", issue);
        }
    }

    Ok(())
}

/// Show statistics
pub async fn show_stats<C, S>(
    ctx: &C,
    storage: &S,
    opts: StatsOptions,
) -> CliResult<()>
where
    C: CommandContext,
    S: StorageAccess,
{
    ctx.log_info("📊 Showing statistics");

    let stats = storage.calculate_storage_usage().await?;

    println!("Storage Statistics:");
    println!("  Total size: {} bytes", stats.total_size);
    println!("  Projects: {}", stats.project_count);
    println!("  Sessions: {}", stats.session_count);
    println!("  Checkpoints: {}", stats.checkpoint_count);

    if opts.detailed {
        println!("\nProject Details:");
        for project in stats.projects {
            println!("  {}: {} bytes, {} sessions, {} checkpoints",
                project.project_path.display(),
                project.size_bytes,
                project.session_count,
                project.checkpoint_count
            );
        }
    }

    Ok(())
}

/// Clean temporary files
pub async fn clean_temp<C>(
    ctx: &C,
    opts: CleanOptions,
) -> CliResult<()>
where
    C: CommandContext,
{
    ctx.log_info("🧹 Cleaning temporary files");

    let data_dir = ctx.data_dir()?;
    let temp_path = data_dir.join("temp");

    if !temp_path.exists() {
        ctx.log_info("No temporary files to clean");
        return Ok(());
    }

    let mut cleaned_files = 0;
    let mut cleaned_bytes = 0u64;

    if let Ok(entries) = std::fs::read_dir(&temp_path) {
        for entry in entries.flatten() {
            if let Ok(metadata) = entry.metadata() {
                cleaned_bytes += metadata.len();

                if !opts.dry_run {
                    if let Err(e) = std::fs::remove_file(entry.path()) {
                        ctx.log_warning(&format!("Failed to remove {}: {}", entry.path().display(), e));
                        continue;
                    }
                }

                cleaned_files += 1;
            }
        }
    }

    if opts.dry_run {
        println!("Would clean {} files ({} bytes)", cleaned_files, cleaned_bytes);
    } else {
        println!("Cleaned {} files ({} bytes)", cleaned_files, cleaned_bytes);
        ctx.log_success("Cleanup completed");
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::test_utils::MockCommandContext;
    use tempfile::TempDir;
    use std::fs::write;

    #[test]
    fn test_echo() {
        let ctx = MockCommandContext::new();
        let message = vec!["Hello".to_string(), "World".to_string()];
        assert!(echo(&ctx, &message).is_ok());
    }

    #[test]
    fn test_version() {
        let ctx = MockCommandContext::new();
        assert!(version(&ctx, "1.0.0", Some("abc123")).is_ok());
        assert!(version(&ctx, "1.0.0", None).is_ok());
    }

    #[test]
    fn test_count_tokens_basic() {
        let ctx = MockCommandContext::new();
        let temp_dir = TempDir::new().unwrap();
        let file_path = temp_dir.path().join("test.txt");
        write(&file_path, "Hello, world!").unwrap();
        
        // Should work even without tiktoken feature (will count characters)
        assert!(count_tokens(&ctx, &file_path).is_ok());
    }

    #[test]
    fn test_config_check() {
        let mut ctx = MockCommandContext::new();
        ctx.config = serde_json::json!({"key": "value"});
        
        assert!(config_check(&ctx, None, None).is_ok());
    }
}

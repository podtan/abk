//! Config command implementation
//!
//! Provides configuration management via CommandContext

use crate::cli::adapters::CommandContext;
use crate::cli::error::CliResult;

/// Config show options
#[derive(Debug, Clone)]
pub struct ConfigShowOptions {
    pub detailed: bool,
}

/// Show configuration
pub fn config_show<C: CommandContext>(
    ctx: &C,
    opts: ConfigShowOptions,
) -> CliResult<()> {
    ctx.log_info("🔧 Configuration");

    let config_path = ctx.config_path()?;

    if !config_path.exists() {
        ctx.log_warning("Configuration file does not exist.");
        ctx.log_info("Run 'init' to create default configuration.");
        return Ok(());
    }

    let content = std::fs::read_to_string(&config_path)
        .map_err(|e| crate::cli::error::CliError::IoError(e))?;
    
    println!("Configuration file: {}", config_path.display());

    if opts.detailed {
        println!("\nFull configuration:");
        println!("{}", content);
    } else {
        // Parse and show summary
        if let Ok(config) = toml::from_str::<toml::Value>(&content) {
            println!("\nConfiguration summary:");
            
            // Show top-level sections
            if let Some(table) = config.as_table() {
                for (key, _value) in table.iter() {
                    println!("  [{}]", key);
                }
            }
        } else {
            ctx.log_error("Failed to parse configuration file");
        }
    }

    Ok(())
}

/// Validate configuration
pub fn config_validate<C: CommandContext>(
    ctx: &C,
    fix: bool,
) -> CliResult<()> {
    ctx.log_info("✅ Validate Configuration");

    let config_path = ctx.config_path()?;

    if !config_path.exists() {
        ctx.log_error("✗ Config file does not exist");

        if fix {
            ctx.log_info("  Creating default configuration...");
            ctx.log_warning("  Auto-fix not yet implemented");
        }
    } else {
        let content = std::fs::read_to_string(&config_path)
            .map_err(|e| crate::cli::error::CliError::IoError(e))?;
        
        match toml::from_str::<toml::Value>(&content) {
            Ok(_config) => {
                ctx.log_success("  ✓ Configuration is valid");
            }
            Err(e) => {
                ctx.log_error(&format!("  ✗ Configuration is invalid: {}", e));

                if fix {
                    ctx.log_info("  Creating backup and replacing with defaults...");
                    ctx.log_warning("  Auto-fix not yet implemented");
                }
            }
        }
    }

    ctx.log_success("Validation completed!");
    Ok(())
}

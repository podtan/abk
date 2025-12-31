//! Extension management commands
//!
//! Commands for managing ABK extensions:
//! - `extension list` - List installed extensions
//! - `extension install <path>` - Install an extension
//! - `extension remove <id>` - Remove an extension
//! - `extension info <id>` - Show extension details

use crate::cli::adapters::CommandContext;
use crate::cli::error::{CliError, CliResult};

/// List installed extensions
pub async fn list<C: CommandContext>(ctx: &C) -> CliResult<()> {
    let extensions_dir = get_extensions_dir(ctx)?;
    
    ctx.log_info(&format!("Extensions directory: {}", extensions_dir.display()));
    
    if !extensions_dir.exists() {
        ctx.log_info("No extensions installed.");
        return Ok(());
    }
    
    // Import extension manager when feature is available
    #[cfg(feature = "extension")]
    {
        use crate::extension::ExtensionManager;
        
        let mut manager = ExtensionManager::new(&extensions_dir).await
            .map_err(|e| CliError::ExtensionError(format!("Failed to create extension manager: {}", e)))?;
        
        let manifests = manager.discover().await
            .map_err(|e| CliError::ExtensionError(format!("Failed to discover extensions: {}", e)))?;
        
        if manifests.is_empty() {
            ctx.log_info("No extensions installed.");
            return Ok(());
        }
        
        ctx.log_info(&format!("Found {} extension(s):\n", manifests.len()));
        
        for manifest in manifests {
            let caps = manifest.list_capabilities().join(", ");
            println!("  {} v{}", manifest.extension.id, manifest.extension.version);
            println!("    Name: {}", manifest.extension.name);
            println!("    Description: {}", manifest.extension.description);
            println!("    Capabilities: [{}]", caps);
            println!();
        }
    }
    
    #[cfg(not(feature = "extension"))]
    {
        ctx.log_warn("Extension feature not enabled. Rebuild ABK with --features extension");
    }
    
    Ok(())
}

/// Install an extension from a directory
pub async fn install<C: CommandContext>(ctx: &C, source_path: &str, extension_id: Option<&str>) -> CliResult<()> {
    let source = std::path::PathBuf::from(source_path);
    
    if !source.exists() {
        return Err(CliError::ExtensionError(format!("Source path does not exist: {}", source_path)));
    }
    
    // Check for extension.toml
    let manifest_path = if source.is_dir() {
        source.join("extension.toml")
    } else if source.file_name().map(|n| n == "extension.toml").unwrap_or(false) {
        source.clone()
    } else {
        return Err(CliError::ExtensionError(
            "Source must be a directory containing extension.toml or the extension.toml file itself".to_string()
        ));
    };
    
    if !manifest_path.exists() {
        return Err(CliError::ExtensionError(format!(
            "extension.toml not found at {}",
            manifest_path.display()
        )));
    }
    
    // Parse manifest to get extension ID
    #[cfg(feature = "extension")]
    let ext_id = {
        use crate::extension::ExtensionManifest;
        let manifest = ExtensionManifest::from_file(&manifest_path)
            .map_err(|e| CliError::ExtensionError(format!("Failed to parse manifest: {}", e)))?;
        manifest.extension.id
    };
    
    #[cfg(not(feature = "extension"))]
    let ext_id = extension_id.map(|s| s.to_string()).unwrap_or_else(|| {
        source.file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("unknown")
            .to_string()
    });
    
    let extensions_dir = get_extensions_dir(ctx)?;
    let target_dir = extensions_dir.join(&ext_id);
    
    // Create extensions directory if needed
    std::fs::create_dir_all(&extensions_dir)
        .map_err(|e| CliError::ExtensionError(format!("Failed to create extensions directory: {}", e)))?;
    
    // Check if already exists
    if target_dir.exists() {
        ctx.log_warn(&format!("Extension '{}' already installed. Removing old version...", ext_id));
        std::fs::remove_dir_all(&target_dir)
            .map_err(|e| CliError::ExtensionError(format!("Failed to remove old extension: {}", e)))?;
    }
    
    // Copy extension files
    let source_dir = manifest_path.parent().unwrap_or(&source);
    copy_dir_recursive(source_dir, &target_dir)?;
    
    ctx.log_success(&format!("Extension '{}' installed to {}", ext_id, target_dir.display()));
    
    Ok(())
}

/// Remove an installed extension
pub async fn remove<C: CommandContext>(ctx: &C, extension_id: &str) -> CliResult<()> {
    let extensions_dir = get_extensions_dir(ctx)?;
    let target_dir = extensions_dir.join(extension_id);
    
    if !target_dir.exists() {
        return Err(CliError::ExtensionError(format!(
            "Extension '{}' not found at {}",
            extension_id,
            target_dir.display()
        )));
    }
    
    std::fs::remove_dir_all(&target_dir)
        .map_err(|e| CliError::ExtensionError(format!("Failed to remove extension: {}", e)))?;
    
    ctx.log_success(&format!("Extension '{}' removed", extension_id));
    
    Ok(())
}

/// Show details about a specific extension
pub async fn info<C: CommandContext>(ctx: &C, extension_id: &str) -> CliResult<()> {
    let extensions_dir = get_extensions_dir(ctx)?;
    let ext_dir = extensions_dir.join(extension_id);
    
    if !ext_dir.exists() {
        return Err(CliError::ExtensionError(format!(
            "Extension '{}' not found",
            extension_id
        )));
    }
    
    #[cfg(feature = "extension")]
    {
        use crate::extension::ExtensionManifest;
        
        let manifest_path = ext_dir.join("extension.toml");
        let manifest = ExtensionManifest::from_file(&manifest_path)
            .map_err(|e| CliError::ExtensionError(format!("Failed to parse manifest: {}", e)))?;
        
        println!("Extension: {}", manifest.extension.id);
        println!("Name: {}", manifest.extension.name);
        println!("Version: {}", manifest.extension.version);
        println!("API Version: {}", manifest.extension.api_version);
        println!("Description: {}", manifest.extension.description);
        
        if !manifest.extension.authors.is_empty() {
            println!("Authors: {}", manifest.extension.authors.join(", "));
        }
        
        if let Some(repo) = &manifest.extension.repository {
            println!("Repository: {}", repo);
        }
        
        println!("\nLibrary:");
        println!("  Kind: {}", manifest.lib.kind);
        println!("  Path: {}", manifest.lib.path);
        
        let caps = manifest.list_capabilities();
        println!("\nCapabilities: [{}]", caps.join(", "));
        
        // Show WASM file info if exists
        let wasm_path = ext_dir.join(&manifest.lib.path);
        if wasm_path.exists() {
            if let Ok(metadata) = std::fs::metadata(&wasm_path) {
                let size_kb = metadata.len() / 1024;
                println!("\nWASM Binary: {} ({} KB)", wasm_path.display(), size_kb);
            }
        } else {
            ctx.log_warn(&format!("WASM binary not found: {}", wasm_path.display()));
        }
    }
    
    #[cfg(not(feature = "extension"))]
    {
        ctx.log_warn("Extension feature not enabled. Rebuild ABK with --features extension");
    }
    
    Ok(())
}

/// Get the extensions directory for the current agent
fn get_extensions_dir<C: CommandContext>(ctx: &C) -> CliResult<std::path::PathBuf> {
    let data_dir = ctx.data_dir()?;
    Ok(data_dir.join("extensions"))
}

/// Recursively copy a directory
fn copy_dir_recursive(src: &std::path::Path, dst: &std::path::Path) -> CliResult<()> {
    std::fs::create_dir_all(dst)
        .map_err(|e| CliError::ExtensionError(format!("Failed to create directory {}: {}", dst.display(), e)))?;
    
    for entry in std::fs::read_dir(src)
        .map_err(|e| CliError::ExtensionError(format!("Failed to read directory {}: {}", src.display(), e)))?
    {
        let entry = entry
            .map_err(|e| CliError::ExtensionError(format!("Failed to read entry: {}", e)))?;
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());
        
        if src_path.is_dir() {
            copy_dir_recursive(&src_path, &dst_path)?;
        } else {
            std::fs::copy(&src_path, &dst_path)
                .map_err(|e| CliError::ExtensionError(format!(
                    "Failed to copy {} to {}: {}",
                    src_path.display(),
                    dst_path.display(),
                    e
                )))?;
        }
    }
    
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;
    
    // Mock context for testing
    struct MockContext {
        data_dir: std::path::PathBuf,
    }
    
    impl MockContext {
        fn new(temp_dir: &TempDir) -> Self {
            Self {
                data_dir: temp_dir.path().to_path_buf(),
            }
        }
    }
    
    #[async_trait::async_trait]
    impl CommandContext for MockContext {
        fn config_path(&self) -> CliResult<std::path::PathBuf> {
            Ok(self.data_dir.join("config.toml"))
        }
        
        fn config(&self) -> &crate::config::Configuration {
            unimplemented!()
        }
        
        fn load_config(&self) -> CliResult<serde_json::Value> {
            Ok(serde_json::json!({}))
        }
        
        fn working_dir(&self) -> CliResult<std::path::PathBuf> {
            Ok(self.data_dir.clone())
        }
        
        fn project_hash(&self) -> CliResult<String> {
            Ok("test".to_string())
        }
        
        fn data_dir(&self) -> CliResult<std::path::PathBuf> {
            Ok(self.data_dir.clone())
        }
        
        fn cache_dir(&self) -> CliResult<std::path::PathBuf> {
            Ok(self.data_dir.clone())
        }
        
        fn log_info(&self, message: &str) {
            println!("{}", message);
        }
        
        fn log_warn(&self, message: &str) {
            eprintln!("Warning: {}", message);
        }
        
        fn log_error(&self, message: &str) -> CliResult<()> {
            eprintln!("Error: {}", message);
            Ok(())
        }
        
        fn log_success(&self, message: &str) {
            println!("✓ {}", message);
        }
        
        async fn create_agent(&self) -> Result<crate::agent::Agent, Box<dyn std::error::Error + Send + Sync>> {
            unimplemented!()
        }
    }
    
    #[test]
    fn test_get_extensions_dir() {
        let temp_dir = TempDir::new().unwrap();
        let ctx = MockContext::new(&temp_dir);
        let ext_dir = get_extensions_dir(&ctx).unwrap();
        assert!(ext_dir.ends_with("extensions"));
    }
    
    #[test]
    fn test_copy_dir_recursive() {
        let temp_dir = TempDir::new().unwrap();
        let src_dir = temp_dir.path().join("src");
        let dst_dir = temp_dir.path().join("dst");
        
        // Create source structure
        std::fs::create_dir_all(&src_dir).unwrap();
        std::fs::write(src_dir.join("file1.txt"), "content1").unwrap();
        std::fs::create_dir(src_dir.join("subdir")).unwrap();
        std::fs::write(src_dir.join("subdir/file2.txt"), "content2").unwrap();
        
        // Copy
        copy_dir_recursive(&src_dir, &dst_dir).unwrap();
        
        // Verify
        assert!(dst_dir.join("file1.txt").exists());
        assert!(dst_dir.join("subdir/file2.txt").exists());
        assert_eq!(
            std::fs::read_to_string(dst_dir.join("file1.txt")).unwrap(),
            "content1"
        );
    }
}

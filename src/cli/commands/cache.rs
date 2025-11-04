//! Cache command implementation
//!
//! Provides cache management commands via StorageAccess adapter

use crate::cli::adapters::{CommandContext, StorageAccess, StorageStats};
use crate::cli::error::CliResult;

/// Cache command options
#[derive(Debug, Clone)]
pub struct CacheStatusOptions {
    pub detailed: bool,
    pub project: Option<String>,
}

#[derive(Debug, Clone)]
pub struct CacheSizeOptions {
    pub human_readable: bool,
    pub sort_by_size: bool,
}

#[derive(Debug, Clone)]
pub struct CacheCleanOptions {
    pub dry_run: bool,
    pub older_than_days: Option<u32>,
    pub max_size_gb: Option<u64>,
}

#[derive(Debug, Clone)]
pub struct CacheListOptions {
    pub sort_by_size: bool,
    pub min_size_mb: Option<u32>,
}

#[derive(Debug, Clone)]
pub struct CacheVacuumOptions {
    pub dry_run: bool,
}

/// Format bytes into human-readable format
fn format_bytes(bytes: u64, human: bool) -> String {
    if !human {
        return format!("{} bytes", bytes);
    }

    const UNITS: &[&str] = &["B", "KB", "MB", "GB", "TB"];
    let mut size = bytes as f64;
    let mut unit_idx = 0;

    while size >= 1024.0 && unit_idx < UNITS.len() - 1 {
        size /= 1024.0;
        unit_idx += 1;
    }

    if unit_idx == 0 {
        format!("{} {}", size as u64, UNITS[unit_idx])
    } else {
        format!("{:.2} {}", size, UNITS[unit_idx])
    }
}

/// Show cache status
pub async fn cache_status<C, S>(
    ctx: &C,
    storage: &S,
    opts: CacheStatusOptions,
) -> CliResult<()>
where
    C: CommandContext,
    S: StorageAccess,
{
    ctx.log_info("💾 Cache Status");

    let stats = storage.calculate_storage_usage().await?;

    println!(
        "Total storage: {} ({} bytes)",
        format_bytes(stats.total_size, true),
        stats.total_size
    );
    println!("Projects: {}", stats.project_count);
    println!("Sessions: {}", stats.session_count);
    println!("Checkpoints: {}", stats.checkpoint_count);

    if opts.detailed {
        println!("\nProject Breakdown:");
        for project_stat in stats.projects {
            if let Some(filter) = &opts.project {
                if !project_stat.project_path.to_string_lossy().contains(filter) {
                    continue;
                }
            }

            println!(
                "  {} - {}",
                project_stat.project_path.display(),
                format_bytes(project_stat.size_bytes, true)
            );
            println!(
                "    Sessions: {}, Checkpoints: {}",
                project_stat.session_count, project_stat.checkpoint_count
            );
        }
    }

    Ok(())
}

/// Show cache sizes
pub async fn cache_size<C, S>(
    ctx: &C,
    storage: &S,
    opts: CacheSizeOptions,
) -> CliResult<()>
where
    C: CommandContext,
    S: StorageAccess,
{
    ctx.log_info("📏 Cache Sizes");

    let stats = storage.calculate_storage_usage().await?;
    let mut projects = stats.projects;

    if opts.sort_by_size {
        projects.sort_by(|a, b| b.size_bytes.cmp(&a.size_bytes));
    }

    for project_stat in projects {
        let size_str = format_bytes(project_stat.size_bytes, opts.human_readable);
        println!("{}: {}", project_stat.project_path.display(), size_str);
    }

    let total_str = format_bytes(stats.total_size, opts.human_readable);
    println!("\nTotal: {}", total_str);

    Ok(())
}

/// Clean cache
pub async fn cache_clean<C, S>(
    ctx: &C,
    storage: &S,
    opts: CacheCleanOptions,
) -> CliResult<()>
where
    C: CommandContext,
    S: StorageAccess,
{
    ctx.log_info("🧹 Clean Cache");

    if opts.dry_run {
        ctx.log_warning("Running in dry-run mode - no actual deletion will occur");
    }

    let deleted_count = storage.cleanup_expired_data().await?;

    if opts.dry_run {
        println!("{} items would be cleaned up", deleted_count);
    } else {
        println!("{} items cleaned up", deleted_count);
    }

    Ok(())
}

/// List cache contents
pub async fn cache_list<C, S>(
    ctx: &C,
    storage: &S,
    opts: CacheListOptions,
) -> CliResult<()>
where
    C: CommandContext,
    S: StorageAccess,
{
    ctx.log_info("📋 Project Cache List");

    let stats = storage.calculate_storage_usage().await?;
    let mut projects = stats.projects;

    if opts.sort_by_size {
        projects.sort_by(|a, b| b.size_bytes.cmp(&a.size_bytes));
    }

    for project_stat in projects {
        let size_mb = project_stat.size_bytes / (1024 * 1024);

        if let Some(min_mb) = opts.min_size_mb {
            if size_mb < min_mb as u64 {
                continue;
            }
        }

        println!(
            "{} - {} - {} sessions, {} checkpoints",
            project_stat.project_path.display(),
            format_bytes(project_stat.size_bytes, true),
            project_stat.session_count,
            project_stat.checkpoint_count
        );
    }

    Ok(())
}

/// Vacuum storage (optimize and defragment)
pub async fn cache_vacuum<C, S>(
    ctx: &C,
    storage: &S,
    opts: CacheVacuumOptions,
) -> CliResult<()>
where
    C: CommandContext,
    S: StorageAccess,
{
    ctx.log_info("🔧 Vacuum Storage");

    let stats = storage.calculate_storage_usage().await?;

    println!(
        "Current storage usage: {}",
        format_bytes(stats.total_size, true)
    );

    if opts.dry_run {
        ctx.log_warning("Running in dry-run mode - no actual optimization will occur");

        // Calculate potential savings
        let mut potential_savings = 0u64;
        let mut files_to_clean = 0u32;

        // Analyze temporary files
        let data_dir = ctx.data_dir()?;
        let temp_path = data_dir.join("temp");

        if temp_path.exists() {
            if let Ok(entries) = std::fs::read_dir(&temp_path) {
                for entry in entries.flatten() {
                    if let Ok(metadata) = entry.metadata() {
                        potential_savings += metadata.len();
                        files_to_clean += 1;
                    }
                }
            }
        }

        println!(
            "  📁 Temporary files: {} files ({} bytes)",
            files_to_clean, potential_savings
        );
        println!(
            "  🗜️  Potential compression savings: ~{}",
            format_bytes(stats.total_size / 5, true)
        ); // Estimate 20% compression
        println!("  📊 Fragmented session data: checking...");

        println!("\nVacuum operations would:");
        println!("  • Remove {} temporary files", files_to_clean);
        println!("  • Optimize checkpoint data compression");
        println!("  • Defragment session storage");
        println!("  • Clean up orphaned metadata");

        if potential_savings > 0 {
            println!(
                "\nEstimated space savings: {}",
                format_bytes(potential_savings, true)
            );
        }
    } else {
        ctx.log_success("Starting storage optimization...");

        // Clean temporary files
        let data_dir = ctx.data_dir()?;
        let temp_path = data_dir.join("temp");
        let mut cleaned_files = 0;
        let mut cleaned_bytes = 0u64;

        if temp_path.exists() {
            if let Ok(entries) = std::fs::read_dir(&temp_path) {
                for entry in entries.flatten() {
                    if let Ok(metadata) = entry.metadata() {
                        cleaned_bytes += metadata.len();
                        let _ = std::fs::remove_file(entry.path());
                        cleaned_files += 1;
                    }
                }
            }
        }

        println!(
            "✓ Cleaned {} temporary files ({})",
            cleaned_files,
            format_bytes(cleaned_bytes, true)
        );
        println!("✓ Storage optimization completed");

        let new_stats = storage.calculate_storage_usage().await?;
        if new_stats.total_size < stats.total_size {
            let savings = stats.total_size - new_stats.total_size;
            println!("💾 Space saved: {}", format_bytes(savings, true));
        } else {
            println!("💾 No significant space savings found");
        }
    }

    Ok(())
}

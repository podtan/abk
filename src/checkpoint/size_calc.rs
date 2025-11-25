//! Storage size calculation utilities with caching and performance optimization

use super::{AtomicOps, CheckpointError, CheckpointResult};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};
use tokio::fs;

/// Storage size calculator with caching
pub struct StorageSizeCalculator {
    cache: HashMap<PathBuf, CachedSizeInfo>,
    cache_ttl: Duration,
}

/// Cached size information
#[derive(Debug, Clone, Serialize, Deserialize)]
struct CachedSizeInfo {
    size_bytes: u64,
    file_count: u64,
    last_calculated: DateTime<Utc>,
    last_modified: SystemTime,
}

impl StorageSizeCalculator {
    /// Create a new size calculator with specified cache TTL
    pub fn new(cache_ttl_seconds: u64) -> Self {
        Self {
            cache: HashMap::new(),
            cache_ttl: Duration::from_secs(cache_ttl_seconds),
        }
    }

    /// Calculate directory size with caching
    pub async fn calculate_size(&mut self, dir_path: &Path) -> CheckpointResult<SizeInfo> {
        // Check if we have a valid cached result
        if let Some(cached) = self.get_cached_size(dir_path).await? {
            return Ok(SizeInfo {
                size_bytes: cached.size_bytes,
                file_count: cached.file_count,
                last_calculated: cached.last_calculated,
                from_cache: true,
            });
        }

        // Calculate fresh size
        let start_time = std::time::Instant::now();
        let (size_bytes, file_count) = self.calculate_directory_size_recursive(dir_path).await?;
        let _calculation_time = start_time.elapsed();

        let now = Utc::now();
        let last_modified = self.get_directory_last_modified(dir_path).await?;

        // Cache the result
        let cached_info = CachedSizeInfo {
            size_bytes,
            file_count,
            last_calculated: now,
            last_modified,
        };
        self.cache.insert(dir_path.to_path_buf(), cached_info);

        // Optionally persist cache to disk for large directories
        if file_count > 1000 {
            self.save_cache_to_disk(dir_path).await?;
        }

        Ok(SizeInfo {
            size_bytes,
            file_count,
            last_calculated: now,
            from_cache: false,
        })
    }

    /// Get cached size if still valid
    async fn get_cached_size(&self, dir_path: &Path) -> CheckpointResult<Option<CachedSizeInfo>> {
        // Check in-memory cache first
        if let Some(cached) = self.cache.get(dir_path) {
            if self.is_cache_valid(cached, dir_path).await? {
                return Ok(Some(cached.clone()));
            }
        }

        // Try to load from disk cache
        self.load_cache_from_disk(dir_path).await
    }

    /// Check if cached size is still valid
    async fn is_cache_valid(
        &self,
        cached: &CachedSizeInfo,
        dir_path: &Path,
    ) -> CheckpointResult<bool> {
        // Check TTL
        let age = Utc::now().signed_duration_since(cached.last_calculated);
        if age > chrono::Duration::from_std(self.cache_ttl).unwrap() {
            return Ok(false);
        }

        // Check if directory has been modified
        let current_modified = self.get_directory_last_modified(dir_path).await?;
        Ok(current_modified <= cached.last_modified)
    }

    /// Calculate directory size recursively with optimizations
    async fn calculate_directory_size_recursive(
        &self,
        dir_path: &Path,
    ) -> CheckpointResult<(u64, u64)> {
        if !dir_path.exists() {
            return Ok((0, 0));
        }

        let mut total_size = 0u64;
        let mut total_files = 0u64;
        let mut entries = fs::read_dir(dir_path).await?;

        while let Some(entry) = entries.next_entry().await? {
            let metadata = entry.metadata().await?;

            if metadata.is_dir() {
                let (subdir_size, subdir_files) =
                    Box::pin(self.calculate_directory_size_recursive(&entry.path())).await?;
                total_size += subdir_size;
                total_files += subdir_files;
            } else if metadata.is_file() {
                total_size += metadata.len();
                total_files += 1;
            }
            // Skip symlinks and special files for safety
        }

        Ok((total_size, total_files))
    }

    /// Get directory last modification time (newest file/dir in tree)
    async fn get_directory_last_modified(&self, dir_path: &Path) -> CheckpointResult<SystemTime> {
        let metadata = fs::metadata(dir_path).await?;
        let mut latest = metadata.modified()?;

        let mut entries = fs::read_dir(dir_path).await?;
        while let Some(entry) = entries.next_entry().await? {
            let entry_metadata = entry.metadata().await?;
            let entry_modified = entry_metadata.modified()?;

            if entry_modified > latest {
                latest = entry_modified;
            }

            // For directories, recursively check (but limit depth for performance)
            if entry_metadata.is_dir() {
                let subdir_modified =
                    Box::pin(self.get_directory_last_modified(&entry.path())).await?;
                if subdir_modified > latest {
                    latest = subdir_modified;
                }
            }
        }

        Ok(latest)
    }

    /// Save cache to disk for persistence
    async fn save_cache_to_disk(&self, dir_path: &Path) -> CheckpointResult<()> {
        if let Some(cached) = self.cache.get(dir_path) {
            let cache_file = self.get_cache_file_path(dir_path);
            AtomicOps::write_json(&cache_file, cached)?;
        }
        Ok(())
    }

    /// Load cache from disk
    async fn load_cache_from_disk(
        &self,
        dir_path: &Path,
    ) -> CheckpointResult<Option<CachedSizeInfo>> {
        let cache_file = self.get_cache_file_path(dir_path);

        if !cache_file.exists() {
            return Ok(None);
        }

        match AtomicOps::read_json::<CachedSizeInfo>(&cache_file) {
            Ok(cached) => {
                if self.is_cache_valid(&cached, dir_path).await? {
                    Ok(Some(cached))
                } else {
                    // Clean up expired cache file
                    let _ = fs::remove_file(&cache_file).await;
                    Ok(None)
                }
            }
            Err(_) => {
                // Clean up corrupted cache file
                let _ = fs::remove_file(&cache_file).await;
                Ok(None)
            }
        }
    }

    /// Get cache file path for directory
    fn get_cache_file_path(&self, dir_path: &Path) -> PathBuf {
        let cache_dir = dir_path.join(".agent_cache");
        cache_dir.join("size_cache.json")
    }

    /// Clear cache for a directory
    pub async fn invalidate_cache(&mut self, dir_path: &Path) -> CheckpointResult<()> {
        self.cache.remove(dir_path);

        let cache_file = self.get_cache_file_path(dir_path);
        if cache_file.exists() {
            fs::remove_file(&cache_file).await?;
        }

        Ok(())
    }

    /// Clear all caches
    pub async fn clear_all_caches(&mut self) -> CheckpointResult<()> {
        for dir_path in self.cache.keys().cloned().collect::<Vec<_>>() {
            self.invalidate_cache(&dir_path).await?;
        }
        Ok(())
    }

    /// Get cache statistics
    pub fn get_cache_stats(&self) -> CacheStats {
        let total_cached_size: u64 = self.cache.values().map(|c| c.size_bytes).sum();
        let total_cached_files: u64 = self.cache.values().map(|c| c.file_count).sum();

        CacheStats {
            cached_directories: self.cache.len(),
            total_cached_size,
            total_cached_files,
            oldest_cache_entry: self.cache.values().map(|c| c.last_calculated).min(),
        }
    }
}

/// Size information result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SizeInfo {
    pub size_bytes: u64,
    pub file_count: u64,
    pub last_calculated: DateTime<Utc>,
    pub from_cache: bool,
}

/// Cache statistics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CacheStats {
    pub cached_directories: usize,
    pub total_cached_size: u64,
    pub total_cached_files: u64,
    pub oldest_cache_entry: Option<DateTime<Utc>>,
}

/// Utility functions for size formatting and calculations
pub struct SizeUtils;

impl SizeUtils {
    /// Format bytes in human-readable format with high precision
    pub fn format_bytes(bytes: u64, binary: bool) -> String {
        let (units, threshold): (&[&str], f64) = if binary {
            (&["B", "KiB", "MiB", "GiB", "TiB", "PiB"], 1024.0)
        } else {
            (&["B", "KB", "MB", "GB", "TB", "PB"], 1000.0)
        };

        if bytes == 0 {
            return "0 B".to_string();
        }

        let bytes_f = bytes as f64;
        let unit_index = (bytes_f.log10() / threshold.log10()).floor() as usize;
        let unit_index = unit_index.min(units.len() - 1);

        let value = bytes_f / threshold.powi(unit_index as i32);

        if value >= 100.0 {
            format!("{:.0} {}", value, units[unit_index])
        } else if value >= 10.0 {
            format!("{:.1} {}", value, units[unit_index])
        } else {
            format!("{:.2} {}", value, units[unit_index])
        }
    }

    /// Parse size string back to bytes
    pub fn parse_size_string(size_str: &str) -> CheckpointResult<u64> {
        let size_str = size_str.trim().to_uppercase();

        let (value_str, unit) = if let Some(pos) = size_str.find(char::is_alphabetic) {
            (size_str[..pos].trim(), size_str[pos..].trim())
        } else {
            (size_str.as_str(), "B")
        };

        let value: f64 = value_str.parse().map_err(|_| {
            CheckpointError::validation(format!("Invalid size value: {}", value_str))
        })?;

        let multiplier: u64 = match unit {
            "B" => 1,
            "KB" => 1_000,
            "MB" => 1_000_000,
            "GB" => 1_000_000_000,
            "TB" => 1_000_000_000_000,
            "KIB" => 1_024,
            "MIB" => 1_048_576,
            "GIB" => 1_073_741_824,
            "TIB" => 1_099_511_627_776,
            _ => {
                return Err(CheckpointError::validation(format!(
                    "Unknown size unit: {}",
                    unit
                )))
            }
        };

        Ok((value * multiplier as f64) as u64)
    }

    /// Calculate size difference as percentage
    pub fn size_change_percentage(old_size: u64, new_size: u64) -> f64 {
        if old_size == 0 {
            if new_size == 0 {
                0.0
            } else {
                100.0
            }
        } else {
            ((new_size as f64 - old_size as f64) / old_size as f64) * 100.0
        }
    }

    /// Get size category for quick classification
    pub fn get_size_category(bytes: u64) -> SizeCategory {
        match bytes {
            0 => SizeCategory::Empty,
            1..=1024 => SizeCategory::Tiny,
            1025..=1048576 => SizeCategory::Small, // 1 KB - 1 MB
            1048577..=104857600 => SizeCategory::Medium, // 1 MB - 100 MB
            104857601..=1073741824 => SizeCategory::Large, // 100 MB - 1 GB
            _ => SizeCategory::VeryLarge,
        }
    }
}

/// Size categories for classification
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum SizeCategory {
    Empty,
    Tiny,
    Small,
    Medium,
    Large,
    VeryLarge,
}

impl std::fmt::Display for SizeCategory {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SizeCategory::Empty => write!(f, "empty"),
            SizeCategory::Tiny => write!(f, "tiny"),
            SizeCategory::Small => write!(f, "small"),
            SizeCategory::Medium => write!(f, "medium"),
            SizeCategory::Large => write!(f, "large"),
            SizeCategory::VeryLarge => write!(f, "very large"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;
    use tokio::fs;

    #[tokio::test]
    async fn test_size_calculation() {
        let temp_dir = tempdir().unwrap();
        let test_dir = temp_dir.path().join("test");
        fs::create_dir_all(&test_dir).await.unwrap();

        // Create some test files
        fs::write(test_dir.join("file1.txt"), "hello")
            .await
            .unwrap();
        fs::write(test_dir.join("file2.txt"), "world!")
            .await
            .unwrap();

        let mut calculator = StorageSizeCalculator::new(300); // 5 minute TTL
        let size_info = calculator.calculate_size(&test_dir).await.unwrap();

        assert_eq!(size_info.size_bytes, 11); // "hello" + "world!" = 11 bytes
        assert_eq!(size_info.file_count, 2);
        assert!(!size_info.from_cache);

        // Second calculation should use cache
        let size_info2 = calculator.calculate_size(&test_dir).await.unwrap();
        assert!(size_info2.from_cache);
    }

    #[test]
    fn test_size_formatting() {
        assert_eq!(SizeUtils::format_bytes(0, false), "0 B");
        assert_eq!(SizeUtils::format_bytes(1024, false), "1.02 KB");
        assert_eq!(SizeUtils::format_bytes(1048576, false), "1.05 MB");
        assert_eq!(SizeUtils::format_bytes(1024, true), "1.00 KiB");
    }

    #[test]
    fn test_size_parsing() {
        assert_eq!(SizeUtils::parse_size_string("1024 B").unwrap(), 1024);
        assert_eq!(SizeUtils::parse_size_string("1 KB").unwrap(), 1000);
        assert_eq!(SizeUtils::parse_size_string("1 KiB").unwrap(), 1024);
        assert_eq!(SizeUtils::parse_size_string("1.5 MB").unwrap(), 1_500_000);
    }

    #[test]
    fn test_size_categories() {
        assert_eq!(SizeUtils::get_size_category(0), SizeCategory::Empty);
        assert_eq!(SizeUtils::get_size_category(512), SizeCategory::Tiny);
        assert_eq!(SizeUtils::get_size_category(50_000), SizeCategory::Small);
        assert_eq!(
            SizeUtils::get_size_category(50_000_000),
            SizeCategory::Medium
        );
        assert_eq!(
            SizeUtils::get_size_category(500_000_000),
            SizeCategory::Large
        );
        assert_eq!(
            SizeUtils::get_size_category(2_000_000_000),
            SizeCategory::VeryLarge
        );
    }
}

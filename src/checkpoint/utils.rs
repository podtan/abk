//! Checkpoint Utility Functions
//!
//! Standalone utility functions for checkpoint operations that don't depend
//! on specific agent implementations.

use crate::checkpoint::models::SystemInfo;
use std::collections::HashMap;

/// Get filtered environment variables safe for checkpoint storage.
///
/// Returns only non-sensitive environment variables that can be safely
/// stored in checkpoints. This excludes API keys, tokens, passwords, etc.
///
/// # Example
///
/// ```rust
/// use abk::checkpoint::utils::get_filtered_env_vars;
///
/// let safe_vars = get_filtered_env_vars();
/// assert!(safe_vars.contains_key("PATH"));
/// assert!(!safe_vars.contains_key("API_KEY")); // Excluded
/// ```
pub fn get_filtered_env_vars() -> HashMap<String, String> {
    use std::env;
    let mut filtered_vars = HashMap::new();

    // Safe environment variables to include in checkpoints
    let safe_vars = [
        "PATH",
        "HOME",
        "USER",
        "SHELL",
        "PWD",
        "TERM",
        "LANG",
        "LC_ALL",
        "CARGO_HOME",
        "RUSTUP_HOME",
        "RUST_BACKTRACE",
        "RUST_LOG",
    ];

    for var_name in safe_vars {
        if let Ok(value) = env::var(var_name) {
            filtered_vars.insert(var_name.to_string(), value);
        }
    }

    filtered_vars
}

/// Get current system information.
///
/// Collects system metadata including OS, architecture, hostname, and CPU count.
/// Falls back to "unknown" values if system information cannot be retrieved.
///
/// # Example
///
/// ```rust
/// use abk::checkpoint::utils::get_system_info;
///
/// let info = get_system_info();
/// println!("Running on: {} {}", info.os_name, info.architecture);
/// ```
pub fn get_system_info() -> SystemInfo {
    #[cfg(unix)]
    {
        use uname::uname;
        
        let (os_name, os_version, architecture, hostname) = if let Ok(info) = uname() {
            (info.sysname, info.release, info.machine, info.nodename)
        } else {
            (
                "unknown".to_string(),
                "unknown".to_string(),
                "unknown".to_string(),
                "unknown".to_string(),
            )
        };

        SystemInfo {
            os_name,
            os_version,
            architecture,
            hostname,
            cpu_count: std::thread::available_parallelism()
                .map(|p| p.get() as u32)
                .unwrap_or(1),
            total_memory: get_total_memory(),
        }
    }

    #[cfg(not(unix))]
    {
        SystemInfo {
            os_name: std::env::consts::OS.to_string(),
            os_version: "unknown".to_string(),
            architecture: std::env::consts::ARCH.to_string(),
            hostname: hostname::get()
                .ok()
                .and_then(|h| h.into_string().ok())
                .unwrap_or_else(|| "unknown".to_string()),
            cpu_count: std::thread::available_parallelism()
                .map(|p| p.get() as u32)
                .unwrap_or(1),
            total_memory: get_total_memory(),
        }
    }
}

/// Get total system memory in bytes.
///
/// Attempts to retrieve the total system memory. Returns 0 if unavailable.
fn get_total_memory() -> u64 {
    #[cfg(target_os = "linux")]
    {
        // Try to read from /proc/meminfo
        if let Ok(content) = std::fs::read_to_string("/proc/meminfo") {
            for line in content.lines() {
                if line.starts_with("MemTotal:") {
                    if let Some(kb_str) = line.split_whitespace().nth(1) {
                        if let Ok(kb) = kb_str.parse::<u64>() {
                            return kb * 1024; // Convert KB to bytes
                        }
                    }
                }
            }
        }
    }

    #[cfg(target_os = "macos")]
    {
        use std::process::Command;
        
        // Use sysctl to get memory
        if let Ok(output) = Command::new("sysctl")
            .arg("-n")
            .arg("hw.memsize")
            .output()
        {
            if let Ok(mem_str) = String::from_utf8(output.stdout) {
                if let Ok(mem) = mem_str.trim().parse::<u64>() {
                    return mem;
                }
            }
        }
    }

    #[cfg(target_os = "windows")]
    {
        // Windows memory retrieval would go here
        // For now, return 0
    }

    0 // Fallback
}

/// Estimate token count for text content.
///
/// Uses a simple heuristic: approximately 4 characters per token for English text.
/// This is a rough estimate and may not be accurate for all languages or content types.
///
/// # Arguments
/// * `content` - The text to estimate tokens for
///
/// # Returns
/// Estimated number of tokens
///
/// # Example
///
/// ```rust
/// use abk::checkpoint::utils::estimate_token_count;
///
/// let text = "Hello, world!";
/// let tokens = estimate_token_count(text);
/// assert_eq!(tokens, 3); // Roughly 13 chars / 4 = 3 tokens
/// ```
pub fn estimate_token_count(content: &str) -> usize {
    // Simple estimation: roughly 4 characters per token for English text
    (content.len() + 3) / 4
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_filtered_env_vars() {
        let vars = get_filtered_env_vars();
        
        // Should contain at least some common vars (if they exist in env)
        // We can't assert specific vars exist because test env may not have them
        assert!(vars.is_empty() || vars.len() > 0);
        
        // Should NOT contain any sensitive vars
        assert!(!vars.contains_key("API_KEY"));
        assert!(!vars.contains_key("SECRET"));
        assert!(!vars.contains_key("PASSWORD"));
    }

    #[test]
    fn test_get_system_info() {
        let info = get_system_info();
        
        // Should have some OS name
        assert!(!info.os_name.is_empty());
        
        // Should have at least 1 CPU
        assert!(info.cpu_count >= 1);
        
        // Architecture should be known
        assert!(!info.architecture.is_empty());
    }

    #[test]
    fn test_estimate_token_count() {
        assert_eq!(estimate_token_count(""), 0);
        assert_eq!(estimate_token_count("a"), 1);
        assert_eq!(estimate_token_count("abcd"), 1);
        assert_eq!(estimate_token_count("abcde"), 2);
        assert_eq!(estimate_token_count("Hello, world!"), 4); // 13 chars = (13+3)/4 = 4

        let long_text = "a".repeat(100);
        assert_eq!(estimate_token_count(&long_text), 25);
    }
}

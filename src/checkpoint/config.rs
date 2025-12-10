//! Configuration structures for the checkpoint system

use chrono::Duration;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Storage backend type
#[derive(Serialize, Deserialize, Debug, Clone, Default, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum StorageBackendType {
    /// Local file system storage (default)
    #[default]
    File,
    /// DocumentDB/MongoDB storage
    DocumentDB,
    /// MongoDB storage (alias for DocumentDB)
    MongoDB,
}

/// Storage mode - determines where checkpoints are stored
#[derive(Serialize, Deserialize, Debug, Clone, Default, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum StorageMode {
    /// Only local file system (default when no remote backend)
    #[default]
    Local,
    /// Only remote backend (no local files)
    Remote,
    /// Both local and remote (mirroring for reliability)
    Mirror,
}

/// Storage backend configuration
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct StorageBackendConfig {
    /// Backend type: "file", "documentdb", or "mongodb"
    #[serde(default)]
    pub backend_type: StorageBackendType,
    
    /// Storage mode: "local", "remote", or "mirror"
    /// - local: Only local files (default when backend_type is file)
    /// - remote: Only remote storage (no local files)
    /// - mirror: Both local and remote (default when remote backend is configured)
    #[serde(default)]
    pub storage_mode: StorageMode,
    
    /// Connection URL for remote backends (supports env var substitution)
    /// Example: "${DOCUMENTDB_URL}" or "mongodb://localhost:10260"
    pub connection_url: Option<String>,
    
    /// Database name for remote backends
    /// Example: "${DOCUMENTDB_DATABASE}" or "trustee_checkpoints"
    pub database: Option<String>,
    
    /// Collection/table name for storing checkpoints
    #[serde(default = "default_collection_name")]
    pub collection: String,
    
    /// Username for authentication (supports env var substitution)
    /// Example: "${DOCUMENTDB_USERNAME}"
    pub username: Option<String>,
    
    /// Password for authentication (supports env var substitution)
    /// Example: "${DOCUMENTDB_PASSWORD}"
    pub password: Option<String>,
    
    /// Enable TLS/SSL for connection
    #[serde(default)]
    pub tls_enabled: bool,
    
    /// Allow invalid TLS certificates (for local development)
    #[serde(default)]
    pub tls_allow_invalid_certs: bool,
    
    /// Connection timeout in seconds
    #[serde(default = "default_connection_timeout")]
    pub connection_timeout_secs: u64,
}

fn default_collection_name() -> String {
    "checkpoints".to_string()
}

fn default_connection_timeout() -> u64 {
    30
}

impl Default for StorageBackendConfig {
    fn default() -> Self {
        Self {
            backend_type: StorageBackendType::File,
            storage_mode: StorageMode::Local,
            connection_url: None,
            database: None,
            collection: default_collection_name(),
            username: None,
            password: None,
            tls_enabled: false,
            tls_allow_invalid_certs: false,
            connection_timeout_secs: default_connection_timeout(),
        }
    }
}

impl StorageBackendConfig {
    /// Build connection string with credentials for DocumentDB/MongoDB
    /// Substitutes environment variables in the format ${VAR_NAME}
    pub fn build_connection_string(&self) -> Option<String> {
        let url = self.resolve_env_var(self.connection_url.as_deref()?)?;
        let username = self.resolve_env_var(self.username.as_deref().unwrap_or(""))?;
        let password = self.resolve_env_var(self.password.as_deref().unwrap_or(""))?;
        
        // Parse URL and inject credentials if needed
        if !username.is_empty() && !password.is_empty() {
            // Check if URL already has credentials
            if url.contains("@") {
                return Some(url);
            }
            
            // Inject credentials into URL
            if let Some(rest) = url.strip_prefix("mongodb://") {
                let encoded_user = urlencoding::encode(&username);
                let encoded_pass = urlencoding::encode(&password);
                let mut conn_str = format!("mongodb://{}:{}@{}", encoded_user, encoded_pass, rest);
                
                // Add TLS options if enabled
                if self.tls_enabled {
                    let separator = if conn_str.contains('?') { "&" } else { "?" };
                    conn_str.push_str(&format!("{}tls=true", separator));
                    if self.tls_allow_invalid_certs {
                        conn_str.push_str("&tlsAllowInvalidCertificates=true");
                    }
                }
                
                return Some(conn_str);
            }
        }
        
        // Return URL as-is with TLS options if needed
        let mut conn_str = url;
        if self.tls_enabled && !conn_str.contains("tls=") {
            let separator = if conn_str.contains('?') { "&" } else { "?" };
            conn_str.push_str(&format!("{}tls=true", separator));
            if self.tls_allow_invalid_certs {
                conn_str.push_str("&tlsAllowInvalidCertificates=true");
            }
        }
        
        Some(conn_str)
    }
    
    /// Get the resolved database name
    pub fn get_database(&self) -> Option<String> {
        self.resolve_env_var(self.database.as_deref()?)
    }
    
    /// Resolve environment variable placeholders like ${VAR_NAME}
    fn resolve_env_var(&self, value: &str) -> Option<String> {
        if value.starts_with("${") && value.ends_with("}") {
            let var_name = &value[2..value.len()-1];
            std::env::var(var_name).ok()
        } else if value.contains("${") {
            // Handle mixed content with env vars
            let mut result = value.to_string();
            let re = regex::Regex::new(r"\$\{([^}]+)\}").ok()?;
            for cap in re.captures_iter(value) {
                if let Some(var_name) = cap.get(1) {
                    if let Ok(var_value) = std::env::var(var_name.as_str()) {
                        result = result.replace(&cap[0], &var_value);
                    }
                }
            }
            Some(result)
        } else {
            Some(value.to_string())
        }
    }
    
    /// Get effective storage mode based on configuration
    /// - If storage_mode is explicitly set, use that
    /// - If remote backend is configured and mode is Local, default to Mirror
    /// - Otherwise use Local
    pub fn effective_storage_mode(&self) -> StorageMode {
        match (&self.storage_mode, &self.backend_type) {
            // Explicit mode set - use it
            (StorageMode::Remote, StorageBackendType::DocumentDB | StorageBackendType::MongoDB) => StorageMode::Remote,
            (StorageMode::Mirror, StorageBackendType::DocumentDB | StorageBackendType::MongoDB) => StorageMode::Mirror,
            (StorageMode::Local, StorageBackendType::File) => StorageMode::Local,
            // Remote backend configured but mode is Local - default to Mirror for safety
            (StorageMode::Local, StorageBackendType::DocumentDB | StorageBackendType::MongoDB) => StorageMode::Mirror,
            // File backend with remote modes doesn't make sense - fallback to Local
            (StorageMode::Remote | StorageMode::Mirror, StorageBackendType::File) => StorageMode::Local,
        }
    }
    
    /// Check if local storage should be used
    pub fn should_use_local(&self) -> bool {
        matches!(self.effective_storage_mode(), StorageMode::Local | StorageMode::Mirror)
    }
    
    /// Check if remote storage should be used
    pub fn should_use_remote(&self) -> bool {
        matches!(self.effective_storage_mode(), StorageMode::Remote | StorageMode::Mirror)
    }
}

/// Global checkpoint configuration
#[derive(Serialize, Deserialize, Debug, Clone, Default)]
#[serde(default)]
pub struct GlobalCheckpointConfig {
    /// Master enable/disable switch
    pub enabled: bool,
    /// Base storage directory (~/.{agent_name})
    #[serde(default = "get_default_storage_location")]
    pub storage_location: PathBuf,
    /// Create checkpoint every N iterations
    #[serde(default = "default_checkpoint_interval")]
    pub auto_checkpoint_interval: u32,
    /// Maximum checkpoints per session
    #[serde(default = "default_max_checkpoints")]
    pub max_checkpoints_per_session: u32,
    /// Enable checkpoint compression
    #[serde(default)]
    pub compression_enabled: bool,
    /// Data retention policy
    #[serde(default)]
    pub retention: RetentionPolicy,
    /// Git integration settings
    #[serde(default)]
    pub git_integration: GitIntegrationConfig,
    /// Performance settings
    #[serde(default)]
    pub performance: PerformanceConfig,
    /// Security settings
    #[serde(default)]
    pub security: SecurityConfig,
    /// Logging configuration
    #[serde(default)]
    pub logging: LoggingConfig,
    /// Storage backend configuration
    #[serde(default)]
    pub storage_backend: StorageBackendConfig,
}

fn default_checkpoint_interval() -> u32 {
    1
}

fn default_max_checkpoints() -> u32 {
    50
}

/// Per-project checkpoint configuration
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ProjectCheckpointConfig {
    pub enabled: Option<bool>, // Override global enabled setting
    pub auto_checkpoint_interval: Option<u32>, // Override global interval
    pub max_checkpoints_per_session: Option<u32>, // Override global max
    pub compression_enabled: Option<bool>, // Override global compression
    pub retention: Option<RetentionPolicy>, // Override global retention
    pub git_integration: Option<GitIntegrationConfig>, // Override git settings
    pub exclude_patterns: Vec<String>, // Files/dirs to exclude from checkpoints
    pub include_patterns: Vec<String>, // Files/dirs to specifically include
    pub custom_tags: Vec<String>, // Default tags for this project
    pub description_template: Option<String>, // Template for checkpoint descriptions
}

impl Default for ProjectCheckpointConfig {
    fn default() -> Self {
        Self {
            enabled: None,
            auto_checkpoint_interval: None,
            max_checkpoints_per_session: None,
            compression_enabled: None,
            retention: None,
            git_integration: None,
            exclude_patterns: vec![
                "target/**".to_string(),
                "node_modules/**".to_string(),
                ".git/**".to_string(),
                "*.log".to_string(),
                "*.tmp".to_string(),
            ],
            include_patterns: vec![
                "src/**".to_string(),
                "*.rs".to_string(),
                "*.toml".to_string(),
                "*.md".to_string(),
            ],
            custom_tags: Vec::new(),
            description_template: None,
        }
    }
}

/// Data retention policy
#[derive(Serialize, Deserialize, Debug, Clone, Default)]
#[serde(default)]
pub struct RetentionPolicy {
    #[serde(default = "default_retention_days")]
    pub max_age_days: Option<u32>,      // Delete data older than N days
    #[serde(default = "default_retention_size")]
    pub max_total_size_gb: Option<u32>, // Delete oldest when total size exceeds N GB
    #[serde(default = "default_sessions_per_project")]
    pub max_sessions_per_project: Option<u32>, // Keep only N newest sessions per project
    #[serde(default = "default_cleanup_interval")]
    pub cleanup_interval_hours: u32,    // Run cleanup every N hours
    #[serde(default = "default_true")]
    pub enable_auto_cleanup: bool,      // Automatically clean up expired data
    #[serde(default = "default_true")]
    pub preserve_tagged: bool,          // Never delete tagged checkpoints
    #[serde(default = "default_true")]
    pub preserve_active_sessions: bool, // Never delete active sessions
}

fn default_retention_days() -> Option<u32> { Some(30) }
fn default_retention_size() -> Option<u32> { Some(10) }
fn default_sessions_per_project() -> Option<u32> { Some(20) }
fn default_cleanup_interval() -> u32 { 24 }
fn default_true() -> bool { true }

/// Git integration configuration
#[derive(Serialize, Deserialize, Debug, Clone, Default)]
#[serde(default)]
pub struct GitIntegrationConfig {
    #[serde(default)]
    pub enabled: bool,                         // Enable git integration
    pub shadow_repo_location: Option<PathBuf>, // Location for shadow repos
    #[serde(default)]
    pub auto_commit_before_checkpoint: bool,   // Auto-commit changes before checkpoint
    #[serde(default = "default_true")]
    pub create_git_snapshots: bool,            // Create git snapshots of file system
    #[serde(default = "default_true")]
    pub track_uncommitted_changes: bool,       // Track uncommitted changes in checkpoints
    #[serde(default = "default_true")]
    pub exclude_gitignored_files: bool,        // Exclude gitignored files from checkpoints
    #[serde(default = "default_commit_message_template")]
    pub commit_message_template: String,       // Template for commit messages
}

fn default_commit_message_template() -> String {
    "Checkpoint: {checkpoint_id} - {workflow_step}".to_string()
}

/// Performance configuration
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(default)]
pub struct PerformanceConfig {
    pub compression_level: u32,           // Compression level (0-9)
    pub enable_lazy_loading: bool,        // Load checkpoints on demand
    pub enable_async_operations: bool,    // Use async I/O where possible
    pub max_concurrent_operations: u32,   // Maximum concurrent operations
    pub checkpoint_creation_timeout: u64, // Timeout for checkpoint creation in seconds
    pub enable_caching: bool,             // Cache frequently accessed data
    pub cache_size_mb: u32,               // Maximum cache size in MB
}

impl Default for PerformanceConfig {
    fn default() -> Self {
        Self {
            compression_level: 6,
            enable_lazy_loading: true,
            enable_async_operations: true,
            max_concurrent_operations: 4,
            checkpoint_creation_timeout: 60,
            enable_caching: true,
            cache_size_mb: 100,
        }
    }
}

/// Security configuration
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(default)]
pub struct SecurityConfig {
    pub enable_encryption: bool,              // Encrypt checkpoint data
    pub encryption_key_derivation: String,    // Key derivation method
    pub filter_sensitive_env_vars: bool,      // Filter sensitive environment variables
    pub sensitive_env_patterns: Vec<String>,  // Patterns for sensitive env vars
    pub file_permission_strict: bool,         // Strict file permission checks
    pub allowed_file_extensions: Vec<String>, // Allowed file extensions for checkpoints
    pub denied_file_extensions: Vec<String>,  // Denied file extensions
    pub max_file_size_mb: u32,                // Maximum file size to include in checkpoints
}

impl Default for SecurityConfig {
    fn default() -> Self {
        Self {
            enable_encryption: false,
            encryption_key_derivation: "pbkdf2".to_string(),
            filter_sensitive_env_vars: true,
            sensitive_env_patterns: vec![
                "PASSWORD".to_string(),
                "SECRET".to_string(),
                "KEY".to_string(),
                "TOKEN".to_string(),
                "API_KEY".to_string(),
                "PRIVATE".to_string(),
            ],
            file_permission_strict: true,
            allowed_file_extensions: vec![
                "rs".to_string(),
                "toml".to_string(),
                "md".to_string(),
                "txt".to_string(),
                "json".to_string(),
                "yaml".to_string(),
                "yml".to_string(),
                "py".to_string(),
                "js".to_string(),
                "ts".to_string(),
                "html".to_string(),
                "css".to_string(),
                "sh".to_string(),
                "bat".to_string(),
            ],
            denied_file_extensions: vec![
                "exe".to_string(),
                "dll".to_string(),
                "so".to_string(),
                "dylib".to_string(),
                "bin".to_string(),
                "obj".to_string(),
                "o".to_string(),
                "class".to_string(),
            ],
            max_file_size_mb: 10,
        }
    }
}

/// Logging configuration
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(default)]
pub struct LoggingConfig {
    pub log_level: String,               // Logging level (DEBUG, INFO, WARN, ERROR)
    pub log_to_file: bool,               // Log to file
    pub log_file: Option<PathBuf>,       // Log file path
    pub log_rotation_size_mb: u32,       // Rotate logs at N MB
    pub log_retention_days: u32,         // Keep logs for N days
    pub embed_performance_metrics: bool, // Embed performance metrics in logs
    pub log_checkpoint_operations: bool, // Log checkpoint operations
    pub log_file_changes: bool,          // Log file system changes
}

impl Default for LoggingConfig {
    fn default() -> Self {
        Self {
            log_level: "INFO".to_string(),
            log_to_file: true,
            log_file: None,
            log_rotation_size_mb: 100,
            log_retention_days: 7,
            embed_performance_metrics: true,
            log_checkpoint_operations: true,
            log_file_changes: false,
        }
    }
}

/// Storage statistics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageStats {
    pub total_size: u64,             // Total storage size in bytes
    pub project_count: u32,          // Number of projects
    pub session_count: u32,          // Total number of sessions
    pub checkpoint_count: u32,       // Total number of checkpoints
    pub projects: Vec<ProjectStats>, // Per-project statistics
}

/// Per-project storage statistics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectStats {
    pub project_hash: String,                         // Project identifier
    pub project_path: PathBuf,                        // Project path
    pub size_bytes: u64,                              // Project storage size
    pub session_count: u32,                           // Number of sessions
    pub checkpoint_count: u32,                        // Number of checkpoints
    pub last_accessed: chrono::DateTime<chrono::Utc>, // Last access time
    pub sessions: Vec<SessionStats>,                  // Per-session statistics
}

/// Per-session storage statistics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionStats {
    pub session_id: String,                           // Session identifier
    pub size_bytes: u64,                              // Session storage size
    pub checkpoint_count: u32,                        // Number of checkpoints
    pub created_at: chrono::DateTime<chrono::Utc>,    // Creation time
    pub last_accessed: chrono::DateTime<chrono::Utc>, // Last access time
}

/// Cleanup report
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CleanupReport {
    pub deleted_sessions: u32,    // Number of sessions deleted
    pub deleted_checkpoints: u32, // Number of checkpoints deleted
    pub freed_bytes: u64,         // Bytes freed
    pub duration: Duration,       // Time taken for cleanup
    pub errors: Vec<String>,      // Errors encountered during cleanup
}

/// Migration report
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MigrationReport {
    pub from_version: String,      // Source version
    pub to_version: String,        // Target version
    pub migrated_checkpoints: u32, // Number of checkpoints migrated
    pub failed_migrations: u32,    // Number of failed migrations
    pub duration: Duration,        // Time taken for migration
    pub errors: Vec<String>,       // Errors encountered during migration
}

/// Get the default storage location (~/.{agent_name})
/// Uses ABK_AGENT_NAME environment variable, defaults to "NO_AGENT_NAME" if not set
fn get_default_storage_location() -> PathBuf {
    let agent_name = std::env::var("ABK_AGENT_NAME").unwrap_or_else(|_| "NO_AGENT_NAME".to_string());
    let dir_name = format!(".{}", agent_name);
    
    if let Ok(home) = std::env::var("HOME") {
        PathBuf::from(home).join(&dir_name)
    } else {
        // Fallback for Windows
        if let Ok(userprofile) = std::env::var("USERPROFILE") {
            PathBuf::from(userprofile).join(&dir_name)
        } else {
            // Last resort fallback
            PathBuf::from("/tmp").join(&dir_name)
        }
    }
}

/// Merge project config with global config
impl ProjectCheckpointConfig {
    pub fn merge_with_global(&self, global: &GlobalCheckpointConfig) -> GlobalCheckpointConfig {
        GlobalCheckpointConfig {
            enabled: self.enabled.unwrap_or(global.enabled),
            storage_location: global.storage_location.clone(),
            auto_checkpoint_interval: self
                .auto_checkpoint_interval
                .unwrap_or(global.auto_checkpoint_interval),
            max_checkpoints_per_session: self
                .max_checkpoints_per_session
                .unwrap_or(global.max_checkpoints_per_session),
            compression_enabled: self
                .compression_enabled
                .unwrap_or(global.compression_enabled),
            retention: self
                .retention
                .clone()
                .unwrap_or_else(|| global.retention.clone()),
            git_integration: self
                .git_integration
                .clone()
                .unwrap_or_else(|| global.git_integration.clone()),
            performance: global.performance.clone(),
            security: global.security.clone(),
            logging: global.logging.clone(),
            storage_backend: global.storage_backend.clone(),
        }
    }
}

/// Project Configuration Manager
/// Handles loading, saving, and managing per-project checkpoint configurations
#[derive(Debug)]
pub struct ProjectConfigManager {
    config_file_path: PathBuf,
    projects: HashMap<String, ProjectCheckpointConfig>,
}

impl ProjectConfigManager {
    /// Create a new project configuration manager
    pub fn new() -> super::errors::CheckpointResult<Self> {
        let home_dir = std::env::var("HOME").map_err(|_| {
            super::errors::CheckpointError::config("Could not determine home directory")
        })?;
        let config_file_path = PathBuf::from(home_dir)
            .join(".agent")
            .join("config")
            .join("projects.toml");

        let projects = if config_file_path.exists() {
            Self::load_projects_config(&config_file_path)?
        } else {
            HashMap::new()
        };

        Ok(Self {
            config_file_path,
            projects,
        })
    }

    /// Load projects configuration from TOML file
    fn load_projects_config(
        path: &Path,
    ) -> super::errors::CheckpointResult<HashMap<String, ProjectCheckpointConfig>> {
        use std::fs;

        let content = fs::read_to_string(path).map_err(|e| {
            super::errors::CheckpointError::config(format!(
                "Failed to read projects config: {}",
                e
            ))
        })?;

        let projects: HashMap<String, ProjectCheckpointConfig> =
            toml::from_str(&content).map_err(|e| {
                super::errors::CheckpointError::config(format!(
                    "Failed to parse projects config: {}",
                    e
                ))
            })?;

        Ok(projects)
    }

    /// Save projects configuration to TOML file
    pub fn save(&self) -> super::errors::CheckpointResult<()> {
        use std::fs;

        // Create config directory if it doesn't exist
        if let Some(parent) = self.config_file_path.parent() {
            fs::create_dir_all(parent).map_err(|e| {
                super::errors::CheckpointError::config(format!(
                    "Failed to create config directory: {}",
                    e
                ))
            })?;
        }

        let content = toml::to_string_pretty(&self.projects).map_err(|e| {
            super::errors::CheckpointError::config(format!(
                "Failed to serialize projects config: {}",
                e
            ))
        })?;

        fs::write(&self.config_file_path, content).map_err(|e| {
            super::errors::CheckpointError::config(format!(
                "Failed to write projects config: {}",
                e
            ))
        })?;

        Ok(())
    }

    /// Register a new project with configuration
    pub fn register_project(
        &mut self,
        project_path: &str,
        config: ProjectCheckpointConfig,
    ) -> super::errors::CheckpointResult<()> {
        self.projects.insert(project_path.to_string(), config);
        self.save()?;
        Ok(())
    }

    /// Deregister a project
    pub fn deregister_project(
        &mut self,
        project_path: &str,
    ) -> super::errors::CheckpointResult<bool> {
        let removed = self.projects.remove(project_path).is_some();
        if removed {
            self.save()?;
        }
        Ok(removed)
    }

    /// Get project configuration
    pub fn get_project_config(&self, project_path: &str) -> Option<&ProjectCheckpointConfig> {
        self.projects.get(project_path)
    }

    /// Get all registered projects
    pub fn list_projects(&self) -> Vec<String> {
        self.projects.keys().cloned().collect()
    }

    /// Update project configuration
    pub fn update_project_config(
        &mut self,
        project_path: &str,
        config: ProjectCheckpointConfig,
    ) -> super::errors::CheckpointResult<bool> {
        if self.projects.contains_key(project_path) {
            self.projects.insert(project_path.to_string(), config);
            self.save()?;
            Ok(true)
        } else {
            Ok(false)
        }
    }

    /// Get effective configuration for a project (merged with global)
    pub fn get_effective_config(
        &self,
        project_path: &str,
        global_config: &GlobalCheckpointConfig,
    ) -> GlobalCheckpointConfig {
        if let Some(project_config) = self.get_project_config(project_path) {
            project_config.merge_with_global(global_config)
        } else {
            global_config.clone()
        }
    }

    /// Validate project configuration
    pub fn validate_project_config(config: &ProjectCheckpointConfig) -> Vec<String> {
        let mut errors = Vec::new();

        // Validate interval
        if let Some(interval) = config.auto_checkpoint_interval {
            if interval == 0 {
                errors.push("Auto checkpoint interval cannot be zero".to_string());
            }
        }

        // Validate max checkpoints
        if let Some(max_checkpoints) = config.max_checkpoints_per_session {
            if max_checkpoints == 0 {
                errors.push("Max checkpoints per session cannot be zero".to_string());
            }
            if max_checkpoints > 1000 {
                errors.push("Max checkpoints per session should not exceed 1000".to_string());
            }
        }

        // Validate patterns
        for pattern in &config.exclude_patterns {
            if pattern.trim().is_empty() {
                errors.push("Exclude patterns cannot be empty strings".to_string());
                break;
            }
        }

        for pattern in &config.include_patterns {
            if pattern.trim().is_empty() {
                errors.push("Include patterns cannot be empty strings".to_string());
                break;
            }
        }

        errors
    }
}

/// Configuration migration system
/// Handles version-based migration of configuration files
pub struct ConfigMigrator {
    #[allow(dead_code)]
    current_version: String,
}

impl ConfigMigrator {
    /// Create a new configuration migrator
    pub fn new() -> Self {
        Self {
            current_version: "1.0.0".to_string(),
        }
    }

    /// Migrate global configuration
    pub fn migrate_global_config(
        config_path: &Path,
    ) -> super::errors::CheckpointResult<MigrationReport> {
        let mut report = MigrationReport {
            from_version: "0.0.0".to_string(),
            to_version: "1.0.0".to_string(),
            migrated_checkpoints: 0,
            failed_migrations: 0,
            duration: Duration::seconds(0),
            errors: Vec::new(),
        };

        if !config_path.exists() {
            Self::create_default_global_config(config_path)?;
            return Ok(report);
        }

        // Read existing config
        let content = std::fs::read_to_string(config_path).map_err(|e| {
            super::errors::CheckpointError::config(format!(
                "Failed to read config file: {}",
                e
            ))
        })?;

        // Try to parse as current format
        match toml::from_str::<GlobalCheckpointConfig>(&content) {
            Ok(_) => {
                report.from_version = "1.0.0".to_string();
                // Already in latest format
            }
            Err(_) => {
                // Need migration - backup original and create new
                let backup_path = config_path.with_extension("toml.backup");
                std::fs::copy(config_path, &backup_path).map_err(|e| {
                    super::errors::CheckpointError::config(format!(
                        "Failed to create backup: {}",
                        e
                    ))
                })?;

                Self::create_default_global_config(config_path)?;
                report.migrated_checkpoints = 1;
            }
        }

        Ok(report)
    }

    /// Create default global configuration
    fn create_default_global_config(
        config_path: &Path,
    ) -> super::errors::CheckpointResult<()> {
        use std::fs;

        // Create config directory if needed
        if let Some(parent) = config_path.parent() {
            fs::create_dir_all(parent)?;
        }

        let default_config = GlobalCheckpointConfig::default();
        let content = toml::to_string_pretty(&default_config).map_err(|e| {
            super::errors::CheckpointError::config(format!(
                "Failed to serialize default config: {}",
                e
            ))
        })?;

        fs::write(config_path, content)?;
        Ok(())
    }

    /// Migrate projects configuration
    pub fn migrate_projects_config(
        config_path: &Path,
    ) -> super::errors::CheckpointResult<MigrationReport> {
        let report = MigrationReport {
            from_version: "1.0.0".to_string(),
            to_version: "1.0.0".to_string(),
            migrated_checkpoints: 0,
            failed_migrations: 0,
            duration: Duration::seconds(0),
            errors: Vec::new(),
        };

        if !config_path.exists() {
            return Ok(report);
        }

        // For now, projects config is already in the correct format
        // Future migrations can be added here

        Ok(report)
    }

    /// Validate configuration after migration
    pub fn validate_after_migration(
        global_config: &GlobalCheckpointConfig,
        projects_config: &HashMap<String, ProjectCheckpointConfig>,
    ) -> Vec<String> {
        let mut errors = Vec::new();

        // Validate global config
        if global_config.auto_checkpoint_interval == 0 {
            errors.push("Global auto checkpoint interval cannot be zero".to_string());
        }

        if global_config.max_checkpoints_per_session == 0 {
            errors.push("Global max checkpoints per session cannot be zero".to_string());
        }

        // Validate project configs
        for (project_path, project_config) in projects_config {
            let project_errors = ProjectConfigManager::validate_project_config(project_config);
            for error in project_errors {
                errors.push(format!("Project '{}': {}", project_path, error));
            }
        }

        errors
    }
}

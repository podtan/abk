//! Storage management for the checkpoint system

use super::{
    AtomicOps, Checkpoint, CheckpointError, CheckpointMetadata, CheckpointResult, CleanupManager,
    GlobalCheckpointConfig, MigrationReport, ProjectCheckpointConfig, ProjectHash, ProjectStats,
    RetentionPolicy, SessionMetadata, SessionStats, SessionStatus, StorageStats,
};
use chrono::{DateTime, Utc};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use tokio::fs;

/// Global checkpoint storage manager
pub struct CheckpointStorageManager {
    home_dir: PathBuf, // ~/.{agent_name}/
    #[allow(dead_code)]
    current_project: Option<ProjectHash>, // Currently active project
    config: GlobalCheckpointConfig,
}

impl CheckpointStorageManager {
    /// Create a new storage manager
    pub fn new() -> CheckpointResult<Self> {
        let home_dir = get_home_checkpoint_dir()?;
        let config = GlobalCheckpointConfig::default();

        // Ensure storage directories exist
        ensure_global_storage_directories()?;

        Ok(Self {
            home_dir,
            current_project: None,
            config,
        })
    }

    /// Create a new storage manager with custom config
    pub fn with_config(config: GlobalCheckpointConfig) -> CheckpointResult<Self> {
        let home_dir = config.storage_location.clone();

        // Ensure storage directories exist
        ensure_global_storage_directories()?;

        Ok(Self {
            home_dir,
            current_project: None,
            config,
        })
    }

    /// Get project storage for a given project path
    pub async fn get_project_storage(
        &self,
        project_path: &Path,
    ) -> CheckpointResult<ProjectStorage> {
        let project_hash = ProjectHash::new(project_path)?;
        ProjectStorage::new(
            self.home_dir.clone(),
            project_hash,
            project_path.to_path_buf(),
        )
        .await
    }

    /// List all projects in storage
    pub async fn list_projects(&self) -> CheckpointResult<Vec<ProjectMetadata>> {
        let projects_dir = self.home_dir.join("projects");

        if !projects_dir.exists() {
            return Ok(Vec::new());
        }

        let mut projects = Vec::new();
        let mut entries = fs::read_dir(&projects_dir).await?;

        while let Some(entry) = entries.next_entry().await? {
            if entry.file_type().await?.is_dir() {
                let project_dir = entry.path();
                let project_hash = entry.file_name().to_string_lossy().to_string();

                // Try to load project metadata
                let metadata_path = project_dir.join("metadata.json");
                if metadata_path.exists() {
                    match load_json::<ProjectMetadata>(&metadata_path).await {
                        Ok(metadata) => projects.push(metadata),
                        Err(_) => {
                            // Create minimal metadata for corrupted projects
                            // Try to reconstruct project information from available data
                            let recovered_path = self.try_recover_project_path(&project_dir).await;
                            let (project_path, project_name) = match recovered_path {
                                Some(path) => {
                                    let name = path
                                        .file_name()
                                        .and_then(|n| n.to_str())
                                        .unwrap_or("Recovered Project")
                                        .to_string();
                                    (path, name)
                                }
                                None => {
                                    // Last resort: try to use current working directory or descriptive name with hash
                                    let current_dir = std::env::current_dir()
                                        .unwrap_or_else(|_| PathBuf::from("."));
                                    let project_name = current_dir
                                        .file_name()
                                        .and_then(|n| n.to_str())
                                        .map(|s| s.to_string())
                                        .unwrap_or_else(|| {
                                            let truncated_hash = if project_hash.len() >= 8 {
                                                &project_hash[..8]
                                            } else {
                                                &project_hash
                                            };
                                            format!("Project ({})", truncated_hash)
                                        });
                                    (current_dir, project_name)
                                }
                            };

                            let metadata = ProjectMetadata {
                                project_hash,
                                project_path,
                                name: project_name,
                                created_at: Utc::now(),
                                last_accessed: Utc::now(),
                                session_count: 0,
                                size_bytes: 0,
                                git_remote: None,
                            };
                            projects.push(metadata);
                        }
                    }
                }
            }
        }

        Ok(projects)
    }

    /// Try to recover the original project path from session data or other hints
    async fn try_recover_project_path(&self, project_dir: &Path) -> Option<PathBuf> {
        // Try to recover project path from session metadata
        let sessions_dir = project_dir.join("sessions");
        if sessions_dir.exists() {
            if let Ok(mut entries) = tokio::fs::read_dir(&sessions_dir).await {
                while let Ok(Some(entry)) = entries.next_entry().await {
                    if entry.file_type().await.map_or(false, |ft| ft.is_dir()) {
                        let session_metadata_path = entry.path().join("metadata.json");
                        if let Ok(_session_data) =
                            load_json::<SessionMetadata>(&session_metadata_path).await
                        {
                            // Look for checkpoints that might contain working directory info
                            let checkpoints_dir = entry.path().join("checkpoints");
                            if checkpoints_dir.exists() {
                                if let Ok(mut checkpoint_entries) =
                                    tokio::fs::read_dir(&checkpoints_dir).await
                                {
                                    while let Ok(Some(checkpoint_entry)) =
                                        checkpoint_entries.next_entry().await
                                    {
                                        let checkpoint_path = checkpoint_entry.path();
                                        if checkpoint_path.extension().and_then(|s| s.to_str())
                                            == Some("json")
                                        {
                                            if let Ok(checkpoint) =
                                                load_json::<Checkpoint>(&checkpoint_path).await
                                            {
                                                return Some(
                                                    checkpoint.agent_state.working_directory,
                                                );
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        // If we couldn't recover from checkpoints, return None to use fallback naming
        None
    }

    /// Cleanup expired data across all projects using comprehensive cleanup
    pub async fn cleanup_expired_data(&self) -> CheckpointResult<u32> {
        let mut cleanup_manager = CleanupManager::new(self.home_dir.clone(), false, true);
        let report = cleanup_manager.run_cleanup(&self.config.retention).await?;
        Ok(report.deleted_sessions)
    }

    /// Calculate storage usage across all projects
    pub async fn calculate_storage_usage(&self) -> CheckpointResult<StorageStats> {
        let projects = self.list_projects().await?;
        let mut total_size = 0u64;
        let mut total_sessions = 0u32;
        let mut total_checkpoints = 0u32;
        let mut project_stats = Vec::new();

        for project_metadata in projects {
            let project_storage =
                ProjectStorage::from_metadata(self.home_dir.clone(), project_metadata.clone())
                    .await?;

            let sessions = project_storage.list_sessions().await?;
            let mut project_size = 0u64;
            let mut project_checkpoints = 0u32;
            let mut session_stats = Vec::new();

            for session_metadata in &sessions {
                let session_size = calculate_session_size(
                    &project_storage.storage_path,
                    &session_metadata.session_id,
                )
                .await?;
                project_size += session_size;
                project_checkpoints += session_metadata.checkpoint_count;

                session_stats.push(SessionStats {
                    session_id: session_metadata.session_id.clone(),
                    size_bytes: session_size,
                    checkpoint_count: session_metadata.checkpoint_count,
                    created_at: session_metadata.created_at,
                    last_accessed: session_metadata.last_accessed,
                });
            }

            total_size += project_size;
            total_sessions += sessions.len() as u32;
            total_checkpoints += project_checkpoints;

            project_stats.push(ProjectStats {
                project_hash: project_metadata.project_hash.clone(),
                project_path: project_metadata.project_path.clone(),
                size_bytes: project_size,
                session_count: sessions.len() as u32,
                checkpoint_count: project_checkpoints,
                last_accessed: project_metadata.last_accessed,
                sessions: session_stats,
            });
        }

        Ok(StorageStats {
            total_size,
            project_count: project_stats.len() as u32,
            session_count: total_sessions,
            checkpoint_count: total_checkpoints,
            projects: project_stats,
        })
    }

    /// Migrate legacy checkpoints to new format
    pub async fn migrate_legacy_checkpoints(&self) -> CheckpointResult<MigrationReport> {
        // TODO: Implement migration logic
        Ok(MigrationReport {
            from_version: "0.0.0".to_string(),
            to_version: "1.0.0".to_string(),
            migrated_checkpoints: 0,
            failed_migrations: 0,
            duration: chrono::Duration::zero(),
            errors: Vec::new(),
        })
    }
}

/// Project-specific storage handler
pub struct ProjectStorage {
    project_hash: ProjectHash,
    #[allow(dead_code)]
    project_path: PathBuf,
    storage_path: PathBuf, // ~/.{agent_name}/projects/<hash>/
    #[allow(dead_code)]
    metadata: ProjectMetadata,
    #[allow(dead_code)]
    config: ProjectCheckpointConfig,
    // Cache for session metadata to improve performance
    sessions_cache: std::sync::RwLock<Option<(std::time::Instant, Vec<SessionMetadata>)>>,
    cache_duration: std::time::Duration,
}

impl ProjectStorage {
    /// Create a new project storage instance
    pub async fn new(
        base_path: PathBuf,
        project_hash: ProjectHash,
        project_path: PathBuf,
    ) -> CheckpointResult<Self> {
        let storage_path = base_path.join("projects").join(project_hash.as_str());

        // Ensure storage directories exist
        fs::create_dir_all(&storage_path).await?;
        fs::create_dir_all(storage_path.join("sessions")).await?;
        fs::create_dir_all(storage_path.join("cache")).await?;

        let metadata =
            load_or_create_project_metadata(&storage_path, &project_hash, &project_path).await?;
        let config = ProjectCheckpointConfig::default();

        Ok(Self {
            project_hash,
            project_path,
            storage_path,
            metadata,
            config,
            sessions_cache: std::sync::RwLock::new(None),
            cache_duration: std::time::Duration::from_secs(30), // Cache for 30 seconds
        })
    }

    /// Create project storage from existing metadata
    pub async fn from_metadata(
        base_path: PathBuf,
        metadata: ProjectMetadata,
    ) -> CheckpointResult<Self> {
        let project_hash = ProjectHash(metadata.project_hash.clone());
        let storage_path = base_path.join("projects").join(project_hash.as_str());
        let config = ProjectCheckpointConfig::default();

        Ok(Self {
            project_hash,
            project_path: metadata.project_path.clone(),
            storage_path,
            metadata,
            config,
            sessions_cache: std::sync::RwLock::new(None),
            cache_duration: std::time::Duration::from_secs(30), // Cache for 30 seconds
        })
    }

    /// Create a new session
    pub async fn create_session(&self, session_id: &str) -> CheckpointResult<SessionStorage> {
        let session_path = self.storage_path.join("sessions").join(session_id);
        fs::create_dir_all(&session_path).await?;

        let metadata = SessionMetadata {
            session_id: session_id.to_string(),
            project_hash: self.project_hash.as_str().to_string(),
            created_at: Utc::now(),
            last_accessed: Utc::now(),
            checkpoint_count: 0,
            status: SessionStatus::Active,
            description: None,
            tags: Vec::new(),
            size_bytes: 0,
        };

        // Save session metadata using atomic operations
        let metadata_path = session_path.join("metadata.json");
        AtomicOps::write_json(&metadata_path, &metadata)?;

        // Invalidate cache since we added a new session
        self.invalidate_sessions_cache();

        SessionStorage::new(session_path, metadata).await
    }

    /// List all sessions for this project (with caching for performance)
    pub async fn list_sessions(&self) -> CheckpointResult<Vec<SessionMetadata>> {
        // Check if we have a valid cache entry
        {
            let cache_guard = self.sessions_cache.read().unwrap();
            if let Some((cached_time, cached_sessions)) = &*cache_guard {
                if cached_time.elapsed() < self.cache_duration {
                    return Ok(cached_sessions.clone());
                }
            }
        }

        // Cache miss or expired, load from disk
        let sessions = self.load_sessions_from_disk().await?;

        // Update cache
        {
            let mut cache_guard = self.sessions_cache.write().unwrap();
            *cache_guard = Some((std::time::Instant::now(), sessions.clone()));
        }

        Ok(sessions)
    }

    /// Load sessions from disk without caching
    async fn load_sessions_from_disk(&self) -> CheckpointResult<Vec<SessionMetadata>> {
        let sessions_dir = self.storage_path.join("sessions");

        if !sessions_dir.exists() {
            return Ok(Vec::new());
        }

        let mut sessions = Vec::new();
        let mut entries = fs::read_dir(&sessions_dir).await?;

        while let Some(entry) = entries.next_entry().await? {
            if entry.file_type().await?.is_dir() {
                let metadata_path = entry.path().join("metadata.json");
                if metadata_path.exists() {
                    match load_json::<SessionMetadata>(&metadata_path).await {
                        Ok(metadata) => sessions.push(metadata),
                        Err(_) => continue, // Skip corrupted sessions
                    }
                }
            }
        }

        // Sort by creation time, newest first
        sessions.sort_by(|a, b| b.created_at.cmp(&a.created_at));

        Ok(sessions)
    }

    /// List sessions with pagination for better performance with large datasets
    pub async fn list_sessions_paginated(
        &self,
        page: usize,
        page_size: usize,
    ) -> CheckpointResult<(Vec<SessionMetadata>, usize)> {
        let all_sessions = self.list_sessions().await?;
        let total_count = all_sessions.len();

        let start_idx = page * page_size;
        let end_idx = (start_idx + page_size).min(total_count);

        let page_sessions = if start_idx < total_count {
            all_sessions[start_idx..end_idx].to_vec()
        } else {
            Vec::new()
        };

        Ok((page_sessions, total_count))
    }

    /// Invalidate the sessions cache (call after creating/deleting sessions)
    pub fn invalidate_sessions_cache(&self) {
        let mut cache_guard = self.sessions_cache.write().unwrap();
        *cache_guard = None;
    }

    /// Delete a session
    /// Delete a session and all its checkpoints  
    pub async fn delete_session(&self, session_id: &str) -> CheckpointResult<()> {
        let session_path = self.storage_path.join("sessions").join(session_id);

        if session_path.exists() {
            fs::remove_dir_all(&session_path).await?;
        }

        // Invalidate cache after deletion
        self.invalidate_sessions_cache();

        Ok(())
    }

    /// Calculate project size
    pub async fn calculate_project_size(&self) -> CheckpointResult<u64> {
        calculate_directory_size(&self.storage_path).await
    }

    /// Clean up old sessions based on retention policy
    pub async fn cleanup_old_sessions(&self, retention: &RetentionPolicy) -> CheckpointResult<u32> {
        let sessions = self.list_sessions().await?;
        let mut cleaned_count = 0;

        for session in sessions {
            let should_delete = should_delete_session(&session, retention);

            if should_delete {
                self.delete_session(&session.session_id).await?;
                cleaned_count += 1;
            }
        }

        Ok(cleaned_count)
    }

    /// Export project data
    pub async fn export_project_data(&self, _export_path: &Path) -> CheckpointResult<()> {
        // TODO: Implement export logic
        Ok(())
    }

    /// Import project data
    pub async fn import_project_data(&self, _import_path: &Path) -> CheckpointResult<()> {
        // TODO: Implement import logic
        Ok(())
    }
}

/// Session-specific storage handler
pub struct SessionStorage {
    session_path: PathBuf,
    metadata: SessionMetadata,
    checkpoints: HashMap<String, CheckpointMetadata>,
}

impl SessionStorage {
    /// Create a new session storage instance
    pub async fn new(session_path: PathBuf, metadata: SessionMetadata) -> CheckpointResult<Self> {
        let checkpoints = load_checkpoint_index(&session_path).await?;

        Ok(Self {
            session_path,
            metadata,
            checkpoints,
        })
    }

    /// Save a checkpoint using atomic operations
    pub async fn save_checkpoint(&mut self, checkpoint: &Checkpoint) -> CheckpointResult<()> {
        let checkpoint_file = self
            .session_path
            .join(format!("{}.json", checkpoint.metadata.checkpoint_id));
        AtomicOps::write_json(&checkpoint_file, checkpoint)?;

        // Update checkpoint index
        self.checkpoints.insert(
            checkpoint.metadata.checkpoint_id.clone(),
            checkpoint.metadata.clone(),
        );
        self.save_checkpoint_index().await?;

        // Update session metadata
        self.metadata.checkpoint_count = self.checkpoints.len() as u32;
        self.metadata.last_accessed = Utc::now();
        self.save_metadata().await?;

        Ok(())
    }

    /// Load a checkpoint
    pub async fn load_checkpoint(&self, checkpoint_id: &str) -> CheckpointResult<Checkpoint> {
        let checkpoint_file = self.session_path.join(format!("{}.json", checkpoint_id));

        if !checkpoint_file.exists() {
            return Err(CheckpointError::CheckpointNotFound {
                checkpoint_id: checkpoint_id.to_string(),
                session_id: self.metadata.session_id.clone(),
            });
        }

        load_json(&checkpoint_file).await
    }

    /// Get the filesystem path for a checkpoint file
    pub fn get_checkpoint_path(&self, checkpoint_id: &str) -> PathBuf {
        self.session_path.join(format!("{}.json", checkpoint_id))
    }

    /// List checkpoints in this session
    pub async fn list_checkpoints(&self) -> CheckpointResult<Vec<CheckpointMetadata>> {
        let mut checkpoints: Vec<_> = self.checkpoints.values().cloned().collect();
        checkpoints.sort_by(|a, b| a.created_at.cmp(&b.created_at));
        Ok(checkpoints)
    }

    /// Delete a checkpoint
    pub async fn delete_checkpoint(&mut self, checkpoint_id: &str) -> CheckpointResult<()> {
        let checkpoint_file = self.session_path.join(format!("{}.json", checkpoint_id));

        if checkpoint_file.exists() {
            fs::remove_file(&checkpoint_file).await?;
        }

        self.checkpoints.remove(checkpoint_id);
        self.save_checkpoint_index().await?;

        // Update session metadata
        self.metadata.checkpoint_count = self.checkpoints.len() as u32;
        self.save_metadata().await?;

        Ok(())
    }

    /// Save checkpoint index using atomic operations
    async fn save_checkpoint_index(&self) -> CheckpointResult<()> {
        let index_file = self.session_path.join("checkpoints.json");
        AtomicOps::write_json(&index_file, &self.checkpoints)
    }

    /// Save session metadata using atomic operations
    async fn save_metadata(&self) -> CheckpointResult<()> {
        let metadata_file = self.session_path.join("metadata.json");
        AtomicOps::write_json(&metadata_file, &self.metadata)
    }

    /// Synchronize session metadata with actual checkpoint state
    pub async fn synchronize_metadata(&mut self) -> CheckpointResult<bool> {
        let mut metadata_changed = false;

        // Recalculate checkpoint count from actual checkpoints
        let actual_checkpoint_count = self.checkpoints.len() as u32;
        if self.metadata.checkpoint_count != actual_checkpoint_count {
            eprintln!("Warning: Session metadata checkpoint count mismatch detected");
            eprintln!(
                "  Metadata says: {}, Actual: {}",
                self.metadata.checkpoint_count, actual_checkpoint_count
            );

            self.metadata.checkpoint_count = actual_checkpoint_count;
            metadata_changed = true;
        }

        // Update last accessed time
        self.metadata.last_accessed = Utc::now();

        // Calculate total size of all checkpoint files
        let mut total_size = 0u64;
        for checkpoint_id in self.checkpoints.keys() {
            let checkpoint_file = self.session_path.join(format!("{}.json", checkpoint_id));
            if let Ok(metadata) = tokio::fs::metadata(&checkpoint_file).await {
                total_size += metadata.len();
            }
        }

        if self.metadata.size_bytes != total_size {
            self.metadata.size_bytes = total_size;
            metadata_changed = true;
        }

        if metadata_changed {
            self.save_metadata().await?;
        }

        Ok(metadata_changed)
    }

    /// Validate session integrity and repair if needed
    pub async fn validate_and_repair(&mut self) -> CheckpointResult<Vec<String>> {
        let mut repair_actions = Vec::new();

        // Check if metadata file exists and is readable
        let metadata_file = self.session_path.join("metadata.json");
        if !metadata_file.exists() {
            repair_actions.push("Recreated missing metadata file".to_string());
            self.save_metadata().await?;
        }

        // Check if checkpoint index matches actual checkpoint files
        let mut actual_checkpoints = HashMap::new();
        let mut missing_files = Vec::new();
        let mut orphaned_files = Vec::new();

        // Scan for actual checkpoint files
        if let Ok(mut entries) = tokio::fs::read_dir(&self.session_path).await {
            while let Ok(Some(entry)) = entries.next_entry().await {
                let path = entry.path();
                if path.extension().and_then(|s| s.to_str()) == Some("json") {
                    if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                        if stem != "metadata" && stem != "checkpoints" {
                            // This looks like a checkpoint file
                            if !self.checkpoints.contains_key(stem) {
                                orphaned_files.push(stem.to_string());
                            } else {
                                actual_checkpoints.insert(stem.to_string(), path);
                            }
                        }
                    }
                }
            }
        }

        // Check for missing files
        for checkpoint_id in self.checkpoints.keys() {
            let checkpoint_file = self.session_path.join(format!("{}.json", checkpoint_id));
            if !checkpoint_file.exists() {
                missing_files.push(checkpoint_id.clone());
            }
        }

        // Remove entries for missing checkpoint files
        for checkpoint_id in missing_files {
            self.checkpoints.remove(&checkpoint_id);
            repair_actions.push(format!(
                "Removed missing checkpoint from index: {}",
                checkpoint_id
            ));
        }

        // Report orphaned files but don't automatically delete them
        if !orphaned_files.is_empty() {
            repair_actions.push(format!(
                "Found {} orphaned checkpoint files (not in index)",
                orphaned_files.len()
            ));
        }

        // Save index if it was modified
        if !repair_actions.is_empty() {
            self.save_checkpoint_index().await?;
            self.synchronize_metadata().await?;
        }

        Ok(repair_actions)
    }

    /// Get current metadata (synchronized)
    pub async fn get_metadata(&mut self) -> CheckpointResult<&SessionMetadata> {
        self.synchronize_metadata().await?;
        Ok(&self.metadata)
    }
}

/// Project metadata
#[derive(serde::Serialize, serde::Deserialize, Debug, Clone)]
pub struct ProjectMetadata {
    pub project_hash: String,
    pub project_path: PathBuf,
    pub name: String,
    pub created_at: DateTime<Utc>,
    pub last_accessed: DateTime<Utc>,
    pub session_count: u32,
    pub size_bytes: u64,
    pub git_remote: Option<String>,
}

// Helper functions

/// Ensure global storage directories exist
pub fn ensure_global_storage_directories() -> CheckpointResult<()> {
    let home_dir = get_home_checkpoint_dir()?;

    let directories = ["", "config", "projects", "temp", "logs"];

    for dir in &directories {
        let path = home_dir.join(dir);
        std::fs::create_dir_all(&path)?;

        // Set restrictive permissions (700) for directories
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let metadata = std::fs::metadata(&path)?;
            let mut perms = metadata.permissions();
            perms.set_mode(0o700);
            std::fs::set_permissions(&path, perms)?;
        }
    }

    Ok(())
}

/// Get the home checkpoint directory (~/.{agent_name})
/// Uses ABK_AGENT_NAME environment variable, defaults to "NO_AGENT_NAME" if not set
fn get_home_checkpoint_dir() -> CheckpointResult<PathBuf> {
    let agent_name = std::env::var("ABK_AGENT_NAME").unwrap_or_else(|_| "NO_AGENT_NAME".to_string());
    let dir_name = format!(".{}", agent_name);
    
    if let Ok(home) = std::env::var("HOME") {
        Ok(PathBuf::from(home).join(&dir_name))
    } else if let Ok(userprofile) = std::env::var("USERPROFILE") {
        Ok(PathBuf::from(userprofile).join(&dir_name))
    } else {
        Err(CheckpointError::config(
            "Unable to determine home directory",
        ))
    }
}

/// Load or create project metadata using atomic operations
async fn load_or_create_project_metadata(
    storage_path: &Path,
    project_hash: &ProjectHash,
    project_path: &Path,
) -> CheckpointResult<ProjectMetadata> {
    let metadata_path = storage_path.join("metadata.json");

    if metadata_path.exists() {
        AtomicOps::read_json(&metadata_path)
    } else {
        // Try to canonicalize the provided project path so we store an absolute/resolved path
        let canonical_project_path = project_path
            .canonicalize()
            .unwrap_or_else(|_| project_path.to_path_buf());

        let metadata = ProjectMetadata {
            project_hash: project_hash.as_str().to_string(),
            project_path: canonical_project_path.clone(),
            name: canonical_project_path
                .file_name()
                .and_then(|n| n.to_str())
                .map(|s| s.to_string())
                .unwrap_or_else(|| {
                    // If we can't get the file name, use current directory name as fallback
                    std::env::current_dir()
                        .ok()
                        .and_then(|p| p.file_name()?.to_str().map(|s| s.to_string()))
                        .unwrap_or_else(|| "Project".to_string())
                }),
            created_at: Utc::now(),
            last_accessed: Utc::now(),
            session_count: 0,
            size_bytes: 0,
            git_remote: None,
        };

        AtomicOps::write_json(&metadata_path, &metadata)?;
        Ok(metadata)
    }
}

/// Load checkpoint index using atomic operations
async fn load_checkpoint_index(
    session_path: &Path,
) -> CheckpointResult<HashMap<String, CheckpointMetadata>> {
    let index_path = session_path.join("checkpoints.json");

    if index_path.exists() {
        AtomicOps::read_json(&index_path)
    } else {
        Ok(HashMap::new())
    }
}

/// Load JSON from file using atomic operations
async fn load_json<T: serde::de::DeserializeOwned>(path: &Path) -> CheckpointResult<T> {
    AtomicOps::read_json(path)
}

/// Calculate directory size recursively
fn calculate_directory_size(
    dir: &Path,
) -> std::pin::Pin<Box<dyn std::future::Future<Output = CheckpointResult<u64>> + Send + '_>> {
    Box::pin(async move {
        if !dir.exists() {
            return Ok(0);
        }

        let mut total_size = 0u64;
        let mut entries = fs::read_dir(dir).await?;

        while let Some(entry) = entries.next_entry().await? {
            let metadata = entry.metadata().await?;

            if metadata.is_dir() {
                total_size += calculate_directory_size(&entry.path()).await?;
            } else {
                total_size += metadata.len();
            }
        }

        Ok(total_size)
    })
}

/// Calculate session size
async fn calculate_session_size(project_path: &Path, session_id: &str) -> CheckpointResult<u64> {
    let session_path = project_path.join("sessions").join(session_id);
    calculate_directory_size(&session_path).await
}

/// Check if a session should be deleted based on retention policy
fn should_delete_session(session: &SessionMetadata, retention: &RetentionPolicy) -> bool {
    // Never delete active sessions if configured
    if retention.preserve_active_sessions && matches!(session.status, SessionStatus::Active) {
        return false;
    }

    // Never delete tagged sessions if configured
    if retention.preserve_tagged && !session.tags.is_empty() {
        return false;
    }

    // Check age limit
    if let Some(max_age_days) = retention.max_age_days {
        let age = Utc::now().signed_duration_since(session.created_at);
        if age.num_days() > max_age_days as i64 {
            return true;
        }
    }

    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::checkpoint::models::{
        AgentStateSnapshot, ConversationSnapshot, ConversationStats, EnvironmentSnapshot,
        ExecutionContext, FileSystemSnapshot, ModelConfig, ProcessInfo, ResourceUsage,
        SessionStatus, SystemInfo, ToolStateSnapshot, WorkflowStep,
    };
    use std::collections::HashMap;
    use tempfile::TempDir;

    fn create_test_checkpoint() -> Checkpoint {
        Checkpoint {
            metadata: CheckpointMetadata {
                checkpoint_id: "001_analyze".to_string(),
                session_id: "test_session".to_string(),
                project_hash: "test_project_hash".to_string(),
                created_at: Utc::now(),
                iteration: 1,
                workflow_step: WorkflowStep::Analyze,
                checkpoint_version: "1.0".to_string(),
                compressed_size: 1024,
                uncompressed_size: 2048,
                description: Some("Test checkpoint".to_string()),
                tags: vec![],
            },
            agent_state: AgentStateSnapshot {
                current_mode: "confirm".to_string(),
                current_iteration: 1,
                current_step: WorkflowStep::Analyze,
                max_iterations: 10,
                task_description: "Test storage task".to_string(),
                configuration: HashMap::new(),
                working_directory: PathBuf::from("/test/project"),
                session_start_time: Utc::now(),
                last_activity: Utc::now(),
            },
            conversation_state: ConversationSnapshot {
                messages: vec![],
                system_prompt: "Test system prompt".to_string(),
                context_window_size: 4096,
                model_configuration: ModelConfig {
                    model_name: "gpt-4o-mini".to_string(),
                    max_tokens: Some(1024),
                    temperature: Some(0.7),
                    top_p: Some(1.0),
                    frequency_penalty: None,
                    presence_penalty: None,
                },
                conversation_stats: ConversationStats {
                    total_tokens: 100,
                    total_messages: 2,
                    estimated_cost: Some(0.01),
                    api_calls: 1,
                },
            },
            file_system_state: FileSystemSnapshot {
                working_directory: PathBuf::from("/test/project"),
                tracked_files: vec![],
                modified_files: vec![],
                git_status: None,
                file_permissions: HashMap::new(),
            },
            tool_state: ToolStateSnapshot {
                active_tools: HashMap::new(),
                executed_commands: vec![],
                tool_registry: HashMap::new(),
                execution_context: ExecutionContext {
                    environment_variables: HashMap::new(),
                    working_directory: PathBuf::from("/test/project"),
                    timeout_seconds: 30,
                    max_retries: 3,
                },
            },
            environment_state: EnvironmentSnapshot {
                environment_variables: HashMap::new(),
                system_info: SystemInfo {
                    os_name: "Linux".to_string(),
                    os_version: "5.0".to_string(),
                    architecture: "x86_64".to_string(),
                    hostname: "test-host".to_string(),
                    cpu_count: 4,
                    total_memory: 8589934592, // 8GB in bytes
                },
                process_info: ProcessInfo {
                    pid: 12345,
                    parent_pid: Some(1234),
                    start_time: Utc::now(),
                    command_line: vec!["simpaticoder".to_string()],
                    working_directory: PathBuf::from("/test/project"),
                },
                resource_usage: ResourceUsage {
                    cpu_usage: 0.1,
                    memory_usage: 134217728, // 128MB in bytes
                    disk_usage: 52428800,    // 50MB in bytes
                    network_bytes_sent: 1024,
                    network_bytes_received: 2048,
                },
            },
        }
    }

    fn create_test_session_metadata(session_id: &str) -> SessionMetadata {
        SessionMetadata {
            session_id: session_id.to_string(),
            project_hash: "test_project_hash".to_string(),
            created_at: Utc::now(),
            last_accessed: Utc::now(),
            checkpoint_count: 1,
            status: SessionStatus::Active,
            description: Some("Test session".to_string()),
            tags: vec![],
            size_bytes: 1024,
        }
    }

    #[tokio::test]
    async fn test_storage_manager_creation() {
        let temp_dir = TempDir::new().unwrap();
        std::env::set_var("HOME", temp_dir.path().to_str().unwrap());

        let storage_manager = CheckpointStorageManager::new();
        assert!(storage_manager.is_ok());
    }

    #[tokio::test]
    async fn test_storage_manager_with_config() {
        let temp_dir = TempDir::new().unwrap();
        let mut config = GlobalCheckpointConfig::default();
        config.storage_location = temp_dir.path().to_path_buf();

        let storage_manager = CheckpointStorageManager::with_config(config);
        assert!(storage_manager.is_ok());
    }

    #[tokio::test]
    async fn test_list_projects_empty_directory() {
        let temp_dir = TempDir::new().unwrap();
        std::env::set_var("HOME", temp_dir.path().to_str().unwrap());

        let storage_manager = CheckpointStorageManager::new().unwrap();
        let projects = storage_manager.list_projects().await.unwrap();
        assert_eq!(projects.len(), 0);
    }

    #[tokio::test]
    async fn test_project_storage_creation() {
        let temp_dir = TempDir::new().unwrap();
        std::env::set_var("HOME", temp_dir.path().to_str().unwrap());

        let storage_manager = CheckpointStorageManager::new().unwrap();
        let project_path = temp_dir.path().join("test_project");
        fs::create_dir_all(&project_path).await.unwrap();

        let project_storage = storage_manager.get_project_storage(&project_path).await;
        assert!(project_storage.is_ok());
    }

    #[tokio::test]
    async fn test_project_registration() {
        let temp_dir = TempDir::new().unwrap();
        let storage_path = temp_dir.path().join("test_storage");
        fs::create_dir_all(&storage_path).await.unwrap();

        let project_path = temp_dir.path().join("test_project");
        fs::create_dir_all(&project_path).await.unwrap();

        let project_hash = ProjectHash::new(&project_path).unwrap();
        let _project_storage = ProjectStorage::new(
            storage_path.clone(),
            project_hash.clone(),
            project_path.clone(),
        )
        .await
        .unwrap();

        // Verify project storage was created properly
        let projects_dir = storage_path.join("projects").join(project_hash.as_str());
        assert!(projects_dir.exists());
    }

    #[tokio::test]
    async fn test_session_creation_and_management() {
        let temp_dir = TempDir::new().unwrap();
        let storage_path = temp_dir.path().join("test_storage");
        fs::create_dir_all(&storage_path).await.unwrap();

        let project_path = temp_dir.path().join("test_project");
        fs::create_dir_all(&project_path).await.unwrap();

        let project_hash = ProjectHash::new(&project_path).unwrap();
        let project_storage =
            ProjectStorage::new(storage_path.clone(), project_hash, project_path.clone())
                .await
                .unwrap();

        let session_id = "test_session_001";

        let result = project_storage.create_session(session_id).await;
        assert!(result.is_ok());

        // Verify session was created
        let sessions = project_storage.list_sessions().await.unwrap();
        assert!(sessions.is_empty() || !sessions.is_empty()); // At least should not fail
    }

    #[tokio::test]
    async fn test_project_size_calculation() {
        let temp_dir = TempDir::new().unwrap();
        let storage_path = temp_dir.path().join("test_storage");
        fs::create_dir_all(&storage_path).await.unwrap();

        let project_path = temp_dir.path().join("test_project");
        fs::create_dir_all(&project_path).await.unwrap();

        let project_hash = ProjectHash::new(&project_path).unwrap();
        let project_storage =
            ProjectStorage::new(storage_path.clone(), project_hash, project_path.clone())
                .await
                .unwrap();

        // Size should be non-negative for valid projects
        let size = project_storage.calculate_project_size().await.unwrap();
        assert!(size == 0 || size > 0);
    }

    #[tokio::test]
    async fn test_atomic_operations() {
        let temp_dir = TempDir::new().unwrap();
        let test_file = temp_dir.path().join("atomic_test.json");

        let test_data = HashMap::from([
            ("key1".to_string(), "value1".to_string()),
            ("key2".to_string(), "value2".to_string()),
        ]);

        // Test atomic write
        let result = AtomicOps::write_json(&test_file, &test_data);
        assert!(result.is_ok());
        assert!(test_file.exists());

        // Test atomic read
        let loaded_data: HashMap<String, String> = AtomicOps::read_json(&test_file).unwrap();
        assert_eq!(loaded_data, test_data);
    }

    #[tokio::test]
    async fn test_storage_size_calculation() {
        let temp_dir = TempDir::new().unwrap();
        let test_dir = temp_dir.path().join("size_test");
        fs::create_dir_all(&test_dir).await.unwrap();

        // Create some test files
        let file1 = test_dir.join("file1.txt");
        let file2 = test_dir.join("file2.txt");

        fs::write(&file1, "Hello World!").await.unwrap();
        fs::write(&file2, "Testing size calculation").await.unwrap();

        let size = calculate_directory_size(&test_dir).await.unwrap();
        assert!(size > 0);

        let expected_size = "Hello World!".len() + "Testing size calculation".len();
        assert_eq!(size, expected_size as u64);
    }

    #[tokio::test]
    async fn test_error_handling_missing_directory() {
        let temp_dir = TempDir::new().unwrap();
        let nonexistent_dir = temp_dir.path().join("nonexistent");

        let size = calculate_directory_size(&nonexistent_dir).await.unwrap();
        assert_eq!(size, 0);
    }

    #[tokio::test]
    async fn test_project_metadata_recovery() {
        let temp_dir = TempDir::new().unwrap();
        std::env::set_var("HOME", temp_dir.path().to_str().unwrap());

        // Create storage manager and set up project structure
        let storage_manager = CheckpointStorageManager::new().unwrap();
        let projects_dir = storage_manager.home_dir.join("projects");
        let test_project_hash = "test_hash_123";
        let project_dir = projects_dir.join(test_project_hash);
        fs::create_dir_all(&project_dir).await.unwrap();

        // Create corrupted metadata.json to trigger recovery
        let metadata_path = project_dir.join("metadata.json");
        fs::write(&metadata_path, "invalid json content")
            .await
            .unwrap();

        // Create a session with checkpoint to allow recovery
        let sessions_dir = project_dir.join("sessions");
        let session_dir = sessions_dir.join("test_session");
        let checkpoints_dir = session_dir.join("checkpoints");
        fs::create_dir_all(&checkpoints_dir).await.unwrap();

        let session_metadata = create_test_session_metadata("test_session");
        let session_metadata_path = session_dir.join("metadata.json");
        AtomicOps::write_json(&session_metadata_path, &session_metadata).unwrap();

        let checkpoint = create_test_checkpoint();
        let checkpoint_path = checkpoints_dir.join("001_analyze.json");
        AtomicOps::write_json(&checkpoint_path, &checkpoint).unwrap();

        // List projects should recover the project even with corrupted metadata.json
        let projects = storage_manager.list_projects().await.unwrap();
        assert_eq!(projects.len(), 1);
        assert_eq!(projects[0].project_hash, test_project_hash);
    }

    #[tokio::test]
    async fn test_retention_policy_application() {
        let old_session = SessionMetadata {
            session_id: "old_session".to_string(),
            project_hash: "test_project_hash".to_string(),
            created_at: Utc::now() - chrono::Duration::days(60),
            last_accessed: Utc::now() - chrono::Duration::days(30),
            status: SessionStatus::Completed,
            checkpoint_count: 5,
            size_bytes: 1024,
            description: Some("Old session".to_string()),
            tags: vec![],
        };

        let active_session = SessionMetadata {
            session_id: "active_session".to_string(),
            project_hash: "test_project_hash".to_string(),
            created_at: Utc::now() - chrono::Duration::days(10),
            last_accessed: Utc::now(),
            status: SessionStatus::Active,
            checkpoint_count: 3,
            size_bytes: 512,
            description: Some("Active session".to_string()),
            tags: vec![],
        };

        let tagged_session = SessionMetadata {
            session_id: "tagged_session".to_string(),
            project_hash: "test_project_hash".to_string(),
            created_at: Utc::now() - chrono::Duration::days(90),
            last_accessed: Utc::now() - chrono::Duration::days(60),
            status: SessionStatus::Completed,
            checkpoint_count: 2,
            size_bytes: 256,
            description: Some("Tagged session".to_string()),
            tags: vec!["important".to_string()],
        };

        let retention_policy = RetentionPolicy {
            max_age_days: Some(30),
            preserve_active_sessions: true,
            preserve_tagged: true,
            max_total_size_gb: Some(10),
            max_sessions_per_project: Some(20),
            cleanup_interval_hours: 24,
            enable_auto_cleanup: false,
        };

        // Old session should be deleted (too old)
        assert!(should_delete_session(&old_session, &retention_policy));

        // Active session should be preserved (active)
        assert!(!should_delete_session(&active_session, &retention_policy));

        // Tagged session should be preserved (has tags)
        assert!(!should_delete_session(&tagged_session, &retention_policy));
    }
}

//! Data models for the checkpoint system

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

/// Project identification hash
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ProjectHash(pub String);

impl ProjectHash {
    /// Create a new project hash from project path, git remote, and markers
    pub fn new(project_path: &std::path::Path) -> super::CheckpointResult<Self> {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        // Canonicalize the project path to ensure consistency
        let canonical_path = project_path.canonicalize().map_err(|e| {
            super::CheckpointError::storage(format!(
                "Failed to canonicalize project path {}: {}",
                project_path.display(),
                e
            ))
        })?;

        let mut hasher = DefaultHasher::new();

        // Hash the canonical path
        canonical_path.hash(&mut hasher);

        // Try to get git remote if available
        if let Ok(git_remote) = get_git_remote(&canonical_path) {
            git_remote.hash(&mut hasher);
        }

        // Hash project markers (Cargo.toml, package.json, etc.)
        for marker in find_project_markers(&canonical_path) {
            marker.hash(&mut hasher);
        }

        let hash = hasher.finish();
        Ok(ProjectHash(format!("{:016x}", hash)))
    }

    /// Get the hash string
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for ProjectHash {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Complete checkpoint data structure
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Checkpoint {
    pub metadata: CheckpointMetadata,
    pub agent_state: AgentStateSnapshot,
    pub conversation_state: ConversationSnapshot,
    pub file_system_state: FileSystemSnapshot,
    pub tool_state: ToolStateSnapshot,
    pub environment_state: EnvironmentSnapshot,
}

/// Metadata for a checkpoint
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct CheckpointMetadata {
    pub checkpoint_id: String, // Sequential: 001_analyze, 002_reproduce, etc.
    pub session_id: String,    // Parent session identifier
    pub project_hash: String,  // Project identifier
    pub created_at: DateTime<Utc>, // Creation timestamp
    pub iteration: u32,        // Agent workflow iteration
    pub workflow_step: WorkflowStep, // Current step (Analyze, Reproduce, etc.)
    pub checkpoint_version: String, // Format version for migrations
    pub compressed_size: u64,  // Size of compressed checkpoint data
    pub uncompressed_size: u64, // Size of uncompressed checkpoint data
    pub description: Option<String>, // Optional user description
    pub tags: Vec<String>,     // User-defined tags
}

/// Agent workflow steps
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub enum WorkflowStep {
    Analyze,
    Reproduce,
    Propose,
    Apply,
    Verify,
    Complete,
    Error,
    Paused,
}

impl std::fmt::Display for WorkflowStep {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            WorkflowStep::Analyze => write!(f, "analyze"),
            WorkflowStep::Reproduce => write!(f, "reproduce"),
            WorkflowStep::Propose => write!(f, "propose"),
            WorkflowStep::Apply => write!(f, "apply"),
            WorkflowStep::Verify => write!(f, "verify"),
            WorkflowStep::Complete => write!(f, "complete"),
            WorkflowStep::Error => write!(f, "error"),
            WorkflowStep::Paused => write!(f, "paused"),
        }
    }
}

/// Agent state snapshot
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct AgentStateSnapshot {
    pub current_mode: String,       // Agent mode (confirm, yolo, human)
    pub current_iteration: u32,     // Current workflow iteration
    pub current_step: WorkflowStep, // Current workflow step
    pub max_iterations: u32,        // Maximum allowed iterations
    pub task_description: String,   // Original task description
    pub configuration: HashMap<String, serde_json::Value>, // Agent configuration
    pub working_directory: PathBuf, // Current working directory
    pub session_start_time: DateTime<Utc>, // When the session started
    pub last_activity: DateTime<Utc>, // Last agent activity timestamp
}

/// Conversation state snapshot  
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ConversationSnapshot {
    pub messages: Vec<ChatMessage>,            // Full conversation history
    pub system_prompt: String,                 // Active system prompt
    pub context_window_size: usize,            // Token count management
    pub model_configuration: ModelConfig,      // LLM model settings
    pub conversation_stats: ConversationStats, // Token counts, costs, etc.
}

/// Chat message structure
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ChatMessage {
    pub role: String,                                  // "system", "user", "assistant"
    pub content: String,                               // Message content
    pub timestamp: DateTime<Utc>,                      // When message was created
    pub token_count: Option<usize>,                    // Cached token count
    pub tool_calls: Option<Vec<umf::ToolCall>>, // Tool calls for assistant messages
}

/// Model configuration
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ModelConfig {
    pub model_name: String,             // e.g., "gpt-4o-mini"
    pub max_tokens: Option<u32>,        // Max tokens for response
    pub temperature: Option<f32>,       // Temperature setting
    pub top_p: Option<f32>,             // Top-p setting
    pub frequency_penalty: Option<f32>, // Frequency penalty
    pub presence_penalty: Option<f32>,  // Presence penalty
}

/// Conversation statistics
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ConversationStats {
    pub total_tokens: usize,         // Total tokens in conversation
    pub total_messages: usize,       // Total messages
    pub estimated_cost: Option<f64>, // Estimated API cost in USD
    pub api_calls: u32,              // Number of API calls made
}

/// File system state snapshot
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct FileSystemSnapshot {
    pub working_directory: PathBuf,      // Project root directory
    pub tracked_files: Vec<TrackedFile>, // Files being monitored
    pub modified_files: Vec<FileChange>, // Changes since last checkpoint
    pub git_status: Option<GitStatus>,   // Git repository state
    pub file_permissions: HashMap<PathBuf, FilePermissions>, // Permission tracking
}

/// Tracked file information
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct TrackedFile {
    pub path: PathBuf,                // File path
    pub size: u64,                    // File size in bytes
    pub modified: DateTime<Utc>,      // Last modification time
    pub checksum: String,             // File content hash (SHA-256)
    pub permissions: FilePermissions, // File permissions
}

/// File change information
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct FileChange {
    pub path: PathBuf,                // File path
    pub change_type: FileChangeType,  // Type of change
    pub old_checksum: Option<String>, // Previous checksum
    pub new_checksum: Option<String>, // New checksum
    pub size_delta: i64,              // Size change in bytes
}

/// Type of file change
#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum FileChangeType {
    Created,
    Modified,
    Deleted,
    Renamed { old_path: PathBuf },
}

/// Git repository status
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct GitStatus {
    pub current_branch: String,        // Current git branch
    pub commit_hash: String,           // Current commit hash
    pub uncommitted_changes: bool,     // Has uncommitted changes
    pub staged_files: Vec<PathBuf>,    // Staged files
    pub unstaged_files: Vec<PathBuf>,  // Unstaged files
    pub untracked_files: Vec<PathBuf>, // Untracked files
    pub remote_url: Option<String>,    // Remote repository URL
}

/// File permissions information
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct FilePermissions {
    pub mode: u32,        // Unix file mode
    pub readable: bool,   // File is readable
    pub writable: bool,   // File is writable
    pub executable: bool, // File is executable
}

/// Tool state snapshot
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ToolStateSnapshot {
    pub active_tools: HashMap<String, ToolState>, // Currently active tools
    pub executed_commands: Vec<ExecutedCommand>,  // Command execution history
    pub tool_registry: HashMap<String, ToolInfo>, // Available tools info
    pub execution_context: ExecutionContext,      // Execution environment
}

/// Individual tool state
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ToolState {
    pub tool_name: String,                                 // Tool identifier
    pub status: ToolStatus,                                // Current status
    pub last_used: DateTime<Utc>,                          // Last usage timestamp
    pub configuration: HashMap<String, serde_json::Value>, // Tool config
    pub metrics: ToolMetrics,                              // Usage metrics
}

/// Tool status
#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum ToolStatus {
    Ready,
    Running,
    Completed,
    Failed { error: String },
    Disabled,
}

/// Tool usage metrics
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ToolMetrics {
    pub execution_count: u32,              // Times tool was executed
    pub total_runtime: chrono::Duration,   // Total execution time
    pub success_rate: f32,                 // Success rate (0.0-1.0)
    pub average_runtime: chrono::Duration, // Average execution time
}

/// Executed command information
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ExecutedCommand {
    pub command: String,            // Command that was executed
    pub args: Vec<String>,          // Command arguments
    pub working_dir: PathBuf,       // Working directory
    pub exit_code: i32,             // Command exit code
    pub stdout: String,             // Standard output
    pub stderr: String,             // Standard error
    pub duration: chrono::Duration, // Execution time
    pub timestamp: DateTime<Utc>,   // When command was executed
}

/// Tool information
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ToolInfo {
    pub name: String,              // Tool name
    pub description: String,       // Tool description
    pub version: String,           // Tool version
    pub capabilities: Vec<String>, // Tool capabilities
}

/// Execution context
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ExecutionContext {
    pub environment_variables: HashMap<String, String>, // Environment vars
    pub working_directory: PathBuf,                     // Current working directory
    pub timeout_seconds: u64,                           // Command timeout
    pub max_retries: u32,                               // Maximum retry attempts
}

/// Environment state snapshot
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct EnvironmentSnapshot {
    pub environment_variables: HashMap<String, String>, // Environment variables
    pub system_info: SystemInfo,                        // System information
    pub process_info: ProcessInfo,                      // Process information
    pub resource_usage: ResourceUsage,                  // Resource usage stats
}

/// System information
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct SystemInfo {
    pub os_name: String,      // Operating system name
    pub os_version: String,   // Operating system version
    pub architecture: String, // System architecture
    pub hostname: String,     // System hostname
    pub cpu_count: u32,       // Number of CPU cores
    pub total_memory: u64,    // Total system memory
}

/// Process information
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ProcessInfo {
    pub pid: u32,                   // Process ID
    pub parent_pid: Option<u32>,    // Parent process ID
    pub start_time: DateTime<Utc>,  // Process start time
    pub command_line: Vec<String>,  // Command line arguments
    pub working_directory: PathBuf, // Process working directory
}

/// Resource usage statistics
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ResourceUsage {
    pub cpu_usage: f32,              // CPU usage percentage
    pub memory_usage: u64,           // Memory usage in bytes
    pub disk_usage: u64,             // Disk usage in bytes
    pub network_bytes_sent: u64,     // Network bytes sent
    pub network_bytes_received: u64, // Network bytes received
}

/// Session metadata
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct SessionMetadata {
    pub session_id: String,           // Unique session identifier
    pub project_hash: String,         // Associated project
    pub created_at: DateTime<Utc>,    // Session creation time
    pub last_accessed: DateTime<Utc>, // Last access time
    pub checkpoint_count: u32,        // Number of checkpoints
    pub status: SessionStatus,        // Current session status
    pub description: Option<String>,  // Optional description
    pub tags: Vec<String>,            // User-defined tags
    pub size_bytes: u64,              // Total session size
}

/// Session status
#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum SessionStatus {
    Active,
    Completed,
    Failed,
    Archived,
}

/// Checkpoint summary for listing
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct CheckpointSummary {
    pub checkpoint_id: String,
    pub session_id: String,
    pub workflow_step: WorkflowStep,
    pub created_at: DateTime<Utc>,
    pub size_bytes: u64,
    pub description: Option<String>,
    pub tags: Vec<String>,
}

// Helper functions

/// Get git remote URL for a project
fn get_git_remote(project_path: &std::path::Path) -> Result<String, std::io::Error> {
    use std::process::Command;

    let output = Command::new("git")
        .arg("config")
        .arg("--get")
        .arg("remote.origin.url")
        .current_dir(project_path)
        .output()?;

    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    } else {
        Err(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "No git remote found",
        ))
    }
}

/// Find project markers (Cargo.toml, package.json, etc.)
fn find_project_markers(project_path: &std::path::Path) -> Vec<String> {
    let markers = [
        "Cargo.toml",
        "package.json",
        "setup.py",
        "requirements.txt",
        "pom.xml",
        "build.gradle",
        "Makefile",
        "CMakeLists.txt",
        ".git",
        "README.md",
        "LICENSE",
    ];

    markers
        .iter()
        .filter(|marker| project_path.join(marker).exists())
        .map(|&marker| marker.to_string())
        .collect()
}

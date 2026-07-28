//! Run-time context for identity and configuration.
//!
//! This module provides types that allow callers to specify project and session
//! identity, overriding the default path-based identity derivation used by the
//! checkpoint system.
//!
//! ## Design: ID vs Name
//!
//! Each identity has two fields:
//! - **`id`** — immutable storage partition key. Changing this moves all
//!   checkpoint data to a new directory. Never changes for the lifetime of
//!   a project/session.
//! - **`name`** — mutable human-readable display label. Renaming a project
//!   or session only updates this field; the `id` stays the same, so no
//!   checkpoint files move.
//!
//! This separation ensures that renaming a project directory (which would
//! change the path-based hash) no longer orphans existing checkpoints.
//!
//! ## Example
//!
//! ```
//! use abk::context::{RunContext, ProjectIdentity};
//!
//! let ctx = RunContext {
//!     project: Some(ProjectIdentity {
//!         id: "my-project-uuid".to_string(),
//!         name: Some("My Cool Project".to_string()),
//!     }),
//!     ..Default::default()
//! };
//! ```

// Re-export serde derives only when a feature that provides serde is active.
// We check for the `checkpoint`, `config`, `agent`, or `orchestration` features
// since they all pull in serde as a dependency.
#[cfg(any(
    feature = "checkpoint",
    feature = "config",
    feature = "agent",
    feature = "orchestration",
))]
use serde::{Deserialize, Serialize};

/// Identity for a project (storage partition key).
///
/// When provided via [`RunContext`], the `id` field is used directly as the
/// project hash for checkpoint storage, overriding the default path-based
/// SHA-256 hash. This enables:
///
/// - Renaming a project directory without losing checkpoints
/// - Multiple working directories sharing a single project's checkpoints
/// - Human-readable project names in checkpoint storage paths
///
/// The `id` is immutable (changing it creates a new storage partition).
/// The `name` is a mutable display label for UI purposes.
#[derive(Debug, Clone)]
#[cfg_attr(
    any(feature = "checkpoint", feature = "config", feature = "agent", feature = "orchestration"),
    derive(Serialize, Deserialize)
)]
pub struct ProjectIdentity {
    /// Stable, immutable ID used as the storage partition key.
    ///
    /// This value is used directly as the checkpoint storage directory
    /// name. It should be globally unique (e.g., a UUID or a path hash).
    /// Renaming a project must never change this value.
    pub id: String,

    /// Optional human-readable display name for the project.
    ///
    /// Shown in UIs (TUI, web sidebar) and checkpoint listings.
    /// Can be freely changed without affecting storage.
    pub name: Option<String>,
}

impl PartialEq for ProjectIdentity {
    fn eq(&self, other: &Self) -> bool {
        // Identity is determined solely by the immutable `id`.
        // Two projects with the same id but different names are the same project.
        self.id == other.id
    }
}

impl Eq for ProjectIdentity {}

impl std::hash::Hash for ProjectIdentity {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.id.hash(state);
    }
}

impl std::fmt::Display for ProjectIdentity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.name {
            Some(name) => write!(f, "{} ({})", name, self.id),
            None => write!(f, "{}", self.id),
        }
    }
}

/// Identity for a session within a project.
///
/// When provided via [`RunContext`], the `id` field is used directly as the
/// session ID for checkpoint storage, overriding the default
/// `session_{timestamp}_{task_slug}` naming.
///
/// Like [`ProjectIdentity`], the `id` is immutable and the `name` is a
/// mutable display label.
#[derive(Debug, Clone)]
#[cfg_attr(
    any(feature = "checkpoint", feature = "config", feature = "agent", feature = "orchestration"),
    derive(Serialize, Deserialize)
)]
pub struct SessionIdentity {
    /// Stable, immutable session ID used for storage.
    ///
    /// This becomes the session directory name under the project's checkpoint
    /// directory. Should be unique within the project.
    pub id: String,

    /// Optional human-readable display name for the session.
    ///
    /// Shown in UIs (TUI, web sidebar) and checkpoint listings.
    pub name: Option<String>,
}

impl PartialEq for SessionIdentity {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}

impl Eq for SessionIdentity {}

impl std::hash::Hash for SessionIdentity {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.id.hash(state);
    }
}

impl std::fmt::Display for SessionIdentity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.name {
            Some(name) => write!(f, "{} ({})", name, self.id),
            None => write!(f, "{}", self.id),
        }
    }
}

/// Runtime context that carries identity and configuration, replacing
/// process-global state.
///
/// `RunContext` is the primary state carrier for ABK. It holds:
/// - Optional project/session identity (for checkpoint storage partitioning)
/// - Optional agent name (replaces `ABK_AGENT_NAME` env var)
/// - Optional token store (for per-user MCP credential isolation)
///
/// ## Backward Compatibility
///
/// All fields are optional. When `RunContext::default()` is used (all fields
/// `None`), ABK falls back to environment-variable-based behavior — project
/// identity is derived from the working directory path, session IDs are
/// auto-generated from timestamps, and agent name is read from the
/// `ABK_AGENT_NAME` environment variable.
///
/// ## Example
///
/// ```
/// use abk::context::{RunContext, ProjectIdentity, SessionIdentity};
///
    /// let ctx = RunContext {
    ///     project: Some(ProjectIdentity {
    ///         id: "proj-abc123".to_string(),
    ///         name: Some("My Project".to_string()),
    ///     }),
    ///     session: Some(SessionIdentity {
    ///         id: "sess-001".to_string(),
    ///         name: None,
    ///     }),
    ///     agent_name: Some("trustee".to_string()),
    ///     home_dir: None,
    ///     #[cfg(feature = "registry-mcp-token")]
    ///     token_store: None,
    /// };
/// ```
#[derive(Clone, Default)]
#[cfg_attr(
    any(feature = "checkpoint", feature = "config", feature = "agent", feature = "orchestration"),
    derive(Serialize, Deserialize)
)]
pub struct RunContext {
    /// Optional project identity. When set, the `id` overrides path-based
    /// project hashing for checkpoint storage.
    pub project: Option<ProjectIdentity>,

    /// Optional session identity. When set, the `id` overrides
    /// timestamp-based session ID generation.
    pub session: Option<SessionIdentity>,

    /// Optional agent name. When set, overrides the `ABK_AGENT_NAME`
    /// environment variable for checkpoint storage paths, system messages,
    /// and all other places that previously read the env var.
    ///
    /// When `None`, falls back to `ABK_AGENT_NAME` environment variable
    /// (or a default like `"agent"` if the env var is also unset).
    pub agent_name: Option<String>,

    /// Optional home directory for checkpoint storage.
    ///
    /// When set, overrides the default `~/.{agent_name}/` path for checkpoint
    /// storage. This enables per-user isolation in multi-user deployments:
    /// e.g., `~/.trustee/users/{user_hash}/`.
    ///
    /// When `None`, falls back to the default `~/.{agent_name}/` path
    /// computed from the agent name.
    pub home_dir: Option<std::path::PathBuf>,

    /// Optional token store for MCP credential isolation.
    ///
    /// When set, MCP credential initialization uses this token store
    /// instead of creating a `FileTokenStore` from the agent name.
    /// This enables per-user token isolation in multi-user deployments.
    ///
    /// Only available with the `registry-mcp-token` feature.
    #[cfg(feature = "registry-mcp-token")]
    #[cfg_attr(
        any(feature = "checkpoint", feature = "config", feature = "agent", feature = "orchestration"),
        serde(skip)
    )]
    pub token_store: Option<std::sync::Arc<dyn pep::token_store::TokenStore>>,
}

impl RunContext {
    /// Create a new empty `RunContext` (equivalent to `Default::default()`).
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the project identity.
    pub fn with_project(mut self, project: ProjectIdentity) -> Self {
        self.project = Some(project);
        self
    }

    /// Set the session identity.
    pub fn with_session(mut self, session: SessionIdentity) -> Self {
        self.session = Some(session);
        self
    }

    /// Set the agent name.
    pub fn with_agent_name(mut self, agent_name: impl Into<String>) -> Self {
        self.agent_name = Some(agent_name.into());
        self
    }

    /// Set the home directory for checkpoint storage.
    ///
    /// This enables per-user isolation when the home_dir is set to something
    /// like `~/.trustee/users/{user_hash}/`.
    pub fn with_home_dir(mut self, home_dir: impl Into<std::path::PathBuf>) -> Self {
        self.home_dir = Some(home_dir.into());
        self
    }

    /// Set the token store (requires `registry-mcp-token` feature).
    #[cfg(feature = "registry-mcp-token")]
    pub fn with_token_store(
        mut self,
        store: std::sync::Arc<dyn pep::token_store::TokenStore>,
    ) -> Self {
        self.token_store = Some(store);
        self
    }

    /// Get the project identity, if any.
    pub fn project(&self) -> Option<&ProjectIdentity> {
        self.project.as_ref()
    }

    /// Get the session identity, if any.
    pub fn session(&self) -> Option<&SessionIdentity> {
        self.session.as_ref()
    }

    /// Get the agent name, if any.
    pub fn agent_name(&self) -> Option<&str> {
        self.agent_name.as_deref()
    }

    /// Get the home directory, if any.
    pub fn home_dir(&self) -> Option<&std::path::Path> {
        self.home_dir.as_deref()
    }

    /// Get the token store, if any.
    #[cfg(feature = "registry-mcp-token")]
    pub fn token_store(&self) -> Option<&std::sync::Arc<dyn pep::token_store::TokenStore>> {
        self.token_store.as_ref()
    }

    /// Resolve the agent name, falling back to the `ABK_AGENT_NAME`
    /// environment variable, and then to the provided `default`.
    ///
    /// This is used internally by `SessionManager`, `CheckpointStorageManager`,
    /// `ResumeTracker`, etc. to replace direct `std::env::var("ABK_AGENT_NAME")`
    /// calls.
    pub fn resolve_agent_name(&self, default: &str) -> String {
        self.agent_name
            .clone()
            .or_else(|| std::env::var("ABK_AGENT_NAME").ok())
            .unwrap_or_else(|| default.to_string())
    }

    /// Resolve the home directory for checkpoint storage.
    ///
    /// Falls back to `~/.{agent_name}/` when not explicitly set.
    /// Returns an error if the home directory cannot be determined.
    pub fn resolve_home_dir(&self) -> Result<std::path::PathBuf, String> {
        if let Some(ref dir) = self.home_dir {
            return Ok(dir.clone());
        }
        // Fall back to the agent name-based path
        let agent_name = self.resolve_agent_name("agent");
        let dir_name = format!(".{}", agent_name);
        let home = crate::get_home_dir()
            .map_err(|e| format!("Unable to determine home directory: {}", e))?;
        Ok(std::path::PathBuf::from(home).join(dir_name))
    }
}

impl std::fmt::Debug for RunContext {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RunContext")
            .field("project", &self.project)
            .field("session", &self.session)
            .field("agent_name", &self.agent_name)
            .field("home_dir", &self.home_dir)
            .field(
                "token_store",
                #[cfg(feature = "registry-mcp-token")]
                {
                    &self.token_store.as_ref().map(|_| "<TokenStore>")
                },
                #[cfg(not(feature = "registry-mcp-token"))]
                {
                    &None::<&str>
                },
            )
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_run_context_default() {
        let ctx = RunContext::default();
        assert!(ctx.project.is_none());
        assert!(ctx.session.is_none());
        assert!(ctx.agent_name.is_none());
        assert!(ctx.home_dir.is_none());
    }

    #[test]
    fn test_run_context_builder() {
        let ctx = RunContext::new()
            .with_project(ProjectIdentity {
                id: "test-project".to_string(),
                name: Some("Test Project".to_string()),
            })
            .with_session(SessionIdentity {
                id: "test-session".to_string(),
                name: None,
            })
            .with_agent_name("my-agent");

        assert_eq!(ctx.project().unwrap().id, "test-project");
        assert_eq!(ctx.project().unwrap().name.as_deref(), Some("Test Project"));
        assert_eq!(ctx.session().unwrap().id, "test-session");
        assert!(ctx.session().unwrap().name.is_none());
        assert_eq!(ctx.agent_name(), Some("my-agent"));
    }

    #[test]
    fn test_resolve_agent_name_from_context() {
        let ctx = RunContext::new().with_agent_name("explicit-name");
        assert_eq!(ctx.resolve_agent_name("fallback"), "explicit-name");
    }

    #[test]
    fn test_resolve_agent_name_fallback_to_default() {
        // When neither context nor env var is set, should use default
        // Save and clear env var to ensure deterministic test
        let saved = std::env::var("ABK_AGENT_NAME").ok();
        std::env::remove_var("ABK_AGENT_NAME");

        let ctx = RunContext::default();
        assert_eq!(ctx.resolve_agent_name("default-agent"), "default-agent");

        // Restore
        if let Some(v) = saved {
            std::env::set_var("ABK_AGENT_NAME", v);
        }
    }

    #[test]
    fn test_project_identity_equality_by_id() {
        let a = ProjectIdentity {
            id: "abc".to_string(),
            name: Some("Alpha".to_string()),
        };
        let b = ProjectIdentity {
            id: "abc".to_string(),
            name: Some("Beta".to_string()),
        };
        // Same id → equal regardless of name
        assert_eq!(a, b);
    }

    #[test]
    fn test_project_identity_inequality() {
        let a = ProjectIdentity {
            id: "abc".to_string(),
            name: None,
        };
        let b = ProjectIdentity {
            id: "xyz".to_string(),
            name: None,
        };
        assert_ne!(a, b);
    }

    #[test]
    fn test_session_identity_display() {
        let with_name = SessionIdentity {
            id: "sess-001".to_string(),
            name: Some("First Chat".to_string()),
        };
        assert_eq!(with_name.to_string(), "First Chat (sess-001)");

        let without_name = SessionIdentity {
            id: "sess-002".to_string(),
            name: None,
        };
        assert_eq!(without_name.to_string(), "sess-002");
    }

    #[test]
    fn test_project_identity_display() {
        let p = ProjectIdentity {
            id: "uuid-1234".to_string(),
            name: Some("Trustee".to_string()),
        };
        assert_eq!(p.to_string(), "Trustee (uuid-1234)");
    }
}

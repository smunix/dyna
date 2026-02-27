//! Shared error types for the Dyna distributed CRUD system.
//!
//! Defines [`DynaError`] with variants for resource, patch, changeset, channel,
//! conflict, and storage errors. All fallible functions in `dyna-core` return
//! [`DynaResult<T>`].

use thiserror::Error;

/// Top-level error type for the Dyna common library.
#[derive(Error, Debug)]
pub enum DynaError {
    #[error("Resource not found: {0}")]
    ResourceNotFound(String),

    #[error("Patch not found: {0}")]
    PatchNotFound(String),

    #[error("Changeset not found: {0}")]
    ChangesetNotFound(String),

    #[error("Channel not found: {0}")]
    ChannelNotFound(String),

    #[error("Conflict detected on resource '{resource}': {description}")]
    Conflict {
        resource: String,
        description: String,
    },

    #[error("Dependency missing: '{0}' depends on '{1}' which is not available")]
    DependencyMissing(String, String),

    #[error("Hash mismatch: expected '{expected}', got '{actual}'")]
    HashMismatch { expected: String, actual: String },

    #[error("Concurrent modification detected: {0}")]
    ConcurrentModification(String),

    #[error("Changeset is immutable: {0}")]
    ImmutableChangeset(String),

    #[error("Staged changes exist: commit or discard before proceeding")]
    StagedChangesExist,

    #[error("Serialization error: {0}")]
    Serialization(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("Repository not initialized. Run 'dyna init' first.")]
    NotInitialized,

    #[error("Repository already initialized at {0}")]
    AlreadyInitialized(String),

    #[error("Nothing to commit: no staged changes")]
    NothingToCommit,

    #[error("Remote error: {0}")]
    Remote(String),

    #[error("{0}")]
    Other(String),
}

/// Convenience Result type alias.
pub type DynaResult<T> = Result<T, DynaError>;

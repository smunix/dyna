//! Actor implementations for the Dyna remote server.
//!
//! The server is organized into three elfo actor groups:
//! - [`api`]: HTTP bridge (axum) for CLI client communication.
//! - [`changeset`]: Core business logic for changeset operations.
//! - [`storage`]: S3-compatible persistent storage via `object_store`.

pub mod storage;
pub mod changeset;
pub mod api;

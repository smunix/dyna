//! Actor implementations for the Dyna remote server.
//!
//! The server is organized into three elfo actor groups:
//! - [`api_gateway`]: HTTP bridge (axum) for CLI client communication.
//! - [`changeset_manager`]: Core business logic for changeset operations.
//! - [`storage`]: S3-compatible persistent storage via `object_store`.

pub mod storage;
pub mod changeset_manager;
pub mod api_gateway;

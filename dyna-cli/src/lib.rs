//! Dyna CLI library — reusable Rust API for the Dyna distributed CRUD system.
//!
//! This library exposes the core building blocks of the `dyna` CLI tool:
//!
//! - [`repository::Repository`]: Local repository management (staging, commits,
//!   channels, snapshots, conflicts) backed by the [`vfs`] crate.
//! - [`sync_client::SyncClient`]: HTTP client for push/pull/clone/promote
//!   operations against a remote Dyna server.
//! - [`commands`]: Individual command implementations that compose `Repository`
//!   and `SyncClient` to perform high-level operations.
//!
//! The binary `dyna` (in `main.rs`) is a thin CLI wrapper around this library.

pub mod commands;
pub mod repository;
pub mod sync_client;

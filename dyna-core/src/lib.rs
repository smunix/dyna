//! # dyna-core
//!
//! Shared types, data model, changeset engine, and protocol definitions for the
//! Dyna distributed CRUD system. This crate is used by both the CLI client and
//! the remote server.
//!
//! The data model is **changeset-centric**, inspired by the Jujutsu VCS:
//!
//! - A [`models::Changeset`] is the primary unit of work, grouping one or more
//!   [`models::Patch`] objects that were committed together.
//! - Each changeset has a stable `change_id` (survives rewrites) and a
//!   content-addressed `commit_hash` (changes when content changes).
//! - Changesets form a DAG via parent references and can be mutable locally
//!   until promoted to a shared channel, at which point they become immutable.
//! - A [`models::Patch`] represents a set of JSON Patch (RFC 6902) operations
//!   targeting a single JSON resource.
//! - A [`models::Channel`] is a named bookmark pointing to an ordered sequence
//!   of changeset IDs.

pub mod models;
pub mod patch;
pub mod hash;
pub mod protocol;
pub mod error;
pub mod channel;
pub mod diff;
pub mod compression;
pub mod notification;

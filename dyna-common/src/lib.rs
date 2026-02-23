//! # dyna-common
//!
//! Shared types, data model, patch engine, and protocol definitions for the Dyna
//! distributed CRUD system. This crate is used by both the CLI client and the
//! remote server.

pub mod models;
pub mod patch;
pub mod hash;
pub mod protocol;
pub mod error;
pub mod channel;
pub mod diff;

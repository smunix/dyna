//! CLI command implementations for the Dyna distributed CRUD tool.
//!
//! Each submodule implements a single CLI command. The commands are
//! **changeset-centric**, operating on [`Changeset`] objects as the
//! primary unit of work:
//!
//! - [`init`], [`clone`]: Repository lifecycle.
//! - [`add`], [`commit`], [`describe`], [`squash`]: Staging and changeset creation.
//! - [`push`], [`pull`], [`promote`]: Remote synchronization.
//! - [`status`], [`log`], [`diff`]: Inspection and history.
//! - [`channel`]: Channel (bookmark) management.
//! - [`resolve`]: Interactive conflict resolution.
//! - [`restore`]: Revert files to snapshot state from channels or changesets.

pub mod init;
pub mod clone;
pub mod add;
pub mod commit;
pub mod push;
pub mod pull;
pub mod status;
pub mod log;
pub mod resolve;
pub mod restore;
pub mod channel;
pub mod diff;
pub mod promote;
pub mod squash;
pub mod describe;

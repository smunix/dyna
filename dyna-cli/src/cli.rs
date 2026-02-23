//! CLI argument definitions using clap's Derive API.

use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(
    name = "dyna",
    author,
    version,
    about = "A distributed CRUD tool for collaborative JSON resource editing, inspired by Pijul.",
    long_about = None
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand, Debug)]
pub enum Commands {
    /// Initialize a new Dyna repository in the current directory.
    Init,

    /// Clone a repository from a remote server URL.
    Clone {
        /// The URL of the remote repository.
        url: String,
        /// Optional: the local directory to clone into.
        #[arg(short, long)]
        directory: Option<PathBuf>,
    },

    /// Stage a JSON resource file for the next commit.
    Add {
        /// Path to the JSON resource file to stage.
        path: PathBuf,
    },

    /// Record staged changes as a new patch (changeset).
    Commit {
        /// A descriptive message for this changeset.
        #[arg(short, long)]
        message: String,
    },

    /// Push local patches to the remote server.
    Push,

    /// Fetch and merge remote patches into the local state.
    Pull,

    /// Show the working directory and staging area status.
    Status,

    /// Display the patch history for the current channel.
    Log {
        /// Number of recent patches to display.
        #[arg(short = 'n', long, default_value = "20")]
        count: usize,
    },

    /// Interactively resolve conflicts for a resource.
    Resolve {
        /// Path to the resource file with conflicts.
        path: PathBuf,
    },

    /// Switch to or create a channel (branch).
    Channel {
        /// Name of the channel.
        name: String,
        /// Create the channel if it doesn't exist.
        #[arg(short, long)]
        create: bool,
    },

    /// Promote patches from the current channel to the main channel.
    Promote,
}

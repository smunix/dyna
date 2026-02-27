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

    /// Stage JSON resource file(s) for the next commit.
    ///
    /// Accepts a single JSON file or a directory. When a directory is given,
    /// all `.json` files within it are staged recursively.
    Add {
        /// Path to a JSON file or directory to stage.
        path: PathBuf,

        /// Recursively add all `.json` files in the given directory.
        /// This flag is implied when a directory path is provided, but can be
        /// used explicitly for clarity.
        #[arg(short, long)]
        recursive: bool,
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

    /// Show detailed diffs (operations) for staged files.
    ///
    /// Without arguments, shows diffs for all staged files. Optionally
    /// specify a file path to filter to a single resource.
    Diff {
        /// Optional: path to a specific staged file to diff.
        path: Option<PathBuf>,
    },

    /// Display the patch history for the current channel.
    ///
    /// Use `--verbose` to include detailed operations for each patch, or
    /// `--patch <hash>` to inspect a single patch in full detail.
    Log {
        /// Number of recent patches to display.
        #[arg(short = 'n', long, default_value = "20")]
        count: usize,

        /// Show detailed operations for each patch in the log.
        #[arg(short, long)]
        verbose: bool,

        /// Show full detail for a specific patch by its hash (or hash prefix).
        #[arg(short, long)]
        patch: Option<String>,
    },

    /// Interactively resolve conflicts for a resource.
    Resolve {
        /// Path to the resource file with conflicts.
        path: PathBuf,
    },

    /// Manage channels (branches).
    ///
    /// Without flags, switches to the named channel. Use `--list` to list
    /// channels, `--remote` to include remote channels, and `--create` to
    /// create a new channel.
    ///
    /// Switching is blocked if there are staged but uncommitted files.
    /// On switch, the working directory is cleaned and repopulated with
    /// the target channel's committed resource state.
    Channel {
        /// Name of the channel (required unless --list is used).
        name: Option<String>,

        /// Create the channel if it doesn't exist.
        #[arg(short, long)]
        create: bool,

        /// List all channels instead of switching.
        #[arg(short, long)]
        list: bool,

        /// When used with --list, also fetch and display remote channels.
        #[arg(short, long)]
        remote: bool,
    },

    /// Promote patches from the current channel to the main channel.
    Promote,
}

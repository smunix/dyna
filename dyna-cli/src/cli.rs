//! CLI argument definitions using clap's Derive API.

use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(
    name = "dyna",
    author,
    version,
    about = "A distributed CRUD tool for collaborative JSON resource editing.\nInspired by Pijul and Jujutsu, with changeset-centric workflow.",
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

    /// Stage JSON resource file(s) for the next changeset.
    ///
    /// Accepts a single JSON file or a directory. When a directory is given,
    /// all `.json` files within it are staged recursively.
    Add {
        /// Path to a JSON file or directory to stage.
        path: PathBuf,
        /// Recursively add all `.json` files in the given directory.
        #[arg(short, long)]
        recursive: bool,
    },

    /// Record staged changes as a new changeset.
    ///
    /// A changeset groups one or more patches (one per modified resource)
    /// into a single logical unit with a message, author, and parent
    /// changeset references. This is analogous to `jj commit`.
    Commit {
        /// A descriptive message for this changeset.
        #[arg(short, long)]
        message: String,
    },

    /// Push local changesets to the remote server.
    Push,

    /// Fetch and merge remote changesets into the local state.
    Pull,

    /// Show the working directory and staging area status.
    Status,

    /// Show detailed diffs (operations) for staged files.
    Diff {
        /// Optional: path to a specific staged file to diff.
        path: Option<PathBuf>,
    },

    /// Display the changeset history for the current channel.
    ///
    /// Shows changesets (not individual patches) as the primary log unit.
    /// Use `--verbose` to include patches and their operations, or
    /// `--changeset <id>` to inspect a single changeset in full detail.
    Log {
        /// Number of recent changesets to display.
        #[arg(short = 'n', long, default_value = "20")]
        count: usize,

        /// Show patches and their operations within each changeset.
        #[arg(short, long)]
        verbose: bool,

        /// Show full detail for a specific changeset by its change_id
        /// (or change_id prefix).
        #[arg(short, long)]
        changeset: Option<String>,

        /// When used with --changeset, also show detailed operations
        /// for each patch in the changeset.
        #[arg(short, long)]
        patches: bool,
    },

    /// Interactively resolve conflicts for a resource.
    Resolve {
        /// Path to the resource file with conflicts.
        path: PathBuf,
    },

    /// Manage channels (bookmarks into the changeset DAG).
    ///
    /// Without flags, switches to the named channel. Use `--list` to list
    /// channels, `--remote` to include remote channels, and `--create` to
    /// create a new channel.
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

    /// Promote changesets from the current channel to the main channel.
    ///
    /// Promoted changesets are marked as immutable.
    Promote,

    /// Describe (amend the message of) a changeset.
    ///
    /// Defaults to the current working changeset if no change_id is given.
    Describe {
        /// The change_id (or prefix) of the changeset to describe.
        change_id: Option<String>,
        /// The new description message.
        #[arg(short, long)]
        message: String,
    },
}

//! CLI argument definitions using clap's Derive API.
//!
//! Defines the `dyna` command-line interface with a changeset-centric command
//! set. Commands operate on **changesets** (groups of patches) rather than
//! individual patches.
//!
//! Commands: `init`, `clone`, `add`, `commit`, `describe`, `squash`, `push`,
//! `pull`, `status`, `diff`, `log`, `resolve`, `restore`, `channel`, `promote`.

use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(
    name = "dyna",
    author,
    version,
    about = "A distributed CRUD tool for collaborative JSON resource editing, with changeset-centric workflow.",
    long_about = None
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand, Debug)]
pub enum Commands {
    /// Initialize a new Dyna repository in the current directory.
    Init {
        /// Optional: the remote server URL to configure.
        #[arg(short, long)]
        remote: Option<String>,
    },

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
    /// Accepts a single JSON file, a directory, or a glob pattern.
    /// When a directory is given, all `.json` files within it are staged
    /// recursively.
    ///
    /// Use `--delete` to stage the removal of tracked files that have been
    /// deleted from the filesystem. Supports:
    /// - Single file:  `dyna add --delete data/users/config.json`
    /// - Directory:     `dyna add --delete data/users/`
    /// - Glob pattern:  `dyna add --delete "data/**/*.json"`
    Add {
        /// Path or glob pattern for JSON file(s) / directory to stage.
        pattern: String,
        /// Recursively add all `.json` files in the given directory.
        #[arg(short, long)]
        recursive: bool,
        /// Stage the removal of deleted tracked file(s), directory, or glob.
        #[arg(short, long)]
        delete: bool,
    },

    /// Record staged changes as a new changeset.
    ///
    /// A changeset groups one or more patches (one per modified resource)
    /// into a single logical unit with a message, author, and parent
    /// changeset references.
    Commit {
        /// A descriptive message for this changeset.
        #[arg(short, long)]
        message: String,
    },

    /// Push local changesets to the remote server.
    ///
    /// Defaults to the current channel if no channel name is given.
    /// Use `--force` to resend ALL local changesets, ignoring sync state.
    /// This is useful when the remote server has lost its state (e.g.
    /// after a restart with in-memory storage).
    Push {
        /// Optional: the channel to push (defaults to current channel).
        #[arg(short, long)]
        channel: Option<String>,
        /// Force-push all local changesets, ignoring sync state.
        /// Useful when the remote server has lost its in-memory state.
        #[arg(short, long)]
        force: bool,
    },

    /// Fetch and merge remote changesets into the local state.
    ///
    /// Defaults to the current channel if no channel name is given.
    Pull {
        /// Optional: the channel to pull (defaults to current channel).
        #[arg(short, long)]
        channel: Option<String>,
    },

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

    /// Restore a file to its snapshot state.
    ///
    /// Without flags, restores from the current channel's latest snapshot.
    /// Use `--channel` to restore from a specific channel's head, or
    /// `--changeset` to restore from a specific changeset in any channel.
    Restore {
        /// Path to the file to restore.
        path: PathBuf,
        /// Restore from the head of a specific channel.
        #[arg(short = 'C', long)]
        channel: Option<String>,
        /// Restore from a specific changeset (change_id or prefix).
        #[arg(short = 's', long)]
        changeset: Option<String>,
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
        /// When used with --list, show only remote channels.
        #[arg(short, long)]
        remote: bool,
        /// When used with --list, show only local channels.
        #[arg(long)]
        local: bool,
    },

    /// Promote changesets from a channel to the main channel.
    ///
    /// By default, promotes from the current channel. Use `--channel` to
    /// specify a different source channel without switching to it first.
    ///
    /// Promoted changesets are marked as immutable.
    Promote {
        /// The source channel to promote from (defaults to current channel).
        #[arg(short, long)]
        channel: Option<String>,
    },

    /// Squash a changeset into its parent.
    ///
    /// Merges the child changeset's patches into the parent: patches targeting
    /// the same resource are combined (parent_snapshot from target, result_snapshot
    /// from child, operations recomputed); patches targeting new resources are
    /// appended. The child changeset is then removed from the channel history.
    ///
    /// Without flags, squashes the working changeset (or channel head) into its
    /// parent. Use `--revision` to pick a specific changeset, `--into` to squash
    /// into a non-parent ancestor, and `--message` to override the resulting
    /// message.
    Squash {
        /// The changeset to squash (change_id or prefix). Defaults to working
        /// changeset or channel head.
        #[arg(short, long)]
        revision: Option<String>,
        /// Squash into this changeset instead of the immediate parent.
        #[arg(short, long)]
        into: Option<String>,
        /// Override the resulting changeset's message.
        #[arg(short, long)]
        message: Option<String>,
    },

    /// Query the change history of a specific resource.
    ///
    /// Fetches the history of all changes to a given resource_id from the
    /// remote server, showing changesets across all channels.
    History {
        /// The resource_id to query (e.g. acme.entity.User).
        resource_id: String,
        /// Show detailed operations for each changeset.
        #[arg(short, long)]
        verbose: bool,
    },

    /// Revert a changeset by creating a new changeset with inverse patches.
    ///
    /// Loads the target changeset, inverts all of its patches, and commits
    /// the inverse as a new changeset on the current (or specified) channel.
    Revert {
        /// The change_id (or prefix) of the changeset to revert.
        change_id: String,
        /// The channel to apply the revert on (defaults to current channel).
        #[arg(short = 'C', long)]
        channel: Option<String>,
    },

    /// Cherry-pick a changeset from another channel onto the current channel.
    ///
    /// Copies the patches from the source changeset and applies them as a new
    /// changeset on the destination channel (current channel by default).
    /// The new changeset records the destination channel's head as its parent.
    CherryPick {
        /// The change_id (or prefix) of the changeset to cherry-pick.
        change_id: String,
        /// The destination channel (defaults to current channel).
        #[arg(short = 'C', long)]
        channel: Option<String>,
    },

    /// Load resources from a JSON file into the working directory and stage them.
    ///
    /// The file must contain a JSON array of objects. Each object must have a
    /// `res_id` field specifying the resource ID; the remaining fields form the
    /// resource content. Each matching resource is written to the working
    /// directory and staged for the next commit.
    ///
    /// Use `--limit` to cap the number of resources loaded, and `--filter` to
    /// restrict loading to resource IDs matching a regex pattern.
    #[command(name = "load-file")]
    LoadFile {
        /// Path to the JSON file containing the resource array.
        file: PathBuf,
        /// Maximum number of resources to load.
        #[arg(short, long)]
        limit: Option<usize>,
        /// Regex pattern to filter resource IDs.
        #[arg(short, long)]
        filter: Option<String>,
    },

    /// Unstage previously staged changes, moving them back to the working directory.
    ///
    /// Accepts a resource ID, a file path, or `--all` to unstage everything.
    /// For staged deletions, the file is restored from its previous snapshot.
    Unstage {
        /// Resource ID or file path to unstage (optional if --all is used).
        pattern: Option<String>,
        /// Unstage all staged changes.
        #[arg(short, long)]
        all: bool,
    },

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

//! Dyna CLI — distributed CRUD tool for collaborative JSON resource editing.
//!
//! This is the entry point for the `dyna` command-line tool. It parses CLI
//! arguments via `clap` and dispatches to the appropriate command handler.
//! The workflow is **changeset-centric**: all operations revolve around
//! [`Changeset`] objects that group patches together.

mod cli;

use clap::Parser;
use cli::{Cli, Commands};
use dyna_cli::commands;
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("warn")),
        )
        .init();

    let cli = Cli::parse();

    let result = match cli.command {
        Commands::Init { remote } => commands::init::execute(remote).await,
        Commands::Clone { url, directory } => commands::clone::execute(url, directory).await,
        Commands::Add {
            pattern,
            recursive: _,
            delete,
        } => commands::add::execute(pattern, delete).await,
        Commands::Commit { message } => commands::commit::execute(message).await,
        Commands::Push { channel, force } => commands::push::execute(channel, force).await,
        Commands::Pull { channel } => commands::pull::execute(channel).await,
        Commands::Status => commands::status::execute().await,
        Commands::Diff { path } => commands::diff::execute(path).await,
        Commands::Log {
            count,
            verbose,
            changeset,
            patches,
        } => commands::log::execute(count, verbose, changeset, patches).await,
        Commands::Resolve { path } => commands::resolve::execute(path).await,
        Commands::Restore {
            path,
            channel,
            changeset,
        } => commands::restore::execute(path, channel, changeset).await,
        Commands::Channel {
            name,
            create,
            list,
            remote,
            local,
        } => commands::channel::execute(name, create, list, remote, local).await,
        Commands::Promote { channel } => commands::promote::execute(channel).await,
        Commands::Squash {
            revision,
            into,
            message,
        } => commands::squash::execute(revision, into, message).await,
        Commands::History {
            resource_id,
            verbose,
        } => commands::history::execute(resource_id, verbose).await,
        Commands::Revert { change_id, channel } => {
            commands::revert::execute(change_id, channel).await
        }
        Commands::CherryPick { change_id, channel } => {
            commands::cherry_pick::execute(change_id, channel).await
        }
        Commands::LoadFile {
            file,
            limit,
            filter,
        } => commands::load_file::execute(file, limit, filter).await,
        Commands::Describe { change_id, message } => {
            commands::describe::execute(change_id, message).await
        }
        Commands::Unstage { pattern, all } => commands::unstage::execute(pattern, all).await,
    };

    if let Err(e) = result {
        eprintln!("\x1b[31merror:\x1b[0m {}", e);
        std::process::exit(1);
    }

    Ok(())
}

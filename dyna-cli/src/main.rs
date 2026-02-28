//! Dyna CLI — distributed CRUD tool for collaborative JSON resource editing.
//!
//! This is the entry point for the `dyna` command-line tool. It parses CLI
//! arguments via `clap` and dispatches to the appropriate command handler.
//! The workflow is **changeset-centric**, inspired by Jujutsu: all operations
//! revolve around [`Changeset`] objects that group patches together.

mod cli;
mod commands;
mod repository;
mod sync_client;

use clap::Parser;
use cli::{Cli, Commands};
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
        Commands::Init => commands::init::execute().await,
        Commands::Clone { url, directory } => commands::clone::execute(url, directory).await,
        Commands::Add {
            path,
            recursive: _,
            delete,
        } => commands::add::execute(path, delete).await,
        Commands::Commit { message } => commands::commit::execute(message).await,
        Commands::Push { channel } => commands::push::execute(channel).await,
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
        Commands::Promote => commands::promote::execute().await,
        Commands::Describe { change_id, message } => {
            commands::describe::execute(change_id, message).await
        }
    };

    if let Err(e) = result {
        eprintln!("\x1b[31merror:\x1b[0m {}", e);
        std::process::exit(1);
    }

    Ok(())
}

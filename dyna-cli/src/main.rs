//! Dyna CLI — distributed CRUD tool for collaborative JSON resource editing.
//!
//! Changeset-centric workflow inspired by Pijul and Jujutsu.

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
        Commands::Add { path, recursive: _ } => commands::add::execute(path).await,
        Commands::Commit { message } => commands::commit::execute(message).await,
        Commands::Push => commands::push::execute().await,
        Commands::Pull => commands::pull::execute().await,
        Commands::Status => commands::status::execute().await,
        Commands::Diff { path } => commands::diff::execute(path).await,
        Commands::Log {
            count,
            verbose,
            changeset,
            patches,
        } => commands::log::execute(count, verbose, changeset, patches).await,
        Commands::Resolve { path } => commands::resolve::execute(path).await,
        Commands::Channel {
            name,
            create,
            list,
            remote,
        } => commands::channel::execute(name, create, list, remote).await,
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

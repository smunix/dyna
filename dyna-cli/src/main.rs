//! Dyna CLI - A distributed CRUD tool for collaborative JSON resource editing.
//!
//! This is the main entry point for the `dyna` command-line tool.

mod cli;
mod commands;
mod repository;
mod sync_client;

use clap::Parser;
use cli::{Cli, Commands};
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Initialize tracing/logging
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("warn")),
        )
        .init();

    let cli = Cli::parse();

    let result = match cli.command {
        Commands::Init => commands::init::execute().await,
        Commands::Clone { url, directory } => commands::clone::execute(url, directory).await,
        Commands::Add { path, recursive } => commands::add::execute(path, recursive).await,
        Commands::Commit { message } => commands::commit::execute(message).await,
        Commands::Push => commands::push::execute().await,
        Commands::Pull => commands::pull::execute().await,
        Commands::Status => commands::status::execute().await,
        Commands::Log { count } => commands::log::execute(count).await,
        Commands::Resolve { path } => commands::resolve::execute(path).await,
        Commands::Channel {
            name,
            create,
            list,
            remote,
        } => commands::channel::execute(name, create, list, remote).await,
        Commands::Promote => commands::promote::execute().await,
    };

    if let Err(e) = result {
        eprintln!("\x1b[31merror:\x1b[0m {}", e);
        std::process::exit(1);
    }

    Ok(())
}

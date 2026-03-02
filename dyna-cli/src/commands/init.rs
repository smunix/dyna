//! `dyna init` command implementation.

use anyhow::Result;
use crate::repository::Repository;

pub async fn execute(remote: Option<String>) -> Result<()> {
    let cwd = std::env::current_dir()?;
    let repo = Repository::init(&cwd)?;

    // If a remote URL was provided, update the config immediately.
    if let Some(ref url) = remote {
        let mut config = repo.load_config()?;
        config.remote_url = Some(url.clone());
        repo.save_config(&config)?;
    }

    println!(
        "Initialized empty Dyna repository in {}",
        repo.dyna_dir.display()
    );
    println!("  Default channel: main");
    if let Some(url) = remote {
        println!("  Remote URL: {}", url);
    } else {
        println!("  Edit .dyna/config.toml to set your user name, email, and remote URL.");
    }

    Ok(())
}

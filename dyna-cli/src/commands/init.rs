//! `dyna init` command implementation.

use anyhow::Result;
use crate::repository::Repository;

pub async fn execute() -> Result<()> {
    let cwd = std::env::current_dir()?;
    let repo = Repository::init(&cwd)?;

    println!(
        "Initialized empty Dyna repository in {}",
        repo.dyna_dir.display()
    );
    println!("  Default channel: main");
    println!("  Edit .dyna/config.toml to set your user name, email, and remote URL.");

    Ok(())
}

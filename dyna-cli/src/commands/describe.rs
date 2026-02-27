//! `dyna describe` command implementation.
//!
//! Amends the message of a changeset, similar to `jj describe`.
//! Defaults to the current working changeset if no change_id is given.

use anyhow::{Result, bail};
use colored::Colorize;

use crate::repository::Repository;

pub async fn execute(change_id: Option<String>, message: String) -> Result<()> {
    let repo = Repository::find_current()?;

    // Determine which changeset to describe
    let target_id = match change_id {
        Some(id) => {
            // Try exact match, then prefix match
            match repo.load_changeset(&id) {
                Ok(cs) => cs.change_id,
                Err(_) => {
                    let matches = repo.find_changeset_by_prefix(&id)?;
                    match matches.len() {
                        0 => bail!("No changeset found matching '{}'", id),
                        1 => matches[0].change_id.clone(),
                        n => {
                            println!(
                                "{} Ambiguous prefix '{}' matches {} changesets:",
                                "warning:".yellow().bold(),
                                id,
                                n
                            );
                            for m in &matches {
                                println!("  {} - {}", m.short_change_id(), m.message);
                            }
                            bail!("Please provide a longer prefix.");
                        }
                    }
                }
            }
        }
        None => {
            // Default to working changeset
            match repo.working_change_id()? {
                Some(id) => id,
                None => bail!("No working changeset. Commit some changes first, or specify a change_id."),
            }
        }
    };

    let mut cs = repo.load_changeset(&target_id)?;

    if cs.immutable {
        bail!(
            "Changeset {} is immutable and cannot be modified.",
            cs.short_change_id()
        );
    }

    let old_message = cs.message.clone();
    cs.message = message.clone();
    cs.updated_at = chrono::Utc::now();

    // Recompute commit hash since content changed
    cs.recompute_hash();

    repo.store_changeset(&cs)?;

    println!(
        "Updated changeset {}",
        cs.short_change_id().to_string().yellow().bold()
    );
    if !old_message.is_empty() {
        println!("  was: {}", old_message.dimmed());
    }
    println!("  now: {}", message);

    Ok(())
}

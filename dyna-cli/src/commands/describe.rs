//! `dyna describe` command implementation.
//!
//! Amends the message of a changeset.
//! Defaults to the current working changeset if no change_id is given.

use anyhow::{Result, bail};
use itertools::izip;
use colored::Colorize;

use crate::repository::Repository;

pub async fn execute(change_id: Option<String>, message: String) -> Result<()> {
    let repo = Repository::find_current()?;

    // Determine which changeset to describe
    let target_id = change_id
        .map(|id| {
            repo.load_changeset(&id)
                .map(|cs| cs.change_id)
                .or_else(|_| {
                    repo.find_changeset_by_prefix(&id).and_then(|matches| {
                        match matches.len() {
                            0 => bail!("No changeset found matching '{}'", id),
                            1 => Ok(matches[0].change_id.clone()),
                            n => {
                                izip!(&matches).for_each(|m| {
                                    println!("  {} - {}", m.short_change_id(), m.message);
                                });
                                bail!(
                                    "Ambiguous prefix '{}' matches {} changesets. Please provide a longer prefix.",
                                    id, n
                                );
                            }
                        }
                    })
                })
        })
        .unwrap_or_else(|| {
            repo.working_change_id()?
                .ok_or_else(|| {
                    anyhow::anyhow!(
                        "No working changeset. Commit some changes first, or specify a change_id."
                    )
                })
        })?;

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
    cs.recompute_hash();

    repo.store_changeset(&cs)?;

    println!(
        "Updated changeset {}",
        cs.short_change_id().to_string().yellow().bold()
    );
    (!old_message.is_empty()).then(|| println!("  was: {}", old_message.dimmed()));
    println!("  now: {}", message);

    Ok(())
}

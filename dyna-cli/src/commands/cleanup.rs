use anyhow::Result;
use chrono::{DateTime, NaiveDateTime, Utc};
use std::collections::HashSet;

/// Parse a cutoff string into a NaiveDateTime.
///
/// Supports:
/// - ISO 8601 datetime: "2026-01-15T00:00:00"
/// - Duration suffixes: "30d", "6m", "1y", "24h", "3600s", "90min"
fn parse_cutoff(s: &str) -> Result<DateTime<Utc>> {
    // Try ISO 8601 first
    if let Ok(dt) = NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S") {
        return Ok(dt.and_utc());
    }
    if let Ok(dt) = NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S%.f") {
        return Ok(dt.and_utc());
    }

    let now = Utc::now();

    // Try duration patterns
    if let Some(num) = s.strip_suffix("min") {
        let n: i64 = num.trim().parse()?;
        return Ok(now - chrono::TimeDelta::minutes(n));
    }
    if let Some(num) = s.strip_suffix('d') {
        let n: i64 = num.trim().parse()?;
        return Ok(now - chrono::TimeDelta::days(n));
    }
    if let Some(num) = s.strip_suffix('m') {
        let n: i64 = num.trim().parse()?;
        return Ok(now - chrono::TimeDelta::days(n * 30));
    }
    if let Some(num) = s.strip_suffix('y') {
        let n: i64 = num.trim().parse()?;
        return Ok(now - chrono::TimeDelta::days(n * 365));
    }
    if let Some(num) = s.strip_suffix('h') {
        let n: i64 = num.trim().parse()?;
        return Ok(now - chrono::TimeDelta::hours(n));
    }
    if let Some(num) = s.strip_suffix('s') {
        let n: i64 = num.trim().parse()?;
        return Ok(now - chrono::TimeDelta::seconds(n));
    }

    anyhow::bail!(
        "Invalid cutoff format '{}'. Use ISO 8601 (2026-01-15T00:00:00) or duration (30d, 6m, 1y, 24h, 90min, 3600s).",
        s
    );
}

/// Execute the `cleanup` command.
///
/// Finds all channels (excluding main) whose changesets are a subset of main's,
/// optionally filtered by a cutoff date, and deletes them.
pub async fn execute(cutoff: Option<String>, remote: bool, dry_run: bool) -> Result<()> {
    let repo = crate::repository::Repository::find_current()?;

    let cutoff_dt: Option<DateTime<Utc>> = match &cutoff {
        Some(s) => Some(parse_cutoff(s)?),
        None => None,
    };

    // Load main channel
    let main_channel = repo.load_channel("main")?;
    let main_set: HashSet<&String> = main_channel.changesets.iter().collect();

    // List all local channels
    let channels = repo.list_channels()?;
    let mut deleted_count = 0;

    for ch in &channels {
        // Skip main
        if ch.name == "main" {
            continue;
        }

        // Check if all changesets are in main
        let all_promoted = ch.changesets.iter().all(|id| main_set.contains(id));
        if !all_promoted {
            continue;
        }

        // Check cutoff if specified
        if let Some(cutoff_dt) = &cutoff_dt {
            if ch.updated_at > *cutoff_dt {
                // Channel was updated after cutoff, skip
                continue;
            }
        }

        if dry_run {
            println!(
                "[dry-run] Would delete channel '{}' ({} changesets)",
                ch.name,
                ch.changesets.len()
            );
        } else {
            // Delete locally
            if let Err(e) = repo.delete_channel(&ch.name) {
                eprintln!("Failed to delete local channel '{}': {}", ch.name, e);
                continue;
            }
            println!("Deleted local channel '{}'", ch.name);

            // Delete remotely if requested
            if remote {
                let sync_client = crate::sync_client::SyncClient::from_repo(&repo)?;
                let request = dyna_core::protocol::DeleteChannelRequest {
                    channel: ch.name.clone(),
                    force: true,
                };
                match sync_client.delete_channel(&request).await {
                    Ok(resp) => {
                        if resp.success {
                            println!("Deleted remote channel '{}'", ch.name);
                        } else {
                            let err = resp.error.unwrap_or_default();
                            eprintln!("Failed to delete remote channel '{}': {}", ch.name, err);
                        }
                    }
                    Err(e) => {
                        eprintln!("Failed to delete remote channel '{}': {}", ch.name, e);
                    }
                }
            }

            deleted_count += 1;
        }
    }

    if dry_run {
        println!("Dry run complete. No channels were deleted.");
    } else {
        println!("Cleanup complete. {} channel(s) deleted.", deleted_count);
    }

    Ok(())
}

use anyhow::Result;
use chrono::{DateTime, NaiveDateTime, Utc};
use itertools::izip;
use std::collections::HashSet;

/// Parse a cutoff string into a NaiveDateTime.
///
/// Supports:
/// - ISO 8601 datetime: "2026-01-15T00:00:00"
/// - Duration suffixes: "30d", "6m", "1y", "24h", "3600s", "90min"
fn parse_cutoff(s: &str) -> Result<DateTime<Utc>> {
    // Try ISO 8601 first
    NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S")
        .or_else(|_| NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S%.f"))
        .map(|dt| dt.and_utc())
        .or_else(|_| {
            let now = Utc::now();

            // Try duration patterns
            let pairs: &[(&str, fn(i64) -> chrono::TimeDelta)] = &[
                ("min", |n| chrono::TimeDelta::minutes(n)),
                ("d", |n| chrono::TimeDelta::days(n)),
                ("m", |n| chrono::TimeDelta::days(n * 30)),
                ("y", |n| chrono::TimeDelta::days(n * 365)),
                ("h", |n| chrono::TimeDelta::hours(n)),
                ("s", |n| chrono::TimeDelta::seconds(n)),
            ];

            izip!(pairs)
                .find_map(|(suffix, make_delta)| {
                    s.strip_suffix(suffix).and_then(|num| {
                        num.trim()
                            .parse::<i64>()
                            .ok()
                            .map(|n| now - make_delta(n))
                    })
                })
                .ok_or_else(|| {
                    anyhow::anyhow!(
                        "Invalid cutoff format '{}'. Use ISO 8601 (2026-01-15T00:00:00) or duration (30d, 6m, 1y, 24h, 90min, 3600s).",
                        s
                    )
                })
        })
}

/// Execute the `cleanup` command.
///
/// Finds all channels (excluding main) whose changesets are a subset of main's,
/// optionally filtered by a cutoff date, and deletes them.
pub async fn execute(cutoff: Option<String>, remote: bool, dry_run: bool) -> Result<()> {
    let repo = crate::repository::Repository::find_current()?;

    let cutoff_dt: Option<DateTime<Utc>> = cutoff.as_ref().map(|s| parse_cutoff(s)).transpose()?;

    // Load main channel
    let main_channel = repo.load_channel("main")?;
    let main_set: HashSet<&String> = izip!(&main_channel.changesets).map(|id| id).collect();

    // List all local channels
    let channels = repo.list_channels()?;

    let candidates: Vec<_> = izip!(&channels)
        .filter(|ch| ch.name != "main")
        .filter(|ch| izip!(&ch.changesets).all(|id| main_set.contains(id)))
        .filter(|ch| {
            cutoff_dt
                .as_ref()
                .map(|dt| ch.updated_at <= *dt)
                .unwrap_or(true)
        })
        .map(|ch| ch)
        .collect();

    let deleted_count = if dry_run {
        izip!(&candidates).for_each(|ch| {
            println!(
                "[dry-run] Would delete channel '{}' ({} changesets)",
                ch.name,
                ch.changesets.len()
            );
        });
        0
    } else {
        izip!(&candidates).fold(0usize, |count, ch| {
            repo.delete_channel(&ch.name)
                .map(|_| {
                    println!("Deleted local channel '{}'", ch.name);

                    if remote {
                        let config = repo.load_config().ok();
                        let remote_url = config.as_ref().and_then(|c| c.remote_url.as_ref());
                        if let Some(url) = remote_url {
                            let sync_client = crate::sync_client::SyncClient::new(url);
                            let request = dyna_core::protocol::DeleteChannelRequest {
                                channel: ch.name.clone(),
                                force: true,
                            };
                            let _ = tokio::runtime::Handle::current().block_on(async {
                                sync_client
                                    .delete_channel(&request)
                                    .await
                                    .map(|resp| {
                                        resp.success
                                            .then(|| {
                                                println!("Deleted remote channel '{}'", ch.name)
                                            })
                                            .unwrap_or_else(|| {
                                                let err = resp.error.clone().unwrap_or_default();
                                                eprintln!(
                                                    "Failed to delete remote channel '{}': {}",
                                                    ch.name, err
                                                );
                                            });
                                    })
                                    .unwrap_or_else(|e| {
                                        eprintln!(
                                            "Failed to delete remote channel '{}': {}",
                                            ch.name, e
                                        );
                                    })
                            });
                        }
                    }

                    count + 1
                })
                .unwrap_or_else(|e| {
                    eprintln!("Failed to delete local channel '{}': {}", ch.name, e);
                    count
                })
        })
    };

    if dry_run {
        println!("Dry run complete. No channels were deleted.");
    } else {
        println!("Cleanup complete. {} channel(s) deleted.", deleted_count);
    }

    Ok(())
}

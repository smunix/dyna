//! # lazy-cat demo
//!
//! Connects to a running Dyna server, lazily loads resources from a channel,
//! and prints detailed live updates (metadata + content) as they arrive.
//!
//! ## Usage
//!
//! ```bash
//! # Start a dyna-server first, then:
//! cargo run --bin demo -p lazy-cat -- http://localhost:8080 main
//! ```

use lazy_cat::LazyClient;
use std::env;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Initialise tracing so we can see WebSocket reconnect messages
    tracing_subscriber::fmt()
        .with_env_filter("lazy_cat=info")
        .init();

    let args: Vec<String> = env::args().collect();
    let server_url = args
        .get(1)
        .map(String::as_str)
        .unwrap_or("http://localhost:8080");
    let channel = args.get(2).map(String::as_str).unwrap_or("main");

    println!("Connecting to {} (channel: {})…", server_url, channel);
    let client = LazyClient::connect(server_url, channel).await?;

    // -----------------------------------------------------------------------
    // Register a live-update callback that prints full details
    // -----------------------------------------------------------------------
    client
        .on_update(|event| {
            println!("\n{}", "═".repeat(72));
            println!(
                "  ⚡ Live update — {:?} on channel '{}'",
                event.kind, event.channel
            );
            println!("     Timestamp : {}", event.timestamp);
            event
                .new_head
                .as_ref()
                .map(|h| println!("     New HEAD  : {}", h));
            println!(
                "     Resources : {} affected",
                event.affected_resource_ids.len()
            );
            println!("{}", "─".repeat(72));

            // Print per-changeset metadata
            event.changesets.iter().enumerate().for_each(|(i, cs)| {
                println!("\n  Changeset #{} [{}]", i + 1, cs.change_id);
                println!("    Author     : {}", cs.author);
                println!("    Message    : {}", cs.message);
                println!("    Patches    : {}", cs.patch_count);
                println!(
                    "    Resources  : {}",
                    cs.affected_resources.join(", ")
                );
            });

            // Print updated resource content
            if !event.updated_snapshots.is_empty() {
                println!("\n{}", "─".repeat(72));
                println!("  Updated resource snapshots:\n");

                event
                    .updated_snapshots
                    .iter()
                    .for_each(|(rid, value)| {
                        println!("  📄 {rid}:");
                        serde_json::to_string_pretty(value)
                            .map(|pretty| {
                                pretty.lines().for_each(|line| {
                                    println!("     {line}");
                                });
                            })
                            .unwrap_or_else(|e| {
                                println!("     <serialisation error: {e}>");
                            });
                        println!();
                    });
            } else {
                println!(
                    "\n  (no snapshots available — resources may not have been loaded yet)"
                );
            }

            println!("{}", "═".repeat(72));
        })
        .await;

    // -----------------------------------------------------------------------
    // List all known resource IDs
    // -----------------------------------------------------------------------
    let ids = client.list_resources().await?;
    println!("\nKnown resources ({}):", ids.len());
    ids.iter().take(20).for_each(|id| println!("  • {id}"));
    if ids.len() > 20 {
        println!("  … and {} more", ids.len() - 20);
    }

    // -----------------------------------------------------------------------
    // Lazily fetch the first resource (if any)
    // -----------------------------------------------------------------------
    if let Some(first_id) = ids.first() {
        println!("\nFetching '{first_id}'…");
        let value = client.get(first_id).await?;
        println!("{}", serde_json::to_string_pretty(&value)?);
    }

    // -----------------------------------------------------------------------
    // Stream all resources through a continuation
    // -----------------------------------------------------------------------
    println!("\nStreaming all resources via for_each_all…");
    let mut count = 0usize;
    client
        .for_each_all(|id, val| {
            count += 1;
            if count <= 5 {
                println!(
                    "  {id}: {}",
                    serde_json::to_string(val)
                        .unwrap_or_else(|_| "<error>".to_string())
                        .chars()
                        .take(80)
                        .collect::<String>()
                );
            }
        })
        .await?;
    println!("  … streamed {count} resource(s) total");

    // -----------------------------------------------------------------------
    // Keep the process alive to receive WebSocket updates
    // -----------------------------------------------------------------------
    println!("\nListening for live updates (Ctrl-C to quit)…");
    tokio::signal::ctrl_c().await?;

    Ok(())
}

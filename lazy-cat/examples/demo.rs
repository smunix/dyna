//! # lazy-cat demo
//!
//! Connects to a running Dyna server, lazily loads resources from a channel,
//! and prints live updates as they arrive.
//!
//! ## Usage
//!
//! ```bash
//! # Start a dyna-server first, then:
//! cargo run --example lazy-cat-demo -- http://localhost:8080 main
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

    // Register a live-update callback
    client
        .on_update(|affected| {
            println!(
                "\n  ⚡ Live update — {} resource(s) changed: {:?}",
                affected.len(),
                affected
            );
        })
        .await;

    // List all known resource IDs
    let ids = client.list_resources().await?;
    println!("\nKnown resources ({}):", ids.len());
    ids.iter().for_each(|id| println!("  • {id}"));

    // Lazily fetch the first resource (if any)
    if let Some(first_id) = ids.first() {
        println!("\nFetching '{first_id}'…");
        let value = client.get(first_id).await?;
        println!("{}", serde_json::to_string_pretty(&value)?);
    }

    // Fetch all resources at once
    println!("\nFetching all resources…");
    let all = client.get_all().await?;
    all.iter().for_each(|(id, val)| {
        println!(
            "  {id}: {}",
            serde_json::to_string(val)
                .unwrap_or_else(|_| "<error>".to_string())
                .chars()
                .take(80)
                .collect::<String>()
        );
    });

    // Keep the process alive to receive WebSocket updates
    println!("\nListening for live updates (Ctrl-C to quit)…");
    tokio::signal::ctrl_c().await?;

    Ok(())
}

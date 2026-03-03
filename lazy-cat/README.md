# lazy-cat

Lazy, on-demand resource loader for Dyna servers — Rust edition.

`LazyClient` uses `dyna-cli` as a local in-memory caching layer and a WebSocket connection for live updates. Resources are fetched from the remote server only when first requested.

## Quick start

```rust
use lazy_cat::LazyClient;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let client = LazyClient::connect("http://localhost:8080", "main").await?;

    // Fetch a single resource on demand
    let value = client.get("acme.entity.User").await?;
    println!("{}", serde_json::to_string_pretty(&value)?);

    // List all available resource IDs (metadata only)
    let ids = client.list_resources().await?;
    println!("Available: {:?}", ids);

    // Register a live-update callback
    client.on_update(|affected| {
        println!("Updated: {:?}", affected);
    }).await;

    Ok(())
}
```

## Architecture

```
┌──────────────┐     clone/pull     ┌──────────────┐
│  LazyClient  │ ◄────────────────► │  dyna-server  │
│              │                    │              │
│  ┌────────┐  │     WebSocket      │  /api/v1/ws  │
│  │MemRepo │  │ ◄──────────────── │              │
│  └────────┘  │                    └──────────────┘
└──────────────┘
```

- **Clone**: On `connect()`, a lightweight clone fetches channel metadata and changeset history
- **Lazy load**: Resource bodies are materialised only on first `get()` call
- **Live sync**: A background WebSocket listener automatically pulls new changesets when the server broadcasts push/promotion notifications

## Running the demo

```bash
# Start a dyna-server first, then:
cargo run --bin demo -p lazy-cat -- http://localhost:8080 main

# Or via Nix:
nix run .#lazy-cat-demo -- http://localhost:8080 main
```

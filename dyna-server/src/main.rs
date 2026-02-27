//! Dyna remote server — changeset-centric distributed CRUD backend.
//!
//! This server uses the elfo-rs actor framework to manage three actor groups:
//!
//! - **API Actor**: Bridges HTTP (axum) with elfo message-passing.
//!   Exposes endpoints for push, pull, clone, promote, and changeset retrieval.
//! - **Changeset Actor**: Core business logic for validating, storing,
//!   and retrieving changesets. Handles channel management and promotion.
//! - **Storage Actor**: Encapsulates all S3 I/O via the `object_store` crate.
//!   Stores changesets, patches, channels, and snapshots.
//!
//! The actors communicate via elfo's typed message-passing system, providing
//! fault isolation and clean separation of concerns. The topology is:
//! `API → Changeset → Storage`.

mod actors;
mod messages;

use elfo::config::AnyConfig;
use std::sync::Arc;

/// Build the elfo topology with all actor groups and their connections.
fn topology(bind_addr: String, store: Arc<dyn object_store::ObjectStore>) -> elfo::Topology {
    let topology = elfo::Topology::empty();

    // System actors
    let logger = elfo::batteries::logger::init();
    let loggers = topology.local("system.loggers");
    let configurers = topology.local("system.configurers").entrypoint();

    // Application actors
    let api = topology.local("api");
    let changeset = topology.local("changeset");
    let storage = topology.local("storage");

    // Define message routing:
    // API -> Changeset (for business logic)
    api.route_all_to(&changeset);
    // Changeset -> Storage (for S3 I/O)
    changeset.route_all_to(&storage);

    // Mount actor implementations
    api.mount(actors::api::new(bind_addr));
    changeset.mount(actors::changeset::new());
    storage.mount(actors::storage::new(store));
    loggers.mount(logger);

    // Use a fixture config (no config file needed)
    configurers.mount(elfo::batteries::configurer::fixture(
        &topology,
        AnyConfig::default(),
    ));

    topology
}

#[tokio::main]
async fn main() {
    // Parse configuration from environment variables
    let bind_addr = std::env::var("DYNA_BIND_ADDR").unwrap_or_else(|_| "0.0.0.0:8080".into());
    let use_s3 = std::env::var("DYNA_USE_S3").unwrap_or_else(|_| "false".into());

    println!(
        r#"
  ____
 |  _ \ _   _ _ __   __ _
 | | | | | | | '_ \ / _` |
 | |_| | |_| | | | | (_| |
 |____/ \__, |_| |_|\__,_|
        |___/
  Distributed CRUD Server v{}
"#,
        env!("CARGO_PKG_VERSION")
    );

    println!("Configuration:");
    println!("  Bind address: {}", bind_addr);

    // Create the object store
    let store: Arc<dyn object_store::ObjectStore> = if use_s3 == "true" {
        println!("  Storage: S3 (from environment)");
        match actors::storage::create_s3_store() {
            Ok(s) => s,
            Err(e) => {
                eprintln!("Failed to create S3 store: {}", e);
                eprintln!("Falling back to in-memory store.");
                actors::storage::create_memory_store()
            }
        }
    } else {
        println!("  Storage: In-memory (development mode)");
        println!("  Set DYNA_USE_S3=true and configure AWS credentials for production.");
        actors::storage::create_memory_store()
    };

    println!("\nStarting actor system...\n");

    // Build and start the elfo topology
    let topo = topology(bind_addr, store);
    elfo::init::start(topo).await;
}

//! Shared server bootstrap logic used by all binary targets.
//!
//! Each binary (`dyna-server`, `dyna-server-je`, `dyna-server-mim`) calls
//! [`run`] after optionally installing a custom global allocator.

use crate::actors;
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

/// Run the Dyna server.  Call this from each binary's `main` after setting up
/// the global allocator (if any).
pub async fn run(allocator_label: &str) {
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
  Allocator: {}
"#,
        env!("CARGO_PKG_VERSION"),
        allocator_label,
    );

    println!("Configuration:");
    println!("  Bind address: {}", bind_addr);
    println!("  Allocator:    {}", allocator_label);

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

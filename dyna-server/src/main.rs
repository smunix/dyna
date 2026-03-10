//! Dyna remote server — changeset-centric distributed CRUD backend.
//!
//! **Binary: `dyna-server`** — uses the system default allocator.
//!
//! See also:
//! - `dyna-server-je`  — jemalloc allocator
//! - `dyna-server-mim` — mimalloc allocator

mod actors;
mod messages;
mod ws;
mod server;

#[tokio::main]
async fn main() {
    server::run("system (default)").await;
}

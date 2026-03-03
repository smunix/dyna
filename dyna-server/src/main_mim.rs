//! Dyna remote server — changeset-centric distributed CRUD backend.
//!
//! **Binary: `dyna-server-mim`** — uses **mimalloc** as the global allocator.

mod actors;
mod messages;
mod ws;
mod server;

#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

#[tokio::main]
async fn main() {
    server::run("mimalloc").await;
}

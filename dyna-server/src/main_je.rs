//! Dyna remote server — changeset-centric distributed CRUD backend.
//!
//! **Binary: `dyna-server-je`** — uses **jemalloc** as the global allocator
//! via `tikv-jemallocator`.

mod actors;
mod messages;
mod ws;
mod server;

#[global_allocator]
static GLOBAL: tikv_jemallocator::Jemalloc = tikv_jemallocator::Jemalloc;

#[tokio::main]
async fn main() {
    server::run("jemalloc").await;
}

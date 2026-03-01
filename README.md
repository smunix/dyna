# Dyna

**A distributed CRUD system for collaborative JSON resource editing.**

Dyna enables multiple users to concurrently edit a shared set of JSON resources stored in S3, with full version history, conflict resolution, and a powerful changeset-based model. It consists of four crates and a browser-based Elm UI:

| Crate | Description |
|-------|-------------|
| **dyna-core** | Shared library: models, diff/patch engine, protocol types, compression, notifications. |
| **dyna-cli** | Command-line client with VFS-abstracted local repository storage. |
| **dyna-server** | Remote server built on [elfo-rs](https://github.com/elfo-rs/elfo) actors + Axum, with S3 storage and WebSocket notifications. |
| **dyna-wasm** | WebAssembly client for browser-based usage, with in-memory VFS and `web-sys` fetch/WebSocket. |
| **dyna-elm** | Elm-based browser UI that uses `dyna-wasm` for full collaborative JSON editing. |

---

## Architecture

```
┌─────────────────────┐                          ┌──────────────────────────────────────┐
│   dyna CLI (local)  │         HTTP/gzip         │         dyna-server (remote)         │
│                     │◄────────────────────────►│                                      │
│  .dyna/  (VFS)      │         REST API          │  ┌──────────────┐                    │
│  ├── changesets/    │                           │  │ API Actor     │ (axum + elfo)     │
│  ├── snapshots/     │                           │  │ + WS Hub      │                    │
│  ├── channels/      │                           │  └──────┬───────┘                    │
│  ├── staging/       │                           │         │ elfo messages              │
│  └── config.toml    │                           │  ┌──────▼───────┐                    │
└─────────────────────┘                           │  │ Changeset     │                    │
                                                  │  │ Actor         │ (business logic)  │
┌─────────────────────┐                           │  └──────┬───────┘                    │
│  dyna-elm (browser)  │         HTTP/gzip         │         │ elfo messages              │
│  ┌───────────────┐  │◄────────────────────────►│  ┌──────▼───────┐                    │
│  │  Elm UI        │  │         + WebSocket       │  │ Storage       │ (object_store)    │
│  │  (ports)       │  │                           │  │ Actor         │                    │
│  └───────┬───────┘  │                           │  └──────┬───────┘                    │
│          │ JS FFI    │                           │         │                            │
│  ┌───────▼───────┐  │                           │  ┌──────▼───────┐                    │
│  │  dyna-wasm     │  │                           │  │  S3 Bucket    │                    │
│  │  (MemoryFS)    │  │                           │  └──────────────┘                    │
│  └───────────────┘  │                           │                                      │
└─────────────────────┘                           └──────────────────────────────────────┘
```

## Key Concepts

| Concept | Description |
|---------|-------------|
| **Resource** | A JSON document identified by a dotted resource ID (e.g., `acme.entity.User`), stored as a file hierarchy. |
| **Patch** | An immutable set of JSON Patch (RFC 6902) operations targeting a single resource, with parent and result snapshots. |
| **Changeset** | A group of patches committed together, with a stable `change_id`, `commit_hash`, parent tracking, and author. The primary unit of work. |
| **Channel** | A named bookmark pointing to an ordered list of changesets, similar to a Git branch. |
| **Promote** | The act of merging changesets from a feature channel into `main`. Triggers WebSocket notifications to all connected clients. |
| **Squash** | Merge a child changeset into its parent, combining patches. |
| **Restore** | Revert a file to its snapshot state from a channel head or specific changeset. |

## CLI Commands

| Command | Description |
|---------|-------------|
| `dyna init` | Initialize a new repository. |
| `dyna clone <url>` | Clone a repository from a remote server, recreating the full filesystem hierarchy. |
| `dyna add <pattern>` | Stage JSON resource file(s), directory, or glob pattern for the next commit. |
| `dyna add --delete <pattern>` | Stage the removal of deleted tracked file(s), directory, or glob pattern. |
| `dyna commit -m "msg"` | Record staged changes as a new changeset. Removes snapshots for deletions. |
| `dyna describe -m "msg"` | Amend the message of the current working changeset. |
| `dyna squash` | Squash a changeset into its parent. Supports `--revision`, `--into`, `--message`. |
| `dyna push` | Push local changesets to the remote server. |
| `dyna pull` | Fetch and merge remote changesets. |
| `dyna status` | Show working directory status: staged, modified, deleted tracked files, and unstaged modifications on staged files. |
| `dyna log` | Display changeset history. Use `--changeset <id>` for details, `--patches` for operations. |
| `dyna diff` | Show diffs for staged files. |
| `dyna history <resource_id>` | Query the change history of a specific resource from the remote server. Use `--verbose` for operations. |
| `dyna resolve <file>` | Interactively resolve conflicts. |
| `dyna restore <file>` | Restore a file to its snapshot state. Supports `--channel` and `--changeset` flags. |
| `dyna channel <name>` | Switch to or create a channel (bookmark). |
| `dyna promote` | Promote current channel's changesets to `main`. Supports `--channel` to specify source. |

## Getting Started

### Building with Nix (recommended)

Dyna ships with a Nix flake that uses [flake-parts](https://flake.parts) and [crane](https://crane.dev) for reproducible builds of all components.

```bash
# Enter the development shell (all tools available: Rust, wasm-pack, Elm, etc.)
nix develop

# Build individual packages
nix build .#dyna-cli
nix build .#dyna-server
nix build .#dyna-wasm
nix build .#dyna-elm

# Build everything
nix flake check

# Run directly
nix run .#dyna-cli
nix run .#dyna-server

# Build Docker/OCI images for deployment
nix build .#dyna-server-image
nix build .#dyna-elm-image
```

The `nix develop` shell provides:

| Tool | Purpose |
|------|---------|
| Rust toolchain (stable + wasm32 target) | Build all Rust crates natively and for WASM |
| rust-analyzer, clippy, rustfmt | IDE support and linting |
| wasm-pack, wasm-bindgen-cli | WASM packaging and JS/TS glue generation |
| elm, elm-format, elm-test, elm-review | Elm development toolchain |
| cargo-nextest, cargo-watch | Testing and live-reload |

### Building with Cargo (manual)

```bash
# Build all native crates
cargo build --release

# Build the WASM package
wasm-pack build dyna-wasm --target web --out-dir ../pkg

# Build the Elm UI
cd dyna-elm && elm make src/Main.elm --output=public/elm.js
```

### Nix Flake Outputs

| Output | Description |
|--------|-------------|
| `packages.dyna-cli` | Native CLI binary |
| `packages.dyna-server` | Native server binary |
| `packages.dyna-wasm` | WASM + JS/TS glue (via wasm-bindgen) |
| `packages.dyna-elm` | Compiled Elm app with bundled WASM package |
| `packages.dyna-server-image` | OCI/Docker image for the server |
| `packages.dyna-elm-image` | OCI/Docker image serving the Elm UI (static-web-server) |
| `apps.dyna-cli` | `nix run .#dyna-cli` |
| `apps.dyna-server` | `nix run .#dyna-server` |
| `devShells.default` | Full development environment |
| `checks.*` | Clippy, fmt, nextest, and build checks |

### Use the CLI

```bash
# Initialize a new repository
mkdir my-project && cd my-project
dyna init

# Configure remote (edit .dyna/config.toml)
# remote_url = "http://localhost:8080"

# Create and edit a resource
mkdir -p data/users
echo '{"name": "Alice", "role": "admin"}' > data/users/config.json
dyna add data/users/config.json
dyna commit -m "Add user Alice"

# Push to remote
dyna push

# Work on a feature channel
dyna channel -c feature-x
echo '{"name": "Bob"}' > data/users/bob.json
dyna add data/users/bob.json
dyna commit -m "Add user Bob"
dyna push

# Promote to main
dyna promote

# View history of a resource
dyna history data.users.config --verbose

# Delete a resource
rm data/users/bob.json
dyna add --delete data/users/bob.json
dyna commit -m "Remove Bob"

# Delete an entire directory
rm -rf data/users/
dyna add --delete "data/users/"
dyna commit -m "Remove all users"

# Restore a file from a specific channel
dyna restore data/users/config.json --channel main

# Squash the last two changesets
dyna squash
```

### Use the Elm UI

1. Start the dyna-server: `cargo run -p dyna-server`
2. Build the WASM package: `wasm-pack build dyna-wasm --target web --out-dir ../pkg`
3. Copy `pkg/` into `dyna-elm/public/pkg/`
4. Open `dyna-elm/public/index.html` in a browser
5. Enter the server URL and start editing resources

## Compression

All data is gzip-compressed at both the **storage** and **transport** layers:

| Layer | What | How |
|-------|------|-----|
| **Storage (CLI)** | All `.dyna/` metadata files | `flate2` gzip via VFS helpers; transparent read (backwards-compatible) |
| **Storage (WASM)** | All in-memory `.dyna/` metadata | Same compression module via `dyna-core` |
| **Storage (Server)** | S3 objects (`*.json.gz`) | Compressed before PUT; fallback to `.json` on GET |
| **Transport (Request)** | HTTP request bodies | `Content-Encoding: gzip` header; server middleware decompresses |
| **Transport (Response)** | HTTP response bodies | `tower-http` CompressionLayer; clients send `Accept-Encoding: gzip` |

All reads use transparent decompression (gzip magic byte detection), ensuring backwards compatibility with uncompressed data.

## WebSocket Notifications

The server broadcasts real-time notifications via WebSocket on `/ws`:

| Event | Trigger | Payload |
|-------|---------|---------|
| `promotion` | `dyna promote` | Source/target channel, promoted changeset IDs, affected resource IDs, new head |
| `push` | `dyna push` | Channel name, pushed changeset IDs, affected resource IDs |

Clients (dyna-wasm / dyna-elm) can subscribe to receive these events and update their UI in real-time.

## S3 Object Layout

```
s3://dyna-bucket/
├── changesets/
│   ├── <commit_hash>.json.gz    # Immutable changeset objects (gzip-compressed)
│   └── ...
├── channels/
│   ├── main.json.gz             # Channel metadata (ordered list of change_ids)
│   └── feature-x.json.gz
└── snapshots/
    ├── <resource_id>.json.gz    # Latest materialized resource state
    └── ...
```

## Filesystem Abstraction (VFS)

Both `dyna-cli` and `dyna-wasm` use the [`vfs`](https://docs.rs/vfs) crate for filesystem abstraction:

- **dyna-cli**: `PhysicalFS` for real disk I/O
- **dyna-wasm**: `MemoryFS` for in-browser memory storage
- **Tests**: Can use `MemoryFS` for fast, isolated testing

The `Repository` struct provides a unified API (`read_work_file`, `write_work_file`, `write_resource_file`, etc.) that works identically across both backends.

## Resource ID ↔ Filesystem Mapping

Resource IDs use dot-separated segments that map to filesystem paths:

| Resource ID | Filesystem Path |
|-------------|----------------|
| `data.users.config` | `data/users/config.json` |
| `schemas.v2.order` | `schemas/v2/order.json` |
| `acme.entity.User` | `acme/entity/User.json` |

## License

See individual crate directories for license information.

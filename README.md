# Dyna

**A distributed CRUD system for collaborative JSON resource editing, with lazy on-demand loading clients for every major platform.**

Dyna enables multiple users to concurrently edit a shared set of JSON resources stored in S3, with full version history, conflict resolution, and a powerful changeset-based model. The ecosystem spans thirteen packages across five languages:

## Project Map

### Core Platform

| Crate | Language | Description |
|-------|----------|-------------|
| **dyna-core** | Rust | Shared library: models, diff/patch engine (RFC 6902), protocol types, compression, notifications. |
| **dyna-cli** | Rust | Command-line client with VFS-abstracted local repository storage. |
| **dyna-server** | Rust | Remote server built on [elfo-rs](https://github.com/elfo-rs/elfo) actors + Axum, with S3 storage and WebSocket notifications. |
| **dyna-wasm** | Rust → WASM | Full WebAssembly client for browser-based usage, with in-memory VFS and `web-sys` fetch/WebSocket. |
| **dyna-app** | Elm | Browser UI that uses `dyna-wasm` for full collaborative JSON editing. |
| **dyna-py** | Python (PyO3) | Python bindings that call directly into `dyna-cli`, with a `click`-based CLI. |
| **dyna-go** | Go | Go client library with `afero` filesystem abstraction for local repository storage. |

### Lazy On-Demand Loading Clients

| Crate | Language | Description |
|-------|----------|-------------|
| **lazy-cat** | Rust | Async lazy resource loader — materialises snapshots on demand from changesets, with WebSocket live updates. |
| **lazy-go** | Go | Go lazy resource loader — mirrors lazy-cat's API with goroutine-based WebSocket listener. |
| **lazy-py** | Python | Async Python lazy resource loader — `asyncio`/`websockets` based, with `UpdateEvent` dataclass. |
| **lazy-wasm** | Rust → WASM | Browser WASM lazy loader — `Rc<RefCell<…>>` single-threaded design with `web-sys` WebSocket. |
| **lazy-elm-demo** | Elm + JS | Two-panel Elm UI demonstrating `lazy-wasm` integration with ports and live update rendering. |

---

## Architecture

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                              Dyna Ecosystem                                │
│                                                                             │
│  ┌──────────────────────────────────────────────────────────────────────┐   │
│  │                        FULL CLIENTS                                  │   │
│  │                                                                      │   │
│  │  ┌─────────────┐  ┌─────────────┐  ┌─────────────┐  ┌────────────┐ │   │
│  │  │  dyna-cli   │  │  dyna-wasm  │  │  dyna-py    │  │  dyna-go   │ │   │
│  │  │  (Rust)     │  │  (WASM)     │  │  (Python)   │  │  (Go)      │ │   │
│  │  │  PhysicalFS │  │  MemoryFS   │  │  PyO3→CLI   │  │  afero FS  │ │   │
│  │  └──────┬──────┘  └──────┬──────┘  └──────┬──────┘  └─────┬──────┘ │   │
│  └─────────┼────────────────┼────────────────┼────────────────┼────────┘   │
│            │                │                │                │             │
│  ┌─────────┼────────────────┼────────────────┼────────────────┼────────┐   │
│  │         │          LAZY CLIENTS           │                │        │   │
│  │         │                │                │                │        │   │
│  │  ┌──────┴──────┐  ┌─────┴───────┐  ┌─────┴───────┐  ┌────┴──────┐ │   │
│  │  │  lazy-cat   │  │  lazy-wasm  │  │  lazy-py    │  │  lazy-go  │ │   │
│  │  │  (Rust)     │  │  (WASM)     │  │  (Python)   │  │  (Go)     │ │   │
│  │  │  on-demand  │  │  on-demand  │  │  on-demand  │  │  on-demand│ │   │
│  │  └──────┬──────┘  └──────┬──────┘  └──────┬──────┘  └─────┬─────┘ │   │
│  └─────────┼────────────────┼────────────────┼────────────────┼───────┘   │
│            │                │                │                │            │
│            └────────┬───────┴────────┬───────┴────────┬───────┘            │
│                     │   HTTP + WS    │                │                    │
│                     ▼                ▼                ▼                    │
│            ┌────────────────────────────────────────────────┐              │
│            │                 dyna-server                     │              │
│            │  ┌──────────┐  ┌────────────┐  ┌────────────┐ │              │
│            │  │ API Actor│→│ Changeset   │→│ Storage     │ │              │
│            │  │ (Axum)   │  │ Actor       │  │ Actor (S3) │ │              │
│            │  └──────────┘  └────────────┘  └────────────┘ │              │
│            │       ↕ WebSocket                              │              │
│            └────────────────────────────────────────────────┘              │
│                                    │                                       │
│                              ┌─────▼─────┐                                │
│                              │  S3 Bucket │                                │
│                              └───────────┘                                │
└─────────────────────────────────────────────────────────────────────────────┘
```

---

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
| **Materialise** | Reconstruct a resource's current JSON value by replaying its patches from changesets. Used by lazy clients for on-demand loading. |

---

## Lazy On-Demand Loading

The lazy loading clients solve a critical problem: **how to efficiently work with repositories containing tens of thousands of resources without downloading all of them upfront**. Instead of cloning the full repository and materialising every snapshot, lazy clients:

1. **Clone metadata only** — download channel and changeset metadata from the server (lightweight)
2. **Build a resource index** — extract known resource IDs from changeset patches (no data fetched yet)
3. **Materialise on demand** — when a resource is first accessed via `get()`, replay its patches from stored changesets to reconstruct the current JSON value
4. **Cache locally** — once materialised, the snapshot is stored locally and returned instantly on subsequent reads
5. **Stay in sync** — a WebSocket listener receives push/promotion notifications, pulls new changesets, and materialises updated snapshots automatically

### Lazy Client API (Uniform Across All Languages)

All four lazy clients expose the same conceptual API:

| Method | Description |
|--------|-------------|
| `connect(server_url, channel)` | Clone metadata, build resource index, open WebSocket for live updates. |
| `get(resource_id)` | Materialise and return a single resource's JSON value (lazy, cached). |
| `get_many(resource_ids)` | Materialise and return multiple resources at once. |
| `get_all()` | Materialise and return all known resources. |
| `list_resources()` | Return sorted list of all known resource IDs (no materialisation). |
| `for_each_all(callback)` | Stream all resources through a callback function. |
| `on_update(callback)` | Register a callback for live update events (push/promotion). |
| `channel()` | Return the current channel name. |
| `server_url()` | Return the connected server URL. |

### UpdateEvent Structure

When a live update arrives, the callback receives a rich `UpdateEvent` containing:

```
UpdateEvent {
    kind:          "push" | "promotion"
    timestamp:     ISO 8601 timestamp
    channel:       channel name
    new_head:      new HEAD changeset ID (if applicable)
    changesets: [
        {
            change_id:          changeset hash
            author:             committer name
            message:            commit message
            patch_count:        number of patches
            affected_resources: [resource_id, ...]
        },
        ...
    ]
    affected_resource_ids:  [all affected resource IDs]
    updated_snapshots:      {resource_id: JSON value, ...}
}
```

### Materialisation Flow

```
                    get("acme.user.Alice")
                            │
                            ▼
                   ┌─────────────────┐
                   │ Cached locally?  │
                   └────┬────────┬───┘
                   yes  │        │ no
                        ▼        ▼
                   ┌────────┐  ┌──────────────────────────┐
                   │ Return │  │ Load channel's changeset  │
                   │ cached │  │ list from local store     │
                   └────────┘  └────────────┬─────────────┘
                                            │
                                            ▼
                               ┌────────────────────────┐
                               │ For each changeset:     │
                               │   find patches where    │
                               │   resource_id matches   │
                               └────────────┬───────────┘
                                            │
                                            ▼
                               ┌────────────────────────┐
                               │ If result_snapshot      │
                               │   exists → use it       │
                               │ Else → apply RFC 6902   │
                               │   operations to {}      │
                               └────────────┬───────────┘
                                            │
                                            ▼
                               ┌────────────────────────┐
                               │ Save snapshot locally,  │
                               │ mark as loaded, return  │
                               └────────────────────────┘
```

### Language-Specific Examples

**Rust (lazy-cat):**
```rust
use lazy_cat::LazyClient;

let client = LazyClient::connect("http://localhost:8080", "main").await?;
println!("Known: {:?}", client.list_resources().await?);

let alice = client.get("acme.user.Alice").await?;
println!("Alice: {}", serde_json::to_string_pretty(&alice)?);

client.on_update(|event| {
    println!("Update: {} on {}", event.kind, event.channel);
    for cs in &event.changesets {
        println!("  {} by {}: {}", cs.change_id, cs.author, cs.message);
    }
    for (id, val) in &event.updated_snapshots {
        println!("  {} → {}", id, val);
    }
}).await;
```

**Go (lazy-go):**
```go
import "lazy-go/lazycat"

client, err := lazycat.Connect(ctx, "http://localhost:8080", "main")
defer client.Close()

ids := client.ListResources()
alice, _ := client.Get("acme.user.Alice")
fmt.Println(string(alice))

client.OnUpdate(func(event *lazycat.UpdateEvent) {
    fmt.Printf("Update: %s on %s\n", event.Kind, event.Channel)
    for _, cs := range event.Changesets {
        fmt.Printf("  %s by %s: %s\n", cs.ChangeID, cs.Author, cs.Message)
    }
    for id, val := range event.UpdatedSnapshots {
        fmt.Printf("  %s → %s\n", id, string(val))
    }
})
```

**Python (lazy-py):**
```python
from lazy_py import LazyClient

client = await LazyClient.connect("http://localhost:8080", "main")
ids = await client.list_resources()
alice = await client.get("acme.user.Alice")
print(json.dumps(alice, indent=2))

def on_update(event):
    print(f"Update: {event.kind} on {event.channel}")
    for cs in event.changesets:
        print(f"  {cs.change_id} by {cs.author}: {cs.message}")
    for rid, val in event.updated_snapshots.items():
        print(f"  {rid} → {json.dumps(val)}")

client.on_update(on_update)
```

**JavaScript / WASM (lazy-wasm):**
```javascript
import init, { LazyWasmClient } from './lazy_wasm.js';

await init();
const client = await LazyWasmClient.connect("http://localhost:8080", "main");

const ids = client.list_resources();
const alice = client.get("acme.user.Alice");
console.log(JSON.stringify(alice, null, 2));

client.on_update((event) => {
    console.log(`Update: ${event.kind} on ${event.channel}`);
    event.changesets.forEach(cs => {
        console.log(`  ${cs.change_id} by ${cs.author}: ${cs.message}`);
    });
    Object.entries(event.updated_snapshots).forEach(([id, val]) => {
        console.log(`  ${id} →`, val);
    });
});
```

---

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

---

## Getting Started

### Building with Nix (recommended)

Dyna ships with a Nix flake that uses [flake-parts](https://flake.parts) and [crane](https://crane.dev) for reproducible builds of all components.

```bash
# Enter the development shell (all tools available: Rust, wasm-pack, Elm, Go, Python, etc.)
nix develop

# Build individual packages
nix build .#dyna-cli
nix build .#dyna-server
nix build .#dyna-wasm
nix build .#dyna-app
nix build .#dyna-py
nix build .#dyna-go
nix build .#lazy-cat
nix build .#lazy-go
nix build .#lazy-wasm
nix build .#lazy-elm-demo

# Build everything
nix flake check

# Run directly
nix run .#dyna-cli
nix run .#dyna-server
nix run .#dyna-app-serve         # build & serve the Elm UI locally
nix run .#dyna-py                # Python CLI

# Run demo apps
nix run .#lazy-cat-demo -- http://localhost:8080 main
nix run .#lazy-go-demo -- http://localhost:8080 main
nix run .#lazy-py-demo -- http://localhost:8080 main
nix run .#lazy-elm-serve         # build & serve the Elm lazy UI on port 3001

# Build Docker/OCI images for deployment
nix build .#dyna-server-image
nix build .#dyna-app-image
```

The `nix develop` shell provides:

| Tool | Purpose |
|------|---------|
| **Pre-built binaries** | |
| `dyna` (dyna-cli) | CLI binary, directly executable |
| `dyna-server` | Server binary, directly executable |
| `dyna-server-je` | Server with jemalloc allocator |
| `dyna-server-mim` | Server with mimalloc allocator |
| `dyna-app-serve` | Build & serve the Elm UI locally |
| `dyna-py` | Python CLI (via PyO3), directly executable |
| `dyna-go` | Go client library |
| `lazy-cat` | Rust lazy resource loader |
| `lazy-go` | Go lazy resource loader |
| `lazy-wasm` | WASM lazy resource loader |
| `lazy-elm-serve` | Build & serve the Elm lazy UI |
| **Demo apps** (`nix run .#<name>`) | |
| `lazy-cat-demo` | Rust lazy loader demo |
| `lazy-go-demo` | Go lazy loader demo |
| `lazy-py-demo` | Python lazy loader demo |
| `lazy-elm-serve` | Elm lazy UI demo |
| **Development toolchain** | |
| Rust (stable + wasm32 target) | Build all Rust crates natively and for WASM |
| rust-analyzer, clippy, rustfmt | IDE support and linting |
| wasm-pack, wasm-bindgen-cli | WASM packaging and JS/TS glue generation |
| elm, elm-format, elm-test, elm-review | Elm development toolchain |
| Go 1.23+ | Build Go crates |
| Python 3 + maturin + click | Rebuild dyna-py from source |
| cargo-nextest, cargo-watch | Testing and live-reload |

### Building with Cargo (manual)

```bash
# Build all native Rust crates
cargo build --release

# Build the WASM packages
wasm-pack build dyna-wasm --target web --out-dir ../pkg
wasm-pack build lazy-wasm --target web --out-dir ../lazy-elm-demo/public/pkg

# Build the Elm UIs
cd dyna-app && elm make src/Main.elm --output=public/elm.js
cd lazy-elm-demo && elm make src/Main.elm --output=public/elm.js

# Build Go packages
cd lazy-go && go build ./...
```

### Nix Flake Outputs

| Output | Description |
|--------|-------------|
| **Packages** | |
| `packages.dyna-cli` | Native CLI binary |
| `packages.dyna-server` | Native server binary |
| `packages.dyna-wasm` | WASM + JS/TS glue (via wasm-bindgen) |
| `packages.dyna-app` | Compiled Elm app with bundled WASM package |
| `packages.dyna-py` | Python package with native Rust extension (via maturin) |
| `packages.dyna-go` | Go client library |
| `packages.lazy-cat` | Rust lazy loader binary (with demo) |
| `packages.lazy-go` | Go lazy loader binary (with demo) |
| `packages.lazy-wasm` | WASM lazy loader + JS/TS glue |
| `packages.lazy-elm-demo` | Compiled Elm lazy UI with bundled WASM |
| **OCI Images** | |
| `packages.dyna-server-image` | Docker image for the server |
| `packages.dyna-app-image` | Docker image serving the Elm UI (static-web-server) |
| **Apps** (`nix run .#<name>`) | |
| `apps.dyna-cli` | Run the CLI |
| `apps.dyna-server` | Run the server |
| `apps.dyna-app-serve` | Build & serve the full Elm UI |
| `apps.dyna-py` | Run the Python CLI |
| `apps.lazy-cat-demo` | Run the Rust lazy loader demo |
| `apps.lazy-go-demo` | Run the Go lazy loader demo |
| `apps.lazy-py-demo` | Run the Python lazy loader demo |
| `apps.lazy-elm-serve` | Build & serve the Elm lazy UI |
| **Other** | |
| `devShells.default` | Full development environment with all binaries on PATH |
| `checks.*` | Clippy, fmt, nextest, and build checks |

---

## Use the CLI

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

---

## Use the Elm UI

1. Start the dyna-server: `nix run .#dyna-server` (or `cargo run -p dyna-server`)
2. Serve the UI: `nix run .#dyna-app-serve`
3. Open the URL in a browser and start editing resources

For the lazy loading demo UI:
1. Start the dyna-server: `nix run .#dyna-server`
2. Serve the lazy UI: `nix run .#lazy-elm-serve`
3. Open `http://localhost:3001` in a browser
4. Enter the server URL and channel, then click Connect

---

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

---

## WebSocket Notifications

The server broadcasts real-time notifications via WebSocket on `/api/v1/ws`:

| Event | Trigger | Payload |
|-------|---------|---------|
| `promotion` | `dyna promote` | Source/target channel, promoted changeset IDs, affected resource IDs, new head |
| `push` | `dyna push` | Channel name, pushed changeset IDs, affected resource IDs |

All clients (full and lazy) can subscribe to receive these events. Full clients (dyna-wasm, dyna-app) update their local repository. Lazy clients (lazy-cat, lazy-go, lazy-py, lazy-wasm) pull new changesets and materialise updated snapshots, delivering rich `UpdateEvent` objects to registered callbacks.

---

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
    ├── main/
    │   ├── <resource_id>.json.gz    # Latest materialized resource state
    │   └── ...
    └── feature-x/
        └── ...
```

---

## Filesystem Abstraction (VFS)

Both full and lazy clients use the [`vfs`](https://docs.rs/vfs) crate for filesystem abstraction:

| Client | Backend | Storage |
|--------|---------|---------|
| **dyna-cli** | `PhysicalFS` | Real disk I/O |
| **dyna-wasm** | `MemoryFS` | In-browser memory |
| **lazy-cat** | `PhysicalFS` (via dyna-cli) | Temp directory |
| **lazy-wasm** | `MemoryFS` | In-browser memory |
| **lazy-go** | `afero.OsFs` | Temp directory |
| **lazy-py** | `PhysicalFS` (via dyna-cli/PyO3) | Temp directory |
| **Tests** | `MemoryFS` | Fast, isolated |

The `Repository` struct provides a unified API (`store_changeset`, `load_snapshot`, `save_channel`, etc.) that works identically across all backends.

---

## Resource ID ↔ Filesystem Mapping

Resource IDs use dot-separated segments that map to filesystem paths:

| Resource ID | Filesystem Path |
|-------------|----------------|
| `data.users.config` | `data/users/config.json` |
| `schemas.v2.order` | `schemas/v2/order.json` |
| `acme.entity.User` | `acme/entity/User.json` |

---

## Server REST API

| Method | Endpoint | Description |
|--------|----------|-------------|
| `POST` | `/api/v1/clone` | Clone repository metadata (channels, changesets, snapshots) |
| `POST` | `/api/v1/push` | Push new changesets to the server |
| `POST` | `/api/v1/pull` | Pull new changesets since a given HEAD |
| `POST` | `/api/v1/promote` | Promote a channel's changesets to main |
| `GET` | `/api/v1/channels` | List all channels |
| `GET` | `/api/v1/channels/:name` | Get a specific channel |
| `GET` | `/api/v1/changesets/:id` | Get a specific changeset |
| `GET` | `/api/v1/history/:resource_id` | Get change history for a resource |
| `GET` | `/api/v1/ws` | WebSocket endpoint for live notifications |
| `GET` | `/health` | Health check |

---

## License

See individual crate directories for license information.

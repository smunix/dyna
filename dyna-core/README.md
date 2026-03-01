# dyna-core

**Shared library for the Dyna distributed CRUD system.**

`dyna-core` contains the core data models, diff/patch engine, protocol types, compression utilities, and notification types shared across all Dyna crates (`dyna-cli`, `dyna-server`, `dyna-wasm`).

## Modules

| Module | Description |
|--------|-------------|
| `models` | Core data types: `Resource`, `Patch`, `PatchOperation`, `Changeset`, `Channel`, `StagedChange`, `Conflict`, `SyncState`, `RepoConfig` |
| `diff` | JSON diff engine: computes RFC 6902 operations between two JSON values |
| `patch` | Patch construction, application, and commutativity checking |
| `hash` | SHA-256 hashing for content-addressable storage |
| `protocol` | HTTP API request/response types shared between client and server |
| `compression` | Gzip compression/decompression with transparent read (backwards-compatible) |
| `notification` | WebSocket notification event types for real-time updates |

## Key Types

### Models

```rust
// A JSON resource with a unique dotted ID
struct Resource { id: String, content: Value }

// A group of patches committed together
struct Changeset {
    change_id: String,
    commit_hash: String,
    parent_change_ids: Vec<String>,
    patches: Vec<Patch>,
    message: String,
    author: String,
    timestamp: String,
    is_promoted: bool,
}

// A named bookmark into the changeset DAG
struct Channel {
    name: String,
    changeset_ids: Vec<String>,
    head_change_id: Option<String>,
}
```

### Compression

```rust
use dyna_core::compression;

// Compress/decompress bytes
let compressed = compression::compress(b"hello world")?;
let decompressed = compression::decompress(&compressed)?;

// Compress/decompress JSON
let compressed = compression::compress_json(&my_struct)?;
let decoded: MyStruct = compression::decompress_json(&compressed)?;

// Transparent read (auto-detects gzip by magic bytes)
let data = compression::read_transparent(&maybe_compressed_bytes)?;
let text = compression::read_transparent_str(&maybe_compressed_bytes)?;
```

### Notifications

```rust
use dyna_core::notification::DynaNotification;

let event = DynaNotification::promotion(
    "feature-x",
    "main",
    vec!["a7f3bc12".into()],
    vec!["acme.entity.User".into()],
    Some("8c2f4b1e".into()),
);
let json = serde_json::to_string(&event)?;
```

## Building

```bash
cargo build -p dyna-core
cargo test -p dyna-core
```

## Dependencies

- `serde` / `serde_json` — JSON serialization
- `itertools` — functional iterator utilities
- `flate2` — gzip compression
- `chrono` — timestamp generation for notifications

# Dyna

**A distributed CRUD system for collaborative JSON resource editing, inspired by [Pijul](https://pijul.org/).**

Dyna enables multiple users to concurrently edit a shared set of JSON resources stored in S3, with full version history, conflict resolution, and a patch-based changeset model. It consists of a local CLI tool and a remote server built on the [elfo-rs](https://github.com/elfo-rs/elfo) actor framework.

---

## Architecture

```
┌─────────────────────┐         ┌──────────────────────────────────────┐
│   dyna CLI (local)  │  HTTP   │         dyna-server (remote)         │
│                     │◄───────►│                                      │
│  .dyna/             │  REST   │  ┌──────────────┐                    │
│  ├── patches/       │  API    │  │ API Gateway   │ (axum + elfo)     │
│  ├── channels/      │         │  │ Actor         │                    │
│  ├── staging/       │         │  └──────┬───────┘                    │
│  ├── snapshots/     │         │         │ elfo messages              │
│  ├── conflicts/     │         │  ┌──────▼───────┐                    │
│  ├── HEAD           │         │  │ Changeset     │                    │
│  ├── config.toml    │         │  │ Manager Actor │ (business logic)  │
│  └── sync_state.json│         │  └──────┬───────┘                    │
│                     │         │         │ elfo messages              │
│                     │         │  ┌──────▼───────┐                    │
│                     │         │  │ S3 Storage    │ (object_store)    │
│                     │         │  │ Actor         │                    │
│                     │         │  └──────┬───────┘                    │
│                     │         │         │                            │
│                     │         │  ┌──────▼───────┐                    │
│                     │         │  │  S3 Bucket    │                    │
│                     │         │  └──────────────┘                    │
└─────────────────────┘         └──────────────────────────────────────┘
```

## Key Concepts

| Concept | Description |
|---------|-------------|
| **Resource** | A JSON document identified by a unique ID, stored in S3. |
| **Patch** | An immutable, content-addressed changeset (set of JSON Patch operations) targeting a single resource. |
| **Channel** | A named, ordered sequence of patches (analogous to a branch in Git or a channel in Pijul). |
| **Staging Area** | Local area where changes are prepared before committing. |
| **Promote** | The act of merging patches from a feature channel into `main`. |
| **Snapshot** | A materialized view of a resource at a point in time. |

## Project Structure

```
dyna/
├── Cargo.toml              # Workspace manifest
├── dyna-common/            # Shared library crate
│   └── src/
│       ├── lib.rs          # Module declarations
│       ├── models.rs       # Core data types (Patch, Channel, StagedChange, etc.)
│       ├── error.rs        # Shared error types
│       ├── hash.rs         # Content-addressable hashing (SHA-256)
│       ├── diff.rs         # JSON diff engine & three-way merge
│       ├── patch.rs        # Patch building, serialization, commutativity
│       ├── channel.rs      # Channel management & promotion logic
│       └── protocol.rs     # HTTP API request/response types
├── dyna-cli/               # CLI client binary crate
│   └── src/
│       ├── main.rs         # Entry point (clap)
│       ├── cli.rs          # CLI argument definitions
│       ├── repository.rs   # Local .dyna/ repository management
│       ├── sync_client.rs  # HTTP client for server communication
│       └── commands/       # One file per CLI command
│           ├── init.rs
│           ├── clone.rs
│           ├── add.rs
│           ├── commit.rs
│           ├── push.rs
│           ├── pull.rs
│           ├── status.rs
│           ├── log.rs
│           ├── resolve.rs
│           ├── channel.rs
│           └── promote.rs
└── dyna-server/            # Remote server binary crate
    └── src/
        ├── main.rs         # Entry point (elfo topology setup)
        ├── messages.rs     # Elfo message definitions
        └── actors/
            ├── storage.rs          # S3 Storage Actor (object_store)
            ├── changeset_manager.rs # Changeset Manager Actor (business logic)
            └── api_gateway.rs      # API Gateway Actor (axum HTTP bridge)
```

## CLI Commands

| Command | Description |
|---------|-------------|
| `dyna init` | Initialize a new repository in the current directory. |
| `dyna clone <url>` | Clone a repository from a remote server. |
| `dyna add <file.json>` | Stage a JSON resource for the next commit. |
| `dyna commit -m "msg"` | Record staged changes as a new patch. |
| `dyna push` | Push local patches to the remote server. |
| `dyna pull` | Fetch and merge remote patches. |
| `dyna status` | Show working directory and staging area status. |
| `dyna log [-n 20]` | Display patch history for the current channel. |
| `dyna resolve <file>` | Interactively resolve conflicts for a resource. |
| `dyna channel <name> [--create]` | Switch to or create a channel. |
| `dyna promote` | Promote current channel's patches to `main`. |

## Server Architecture (elfo-rs Actors)

The server is built on the **elfo-rs** actor framework, providing fault isolation, clean message-passing, and structured concurrency.

### Actor Hierarchy

1. **API Gateway Actor** — Bridges HTTP (axum) with the elfo actor system. Receives REST requests, translates them into typed elfo messages, and forwards them to the Changeset Manager.

2. **Changeset Manager Actor** — Core business logic. Validates patches, manages channels, handles push/pull/clone/promote operations, and orchestrates conflict detection.

3. **S3 Storage Actor** — Encapsulates all S3 I/O via the `object_store` crate. Handles storing/retrieving patches, channels, and resource snapshots.

### Message Flow

```
HTTP Request → API Gateway → HandlePush/Pull/Clone/Promote → Changeset Manager
                                                                    │
                                                    StorePatch/LoadPatch/SaveChannel
                                                                    │
                                                                    ▼
                                                             S3 Storage Actor
                                                                    │
                                                                    ▼
                                                              S3 Bucket
```

## Getting Started

### Prerequisites

- Rust 1.75+ (install via [rustup](https://rustup.rs/))
- For production: AWS credentials configured for S3 access

### Build

```bash
cargo build --release
```

### Run the Server (Development Mode)

```bash
# Uses in-memory storage by default
cargo run --bin dyna-server

# With S3 storage
DYNA_USE_S3=true \
AWS_ACCESS_KEY_ID=your-key \
AWS_SECRET_ACCESS_KEY=your-secret \
AWS_REGION=us-east-1 \
DYNA_S3_BUCKET=my-dyna-bucket \
cargo run --bin dyna-server
```

### Use the CLI

```bash
# Initialize a new repository
mkdir my-project && cd my-project
dyna init

# Configure remote
# Edit .dyna/config.toml and set remote_url = "http://localhost:8080"

# Create and edit a resource
echo '{"name": "Alice", "role": "admin"}' > user-001.json
dyna add user-001.json
dyna commit -m "Add user Alice"

# Push to remote
dyna push

# Create a feature channel
dyna channel feature-x --create

# Make changes on the feature channel
echo '{"name": "Alice", "role": "superadmin"}' > user-001.json
dyna add user-001.json
dyna commit -m "Promote Alice to superadmin"

# Promote to main
dyna promote
```

## S3 Object Layout

```
s3://dyna-bucket/
├── patches/
│   ├── <sha256-hex>.json       # Immutable patch objects
│   └── ...
├── channels/
│   ├── main.json               # Channel metadata (ordered patch list)
│   └── feature-x.json
└── snapshots/
    ├── user-001.json           # Latest materialized resource state
    └── ...
```

## Conflict Resolution

Dyna uses a **three-way merge** strategy inspired by Pijul's patch theory:

1. **Automatic merge**: When two patches modify non-overlapping paths in the same resource, they are merged automatically.
2. **Conflict detection**: When patches modify the same JSON path, a conflict is recorded.
3. **Interactive resolution**: The `dyna resolve` command presents each conflict with local, remote, and base values, allowing the user to choose or provide a custom resolution.

## Technology Stack

| Component | Technology |
|-----------|-----------|
| Shared types | `serde`, `serde_json`, `sha2`, `chrono`, `uuid` |
| CLI framework | `clap` (derive API), `colored`, `dialoguer` |
| HTTP client | `reqwest` |
| Actor framework | `elfo` (0.2.0-alpha.20) |
| HTTP server | `axum` |
| Object storage | `object_store` (supports S3, GCS, Azure, local FS, in-memory) |
| Async runtime | `tokio` |

## License

MIT

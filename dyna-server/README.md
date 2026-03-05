# dyna-server

**Distributed CRUD server for collaborative JSON editing — the central coordination point for the Dyna ecosystem.**

---

## Problem Statement

Collaborative editing of structured JSON data across distributed teams requires a central coordination server that can:

- Accept changesets from multiple clients (CLI, browser, Go, Python) and persist them durably
- Maintain channel state (branch-like bookmarks) and resolve ordering
- Broadcast real-time notifications so all connected clients see updates immediately
- Scale horizontally with pluggable storage backends (local memory or S3-compatible object stores)

Without a dedicated server, clients would need complex peer-to-peer conflict resolution. `dyna-server` provides the authoritative coordination layer that makes Dyna's changeset-centric model practical for multi-user workflows.

---

## Intent and Goals

| Goal | Description |
|------|-------------|
| **Changeset authority** | Single source of truth for changeset ordering, channel heads, and resource snapshots |
| **Real-time collaboration** | WebSocket notification hub broadcasts push/promotion events to all connected clients instantly |
| **Pluggable storage** | Swap between in-memory storage (development) and S3-compatible object stores (production) via environment variables |
| **Actor isolation** | Business logic, HTTP handling, and storage I/O run in separate actor groups for fault isolation and testability |
| **Allocator flexibility** | Three binary targets with different allocators (system, jemalloc, mimalloc) for workload-specific tuning |
| **Compression everywhere** | Gzip compression on both transport (HTTP) and storage (S3 objects) layers |

---

## Architecture

### System Context

```
┌─────────────┐   ┌─────────────┐   ┌─────────────┐   ┌─────────────┐
│  dyna-cli   │   │  dyna-app   │   │  dyna-go    │   │  dyna-py    │
│  (Rust CLI) │   │ (Elm + WASM)│   │ (Go client) │   │  (Python)   │
└──────┬──────┘   └──────┬──────┘   └──────┬──────┘   └──────┬──────┘
       │                 │                 │                 │
       │    HTTP POST    │   HTTP POST     │   HTTP POST     │
       │    + gzip       │   + gzip        │   + gzip        │
       ▼                 ▼                 ▼                 ▼
┌──────────────────────────────────────────────────────────────────────┐
│                        dyna-server                                   │
│                                                                      │
│  ┌──────────┐    ┌──────────────┐    ┌────────────┐                 │
│  │ API Actor│───▶│Changeset     │───▶│ Storage    │                 │
│  │ (axum)   │    │Actor (logic) │    │ Actor (S3) │                 │
│  └────┬─────┘    └──────────────┘    └─────┬──────┘                 │
│       │                                     │                        │
│       │  WebSocket                          │  object_store          │
│       ▼                                     ▼                        │
│  ┌──────────┐                        ┌────────────┐                 │
│  │Notif Hub │                        │ S3 / Local │                 │
│  │(broadcast)│                       │  Storage   │                 │
│  └──────────┘                        └────────────┘                 │
└──────────────────────────────────────────────────────────────────────┘
       │
       │  WebSocket frames (JSON)
       ▼
┌─────────────┐   ┌─────────────┐   ┌─────────────┐
│  lazy-cat   │   │  lazy-go    │   │  lazy-py    │
│  (Rust)     │   │  (Go)       │   │  (Python)   │
└─────────────┘   └─────────────┘   └─────────────┘
```

### Internal Actor Topology

```
                    ┌─────────────────────┐
                    │  system.configurers  │  (elfo fixture config)
                    └─────────────────────┘
                    ┌─────────────────────┐
                    │   system.loggers     │  (elfo tracing logger)
                    └─────────────────────┘

  ┌─────────────────────────────────────────────────────────────────┐
  │                      Application Actors                         │
  │                                                                 │
  │  ┌─────────┐  route_all_to  ┌────────────┐  route_all_to  ┌────────┐
  │  │   api   │ ──────────────▶│ changeset  │ ──────────────▶│storage │
  │  │         │                │            │                │        │
  │  │ • axum  │                │ • push     │                │ • S3   │
  │  │ • routes│                │ • pull     │                │ • get  │
  │  │ • ws    │                │ • clone    │                │ • put  │
  │  │ • cors  │                │ • promote  │                │ • list │
  │  └─────────┘                │ • channels │                └────────┘
  │       │                     └────────────┘
  │       │
  │  ┌────────────┐
  │  │ Notif Hub  │  (Arc<NotificationHub>)
  │  │ broadcast  │  shared between api & changeset
  │  └────────────┘
  └─────────────────────────────────────────────────────────────────┘
```

### Request Flow (Push Example)

```
Client                  API Actor           Changeset Actor      Storage Actor
  │                        │                      │                    │
  │  POST /api/v1/push     │                      │                    │
  │  Content-Encoding:gzip │                      │                    │
  │───────────────────────▶│                      │                    │
  │                        │  HandlePush(req)      │                    │
  │                        │─────────────────────▶│                    │
  │                        │                      │  StoreChangeset    │
  │                        │                      │───────────────────▶│
  │                        │                      │                    │ PUT s3://changesets/
  │                        │                      │  StoreChannel      │
  │                        │                      │───────────────────▶│
  │                        │                      │                    │ PUT s3://channels/
  │                        │                      │  SaveSnapshot      │
  │                        │                      │───────────────────▶│
  │                        │                      │                    │ PUT s3://snapshots/
  │                        │  PushResponse         │                    │
  │                        │◀─────────────────────│                    │
  │  200 OK (gzip)         │                      │                    │
  │◀───────────────────────│                      │                    │
  │                        │                      │                    │
  │                        │  broadcast(Notification::push)            │
  │                        │──────────▶ WebSocket clients              │
```

---

## API Reference

### REST Endpoints

| Method | Path | Request Body | Response | Description |
|--------|------|-------------|----------|-------------|
| `GET` | `/api/v1/health` | — | `HealthResponse` | Server health check (status, version, uptime) |
| `POST` | `/api/v1/push` | `PushRequest` | `PushResponse` | Push changesets to a channel |
| `POST` | `/api/v1/pull` | `PullRequest` | `PullResponse` | Pull new changesets since a known head |
| `POST` | `/api/v1/clone` | `CloneRequest` | `CloneResponse` | Full repository clone (all channels, changesets, snapshots) |
| `POST` | `/api/v1/promote` | `PromoteRequest` | `PromoteResponse` | Promote changesets from source to target channel |
| `GET` | `/api/v1/channels` | — | `ListChannelsResponse` | List all channels |
| `POST` | `/api/v1/channels` | `CreateChannelRequest` | `CreateChannelResponse` | Create a new channel (optionally forking from existing) |
| `POST` | `/api/v1/channels/delete` | `DeleteChannelRequest` | `DeleteChannelResponse` | Delete a channel (force flag for unmerged) |
| `GET` | `/api/v1/changesets/{change_id}` | — | `GetChangesetResponse` | Get details of a specific changeset |
| `GET` | `/api/v1/resources/{resource_id}/history` | — | `ResourceHistoryResponse` | Get change history for a resource |
| `GET` | `/api/v1/ws` | — | WebSocket upgrade | Real-time notification stream |

### Protocol Types

#### PushRequest / PushResponse

```json
// PushRequest
{
  "channel": "feature-x",
  "changesets": [
    {
      "change_id": "a1b2c3d4e5f67890",
      "message": "Add user Alice",
      "author": "alice",
      "parents": [],
      "patches": [
        {
          "hash": "sha256:...",
          "target_resource": "acme.user.Alice",
          "operations": [
            { "op": "add", "path": "", "value": { "name": "Alice", "age": 30 } }
          ],
          "parent_snapshot": null,
          "result_snapshot": { "name": "Alice", "age": 30 }
        }
      ],
      "commit_hash": "sha256:...",
      "created_at": "2026-03-04T00:00:00Z",
      "updated_at": "2026-03-04T00:00:00Z",
      "empty": false
    }
  ]
}

// PushResponse
{
  "success": true,
  "change_ids": ["a1b2c3d4e5f67890"],
  "error": null
}
```

#### PullRequest / PullResponse

```json
// PullRequest
{
  "channel": "main",
  "since_change_id": "a1b2c3d4e5f67890"
}

// PullResponse
{
  "channel": { "name": "main", "head_change_id": "...", "changesets": [...] },
  "changesets": [...],
  "new_changesets_count": 2
}
```

#### CloneResponse

```json
{
  "channels": [...],
  "changesets": [...],
  "snapshots": {
    "acme.user.Alice": { "name": "Alice", "age": 30 },
    "acme.user.Bob": { "name": "Bob", "age": 25 }
  }
}
```

### WebSocket Notifications

Clients connect to `GET /api/v1/ws` and receive JSON text frames:

#### Push Notification

```json
{
  "kind": "push",
  "timestamp": "2026-03-04T05:38:57.609259+00:00",
  "payload": {
    "Push": {
      "channel": "ps-01",
      "changeset_count": 1,
      "new_head": "18998cca3b59e730",
      "changesets": [
        {
          "change_id": "18998cca3b59e730",
          "message": "add Tom",
          "author": "psalumu",
          "patch_count": 1,
          "affected_resources": ["acme.user.Tom"]
        }
      ]
    }
  }
}
```

#### Promotion Notification

```json
{
  "kind": "promotion",
  "timestamp": "2026-03-04T06:00:00.000000+00:00",
  "payload": {
    "Promotion": {
      "source_channel": "feature-x",
      "target_channel": "main",
      "promoted_changesets": [...],
      "new_head": "...",
      "total_resources_affected": 3
    }
  }
}
```

---

## Technical Design

### Actor Model (elfo-rs)

The server uses the **elfo** actor framework for structured concurrency:

| Actor Group | Responsibility | Messages Handled |
|-------------|---------------|------------------|
| `api` | HTTP request/response bridge, WebSocket management | Incoming HTTP → elfo messages |
| `changeset` | Business logic: validate changesets, update channels, compute snapshots | `HandlePush`, `HandlePull`, `HandleClone`, `HandlePromote`, `HandleListChannels`, `HandleCreateChannel`, `HandleDeleteChannel`, `HandleGetChangeset`, `HandleResourceHistory` |
| `storage` | Persistent I/O via `object_store` crate | `StoreChangeset`, `LoadChangeset`, `StoreChannel`, `LoadChannel`, `ListChannels`, `SaveSnapshot`, `LoadSnapshot`, `LoadAllSnapshots`, `LoadAllChangesets` |

Messages flow unidirectionally: `api → changeset → storage`. Responses flow back via elfo's request-reply pattern.

### Storage Backend

The `object_store` crate provides a unified interface for:

| Backend | Configuration | Use Case |
|---------|--------------|----------|
| `InMemory` | Default (no env vars) | Development, testing |
| `AmazonS3` | `DYNA_USE_S3=true` + `AWS_*` env vars | Production |
| Any S3-compatible | `AWS_ENDPOINT` override | MinIO, R2, etc. |

#### S3 Object Layout

```
s3://dyna-bucket/
├── changesets/
│   └── <commit_hash>.json.gz     # Immutable changeset objects
├── channels/
│   └── <channel_name>.json.gz    # Channel metadata (ordered changeset IDs)
└── snapshots/
    └── <channel>/<resource_id>.json.gz  # Materialized resource state
```

### Compression

| Layer | Direction | Mechanism |
|-------|-----------|----------|
| **Transport (request)** | Client → Server | `Content-Encoding: gzip` header; axum middleware decompresses |
| **Transport (response)** | Server → Client | `tower-http` `CompressionLayer`; clients send `Accept-Encoding: gzip` |
| **Storage (write)** | Server → S3 | `flate2` gzip before `PUT`; objects stored as `*.json.gz` |
| **Storage (read)** | S3 → Server | Transparent decompression (gzip magic byte detection) |

### Binary Targets

| Binary | Allocator | Use Case |
|--------|-----------|----------|
| `dyna-server` | System default | General purpose, debugging |
| `dyna-server-je` | jemalloc (`tikv-jemallocator`) | High-throughput production (reduced fragmentation) |
| `dyna-server-mim` | mimalloc | Low-latency production (faster small allocations) |

### WebSocket Hub

The `NotificationHub` uses `tokio::sync::broadcast` for fan-out:

```
                    broadcast::Sender
                         │
              ┌──────────┼──────────┐
              ▼          ▼          ▼
         Receiver_1  Receiver_2  Receiver_N
              │          │          │
         ws_client_1 ws_client_2 ws_client_N
```

- Each WebSocket connection spawns a task that subscribes to the broadcast channel
- Ping/pong keepalive prevents stale connections
- Lagged receivers (slow clients) are automatically dropped

### Environment Variables

| Variable | Default | Description |
|----------|---------|-------------|
| `DYNA_BIND_ADDR` | `0.0.0.0:8080` | HTTP listen address |
| `DYNA_USE_S3` | `false` | Enable S3 storage backend |
| `AWS_ACCESS_KEY_ID` | — | S3 credentials |
| `AWS_SECRET_ACCESS_KEY` | — | S3 credentials |
| `AWS_DEFAULT_REGION` | — | S3 region |
| `AWS_ENDPOINT` | — | Custom S3 endpoint (MinIO, R2) |
| `S3_BUCKET` | `dyna` | S3 bucket name |

---

## Integration Examples

### Starting the Server

```bash
# In-memory storage (development)
dyna-server

# With S3 storage
DYNA_USE_S3=true \
  AWS_ACCESS_KEY_ID=minioadmin \
  AWS_SECRET_ACCESS_KEY=minioadmin \
  AWS_ENDPOINT=http://localhost:9000 \
  S3_BUCKET=dyna \
  dyna-server-je
```

### Using with dyna-cli

```bash
# Initialize a local repo pointing to the server
mkdir my-project && cd my-project
dyna init
# Edit .dyna/config.toml to set remote_url = "http://localhost:8080"

# Create a resource, commit, and push
echo '{"name": "Alice"}' > users/alice.json
dyna add users/alice.json
dyna commit -m "Add Alice"
dyna push

# Clone from another machine
dyna clone http://localhost:8080
```

### Using with curl

```bash
# Health check
curl http://localhost:8080/api/v1/health

# List channels
curl http://localhost:8080/api/v1/channels

# Clone (get all data)
curl -X POST http://localhost:8080/api/v1/clone \
  -H "Content-Type: application/json" \
  -d '{"channel": "main"}'

# Subscribe to WebSocket notifications
websocat ws://localhost:8080/api/v1/ws
```

### Docker Deployment

```bash
# Build the OCI image with Nix
nix build .#dyna-server-image

# Load and run
docker load < result
docker run -p 8080:8080 \
  -e DYNA_USE_S3=true \
  -e AWS_ACCESS_KEY_ID=minioadmin \
  -e AWS_SECRET_ACCESS_KEY=minioadmin \
  -e AWS_ENDPOINT=http://minio:9000 \
  dyna-server:latest
```

---

## Building

### With Nix (recommended)

```bash
# Build the server binary
nix build .#dyna-server

# Build the OCI/Docker image
nix build .#dyna-server-image

# Run directly
nix run .#dyna-server

# Enter dev shell with all tools
nix develop
```

### With Cargo

```bash
# Build all three binaries
cargo build --release -p dyna-server

# Run with system allocator
cargo run --release --bin dyna-server

# Run with jemalloc
cargo run --release --bin dyna-server-je

# Run with mimalloc
cargo run --release --bin dyna-server-mim
```

---

## Testing

```bash
# Run all workspace tests
cargo nextest run

# Run integration tests with a live server
cargo run --release --bin dyna-server &
cargo nextest run -p dyna-server

# Check with clippy
cargo clippy -p dyna-server -- -D warnings
```

---

## Related Projects

| Project | Description |
|---------|-------------|
| [dyna-core](../dyna-core/) | Shared Rust library: models, protocol, diff, compression |
| [dyna-cli](../dyna-cli/) | Command-line client (init, clone, push, pull, promote, etc.) |
| [dyna-wasm](../dyna-wasm/) | Browser WASM client (used by dyna-app) |
| [dyna-app](../dyna-app/) | Elm web UI for collaborative editing |
| [dyna-py](../dyna-py/) | Python bindings (PyO3/maturin) |
| [dyna-go](../dyna-go/) | Go client library |
| [lazy-cat](../lazy-cat/) | Rust lazy resource loader |
| [lazy-go](../lazy-go/) | Go lazy resource loader |
| [lazy-py](../lazy-py/) | Python lazy resource loader |
| [lazy-wasm](../lazy-wasm/) | Browser WASM lazy resource loader |
| [lazy-elm-demo](../lazy-elm-demo/) | Elm UI demo for lazy-wasm |

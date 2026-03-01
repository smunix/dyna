# dyna-server

**Remote server for the Dyna distributed CRUD system.**

`dyna-server` is built on the [elfo-rs](https://github.com/elfo-rs/elfo) actor framework with an [Axum](https://github.com/tokio-rs/axum) HTTP layer. It manages changeset storage in S3, handles push/pull/promote operations, broadcasts real-time WebSocket notifications, and serves resource history queries.

## Features

- **Actor-based architecture**: Three actors (API, Changeset, Storage) communicate via typed elfo messages
- **S3 storage**: All data persisted to S3-compatible object storage via `object_store`
- **Compressed storage**: All S3 objects stored as `.json.gz` with transparent fallback to uncompressed `.json`
- **Compressed transport**: Axum middleware decompresses gzip request bodies; `tower-http` CompressionLayer compresses responses
- **256 MiB body limit**: Large JSON resources are supported without hitting default Axum limits
- **WebSocket notifications**: Real-time broadcast hub for promotion and push events
- **Resource history**: Query the full change history of any resource across all channels
- **Channel management**: Create, list, and manage channels with optimistic concurrency

## Architecture

```
                    HTTP / WebSocket
                         │
                  ┌──────▼───────┐
                  │  API Actor    │  Axum router + WS hub
                  │               │  Routes: /api/v1/*
                  │               │  WebSocket: /ws
                  └──────┬───────┘
                         │ elfo messages
                  ┌──────▼───────┐
                  │  Changeset   │  Business logic:
                  │  Actor       │  push, pull, promote,
                  │               │  clone, history
                  └──────┬───────┘
                         │ elfo messages
                  ┌──────▼───────┐
                  │  Storage     │  S3 I/O via object_store
                  │  Actor       │  Compressed read/write
                  └──────┬───────┘
                         │
                  ┌──────▼───────┐
                  │  S3 Bucket   │
                  └──────────────┘
```

## API Endpoints

| Method | Path | Description |
|--------|------|-------------|
| `POST` | `/api/v1/push` | Push changesets to a channel |
| `POST` | `/api/v1/pull` | Pull changesets since a given change_id |
| `POST` | `/api/v1/clone` | Clone all channels and changesets |
| `POST` | `/api/v1/promote` | Promote changesets between channels |
| `GET` | `/api/v1/channels` | List all channels |
| `POST` | `/api/v1/channels` | Create a new channel |
| `GET` | `/api/v1/resources/:resource_id/history` | Query change history for a resource |
| `GET` | `/api/v1/health` | Health check |
| `GET` | `/ws` | WebSocket endpoint for real-time notifications |

## WebSocket Notifications

Connect to `/ws` to receive real-time JSON notifications:

### Promotion Event

```json
{
  "event_type": "promotion",
  "timestamp": "2026-03-01T08:00:00Z",
  "source_channel": "feature-x",
  "target_channel": "main",
  "promoted_changesets": ["a7f3bc12", "5e9d1a04"],
  "affected_resources": ["acme.entity.User", "acme.entity.Order"],
  "new_head": "8c2f4b1e",
  "message": "Promoted 2 changeset(s) from feature-x to main"
}
```

### Push Event

```json
{
  "event_type": "push",
  "timestamp": "2026-03-01T08:05:00Z",
  "channel": "feature-x",
  "pushed_changesets": ["c3d4e5f6"],
  "affected_resources": ["acme.entity.Config"],
  "message": "Pushed 1 changeset(s) to feature-x"
}
```

## S3 Object Layout

```
s3://dyna-bucket/
├── changesets/
│   ├── <commit_hash>.json.gz    # Immutable changeset objects
│   └── ...
├── channels/
│   ├── main.json.gz             # Channel metadata
│   └── feature-x.json.gz
└── snapshots/
    ├── <resource_id>.json.gz    # Latest materialized resource state
    └── ...
```

## Configuration

Environment variables:

| Variable | Description | Default |
|----------|-------------|---------|
| `S3_BUCKET` | S3 bucket name | `dyna-bucket` |
| `S3_ENDPOINT` | S3 endpoint URL | AWS default |
| `S3_REGION` | S3 region | `us-east-1` |
| `LISTEN_ADDR` | Server listen address | `0.0.0.0:8080` |

## Building

```bash
cargo build -p dyna-server --release
cargo run -p dyna-server
```

## Dependencies

- `dyna-core` — shared models, diff/patch engine, compression, protocol types, notifications
- `elfo` — actor framework for message-passing architecture
- `axum` — HTTP framework with WebSocket support
- `tower-http` — CORS and compression middleware
- `object_store` — S3-compatible storage backend
- `flate2` — gzip compression
- `tokio` — async runtime

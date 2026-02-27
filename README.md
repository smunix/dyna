# Dyna

**A distributed CRUD system for collaborative JSON resource editing, inspired by [Jujutsu](https://github.com/martinvonz/jj).**

Dyna enables multiple users to concurrently edit a shared set of JSON resources stored in S3, with full version history, conflict resolution, and a powerful changeset-based model. It consists of a local CLI tool and a remote server built on the [elfo-rs](https://github.com/elfo-rs/elfo) actor framework.

---

## Architecture

```
┌─────────────────────┐         ┌──────────────────────────────────────┐
│   dyna CLI (local)  │  HTTP   │         dyna-server (remote)         │
│                     │◄───────►│                                      │
│  .dyna/             │  REST   │  ┌──────────────┐                    │
│  ├── changesets/    │  API    │  │ API Gateway   │ (axum + elfo)     │
│  ├── objects/       │         │  │ Actor         │                    │
│  ├── snapshots/     │         │  └──────┬───────┘                    │
│  ├── HEAD           │         │         │ elfo messages              │
│  └── ...            │         │  ┌──────▼───────┐                    │
│                     │         │  │ Changeset     │                    │
│                     │         │  │ Manager Actor │ (business logic)  │
│                     │         │  └──────┬───────┘                    │
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
| **Patch** | An immutable set of JSON Patch (RFC 6902) operations targeting a single resource. |
| **Changeset** | A group of patches committed together, with a stable ID and parent tracking. The primary unit of work. |
| **Channel** | A named bookmark pointing to a specific changeset, similar to a Git branch. |
| **Promote** | The act of merging changesets from a feature channel into `main`. |

## CLI Commands

| Command | Description |
|---------|-------------|
| `dyna init` | Initialize a new repository. |
| `dyna clone <url>` | Clone a repository from a remote server. |
| `dyna add <file.json>` | Stage a JSON resource for the next commit. |
| `dyna commit -m "msg"` | Record staged changes as a new changeset. |
| `dyna describe -m "msg"` | Amend the message of the current working changeset. |
| `dyna push` | Push local changesets to the remote server. |
| `dyna pull` | Fetch and merge remote changesets. |
| `dyna status` | Show working directory and changeset status. |
| `dyna log` | Display changeset history. Use `--changeset <id>` for details. |
| `dyna diff` | Show diffs for staged files. |
| `dyna resolve <file>` | Interactively resolve conflicts. |
| `dyna channel <name>` | Switch to or create a channel (bookmark). |
| `dyna promote` | Promote current channel's changesets to `main`. |

## Getting Started

### Build

```bash
cargo build --release
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

# Amend the commit message
dyna describe -m "Add user Alice with admin role"

# Push to remote
dyna push

# View the log
dyna log
# Output:
# @ 2a8f3b4c (e5d4c3b2) Add user Alice with admin role
```

## S3 Object Layout

```
s3://dyna-bucket/
├── changesets/
│   ├── <commit_hash>.json      # Immutable changeset objects
│   └── ...
├── patches/
│   ├── <patch_hash>.json       # Immutable patch objects
│   └── ...
├── channels/
│   ├── main.json               # Channel metadata (ordered list of change_ids)
│   └── feature-x.json
└── snapshots/
    ├── <resource_id>.json      # Latest materialized resource state
    └── ...
```

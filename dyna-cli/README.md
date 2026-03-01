# dyna-cli

**Command-line client for the Dyna distributed CRUD system.**

`dyna-cli` provides a Jujutsu-inspired command-line interface for managing JSON resources with full version history, conflict resolution, and collaborative editing via channels.

## Features

- **Changeset-centric workflow**: All operations revolve around changesets (groups of patches), not individual files
- **VFS-abstracted storage**: Uses the `vfs` crate for filesystem abstraction, enabling `PhysicalFS` for production and `MemoryFS` for testing
- **Compressed storage**: All `.dyna/` metadata files are gzip-compressed with transparent backwards-compatible reads
- **Compressed transport**: HTTP requests/responses use gzip compression
- **Jujutsu-inspired commands**: `squash`, `restore`, `describe`, `channel` — familiar to Jujutsu users
- **Glob and directory support**: `dyna add` and `dyna add --delete` accept single files, directories, or glob patterns
- **Resource history**: Query the full change history of any resource from the remote server
- **Smart status**: Shows staged changes, modified files, deleted tracked files, and unstaged modifications on staged files

## Commands

| Command | Description |
|---------|-------------|
| `dyna init` | Initialize a new repository in the current directory |
| `dyna clone <url> [-d dir]` | Clone a repository from a remote server, recreating the full filesystem hierarchy |
| `dyna add <pattern>` | Stage JSON resource file(s), directory, or glob pattern |
| `dyna add --delete <pattern>` | Stage the removal of deleted tracked file(s), directory, or glob |
| `dyna commit -m "msg"` | Record staged changes as a new changeset |
| `dyna describe [-m "msg"]` | Amend the message of the current working changeset |
| `dyna squash [-r id] [-i id] [-m "msg"]` | Squash a changeset into its parent |
| `dyna push [-c channel]` | Push local changesets to the remote server |
| `dyna pull [-c channel]` | Fetch and merge remote changesets |
| `dyna status` | Show working directory and staging area status |
| `dyna log [-n count] [-v] [-c id] [-p]` | Display changeset history |
| `dyna diff [path]` | Show diffs for staged files |
| `dyna history <resource_id> [-v]` | Query change history of a resource from the remote server |
| `dyna resolve <file>` | Interactively resolve conflicts |
| `dyna restore <file> [-C channel] [-s changeset]` | Restore a file to its snapshot state |
| `dyna channel <name> [-c] [-l] [-r] [--local]` | Manage channels (switch, create, list) |
| `dyna promote [--channel name]` | Promote changesets from a channel to `main` |

## Status Output

`dyna status` shows five categories:

```
On channel feature-x

Staged changes:
  modified data/users/config.json (3 ops)
  new      data/orders/schema.json (1 ops)
  deleted  data/old/legacy.json (1 ops)

Staged files with unstaged modifications:
  modified data/users/config.json

Modified but not staged:
  modified data/settings/app.json

Deleted tracked files:
  deleted  data/temp/cache.json
```

## Resource ID ↔ Filesystem Mapping

| Resource ID | Filesystem Path |
|-------------|----------------|
| `data.users.config` | `data/users/config.json` |
| `schemas.v2.order` | `schemas/v2/order.json` |
| `acme.entity.User` | `acme/entity/User.json` |

## Building

```bash
cargo build -p dyna-cli --release
```

## Dependencies

- `dyna-core` — shared models, diff/patch engine, compression, protocol types
- `clap` — CLI argument parsing (derive API)
- `reqwest` — HTTP client with gzip support
- `tokio` — async runtime
- `vfs` — virtual filesystem abstraction
- `glob` — glob pattern matching for file operations
- `serde` / `serde_json` — JSON serialization
- `flate2` — gzip compression (via dyna-core)

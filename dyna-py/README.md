# dyna-py

Python bindings for the **Dyna** distributed CRUD system, powered by [PyO3](https://pyo3.rs).

`dyna-py` calls directly into the Rust `dyna-cli` library — there is **zero code duplication**. Every CLI command is exposed as a method on the `DynaRepo` class.

## Installation

### From source (requires Rust toolchain + maturin)

```bash
cd dyna-py
pip install maturin
maturin develop          # debug build, installs into current venv
maturin develop --release  # optimised build
```

### With Nix

```bash
nix develop   # dyna-py is available as a Python package in the dev shell
```

### With pip (once published)

```bash
pip install dyna-py        # core library
pip install dyna-py[cli]   # includes click-based CLI
```

## Quick Start

```python
from dyna_py import DynaRepo

# Initialise a new repository
repo = DynaRepo.init("/tmp/my-repo", remote_url="http://localhost:8080")

# Or open an existing one
repo = DynaRepo("/tmp/my-repo")

# Or clone from a remote server
repo = DynaRepo.clone_repo("http://localhost:8080", "/tmp/cloned")

# Write a JSON resource
repo.write_resource("acme.entity.User", '{"name": "Alice", "role": "admin"}')

# Stage and commit
repo.add("acme.entity.User")
change_id = repo.commit("Add user Alice")

# Push to remote
result = repo.push()
print(f"Pushed {result['changesets_pushed']} changeset(s)")

# Pull changes from remote
result = repo.pull()
print(f"Pulled {result['changesets_pulled']} changeset(s)")

# Promote a feature branch to main
result = repo.promote(channel="feature/my-branch")
print(f"Promoted {result['promoted_count']} changeset(s)")
```

## DynaRepo API Reference

### Construction

| Method | Description |
|--------|-------------|
| `DynaRepo(path)` | Open an existing repository |
| `DynaRepo.init(path, remote_url=None, user_name=None)` | Create a new repository |
| `DynaRepo.clone_repo(url, path, user_name=None)` | Clone from remote |

### Configuration

| Method | Description |
|--------|-------------|
| `remote_url() -> str \| None` | Get the remote URL |
| `set_remote(url)` | Set the remote URL |
| `user_name() -> str` | Get the user name |
| `set_user_name(name)` | Set the user name |

### Resource I/O

| Method | Description |
|--------|-------------|
| `read_resource(resource_id) -> str` | Read a resource as JSON string |
| `write_resource(resource_id, json_content)` | Write a JSON resource |
| `delete_resource(resource_id)` | Delete a resource from disk |
| `list_resources() -> list[str]` | List all resource IDs |
| `resource_exists(resource_id) -> bool` | Check if a resource exists |

### Staging & Committing

| Method | Description |
|--------|-------------|
| `add(resource_id)` | Stage a resource for commit |
| `add_delete(resource_id)` | Stage a resource deletion |
| `commit(message) -> str` | Commit staged changes, returns change_id |
| `status() -> dict` | Get repository status (channel, staged, modified, deleted, untracked, conflicts) |
| `diff(resource_id) -> str` | Show diff operations as JSON |

### Sync

| Method | Description |
|--------|-------------|
| `push(channel=None) -> dict` | Push to remote (returns `success`, `changesets_pushed`, `channel`) |
| `pull(channel=None) -> dict` | Pull from remote (returns `changesets_pulled`, `resources_updated`, `channel`) |
| `promote(channel=None) -> dict` | Promote to main (returns `success`, `promoted_count`, `source_channel`) |
| `history(resource_id) -> list[dict]` | Query remote change history |

### Channels

| Method | Description |
|--------|-------------|
| `current_channel() -> str` | Get current channel name |
| `list_channels() -> list[str]` | List all channel names |
| `create_channel(name, fork_from=None)` | Create a new channel |
| `switch_channel(name)` | Switch to a channel |

### History & Advanced

| Method | Description |
|--------|-------------|
| `log(count=None, verbose=None) -> list[dict]` | Show changeset log (change_id, commit_hash, message, author, created_at, parents, patch_count, immutable; verbose adds patches) |
| `restore(resource_id, changeset=None)` | Restore a resource to its snapshot state (optionally from a specific changeset) |
| `squash(revision=None, into=None, message=None) -> str` | Squash a changeset into its parent, returns target change_id |
| `describe(change_id, message)` | Update a changeset's commit message |
| `resolve(resource_id)` | Resolve a conflict by accepting the current working file |
| `list_conflicts() -> list[str]` | List conflicted resource IDs |

### Utility

| Method | Description |
|--------|-------------|
| `work_dir() -> str` | Get the working directory path |
| `path_to_resource_id(path) -> str` | Convert a relative file path to a resource ID |
| `resource_id_to_path(resource_id) -> str` | Convert a resource ID to a relative file path |

## CLI

Install with the `cli` extra for a `click`-based command-line interface:

```bash
pip install dyna-py[cli]
```

```bash
# Repository management
dyna-py init /tmp/my-repo --remote http://localhost:8080 --user alice
dyna-py clone http://localhost:8080 /tmp/cloned --user bob
dyna-py status -C /tmp/my-repo

# Staging and committing
dyna-py add acme.entity.User
dyna-py add --delete acme.entity.OldUser
dyna-py commit -m "Add user"

# Sync
dyna-py push
dyna-py pull
dyna-py promote --channel feature/my-branch

# History
dyna-py log -n 10 -v
dyna-py history acme.entity.User
dyna-py diff acme.entity.User

# Channels
dyna-py channel --list
dyna-py channel my-feature --create
dyna-py channel my-feature          # switch

# Advanced
dyna-py squash -m "Combine changes"
dyna-py describe abc123 -m "Better message"
dyna-py restore acme.entity.User
dyna-py restore acme.entity.User --changeset abc123
dyna-py resolve acme.entity.User
```

## Examples

See the `examples/` directory:

- **`basic_usage.py`** — exercises every local operation (init, write, add, commit, log, diff, status, channels, squash, describe, restore). No server required.
- **`sync_workflow.py`** — demonstrates push, pull, clone, and promote with a running Dyna server.

```bash
# Local operations only (no server needed)
python examples/basic_usage.py

# Sync operations (start dyna-server first)
cargo run -p dyna-server &
python examples/sync_workflow.py
```

## Tests

```bash
# Build the extension module first
cd dyna-py
maturin develop

# Run tests
pytest tests/ -v
```

## Architecture

```
dyna-py (PyO3 cdylib)
  └── dyna-cli (Rust library)
        ├── repository.rs  — VFS-based local storage
        ├── sync_client.rs — HTTP client (reqwest)
        └── dyna-core      — models, diff, patch, protocol
```

The PyO3 `DynaRepo` class holds a Rust `Repository`. Sync methods (`push`, `pull`, `clone_repo`, `promote`, `history`) spin up a `tokio::Runtime` to run async Rust futures. All data crosses the Python/Rust boundary as JSON strings or Python dicts — no custom serde is needed on the Python side.

### Key design decisions

1. **Single `DynaRepo` class** — mirrors the Rust `Repository` struct; every CLI command maps to a method.
2. **Blocking bridge** — async Rust code is executed via `tokio::Runtime::block_on()` so Python callers get a synchronous API.
3. **Error mapping** — all Rust `anyhow::Error` values are converted to Python `RuntimeError` with the full error chain.
4. **Zero duplication** — the Python bindings call the same Rust functions as `dyna-cli`; no logic is reimplemented.

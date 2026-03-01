# dyna-go

A pure-Go client library for [Dyna](https://github.com/smunix/dyna) — distributed CRUD for collaborative JSON editing. Uses [afero](https://github.com/spf13/afero) for filesystem abstraction, making it suitable for both in-memory usage (tests, embedded applications) and disk-backed repositories.

## Features

| Category | Operations |
|----------|-----------|
| **Repository** | `Init`, `IsInitialized`, `CloneRepo` |
| **Config** | `RemoteURL`, `SetRemote`, `UserName`, `SetUserName` |
| **Resources** | `ReadResource`, `WriteResource`, `DeleteResource`, `ListResources`, `ResourceExists` |
| **Staging** | `Add`, `AddByResourceID`, `AddDelete` |
| **Commit** | `Commit`, `Status`, `Diff` |
| **History** | `Log`, `Describe`, `Squash`, `Revert`, `CherryPick` |
| **Channels** | `CurrentChannel`, `ListChannels`, `CreateChannel`, `SwitchChannel` |
| **Sync** | `Push`, `Pull`, `Promote`, `ListRemoteChannels`, `ResourceHistory`, `Health` |
| **Conflicts** | `ListConflicts`, `Resolve` |
| **Notifications** | `SubscribeNotifications` (WebSocket) |

## Installation

```bash
go get github.com/smunix/dyna/dyna-go
```

## Quick Start

```go
package main

import (
    "encoding/json"
    "fmt"
    "log"

    "github.com/smunix/dyna/dyna-go/dynago"
)

func main() {
    // Create an in-memory client (no disk I/O)
    client := dynago.NewMemClient()
    client.Init(nil)
    client.SetUserName("alice")

    // Main channel is protected — create a feature channel
    client.CreateChannel("feature/users", nil)
    client.SwitchChannel("feature/users")

    // Write a JSON resource
    user, _ := json.Marshal(map[string]interface{}{
        "name": "Alice", "email": "alice@example.com",
    })
    client.WriteResource("users.alice", user)

    // Stage and commit
    client.AddByResourceID("users.alice")
    changeID, _ := client.Commit("Add Alice")
    fmt.Println("Committed:", changeID)

    // View log
    entries, _ := client.Log()
    for _, e := range entries {
        fmt.Printf("  %s  %s\n", e.ChangeID[:8], e.Message)
    }
}
```

## Architecture

```
dynago/
  models.go        — Data structures (Changeset, Patch, Channel, etc.)
  protocol.go      — Request/response types matching dyna-server API
  repository.go    — Afero-backed local state (config, channels, snapshots, staging)
  diff.go          — JSON diff/patch engine (RFC 6902), invert, three-way merge
  operations.go    — High-level ops (commit, log, revert, cherry-pick, squash, etc.)
  sync_client.go   — HTTP client for dyna-server (push, pull, clone, promote)
  client.go        — Unified Client facade combining Repository + SyncClient
  dynago_test.go   — Comprehensive test suite (23 tests)

examples/
  basic/main.go    — Local operations demo (no server needed)
  sync/main.go     — Remote sync workflow demo
```

## Filesystem Layout

The repository stores state under `.dyna/`:

```
<root>/
  .dyna/
    config.json          — Remote URL, user config
    current_channel      — Active channel name (plain text)
    staged.json          — Staged changes for next commit
    sync_state.json      — Push/pull tracking
    channels/<name>.json — Channel metadata and changeset lists
    changesets/<id>.json — Immutable changeset records
    snapshots/<id>.json  — Current resource state per resource ID
    conflicts/<id>.json  — Unresolved merge conflicts
  <resource files>       — Working directory (resource_id → path mapping)
```

Resource IDs use dot notation (`acme.entity.User`) which maps to file paths (`acme/entity/User.json`).

## Using with afero

```go
// In-memory (testing, embedded)
client := dynago.NewMemClient()

// Disk-backed
fs := afero.NewOsFs()
client := dynago.NewClient(fs, "/path/to/repo")

// Read-only overlay
base := afero.NewOsFs()
overlay := afero.NewMemMapFs()
ufs := afero.NewCopyOnWriteFs(base, overlay)
client := dynago.NewClient(ufs, "/path/to/repo")
```

## Main Channel Protection

Direct commits and pushes to `main` are blocked. The workflow is:

1. Create a feature channel: `client.CreateChannel("feature/x", nil)`
2. Switch to it: `client.SwitchChannel("feature/x")`
3. Make changes, commit, push
4. Promote to main: `client.Promote(ctx, "feature/x", "main")`

Promotion is **remote-first**: the server validates there are no conflicts before accepting.

## Running Tests

```bash
cd dyna-go
go test ./dynago/ -v
```

All 23 tests pass, covering: models, diff/patch, three-way merge, repository init, resource I/O, commit/log, status, main channel protection, describe, revert, cherry-pick, channels, diff command, squash, and delete.

## Dependencies

- [afero](https://github.com/spf13/afero) — Filesystem abstraction
- [gorilla/websocket](https://github.com/gorilla/websocket) — WebSocket client for notifications
- Go standard library (`crypto/sha256`, `encoding/json`, `net/http`, `compress/gzip`)

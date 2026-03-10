# dyna-go: Go Client for the Dyna Distributed CRUD System

A native Go client library for interacting with a `dyna-server`, providing a complete, idiomatic, and robust API for collaborative JSON editing based on a changeset-centric version control model.

## 1. Problem Statement

Developing applications that allow multiple users to concurrently edit complex JSON documents is fraught with challenges. Traditional state management approaches often lead to a host of problems:

*   **Race Conditions:** Without proper locking, concurrent edits can overwrite each other, leading to data loss and an inconsistent state.
*   **Complex State Logic:** Implementing operational transformations (OT) or conflict-free replicated data types (CRDTs) from scratch is a significant engineering effort, requiring deep expertise and extensive testing.
*   **Scalability Bottlenecks:** Centralized database-driven solutions can become a performance bottleneck, especially in highly concurrent environments.
*   **Lack of History and Auditing:** Many systems do not provide a clear, auditable history of changes, making it difficult to track how a document evolved or to revert to a previous state.

The Dyna ecosystem was created to solve these problems by providing a distributed, version-controlled system for JSON data, inspired by Git. `dyna-go` is the official Go client for this ecosystem, enabling Go developers to seamlessly integrate this powerful collaborative editing functionality into their applications.

## 2. Intent and Goals

The design of `dyna-go` is guided by the following principles:

*   **Idiomatic Go:** Provide an API that feels natural to Go developers, embracing Go's conventions for error handling, concurrency, and project structure.
*   **Robustness:** Ensure the client is resilient to network failures and provides clear, actionable error messages.
*   **Performance:** Optimize for low latency and efficient use of network and memory resources, including automatic gzip compression for all payloads.
*   **Completeness:** Implement the full Dyna API, including repository operations (clone, push, pull), channel management, history inspection, and real-time updates via WebSockets.
*   **Testability:** Leverage interfaces and the `afero` filesystem abstraction to make the client easy to test in isolation.

## 3. Architecture

`dyna-go` acts as an intermediary between a Go application and a `dyna-server`. It manages a local repository on a virtual filesystem (provided by `afero`) and synchronizes it with the remote server.

### Component Diagram

```ascii
+-----------------------------------+
|         Your Go Application       |
+-----------------------------------+
              | (uses)
              v
+-----------------------------------+
|             dyna-go               |
|-----------------------------------|
| +-------------+ +---------------+ |
| |   Client    | |  Repository   | |
| |-------------| |---------------| |
| | - REST API  | | - afero FS    | |
| | - WebSocket | | - .dyna/      | |
| +-------------+ +---------------+ |
+-----------------------------------+
      | (HTTP/REST)   | (WebSocket)
      v               v
+-----------------------------------+
|           dyna-server             |
|-----------------------------------|
| - REST API (/api/v1)              |
| - WebSocket (/api/v1/ws)          |
+-----------------------------------+
              | (S3 API)
              v
+-----------------------------------+
|      S3-Compatible Storage        |
+-----------------------------------+
```

### Data Flow

1.  **Clone:** A `POST` request to `/api/v1/clone` initiates the process. The server streams a gzipped tarball of the entire repository state, which the client unpacks into the local `afero` filesystem.
2.  **Push:** The client identifies local changesets not present on the server and sends them in a `POST` request to `/api/v1/push`. The server validates the changesets and, upon success, broadcasts a `PushPayload` notification to all WebSocket clients.
3.  **Pull:** A `POST` request to `/api/v1/pull` tells the server the client's current channel head. The server responds with a list of missing changesets, which the client then fetches individually via `GET /api/v1/changesets/{id}`.
4.  **WebSocket Notifications:** The client can open a persistent WebSocket connection to `/api/v1/ws`. The server will push `Notification` messages (e.g., `PushPayload`, `PromotionPayload`) to the client in real-time, allowing the application to stay up-to-date without polling.

## 4. API Reference

### `Client`

The main entry point for interacting with the Dyna API.

| Method          | Parameters                                      | Return Type       | Description                                                                                                                                                           |
|-----------------|-------------------------------------------------|-------------------|-----------------------------------------------------------------------------------------------------------------------------------------------------------------------|
| `NewClient`     | `serverURL string`, `fs afero.Fs`               | `*Client`         | Creates a new Dyna client. If `fs` is `nil`, an in-memory filesystem is used.                                                                                         |
| `Clone`         | `ctx context.Context`, `repoPath string`        | `error`           | Clones a repository from the server to the specified `repoPath`.                                                                                                      |
| `Push`          | `ctx context.Context`, `repoPath`, `channel string` | `*PushResponse`   | Pushes local changes from the specified channel to the server.                                                                                                        |
| `Pull`          | `ctx context.Context`, `repoPath`, `channel string` | `*PullResponse`   | Pulls remote changes for the specified channel from the server.                                                                                                       |
| `Promote`       | `ctx context.Context`, `repoPath`, `from`, `to string` | `error`           | Promotes one channel to another, effectively merging its history.                                                                                                     |
| `GetChannels`   | `ctx context.Context`, `repoPath string`        | `[]*Channel`      | Retrieves a list of all channels in the repository.                                                                                                                   |
| `CreateChannel` | `ctx context.Context`, `repoPath`, `name string`| `error`           | Creates a new channel.                                                                                                                                                |
| `DeleteChannel` | `ctx context.Context`, `repoPath`, `name string`| `error`           | Deletes a channel.                                                                                                                                                    |
| `GetChangeset`  | `ctx context.Context`, `repoPath`, `id string`  | `*Changeset`      | Retrieves a specific changeset by its content-addressed ID.                                                                                                           |
| `GetHistory`    | `ctx context.Context`, `repoPath`, `resourceID string` | `[]*Changeset`    | Retrieves the history of a resource as a list of changesets.                                                                                                          |
| `Health`        | `ctx context.Context`                           | `string`          | Checks the health of the Dyna server.                                                                                                                                 |
| `Connect`       | `ctx context.Context`, `repoPath string`        | `*websocket.Conn` | Establishes a WebSocket connection for real-time updates.                                                                                                             |

### `Repository`

Represents a local Dyna repository.

| Method         | Parameters                               | Return Type      | Description                                                                                                                                 |
|----------------|------------------------------------------|------------------|---------------------------------------------------------------------------------------------------------------------------------------------|
| `Open`         | `repoPath string`, `fs afero.Fs`         | `*Repository`    | Opens an existing local repository.                                                                                                         |
| `Init`         | `repoPath string`, `fs afero.Fs`         | `*Repository`    | Initializes a new empty repository at `repoPath`.                                                                                           |
| `Add`          | `resourcePath string`, `data []byte`     | `error`          | Stages a change to a resource file.                                                                                                         |
| `Commit`       | `author`, `message string`               | `*Changeset`     | Creates a new changeset from staged changes.                                                                                                |
| `Status`       |                                          | `*RepoStatus`    | Shows the status of the repository, including modified and staged files.                                                                    |
| `ReadResource` | `resourceID string`                      | `[]byte`, `error`| Reads the current state of a resource.                                                                                                      |

## 5. Technical Design

### Data Structures

`dyna-go` mirrors the core data structures from `dyna-core` in idiomatic Go.

*   **`Resource`**: Represents a JSON document. Its ID (e.g., `com.example.user.123`) maps directly to a file path (`com/example/user/123.json`) within the `afero` filesystem.
*   **`Patch`**: A struct representing a JSON Patch operation (RFC 6902), like `{ "op": "add", "path": "/name", "value": "John Doe" }`.
*   **`Changeset`**: A collection of `Patch` objects, along with metadata like author, message, and timestamp. Its ID is the SHA-256 hash of its canonicalized content.
*   **`Channel`**: A simple file in the `.dyna/channels/` directory whose content is the ID of the changeset it points to.

### Concurrency and Goroutine Safety

The `Client` is designed for concurrent use. Methods are safe to be called from multiple goroutines. A single `http.Client` is shared across the `Client` instance for connection reuse. The `Repository` object, however, is **not** goroutine-safe and should be accessed by only one goroutine at a time, or protected by a mutex if shared.

### Error Handling

Errors are handled using the standard Go `error` return pattern. The library defines a set of custom error types (e.g., `ErrNotFound`, `ErrConflict`) to allow callers to programmatically inspect and handle different failure modes. HTTP status codes from the server are translated into corresponding error types.

### Compression

All communication with the `dyna-server` (both REST and WebSocket) is compressed using gzip. For REST requests, `dyna-go` automatically sets the `Content-Encoding: gzip` and `Accept-Encoding: gzip` headers. WebSocket messages are compressed on a per-message basis.

## 6. Integration Examples

### Full Workflow: Initialize, Create, Commit, Push

```go
package main

import (
	"context"
	"fmt"
	"log"

	"spf13/afero"
	"your/project/path/dyna-go"
)

func main() {
	ctx := context.Background()
	fs := afero.NewMemMapFs() // Use in-memory FS for demo

	// 1. Initialize a new client
	client := dynago.NewClient("http://localhost:8080", fs)

	// 2. Initialize a new local repository
	repo, err := dynago.Init("/my-app-data", fs)
	if err != nil {
		log.Fatalf("Failed to init repo: %v", err)
	}

	// 3. Create a new resource
	userJSON := `{"name": "Alice", "email": "alice@example.com"}`
	if err := repo.Add("users/user-1.json", []byte(userJSON)); err != nil {
		log.Fatalf("Failed to add resource: %v", err)
	}

	// 4. Commit the change
	changeset, err := repo.Commit("Go App", "Initial commit: Add user Alice")
	if err != nil {
		log.Fatalf("Failed to commit: %v", err)
	}
	fmt.Printf("Committed new changeset: %s\n", changeset.ID)

	// 5. Push the changes to the server
	pushResp, err := client.Push(ctx, "/my-app-data", "main")
	if err != nil {
		log.Fatalf("Failed to push: %v", err)
	}
	fmt.Printf("Successfully pushed %d changesets.\n", pushResp.ChangesetCount)
}
```

### Real-time Updates with WebSockets

```go
package main

import (
	"context"
	"fmt"
	"log"

	"gorilla/websocket"
	"spf13/afero"
	"your/project/path/dyna-go"
)

func main() {
	ctx, cancel := context.WithCancel(context.Background())
	defer cancel()

	fs := afero.NewMemMapFs()
	client := dynago.NewClient("http://localhost:8080", fs)

	// Assume repo is cloned or initialized

	// Connect to the WebSocket endpoint
	conn, err := client.Connect(ctx, "/my-app-data")
	if err != nil {
		log.Fatalf("WebSocket connection failed: %v", err)
	}
	defer conn.Close()

	fmt.Println("WebSocket connected. Listening for updates...")

	// Loop to read messages
	for {
		var notification dynago.Notification
		if err := conn.ReadJSON(&notification); err != nil {
			if websocket.IsUnexpectedCloseError(err, websocket.CloseGoingAway, websocket.CloseAbnormalClosure) {
				log.Printf("error: %v", err)
			}
			break
		}

		// Process the notification
		switch payload := notification.Payload.(type) {
		case *dynago.PushPayload:
			fmt.Printf("Received push notification for channel '%s' with %d new changesets.\n", payload.Channel, len(payload.Changesets))
			// You might trigger a pull here
		case *dynago.PromotionPayload:
			fmt.Printf("Channel '%s' was promoted to '%s'.\n", payload.From, payload.To)
		}
	}
}
```

## 7. Building

### Without Nix

To use `dyna-go` in your project, simply add it as a dependency in your `go.mod` file:

```sh
go get dyna-go
```

To build your application:

```sh
go build
```

### With Nix

If your project uses Nix Flakes, you can add `dyna-go` to your inputs:

```nix
# flake.nix
{
  inputs.dynago.url = "github:dyna-proj/dyna-go";

  outputs = { self, nixpkgs, dynago }:
    # ...
}
```

Then, run:

```sh
nix build
```

## 8. Testing

To run the tests for `dyna-go` itself, clone the repository and run:

```sh
go test ./...
```

The tests use the `afero` in-memory filesystem and a mock HTTP server to test the client in isolation without needing a running `dyna-server` instance.

## 9. Related Projects

*   **dyna-core**: The core Rust library for the Dyna ecosystem.
*   **dyna-cli**: A command-line interface for Dyna.
*   **dyna-server**: The Dyna server implementation.
*   **dyna-wasm**: A WebAssembly client for Dyna.
*   **dyna-py**: A Python client for Dyna.
*   **dyna-app**: A web-based UI for Dyna.
*   **lazy-cat**: A lazy resource loader for Dyna.
*   **lazy-go**: A lazy resource loader for Dyna in Go.
*   **lazy-py**: A lazy resource loader for Dyna in Python.
*   **lazy-wasm**: A lazy resource loader for Dyna in WebAssembly.
*   **lazy-elm-demo**: A demo application for `lazy-wasm`.

---

This README provides a comprehensive overview of the `dyna-go` library, its architecture, API, and usage. It is intended to be a living document that will be updated as the library evolves. We welcome contributions and feedback from the community.

For more information on the Dyna ecosystem, please refer to the main **Dyna project repository**.

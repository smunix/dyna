# lazy-go: Lazy, On-Demand Resource Loader for Dyna

A Go client library for lazily loading resources from a Dyna server, with support for real-time updates via WebSockets.

## Problem Statement

Modern applications, especially those with rich user interfaces, often deal with large, complex datasets. When interacting with a backend system like Dyna, a distributed CRUD system for JSON documents, fetching the entire dataset at once can be a significant performance bottleneck. This is particularly true for web and mobile applications where initial load time and memory consumption are critical for a good user experience. The 'eager loading' approach, where all data is retrieved upfront, leads to slow startup times, high bandwidth usage, and unnecessary memory pressure on the client. This problem is exacerbated in collaborative environments where data is constantly changing, as the client would need to repeatedly refetch the entire dataset to stay up-to-date.

To address this, a more sophisticated approach is needed: 'lazy loading'. With lazy loading, data is fetched on-demand as the user navigates the application and requests specific pieces of information. This significantly improves performance and scalability. Furthermore, in a collaborative system like Dyna, it is essential for clients to be notified of changes made by other users in real-time. A simple polling mechanism, where the client periodically asks the server for updates, is inefficient and introduces latency. A push-based mechanism, where the server notifies clients of changes as they happen, is far more effective for building responsive, real-time applications.

## Intent and Goals

The primary goal of `lazy-go` is to provide a high-level, idiomatic Go library that simplifies interaction with the Dyna ecosystem by abstracting away the complexities of on-demand data fetching and real-time updates. It is designed to be a 'lazy' client, in contrast to the more low-level 'eager' clients, by fetching resources only when they are explicitly requested. This design philosophy is centered around efficiency, performance, and ease of use.

The key design goals of `lazy-go` are:

*   **Efficiency:** Minimize network traffic and memory usage by only loading the data that is needed.
*   **Simplicity:** Provide a clean, intuitive API that is easy to integrate into any Go application.
*   **Real-time Capabilities:** Offer a seamless way to subscribe to and handle real-time updates from the Dyna server.
*   **Robustness:** Ensure reliable and predictable behavior through comprehensive error handling and a thread-safe design.
*   **Transparency:** Hide the underlying complexities of the Dyna protocol, such as compression and WebSocket management, from the end-user.

*   Provide a `LazyClient` for connecting to a Dyna server.
*   Enable fetching of individual resources by their ID.
*   Allow listing and fetching all resources under a given path.
*   Offer a mechanism to subscribe to real-time updates from the server using WebSockets.
*   Present a clear and rich `UpdateEvent` structure for handling different types of server-sent notifications.

## Architecture

`lazy-go` acts as a client library that communicates with a `dyna-server` instance. It utilizes the `dyna-go` library for the underlying communication protocol with the Dyna server. The architecture can be visualized as follows:

```
+-----------------+      +-----------------+      +-----------------+
| Go Application  |----->|     lazy-go     |----->|     dyna-go     |
+-----------------+      |   (LazyClient)  |      |    (Client)     |
                         +-----------------+      +-----------------+
                                                       |
                                                       |
                                                       v
                                                +-----------------+
                                                |   dyna-server   |
                                                +-----------------+
```

**Data Flow for a `Get` request:**

1.  The Go application calls `lazyClient.Get("acme.entity.User")`.
2.  `lazy-go` checks its local cache for the resource.
3.  If not in the cache, it uses the underlying `dyna-go` client to send a `GET /api/v1/resources/acme.entity.User` request to the `dyna-server`.
4.  The `dyna-server` retrieves the resource, compresses it, and sends it back.
5.  `lazy-go` receives the response, decompresses it, and returns the JSON resource to the application.

**Data Flow for `OnUpdate`:**

1.  The Go application calls `lazyClient.OnUpdate(...)`.
2.  `lazy-go` establishes a WebSocket connection to `/api/v1/ws` on the `dyna-server`.
3.  When a change occurs on the server (e.g., a push or promotion), the `dyna-server` broadcasts a notification.
4.  `lazy-go` receives the WebSocket message, parses it into an `UpdateEvent`, and passes it to the application's handler.


## API Reference

The `lazy-go` library exposes a `LazyClient` with the following methods:

| Method          | Parameters                                     | Returns                               | Description                                                                                                                               |
|-----------------|------------------------------------------------|---------------------------------------|-------------------------------------------------------------------------------------------------------------------------------------------|
| `Connect`       | `url string`                                   | `(*LazyClient, error)`                | Establishes a connection to the Dyna server at the given URL.                                                                             |
| `Get`           | `id string`                                    | `(map[string]interface{}, error)`     | Fetches a single resource by its ID.                                                                                                      |
| `ListResources` | `path string`                                  | `([]string, error)`                    | Lists all resource IDs under a given path.                                                                                                |
| `GetAll`        | `path string`                                  | `([]map[string]interface{}, error)`   | Fetches all resources under a given path.                                                                                                 |
| `ForEachAll`    | `path string, f func(map[string]interface{})` | `error`                               | Iterates over all resources under a given path, executing a callback function for each resource.                                        |
| `OnUpdate`      | `handler func(UpdateEvent)`                    | `error`                               | Subscribes to real-time updates from the server. The provided handler function is called for each `UpdateEvent` received from the server. |

### The `UpdateEvent` struct

The `UpdateEvent` struct represents a notification received from the Dyna server. It has the following structure:

```go
// UpdateEvent represents a real-time update from the Dyna server.
// It can be one of several types, discriminated by the `Type` field.
type UpdateEvent struct {
	Type      string      `json:"type"`
	Payload   interface{} `json:"payload"`
}
```

Possible `Type` values and their corresponding `Payload` structures:

*   **`push`**: A new changeset has been pushed to a channel.
    ```go
    type PushPayload struct {
        Channel     string        `json:"channel"`
        Changeset   ChangesetInfo `json:"changeset"`
    }

    type ChangesetInfo struct {
        ID          string      `json:"id"`
        Author      string      `json:"author"`
        Timestamp   int64       `json:"timestamp"`
        Description string      `json:"description"`
    }
    ```
*   **`promotion`**: A channel has been promoted to another.
    ```go
    type PromotionPayload struct {
        FromChannel string `json:"from_channel"`
        ToChannel   string `json:"to_channel"`
    }
    ```

## Technical Design

`lazy-go` is designed to be a lightweight and efficient client for the Dyna ecosystem. It builds upon the core functionality of `dyna-go` to provide a higher-level, lazy-loading abstraction.

### Data Structures

The primary data structure is the `LazyClient`:

```go
// LazyClient provides a high-level API for interacting with a Dyna server.
type LazyClient struct {
	dynaClient *dynago.Client
	cache      map[string]map[string]interface{}
	mu         sync.Mutex
}
```

*   `dynaClient`: An instance of the `dyna-go` client, used for all communication with the `dyna-server`.
*   `cache`: An in-memory map used to cache resources that have been fetched from the server. This reduces redundant network requests.
*   `mu`: A mutex to ensure thread-safe access to the cache.

### Concurrency Model

The `OnUpdate` method is designed to be non-blocking. When called, it spawns a new goroutine to manage the WebSocket connection. This goroutine listens for incoming messages from the server, deserializes them into `UpdateEvent` structs, and passes them to the application-provided handler function. This allows the main application to continue its execution while listening for updates in the background.

### Error Handling

All public methods of the `LazyClient` return an `error` as their last return value. This allows the calling application to gracefully handle network issues, server errors, or problems with data parsing. The library does not use panics for recoverable errors.

### Compression

All communication with the `dyna-server` is compressed using gzip. This is handled transparently by the underlying `dyna-go` library, so the application using `lazy-go` does not need to be aware of the compression details. This reduces bandwidth usage and improves performance over slow networks.

## Integration Examples

Here is a complete example of how to use the `lazy-go` client to connect to a Dyna server, fetch a resource, and listen for real-time updates.

```go
package main

import (
	"fmt"
	"log"
	"time"

	"lazy-go"
)

func main() {
	// Connect to the Dyna server
	client, err := lazygo.Connect("http://localhost:8080")
	if err != nil {
		log.Fatalf("Failed to connect to Dyna server: %v", err)
	}

	// Fetch a single resource
	user, err := client.Get("acme.entity.User:123")
	if err != nil {
		log.Printf("Failed to get resource: %v", err)
	} else {
		fmt.Printf("Fetched user: %v\n", user)
	}

	// List all resources under a path
	products, err := client.ListResources("acme.entity.Product")
	if err != nil {
		log.Printf("Failed to list resources: %v", err)
	} else {
		fmt.Printf("Products: %v\n", products)
	}

	// Listen for real-time updates
	go func() {
		err := client.OnUpdate(func(event lazygo.UpdateEvent) {
			fmt.Printf("Received update: %+v\n", event)
		})
		if err != nil {
			log.Printf("Failed to subscribe to updates: %v", err)
		}
	}()

	// Keep the application running to receive updates
	fmt.Println("Listening for updates...")
	time.Sleep(10 * time.Minute)
}
```

## Building

### With Nix

If you have Nix installed, you can build the project by running:

```sh
nix build
```

### Without Nix

To build the project without Nix, you will need to have Go installed. Then, you can run:

```sh
go build ./...
```

## Testing

To run the tests for this project, run:

```sh
go test ./...
```

## Related Projects

*   **dyna-core**: The core Rust library for the Dyna ecosystem.
*   **dyna-cli**: A command-line interface for interacting with a Dyna server.
*   **dyna-server**: The main server implementation for the Dyna ecosystem.
*   **dyna-wasm**: A WebAssembly client for the Dyna ecosystem.
*   **dyna-py**: Python bindings for the Dyna ecosystem.
*   **dyna-go**: A Go client library for the Dyna ecosystem.
*   **dyna-app**: A web-based user interface for the Dyna ecosystem.
*   **lazy-cat**: A Rust lazy resource loader for the Dyna ecosystem.
*   **lazy-py**: A Python lazy resource loader for the Dyna ecosystem.
*   **lazy-wasm**: A WebAssembly lazy resource loader for the Dyna ecosystem.
*   **lazy-elm-demo**: A demo application showcasing the use of `lazy-wasm`.

### Detailed Architecture

A more detailed view of the architecture, including the WebSocket connection for real-time updates:

```
+-----------------------------------+
|         Your Go Application       |
+-----------------------------------+
|                                   |
|  +-----------------------------+  |
|  |    lazy-go (LazyClient)     |  |
|  +-----------------------------+  |
|  | - cache: map[string]...     |  |
|  | - dynaClient: *dynago.Client|  |
|  +-----------------------------+  |
|      |             ^              |
|      | (Get,      (Resource)      |
|      | List,...)   |              |
|      v             |              |
|  +-----------------------------+  |
|  |      dyna-go (Client)       |  |
|  +-----------------------------+  |
|      |             ^              |
|      | (HTTP/REST) |              |
|      v             |              |
+-----------------------------------+
       |             ^
       | (Requests) (Responses)
       v             |
+-----------------------------------+
|           dyna-server             |
+-----------------------------------+
|  - REST API (/api/v1)             |
|  - WebSocket (/api/v1/ws)         |
+-----------------------------------+
```

### Caching Strategy

The in-memory cache in `LazyClient` plays a crucial role in optimizing performance. The caching strategy is simple yet effective:

*   **Cache on read:** Whenever a resource is fetched from the server using `Get` or `GetAll`, it is stored in the `cache` map. The resource ID is used as the key.
*   **Cache lookup:** Before sending a request to the server, the `LazyClient` first checks if the resource is already in the cache. If it is, the cached version is returned immediately, avoiding a network round-trip.
*   **No cache invalidation:** The current implementation does not include a cache invalidation mechanism. This is a deliberate design choice to keep the library simple. It is assumed that the application will be restarted or the `LazyClient` will be re-initialized if the cache needs to be cleared. For applications that require more sophisticated caching with TTL (time-to-live) or other invalidation strategies, it is recommended to build a caching layer on top of `lazy-go`.

### WebSocket Implementation

The `OnUpdate` method uses the `gorilla/websocket` library to establish and maintain a WebSocket connection with the `dyna-server`. The implementation details are as follows:

*   **Connection:** A new goroutine is spawned to handle the WebSocket connection, ensuring that the main application thread is not blocked.
*   **Message Handling:** The goroutine continuously listens for incoming messages on the WebSocket. Each message is expected to be a JSON-encoded `UpdateEvent`.
*   **Deserialization:** Incoming JSON messages are deserialized into the `UpdateEvent` struct.
*   **Handler Invocation:** The deserialized `UpdateEvent` is passed to the user-provided handler function for processing.
*   **Reconnection:** The current implementation does not automatically handle reconnection in case the WebSocket connection is lost. The application is responsible for detecting a closed connection and re-establishing it if necessary.

### Handling Different Update Events

Here is an example of how to handle different types of `UpdateEvent` in the `OnUpdate` handler.

```go
package main

import (
	"encoding/json"
	"fmt"
	"log"
	"time"

	"lazy-go"
)

func main() {
	client, err := lazygo.Connect("http://localhost:8080")
	if err != nil {
		log.Fatalf("Failed to connect to Dyna server: %v", err)
	}

	go func() {
		err := client.OnUpdate(func(event lazygo.UpdateEvent) {
			switch event.Type {
			case "push":
				var payload lazygo.PushPayload
				jsonPayload, _ := json.Marshal(event.Payload)
				json.Unmarshal(jsonPayload, &payload)
				fmt.Printf("New push on channel '%s': %s\n", payload.Channel, payload.Changeset.ID)
			case "promotion":
				var payload lazygo.PromotionPayload
				jsonPayload, _ := json.Marshal(event.Payload)
				json.Unmarshal(jsonPayload, &payload)
				fmt.Printf("Channel '%s' promoted to '%s'\n", payload.FromChannel, payload.ToChannel)
			default:
				fmt.Printf("Unknown event type: %s\n", event.Type)
			}
		})
		if err != nil {
			log.Printf("Failed to subscribe to updates: %v", err)
		}
	}()

	fmt.Println("Listening for updates...")
	time.Sleep(10 * time.Minute)
}
```

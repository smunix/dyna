
# `lazy-cat`: Lazy, On-Demand Resource Loader for Dyna

`lazy-cat` is a high-performance, asynchronous resource loader for the **Dyna** distributed CRUD system. It enables lazy, on-demand fetching of JSON resources and provides real-time updates through a persistent WebSocket connection.

## Problem Statement

Modern web and native applications often deal with large, complex, and interconnected datasets. In the context of the Dyna ecosystem, where JSON resources are the primary unit of data, client applications may need to access a vast number of resources. The naive approach of pre-emptively fetching all possible resources at application startup—often referred to as eager loading—is highly inefficient and leads to several significant problems:

*   **High Initial Latency:** Users are forced to wait for a large amount of data to be downloaded and processed before the application becomes interactive, leading to a poor user experience.
*   **Increased Memory Footprint:** Storing a large number of resources in memory, many of which may never be used, consumes significant system resources, which is particularly problematic on memory-constrained devices.
*   **Wasted Bandwidth:** Transferring unused data over the network consumes unnecessary bandwidth, which can be costly and slow, especially on mobile or metered connections.
*   **Scalability Challenges:** As the number of resources grows, the eager-loading approach becomes increasingly untenable, creating a bottleneck that limits the scalability of the application.

`lazy-cat` is designed to solve these problems by providing a robust and efficient lazy-loading mechanism. It allows applications to fetch resources only when they are needed, dramatically improving performance, reducing resource consumption, and enhancing scalability.

## Intent and Goals

The design and development of `lazy-cat` are guided by a clear set of principles and goals:

*   **Efficiency and Performance:** The foremost goal is to minimize data transfer, reduce memory overhead, and decrease initial load times. By fetching resources on-demand, `lazy-cat` ensures that the application only loads the data it needs, resulting in a faster and more responsive user experience.

*   **Real-Time Capabilities:** In a collaborative environment like Dyna, it is crucial that clients have access to the most up-to-date information. `lazy-cat` leverages WebSockets to provide a persistent, real-time communication channel with the Dyna server, pushing updates to clients as soon as they occur.

*   **Developer Experience:** The library is designed with a focus on simplicity and ease of use. It provides a clean, intuitive, and asynchronous API that is easy to integrate into any Rust application. The API abstracts away the complexities of network communication, caching, and real-time updates, allowing developers to focus on their application's core logic.

*   **Resilience and Reliability:** `lazy-cat` is built to be robust and fault-tolerant. It automatically handles network interruptions and connection failures, transparently reconnecting to the server to maintain a stable and reliable data layer for the application.

## Core Concepts

`lazy-cat` introduces a few core concepts that are essential to understanding its functionality:

*   **Lazy Loading:** This is the central concept of the library. Resources are not loaded until they are explicitly requested by the application. This "just-in-time" approach to data fetching is what makes `lazy-cat` so efficient.

*   **On-Demand Fetching:** When a resource is requested, `lazy-cat` fetches it from the Dyna server over an HTTP connection. This is a one-time operation for each resource, as the fetched data is then cached locally.

*   **Real-Time Updates:** `lazy-cat` maintains a persistent WebSocket connection to the Dyna server. This connection is used to receive real-time notifications of any changes to resources that the client has previously loaded. This ensures that the local cache is always kept in sync with the server.

*   **Local Caching:** To avoid repeated network requests for the same resource, `lazy-cat` maintains an in-memory cache of all the resources it has fetched. When a resource is requested, the library first checks the cache. If the resource is present, it is returned immediately, avoiding a network round-trip.

## Architecture

`lazy-cat` is designed as a client-side library that sits between your application and the Dyna server. It manages all the interactions with the server, providing a simplified and efficient data access layer for your application.

### Component Diagram

The following diagram illustrates the high-level architecture of `lazy-cat` and its interaction with the client application and the Dyna server:

```ascii
+-----------------------------------+
|         Client Application        |
+-----------------------------------+
| - Makes API calls to LazyClient   |
| - Receives resources and updates  |
+-----------------------------------+
                 ^
                 |
                 v
+----------------------------------------------------+
|                       `lazy-cat`                   |
|                     (LazyClient)                   |
+----------------------------------------------------+
|                                                    |
|  +------------------+   +-----------------------+  |
|  |  Resource Cache  |   |   WebSocket Client    |  |
|  | (HashMap)        |   | (tokio-tungstenite)   |  |
|  +------------------+   +-----------------------+  |
|          ^                           ^             |
|          |                           |             |
|          v                           v             |
|  +------------------+   +-----------------------+  |
|  |  HTTP Client     |   |   Update Manager      |  |
|  | (reqwest)        |   |                       |  |
|  +------------------+   +-----------------------+  |
|                                                    |
+----------------------------------------------------+
                 ^
                 |                +-----------------------------------+
                 +--------------->|            Dyna Server            |
                 | (HTTP/WS)      +-----------------------------------+
                 |                | - Serves resources via REST API   |
                 v                | - Pushes updates via WebSockets   |
+-----------------------------------+
|         Network Interface         |
+-----------------------------------+
```

### Data Flow

The data flow in a typical `lazy-cat` integration can be broken down into two main scenarios: initial resource fetching and real-time updates.

**1. Initial Resource Fetching:**

```ascii
Client App         LazyClient           Dyna Server
    |                  |
    |--- get(res_id) -->|                  |
    |                  |
    |                  |--- Is res_id in cache? ---
    |                  |        | (No)         |
    |                  |        v              |
    |                  |--- HTTP GET /resources/{id} ->|
    |                  |                      |
    |                  |<-- resource data ----|
    |                  |                      |
    |                  |--- Store in cache ---|
    |                  |        |              |
    |<-- resource data ----|                  |
    |                  |
```

**2. Real-Time Updates:**

```ascii
Client App         LazyClient           Dyna Server
    |                  |                      |
    |--- on_update -->|                      |
    |                  |--- WebSocket Handshake --->|
    |                  |                      |
    |                  |                      |<-- push notification --
    |                  |<---- update_event ---|
    |                  |                      |
    |                  |--- Update cache -----|
    |                  |        |              |
    |<---- update_event ---|                  |
    |                  |
```

## API Reference

The public API of `lazy-cat` is exposed through the `LazyClient` struct. All methods are asynchronous and return a `Result` type to allow for proper error handling.

| Method               | Parameters                               | Return Type                  | Description                                                                                                                               |
| -------------------- | ---------------------------------------- | ---------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------- |
| `connect`            | `ws_url: &str`                           | `Result<Self>`               | Establishes a WebSocket connection to the Dyna server and initializes the `LazyClient`. This is the entry point for using the library.                                                                                    |
| `get`                | `resource_id: &ResourceId`               | `Result<serde_json::Value>`  | Fetches a single resource by its ID. It first checks the local cache. If the resource is not found, it fetches it from the server via an HTTP GET request.                                                                                                      |
| `get_many`           | `resource_ids: &[ResourceId]`            | `Result<Vec<serde_json::Value>>` | Fetches multiple resources by their IDs. This is more efficient than calling `get` in a loop as it can potentially batch requests.                                                                              |
| `for_each`           | `resource_ids: &[ResourceId], callback: F` | `Result<()>`                 | Fetches multiple resources and applies a callback to each one. The callback `F` is a closure that takes a `serde_json::Value`. This is useful for processing multiple resources without collecting them into a `Vec`.                             |
| `for_each_all`       | `callback: F`                            | `Result<()>`                 | Fetches all available resources from the server and applies a callback to each one. The callback `F` is a closure that takes a `serde_json::Value`.                                |
| `list_resources`     |                                          | `Result<Vec<ResourceId>>`    | Retrieves a list of all available resource IDs from the server. This can be used to discover what resources are available.                                                                                           |
| `get_all`            |                                          | `Result<Vec<serde_json::Value>>` | Fetches all resources from the server. This should be used with caution as it can be a resource-intensive operation if there are many resources.                                                                                                    |
| `on_update`          | `callback: F`                            | `Result<()>`                 | Registers a callback to be invoked when a resource is updated on the server. The callback `F` is a closure that takes an `UpdateEvent`. This is the mechanism for receiving real-time updates.                             |

### `UpdateEvent` Struct

The `on_update` callback receives an `UpdateEvent` struct, which contains information about the resource that was updated.

| Field           | Type            | Description                                      |
| --------------- | --------------- | ------------------------------------------------ |
| `resource_id`   | `ResourceId`    | The unique identifier of the resource that was updated.         |
| `new_changeset` | `Changeset`     | The new changeset that was applied to the resource, containing the patch with the changes.          |

## Technical Design

`lazy-cat` is built using modern, asynchronous Rust. It leverages the `tokio` runtime for its core concurrency model, `reqwest` for making HTTP requests, and `tokio-tungstenite` for its WebSocket implementation. The internal design focuses on performance, reliability, and ease of use.

### Caching Strategy

To minimize network latency and reduce redundant data fetching, `lazy-cat` employs an in-memory, write-through caching strategy. The cache is implemented using a `tokio::sync::RwLock` wrapped around a `std::collections::HashMap`. This allows for concurrent read access to the cache while ensuring that writes (insertions and updates) are exclusive, preventing data races.

*   **Read Operations:** When a resource is requested via `get` or `get_many`, the client first acquires a read lock on the cache. If the resource is present, it is cloned and returned immediately. This is a highly concurrent and fast operation.
*   **Write Operations:** If a resource is not found in the cache, a write lock is acquired. The client then proceeds to fetch the resource from the server. Once the resource is successfully fetched, it is inserted into the cache before the write lock is released. This ensures that subsequent requests for the same resource will be served from the cache.
*   **Cache Invalidation:** The cache is kept up-to-date via the real-time update mechanism. When an `UpdateEvent` is received from the server, the client acquires a write lock on the cache and updates the corresponding resource with the new data from the event. This write-through approach ensures that the cache remains consistent with the state of the data on the server.

### Concurrency Model

`lazy-cat` is designed to be fully asynchronous and non-blocking, making it suitable for use in high-performance, concurrent applications. The concurrency model is built around the `tokio` runtime and its ecosystem of asynchronous primitives.

*   **Asynchronous API:** All public methods of the `LazyClient` are `async` and return `Future`s. This allows the client application to perform other work while waiting for network operations to complete.
*   **Task Spawning:** The WebSocket listener runs in a separate `tokio` task, which is spawned when `LazyClient::connect` is called. This task is responsible for listening for incoming messages from the server, deserializing them into `UpdateEvent`s, and dispatching them to the update handler.
*   **Shared State:** The `LazyClient` is designed to be cloneable and shareable across multiple tasks. The internal state, including the resource cache and the WebSocket connection, is wrapped in `Arc` and `RwLock` to allow for safe concurrent access.

### WebSocket Protocol and Communication

The WebSocket connection is the backbone of `lazy-cat`'s real-time capabilities. It is used exclusively for receiving update notifications from the Dyna server.

*   **Connection Lifecycle:** The WebSocket connection is established when `LazyClient::connect` is called. The library handles the entire lifecycle of the connection, including the initial handshake, message framing, and heartbeat (ping/pong) messages to keep the connection alive.
*   **Message Format:** The server is expected to send update notifications as JSON-encoded text messages. Each message should be a JSON object that can be deserialized into the `UpdateEvent` struct. Any messages that do not conform to this format are ignored.
*   **Automatic Reconnection:** Network connections can be unreliable. To handle this, `lazy-cat` implements a robust automatic reconnection mechanism with exponential backoff. If the WebSocket connection is lost, the library will attempt to reconnect after a short delay. If the reconnection attempt fails, it will wait for a longer period before trying again, up to a maximum delay. This prevents the client from overwhelming the server with reconnection requests during a prolonged outage.

### Error Handling

Robust error handling is a key design consideration for `lazy-cat`. The library uses the `anyhow` crate to provide rich, context-aware error messages.

*   **Result Type:** All public methods that can fail return a `anyhow::Result`. This allows the calling code to use the `?` operator for concise error propagation.
*   **Specific Error Types:** The library defines a set of specific error types that can be returned, allowing the calling code to handle different error conditions programmatically. These include errors related to network I/O, serialization/deserialization, and server-side issues.

### Compression

To minimize bandwidth usage, all communication with the Dyna server, both over HTTP and WebSockets, is compressed using gzip. This is handled transparently by the underlying `dyna-core` library and the `reqwest` and `tokio-tungstenite` crates. The `lazy-cat` library itself does not need to be aware of the compression, as it is handled at a lower level.

## Integration Examples

Here are a couple of examples demonstrating how to use `lazy-cat` in a Rust application.

### Basic Usage: Fetching a Resource

This example shows how to connect to a Dyna server and fetch a single resource.

```rust
use lazy_cat::LazyClient;
use dyna_core::resource::ResourceId;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Connect to the Dyna server's WebSocket endpoint
    let client = LazyClient::connect("ws://localhost:8080/api/v1/ws").await?;

    // Parse the resource ID
    let resource_id: ResourceId = "com.example.my_resource".parse()?;

    // Fetch the resource
    let resource = client.get(&resource_id).await?;

    println!("Successfully fetched resource:");
    println!("{:#?}", resource);

    Ok(())
}
```

### Advanced Usage: Real-Time Updates

This example demonstrates how to subscribe to real-time updates for a resource.

```rust
use lazy_cat::LazyClient;
use dyna_core::resource::ResourceId;
use std::time::Duration;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Connect to the Dyna server
    let client = LazyClient::connect("ws://localhost:8080/api/v1/ws").await?;

    // Subscribe to updates
    let update_handle = client.on_update(|update_event| {
        println!("Received a real-time update!");
        println!("Resource ID: {}", update_event.resource_id);
        println!("New Changeset: {:#?}", update_event.new_changeset);
    }).await?;

    println!("Listening for real-time updates... Press Ctrl+C to exit.");

    // Keep the application running to receive updates
    loop {
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
}
```

## Building

### With Nix

If you are using the Nix package manager, you can build the project by running the following command from the root of the repository:

```sh
nix build
```

This will create a `result` directory with the compiled artifacts.

### Without Nix

If you are not using Nix, you can build the project using Cargo, the Rust build tool and package manager. Make sure you have a recent version of the Rust toolchain installed.

```sh
cargo build --release
```

The compiled binary will be located in the `target/release` directory.

## Testing

To run the test suite, you can use Cargo:

```sh
cargo test
```

This will run all the unit and integration tests for the `lazy-cat` library.

## Contributing

Contributions are welcome! If you would like to contribute to `lazy-cat`, please feel free to fork the repository and submit a pull request. If you find a bug or have a feature request, please open an issue on the GitHub repository.

## License

`lazy-cat` is licensed under the MIT License. See the `LICENSE` file for more details.

## Related Projects

`lazy-cat` is part of the broader Dyna ecosystem. Here are some other related projects:

*   **dyna-core**: The core library for the Dyna ecosystem, containing shared data structures, protocols, and the patch engine.
*   **dyna-cli**: A command-line interface for interacting with a Dyna server.
*   **dyna-server**: The reference implementation of the Dyna server.
*   **lazy-go**: A Go implementation of the `lazy-cat` client.
*   **lazy-py**: A Python implementation of the `lazy-cat` client.
*   **lazy-wasm**: A WebAssembly version of the `lazy-cat` client for use in web browsers.
*   **lazy-elm-demo**: A demonstration application showcasing the use of `lazy-wasm` in an Elm web application.

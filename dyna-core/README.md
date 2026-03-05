# dyna-core

A core Rust library providing data models, protocol types, and the diff engine for the Dyna distributed CRUD system.

## Problem Statement

In modern collaborative applications, managing and synchronizing structured data (like JSON) across multiple clients and a central server is a complex challenge. Traditional approaches often lead to race conditions, data loss, and complicated conflict resolution logic. There is a need for a robust system that provides version control for structured data, similar to how Git manages source code. `dyna-core` is the foundational library that addresses this problem by providing the core data structures and logic for a changeset-centric version control system for JSON.

## Intent and Goals

The primary goal of `dyna-core` is to provide a solid foundation for the Dyna ecosystem. It is designed with the following principles in mind:

*   **Correctness**: To ensure data integrity and consistency, even in a distributed and concurrent environment.
*   **Performance**: To be efficient in terms of memory usage and processing speed, especially for diffing and patching operations.
*   **Modularity**: To provide a clear and well-defined API that can be easily integrated into other components of the Dyna ecosystem.
*   **Extensibility**: To be flexible enough to support future enhancements and new features.

## Architecture

The `dyna-core` library sits at the heart of the Dyna ecosystem. It defines the fundamental data models and their interactions. The following diagram illustrates the core components and their relationships:

```
+-----------------+
|    Resource     |
| (JSON Document) |
+-----------------+
        |
        | 1..*
        v
+-----------------+
|      Patch      |
| (RFC 6902 Op)   |
+-----------------+
        |
        | 1..*
        v
+-----------------+
|    Changeset    |
| (Group of       |
|  Patches)       |
+-----------------+
        |
        | 1..*
        v
+-----------------+
|     Channel     |
| (Named Branch)  |
+-----------------+
```

### Data Flow

1.  A **Resource** is a JSON document identified by a unique ID. Resources are the primary unit of data in the Dyna system. They are analogous to files in a file system.
2.  Changes to a Resource are represented as a series of **Patches**, which are RFC 6902 JSON Patch operations. This granular approach to representing changes allows for fine-grained control and efficient conflict resolution.
3.  A group of Patches is bundled into a **Changeset**, which is content-addressable and represents a single unit of change. Each changeset has a unique ID, which is a hash of its contents. This makes changesets immutable and easy to reference.
4.  **Channels** are named pointers to Changesets, similar to branches in Git, allowing for parallel development and feature isolation. This enables workflows where different users can work on the same resource without interfering with each other.

## API Reference

### Data Models

#### `Resource`

Represents a JSON document in the Dyna system.

| Field      | Type           | Description                                      |
| ---------- | -------------- | ------------------------------------------------ |
| `id`       | `String`       | The unique identifier of the resource.           |
| `content`  | `serde_json::Value` | The JSON content of the resource.                |
| `version`  | `String`       | The version of the resource (changeset hash).    |

#### `PatchOperation`

Represents a single RFC 6902 JSON Patch operation.

| Variant   | Description                                      |
| --------- | ------------------------------------------------ |
| `Add`     | Adds a value to an object or inserts it into an array. |
| `Remove`  | Removes a value from an object or array.         |
| `Replace` | Replaces a value.                                |
| `Move`    | Moves a value from one location to another.      |
| `Copy`    | Copies a value from one location to another.     |
| `Test`    | Tests that a value at a specified location is equal to a given value. |

#### `Patch`

A collection of `PatchOperation`s.

| Field        | Type                  | Description                                      |
| ------------ | --------------------- | ------------------------------------------------ |
| `operations` | `Vec<PatchOperation>` | A list of JSON Patch operations.                 |

#### `Changeset`

A group of patches that represent a single atomic change.

| Field       | Type           | Description                                      |
| ----------- | -------------- | ------------------------------------------------ |
| `id`        | `String`       | The content-addressable hash of the changeset.   |
| `patches`   | `Vec<Patch>`   | The list of patches included in the changeset.   |
| `author`    | `String`       | The author of the changeset.                     |
| `timestamp` | `chrono::DateTime<Utc>` | The timestamp of when the changeset was created. |

#### `Channel`

A named pointer to a changeset, similar to a Git branch.

| Field | Type     | Description                                      |
| ----- | -------- | ------------------------------------------------ |
| `name`  | `String` | The name of the channel.                         |
| `head`  | `String` | The hash of the changeset the channel points to. |

### Protocol Types

These types are used for communication between the client and the server.

#### `PushRequest`

| Field        | Type                  | Description                                      |
| ------------ | --------------------- | ------------------------------------------------ |
| `changesets` | `Vec<Changeset>`      | The changesets to be pushed to the server.       |

#### `PushResponse`

| Field        | Type                  | Description                                      |
| ------------ | --------------------- | ------------------------------------------------ |
| `success`    | `bool`                | Whether the push was successful.                 |
| `error`      | `Option<String>`      | An error message if the push failed.             |

#### `PullRequest`

| Field        | Type                  | Description                                      |
| ------------ | --------------------- | ------------------------------------------------ |
| `channel`    | `String`              | The channel to pull changes from.                |
| `since`      | `Option<String>`      | The last changeset hash the client has.          |

#### `PullResponse`

| Field        | Type                  | Description                                      |
| ------------ | --------------------- | ------------------------------------------------ |
| `changesets` | `Vec<Changeset>`      | The new changesets from the server.              |

### Notification Types

These types are used for real-time notifications over WebSockets.

#### `Notification`

An enum that represents different types of notifications.

| Variant         | Payload                | Description                                      |
| --------------- | ---------------------- | ------------------------------------------------ |
| `Pushed`        | `PushPayload`          | A notification that a new changeset has been pushed. |
| `Promoted`      | `PromotionPayload`     | A notification that a channel has been promoted. |

#### `PushPayload`

| Field           | Type                  | Description                                      |
| --------------- | --------------------- | ------------------------------------------------ |
| `changeset_info`| `ChangesetInfo`       | Information about the pushed changeset.          |

#### `PromotionPayload`

| Field           | Type                  | Description                                      |
| --------------- | --------------------- | ------------------------------------------------ |
| `channel`       | `String`              | The name of the promoted channel.                |
| `to_changeset`  | `String`              | The new head of the channel.                     |

## Technical Design

### Data Structures

The core data structures are designed to be efficient and serializable. They use standard Rust types and `serde` for JSON serialization and deserialization. The use of `serde_json::Value` allows for flexible and dynamic handling of JSON data.

### Algorithms

*   **Diff Engine**: `dyna-core` includes a highly optimized implementation of the RFC 6902 JSON Patch standard for generating and applying patches. The diffing algorithm is based on the paper "An O(ND) Difference Algorithm and Its Variations" by Eugene W. Myers, which is known for its efficiency.
*   **Content-Addressable Hashing**: Changesets are identified by a SHA-256 hash of their content, ensuring data integrity and providing a basis for the distributed nature of the system. This also allows for efficient storage and retrieval of changesets.

### Concurrency

`dyna-core` is designed to be thread-safe and can be used in concurrent applications. The data models are immutable where possible, and any modifications result in new instances, which helps to prevent race conditions. The library uses `Arc` and `Mutex` to ensure safe access to shared data when necessary.

### Error Handling

The library uses `thiserror` and `anyhow` for robust error handling. All functions and methods that can fail return a `Result` type, allowing for clean and predictable error management. This makes it easy for developers to handle errors in a structured way.

### Compression

All data is compressed using `gzip` before being stored or transmitted, reducing storage and bandwidth requirements. The `flate2` crate is used for this purpose, which provides a fast and reliable implementation of the DEFLATE algorithm.

## Integration Examples

### Simple Example

Here is a simple example of how to use `dyna-core` to create a changeset and apply it to a resource:

```rust
use dyna_core::{
    PatchOperation, Patch, Changeset, Resource
};
use serde_json::json;

fn main() {
    // Create a new resource
    let mut resource = Resource {
        id: "acme.entity.User:123".to_string(),
        content: json!({
            "name": "Alice",
            "email": "alice@example.com"
        }),
        version: "".to_string(),
    };

    // Create a patch to update the name
    let patch = Patch {
        operations: vec![
            PatchOperation::Replace {
                path: "/name".to_string(),
                value: json!("Bob"),
            },
        ],
    };

    // Create a changeset
    let changeset = Changeset::new(vec![patch], "test-user".to_string());

    // Apply the changeset to the resource
    let new_content = changeset.apply(&resource.content).unwrap();
    resource.content = new_content;
    resource.version = changeset.id.clone();

    println!("Updated resource: {:#?}", resource);
}
```

### Advanced Example: Conflict Resolution

This example demonstrates how to handle a conflict when two users make changes to the same resource concurrently.

```rust
use dyna_core::{Changeset, Patch, PatchOperation, Resource};
use serde_json::json;

fn main() {
    // Initial resource state
    let base_resource = Resource {
        id: "acme.entity.User:123".to_string(),
        content: json!({
            "name": "Alice",
            "email": "alice@example.com"
        }),
        version: "initial".to_string(),
    };

    // User 1 changes the name
    let patch1 = Patch {
        operations: vec![PatchOperation::Replace {
            path: "/name".to_string(),
            value: json!("Alicia"),
        }],
    };
    let changeset1 = Changeset::new(vec![patch1], "user1".to_string());

    // User 2 changes the email
    let patch2 = Patch {
        operations: vec![PatchOperation::Replace {
            path: "/email".to_string(),
            value: json!("alicia@example.com"),
        }],
    };
    let changeset2 = Changeset::new(vec![patch2], "user2".to_string());

    // Apply the first changeset
    let resource_after_cs1 = Resource {
        content: changeset1.apply(&base_resource.content).unwrap(),
        version: changeset1.id.clone(),
        ..
        base_resource.clone()
    };

    // Try to apply the second changeset to the original base resource
    // This will succeed because the changes are not conflicting
    let resource_after_cs2_on_base = changeset2.apply(&base_resource.content).unwrap();

    // To merge the changes, we can apply the second changeset to the resource after the first changeset
    let merged_content = changeset2.apply(&resource_after_cs1.content).unwrap();

    let merged_resource = Resource {
        content: merged_content,
        version: "merged".to_string(), // In a real system, this would be a new changeset hash
        ..
        base_resource
    };

    println!("Merged resource: {:#?}", merged_resource);
}

```

## Building

### With Nix

If you have Nix installed, you can build the project with the following command:

```sh
nix build
```

### Without Nix

If you don't have Nix, you can build the project using Cargo:

```sh
cargo build --release
```

## Testing

To run the tests for `dyna-core`, use the following command:

```sh
cargo test
```

## Related Projects

*   **dyna-cli**: A command-line interface for interacting with the Dyna system.
*   **dyna-server**: The server component of the Dyna system.
*   **dyna-wasm**: A WebAssembly build of `dyna-core` for use in web browsers.
*   **dyna-py**: Python bindings for `dyna-core`.
*   **dyna-go**: Go bindings for `dyna-core`.
*   **dyna-app**: A web application that uses `dyna-wasm` to provide a rich user interface for the Dyna system.
*   **lazy-cat**: A lazy resource loader for Dyna.
*   **lazy-go**: A Go implementation of the lazy resource loader.
*   **lazy-py**: A Python implementation of the lazy resource loader.
*   **lazy-wasm**: A WebAssembly build of the lazy resource loader.
*   **lazy-elm-demo**: A demo application that showcases the capabilities of the lazy loader.

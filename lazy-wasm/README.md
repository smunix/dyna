# lazy-wasm: Lazy, On-Demand Resource Loader for Dyna (WebAssembly)

A WebAssembly (WASM) client that provides lazy, on-demand loading of JSON resources from a Dyna server, with real-time updates over WebSockets. It is the browser counterpart to `lazy-cat`.

## Problem Statement

Modern web applications often deal with large, complex JSON data structures. Fetching all data upfront can lead to slow initial load times and high memory consumption, degrading the user experience. Applications need a way to efficiently load only the data they need, when they need it, while also staying up-to-date with changes on the server.

`lazy-wasm` solves this problem by providing a lightweight, efficient, and easy-to-use client for the Dyna ecosystem that runs directly in the browser. It allows web applications to lazily load JSON resources from a Dyna server, ensuring that only the necessary data is transferred and stored in memory. It also provides a real-time update mechanism, allowing applications to react to changes in the data as they happen.

## Intent and Goals

The primary goal of `lazy-wasm` is to provide a simple, yet powerful, interface for interacting with Dyna servers from a web browser. It is designed with the following principles in mind:

*   **Performance:** `lazy-wasm` is written in Rust and compiled to WebAssembly, ensuring near-native performance in the browser. It uses efficient data structures and algorithms to minimize memory usage and CPU overhead.
*   **Lazy Loading:** Resources are loaded on-demand, reducing initial load times and network traffic.
*   **Real-time Updates:** `lazy-wasm` uses WebSockets to receive real-time updates from the Dyna server, allowing applications to stay in sync with the latest data.
*   **Ease of Use:** The API is designed to be simple and intuitive, making it easy to integrate `lazy-wasm` into any web application.
*   **Small Footprint:** The compiled WASM module is small and has minimal dependencies, making it suitable for use in a wide range of web applications.

## Architecture

`lazy-wasm` consists of a `LazyWasmClient` that runs within the browser and communicates with a `dyna-server` instance. The client uses HTTP for initial data fetching and a WebSocket for real-time updates.

```ascii
+----------------------------------------------------+
|                      Browser                       |
| +------------------------------------------------+ |
| |              Your Web Application (JS/TS)      | |
| +------------------------------------------------+ |
|                         |                          |
|                         | Calls                    |
|                         |                          |
| +------------------------------------------------+ |
| |                  lazy-wasm                     | |
| | +--------------------------------------------+ | |
| | |              LazyWasmClient                | | |
| | +--------------------------------------------+ | |
| |      |         ^          |         ^         | |
| |      | GET     |          | WS      |         | |
| |      |(fetch)  | JSON     | (on_update) | Update  | |
| |      |         |          |         | Event   | |
| |      v         |          v         |         | |
| +------------------------------------------------+ |
+----------------------------------------------------+
           |                 |                 
           | HTTP            | WebSocket
           |                 |
+----------------------------------------------------+
|                     Dyna Server                    |
| +------------------------------------------------+ |
| |      /api/v1/...         | /api/v1/ws          | |
| +------------------------------------------------+ |
|                         |                          |
|                         |                          |
| +------------------------------------------------+ |
| |                 Storage (S3)                   | |
| +------------------------------------------------+ |
+----------------------------------------------------+
```

**Data Flow:**

1.  The web application instantiates `LazyWasmClient` and calls `connect()` to establish a WebSocket connection.
2.  To fetch a resource, the application calls `get()`, `list_resources()`, `get_all()`, or `for_each_all()`.
3.  `lazy-wasm` sends an HTTP GET request to the appropriate `dyna-server` REST endpoint.
4.  The server retrieves the requested resource(s) from the S3 storage backend, GZIP-decompresses them, and returns them as JSON.
5.  `lazy-wasm` deserializes the JSON response and returns it to the web application.
6.  When a resource is updated on the server (e.g., via a `dyna push`), the server sends an `UpdateEvent` notification over the WebSocket connection.
7.  `lazy-wasm` receives the `UpdateEvent` and invokes the callback registered with `on_update()`.

## Getting Started

To get started with `lazy-wasm`, you will need to have a Dyna server running. You can find instructions on how to set up a Dyna server in the [dyna-server](https://github.com/dyna-proj/dyna-server) repository.

Once you have a Dyna server running, you can install `lazy-wasm` in your project:

```bash
npm install @dyna/lazy-wasm
```

Then, you can import and use the `LazyWasmClient` in your JavaScript or TypeScript code:

```javascript
import init, { LazyWasmClient } from "@dyna/lazy-wasm";

async function main() {
  await init();

  const client = new LazyWasmClient("ws://localhost:8080/api/v1/ws");
  await client.connect();

  // ...
}

main();
```

## API Reference

The main entry point for `lazy-wasm` is the `LazyWasmClient` class. It provides the following methods:

| Method           | Parameters                                   | Return Type     | Description                                                                                                                              |
| ---------------- | -------------------------------------------- | --------------- | ---------------------------------------------------------------------------------------------------------------------------------------- |
| `new`            | `server_url: string`                         | `LazyWasmClient`| Creates a new `LazyWasmClient` instance. The URL should point to the WebSocket endpoint of the Dyna server.                                |
| `connect`        |                                              | `Promise<void>` | Establishes a WebSocket connection to the Dyna server. This must be called before any other methods.                                       |
| `get`            | `resource_id: string`                        | `Promise<any>`  | Retrieves a single resource by its ID. The resource is returned as a JavaScript object.                                                  |
| `list_resources` |                                              | `Promise<any>`  | Lists all available resources. The resources are returned as an array of strings, where each string is a resource ID.                      |
| `get_all`        |                                              | `Promise<any>`  | Retrieves all resources. The resources are returned as an array of JavaScript objects.                                                   |
| `for_each_all`   | `callback: (resource: any) => void`          | `Promise<void>` | Iterates over all resources and calls the callback for each one. This is useful for processing a large number of resources without loading them all into memory at once. |
| `on_update`      | `callback: (update: UpdateEvent) => void`    | `void`          | Registers a callback to be called when a resource is updated. The `UpdateEvent` object contains the resource ID and the new resource data. |

### UpdateEvent

The `UpdateEvent` object passed to the `on_update` callback has the following structure:

```typescript
interface UpdateEvent {
  resource_id: string;
  resource: any; // The updated JSON resource
}
```

## Technical Design

### Data Structures

`lazy-wasm` uses the following core data structures from `dyna-core`:

*   **Resource:** Represents a JSON document with its ID and content.
*   **PatchOperation:** Represents a single operation in a JSON Patch (RFC 6902).
*   **Patch:** A collection of `PatchOperation`s.
*   **Changeset:** A content-addressed group of patches.

### Concurrency Model

All `LazyWasmClient` methods are asynchronous and return JavaScript `Promise`s. This is achieved using `wasm-bindgen-futures` to convert Rust `Future`s into `Promise`s. The underlying HTTP requests and WebSocket communication are handled by the browser's event loop, ensuring that the UI remains responsive.

### Error Handling

Errors are propagated as rejected `Promise`s. The error objects are serialized from Rust `anyhow::Error` types into JavaScript `Error` objects, providing a stack trace and a descriptive error message.

### Compression

`lazy-wasm` leverages the `flate2` crate to handle GZIP compression and decompression, aligning with the Dyna ecosystem's standard for data transport and storage. All communication with the `dyna-server` is compressed, reducing network bandwidth usage.

### Virtual File System (VFS)

`lazy-wasm` utilizes an in-memory virtual file system (VFS) provided by the `vfs` crate. This allows it to manage resources and their metadata in a structured way, similar to a traditional file system, but without the need for actual file I/O in the browser. The VFS is used to cache resources, track their state, and provide a consistent interface for accessing them.

### State Management

`lazy-wasm` manages the state of the WebSocket connection and the registered `on_update` callback. The `LazyWasmClient` maintains a reference to the WebSocket and the callback function, ensuring that they are properly managed throughout the lifecycle of the client.

## Integration Examples

Here is a more detailed example of how to use `lazy-wasm` in a simple web application:

**index.html**

```html
<!DOCTYPE html>
<html>
<head>
  <meta charset="utf-8">
  <title>lazy-wasm Demo</title>
</head>
<body>
  <h1>lazy-wasm Demo</h1>
  <ul id="resource-list"></ul>
  <div id="error-message"></div>
  <script src="./bootstrap.js"></script>
</body>
</html>
```

**index.js**

```javascript
import init, { LazyWasmClient } from "./pkg/lazy_wasm.js";

async function main() {
  await init();

  const client = new LazyWasmClient("ws://localhost:8080/api/v1/ws");

  try {
    await client.connect();
    console.log("Connected to Dyna server");

    client.on_update(update => {
      console.log("Resource updated:", update.resource_id, update.resource);
      const resourceElement = document.getElementById(update.resource_id);
      if (resourceElement) {
        resourceElement.textContent = JSON.stringify(update.resource, null, 2);
      }
    });

    const resources = await client.list_resources();
    console.log("Available resources:", resources);

    const resourceList = document.getElementById("resource-list");
    for (const resourceId of resources) {
      const listItem = document.createElement("li");
      listItem.id = resourceId;
      listItem.textContent = resourceId;
      resourceList.appendChild(listItem);
    }

    if (resources.length > 0) {
      const resourceId = resources[0];
      const resource = await client.get(resourceId);
      console.log(`Got resource ${resourceId}:`, resource);
      const resourceElement = document.getElementById(resourceId);
      if (resourceElement) {
        resourceElement.textContent = JSON.stringify(resource, null, 2);
      }
    }

  } catch (error) {
    console.error("An error occurred:", error);
    const errorElement = document.getElementById("error-message");
    if (errorElement) {
      errorElement.textContent = error.message;
    }
  }
}

main();
```

## Building

### With Nix

To build `lazy-wasm` with Nix, simply run:

```bash
nix build
```

### Without Nix

To build `lazy-wasm` without Nix, you will need to have the Rust toolchain and `wasm-pack` installed.

1.  **Install Rust:** Follow the instructions on the [official Rust website](https://www.rust-lang.org/tools/install).
2.  **Install wasm-pack:**

    ```bash
    cargo install wasm-pack
    ```

3.  **Build the project:**

    ```bash
    wasm-pack build --target web
    ```

## Testing

To run the tests for `lazy-wasm`, you will need to have `wasm-bindgen-test` installed.

1.  **Install wasm-bindgen-test:**

    ```bash
    cargo install wasm-bindgen-test
    ```

2.  **Run the tests:**

    ```bash
    wasm-pack test --chrome --headless
    ```

## Dependencies

`lazy-wasm` has the following dependencies:

### Rust

*   `dyna-core`: Core data models and types for the Dyna ecosystem.
*   `serde`, `serde_json`: Serialization and deserialization of data.
*   `wasm-bindgen`, `wasm-bindgen-futures`: For interoperability between Rust and JavaScript.
*   `web-sys`: Raw bindings to Web APIs.
*   `js-sys`: Raw bindings to JavaScript's standard, built-in objects.
*   `console_error_panic_hook`: For logging panic messages to the browser console.
*   `getrandom`: For generating random numbers.
*   `chrono`: For handling dates and times.
*   `uuid`: For working with UUIDs.
*   `anyhow`: For flexible error handling.
*   `itertools`: For extra iterator adaptors, functions, and macros.
*   `vfs`: For an in-memory virtual file system.
*   `flate2`: For GZIP compression and decompression.

### JavaScript

*   A modern web browser with support for WebAssembly, `fetch`, and `WebSocket`.

## License

This project is licensed under the MIT License. See the [LICENSE.txt](LICENSE.txt) file for details.

## Contributing

Contributions are welcome! Please feel free to submit a pull request or open an issue.

## Code of Conduct

This project adheres to the [Contributor Covenant Code of Conduct](https://www.contributor-covenant.org/version/2/1/code_of_conduct/). By participating, you are expected to uphold this code.

## Related Projects

*   [dyna-core](https://github.com/dyna-proj/dyna-core): Core Rust library for the Dyna ecosystem.
*   [dyna-cli](https://github.com/dyna-proj/dyna-cli): Command-line interface for Dyna.
*   [dyna-server](https://github.com/dyna-proj/dyna-server): Dyna server implementation.
*   [dyna-wasm](https://github.com/dyna-proj/dyna-wasm): WebAssembly client for Dyna.
*   [dyna-py](https://github.com/dyna-proj/dyna-py): Python client for Dyna.
*   [dyna-go](https://github.com/dyna-proj/dyna-go): Go client for Dyna.
*   [dyna-app](https://github.com/dyna-proj/dyna-app): Elm web application for Dyna.
*   [lazy-cat](https://github.com/dyna-proj/lazy-cat): Lazy resource loader for Dyna.
*   [lazy-go](https://github.com/dyna-proj/lazy-go): Go lazy resource loader for Dyna.
*   [lazy-py](https://github.com/dyna-proj/lazy-py): Python lazy resource loader for Dyna.
*   [lazy-elm-demo](https://github.com/dyna-proj/lazy-elm-demo): Elm demo application for `lazy-wasm`.

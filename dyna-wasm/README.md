# dyna-wasm: Dyna Protocol in the Browser

A WebAssembly (WASM) client for the Dyna distributed CRUD system, enabling full-featured collaborative JSON editing directly in the browser.

---

## 1. Problem Statement

In the landscape of modern web development, building applications that support real-time collaboration and offline functionality presents a significant engineering challenge. Developers often find themselves reinventing the wheel, implementing complex logic for state synchronization, conflict resolution (CRDTs), and data versioning. The Dyna ecosystem was created to solve this problem by providing a generalized, Git-like version control system specifically for JSON data.

However, for Dyna to be truly effective for building rich, interactive web applications, it needs a first-class client that can run natively in the browser. This client must be lightweight, performant, and expose the full power of the Dyna protocol to JavaScript developers. `dyna-wasm` is the answer to this need. It compiles the core Rust implementation of Dyna into a highly optimized WebAssembly module, allowing developers to build sophisticated collaborative applications with a simple and elegant API, without the complexity of managing server-side connections or writing low-level synchronization code.

`dyna-wasm` addresses the critical gap between powerful backend data systems and the demands of modern, client-centric web applications. It empowers frontend developers to focus on building great user experiences, while `dyna-wasm` handles the complexities of distributed data management under the hood.

---

## 2. Intent and Goals

The design philosophy of `dyna-wasm` is centered around three core principles: **Power**, **Simplicity**, and **Performance**.

*   **Power:** Provide the complete feature set of the Dyna protocol. Nothing should be left out. This includes:
    *   Full repository lifecycle: `init`, `clone`.
    *   Changeset-based versioning: `add`, `commit`, `log`, `diff`.
    *   Seamless remote synchronization: `push`, `pull`.
    *   Branching and merging capabilities via `channel` management.
    *   Deep inspection and history queries.

*   **Simplicity:** Offer a high-level, ergonomic JavaScript API that feels natural to web developers. The complexity of WebAssembly, Rust, and the underlying Dyna protocol should be completely abstracted away. The developer experience is paramount.

*   **Performance:** Deliver a near-native performance experience. By leveraging Rust's zero-cost abstractions and compiling to WebAssembly, `dyna-wasm` can handle large-scale JSON documents and complex operational histories with minimal overhead. The goal is to be so fast that the user never perceives any lag related to data processing.

Our ultimate goal is to make `dyna-wasm` the de-facto standard for building collaborative, data-intensive applications on the web, enabling a new generation of real-time, offline-first user experiences.

---

## 3. Architecture

`dyna-wasm` is a pure client-side library that operates within the browser's sandbox. It communicates with a remote `dyna-server` for data persistence and synchronization.

### High-Level Component Diagram

This diagram illustrates the relationship between the consuming web application, `dyna-wasm`, and the browser environment.

```ascii
+-----------------------------------------------------------------+
| Browser Environment (e.g., Chrome, Firefox)                     |
|                                                                 |
|  +---------------------------+      +-------------------------+   |
|  |      Your Web App         |      |       dyna-wasm         |   |
|  | (React, Vue, Angular, etc.)| <--> |  (WebAssembly Module)   |   |
|  +---------------------------+      +-------------------------+   |
|       |           ^                 |           ^             |   |
|       v           |                 v           |             |   |
|  +---------------------------+      +-------------------------+   |
|  |   JS <-> WASM Bridge      |      |   Rust Implementation   |   |
|  |     (wasm-bindgen)        |      |      (dyna-core)        |   |
|  +---------------------------+      +-------------------------+   |
|       |           ^                 |           ^             |   |
|       v           |                 v           |             |   |
|  +---------------------------+      +-------------------------+   |
|  |    Browser APIs           |      |    In-Memory VFS        |   |
|  | (fetch, WebSocket)        |      |      (vfs crate)        |   |
|  +---------------------------+      +-------------------------+   |
|                                                                 |
+-----------------------------------------------------------------+
           |                                      ^
           | (HTTP/WebSocket)                     | (Data)
           v                                      |
+-----------------------------------------------------------------+
| Remote Environment (Cloud or On-Premise)                        |
|                                                                 |
|  +---------------------------+
|  |      dyna-server          |
|  |   (REST API / WebSocket)  |
|  +---------------------------+
|                                                                 |
+-----------------------------------------------------------------+
```

### Detailed Data Flow

1.  **Initialization:** The web application imports the `dyna-wasm` package and calls the `init()` function, which loads and prepares the WASM module.
2.  **Client Instantiation:** A `DynaClient` is created with the URL of the remote `dyna-server`.
3.  **User Action:** The user performs an action in the web app (e.g., edits a document).
4.  **API Call:** The web app translates this action into a `dyna-wasm` API call, such as `client.add()` followed by `client.commit()`.
5.  **WASM Bridge:** The `wasm-bindgen` layer marshals the JavaScript arguments (e.g., strings, numbers) into Rust-compatible types.
6.  **Core Logic Execution:** The call is forwarded to the underlying Rust function within the WASM module. This function, using the `dyna-core` library, performs the requested operation. For example, a `commit` operation involves:
    a. Reading the staged files from the in-memory VFS.
    b. Generating JSON patches (RFC 6902) representing the changes.
    c. Bundling the patches into a `Changeset` object.
    d. Calculating the content-addressed hash of the changeset.
    e. Saving the new changeset object to the in-memory VFS.
7.  **Network Operation (if any):** If the API call was `push`, `pull`, or `clone`, the Rust code uses the `web-sys` crate to make a `fetch` request to the `dyna-server`'s REST API. The request body contains the serialized protocol data (e.g., a `PushRequest`).
8.  **Server Interaction:** The `dyna-server` processes the request, updates its own storage (e.g., an S3 bucket), and sends a response.
9.  **Response Handling:** The `dyna-wasm` client receives the HTTP response, deserializes it, and updates its internal state and in-memory VFS accordingly.
10. **Return to JavaScript:** The result of the operation (or an error) is passed back across the WASM bridge to the JavaScript environment, resolving or rejecting the `Promise` that was initially returned.

---

## 4. API Reference

The public API of `dyna-wasm` is exposed through the `DynaClient` class. All asynchronous methods return a `Promise`.

### `DynaClient`

**Constructor:** `new DynaClient(remote_url: string)`

| Method                  | Parameters                               | Return Type     | Description                                                                 |
| ----------------------- | ---------------------------------------- | --------------- | --------------------------------------------------------------------------- |
| `init()`                | -                                        | `Promise<void>` | Initializes a new, empty repository in the in-memory file system.           |
| `clone()`               | -                                        | `Promise<void>` | Clones a repository from the remote URL, populating the in-memory VFS.      |
| `add(path: string)`     | `path`: The path to the resource to stage. | `Promise<void>` | Stages a resource for the next commit. The path is relative to the repo root. |
| `commit(message: string)` | `message`: The commit message.           | `Promise<string>` | Creates a new changeset from staged files, returning the changeset ID.      |
| `push()`                | -                                        | `Promise<void>` | Pushes local changesets to the remote repository's current channel.         |
| `pull()`                | -                                        | `Promise<void>` | Fetches and applies changesets from the remote repository's current channel.|
| `status()`              | -                                        | `Promise<string>` | Returns a formatted string detailing new, modified, and staged resources.   |
| `log()`                 | -                                        | `Promise<string>` | Returns a formatted string of the commit history for the current channel.   |
| `channel(subcommand: string, name?: string)` | `subcommand`: 'create', 'delete', 'list', or 'switch'. `name`: The channel name. | `Promise<string>` | Manages channels (branches). `list` returns a list of channels.           |
| `history(resource_id: string)` | `resource_id`: The ID of the resource (e.g., 'acme.user.123'). | `Promise<string>` | Shows the changeset history for a specific resource.                        |
| `diff(changeset_id?: string)` | `changeset_id`: The ID of the changeset to diff. If omitted, shows uncommitted changes. | `Promise<string>` | Shows the differences between the working directory and a changeset.        |
| `revert(changeset_id: string)` | `changeset_id`: The ID of the changeset to revert. | `Promise<void>` | Reverts the changes from a specific changeset.                              |

---

## 5. Technical Design

This section delves into the internal implementation details of `dyna-wasm`.

*   **Core Logic (`dyna-core`):** The heart of `dyna-wasm` is the `dyna-core` Rust crate. This crate is shared across all Dyna projects and provides the canonical implementation of the data model and protocols. Key components include:
    *   **Data Structures:** `Resource`, `PatchOperation`, `Patch`, `Changeset`, `Channel`, `SyncState`.
    *   **Hashing:** Content-addressable storage using SHA-256 for changesets.
    *   **Diff Engine:** An implementation of RFC 6902 for generating and applying JSON patches.
    *   **Compression:** All data is compressed using `gzip` (via the `flate2` crate) before being stored or transmitted.

*   **WASM Binding (`wasm-bindgen`):** We use `wasm-bindgen` to generate the JavaScript and TypeScript interface for our Rust code. This tool automatically handles the complex marshalling of data types across the WASM boundary. We make extensive use of `serde-wasm-bindgen` to seamlessly convert our Rust structs to and from JavaScript objects.

*   **Virtual File System (`vfs`):** Since there is no persistent filesystem in the browser, `dyna-wasm` uses the `vfs` crate to create an in-memory filesystem. This allows the `dyna-core` logic, which is written to expect a filesystem, to operate unmodified. The VFS is essentially a `HashMap<String, Vec<u8>>` that maps file paths to their byte contents.

*   **Asynchronous Operations (`wasm-bindgen-futures`):** All potentially long-running operations, especially network requests, are implemented as `async` functions in Rust. The `wasm-bindgen-futures` crate converts these Rust `Future`s into JavaScript `Promise`s, ensuring that the browser's main thread is never blocked.

*   **Error Handling:** We use the `anyhow` crate for flexible error handling in Rust. When a Rust function returns an error, `wasm-bindgen` automatically converts it into a JavaScript `Error` object and rejects the corresponding `Promise`. This provides a natural and idiomatic way for JavaScript code to handle errors using `try...catch` blocks.

---

## 6. Integration Examples

Below are several examples demonstrating how to integrate `dyna-wasm` into a typical web application.

### Basic Setup (HTML and Vanilla JS)

```html
<!DOCTYPE html>
<html>
<head>
  <title>Dyna-WASM Demo</title>
</head>
<body>
  <h1>Dyna-WASM Demo</h1>
  <script type="module">
    import init, { DynaClient } from './pkg/dyna_wasm.js';

    async function main() {
      await init();
      const client = new DynaClient('http://localhost:8080/api/v1');
      console.log('DynaClient initialized!');
      // You can now use the client object
      await client.init();
      console.log('Repository initialized in memory.');
    }

    main();
  </script>
</body>
</html>
```

### React Hook Example

This example shows how you might create a React hook to manage a Dyna resource.

```javascript
import { useState, useEffect, useCallback } from 'react';
import init, { DynaClient } from 'dyna-wasm';

// Initialize the client once
const clientPromise = init().then(() => new DynaClient('http://localhost:8080/api/v1'));

export function useDynaResource(resourceId) {
  const [resource, setResource] = useState(null);
  const [loading, setLoading] = useState(true);

  useEffect(() => {
    let isMounted = true;
    async function fetchResource() {
      const client = await clientPromise;
      // This is a simplified example; a real implementation would read from the VFS
      // For now, we assume a function `getResource` exists
      // const data = await client.getResource(resourceId);
      // if (isMounted) {
      //   setResource(data);
      //   setLoading(false);
      // }
    }
    fetchResource();
    return () => { isMounted = false; };
  }, [resourceId]);

  const updateResource = useCallback(async (newContent) => {
    const client = await clientPromise;
    const resourceToSave = { id: resourceId, ...newContent };
    await client.add(JSON.stringify(resourceToSave));
    const changesetId = await client.commit(`Update ${resourceId}`);
    await client.push();
    // In a real app, you'd likely re-fetch or update the state
    // based on a WebSocket event.
    setResource(resourceToSave);
  }, [resourceId]);

  return { resource, loading, updateResource };
}
```

---

## 7. Building

You can build `dyna-wasm` using either Nix for a reproducible build environment or manually with the Rust toolchain.

### With Nix

If you have [Nix](https://nixos.org/) installed with Flakes enabled, you can build the project with a single command:

```bash
# Build the package and create a `result` symlink
nix build

# Or, enter a development shell with all dependencies
nix develop
```

This is the recommended approach as it guarantees that you are using the exact same dependencies and toolchain versions as the original developers.

### Without Nix

If you do not have Nix, you can build the project manually. You will need:

*   The Rust toolchain ([rustup.rs](https://rustup.rs/))
*   The `wasm32-unknown-unknown` target
*   `wasm-pack`

1.  **Install Rust and the WASM target:**

    ```bash
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
    rustup target add wasm32-unknown-unknown
    ```

2.  **Install `wasm-pack`:**

    ```bash
    cargo install wasm-pack
    ```

3.  **Build the project:**

    ```bash
    wasm-pack build --target web
    ```

This command will compile the Rust code to WebAssembly, generate the JavaScript bindings, and output everything to a `pkg` directory, ready to be used in a web project.

---

## 8. Testing

`dyna-wasm` includes a comprehensive test suite written with `wasm-bindgen-test`.

To run the tests, you need to have a WebDriver-compatible browser installed (e.g., Firefox or Chrome).

```bash
# Install the necessary test runner
cargo install wasm-bindgen-cli

# Run tests in headless Firefox (requires geckodriver)
wasm-pack test --headless --firefox

# Run tests in headless Chrome (requires chromedriver)
wasm-pack test --headless --chrome
```

This will execute the tests in a real browser environment, ensuring that the integration with browser APIs works as expected.

---

## 9. Related Projects

`dyna-wasm` is part of the larger Dyna ecosystem. Here are some other related projects:

*   **[dyna-core](/../dyna-core):** The core Rust library providing data structures and logic for the Dyna protocol.
*   **[dyna-cli](/../dyna-cli):** A command-line interface for managing Dyna repositories.
*   **[dyna-server](/../dyna-server):** The backend server that stores and synchronizes Dyna data.
*   **[dyna-py](/../dyna-py):** Python bindings for Dyna, for scripting and server-side applications.
*   **[dyna-go](/../dyna-go):** A Go client library for Dyna.
*   **[dyna-app](/../dyna-app):** A complete web-based UI for Dyna, built with Elm and `dyna-wasm`.
*   **[lazy-cat](/../lazy-cat):** A lazy, on-demand resource loader for Dyna.
*   **[lazy-go](/../lazy-go):** A Go lazy resource loader for Dyna.
*   **[lazy-py](/../lazy-py):** A Python async lazy resource loader for Dyna.
*   **[lazy-wasm](/../lazy-wasm):** A WASM lazy resource loader for Dyna.
*   **[lazy-elm-demo](/../lazy-elm-demo):** An Elm demo application for `lazy-wasm`.

---

## 10. License

This project is licensed under the MIT License. See the `LICENSE` file for details.

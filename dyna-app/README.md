# dyna-app: A Web UI for the Dyna Ecosystem

A full-featured, browser-based user interface for the Dyna distributed CRUD system, built with Elm and WebAssembly.

## 1. Problem Statement

The Dyna ecosystem provides a powerful but low-level set of tools for distributed JSON editing. While the `dyna-cli` offers a comprehensive command-line interface, and the `dyna-server` exposes a REST and WebSocket API, there was no intuitive, visual way to interact with Dyna resources, channels, and changesets. This made it challenging for users to visualize the state of their data, manage conflicts, and explore the history of their resources. `dyna-app` was created to fill this gap by providing a rich, interactive web-based UI that makes the power of Dyna accessible to a broader audience.

## 2. Intent and Goals

The primary goal of `dyna-app` is to provide a user-friendly, graphical interface for the Dyna version control system. It is designed to be a complete client-side application that runs entirely in the browser, leveraging the power of `dyna-wasm` to perform all core Dyna operations locally. The design philosophy is centered around clarity, ease of use, and providing a visual representation of the underlying Dyna concepts.

Specifically, `dyna-app` aims to:

*   **Visualize Data:** Provide a clear and intuitive way to browse, view, and edit JSON resources.
*   **Manage Channels:** Enable users to create, switch between, and delete channels (similar to git branches).
*   **Inspect History:** Offer a visual log of changesets for any given resource.
*   **Resolve Conflicts:** Provide a user-friendly interface for viewing and resolving merge conflicts.
*   **Be Performant:** Deliver a fast and responsive user experience by performing all heavy lifting in a WebAssembly module.

## 3. Architecture

`dyna-app` is a single-page application (SPA) built with the Elm programming language. It communicates with a WebAssembly module (`dyna-wasm`) through Elm's port mechanism. The `dyna-wasm` module, in turn, handles all the logic for interacting with a remote `dyna-server`.

Here is an ASCII art diagram illustrating the architecture:

```
+---------------------------------------------------------------------+
| Browser                                                             |
|                                                                     |
| +--------------------------+      +--------------------------------+ |
| |                          |      |                                | |
| |       dyna-app (Elm)     |----->|      dyna-wasm (WebAssembly)   | |
| |                          |      |                                | |
| |  - UI Components         |      |  - In-memory VFS               | |
| |  - State Management      |      |  - Dyna Core Logic             | |
| |  - Port Communication    |      |  - HTTP/WebSocket Client       | |
| |                          |      |                                | |
| +--------------------------+      +--------------------------------+ |
|                                      |
|                                      |
|                                      v
| +---------------------------------------------------------------------+ |
| | dyna-server (Rust)                                                  | |
| |                                                                     | |
| | - REST API (/api/v1)                                                | |
| | - WebSocket API (/api/v1/ws)                                        | |
| | - S3 Storage Backend                                                | |
| +---------------------------------------------------------------------+ |
|                                                                     |
+---------------------------------------------------------------------+
```

**Data Flow:**

1.  The user interacts with the `dyna-app` UI (e.g., clicks a button to save a change).
2.  The Elm application sends a command to the `dyna-wasm` module via a JavaScript port.
3.  The `dyna-wasm` module receives the command and executes the corresponding Dyna operation (e.g., creating a patch, committing a changeset).
4.  `dyna-wasm` communicates with the remote `dyna-server` over HTTP or WebSockets to push or pull changes.
5.  The `dyna-server` processes the request and responds.
6.  `dyna-wasm` receives the response and sends an event back to the Elm application via another port.
7.  The Elm application updates its state and the UI reflects the changes.

## 4. API Reference

`dyna-app` communicates with the `dyna-wasm` module through a set of ports. The following tables document the commands sent from Elm to wasm and the events received from wasm.

### Commands (Elm -> Wasm)

| Command             | Parameters                               | Description                                                                 |
| ------------------- | ---------------------------------------- | --------------------------------------------------------------------------- |
| `clone`             | `url: String`                            | Clones a remote Dyna repository.                                            |
| `pull`              | `()`                                     | Pulls the latest changes from the remote repository.                        |
| `push`              | `()`                                     | Pushes local changes to the remote repository.                              |
| `getResource`       | `id: String`                             | Retrieves a JSON resource by its ID.                                        |
| `saveResource`      | `id: String, content: Json.Encode.Value` | Saves a JSON resource. This will create a new changeset.                    |
| `getChannels`       | `()`                                     | Gets a list of all channels.                                                |
| `createChannel`     | `name: String`                           | Creates a new channel.                                                      |
| `deleteChannel`     | `name: String`                           | Deletes a channel.                                                          |
| `switchChannel`     | `name: String`                           | Switches to a different channel.                                            |
| `getResourceHistory`| `id: String`                             | Retrieves the history of changesets for a resource.                         |
| `getChangeset`      | `id: String`                             | Retrieves a specific changeset by its ID.                                   |

### Events (Wasm -> Elm)

| Event               | Payload                                  | Description                                                                 |
| ------------------- | ---------------------------------------- | --------------------------------------------------------------------------- |
| `cloneSuccess`      | `()`                                     | Indicates that the repository was successfully cloned.                      |
| `pullSuccess`       | `()`                                     | Indicates that the latest changes were successfully pulled.                 |
| `pushSuccess`       | `()`                                     | Indicates that the local changes were successfully pushed.                  |
| `resource`          | `id: String, content: Json.Decode.Value` | The content of a requested resource.                                        |
| `channels`          | `List String`                            | A list of all channels.                                                     |
| `resourceHistory`   | `List ChangesetInfo`                     | The history of a resource.                                                  |
| `changeset`         | `Changeset`                              | The content of a requested changeset.                                       |
| `error`             | `String`                                 | An error message.                                                           |

## 5. Technical Design

`dyna-app` is built using Elm 0.19, a functional programming language that compiles to JavaScript. It follows the "The Elm Architecture" (Model-View-Update) for managing application state.

*   **Model:** The state of the application is stored in a single, immutable data structure.
*   **View:** The UI is a pure function of the model. It renders the current state as HTML.
*   **Update:** User actions and events from the `dyna-wasm` module are handled by an `update` function, which takes the current model and a message and returns a new model.

### Concurrency

All asynchronous operations are handled by the `dyna-wasm` module. The Elm application remains single-threaded and communicates with the wasm module through ports. This keeps the Elm code simple and easy to reason about, while still allowing for non-blocking I/O.

### Error Handling

Errors from the `dyna-wasm` module are sent to the Elm application as `error` events. The Elm application can then display these errors to the user in a user-friendly way.

### Compression

All data exchanged between `dyna-wasm` and the `dyna-server` is gzip-compressed, ensuring efficient use of network bandwidth.

## 6. Integration Examples

Here are some examples of how to use `dyna-app` in your Elm code.

### Getting a Resource

```elm
import Json.Encode as Encode

-- Define the port to send commands to wasm
port commands : Encode.Value -> Cmd msg

-- A function to get a resource
getResource : String -> Cmd msg
getResource id =
    Encode.object
        [ ( "command", Encode.string "getResource" )
        , ( "id", Encode.string id )
        ]
        |> commands
```

### Subscribing to Resource Updates

```elm
import Json.Decode as Decode

-- Define the port to receive events from wasm
port events : (Decode.Value -> msg) -> Sub msg

-- A message to represent a resource update
type Msg
    = ResourceUpdated String Decode.Value

-- A subscription to listen for resource updates
subscriptions : Model -> Sub Msg
subscriptions model =
    events <| Decode.field "event" Decode.string |> Decode.andThen (\event ->
        case event of
            "resource" ->
                Decode.map2 ResourceUpdated
                    (Decode.field "id" Decode.string)
                    (Decode.field "content" Decode.value)

            _ ->
                Decode.fail "Unknown event"
    )
```

## 7. Building

### With Nix

If you have Nix installed, you can build the project with a single command:

```bash
nix build
```

This will build the Elm application and the `dyna-wasm` module and place the resulting artifacts in the `result` directory.

### Without Nix

If you don't have Nix, you can build the project manually:

1.  **Build the `dyna-wasm` module:**

    ```bash
    cd ../dyna-wasm
    cargo build --target wasm32-unknown-unknown --release
    wasm-bindgen --out-dir ../dyna-app/public/wasm --target web target/wasm32-unknown-unknown/release/dyna_wasm.wasm
    ```

2.  **Build the Elm application:**

    ```bash
    cd ../dyna-app
    elm make src/Main.elm --output=public/main.js
    ```

## 8. Testing

To run the tests for `dyna-app`, you can use `elm-test`:

```bash
elm-test
```

This will run all the tests in the `tests` directory.

## 9. Related Projects

*   [dyna-core](https://github.com/example/dyna-core): The core Rust library for the Dyna ecosystem.
*   [dyna-cli](https://github.com/example/dyna-cli): A command-line interface for Dyna.
*   [dyna-server](https://github.com/example/dyna-server): The Dyna server.
*   [dyna-wasm](https://github.com/example/dyna-wasm): The WebAssembly module used by `dyna-app`.
*   [dyna-py](https://github.com/example/dyna-py): Python bindings for Dyna.
*   [dyna-go](https://github.com/example/dyna-go): Go client library for Dyna.
*   [lazy-cat](https://github.com/example/lazy-cat): A lazy resource loader for Dyna.
*   [lazy-go](https://github.com/example/lazy-go): A Go lazy resource loader for Dyna.
*   [lazy-py](https://github.com/example/lazy-py): A Python lazy resource loader for Dyna.
*   [lazy-wasm](https://github.com/example/lazy-wasm): A WebAssembly lazy resource loader for Dyna.
*   [lazy-elm-demo](https://github.com/example/lazy-elm-demo): A demo application for `lazy-wasm`.

## 10. Features

`dyna-app` provides a comprehensive set of features for interacting with the Dyna ecosystem:

*   **Resource Explorer:** A tree-based view of all resources in the repository, allowing you to easily navigate and explore your data.
*   **JSON Editor:** A built-in editor for viewing and modifying JSON resources. The editor provides syntax highlighting and validation.
*   **Diff Viewer:** A side-by-side diff viewer that highlights the changes between two versions of a resource. This is particularly useful for understanding the impact of a changeset or for resolving merge conflicts.
*   **Channel Management:** A simple interface for creating, deleting, and switching between channels. This allows you to work on different features or versions of your data in parallel.
*   **History Viewer:** A chronological log of all changesets for a given resource. You can view the details of each changeset, including the author, timestamp, and the patches that were applied.
*   **Conflict Resolution:** A guided workflow for resolving merge conflicts. `dyna-app` will automatically detect conflicts and present you with a user-friendly interface for choosing the desired changes.
*   **Real-time Updates:** `dyna-app` uses WebSockets to receive real-time updates from the `dyna-server`. This means that any changes made by other collaborators will be reflected in the UI immediately.

## 11. Under the Hood

`dyna-app` is a testament to the power of combining Elm and WebAssembly. Here's a closer look at how these technologies work together to create a seamless user experience.

### The Elm Architecture

The Elm Architecture (TEA) is a simple yet powerful pattern for building web applications. It is based on the idea of a unidirectional data flow, which makes applications easier to reason about and debug. In `dyna-app`, the entire state of the application is stored in a single `Model`. The `view` function takes this model and returns a description of the UI. The `update` function is responsible for handling messages (e.g., user input, events from the wasm module) and updating the model accordingly.

This architecture allows for a clear separation of concerns and makes it easy to manage the complexity of the application. It also enables powerful features like time-travel debugging, which allows you to step back and forth through the history of the application's state.

### WebAssembly Integration

The `dyna-wasm` module is the powerhouse of `dyna-app`. It is written in Rust and compiled to WebAssembly, which allows it to run in the browser at near-native speed. The wasm module is responsible for all the heavy lifting, including:

*   **Dyna Core Logic:** All the core data structures and algorithms of the Dyna ecosystem are implemented in the wasm module. This includes the diff engine, the patch engine, and the changeset-centric version control model.
*   **In-memory VFS:** The wasm module uses an in-memory virtual file system (VFS) to store the Dyna repository. This allows for fast and efficient access to resources and changesets.
*   **HTTP/WebSocket Client:** The wasm module includes a client for communicating with the `dyna-server` over HTTP and WebSockets. This allows `dyna-app` to push and pull changes, and to receive real-time updates.

### Port Communication

Elm and WebAssembly communicate with each other through a system of ports. Ports are a way for Elm to send and receive messages to and from JavaScript. In `dyna-app`, we use ports to send commands from the Elm application to the wasm module, and to receive events from the wasm module back in Elm.

This loose coupling between Elm and wasm allows us to develop and test each part of the application independently. It also makes it easy to swap out the wasm module for a different implementation in the future, without having to change the Elm code.

## 12. Future Work

`dyna-app` is under active development, and there are many exciting features planned for the future, including:

*   **Improved Conflict Resolution:** A more advanced conflict resolution interface that provides more context and makes it easier to resolve complex conflicts.
*   **Offline Support:** The ability to use `dyna-app` even when you are not connected to the internet. Changes will be stored locally and synced with the server when you are back online.
*   **Plugin System:** A plugin system that will allow developers to extend the functionality of `dyna-app` with custom views and editors.
*   **Improved Performance:** Further optimizations to the wasm module and the Elm application to make `dyna-app` even faster and more responsive.

## 13. Data Model

The core of the Dyna ecosystem is its data model, which is designed to be simple, flexible, and efficient. The following are the key data structures used in `dyna-app` and throughout the Dyna ecosystem:

*   **Resource:** A JSON document that is identified by a unique, dot-separated ID (e.g., `acme.entity.User`). Resources are the primary unit of data in Dyna.

*   **Patch:** A JSON Patch (RFC 6902) operation that describes a change to a resource. Patches are used to modify resources in a fine-grained and efficient manner.

*   **Changeset:** A group of patches that are applied to a resource as a single, atomic unit. Changesets are content-addressed, meaning that their ID is a hash of their content. This makes them immutable and easy to verify.

*   **Channel:** A named bookmark that points to a specific changeset. Channels are similar to git branches and are used to manage different versions of a resource.

*   **SyncState:** A data structure that keeps track of the state of a local repository relative to a remote repository. It is used to determine which changesets need to be pushed or pulled.

*   **Conflict:** A data structure that represents a merge conflict between two changesets. It contains information about the conflicting changes and provides a way to resolve the conflict.

This data model is implemented in the `dyna-core` Rust library and is used by all the other projects in the Dyna ecosystem. The use of a common data model ensures that all the different parts of the ecosystem can work together seamlessly.

## 14. Getting Involved

We welcome contributions from the community! If you are interested in getting involved with the development of `dyna-app` or any of the other projects in the Dyna ecosystem, please check out our GitHub repositories. You can also join our community on Discord to chat with other developers and get help with any questions you may have.

### Reporting Bugs

If you find a bug in `dyna-app`, please open an issue on our GitHub repository. Please include as much detail as possible, including steps to reproduce the bug, the expected behavior, and the actual behavior. This will help us to fix the bug as quickly as possible.

### Suggesting Enhancements

If you have an idea for a new feature or an enhancement to an existing feature, please open an issue on our GitHub repository. We are always looking for ways to improve `dyna-app` and make it more useful for our users.

### Submitting Pull Requests

If you would like to contribute code to `dyna-app`, please submit a pull request on our GitHub repository. Please make sure that your code follows our coding style guidelines and that all tests pass before submitting your pull request. We will review your pull request as soon as possible and provide feedback.

## 15. License

`dyna-app` is licensed under the MIT License. See the [LICENSE](LICENSE) file for more details.

# lazy-elm-demo: A Demonstration of the `lazy-wasm` Library

A simple yet comprehensive Elm 0.19 application that demonstrates the real-world usage of the `lazy-wasm` library for building interactive web user interfaces that communicate with a Dyna server.

This project serves as a canonical example for developers looking to integrate a functional, reactive frontend with the powerful, changeset-centric backend of the Dyna ecosystem.

---

## 1. Problem Statement

The Dyna ecosystem offers a powerful, changeset-centric approach to distributed JSON data management. While the core libraries (`dyna-core`, `dyna-server`) and client libraries (`dyna-cli`, `dyna-py`, `dyna-go`) provide the foundational pieces, building a responsive and real-time user interface that consumes data from this ecosystem presents a unique set of challenges. Developers new to the Dyna paradigm need a practical, well-documented example of how to build a web-based frontend that can efficiently connect to a Dyna server, fetch data, and subscribe to live updates. This project, `lazy-elm-demo`, exists to solve this problem by providing a clear and canonical example.

This project aims to bridge the gap between the powerful backend capabilities of Dyna and the modern, functional, and reactive frontend development experience offered by Elm. By providing a clear and concise example, we hope to lower the barrier to entry for developers who want to build rich, collaborative applications on top of the Dyna platform.

The key challenges that this project addresses are:

*   **Asynchronous Communication:** How to manage the asynchronous nature of WebSocket communication in a functional and stateless manner.
*   **State Management:** How to effectively manage the state of the application, including the connection status, resource lists, and real-time updates.
*   **Data Decoding:** How to safely decode JSON data received from the server into Elm data structures.
*   **Integration with WebAssembly:** How to seamlessly integrate an Elm application with a WebAssembly module (`lazy-wasm`) using ports.

---

## 2. Intent and Goals

The design philosophy behind `lazy-elm-demo` is to serve as a living blueprint for frontend development in the Dyna ecosystem. It is intentionally kept simple to focus on the core integration patterns, without being cluttered by complex UI components or business logic. The primary goals are:

*   **To Demonstrate Core Functionality:** To showcase the essential features of the `lazy-wasm` client, including connecting to a server, listing available resources, fetching individual resources, and streaming live updates.
*   **To Provide a Clear Architectural Pattern:** To present a clean and understandable Elm application structure that effectively isolates the `lazy-wasm` communication layer from the UI components.
*   **To Serve as a Starting Point:** To offer a solid foundation that developers can extend and adapt for their own projects, reducing the initial learning curve and development time.
*   **To Promote Best Practices:** To subtly guide developers towards effective patterns for handling asynchronous data and real-time updates in a functional and reactive UI framework like Elm.
*   **To Be a Teaching Tool:** To serve as a practical learning resource for developers who are new to Elm, WebAssembly, or the Dyna ecosystem.

---

## 3. Getting Started

To get the `lazy-elm-demo` application up and running, you will need to have the following prerequisites installed:

*   [Elm 0.19.1](https://guide.elm-lang.org/install/elm.html)
*   [A running `dyna-server` instance](https://github.com/dyna-proj/dyna-server)

Once you have the prerequisites installed, you can follow these steps:

1.  **Clone the repository:**

    ```bash
    git clone https://github.com/dyna-proj/lazy-elm-demo.git
    cd lazy-elm-demo
    ```

2.  **Install the Elm dependencies:**

    ```bash
    elm install
    ```

3.  **Compile the Elm code:**

    ```bash
    elm make src/Main.elm --output=main.js
    ```

4.  **Create an `index.html` file:**

    ```html
    <!DOCTYPE html>
    <html>
    <head>
      <meta charset="utf-8">
      <title>lazy-elm-demo</title>
    </head>
    <body>
      <div id="app"></div>
      <script src="main.js"></script>
      <script>
        var app = Elm.Main.init({ node: document.getElementById("app") });
      </script>
    </body>
    </html>
    ```

5.  **Start a local web server:**

    You can use any simple web server to serve the `index.html` and `main.js` files. For example, you can use Python's built-in HTTP server:

    ```bash
    python -m http.server
    ```

6.  **Open the application in your browser:**

    Open your web browser and navigate to `http://localhost:8000`.

---

## 4. Architecture

The `lazy-elm-demo` application follows a simple, three-tiered architecture that is typical for web applications in the Dyna ecosystem. The components and their interactions are illustrated in the diagram below.

```ascii
+---------------------------------------------------------------------+
|                                                                     |
|                          Browser Environment                        |
|                                                                     |
+---------------------------------------------------------------------+
|                                                                     |
|    +-----------------+       +------------------+       +-------------+   |
|    | lazy-elm-demo   |       | lazy-wasm        |       | dyna-server |   |
|    | (Elm UI)        | <---> | (WebAssembly)    | <---> | (Rust)      |   |
|    +-----------------+       +------------------+       +-------------+   |
|            |                       |                        |           |
|            | (Elm Ports)           | (WebSocket)            |           |
|            +-----------------------+                        |           |
|                                                             |           |
|    +--------------------------------------------------------+           |
|    |                                                                    |
|    |  1. User interacts with the UI (e.g., clicks a button)             |
|    |                                                                    |
|    |  2. Elm app sends a command via a port to the `lazy-wasm` module   |
|    |                                                                    |
|    |  3. `lazy-wasm` sends a request to the `dyna-server` via WebSocket |
|    |                                                                    |
|    |  4. `dyna-server` processes the request and sends a response back  |
|    |                                                                    |
|    |  5. `lazy-wasm` receives the response and sends it to the Elm app  |
|    |     via a subscription port                                        |
|    |                                                                    |
|    |  6. Elm app updates its model and the view is re-rendered          |
|    |                                                                    |
|    +--------------------------------------------------------------------+           |
|                                                                     |
+---------------------------------------------------------------------+
```

**Data Flow:**

1.  The user interacts with the Elm UI, triggering an event (e.g., clicking the "Connect" button).
2.  The Elm application sends a command through a port to the `lazy-wasm` client.
3.  The `lazy-wasm` client, running as a WebAssembly module, establishes a WebSocket connection with the `dyna-server` and sends the appropriate request.
4.  The `dyna-server` processes the request and sends a response back to the `lazy-wasm` client.
5.  The `lazy-wasm` client receives the response and sends it back to the Elm application through a subscription port.
6.  The Elm application receives the data, updates its model, and the view is re-rendered to reflect the new state.

---

## 5. API Reference

The Elm application communicates with the `lazy-wasm` client via a set of ports. This interface is the primary API for the `lazy-elm-demo`.

### Outgoing Ports (Commands)

| Port Name      | Type                | Description                                                                 |
| :------------- | :------------------ | :-------------------------------------------------------------------------- |
| `connect`      | `String -> Cmd msg` | Establishes a WebSocket connection to the Dyna server at the specified URL. |
| `getResources` | `() -> Cmd msg`     | Fetches a list of all available resource IDs from the server.               |
| `streamAll`    | `() -> Cmd msg`     | Subscribes to real-time updates for all resources on the server.            |

### Incoming Ports (Subscriptions)

| Port Name             | Type                           | Description                                                                                             |
| :-------------------- | :----------------------------- | :------------------------------------------------------------------------------------------------------ |
| `receiveResourceList` | `(List String -> msg) -> Sub msg` | Receives a list of resource IDs from the server after a `getResources` command.                         |
| `receiveResource`     | `(D.Value -> msg) -> Sub msg`    | Receives the full JSON content of a single resource.                                                    |
| `receiveUpdate`       | `(D.Value -> msg) -> Sub msg`    | Receives a live update event from the server when a resource is created, updated, or deleted.         |

---

## 6. Technical Design

The `lazy-elm-demo` application is implemented using the standard Elm Architecture (Model-View-Update), which provides a robust and maintainable structure for building web applications.

### Data Structures

The core of the application's state is managed by the `Model` record:

```elm
type alias Model =
    { serverUrl : String
    , connected : Bool
    , resources : List String
    , selectedResource : Maybe D.Value
    , updates : List D.Value
    }
```

*   `serverUrl`: Stores the WebSocket URL of the `dyna-server`.
*   `connected`: A boolean flag that indicates the status of the WebSocket connection.
*   `resources`: A list of strings, where each string is a resource ID (e.g., `"acme.entity.User"`).
*   `selectedResource`: An optional `Json.Decode.Value` that holds the content of the resource currently being viewed.
*   `updates`: A list of `Json.Decode.Value` objects, representing the stream of live update events from the server.

### Concurrency Model

Elm's runtime manages all concurrency through a system of commands and subscriptions. All interactions with the `lazy-wasm` library are asynchronous and non-blocking. When the Elm application sends a command to a port, it does not wait for a response. Instead, it continues to process other events. When the `lazy-wasm` library has data to send back to the Elm application, it does so through a subscription, which the Elm runtime delivers to the application as a message.

This model of concurrency is a key feature of Elm and is what makes it so well-suited for building responsive and reliable user interfaces.

### Error Handling

For the sake of simplicity, this demonstration application includes minimal error handling. In a production environment, it would be crucial to expand upon this by:

*   Handling WebSocket connection errors.
*   Validating the JSON data received from the server.
*   Providing user-friendly feedback when an operation fails.

Here is an example of how you might extend the `Msg` type to handle connection errors:

```elm
type Msg
    = UpdateServerUrl String
    | Connect
    | ConnectionSuccess
    | ConnectionFailed String
    | GetResources
    | StreamAll
    | ReceiveResourceList (List String)
    | ReceiveResource D.Value
    | ReceiveUpdate D.Value
```

---

## 7. Integration Examples

Here is a complete, runnable example of how to use the `lazy-elm-demo` application. This assumes you have a `dyna-server` instance running on `ws://127.0.0.1:9000`.

```elm
-- 1. The user enters the server URL and clicks "Connect".
--    This triggers the `Connect` message.
update Connect model ->
    ( { model | connected = True }, connect model.serverUrl )

-- 2. The user clicks the "List Resources" button.
--    This triggers the `GetResources` message.
update GetResources model ->
    ( model, getResources () )

-- 3. The `lazy-wasm` library fetches the resource list and sends it back.
--    This triggers the `ReceiveResourceList` message.
update (ReceiveResourceList resources) model ->
    ( { model | resources = resources }, Cmd.none )

-- 4. The user clicks on a resource ID in the list.
--    (In a more complete example, this would trigger a `GetResource` message)

-- 5. The user clicks the "Stream All Resources" button.
--    This triggers the `StreamAll` message.
update StreamAll model ->
    ( model, streamAll () )

-- 6. The `lazy-wasm` library sends a live update.
--    This triggers the `ReceiveUpdate` message.
update (ReceiveUpdate newUpdate) model ->
    ( { model | updates = newUpdate :: model.updates }, Cmd.none )
```

---

## 8. Building

### Building without Nix

To build the `lazy-elm-demo` project without using Nix, you will need to have the Elm compiler installed on your system. You can then compile the project by running the following command in the project's root directory:

```bash
elm make src/Main.elm --output=main.js
```

This will generate a `main.js` file that you can include in an HTML file to run the application.

### Building with Nix

If you have the Nix package manager installed, you can build the project by running the following command:

```bash
nix-build
```

This will create a `result` directory containing the compiled `main.js` file.

---

## 9. Testing

This project is intended as a demonstration and does not currently include an automated test suite. In a production setting, you would want to add tests using a library like `elm-explorations/test`.

Here is an example of a simple test you could write:

```elm
module MainTests exposing (..)

import Test exposing (..)
import Main exposing (..)


suite : Test
suite =
    describe "Main"
        [ describe "update"
            [ test "UpdateServerUrl" <|
                \() ->
                    let
                        ( model, _ ) = init ()
                        ( newModel, _ ) = update (UpdateServerUrl "ws://localhost:9001") model
                    in
                    Expect.equal newModel.serverUrl "ws://localhost:9001"
            ]
        ]
```

---

## 10. Project Structure

```
.gitignore
.nix/...
elm.json
README.md
src/
  Main.elm
```

*   **`.gitignore`**: A standard gitignore file.
*   **`.nix`**: Nix-related files for building the project.
*   **`elm.json`**: The Elm project configuration file.
*   **`README.md`**: This file.
*   **`src/Main.elm`**: The main Elm source file.

---

## 11. Contributing

Contributions to `lazy-elm-demo` are welcome! If you find a bug or have an idea for an improvement, please open an issue on the [GitHub repository](https://github.com/dyna-proj/lazy-elm-demo/issues). If you would like to contribute code, please fork the repository and submit a pull request.

When contributing, please ensure that your code adheres to the existing style and that you have added appropriate tests.

---

## 12. License

`lazy-elm-demo` is licensed under the [MIT License](LICENSE).

---

## 13. Related Projects

*   **[dyna-core](https://github.com/dyna-proj/dyna-core):** The core Rust library for the Dyna ecosystem.
*   **[dyna-cli](https://github.com/dyna-proj/dyna-cli):** A command-line interface for interacting with a Dyna server.
*   **[dyna-server](https://github.com/dyna-proj/dyna-server):** The `axum`-based HTTP server for the Dyna ecosystem.
*   **[dyna-wasm](https://github.com/dyna-proj/dyna-wasm):** The WebAssembly client library for Dyna.
*   **[lazy-cat](https://github.com/dyna-proj/lazy-cat):** A lazy resource loader for the Dyna ecosystem.
*   **[lazy-wasm](https://github.com/dyna-proj/lazy-wasm):** The WebAssembly version of the lazy resource loader.

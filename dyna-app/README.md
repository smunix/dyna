# dyna-app

**Browser-based collaborative JSON editor for the Dyna distributed CRUD system.**

`dyna-app` is an [Elm](https://elm-lang.org/) application that uses `dyna-wasm` (via JavaScript ports) to provide a full-featured UI for editing, versioning, and collaborating on JSON resources.

## Features

- **Resource CRUD**: Create, read, update, and delete JSON resources identified by dotted resource IDs (e.g., `acme.entity.User`).
- **Channel-based editing**: All edits happen on feature channels (never directly on `main`). Create new channels or switch between existing ones.
- **Commit & Push**: Stage changes, commit with a message, and push to the remote server.
- **Promote to main**: Promote changesets from a feature channel to `main` with a single click.
- **Change history**: View the full history of changes for any resource, with per-operation detail.
- **Real-time notifications**: WebSocket-based notifications when promotions or pushes happen, displayed as toast popups.
- **JSON validation**: Built-in JSON syntax validation before staging.
- **Protected main channel**: The UI prevents direct edits on `main`, enforcing the channel-based workflow.

## Architecture

```
┌─────────────────────────────────────────────────┐
│                   Browser                        │
│                                                  │
│  ┌──────────────┐     Elm Ports     ┌──────────┐│
│  │   Elm UI      │◄───────────────►│  JS Glue  ││
│  │  (Main.elm)   │                  │           ││
│  │  (Ports.elm)  │                  │  ┌──────┐ ││
│  └──────────────┘                  │  │ WASM │ ││
│                                     │  │Client│ ││
│                                     │  └──────┘ ││
│                                     └──────────┘│
│                                          │       │
│                              fetch + WebSocket   │
└──────────────────────────────────────────┼───────┘
                                           │
                                    ┌──────▼───────┐
                                    │ dyna-server   │
                                    └──────────────┘
```

## Elm ↔ JavaScript Port Interface

The Elm app communicates with `dyna-wasm` through ports defined in `src/Ports.elm`:

### Outgoing Ports (Elm → JS)

| Port | Payload | Description |
|------|---------|-------------|
| `initRepo` | `{ url: String }` | Initialize the WASM client with a server URL |
| `loadResource` | `{ resourceId: String }` | Load a resource by its ID |
| `saveResource` | `{ resourceId, content }` | Write a resource to the in-memory VFS |
| `stageResource` | `{ resourceId: String }` | Stage a resource for commit |
| `deleteResource` | `{ resourceId: String }` | Stage a resource deletion |
| `commitChanges` | `{ message: String }` | Commit staged changes |
| `pushChanges` | `{}` | Push to remote server |
| `pullChanges` | `{}` | Pull from remote server |
| `promoteChannel` | `{}` | Promote current channel to main |
| `createChannel` | `{ name: String }` | Create a new channel |
| `switchChannel` | `{ name: String }` | Switch to an existing channel |
| `fetchChannels` | `{}` | List all channels |
| `fetchHistory` | `{ resourceId: String }` | Fetch resource change history |
| `fetchStatus` | `{}` | Get working directory status |
| `connectWebSocket` | `{ url: String }` | Connect to the notification WebSocket |

### Incoming Ports (JS → Elm)

| Port | Payload | Description |
|------|---------|-------------|
| `resourceLoaded` | `{ resourceId, content, exists }` | Resource load result |
| `resourceSaved` | `{ resourceId, success, error? }` | Save confirmation |
| `commitResult` | `{ success, changeId?, error? }` | Commit result |
| `pushResult` | `{ success, error? }` | Push result |
| `pullResult` | `{ success, error? }` | Pull result |
| `promoteResult` | `{ success, error? }` | Promote result |
| `channelsList` | `{ channels: [...] }` | Channel list response |
| `statusResult` | `{ staged, modified, deleted, ... }` | Status response |
| `historyResult` | `{ entries: [...] }` | Resource history entries |
| `notification` | `{ type, source, target, ... }` | WebSocket notification |

## Building

### Prerequisites

- [Elm](https://elm-lang.org/) 0.19.1+
- [wasm-pack](https://rustwasm.github.io/wasm-pack/)

### Build Steps

```bash
# 1. Build the WASM package
cd .. && wasm-pack build dyna-wasm --target web --out-dir ../pkg

# 2. Copy WASM artifacts into the Elm public directory
cp -r ../pkg dyna-app/public/pkg/

# 3. Compile the Elm application
cd dyna-app && elm make src/Main.elm --output=public/elm.js

# 4. Serve the public/ directory
# Use any static file server, e.g.:
python3 -m http.server 8000 --directory public/
```

### Development

```bash
# Recompile Elm on changes
elm make src/Main.elm --output=public/elm.js

# For optimized production build
elm make src/Main.elm --output=public/elm.js --optimize
```

## Usage

1. Open the app in a browser
2. Enter the dyna-server URL (e.g., `http://localhost:8080`) and click **Connect**
3. Create or switch to a feature channel (editing on `main` is not allowed)
4. Enter a resource ID (e.g., `acme.entity.User`) and click **Load** or start editing
5. Edit the JSON content in the editor
6. Click **Save** to write to the in-memory VFS, then **Stage** to stage for commit
7. Enter a commit message and click **Commit**
8. Click **Push** to send changes to the remote server
9. Click **Promote** to merge your channel's changes into `main`
10. Use the **History** tab to view the change timeline for any resource

## File Structure

```
dyna-app/
├── elm.json              # Elm project configuration
├── public/
│   ├── index.html        # HTML shell with WASM glue JS
│   ├── style.css         # Application styles
│   ├── elm.js            # Compiled Elm application (generated)
│   └── pkg/              # WASM artifacts (copied from wasm-pack output)
├── src/
│   ├── Main.elm          # Main application: model, update, view, subscriptions
│   └── Ports.elm         # Port declarations for JS interop
└── README.md
```

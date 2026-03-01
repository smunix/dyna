# dyna-wasm

**WebAssembly client for the Dyna distributed CRUD system.**

`dyna-wasm` compiles to WASM and provides a JavaScript-friendly API for interacting with the Dyna remote server directly from the browser. All local repository state is stored in an in-memory VFS (`MemoryFS`), so the entire `.dyna/` tree lives in browser memory.

## Features

- **Full repository operations**: init, clone, add, commit, push, pull, promote, squash, restore
- **In-memory VFS**: Uses `vfs::MemoryFS` for zero-dependency browser storage
- **Compressed storage**: All metadata files are gzip-compressed in memory
- **Compressed transport**: HTTP requests use `Content-Encoding: gzip`; responses decoded transparently
- **WebSocket notifications**: Subscribe to real-time promotion/push events via browser WebSocket
- **Resource history**: Query the change history of any resource from the remote server
- **Channel management**: Create, switch, and list channels
- **JSON interop**: All data-returning methods serialize results as JSON strings

## JavaScript API

```js
import init, { DynaClient } from './dyna_wasm.js';

await init();
const client = new DynaClient();

// Initialize with remote server
client.init_repo("https://my-dyna-server.example.com");

// Or clone an existing repo
await client.clone_repo("https://my-dyna-server.example.com");

// Write, stage, commit, push
client.write_file("data/users/config.json", JSON.stringify({ admin: true }));
client.add("data/users/config.json");
client.commit("Initial config");
await client.push();

// Get status
const status = JSON.parse(client.status());

// Subscribe to WebSocket notifications
client.subscribe_notifications("wss://my-server/ws", (event) => {
    console.log("Notification:", JSON.parse(event));
});

// Query resource history
const history = JSON.parse(await client.resource_history("data.users.config"));
```

## Exported Methods

| Category | Methods |
|----------|---------|
| **Init** | `new()`, `init_repo(url)`, `set_remote(url)`, `is_initialized()` |
| **File I/O** | `write_file(path, content)`, `read_file(path)`, `delete_file(path)`, `list_files()` |
| **Staging** | `add(path)`, `add_delete(path)` |
| **Status** | `status()` → JSON with staged, modified, deleted, unstaged-on-staged, conflicts |
| **Commit** | `commit(message)` → change_id |
| **Sync** | `push()`, `pull()`, `clone_repo(url)` — all async, using browser fetch |
| **Channels** | `channels()`, `create_channel(name)`, `switch_channel(name)` |
| **History** | `log(limit?)` → JSON array of changeset entries |
| **Resource History** | `resource_history(resource_id)` → JSON from remote server |
| **Advanced** | `restore(path, channel?, changeset?)`, `squash(revision?, into?, message?)`, `promote(channel?)` |
| **WebSocket** | `subscribe_notifications(ws_url, callback)` — real-time event subscription |

## Building

```bash
# Install wasm-pack
curl https://rustwasm.github.io/wasm-pack/installer/init.sh -sSf | sh

# Build for web target
wasm-pack build dyna-wasm --target web --out-dir ../pkg

# Output:
#   pkg/dyna_wasm_bg.wasm   — optimized WASM binary (~825 KB)
#   pkg/dyna_wasm.js        — JS glue code with ESM exports
#   pkg/dyna_wasm.d.ts      — TypeScript type definitions
#   pkg/package.json        — ready for npm publishing
```

## Dependencies

- `dyna-core` — shared models, diff/patch engine, compression, protocol types
- `wasm-bindgen` — Rust ↔ JS FFI
- `wasm-bindgen-futures` — async JS Promise integration
- `web-sys` — browser API bindings (fetch, WebSocket, Headers, Request, Response)
- `js-sys` — JavaScript built-in bindings
- `vfs` — virtual filesystem abstraction (MemoryFS)
- `serde` / `serde_json` — JSON serialization
- `flate2` — gzip compression

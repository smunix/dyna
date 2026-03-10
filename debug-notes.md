# Debug Notes: "Failed to initialise repository"

## Flow Analysis

1. User clicks "New Repository" in the Elm UI
2. Elm sends `ConnectToServer` message → calls `Ports.initRepo model.serverUrl`
3. JS bridge receives the port call:
   ```js
   app.ports.initRepo.subscribe(async function(url) {
       await wasmReady;
       if (!requireClient(app.ports.onInitResult)) return;
       try {
           client.init_repo(url);  // <-- This is synchronous, NOT async
           app.ports.onInitResult.send({ success: true, error: null });
       } catch (e) {
           app.ports.onInitResult.send({ success: false, error: e.message });
       }
   });
   ```
4. `DynaClient.init_repo()` in WASM:
   - Calls `self.repo.init()` → creates in-memory `.dyna/` directory structure
   - Sets `self.remote_url`
   - Loads config, sets remote_url, saves config
   - Returns `Ok(())`

5. On success, Elm receives `GotInitResult` with `{ success: true }` → navigates to ResourcesPage

## Possible Failure Points

### Most likely: WASM module not loaded
The `loadWasm()` function tries to import `./pkg/dyna_wasm.js`. If this file doesn't exist:
- `client` remains `null`
- `requireClient()` sends `{ success: false, error: "WASM module not loaded..." }`
- Elm shows "Failed to initialise repository"

### Evidence from dyna-app-serve script:
The script builds WASM with `wasm-pack build dyna-wasm --target web --out-dir "$PKG_DIR"`.
If wasm-pack fails, it only prints a warning and continues. The UI renders but WASM ops fail.

### Second possibility: init_repo throws
If `init_repo` is called twice, `Repository::init()` will bail with `DynaError::AlreadyInitialized`.
But this is the first call, so unlikely.

### Third possibility: Port default (8080) conflict
The `dyna-app-serve` script defaults to port 8080 (`PORT="${1:-8080}"`).
The `dyna-server` also defaults to port 8080 (`DYNA_BIND_ADDR=0.0.0.0:8080`).
If both are running on 8080, the app serve would fail to bind.
BUT the user says "dyna-server is up and listening to port 8080", so the app must be on a different port.

## Root Cause Assessment

The most likely issue is that `init_repo()` is a **synchronous** call that only initializes the
in-memory repository. It does NOT contact the server at all. The `init_repo` method:
1. Creates `.dyna/` directory structure in memory
2. Sets the remote URL in config

But the Elm UI then immediately tries to:
- `requestStatus()` 
- `requestChannels()`
- `listSnapshots()`
- `connectNotifications()`

These are all LOCAL operations on the in-memory repo. They don't contact the server either.

So the actual question is: **why does init_repo() throw?**

Looking more carefully at the WASM loading:
```js
const { default: init, DynaClient } = await import('./pkg/dyna_wasm.js');
```

If `dyna-app-serve` failed to build the WASM package (wasm-pack failed), then
`./pkg/dyna_wasm.js` doesn't exist, and `client` stays null.

The `requireClient` function then sends: `{ success: false, error: "WASM module not loaded..." }`

This matches the "Failed to initialise repository" error exactly.

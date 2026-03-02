# Debug Notes 3: Browser console errors

## Screenshot 1 (browser console):
1. `Uncaught TypeError: Cannot read properties of undefined (reading 'subscribe')` at `(index):282:29`
2. `[dyna] WASM module not available: unreachable` — loadWasm @ (index):48
3. `[dyna] The UI will render but repository operations will not work.` — loadWasm @ (index):49
4. `[dyna] WASM module not loaded. Ensure the pkg/ directory contains dyna_wasm.js and dyna_wasm_bg.wasm.` — (index):60

## Screenshot 2 (terminal):
- `ls -la dyna-app/public/pkg/` shows files ARE present:
  - dyna_wasm.d.ts (10k)
  - dyna_wasm.js (40k)
  - dyna_wasm_bg.wasm (1.3M)
  - dyna_wasm_bg.wasm.d.ts (4.1k)
- All files have `.r--r--r--` permissions (read-only, copied from Nix store)

## Screenshot 3 (terminal):
- `rm -fr dyna-app/public/pkg` then `dyna-app-serve`
- Shows "Copying pre-built WASM artifacts from Nix store..." — WASM package ready.
- Elm compilation succeeds
- Serving on http://localhost:3000
- GET /elm.js returns 200
- No request for pkg/dyna_wasm.js visible in the server log

## Analysis

### Error 1: TypeError at (index):282
Line 282 in the rendered HTML corresponds to `app.ports.listFiles.subscribe(...)`.
But wait — the Elm Ports.elm DOES declare `listFiles` as a port. However, Elm only
creates the JS port object if the port is actually USED in the Elm code (i.e., if
there's a Cmd that sends through it). If `listFiles` is declared but never called
via `Ports.listFiles ()` in any update branch, Elm's dead-code elimination (with
--optimize) may remove it, making `app.ports.listFiles` undefined.

Let me check if `listFiles` is actually used in the update function...

### Error 2: WASM "unreachable"
The `unreachable` trap happens during `new DynaClient()` or `await init()`.
This could be:
- console_error_panic_hook not being set up (the constructor comment says it should)
- A missing import or incompatible WASM binary

But actually — Error 1 happens FIRST (at line 282, which is synchronous code).
The `<script type="module">` runs top-to-bottom. The port subscriptions at lines
71-331 are all synchronous `.subscribe()` calls. If `app.ports.listFiles` is
undefined, the script crashes at line 282 and NEVER reaches the WASM loading code.

Wait, no — the WASM loading is async (loadWasm() returns a promise). The subscribe
calls are synchronous. So the execution order is:
1. Elm.Main.init() — synchronous
2. loadWasm() starts — returns promise, stored in wasmReady
3. All .subscribe() calls execute synchronously
4. If any port is undefined, it throws at that line and stops further subscribe calls
5. loadWasm() completes asynchronously (but the error is independent)

Actually, looking more carefully: the TypeError at (index):282 would crash the
entire module script. This means ALL port subscriptions AFTER line 282 would
NOT be registered. But the ones BEFORE (initRepo at line 71, etc.) WOULD work.

But the WASM error "unreachable" is separate — it happens during loadWasm().

The key question: is `app.ports.listFiles` undefined? If so, it means the Elm
compiler with --optimize removed the port because it's never used as a Cmd.

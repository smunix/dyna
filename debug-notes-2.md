# Debug Notes 2: WASM module not loaded

## Screenshot Error
"Failed to initialise repository: WASM module not loaded. Ensure the pkg/ directory contains dyna_wasm.js and dyna_wasm_bg.wasm."

## Context
- User is running `dyna-app-serve` from project root inside `nix develop` shell
- The `nix develop` shell provides: wasm-pack, wasm-bindgen-cli, Rust toolchain with wasm32-unknown-unknown target

## Problem in dyna-app-serve script

The `wasm-pack build` command:
```
wasm-pack build dyna-wasm --target web --out-dir "$PKG_DIR"
```

Where `$PKG_DIR` = `$(pwd)/dyna-app/public/pkg`

### Issue: `--out-dir` with wasm-pack

`wasm-pack build` `--out-dir` is **relative to the crate directory**, NOT the current working directory.

So when running from project root:
- CWD = `/path/to/dyna` (project root)
- Crate = `dyna-wasm` (relative to CWD)
- `--out-dir "$PKG_DIR"` where PKG_DIR = `/path/to/dyna/dyna-app/public/pkg` (absolute)

Wait — PKG_DIR is computed as an absolute path from `$ELM_SRC/public/pkg` where
ELM_SRC = `$(pwd)/dyna-app`. So it IS an absolute path. That should work.

### Alternative issue: wasm-pack build actually fails

The script catches the failure but only prints a warning and continues.
The user would see "Warning: wasm-pack build failed" in the terminal.

Possible reasons wasm-pack fails:
1. Version mismatch between wasm-bindgen CLI and the wasm-bindgen dependency in Cargo.toml
2. Missing wasm32-unknown-unknown target (but nix develop should provide it)
3. Cargo workspace resolution issues

### Most likely: wasm-bindgen version mismatch

The Nix flake pins `pkgs.wasm-bindgen-cli` from nixpkgs, but the Cargo.lock
pins a specific version of `wasm-bindgen`. If these don't match exactly,
wasm-pack fails with:
"it looks like the Rust project used to create this wasm file was linked against
version of wasm-bindgen that uses a different bindgen format than this binary"

This is a VERY common wasm-pack failure mode.

### Fix approach

The `dyna-app-serve` script should use the Nix-built WASM package (`dyna-wasm`)
that was already built correctly by the Nix build system, instead of trying to
rebuild from source with wasm-pack at runtime.

In the `nix develop` shell, `dyna-wasm` is available as a package. The flake
already builds it correctly using crane + wasm-bindgen-cli with matching versions.

The pre-built `dyna-app-serve` from `nix build .#dyna-app-serve` already has
the correct WASM artifacts bundled (via the `dyna-app` derivation which copies
`${dyna-wasm}/*` into `$out/pkg/`). But the dev script tries to rebuild from
source, which can fail.

### Better fix: Use pre-built WASM artifacts from Nix store

In the devShell, `dyna-wasm` package is on PATH (well, it's a derivation output).
We could reference it. But actually, the script should fall back to using the
Nix-built artifacts if wasm-pack fails.

Or better: the script should reference the Nix store path of the dyna-wasm
package directly, since it's a writeShellScriptBin that has access to Nix
interpolation.

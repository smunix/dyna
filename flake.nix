{
  description = "Dyna — Distributed CRUD for Collaborative JSON Editing";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";

    flake-parts.url = "github:hercules-ci/flake-parts";

    crane.url = "github:ipetkov/crane";

    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs = inputs@{ self, nixpkgs, flake-parts, crane, rust-overlay, ... }:
    flake-parts.lib.mkFlake { inherit inputs; } {
      systems = [ "x86_64-linux" "aarch64-linux" "x86_64-darwin" "aarch64-darwin" ];

      perSystem = { pkgs, system, lib, ... }:
        let
          # ── Nixpkgs with Rust overlay ──────────────────────────────────
          overlayPkgs = import nixpkgs {
            inherit system;
            overlays = [ (import rust-overlay) ];
          };

          # ── Rust toolchains ────────────────────────────────────────────
          #
          # Native toolchain: builds dyna-cli and dyna-server
          nativeToolchain = overlayPkgs.rust-bin.stable.latest.default.override {
            extensions = [ "rust-src" "rust-analyzer" "clippy" "rustfmt" ];
          };

          # WASM toolchain: builds dyna-wasm for wasm32-unknown-unknown
          wasmToolchain = overlayPkgs.rust-bin.stable.latest.default.override {
            targets = [ "wasm32-unknown-unknown" ];
            extensions = [ "rust-src" ];
          };

          # ── Crane libraries ────────────────────────────────────────────
          craneLib = (crane.mkLib pkgs).overrideToolchain nativeToolchain;
          craneLibWasm = (crane.mkLib pkgs).overrideToolchain wasmToolchain;

          # ── Source filtering ───────────────────────────────────────────
          #
          # Include Rust sources, Cargo manifests, and the lock file.
          # Elm and Nix files are excluded from the Rust build.
          rustSrc = lib.fileset.toSource {
            root = ./.;
            fileset = lib.fileset.unions [
              ./Cargo.toml
              ./Cargo.lock
              (craneLib.fileset.commonCargoSources ./dyna-core)
              (craneLib.fileset.commonCargoSources ./dyna-cli)
              (craneLib.fileset.commonCargoSources ./dyna-server)
              (craneLib.fileset.commonCargoSources ./dyna-wasm)
              (craneLib.fileset.commonCargoSources ./dyna-py)
            ];
          };

          # ── Common build arguments ─────────────────────────────────────
          commonArgs = {
            src = rustSrc;
            strictDeps = true;
            buildInputs = lib.optionals pkgs.stdenv.isDarwin [
              pkgs.libiconv
              pkgs.darwin.apple_sdk.frameworks.Security
              pkgs.darwin.apple_sdk.frameworks.SystemConfiguration
            ];
          };

          # ── Dependency caching (native) ────────────────────────────────
          cargoArtifacts = craneLib.buildDepsOnly (commonArgs // {
            pname = "dyna-workspace-deps";
          });

          # ── Individual crate args ──────────────────────────────────────
          individualCrateArgs = commonArgs // {
            inherit cargoArtifacts;
            doCheck = false; # tests run separately via checks
          };

          # ── Native packages ────────────────────────────────────────────

          dyna-cli = craneLib.buildPackage (individualCrateArgs // {
            pname = "dyna-cli";
            cargoExtraArgs = "-p dyna-cli";
          });

          dyna-server = craneLib.buildPackage (individualCrateArgs // {
            pname = "dyna-server";
            cargoExtraArgs = "-p dyna-server";
          });

          # ── WASM package ───────────────────────────────────────────────
          #
          # We build dyna-wasm using crane with the wasm32 toolchain,
          # then run wasm-bindgen-cli to produce the JS/TS glue.
          wasmCargoArtifacts = craneLibWasm.buildDepsOnly (commonArgs // {
            pname = "dyna-wasm-deps";
            cargoExtraArgs = "-p dyna-wasm --target wasm32-unknown-unknown";
            doCheck = false;
          });

          dyna-wasm-raw = craneLibWasm.buildPackage (commonArgs // {
            pname = "dyna-wasm";
            cargoArtifacts = wasmCargoArtifacts;
            cargoExtraArgs = "-p dyna-wasm --target wasm32-unknown-unknown";
            doCheck = false;
            # The raw .wasm file ends up in the target directory
            installPhaseCommand = ''
              mkdir -p $out/lib
              cp target/wasm32-unknown-unknown/release/dyna_wasm.wasm $out/lib/ 2>/dev/null || \
              cp target/wasm32-unknown-unknown/release/*.wasm $out/lib/ 2>/dev/null || true
            '';
          });

          dyna-wasm = pkgs.stdenv.mkDerivation {
            pname = "dyna-wasm";
            version = "0.1.0";
            dontUnpack = true;
            nativeBuildInputs = [ pkgs.wasm-bindgen-cli ];
            buildPhase = ''
              wasm-bindgen \
                --target web \
                --out-dir $out \
                --out-name dyna_wasm \
                ${dyna-wasm-raw}/lib/dyna_wasm.wasm
            '';
            installPhase = "true"; # output already in $out
          };

          # ── Elm package ────────────────────────────────────────────────
          dyna-app = pkgs.stdenv.mkDerivation {
            pname = "dyna-app";
            version = "0.1.0";
            src = ./dyna-app;
            nativeBuildInputs = [ pkgs.elmPackages.elm ];

            # Elm needs a writable home for its cache
            HOME = "$TMPDIR";

            buildPhase = ''
              mkdir -p $TMPDIR/.elm
              export ELM_HOME=$TMPDIR/.elm
              elm make src/Main.elm --optimize --output=elm.js
            '';

            installPhase = ''
              mkdir -p $out
              cp -r public/* $out/ 2>/dev/null || true
              cp elm.js $out/elm.js

              # Bundle the WASM package alongside the Elm app
              mkdir -p $out/pkg
              cp ${dyna-wasm}/* $out/pkg/ 2>/dev/null || true
            '';
          };

          # ── Python package (dyna-py via maturin) ────────────────────
          #
          # Builds the PyO3 cdylib and installs it as a Python package.
          dyna-py = pkgs.python3Packages.buildPythonPackage {
            pname = "dyna-py";
            version = "0.1.0";
            format = "pyproject";

            src = lib.fileset.toSource {
              root = ./.;
              fileset = lib.fileset.unions [
                ./Cargo.toml
                ./Cargo.lock
                (craneLib.fileset.commonCargoSources ./dyna-core)
                (craneLib.fileset.commonCargoSources ./dyna-cli)
                (craneLib.fileset.commonCargoSources ./dyna-py)
                ./dyna-py/pyproject.toml
                ./dyna-py/python
              ];
            };

            cargoDeps = pkgs.rustPlatform.importCargoLock {
              lockFile = ./Cargo.lock;
            };

            nativeBuildInputs = [
              pkgs.rustPlatform.cargoSetupHook
              pkgs.rustPlatform.maturinBuildHook
              nativeToolchain
              pkgs.pkg-config
            ];

            buildInputs = [
              pkgs.openssl
            ] ++ lib.optionals pkgs.stdenv.isDarwin [
              pkgs.libiconv
              pkgs.darwin.apple_sdk.frameworks.Security
              pkgs.darwin.apple_sdk.frameworks.SystemConfiguration
            ];

            propagatedBuildInputs = [
              pkgs.python3Packages.click
            ];

            pythonImportsCheck = [ "dyna_py" ];
          };

          # ── Docker / OCI images ────────────────────────────────────────

          dyna-server-image = pkgs.dockerTools.buildLayeredImage {
            name = "dyna-server";
            tag = "latest";
            contents = [ dyna-server pkgs.cacert ];
            config = {
              Cmd = [ "${dyna-server}/bin/dyna-server" ];
              ExposedPorts = { "8080/tcp" = {}; };
              Env = [ "SSL_CERT_FILE=${pkgs.cacert}/etc/ssl/certs/ca-bundle.crt" ];
            };
          };

          dyna-app-image = pkgs.dockerTools.buildLayeredImage {
            name = "dyna-app";
            tag = "latest";
            contents = [ dyna-app pkgs.static-web-server ];
            config = {
              Cmd = [
                "${pkgs.static-web-server}/bin/static-web-server"
                "--root" "${dyna-app}"
                "--port" "8080"
              ];
              ExposedPorts = { "8080/tcp" = {}; };
            };
          };

          # ── dyna-app-serve script ─────────────────────────────────
          #
          # A convenience wrapper that builds the Elm app (if needed),
          # bundles the WASM package, and serves the result locally.
          dyna-app-serve = pkgs.writeShellScriptBin "dyna-app-serve" ''
            set -euo pipefail

            PORT="''${1:-8080}"
            SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"

            # Locate the dyna-app source directory
            ELM_SRC="''${DYNA_ELM_SRC:-$(pwd)/dyna-app}"
            if [ ! -f "$ELM_SRC/elm.json" ]; then
              echo "Error: Cannot find dyna-app/elm.json"
              echo "Run this from the workspace root, or set DYNA_ELM_SRC."
              exit 1
            fi

            # Build the WASM package if not already built
            WASM_OUT="$(pwd)/pkg"
            if [ ! -f "$WASM_OUT/dyna_wasm.js" ]; then
              echo "Building dyna-wasm..."
              wasm-pack build dyna-wasm --target web --out-dir "$WASM_OUT"
            fi

            # Compile the Elm app
            echo "Compiling Elm app..."
            (cd "$ELM_SRC" && elm make src/Main.elm --optimize --output=public/elm.js)

            # Copy WASM package into the Elm public directory
            mkdir -p "$ELM_SRC/public/pkg"
            cp "$WASM_OUT"/dyna_wasm* "$ELM_SRC/public/pkg/" 2>/dev/null || true

            # Serve the Elm app
            echo ""
            echo "  Serving dyna-app on http://localhost:$PORT"
            echo "  Press Ctrl+C to stop."
            echo ""
            ${pkgs.python3}/bin/python3 -m http.server "$PORT" --directory "$ELM_SRC/public"
          '';

        in
        {
          # ── Checks ───────────────────────────────────────────────────
          checks = {
            inherit dyna-cli dyna-server dyna-wasm dyna-app;

            dyna-workspace-clippy = craneLib.cargoClippy (commonArgs // {
              inherit cargoArtifacts;
              cargoClippyExtraArgs = "--all-targets -- --deny warnings";
            });

            dyna-workspace-fmt = craneLib.cargoFmt {
              src = rustSrc;
            };

            dyna-workspace-tests = craneLib.cargoNextest (commonArgs // {
              inherit cargoArtifacts;
              partitions = 1;
              partitionType = "count";
            });
          };

          # ── Packages ─────────────────────────────────────────────────
          packages = {
            inherit dyna-cli dyna-server dyna-wasm dyna-app dyna-py dyna-app-serve;
            inherit dyna-server-image dyna-app-image;
            default = dyna-cli;
          };

          # ── Apps ─────────────────────────────────────────────────────
          apps = {
            dyna-cli = {
              type = "app";
              program = "${dyna-cli}/bin/dyna-cli";
            };
            dyna-server = {
              type = "app";
              program = "${dyna-server}/bin/dyna-server";
            };
            dyna-app-serve = {
              type = "app";
              program = "${dyna-app-serve}/bin/dyna-app-serve";
            };
            dyna-py = {
              type = "app";
              program = "${dyna-py}/bin/dyna-py";
            };
            default = {
              type = "app";
              program = "${dyna-cli}/bin/dyna-cli";
            };
          };

          # ── Dev shell ────────────────────────────────────────────────
          #
          # `nix develop` drops you into a shell with all tools available:
          #   - Rust toolchain (with wasm32 target, clippy, rustfmt, rust-analyzer)
          #   - wasm-pack, wasm-bindgen-cli
          #   - Elm compiler
          #   - cargo-nextest, cargo-watch
          #   - Built dyna-cli and dyna-server on PATH
          devShells.default = pkgs.mkShell {
            inputsFrom = [ dyna-cli dyna-server ];

            nativeBuildInputs = [
              # Rust toolchain with WASM target + dev extensions
              (overlayPkgs.rust-bin.stable.latest.default.override {
                targets = [ "wasm32-unknown-unknown" ];
                extensions = [
                  "rust-src"
                  "rust-analyzer"
                  "clippy"
                  "rustfmt"
                ];
              })

              # Pre-built Dyna binaries on PATH
              dyna-cli
              dyna-server
              dyna-app-serve

              # Python (dyna-py)
              pkgs.python3
              pkgs.python3Packages.maturin
              pkgs.python3Packages.click
              dyna-py

              # WASM tooling
              pkgs.wasm-pack
              pkgs.wasm-bindgen-cli

              # Elm
              pkgs.elmPackages.elm
              pkgs.elmPackages.elm-format
              pkgs.elmPackages.elm-test
              pkgs.elmPackages.elm-review

              # Cargo extras
              pkgs.cargo-nextest
              pkgs.cargo-watch

              # General
              pkgs.pkg-config
              pkgs.openssl
            ];

            shellHook = ''
              echo ""
              echo "  ╔══════════════════════════════════════════════════════════╗"
              echo "  ║              Dyna Development Shell                      ║"
              echo "  ╠══════════════════════════════════════════════════════════╣"
              echo "  ║                                                          ║"
              echo "  ║  Pre-built binaries (on PATH):                           ║"
              echo "  ║    dyna             — CLI client                         ║"
              echo "  ║    dyna-server      — start the server                   ║"
              echo "  ║    dyna-app-serve   — build & serve the Elm UI           ║"
              echo "  ║    dyna-py          — Python CLI (via PyO3)               ║"
              echo "  ║                                                          ║"
              echo "  ║  Development commands:                                   ║"
              echo "  ║    cargo build      — build native crates from source    ║"
              echo "  ║    cargo test       — run all tests                      ║"
              echo "  ║    cargo watch      — rebuild on file changes            ║"
              echo "  ║    wasm-pack build dyna-wasm --target web                ║"
              echo "  ║    maturin develop  — rebuild dyna-py from source         ║"
              echo "  ║                                                          ║"
              echo "  ╚══════════════════════════════════════════════════════════╝"
              echo ""
            '';

            # Ensure openssl is found by Rust builds
            PKG_CONFIG_PATH = "${pkgs.openssl.dev}/lib/pkgconfig";
          };
        };
    };
}

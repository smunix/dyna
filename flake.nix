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
          nativeToolchain = overlayPkgs.rust-bin.stable.latest.default.override {
            extensions = [ "rust-src" "rust-analyzer" "clippy" "rustfmt" ];
          };

          wasmToolchain = overlayPkgs.rust-bin.stable.latest.default.override {
            targets = [ "wasm32-unknown-unknown" ];
            extensions = [ "rust-src" ];
          };

          # ── Crane libraries ────────────────────────────────────────────
          craneLib = (crane.mkLib pkgs).overrideToolchain nativeToolchain;
          craneLibWasm = (crane.mkLib pkgs).overrideToolchain wasmToolchain;

          # ── Platform-specific build inputs ─────────────────────────────
          darwinBuildInputs = lib.optionals pkgs.stdenv.isDarwin [
            pkgs.libiconv
            pkgs.openssl
            pkgs.darwin.apple_sdk.frameworks.Security
            pkgs.darwin.apple_sdk.frameworks.SystemConfiguration
          ];

          # ════════════════════════════════════════════════════════════════
          # Helper: resolve workspace dependency crate paths from Cargo.toml
          # ════════════════════════════════════════════════════════════════
          #
          # Reads a crate's Cargo.toml, finds all `{ workspace = true }` or
          # `{ path = "..." }` dependencies that reference sibling workspace
          # crates, and returns their directory paths.
          #
          # This is the core of the deduplication: instead of manually listing
          # which crates each package depends on, we derive it from the
          # Cargo.toml itself.

          workspaceMembers = [ "dyna-core" "dyna-cli" "dyna-server" "dyna-wasm" "dyna-py" ];

          # Parse a crate's Cargo.toml and extract workspace dependency names
          # that are also workspace members (i.e. local crate deps).
          readLocalDeps = cratePath:
            let
              cargoToml = builtins.fromTOML (builtins.readFile (cratePath + "/Cargo.toml"));
              # Collect all dependency sections
              allDeps = (cargoToml.dependencies or {})
                     // (cargoToml.dev-dependencies or {})
                     // (cargoToml.build-dependencies or {});
              # A dep is local if it's a workspace member name and either:
              #   - has `workspace = true`
              #   - has a `path` attribute
              isLocalDep = name: spec:
                builtins.elem name workspaceMembers
                && (
                  (builtins.isAttrs spec && (spec.workspace or false || spec ? path))
                  || false
                );
              localDepNames = builtins.filter
                (name: isLocalDep name (allDeps.${name}))
                (builtins.attrNames allDeps);
            in
              localDepNames;

          # Recursively resolve all transitive local deps for a crate.
          # Returns a deduplicated list of crate directory paths (as strings).
          resolveAllLocalDeps = cratePath:
            let
              directNames = readLocalDeps cratePath;
              directPaths = map (name: ./. + "/${name}") directNames;
              transitive = builtins.concatMap resolveAllLocalDeps directPaths;
            in
              lib.lists.unique (directPaths ++ transitive);

          # ════════════════════════════════════════════════════════════════
          # Helper: build filtered source tree for a crate
          # ════════════════════════════════════════════════════════════════
          #
          # Includes the workspace root manifests, the crate itself, and all
          # of its transitive local dependencies.

          mkCrateSrc =
            { cratePath       # Path to the crate directory (e.g. ./dyna-cli)
            , extraPaths ? [] # Additional paths to include (e.g. pyproject.toml)
            , useCraneLib ? craneLib
            }:
            let
              localDeps = resolveAllLocalDeps cratePath;
              crateFilesets = map (p: useCraneLib.fileset.commonCargoSources p) ([ cratePath ] ++ localDeps);
              extraFilesets = map (p: p) extraPaths;
            in
              lib.fileset.toSource {
                root = ./.;
                fileset = lib.fileset.unions ([
                  ./Cargo.toml
                  ./Cargo.lock
                ] ++ crateFilesets ++ extraFilesets);
              };

          # ════════════════════════════════════════════════════════════════
          # Helper: build a native Rust package (deps cache + final build)
          # ════════════════════════════════════════════════════════════════
          #
          # Given a crate name, produces { src, cargoArtifacts, package }
          # with all boilerplate handled automatically.

          mkRustPackage =
            { pname                    # Package name (e.g. "dyna-cli")
            , cratePath                # Path to the crate directory
            , extraSrcPaths ? []       # Extra paths for source filtering
            , extraBuildInputs ? []    # Additional build inputs
            , buildPackageArgs ? {}    # Extra args passed to buildPackage
            }:
            let
              src = mkCrateSrc {
                inherit cratePath;
                extraPaths = extraSrcPaths;
              };

              cargoArtifacts = craneLib.buildDepsOnly {
                pname = "${pname}-deps";
                inherit src;
                strictDeps = true;
                cargoExtraArgs = "-p ${pname}";
                buildInputs = darwinBuildInputs ++ extraBuildInputs;
              };

              package = craneLib.buildPackage ({
                inherit pname src cargoArtifacts;
                strictDeps = true;
                cargoExtraArgs = "-p ${pname}";
                doCheck = false;
                buildInputs = darwinBuildInputs ++ extraBuildInputs;
              } // buildPackageArgs);
            in
              { inherit src cargoArtifacts package; };

          # ════════════════════════════════════════════════════════════════
          # Helper: build a WASM package (deps cache + raw build + bindgen)
          # ════════════════════════════════════════════════════════════════
          #
          # Produces the raw .wasm via crane, then runs wasm-bindgen-cli
          # to generate JS/TS glue code.

          mkWasmPackage =
            { pname                    # Package name (e.g. "dyna-wasm")
            , cratePath                # Path to the crate directory
            , wasmFileName             # Base name of the .wasm file (e.g. "dyna_wasm")
            , extraSrcPaths ? []       # Extra paths for source filtering
            , extraBuildInputs ? []    # Additional build inputs
            , bindgenTarget ? "web"    # wasm-bindgen target (web, nodejs, etc.)
            }:
            let
              wasmTarget = "wasm32-unknown-unknown";

              src = mkCrateSrc {
                inherit cratePath;
                extraPaths = extraSrcPaths;
                useCraneLib = craneLibWasm;
              };

              cargoArtifacts = craneLibWasm.buildDepsOnly {
                pname = "${pname}-deps";
                inherit src;
                strictDeps = true;
                cargoExtraArgs = "-p ${pname} --target ${wasmTarget}";
                doCheck = false;
                buildInputs = darwinBuildInputs ++ extraBuildInputs;
              };

              raw = craneLibWasm.buildPackage {
                inherit pname src cargoArtifacts;
                strictDeps = true;
                cargoExtraArgs = "-p ${pname} --target ${wasmTarget}";
                doCheck = false;
                buildInputs = darwinBuildInputs ++ extraBuildInputs;
                installPhaseCommand = ''
                  mkdir -p $out/lib
                  cp target/${wasmTarget}/release/${wasmFileName}.wasm $out/lib/ 2>/dev/null || \
                  cp target/${wasmTarget}/release/*.wasm $out/lib/ 2>/dev/null || true
                '';
              };

              package = pkgs.stdenv.mkDerivation {
                inherit pname;
                version = "0.1.0";
                dontUnpack = true;
                nativeBuildInputs = [ pkgs.wasm-bindgen-cli ];
                buildPhase = ''
                  wasm-bindgen \
                    --target ${bindgenTarget} \
                    --out-dir $out \
                    --out-name ${wasmFileName} \
                    ${raw}/lib/${wasmFileName}.wasm
                '';
                installPhase = "true";
              };
            in
              { inherit src cargoArtifacts raw package; };

          # ════════════════════════════════════════════════════════════════
          # Helper: build the full workspace source (for checks)
          # ════════════════════════════════════════════════════════════════

          workspaceSrc = lib.fileset.toSource {
            root = ./.;
            fileset = lib.fileset.unions ([
              ./Cargo.toml
              ./Cargo.lock
            ] ++ map
              (name: craneLib.fileset.commonCargoSources (./. + "/${name}"))
              workspaceMembers);
          };

          workspaceCargoArtifacts = craneLib.buildDepsOnly {
            pname = "dyna-workspace-deps";
            src = workspaceSrc;
            strictDeps = true;
            buildInputs = darwinBuildInputs;
          };

          # ════════════════════════════════════════════════════════════════
          # Package definitions (using helpers)
          # ════════════════════════════════════════════════════════════════

          # ── Native packages ────────────────────────────────────────────

          cliResult = mkRustPackage {
            pname = "dyna-cli";
            cratePath = ./dyna-cli;
          };
          dyna-cli = cliResult.package;

          serverResult = mkRustPackage {
            pname = "dyna-server";
            cratePath = ./dyna-server;
          };
          dyna-server = serverResult.package;

          # ── WASM package ───────────────────────────────────────────────

          wasmResult = mkWasmPackage {
            pname = "dyna-wasm";
            cratePath = ./dyna-wasm;
            wasmFileName = "dyna_wasm";
          };
          dyna-wasm = wasmResult.package;

          # ── Elm package ────────────────────────────────────────────────
          dyna-app = pkgs.stdenv.mkDerivation {
            pname = "dyna-app";
            version = "0.1.0";
            src = ./dyna-app;
            nativeBuildInputs = [ pkgs.elmPackages.elm ];

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
          dyna-py = pkgs.python3Packages.buildPythonPackage {
            pname = "dyna-py";
            version = "0.1.0";
            format = "pyproject";

            src = mkCrateSrc {
              cratePath = ./dyna-py;
              extraPaths = [
                ./dyna-py/pyproject.toml
                ./dyna-py/python
              ];
            };

            # Tell cargoSetupHook that the crate lives in a subdirectory,
            # not at the workspace root.  Without this, maturin tries to
            # parse the workspace Cargo.toml (which has [workspace] but
            # no [package]) and fails with "missing field `package`".
            cargoRoot = "dyna-py";

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
            ] ++ darwinBuildInputs;

            propagatedBuildInputs = [
              pkgs.python3Packages.click
            ];

            # Point maturin at the subcrate manifest so it doesn't
            # pick up the workspace root Cargo.toml.
            maturinBuildFlags = [ "--manifest-path" "dyna-py/Cargo.toml" ];

            pythonImportsCheck = [ "dyna_py" ];
          };

          # ── Go package (dyna-go) ──────────────────────────────────────
          dyna-go = pkgs.buildGoModule {
            pname = "dyna-go";
            version = "0.1.0";
            src = ./dyna-go;
            vendorHash = null;
            subPackages = [ "dynago" ];
            buildPhase = ''
              runHook preBuild
              go build ./dynago/...
              runHook postBuild
            '';
            checkPhase = ''
              runHook preCheck
              go test ./dynago/ -v -count=1
              runHook postCheck
            '';
            installPhase = ''
              runHook preInstall
              mkdir -p $out/share/dyna-go
              cp -r . $out/share/dyna-go/
              runHook postInstall
            '';
            meta = {
              description = "Go client library for Dyna — Distributed CRUD for Collaborative JSON Editing";
              homepage = "https://github.com/smunix/dyna";
            };
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
          dyna-serve-py = pkgs.writeText "dyna-serve.py" ''
            import http.server, functools, os

            class WasmHandler(http.server.SimpleHTTPRequestHandler):
                extensions_map = {
                    **http.server.SimpleHTTPRequestHandler.extensions_map,
                    ".wasm": "application/wasm",
                    ".js":   "application/javascript",
                    ".mjs":  "application/javascript",
                    ".json": "application/json",
                }

            serve_dir = os.environ["SERVE_DIR"]
            port = int(os.environ["SERVE_PORT"])
            handler = functools.partial(WasmHandler, directory=serve_dir)
            with http.server.HTTPServer(("", port), handler) as httpd:
                httpd.serve_forever()
          '';

          dyna-app-serve = pkgs.writeShellScriptBin "dyna-app-serve" ''
            set -euo pipefail

            PORT="''${1:-8080}"

            # Locate the dyna-app source directory
            ELM_SRC="''${DYNA_ELM_SRC:-$(pwd)/dyna-app}"
            if [ ! -f "$ELM_SRC/elm.json" ]; then
              echo "Error: Cannot find dyna-app/elm.json"
              echo "Run this from the workspace root, or set DYNA_ELM_SRC."
              exit 1
            fi

            # Build the WASM package (output goes into dyna-app/public/pkg)
            PKG_DIR="$ELM_SRC/public/pkg"
            mkdir -p "$PKG_DIR"
            if [ ! -f "$PKG_DIR/dyna_wasm.js" ]; then
              echo "Building dyna-wasm..."
              if wasm-pack build dyna-wasm --target web --out-dir "$PKG_DIR" 2>&1; then
                echo "  WASM package built successfully."
              else
                echo "  Warning: wasm-pack build failed. The UI will render but WASM operations will not work."
                echo "  Make sure wasm-pack and the wasm32-unknown-unknown target are available."
              fi
            fi

            # Compile the Elm app
            echo "Compiling Elm app..."
            (cd "$ELM_SRC" && elm make src/Main.elm --optimize --output=public/elm.js)

            # Serve with correct MIME types (especially .wasm -> application/wasm)
            echo ""
            echo "  Serving dyna-app on http://localhost:$PORT"
            echo "  Press Ctrl+C to stop."
            echo ""
            SERVE_DIR="$ELM_SRC/public" SERVE_PORT="$PORT" \
              ${pkgs.python3}/bin/python3 ${dyna-serve-py}
          '';

        in
        {
          # ── Checks ───────────────────────────────────────────────────
          checks = {
            inherit dyna-cli dyna-server dyna-wasm dyna-app dyna-go;

            dyna-workspace-clippy = craneLib.cargoClippy {
              src = workspaceSrc;
              strictDeps = true;
              cargoArtifacts = workspaceCargoArtifacts;
              cargoClippyExtraArgs = "--all-targets -- --deny warnings";
              buildInputs = darwinBuildInputs;
            };

            dyna-workspace-fmt = craneLib.cargoFmt {
              src = workspaceSrc;
            };

            dyna-workspace-tests = craneLib.cargoNextest {
              src = workspaceSrc;
              strictDeps = true;
              cargoArtifacts = workspaceCargoArtifacts;
              partitions = 1;
              partitionType = "count";
              buildInputs = darwinBuildInputs;
            };
          };

          # ── Packages ─────────────────────────────────────────────────
          packages = {
            inherit dyna-cli dyna-server dyna-wasm dyna-app dyna-py dyna-go dyna-app-serve;
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

              # Go (dyna-go)
              pkgs.go

              # Python (dyna-py)
              pkgs.python3
              pkgs.maturin
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
              echo "  ║    dyna-go          — Go client library                    ║"
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

            PKG_CONFIG_PATH = "${pkgs.openssl.dev}/lib/pkgconfig";
          };
        };
    };
}

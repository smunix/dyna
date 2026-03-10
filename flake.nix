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

          # ── Pinned wasm-bindgen-cli (must match Cargo.lock) ───────────
          wasm-bindgen-cli-0_2_111 = pkgs.buildWasmBindgenCli rec {
            src = pkgs.fetchCrate {
              pname = "wasm-bindgen-cli";
              version = "0.2.111";
              hash = "sha256-vCa7VIGmMB3baGQqhkd6r4XmUktt61ibcjDQtRW4PzA=";
            };
            cargoDeps = pkgs.rustPlatform.fetchCargoVendor {
              inherit src;
              inherit (src) pname version;
              hash = "sha256-Sl/AJXq4NSryKIXXo2Fjy6ybVxB8ezka8VQBBxbWPCw=";
            };
          };

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

          workspaceMembers = [ "dyna-core" "dyna-cli" "dyna-server" "dyna-wasm" "dyna-py" "lazy-cat" "lazy-wasm" ];

          # Parse a crate's Cargo.toml and extract workspace dependency names
          # that are also workspace members (i.e. local crate deps).
          readLocalDeps = cratePath:
            let
              cargoToml = builtins.fromTOML (builtins.readFile (cratePath + "/Cargo.toml"));
              allDeps = (cargoToml.dependencies or {})
                     // (cargoToml.dev-dependencies or {})
                     // (cargoToml.build-dependencies or {});
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
          # Returns a deduplicated list of crate directory paths.
          resolveAllLocalDeps = cratePath:
            let
              directNames = readLocalDeps cratePath;
              directPaths = map (name: ./. + "/${name}") directNames;
              transitive = builtins.concatMap resolveAllLocalDeps directPaths;
            in
              lib.lists.unique (directPaths ++ transitive);

          # ════════════════════════════════════════════════════════════════
          # Helper: compute the workspace member names present in a source
          # ════════════════════════════════════════════════════════════════
          #
          # Given a cratePath, returns the list of member names that are
          # actually included by mkCrateSrc (the crate itself + its
          # transitive local deps).

          presentMembersFor = cratePath:
            let
              crateName = builtins.baseNameOf (builtins.toString cratePath);
              depPaths = resolveAllLocalDeps cratePath;
              depNames = map (p: builtins.baseNameOf (builtins.toString p)) depPaths;
            in
              lib.lists.unique ([ crateName ] ++ depNames);

          # Generate a sed command that rewrites the workspace members list
          # in Cargo.toml to only include the given member names.
          patchWorkspaceMembersScript = members:
            let
              membersToml = builtins.concatStringsSep ", "
                (map (m: ''"${m}"'') members);
            in ''
              # Rewrite workspace members to only those present in the source.
              sed -i '/^members = \[/,/^\]/c\members = [${membersToml}]' Cargo.toml
              echo "Patched workspace members to: [${membersToml}]"
            '';

          # ════════════════════════════════════════════════════════════════
          # Helper: generate stub source files for missing workspace members
          # ════════════════════════════════════════════════════════════════
          #
          # When mkCrateSrc includes only Cargo.toml for members outside
          # the dependency tree, cargo still needs the source entry points
          # (src/lib.rs or src/main.rs) declared in those Cargo.toml files.
          # This helper reads each missing member's Cargo.toml at Nix eval
          # time and generates a postPatch script that creates empty stubs.

          stubSourcesFor = cratePath:
            let
              localDeps = resolveAllLocalDeps cratePath;
              presentNames = [ (builtins.baseNameOf (builtins.toString cratePath)) ]
                ++ map (p: builtins.baseNameOf (builtins.toString p)) localDeps;
              missingMembers = builtins.filter
                (name: !(builtins.elem name presentNames))
                workspaceMembers;

              # For each missing member, read its Cargo.toml and determine
              # which source files cargo expects.
              stubCommandsFor = name:
                let
                  toml = builtins.fromTOML (builtins.readFile (./. + "/${name}/Cargo.toml"));
                  hasLib = toml ? lib || (builtins.pathExists (./. + "/${name}/src/lib.rs"));
                  hasBin = toml ? bin || (builtins.pathExists (./. + "/${name}/src/main.rs"));
                  # Check for custom lib path
                  libPath = if toml ? lib && toml.lib ? path then toml.lib.path else "src/lib.rs";
                  # Check for custom bin paths
                  binPaths = if toml ? bin
                    then map (b: b.path or "src/main.rs") toml.bin
                    else if hasBin then [ "src/main.rs" ] else [];
                  allPaths = (if hasLib then [ libPath ] else []) ++ binPaths;
                in
                  builtins.concatStringsSep "\n" (map (p: ''
                    mkdir -p ${name}/$(dirname ${p})
                    touch ${name}/${p}
                  '') allPaths);
            in
              builtins.concatStringsSep "\n" (map stubCommandsFor missingMembers);

          # ════════════════════════════════════════════════════════════════
          # Helper: build filtered source tree for a crate
          # ════════════════════════════════════════════════════════════════

          # ── Patched crates referenced by [patch.crates-io] ──────────
          # These must be included in every filtered source tree so that
          # cargo can resolve the workspace-level patch table.
          patchedCratePaths = [ ./vfs-patch ];

          mkCrateSrc =
            { cratePath
            , extraPaths ? []
            , useCraneLib ? craneLib
            }:
            let
              localDeps = resolveAllLocalDeps cratePath;
              # Full source for the crate and its transitive local deps
              crateFilesets = map (p: useCraneLib.fileset.commonCargoSources p) ([ cratePath ] ++ localDeps);
              extraFilesets = map (p: p) extraPaths;
              # Include Cargo.toml for ALL workspace members so that
              # `cargo metadata` can always resolve the workspace.
              # Without this, cargo fails with "failed to load manifest
              # for workspace member" for members not in the dep tree.
              # We only include the Cargo.toml (not full sources) for
              # members outside the dependency tree, keeping the source
              # tree minimal.
              presentNames = [ (builtins.baseNameOf (builtins.toString cratePath)) ]
                ++ map (p: builtins.baseNameOf (builtins.toString p)) localDeps;
              missingMembers = builtins.filter
                (name: !(builtins.elem name presentNames))
                workspaceMembers;
              stubFilesets = map (name: ./. + "/${name}/Cargo.toml") missingMembers;
            in
              lib.fileset.toSource {
                root = ./.;
                fileset = lib.fileset.unions ([
                  ./Cargo.toml
                  ./Cargo.lock
                ] ++ crateFilesets ++ extraFilesets ++ stubFilesets
                  # Include patched crates from [patch.crates-io] so cargo
                  # can resolve them inside the Nix store.
                  ++ patchedCratePaths);
              };

          # ════════════════════════════════════════════════════════════════
          # Helper: build a native Rust package (deps cache + final build)
          # ════════════════════════════════════════════════════════════════

          mkRustPackage =
            { pname
            , cratePath
            , extraSrcPaths ? []
            , extraBuildInputs ? []
            , buildPackageArgs ? {}
            }:
            let
              src = mkCrateSrc {
                inherit cratePath;
                extraPaths = extraSrcPaths;
              };

              # Generate empty stub source files for workspace members
              # that are not in the dependency tree. mkCrateSrc includes
              # their Cargo.toml but cargo also needs the entry points.
              stubScript = stubSourcesFor cratePath;

              cargoArtifacts = craneLib.buildDepsOnly {
                pname = "${pname}-deps";
                inherit src;
                strictDeps = true;
                cargoExtraArgs = "-p ${pname}";
                buildInputs = darwinBuildInputs ++ extraBuildInputs;
                postPatch = stubScript;
              };

              package = craneLib.buildPackage ({
                inherit pname src cargoArtifacts;
                strictDeps = true;
                cargoExtraArgs = "-p ${pname}";
                doCheck = false;
                buildInputs = darwinBuildInputs ++ extraBuildInputs;
                postPatch = stubScript;
              } // buildPackageArgs);
            in
              { inherit src cargoArtifacts package; };

          # ════════════════════════════════════════════════════════════════
          # Helper: build a WASM package (deps cache + raw build + bindgen)
          # ════════════════════════════════════════════════════════════════

          mkWasmPackage =
            { pname
            , cratePath
            , wasmFileName
            , extraSrcPaths ? []
            , extraBuildInputs ? []
            , bindgenTarget ? "web"
            }:
            let
              wasmTarget = "wasm32-unknown-unknown";

              src = mkCrateSrc {
                inherit cratePath;
                extraPaths = extraSrcPaths;
                useCraneLib = craneLibWasm;
              };

              # Generate empty stub source files for workspace members
              # not in the dependency tree.
              stubScript = stubSourcesFor cratePath;

              cargoArtifacts = craneLibWasm.buildDepsOnly {
                pname = "${pname}-deps";
                inherit src;
                strictDeps = true;
                cargoExtraArgs = "-p ${pname} --target ${wasmTarget}";
                CARGO_BUILD_RUSTFLAGS = "--cfg getrandom_backend=\"wasm_js\"";
                doCheck = false;
                buildInputs = darwinBuildInputs ++ extraBuildInputs;
                postPatch = stubScript;
              };

              raw = craneLibWasm.buildPackage {
                inherit pname src cargoArtifacts;
                strictDeps = true;
                cargoExtraArgs = "-p ${pname} --target ${wasmTarget}";
                CARGO_BUILD_RUSTFLAGS = "--cfg getrandom_backend=\"wasm_js\"";
                doCheck = false;
                buildInputs = darwinBuildInputs ++ extraBuildInputs;
                postPatch = stubScript;
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
                nativeBuildInputs = [ wasm-bindgen-cli-0_2_111 ];
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
              workspaceMembers
              # Include patched crates from [patch.crates-io]
              ++ patchedCratePaths);
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

          serverJeResult = mkRustPackage {
            pname = "dyna-server";
            cratePath = ./dyna-server;
            buildPackageArgs = {
              cargoExtraArgs = "-p dyna-server --bin dyna-server-je";
            };
          };
          dyna-server-je = serverJeResult.package;

          serverMimResult = mkRustPackage {
            pname = "dyna-server";
            cratePath = ./dyna-server;
            buildPackageArgs = {
              cargoExtraArgs = "-p dyna-server --bin dyna-server-mim";
            };
          };
          dyna-server-mim = serverMimResult.package;

          # ── lazy-cat package ─────────────────────────────────────────

          lazyCatResult = mkRustPackage {
            pname = "lazy-cat";
            cratePath = ./lazy-cat;
          };
          lazy-cat = lazyCatResult.package;

          # ── lazy-go package ──────────────────────────────────────────────

          lazy-go = pkgs.buildGoModule {
            pname = "lazy-go";
            version = "0.1.0";
            src = lib.fileset.toSource {
              root = ./.;
              fileset = lib.fileset.unions [
                ./lazy-go
                ./dyna-go
              ];
            };
            vendorHash = null;
            modRoot = "lazy-go";
            subPackages = [ "lazycat" "examples/demo" ];
            buildPhase = ''
              runHook preBuild
              go build ./lazycat/...
              go build -o lazy-go-demo ./examples/demo
              runHook postBuild
            '';
            installPhase = ''
              runHook preInstall
              mkdir -p $out/bin $out/share/lazy-go
              cp lazy-go-demo $out/bin/lazy-go-demo
              cp -r . $out/share/lazy-go/
              runHook postInstall
            '';
            meta = {
              description = "Lazy, on-demand resource loader for Dyna servers (Go)";
            };
          };

          # ── WASM package ───────────────────────────────────────────────

          wasmResult = mkWasmPackage {
            pname = "dyna-wasm";
            cratePath = ./dyna-wasm;
            wasmFileName = "dyna_wasm";
          };
          dyna-wasm = wasmResult.package;

          # ── lazy-wasm WASM package ──────────────────────────────────────

          lazyWasmResult = mkWasmPackage {
            pname = "lazy-wasm";
            cratePath = ./lazy-wasm;
            wasmFileName = "lazy_wasm";
          };
          lazy-wasm = lazyWasmResult.package;

          # ── Elm package (dyna-app) ──────────────────────────────────
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

          # ── Elm package (lazy-elm-demo) ──────────────────────────────
          lazy-elm-demo = pkgs.stdenv.mkDerivation {
            pname = "lazy-elm-demo";
            version = "0.1.0";
            src = ./lazy-elm-demo;
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

              # Bundle the lazy-wasm WASM package alongside the Elm app
              mkdir -p $out/pkg
              cp ${lazy-wasm}/* $out/pkg/ 2>/dev/null || true
            '';
          };

          # ── Python package (dyna-py via maturin) ────────────────────
          dyna-py = let
            pyMembers = presentMembersFor ./dyna-py;
            pyPatchScript = patchWorkspaceMembersScript pyMembers;
          in pkgs.python3Packages.buildPythonPackage {
            pname = "dyna-py";
            version = "0.1.0";
            format = "pyproject";

            src = mkCrateSrc {
              cratePath = ./dyna-py;
              extraPaths = [
                ./dyna-py/pyproject.toml
                ./dyna-py/python
                ./dyna-py/README.md
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

            # Three fixups for the workspace-based maturin build:
            # 1. Copy Cargo.lock into the subcrate dir (cargoSetupHook
            #    validates it relative to cargoRoot).
            # 2. Rewrite workspace members to only those present in the
            #    filtered source (cargo metadata fails on missing members).
            postPatch = ''
              cp Cargo.lock dyna-py/
              ${pyPatchScript}
            '';

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

          # ── Python package (lazy-py) ──────────────────────────────────

          lazy-py = pkgs.python3Packages.buildPythonPackage {
            pname = "lazy-py";
            version = "0.1.0";
            format = "pyproject";

            src = ./lazy-py;

            build-system = [
              pkgs.python3Packages.setuptools
            ];

            dependencies = [
              dyna-py
              pkgs.python3Packages.websockets
            ];

            pythonImportsCheck = [ "lazy_py" ];

            meta = {
              description = "Lazy, on-demand resource loader for Dyna servers (Python)";
            };
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

          dyna-server-je-image = pkgs.dockerTools.buildLayeredImage {
            name = "dyna-server-je";
            tag = "latest";
            contents = [ dyna-server-je pkgs.cacert ];
            config = {
              Cmd = [ "${dyna-server-je}/bin/dyna-server-je" ];
              ExposedPorts = { "8080/tcp" = {}; };
              Env = [ "SSL_CERT_FILE=${pkgs.cacert}/etc/ssl/certs/ca-bundle.crt" ];
            };
          };

          dyna-server-mim-image = pkgs.dockerTools.buildLayeredImage {
            name = "dyna-server-mim";
            tag = "latest";
            contents = [ dyna-server-mim pkgs.cacert ];
            config = {
              Cmd = [ "${dyna-server-mim}/bin/dyna-server-mim" ];
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
                "--port" "3000"
              ];
              ExposedPorts = { "3000/tcp" = {}; };
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

          # ── lazy-elm-serve script ────────────────────────────────────
          lazy-elm-serve = pkgs.writeShellScriptBin "lazy-elm-serve" ''
            set -euo pipefail

            PORT="''${1:-3001}"

            # Locate the lazy-elm-demo source directory
            ELM_SRC="''${LAZY_ELM_SRC:-$(pwd)/lazy-elm-demo}"
            if [ ! -f "$ELM_SRC/elm.json" ]; then
              echo "Error: Cannot find lazy-elm-demo/elm.json"
              echo "Run this from the workspace root, or set LAZY_ELM_SRC."
              exit 1
            fi

            # Populate the WASM package (output goes into lazy-elm-demo/public/pkg)
            PKG_DIR="$ELM_SRC/public/pkg"
            mkdir -p "$PKG_DIR"
            if [ ! -f "$PKG_DIR/lazy_wasm.js" ]; then
              NIX_WASM="${lazy-wasm}"
              if [ -f "$NIX_WASM/lazy_wasm.js" ]; then
                echo "Copying pre-built lazy-wasm artifacts from Nix store..."
                cp -f "$NIX_WASM"/lazy_wasm* "$PKG_DIR/"
                echo "  WASM package ready."
              else
                echo "Building lazy-wasm from source..."
                if ${pkgs.wasm-pack}/bin/wasm-pack build lazy-wasm --target web --out-dir "$PKG_DIR" 2>&1; then
                  echo "  WASM package built successfully."
                else
                  echo "  Error: wasm-pack build failed and no pre-built artifacts available."
                  exit 1
                fi
              fi
            fi

            # Compile the Elm app
            echo "Compiling Elm app..."
            (cd "$ELM_SRC" && ${pkgs.elmPackages.elm}/bin/elm make src/Main.elm --optimize --output=public/elm.js)

            # Serve with correct MIME types
            echo ""
            echo "  Serving lazy-elm-demo on http://localhost:$PORT"
            echo "  Press Ctrl+C to stop."
            echo ""
            SERVE_DIR="$ELM_SRC/public" SERVE_PORT="$PORT" \
              ${pkgs.python3}/bin/python3 ${dyna-serve-py}
          '';

          dyna-app-serve = pkgs.writeShellScriptBin "dyna-app-serve" ''
            set -euo pipefail

            PORT="''${1:-3000}"

            # Locate the dyna-app source directory
            ELM_SRC="''${DYNA_ELM_SRC:-$(pwd)/dyna-app}"
            if [ ! -f "$ELM_SRC/elm.json" ]; then
              echo "Error: Cannot find dyna-app/elm.json"
              echo "Run this from the workspace root, or set DYNA_ELM_SRC."
              exit 1
            fi

            # Populate the WASM package (output goes into dyna-app/public/pkg)
            PKG_DIR="$ELM_SRC/public/pkg"
            mkdir -p "$PKG_DIR"
            if [ ! -f "$PKG_DIR/dyna_wasm.js" ]; then
              # Prefer the pre-built Nix artifacts (version-matched wasm-bindgen)
              NIX_WASM="${dyna-wasm}"
              if [ -f "$NIX_WASM/dyna_wasm.js" ]; then
                echo "Copying pre-built WASM artifacts from Nix store..."
                cp -f "$NIX_WASM"/dyna_wasm* "$PKG_DIR/"
                echo "  WASM package ready."
              else
                # Fallback: build from source with wasm-pack
                echo "Building dyna-wasm from source..."
                if ${pkgs.wasm-pack}/bin/wasm-pack build dyna-wasm --target web --out-dir "$PKG_DIR" 2>&1; then
                  echo "  WASM package built successfully."
                else
                  echo "  Error: wasm-pack build failed and no pre-built artifacts available."
                  echo "  Make sure wasm-pack and the wasm32-unknown-unknown target are available,"
                  echo "  or run 'nix build .#dyna-wasm' first."
                  exit 1
                fi
              fi
            fi

            # Compile the Elm app
            echo "Compiling Elm app..."
            (cd "$ELM_SRC" && ${pkgs.elmPackages.elm}/bin/elm make src/Main.elm --optimize --output=public/elm.js)

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
            inherit dyna-cli dyna-server dyna-server-je dyna-server-mim dyna-wasm lazy-wasm dyna-app lazy-elm-demo dyna-go lazy-cat lazy-go;

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
            inherit dyna-cli dyna-server dyna-server-je dyna-server-mim dyna-wasm lazy-wasm dyna-app lazy-elm-demo dyna-py dyna-go dyna-app-serve lazy-elm-serve lazy-cat lazy-go lazy-py;
            inherit dyna-server-image dyna-server-je-image dyna-server-mim-image dyna-app-image;
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
            dyna-server-je = {
              type = "app";
              program = "${dyna-server-je}/bin/dyna-server-je";
            };
            dyna-server-mim = {
              type = "app";
              program = "${dyna-server-mim}/bin/dyna-server-mim";
            };
            dyna-app-serve = {
              type = "app";
              program = "${dyna-app-serve}/bin/dyna-app-serve";
            };
            dyna-py = {
              type = "app";
              program = "${dyna-py}/bin/dyna-py";
            };
            lazy-cat-demo = {
              type = "app";
              program = "${lazy-cat}/bin/demo";
            };
            lazy-go-demo = {
              type = "app";
              program = "${lazy-go}/bin/lazy-go-demo";
            };
            lazy-py-demo = {
              type = "app";
              program = "${lazy-py}/bin/lazy-py-demo";
            };
            lazy-elm-serve = {
              type = "app";
              program = "${lazy-elm-serve}/bin/lazy-elm-serve";
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
              dyna-server-je
              dyna-server-mim
              dyna-app-serve
              lazy-elm-serve

              # Go (dyna-go)
              pkgs.go

              # Python (dyna-py)
              pkgs.python3
              pkgs.maturin
              pkgs.python3Packages.click
              dyna-py
              lazy-py
              lazy-cat

              # WASM tooling
              pkgs.wasm-pack
              wasm-bindgen-cli-0_2_111

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
              echo "  ║    dyna-server      — start the server (default alloc)   ║"
              echo "  ║    dyna-server-je   — start the server (jemalloc)         ║"
              echo "  ║    dyna-server-mim  — start the server (mimalloc)         ║"
              echo "  ║    dyna-app-serve   — build & serve the Elm UI           ║"
              echo "  ║    dyna-py          — Python CLI (via PyO3)               ║"
              echo "  ║    dyna-go          — Go client library                    ║"
              echo "  ║    lazy-cat         — Rust lazy resource loader             ║"
              echo "  ║    lazy-go          — Go lazy resource loader               ║"
              echo "  ║    lazy-py          — Python lazy resource loader            ║"
              echo "  ║    lazy-wasm        — WASM lazy resource loader              ║"
              echo "  ║    lazy-elm-serve   — build & serve the Elm lazy UI       ║"
              echo "  ║                                                          ║"
              echo "  ║  Demo apps (nix run .#<name>):                           ║"
              echo "  ║    lazy-cat-demo    — Rust lazy loader demo               ║"
              echo "  ║    lazy-go-demo     — Go lazy loader demo                 ║"
              echo "  ║    lazy-py-demo     — Python lazy loader demo             ║"
              echo "  ║    lazy-elm-serve   — Elm lazy loader UI demo               ║"
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

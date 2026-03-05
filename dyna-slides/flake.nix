{
  description = "Dyna presentation — Quarto Reveal.js slides built with Nix";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-24.11";
    flake-utils.url = "github:numtide/flake-utils";
  };

  outputs = { self, nixpkgs, flake-utils }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        pkgs = import nixpkgs { inherit system; };

        # Quarto Reveal.js slide build
        dyna-slides = pkgs.stdenvNoCC.mkDerivation {
          pname = "dyna-slides";
          version = "1.0.0";

          src = ./.;

          nativeBuildInputs = with pkgs; [
            quarto
            chromium          # for mermaid diagram rendering
          ];

          # Quarto needs a writable home for its cache
          buildPhase = ''
            export HOME=$(mktemp -d)
            export CHROMIUM_FLAGS="--no-sandbox"
            export PUPPETEER_EXECUTABLE_PATH="${pkgs.chromium}/bin/chromium"

            # Render the Reveal.js presentation
            quarto render dyna.qmd --to revealjs
          '';

          installPhase = ''
            mkdir -p $out
            cp -r dyna.html $out/index.html
            cp -r dyna_files $out/dyna_files 2>/dev/null || true
            cp dyna-theme.scss $out/ 2>/dev/null || true

            # Also render PDF if possible
            quarto render dyna.qmd --to pdf 2>/dev/null && cp dyna.pdf $out/ || true
          '';

          meta = with pkgs.lib; {
            description = "Dyna — Distributed Version Control for Resource Catalogs (Reveal.js slides)";
            license = licenses.mit;
          };
        };

        # Simple HTTP server for serving the slides
        serve-slides = pkgs.writeShellScriptBin "dyna-slides-serve" ''
          echo "Serving Dyna slides at http://localhost:''${1:-8080}"
          ${pkgs.python3}/bin/python3 -m http.server ''${1:-8080} -d ${dyna-slides}
        '';

      in {
        packages = {
          default = dyna-slides;
          inherit dyna-slides serve-slides;
        };

        apps = {
          default = {
            type = "app";
            program = "${serve-slides}/bin/dyna-slides-serve";
          };
          serve = {
            type = "app";
            program = "${serve-slides}/bin/dyna-slides-serve";
          };
        };

        devShells.default = pkgs.mkShell {
          buildInputs = with pkgs; [
            quarto
            chromium
          ];
          shellHook = ''
            echo "Dyna slides dev shell"
            echo "  quarto preview dyna.qmd   — live preview"
            echo "  quarto render dyna.qmd    — build HTML"
            echo "  nix run                   — serve built slides"
          '';
        };
      }
    );
}

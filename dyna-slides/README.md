# Dyna Slides

**Quarto Reveal.js presentation for the Dyna project** — a distributed version control system for JSON resource catalogs.

## Overview

This directory contains a 15-slide executive presentation built with [Quarto](https://quarto.org/) and [Reveal.js](https://revealjs.com/). The presentation covers the Dyna architecture, data model, lazy loading ecosystem, real-time collaboration, and roadmap.

## Slide Outline

| # | Title | Content |
|---|-------|---------|
| 1 | Title | Dyna introduction and tagline |
| 2 | SDMF Scale | 40,000+ resources, namespace breakdown |
| 3 | Pain Points | Startup latency, no history, no collaboration |
| 4 | Vision | Git-like version control for JSON |
| 5 | Data Model | Resource → Patch → Changeset → Channel |
| 6 | Architecture | Five-language client ecosystem |
| 7 | Server Actors | elfo-rs actors with S3 persistence |
| 8 | Lazy Loading | Minutes to milliseconds startup |
| 9 | Lazy Ecosystem | Rust, Go, Python, WASM lazy loaders |
| 10 | Real-Time | WebSocket collaboration flow |
| 11 | Browser UI | Elm + WASM applications |
| 12 | Nix Builds | Single flake, 24+ packages |
| 13 | Impact | Performance metrics and transformation |
| 14 | Roadmap | Near-term and long-term plans |
| 15 | Thank You | Closing slide |

## Building

### With Nix (recommended)

```bash
# Build the HTML presentation
nix build .#dyna-slides

# Serve locally
nix run .#serve

# Enter dev shell for live preview
nix develop
quarto preview dyna.qmd
```

### Without Nix

```bash
# Install Quarto: https://quarto.org/docs/get-started/
quarto render dyna.qmd --to revealjs
# Open dyna.html in a browser
```

## Theme

The presentation uses a custom **Easel** theme (`dyna-theme.scss`) with:

- **Warm cream** backgrounds (`#faf6f0`)
- **Burnt orange** accents (`#c25e30`)
- **Teal** secondary colour (`#2a7a72`)
- **Playfair Display** headings + **DM Sans** body text
- Mermaid diagrams with neutral theme

## Files

| File | Purpose |
|------|---------|
| `dyna.qmd` | Quarto source document |
| `dyna-theme.scss` | Custom Reveal.js SCSS theme |
| `flake.nix` | Nix flake for building and serving |
| `README.md` | This file |

## Exporting

```bash
# PDF (requires chromium or a TeX installation)
quarto render dyna.qmd --to pdf

# PowerPoint
quarto render dyna.qmd --to pptx
```

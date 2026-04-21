# Rustory

Local-first desktop companion for Lunii and other supported devices.

## Stack

- Desktop runtime: [Tauri v2](https://tauri.app/)
- UI: React 19 + TypeScript (Vite)
- Core: Rust (source of truth for domain, persistence, device access)
- Tooling: pnpm for the frontend, Cargo for the Rust core

## Prerequisites

- Node.js ≥ 20 and [pnpm](https://pnpm.io/) (project uses `pnpm-lock.yaml`)
- Rust stable ≥ 1.95 on the host for `pnpm tauri dev` — the Tauri v2 dependency
  tree requires the `edition2024` feature. The Docker dev image pins the same
  version (`rust:1.95-bookworm`, see `docker/Dockerfile.rust-dev`).
- Docker + Docker Compose (optional — used to run Rust tests without touching host GTK/WebKitGTK libs)

## Getting started

Install the frontend dependencies and launch the desktop app:

```bash
pnpm install
pnpm tauri dev
```

## Commands

```bash
# Frontend
pnpm test          # Vitest (unit + component + IPC contract tests)
pnpm typecheck     # TypeScript strict check
pnpm build         # Production build of the frontend bundle

# Tauri
pnpm tauri dev     # Launch the desktop app in dev mode
pnpm tauri build   # Produce a signed desktop bundle (see release docs)
```

## Rust tests via Docker

Compiling the `src-tauri` crate requires the WebKitGTK / GTK / libsoup / rsvg /
appindicator development headers on Linux. To avoid polluting contributor
hosts, a `rust-dev` Compose service provides a reproducible Rust + Tauri build
environment.

```bash
# Build the image once
docker compose build rust-dev

# Run all Rust tests (unit + integration + contract)
docker compose run --rm rust-dev cargo test

# Quick type-check only
docker compose run --rm rust-dev cargo check --tests
```

Cargo caches (`registry/`, `git/`, `target/`) live in named volumes so rebuilds
stay fast.

## Project layout

- `src/` — React + TypeScript shell (routes, features, shell, IPC facade)
- `src-tauri/` — Rust core (commands, application, domain, infrastructure, IPC)
- `docs/architecture/` — canonical product language and UI-state vocabulary
- `tests/e2e/` — reserved for desktop end-to-end tests

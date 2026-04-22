# Rustory

Local-first desktop companion for Lunii and other supported devices.

## Stack

- Desktop runtime: [Tauri v2](https://tauri.app/)
- UI: React 19 + TypeScript (Vite)
- Core: Rust (source of truth for domain, persistence, device access)
- Tooling: pnpm for the frontend, Cargo for the Rust core

## Prerequisites

- **Required**
  - Node.js ≥ 20 and [pnpm](https://pnpm.io/) on the host (project uses
    `pnpm-lock.yaml`, pinned via `package.json#packageManager`).
  - Docker + Docker Compose — reproducible containers for Rust verification
    commands and for running the desktop app over X11 without polluting the
    host with GTK / WebKitGTK headers.
- **Optional (only if you want to run `pnpm tauri dev` natively on the host)**
  - Rust stable ≥ 1.95 (Tauri v2 needs `edition2024`). The `tauri-dev` image
    pins the same version (`rust:1.95-bookworm`).
  - Native Linux build deps: `pkg-config libwebkit2gtk-4.1-dev
    libjavascriptcoregtk-4.1-dev libglib2.0-dev libgtk-3-dev librsvg2-dev
    libayatana-appindicator3-dev libxdo-dev libssl-dev build-essential`.

## Getting started

Install the frontend dependencies, then launch the desktop app through the
X11-enabled Docker service (no host GTK/WebKitGTK needed):

```bash
pnpm install
pnpm tauri:dev:docker
```

The `tauri:dev:docker` script handles the X11 authorisation dance for you:
it runs `xhost +SI:localuser:root` before starting the container and revokes
the grant on exit (success or failure) so the host never stays permissive.
If you have the native build deps installed on the host and prefer a direct
run:

```bash
pnpm tauri dev
```

## Commands

```bash
# Frontend (host)
pnpm test          # Vitest (unit + component + IPC contract tests)
pnpm typecheck     # TypeScript strict check
pnpm build         # Production build of the frontend bundle

# Tauri
pnpm tauri:dev:docker
                   # Launch the desktop app through Docker/X11 on Linux
pnpm tauri dev     # Launch natively (requires host GTK/WebKitGTK)
pnpm tauri build   # Produce a local desktop bundle — manual delivery only
                   # for now, see docs/release-runbook.md
```

## Tauri and Rust via Docker

Compiling the `src-tauri` crate requires the WebKitGTK / GTK / libsoup / rsvg /
appindicator development headers on Linux. To avoid polluting contributor
hosts, [`compose.yaml`](compose.yaml) declares two reproducible services:

- `rust-dev` — Rust/Cargo only, used by local verification commands
  (`cargo check`, `cargo test`, `cargo fmt`, `cargo clippy`).
- `tauri-dev` — `rust-dev` plus Node.js 20 + pnpm, used by `pnpm tauri dev`
  with X11 forwarding (`/tmp/.X11-unix` bind-mount) and `/dev/dri` GPU
  access.

```bash
# Build the Cargo-only image once
docker compose build rust-dev

# Run all Rust tests (unit + integration + contract)
docker compose run --rm rust-dev cargo test

# Quick type-check only
docker compose run --rm rust-dev cargo check --tests

# Build and launch the GUI dev image (pnpm tauri:dev:docker wraps the
# xhost +/- dance around the `docker compose run` call)
docker compose build tauri-dev
pnpm tauri:dev:docker
```

Cargo caches (`registry/`, `git/`, `target/`) live in named volumes so rebuilds
stay fast. `node_modules/` is shared from the host through the `.:/workspace`
bind-mount — run `pnpm install` once on the host before the first GUI launch.

## Continuous verification

Every push or pull request to `main` runs [`.github/workflows/verify.yml`](.github/workflows/verify.yml),
which exercises the frontend (`pnpm install --frozen-lockfile`, `pnpm typecheck`,
`pnpm test`, `pnpm build`) and the Rust crate (`cargo fmt --check`, `cargo clippy
-- -D warnings`, `cargo test`) in parallel on `ubuntu-24.04`.

Reproduce the same checks locally before pushing:

```bash
pnpm install --frozen-lockfile && pnpm typecheck && pnpm test && pnpm build
docker compose run --rm rust-dev cargo fmt --all -- --check
docker compose run --rm rust-dev cargo clippy --all-targets --all-features -- -D warnings
docker compose run --rm rust-dev cargo test --all-targets
```

A CI-red / local-green divergence is a reproducibility incident — investigate,
don't bypass.

## Delivery

Rustory is on **manual delivery** for now: no automated release workflow, no
Tauri updater, no signed multi-OS pipeline yet. Produce a build with
`pnpm tauri build` on the target platform and distribute the artifacts
off-CI. See [`docs/release-runbook.md`](docs/release-runbook.md) for the full
posture and the four exit criteria that will unlock automated delivery.

## Contributing

See [`CONTRIBUTING.md`](CONTRIBUTING.md) for the verification pipeline,
IPC-contract hygiene, product-language rules and the one-commit-per-unit-of-work
workflow. [`.github/pull_request_template.md`](.github/pull_request_template.md)
pre-loads the pull-request checklist.

## Project layout

- `src/` — React + TypeScript shell (routes, features, shell, IPC facade)
- `src-tauri/` — Rust core (commands, application, domain, infrastructure, IPC)
- `docs/architecture/` — canonical product language and UI-state vocabulary
- `tests/e2e/` — reserved for desktop end-to-end tests

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
pnpm tauri build   # Produce a local desktop bundle — manual delivery only
                   # for now, see docs/release-runbook.md
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

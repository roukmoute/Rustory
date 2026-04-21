# Contributing to Rustory

Ce guide complète [`docs/project-context.md`](docs/project-context.md) avec les conventions opérationnelles utilisées à chaque contribution.

## Pipeline de vérification

Chaque `push` ou `pull_request` vers `main` déclenche [`.github/workflows/verify.yml`](.github/workflows/verify.yml) qui exécute en parallèle :

- **Frontend** — `pnpm install --frozen-lockfile`, `pnpm typecheck`, `pnpm test`, `pnpm build`
- **Rust** — `cargo fmt --all -- --check`, `cargo clippy --all-targets --all-features -- -D warnings`, `cargo test --all-targets`

Commandes locales strictement équivalentes avant de pousser :

```bash
# Frontend (sur l'hôte)
pnpm install --frozen-lockfile
pnpm typecheck
pnpm test
pnpm build

# Rust (via le container reproductible — voir README.md)
docker compose run --rm rust-dev cargo fmt --all -- --check
docker compose run --rm rust-dev cargo clippy --all-targets --all-features -- -D warnings
docker compose run --rm rust-dev cargo test --all-targets
```

La CI est conçue pour refléter ces commandes à l'identique. Si la CI rouge et le local vert divergent, c'est un incident de reproductibilité — à investiguer, pas à contourner.

## Contrats IPC

Toute modification d'un DTO ou d'un code d'erreur IPC passe par trois points **simultanés** :

1. Le miroir Rust dans [`src-tauri/src/ipc/dto/`](src-tauri/src/ipc/dto/) ou [`src-tauri/src/domain/shared/error.rs`](src-tauri/src/domain/shared/error.rs)
2. Le miroir TypeScript dans [`src/shared/ipc-contracts/`](src/shared/ipc-contracts/) ou [`src/shared/errors/app-error.ts`](src/shared/errors/app-error.ts)
3. Un test de contrat de **chaque** côté : [`src-tauri/tests/contracts/`](src-tauri/tests/contracts/) et [`src/ipc/contract-tests/`](src/ipc/contract-tests/)

La conversion `snake_case ↔ camelCase` se fait uniquement au boundary via `serde(rename_all = "camelCase")` côté Rust (`#[serde(rename_all = "camelCase")]` sur chaque DTO). Aucune conversion ad hoc ailleurs, aucun champ renommé à la main côté TypeScript.

## Vocabulaire produit et états UI

Avant d'ajouter une chaîne user-facing ou un nouvel état visible :

- Consulter [`docs/architecture/product-language.md`](docs/architecture/product-language.md) pour le terme canonique (`bibliothèque`, `histoire`, `appareil`, `préparation`, `transfert`, `vérification`).
- Consulter [`docs/architecture/ui-states.md`](docs/architecture/ui-states.md) pour l'état et son libellé (`brouillon local`, `en vérification`, `bloquée`, `transférée et vérifiée`, etc.).

Règle : un concept produit stable → un libellé préféré. Pas de synonyme libre. Si un nouveau terme s'impose, on met à jour ces documents **d'abord**, puis on code.

## Livraison

Rustory est en **livraison manuelle** tant que la chaîne officielle n'est pas en place. Voir [`docs/release-runbook.md`](docs/release-runbook.md) pour la procédure et les critères de passage à la livraison automatisée.

## Workflow de livraison

- Une unité de travail = **un seul commit** Git. Si la CI GitHub échoue, corriger localement puis `git commit --amend --no-edit` (cf. [`docs/project-context.md`](docs/project-context.md)).
- Le commit n'est prêt à revue que si (a) les tests locaux pertinents sont verts et (b) la CI GitHub est verte sur ce commit.

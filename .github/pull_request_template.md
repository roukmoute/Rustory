## Résumé

<!-- 1–3 phrases : quoi et pourquoi. Pas de description exhaustive, le diff parle déjà. -->

## Contexte

<!-- Lien vers l'issue GitHub, la discussion, ou un court rappel de l'intention. Optionnel si le résumé suffit. -->

## Checklist obligatoire

- [ ] `pnpm install --frozen-lockfile && pnpm typecheck && pnpm test && pnpm build` vert localement
- [ ] `cargo fmt --all -- --check && cargo clippy --all-targets --all-features -- -D warnings && cargo test --all-targets` vert localement (au besoin via `docker compose run --rm rust-dev …`)
- [ ] Tout DTO IPC modifié a un test de contrat côté Rust (`src-tauri/tests/contracts/`) **et** côté frontend (`src/ipc/contract-tests/`) à jour
- [ ] Toute chaîne user-facing respecte [`docs/architecture/product-language.md`](../docs/architecture/product-language.md) — pas de `workspace`, `pipeline`, `nouveau projet`, `payload`, `build` comme terme produit
- [ ] Tout nouvel état visible respecte [`docs/architecture/ui-states.md`](../docs/architecture/ui-states.md)
- [ ] **Un seul commit par unité de travail** (cf. [`docs/project-context.md`](../docs/project-context.md)) — si la CI demande un correctif, `git commit --amend` sur le commit existant

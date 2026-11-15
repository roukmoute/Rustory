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

### Hook pre-commit (formatage Rust)

Un hook `pre-commit` versionné dans [`.githooks/`](.githooks/) refuse tout commit dont les fichiers `.rs` **indexés** ne sont pas formatés par rustfmt — le même verrou que `cargo fmt --all -- --check` en CI, mais vérifié sur le contenu qui sera réellement commité (un `git add -p` partiel est donc contrôlé tel quel, indépendamment de l'arbre de travail).

- Activation : `pnpm install` la fait automatiquement (script `prepare` → `git config core.hooksPath .githooks`). À la main : `git config core.hooksPath .githooks`.
- Prérequis : `rustfmt` sur l'hôte (`rustup component add rustfmt`). Sans lui, le hook refuse le commit (fail-closed) et indique la vérification équivalente via Docker.
- En cas de refus : `cargo fmt --all --manifest-path src-tauri/Cargo.toml`, puis réindexer les fichiers listés.
- Contournement délibéré et assumé : `git commit --no-verify` — la CI reste le juge final.

## Aperçu visuel dans le navigateur

Après `pnpm dev`, ouvrir `http://localhost:1420/design-preview.html` pour
contrôler l'interface sans Tauri ni appareil. Cette entrée de développement
utilise des données de démonstration en mémoire et n'est pas incluse dans
la compilation de production.

Variantes disponibles : `?scenario=empty`, `?scenario=connected`,
`?screen=editor` et `?screen=settings`. Pour recommencer après un rechargement,
revenir à l'URL `design-preview.html` : les routes normales nécessitent le
backend Tauri. Les opérations non simulées échouent explicitement ; cet aperçu
ne remplace pas les tests sur appareil.

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

La chaîne officielle de livraison est **active** : `build-release.yml` construit et signe les artefacts des trois plateformes, puis `promote-release.yml` publie la release après contrôle. Voir [`docs/release-runbook.md`](docs/release-runbook.md) pour la procédure. Chaque livraison doit aboutir à une nouvelle version, un tag poussé et une release publique ; laisser une draft ne termine pas la livraison.

## Workflow de livraison

- Une intention autonome = **un commit atomique**. Séparer les changements indépendants ; une livraison peut comporter plusieurs commits.
- Après les vérifications locales, pousser directement sur `main`, sans PR pour le moment, conformément à la consigne de l'utilisateur.
- Réserver `git commit --amend` aux commits qui n'ont pas encore été poussés. Si la CI échoue après publication du commit, ajouter un correctif atomique et relancer les vérifications ; ne jamais forcer le push de `main`.
- Incrémenter la version à chaque livraison et synchroniser dans un même commit `package.json`, `src-tauri/Cargo.toml`, l'entrée `rustory` de `src-tauri/Cargo.lock` et `src-tauri/tauri.conf.json`.
- Après les vérifications locales, pousser le commit sur `main` et son tag `vMAJOR.MINOR.PATCH`. Attendre la réussite de `Verify` et `Build Release` sur ce même commit, contrôler les notes, les artefacts, les signatures et `latest.json`, puis lancer `promote-release.yml` et vérifier la publication effective.
- L'autorisation explicite de l'utilisateur couvre le push et la publication à chaque livraison : les contrôles restent obligatoires, sans nouvelle demande d'accord systématique. Si la version est déjà publique, toute correction nécessite une nouvelle version, un nouveau tag et une nouvelle release.

La politique complète est consignée dans [`docs/project-context.md`](docs/project-context.md).

## Convention de versioning

Les versions de release suivent la convention stricte `MAJOR.MINOR.PATCH` :
- choisir l'incrément SemVer adapté : `PATCH` pour une correction compatible, `MINOR` pour une fonctionnalité compatible, `MAJOR` pour une rupture de compatibilité, en tenant compte du stade `0.x`,
- exactement **trois composantes** séparées par des points (`.`),
- chaque composante est un **entier en chiffres ASCII** (0-9), **zéros de tête refusés** (ex. `01`, `007` invalides),
- un préfixe `v` est **toléré au parse** mais **jamais affiché à l'utilisateur**,
- le parsing est **fail-closed** : un tag hors convention ne produit **JAMAIS** de verdict de mise à jour.

Voir [`src-tauri/src/domain/update/availability.rs`](src-tauri/src/domain/update/availability.rs) pour l'implémentation.



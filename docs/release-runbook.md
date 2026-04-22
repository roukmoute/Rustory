# Release Runbook

## Purpose

Ce document est la référence opérationnelle pour livrer des builds Rustory tant que la chaîne officielle de release (CI multi-OS, signing, feed updater, promotion) n'est pas encore en place. Il matérialise la posture actuelle du socle et définit les critères précis pour en sortir.

## Current posture — manual delivery only

À cette étape :

- **Aucun workflow GitHub Actions `build-release.yml`** et aucun `promote-release.yml` n'existent dans le repo. Seul [`.github/workflows/verify.yml`](../.github/workflows/verify.yml) tourne à chaque `push` / `pull_request` pour valider frontend + Rust.
- **Aucun bloc `plugins.updater`** n'est déclaré dans [`src-tauri/tauri.conf.json`](../src-tauri/tauri.conf.json). Les clients existants ne cherchent pas de feed de mise à jour.
- **Aucune dépendance `tauri-plugin-updater`** n'est présente dans [`src-tauri/Cargo.toml`](../src-tauri/Cargo.toml). Aucune clé de signature n'est attendue par le binaire.

Conséquence : publier un build = fournir un binaire produit localement à l'utilisateur final. Pas de mécanisme automatique de distribution ni de mise à jour.

## Manual build procedure

Exécuter **sur chaque plateforme cible** (Windows, macOS, Linux) :

```bash
# Depuis la racine du repo, sur une checkout `main` propre
pnpm install --frozen-lockfile
pnpm tauri build
```

Les artefacts sortent dans `src-tauri/target/release/bundle/` :

- Linux : AppImage dans `bundle/appimage/`, `.deb` dans `bundle/deb/`
- macOS : `.app` dans `bundle/macos/`, `.dmg` dans `bundle/dmg/`
- Windows : `.msi` dans `bundle/msi/`, `.exe` dans `bundle/nsis/`

Distribution hors CI : upload manuel vers le canal privé convenu avec l'utilisateur (lien direct, partage de fichier). Aucune publication automatique, aucun enregistrement de release GitHub tant que les critères de sortie ci-dessous ne sont pas remplis.

## Why manual for now

Deux invariants interdisent de publier un updater tant qu'ils ne sont pas tenus : les builds officiellement distribués doivent être reproductibles depuis la CI, et les artefacts de mise à jour doivent être signés. Tant que ces deux garanties ne sont pas établies par une chaîne CI signée et une politique de promotion explicite, publier un binaire via un updater serait **plus risqué** que ne pas publier du tout : un utilisateur qui a installé une build manuelle sait qu'il l'a fait manuellement et ne s'attend pas à une mise à jour silencieuse.

La posture manuelle est donc un choix conservateur : si la chaîne de confiance ne peut pas être établie pour une cible, retomber sur la distribution manuelle plutôt que d'embarquer un chemin updater non validé.

## Exit criteria — when to switch to automated delivery

Les quatre éléments ci-dessous relèvent d'un futur travail de release hardening. La posture manuelle reste de rigueur tant que les quatre ne sont pas livrés :

1. **Secrets signing provisionnés** dans les GitHub Actions secrets du repo :
   - `TAURI_SIGNING_PRIVATE_KEY` (clé privée Ed25519 du updater Tauri)
   - `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` (passphrase associée)

   Perte = incident opérationnel critique (pas de rollback possible).

2. **Workflow `.github/workflows/build-release.yml`** avec :
   - matrice `runs-on: [windows-latest, macos-latest, ubuntu-latest]`
   - builds signés produisant des artefacts `.msi`, `.dmg`, `AppImage`
   - upload GitHub Release avec assets attachés
   - génération du fichier `updater.json` (ou équivalent manifest statique)

3. **Workflow `.github/workflows/promote-release.yml`** pour la publication contrôlée du feed updater :
   - déclenchement manuel (`workflow_dispatch`), jamais automatique
   - promotion d'une release candidate vers le canal `stable`
   - publication du manifest updater uniquement après revue humaine explicite

4. **Document `docs/update-signing.md`** décrivant :
   - procédure de génération initiale de la paire de clés
   - procédure de rotation en cas de compromission
   - chaîne de responsabilité et accès aux secrets
   - procédure de récupération en cas de perte (ou pourquoi il n'y en a pas, selon le modèle de confiance choisi)

Quand les quatre sont livrés, mettre à jour cette page : supprimer la section « Manual build procedure » au profit d'un renvoi vers `build-release.yml`, et déplacer la section « Manual posture » dans un encart historique.

## Local persistence footprint

La persistance locale du brouillon utilisateur repose sur SQLite embarqué (via `rusqlite` compilé en `bundled`). Le fichier `rustory.sqlite` est créé et lu dans le répertoire `app_data_dir` résolu par Tauri v2, spécifique à chaque plateforme. Les migrations SQL vivent dans [`src-tauri/migrations/`](../src-tauri/migrations/) et sont appliquées à chaque démarrage, en mode idempotent via une table `schema_migrations`.

Le mode journal `WAL` est activé au premier `open_at`, ce qui produit deux fichiers annexes au voisinage de `rustory.sqlite` : `rustory.sqlite-wal` et `rustory.sqlite-shm`. Ces trois fichiers font partie de l'état local de l'application et doivent rester strictement dans `app_data_dir` — jamais dans le repo, jamais dans un dossier partagé entre utilisateurs.

## Failure-mode guardrails

Même en livraison manuelle, ne **jamais** :

- publier un binaire non issu d'une checkout `main` propre et vérifiée localement
- promouvoir une CI verte comme « release ready » — `verify.yml` valide la compilation et les tests, pas la signature ni la reproductibilité release
- activer un feed updater partiel pointant vers un sous-ensemble de plateformes (donnerait l'illusion d'une couverture complète)
- distribuer un build dont les vérifications locales ne sont pas toutes vertes

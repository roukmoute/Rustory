# Release Runbook

## Purpose

Ce document est la référence opérationnelle pour livrer des builds Rustory. La chaîne officielle de release (CI multi-OS, signing, feed updater, promotion) est désormais OUTILLÉE dans le repo, mais elle n'est PAS ENCORE ACTIVE : son activation est un acte opérateur (provisionner les secrets, pousser un tag, promouvoir une release). Ce document matérialise la posture courante et le chemin d'activation.

## Current posture — manual delivery only (still)

À cette étape :

- **Les workflows GitHub Actions [`build-release.yml`](../.github/workflows/build-release.yml) et [`promote-release.yml`](../.github/workflows/promote-release.yml) EXISTENT** mais n'ont jamais été exercés : aucun secret de signature n'est provisionné, aucun tag `v*` n'a été poussé, aucune release n'a été construite ni promue. `build-release.yml` échoue proprement (fail-closed) tant que les secrets manquent — aucune release non signée ne peut naître. [`verify.yml`](../.github/workflows/verify.yml) reste le seul workflow qui tourne à chaque `push` / `pull_request`.
- **Le bloc `plugins.updater` de [`src-tauri/tauri.conf.json`](../src-tauri/tauri.conf.json) est une COQUILLE NEUTRE** (`{"pubkey": ""}`) — une exception technique assumée : la crate `tauri-plugin-updater` exige une configuration désérialisable à l'enregistrement du plugin (champ `pubkey` requis ; bloc absent = échec au démarrage). Cette coquille ne configure RIEN de réel : la configuration effective du mécanisme est entièrement runtime (endpoint constant + clé publique lue à la compilation, une pubkey vide = chaîne non configurée, fail-closed), et un test de contrat verrouille la coquille dans cet état neutre (jamais d'endpoint statique, jamais de flag dangereux). `createUpdaterArtifacts` n'est PAS committé : la production des artefacts updater n'est activée que par l'overlay de configuration du workflow de release. Conséquence directe : le build local ci-dessous reste possible sans aucune clé.
- **La dépendance `tauri-plugin-updater` est présente** dans [`src-tauri/Cargo.toml`](../src-tauri/Cargo.toml) (`default-features = false`, `features = ["native-tls", "zip"]` — la contrainte TLS de la CI s'applique au plugin comme au reste). Le geste intégré de mise à jour (voir le contrat d'application de mise à jour, [architecture/device-support-profile.md](architecture/device-support-profile.md)) est FAIL-CLOSED : sans clé publique embarquée à la compilation (`RUSTORY_UPDATER_PUBKEY`), toute copie distribuée retombe sur la guidance manuelle. Les clients distribués continuent par ailleurs de CONSULTER la disponibilité d'une version publiée — une lecture seule, bornée, de l'API publique des versions du repo (`releases/latest`), sans aucune action (contrat de disponibilité de mise à jour, inchangé).

Conséquence : publier un build = fournir un binaire produit localement à l'utilisateur final, tant que les conditions d'activation ci-dessous ne sont pas remplies. La posture manuelle RESTE la posture courante tant que : (a) les secrets `TAURI_SIGNING_PRIVATE_KEY` / `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` et la variable `RUSTORY_UPDATER_PUBKEY` ne sont pas provisionnés, ET (b) aucune release n'a été construite ET promue par les workflows.

## Manual build procedure

Exécuter **sur chaque plateforme cible** (Windows, macOS, Linux) :

```bash
# Depuis la racine du repo, sur une checkout `main` propre
pnpm install --frozen-lockfile
pnpm tauri build
```

Les artefacts sortent dans `src-tauri/target/release/bundle/` :

- Linux : AppImage dans `bundle/appimage/`, `.deb` dans `bundle/deb/`, `.rpm` dans `bundle/rpm/`
- macOS : `.app` dans `bundle/macos/`, `.dmg` dans `bundle/dmg/`
- Windows : `.msi` dans `bundle/msi/`, `.exe` dans `bundle/nsis/`

Distribution hors CI : upload manuel vers le canal privé convenu avec l'utilisateur (lien direct, partage de fichier). Aucune publication automatique, aucun enregistrement de release GitHub tant que les critères de sortie ci-dessous ne sont pas remplis.

## Why manual for now

Deux invariants interdisent de publier un updater tant qu'ils ne sont pas tenus : les builds officiellement distribués doivent être reproductibles depuis la CI, et les artefacts de mise à jour doivent être signés. Tant que ces deux garanties ne sont pas établies par une chaîne CI signée exercée au moins une fois et une promotion humaine effective, publier un binaire via un updater serait **plus risqué** que ne pas publier du tout : un utilisateur qui a installé une build manuelle sait qu'il l'a fait manuellement et ne s'attend pas à une mise à jour silencieuse.

La posture manuelle est donc un choix conservateur : si la chaîne de confiance ne peut pas être établie pour une cible, retomber sur la distribution manuelle plutôt que d'embarquer un chemin updater non validé. Le geste intégré embarqué dans le binaire respecte ce choix par construction : sans clé publique embarquée, il retombe sur la guidance manuelle.

## Exit criteria — state of delivery

Trois des quatre critères de sortie sont MATÉRIALISÉS dans le repo ; le premier est un acte opérateur documenté, jamais exécuté par le code :

1. **Secrets signing provisionnés** — À FAIRE (acte opérateur) :
   - `TAURI_SIGNING_PRIVATE_KEY` (clé privée Ed25519 du updater Tauri) et `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` (passphrase associée) dans les GitHub Actions **secrets** du repo
   - `RUSTORY_UPDATER_PUBKEY` (clé publique correspondante) dans les GitHub Actions **variables** du repo — embarquée dans les binaires au moment du build

   La procédure de génération, la chaîne de responsabilité, la rotation et le modèle de perte sont décrits dans [update-signing.md](update-signing.md). Perte de la clé privée = incident opérationnel critique (pas de récupération possible).

2. **Workflow [`.github/workflows/build-release.yml`](../.github/workflows/build-release.yml)** — LIVRÉ :
   - déclenchement sur tag `v*` (ou manuel), matrice `ubuntu-24.04` / `windows-latest` / `macos-latest`
   - builds signés produisant les artefacts installables ET leurs artefacts updater signés (`.sig`)
   - création d'une GitHub Release **draft** avec tous les assets attachés, dont `latest.json`
   - fail-closed : le workflow échoue si les secrets de signature manquent — aucune release non signée, jamais

3. **Workflow [`.github/workflows/promote-release.yml`](../.github/workflows/promote-release.yml)** — LIVRÉ :
   - déclenchement manuel uniquement (`workflow_dispatch` avec le tag en input), jamais automatique
   - garde-fou « jamais de feed partiel » : la promotion vérifie que la release draft porte les artefacts des trois cibles, leurs signatures et un `latest.json` couvrant les trois plateformes — le job échoue sinon
   - publication de la draft après revue humaine explicite : c'est la publication qui rend `releases/latest/download/latest.json` public, donc qui promeut le feed du canal `stable`

4. **Document [`docs/update-signing.md`](update-signing.md)** — LIVRÉ : génération initiale, stockage, chaîne de responsabilité, rotation en cas de compromission, modèle assumé en cas de perte.

## Automated delivery path

Le chemin de livraison automatisé, une fois les secrets provisionnés :

1. **Tag** : pousser un tag `v*` conforme à la convention stricte `vMAJOR.MINOR.PATCH` (aligné sur les trois manifests du repo — le verrou d'alignement des versions est testé en CI).
2. **Build** : `build-release.yml` construit et signe les artefacts des trois cibles, génère `latest.json` + les `.sig`, et attache le tout à une release **draft**. Rien n'est public à ce stade : une draft n'apparaît pas dans `releases/latest`.
3. **Revue humaine** : vérifier la release draft (artefacts présents pour les trois cibles, signatures, versions cohérentes, notes de release).
4. **Promotion** : lancer `promote-release.yml` avec le tag. Le workflow re-vérifie la complétude (jamais de feed partiel) puis publie la draft — le feed `releases/latest/download/latest.json` devient public et les copies AppImage distribuées porteuses de la clé publique voient la mise à jour via le geste intégré.

**Limite assumée** : ces workflows ne peuvent pas être exercés tant qu'aucun secret n'est provisionné — leur relecture a été statique (syntaxe, inputs/outputs de `tauri-apps/tauri-action` vérifiés contre sa documentation). La PREMIÈRE release réelle est leur banc d'essai, sous contrôle opérateur : ne jamais présenter la chaîne comme validée avant ce premier passage complet (build → draft → promotion → mise à jour constatée sur une copie réelle).

La procédure de build manuel ci-dessus reste VALIDE telle quelle : le build local n'exige aucune clé (les artefacts updater ne sont produits que sous l'overlay de configuration du workflow de release).

## Local persistence footprint

La persistance locale du brouillon utilisateur repose sur SQLite embarqué (via `rusqlite` compilé en `bundled`). Le fichier `rustory.sqlite` est créé et lu dans le répertoire `app_data_dir` résolu par Tauri v2, spécifique à chaque plateforme. Les migrations SQL vivent dans [`src-tauri/migrations/`](../src-tauri/migrations/) et sont appliquées à chaque démarrage, en mode idempotent via une table `schema_migrations`.

Le mode journal `WAL` est activé au premier `open_at`, ce qui produit deux fichiers annexes au voisinage de `rustory.sqlite` : `rustory.sqlite-wal` et `rustory.sqlite-shm`. Ces trois fichiers font partie de l'état local de l'application et doivent rester strictement dans `app_data_dir` — jamais dans le repo, jamais dans un dossier partagé entre utilisateurs.

Le titre d'une histoire est modifiable depuis la route `/story/:id/edit`. Un autosave automatique (fenêtre de debounce de 500 ms après le dernier keystroke) écrit la nouvelle valeur via la commande `update_story` et avance `updated_at` à chaque succès. La structure canonique (`structure_json`) et le `content_checksum` restent invariants lors d'un update de métadonnée — aucune migration SQL n'est requise pour cette fonctionnalité. Aucun fichier nouveau n'apparaît sur disque au-delà des trois fichiers SQLite déjà documentés.

L'export local d'une histoire est accessible via le bouton `Exporter l'histoire` sur la route `/story/:id/edit`. La commande Tauri `export_story_with_save_dialog` possède le boundary complet : elle ouvre la fenêtre de sauvegarde native côté Rust (via le plugin `tauri-plugin-dialog`), charge la story sous le lock SQLite, relâche le lock avant toute I/O disque, valide le chemin retourné par la dialog, puis écrit un fichier `.rustory` à l'emplacement sélectionné. Le renderer ne voit ni ne construit jamais un chemin filesystem arbitraire — il passe seulement un nom de fichier suggéré et reçoit un outcome taggé (`{ kind: "exported" | "cancelled" }`). Ce fichier est un document JSON UTF-8 lisible — `{ "rustoryArtifact": {...}, "story": {...} }` — dont le `contentChecksum` est recopié tel quel depuis SQLite, jamais recalculé. L'export est strictement en lecture seule sur `stories` : `title`, `structure_json`, `content_checksum`, `created_at`, `updated_at` restent byte-à-byte inchangés. L'écriture passe par un `NamedTempFile` co-localisé + `fsync` + rename atomique pour qu'un crash pendant l'export ne laisse jamais un fichier partiel derrière. Aucune migration SQL n'est requise, aucun fichier ne persiste en dehors du chemin cible choisi par l'utilisateur.

La récupération d'un brouillon après interruption s'appuie sur une table `story_drafts` (migration `0002_story_drafts.sql`, FK `ON DELETE CASCADE` vers `stories.id`) qui buffer chaque frappe de l'utilisateur 150 ms avant le commit autosave de 500 ms. Sur une fermeture inattendue (crash, `kill -9`, coupure de courant), la row survit et est lue au prochain mount de la route d'édition ; l'utilisateur reçoit une bannière inline avec le diff `tu avais tapé / dernier état enregistré` et choisit explicitement entre `Restaurer le brouillon` ou `Conserver l'état enregistré`. Un autosave réussi consomme la row dans la même transaction `BEGIN IMMEDIATE` (la commande `update_story` exécute `UPDATE stories ... + DELETE FROM story_drafts ...` en une seule unité atomique), et la commande `apply_recovery` exécute le même couple pour la restauration manuelle. Un sous-système de trace minimaliste `infrastructure/diagnostics/recovery_log` écrit dans `{app_data_dir}/diagnostics/recovery.jsonl` une ligne JSONL par event critique (`interrupted_session_detected` au boot si des drafts persistent, `recovery_draft_proposed/applied/discarded/unavailable` à chaque interaction), avec une catégorie issue d'un set fermé pour la traçabilité support. Le fichier est tourné quand il dépasse 10 MB (renommé en `recovery-{timestamp}.jsonl.archived`). Aucune dépendance externe n'a été ajoutée : `serde_json` et `OpenOptions::append` suffisent pour ce périmètre. Aucun message brut OS n'est journalisé.

**Note de sécurité** : la permission Tauri `dialog:allow-save` déclarée dans `src-tauri/capabilities/default.json` n'est **pas** une autorisation d'écriture filesystem générale accordée au renderer. La sécurité d'écriture vient du couplage dialog↔write dans le boundary Rust : le renderer ne peut pas fournir un chemin arbitraire à écrire — il peut seulement inviter l'utilisateur à choisir un emplacement via la dialog, et c'est Rust qui valide (canonicalisation parent, refus des symlinks, refus du dossier `app_data_dir` interne, extension `.rustory` obligatoire ou auto-ajoutée) puis écrit. Cette architecture doit rester intacte lors d'audits : toute commande future qui accepterait un `destination_path` en input direct depuis le renderer réouvrirait une surface d'écriture arbitraire.

## Device Detection Footprint (MVP Phase 1)

Rustory scans mounted USB Mass Storage volumes for the canonical Lunii
marker set (`.md` + `.pi` at the volume root; `.bt` is observed on
some generations but is informational only — see
[device-support-profile.md](architecture/device-support-profile.md)).
The frontend hook `useConnectedLunii` polls the scan silently every
3 s so plug/unplug events surface automatically without the user
clicking `Réessayer la détection`. The manual refresh button stays
available as a fallback. Each scan emits one or more lines into
`{app_data_dir}/diagnostics/device.jsonl` (rotation 10 MB, identical
shape to `recovery.jsonl`). The scan budget is 4 seconds wall-clock
end-to-end (NFR4 cap is 5 s; the 1 s margin absorbs IPC marshalling).

Profiles authorized for read in Phase 1: Lunii Origine v1 (metadata
v3), Lunii Mid-Gen v2 (metadata v6), Lunii V3 (metadata v7) for read
only. Write is hard-blocked at the application capability gate
(`application::device::check_operation_allowed`) for every profile
until Epic 3 wires the transfer pipeline. The matrix lives at
[`docs/architecture/device-support-profile.md`](architecture/device-support-profile.md).

Security note: `.pi` payloads are SHA-256-hashed (truncated to 32 hex
chars) before being used as a `device_identifier`. The raw `.pi`
content NEVER leaves the Rust core or reaches the diagnostics log;
absolute filesystem paths NEVER appear in serialized events. Two new
dependencies were added: `sysinfo = "=0.32"` for cross-platform mount
enumeration, and (Linux only) `zbus = "=5.16"` (blocking-api feature)
for talking to udisks2 over D-Bus when a plugged Lunii volume needs
to be auto-mounted.

Auto-mount (Linux only): on minimal desktop sessions (no GNOME/KDE
session daemon, locked-down polkit) udisks2 may not trigger an
automatic mount when a Lunii is plugged in. Rustory therefore asks
udisks2 — over D-Bus — to mount any block device matching a tight
Lunii signature (`Drive` path contains "STM", `IdType = vfat`,
`IdUsage = filesystem`, `MountPoints` empty). Generic USB sticks are
filtered out by this signature so Rustory never mutates unrelated
media. The behavior can be disabled entirely by setting
`RUSTORY_DEVICE_AUTOMOUNT=0`. macOS / Windows do not need this path
(OS-level auto-mount is universal) and the module compiles to a
no-op on those platforms.

The first scan on a host happens lazily, when the user opens the
library context. A failed scan does not block the rest of the
application: autosave, export and recovery keep running with the
SQLite mutex available throughout — the scan never holds the DB lock.

## Failure-mode guardrails

Même en livraison manuelle, ne **jamais** :

- publier un binaire non issu d'une checkout `main` propre et vérifiée localement
- promouvoir une CI verte comme « release ready » — `verify.yml` valide la compilation et les tests, pas la signature ni la reproductibilité release
- activer un feed updater partiel pointant vers un sous-ensemble de plateformes (donnerait l'illusion d'une couverture complète) — `promote-release.yml` outille désormais ce garde-fou : il refuse de publier une draft dont `latest.json` ne couvre pas les trois cibles, et publier la draft à la main en contournant ce contrôle reste interdit
- publier une release automatiquement — la promotion passe TOUJOURS par la revue humaine puis `promote-release.yml`
- distribuer un build dont les vérifications locales ne sont pas toutes vertes

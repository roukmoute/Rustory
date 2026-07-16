# UI States

## Purpose

This document defines Rustory's canonical user-visible states and the rules for showing them.

It prevents state drift across:
- library views
- transfer surfaces
- recovery flows
- future editing screens
- tests and acceptance criteria

## Authority Rules

- Rust is the authoritative source for canonical business truth.
- The UI may derive presentation, but it must not invent competing truth.
- A visible success state must never appear before the required verification step completes.
- Local state, device state, and last-known transfer outcome must stay distinguishable.

## MVP State Model

These are the minimum user-visible states for `MVP Phase 1`.

| Internal Intent | Preferred UI Label | When It May Be Shown | Must Never Mean |
| --- | --- | --- | --- |
| Local canonical draft exists | `brouillon local` | Story exists locally and is editable/resumable | Present on device |
| Validation/preflight running | `en vérification` | Compatibility or safety checks are actively running | Transfer already started |
| Real blocking issue exists | `bloquée` | A required condition prevents the next action | Generic inconvenience or unknown state |
| Fixable blocking issue exists | `à corriger` | A detected issue blocks the send but the user can repair it (e.g. an invalid title) | A hard unrecoverable block, or a mere warning |
| Validation passed for send | `présumée transférable` | Rust has not found a real block yet | Transfer already succeeded |
| Preparation running | `en préparation` | Media/artifact preparation is in progress | Device write in progress |
| Device write running | `en transfert` | Rust has accepted and is executing a write operation | Verified success |
| Verification succeeded | `transférée et vérifiée` | Write outcome was explicitly confirmed | Mere write completion without proof |
| Retry-safe failure | `échec récupérable` | The user can relaunch from preserved local state | Partial success |
| Incomplete outcome | `état partiel` | Verification or import found a mixed/incomplete result | A successful end state |

## Transfer State Contract

The transfer contract is part of the MVP and should map internal state to UI labels as follows:

| Internal Contract State | UI Label | Notes |
| --- | --- | --- |
| `blocked` | `bloquée` | Show the blocking reason and the next useful action |
| `to_fix` | `à corriger` | A repairable block (e.g. an invalid title); show the cause + the fixing action |
| `presumed_transferable` | `présumée transférable` | Valid for decision surfaces before send |
| `preparing` | `en préparation` | May coexist with preserved local work |
| `transferring` | `en transfert` | Must stay visibly in-context; honest progress (real `%` only during the content copy, never a fake value nor 100 % before the terminal); a non-destructive `Consulter le détail` discloses phase/progress in-context |
| `verified` | `transférée et vérifiée` | Only after explicit confirmation |
| `partial` | `état partiel` | Never collapse into success wording (a verification verdict) |
| `retryable` (`échoué`) | `échec récupérable` | Device left UNTOUCHED; keep enough context for `Relancer` / `Abandonner` |
| `incomplete` | `transfert incomplet` | Write STARTED then interrupted (device mutated); the device may hold a partial copy, a relaunch restores a safe state; distinct from `état partiel`; `Relancer` / `Abandonner` |

## Post-MVP Import State Contract

These states are for post-MVP local structured import flows and must not be mistaken for MVP transfer states. Two flows realize them: the
`Local Artifact Import Contract` below (`.rustory` file import) and the `Structured Folder Creation Contract` (folder → new story); see them for the full two-phase machines, recognition models and error taxonomy.

| Internal Contract State | UI Label | Scope |
| --- | --- | --- |
| `recognized` | `reconnu` | Import analysis found usable material |
| `partial` | `partiel` | Some content is usable, some is not — emitted by the structured-folder flow (a referenced media is missing); never by the `.rustory` flow |
| `needs_review` | `à revoir` | The user must inspect before accepting |
| `blocked` | `bloqué` | Import cannot continue safely |
| `resolved` | `résolu` | The import review was settled by a real write that left the canonical story fully sound (see `Import Review Resolution Contract`); renders with NO marker — the marker's disappearance IS the feedback |

## State Transition Rules

- `brouillon local` may lead to `en vérification` or remain local.
- `en vérification` may lead to `bloquée`, `à corriger`, or `présumée transférable`.
- `à corriger` may lead back to `en vérification` after the user repairs the flagged issue (e.g. renames the story).
- `présumée transférable` may lead to `en préparation` or directly to `en transfert`, depending on the flow.
- `en transfert` may lead to `transférée et vérifiée`, `état partiel`, or `échec récupérable`.
- `échec récupérable` may lead back to `en vérification`, `en préparation`, or `en transfert` through an explicit user action such as `Relancer`.

## Forbidden Ambiguities

The UI must never:

- show `transférée` when verification has not happened
- merge local truth and device truth into one ambiguous badge
- use the same visual label for `bloquée` and `échec récupérable`
- treat `état partiel` as a silent success
- hide a critical issue only in a `toast`
- carry a node media block only in a `toast` — it lives inline at its slot
- paint a node `Enregistré` while one of its fields or media is still blocked
- show an editable node field or media action that cannot be saved (a device-pack story's content is carried by the copied pack — only its title is locally editable; see `Imported Story Edit Scope Contract`)

## Disabled Actions and Reasons

When a primary action is visible but disabled:

- the reason must be explicit and short
- the reason must map to a stable domain cause
- the wording must stay consistent with [product-language.md](./product-language.md)

Preferred patterns:

- `Envoi indisponible: histoire bloquée`
- `Envoi indisponible: appareil non supporté`
- `Relance indisponible: aucun brouillon préservé`
- `Création d'histoire indisponible pour l'instant.`
- `Filtres avancés à venir`
- `Reprise indisponible: aucune histoire sélectionnée`
- `Reprise indisponible: sélection multiple`
- `Bibliothèque incohérente, recharge nécessaire`
- `Création impossible: titre requis`
- `Création impossible: titre trop long (120 caractères maximum, N en trop)` — `N` is the exact excess (`chars - 120`, minimum `1`) computed both by the Rust domain and the TS mirror so the user sees how many code points to trim.
- `Création impossible: titre contient des caractères non autorisés`
- `Reprise indisponible: histoire introuvable`
- `Enregistrement en échec: vérifie le disque local et réessaie.`
- `Récupération indisponible: vérifie le disque local et réessaie.`
- `Restauration en cours: patiente quelques instants.`
- `Édition en attente: choisis d'abord comment reprendre cette histoire.`
- `Envoi indisponible: aucune histoire sélectionnée`
- `Envoi indisponible: sélection multiple`
- `Envoi indisponible: aucun appareil connecté`
- `Envoi indisponible: profil non supporté` — covers a V3 (device write is still being reverse-engineered, like import), any recognized profile with zero write capability (FLAM Gen1 — the capability-closed path, never the "MVP Phase 1" promise), and any unsupported profile
- `Envoi indisponible: profil ambigu`
- `Envoi indisponible: détection en cours`
- `Envoi indisponible: détection en échec`
- `Envoi indisponible: prépare l'histoire d'abord` — no fresh transfer-artifact descriptor yet; run `Préparer` first
- `Envoi indisponible: histoire native non transférable (pas de pack appareil)` — a native story has no device-format pack; enforced by the backend transfer outcome (`notTransferable`) and surfaced in context
- `Envoi indisponible: transfert pas encore activé (MVP Phase 1)` — **legacy fallback only**: it no longer applies to the write-authorized cohorts (V1/V2), where the `Envoyer vers la Lunii` CTA is now activable; it subsists solely where the panel is given no write target at all (tests/storybook)
- `Détection indisponible: vérifie que la Lunii est branchée et réessaie.`
- `Profil non supporté: format métadonnées v{n} non géré`
- `Profil non supporté: firmware {hint} non géré`
- `Profil non supporté: marqueurs appareil incomplets`
- `Profil ambigu: {n} candidats détectés. Débranche les autres puis réessaie.`
- `Lecture appareil indisponible: profil non autorisé`
- `Lecture de la bibliothèque appareil indisponible: vérifie que la Lunii est branchée et réessaie.` — Lunii read path VERBATIM; the FLAM read path emits the family-correct sibling `Lecture de la bibliothèque appareil indisponible: vérifie que l'appareil est branché et réessaie.` (same `DEVICE_SCAN_FAILED` / `fs_read` contract)
- `Lecture de la bibliothèque appareil indisponible: l'index des histoires est illisible.`
- `Lecture de la bibliothèque appareil indisponible: l'appareil met trop de temps à répondre.`
- `Lecture de la bibliothèque appareil indisponible: l'appareil connecté a changé.`
- `Copie indisponible: profil non supporté`
- `Copie indisponible: déjà dans ta bibliothèque`
- `Copie indisponible: contenu incomplet sur l'appareil`
- `Préparation indisponible: aucune histoire sélectionnée`
- `Préparation indisponible: sélection multiple`
- `Préparation indisponible: aucun appareil connecté`
- `Préparation indisponible: profil non supporté`
- `Préparation indisponible: corrige les blocages d'abord`
- `Source indisponible: non activée dans la distribution officielle` — a creation-dialog content-source entry whose kind the current distribution does not activate (see `Content Source Activation Contract`); the reason is Rust-authoritative (carried by the policy DTO), keyboard-reachable
- `Source indisponible: bloquée par la politique de distribution` — same rendering for a kind deliberately blocked by the distribution policy; no line of the current matrix carries this state, but the copy is frozen and tested
- `Sources externes indisponibles pour l'instant.` — the fail-closed reason when the policy read itself failed (or no policy was handed to the dialog): every external-source entry renders disabled, never active-by-default

For a device-pack story (`titleOnly` edit scope) the content zones render the
named pack states INSTEAD of the controls (see `Imported Story Edit Scope
Contract`) — the controls are ABSENT, never shown-but-disabled; `Aperçu` /
`Retirer` only render when a media is present, so they are never
shown-but-disabled either.

Node-media error payloads carry a stable `details.source` so support can triage
without parsing the user-facing message:

- `media_invalid` — the chosen file is refused at attach: `details.stage` is one of `unsupported_format`, `oversize`, `non_filesystem_path`, `unknown_slot`. Surfaces as `MEDIA_INVALID` at the media slot, never a toast.
- `media_processing_failed` — a transport failure of the media store: `details.stage` is one of `staging`, `promote`, `read`, `db`, `invalid_name`, `app_data_unavailable`, `dialog_failed`, `spawn_blocking_join`. Surfaces as `MEDIA_PROCESSING_FAILED`.

Error payloads that surface in the autosave alert carry a stable
`details.source` discriminator so support can triage the cause without
parsing the user-facing message:

- `sqlite_update` — transport failure on the UPDATE itself. `details.stage` is one of `begin_transaction`, `update`, `commit` and `details.kind` is one of `busy`, `locked`, `constraint_violation`, `other`. A `constraint_violation` is re-mapped to `INVALID_STORY_TITLE` so the same canonical reason text as the creation dialog is shown.
- `story_missing` — the UPDATE matched zero rows (the id is no longer in the table). Surfaces as `LIBRARY_INCONSISTENT` with the canonical "Histoire introuvable" copy.
- `story_duplicate` — the UPDATE matched more than one row (schema corruption: duplicate primary keys). Surfaces as `LIBRARY_INCONSISTENT` and includes `details.rowsAffected`.
- `story_id_invalid` — the UPDATE or read received an empty or oversize id. `details.cause` is `empty` or `too_long`.
- `sqlite_select` — transport failure on the detail read. No `stage` subdivision (the query is a single `SELECT`).
- `system_clock_invalid` — `OffsetDateTime::format` refused to produce an ISO-8601 timestamp (system clock outside representable range).

Avoid:

- `Action impossible`
- `Erreur`
- `Not ready`

## Screen Ownership

- `bibliothèque` is the stable home for selection, transfer visibility, and recovery traces
- `édition` is a separate context and must not silently replace library state vocabulary
- `Profil de support` (route `/settings`) is the read-only screen where the distribution's support facts live — devices, local artifacts, content sources, posture; it never becomes a settings/toggles surface (see `Support Profile Screen Contract`)
- `modals` and reports may clarify a state, but should not become the only place where a critical state exists

## Implementation Rule

When a new feature wants to introduce a new user-visible state:

1. Reuse an existing state if it already describes the situation.
2. If reuse is impossible, update this document first.
3. Add or update tests and acceptance criteria using the same wording.

## Window Sizing Contract

Rustory is desktop-only. No `mobile` or `tablet` UI is supported.

| Key | Value |
| --- | --- |
| `default window size` | `1440 × 900` |
| `minimum window size` | `1024 × 720` |
| nominal work range | `portable 13 to 16 inches` |

Enforced in `src-tauri/tauri.conf.json` via `app.windows[0].{width, height, minWidth, minHeight}`.

Two visual modes only:

- **standard mode**: target configuration, full layout, stable hierarchy
- **reduced-density mode**: same architecture, margins and secondary columns lightly compressed. No structural rework, no hamburger, no bottom navigation, no second interface.

Below the minimum size, the window refuses to shrink further through the Tauri constraint. No responsive fallback is attempted.

## Library Layout Contract

The library context is a stable three-column desktop grid. Future routes that
compose the library must reuse this contract rather than invent a variant.

| Zone | Role | ARIA region | CSS column (standard mode) | CSS column (reduced-density) |
| --- | --- | --- | --- | --- |
| Left column | Global filters / navigation entry points | `<nav aria-label="Navigation bibliothèque">` | `minmax(240px, 280px)` | `200px` |
| Center column | Story collection — the main work surface | `<main aria-label="Collection d'histoires">` | `1fr` | `1fr` |
| Right column | Decision panel (device state + send CTA) | `<aside aria-label="Panneau de décision">` | `minmax(320px, 360px)` | `300px` |

Rules:

- The center column is the only working surface; the two side columns support decision without stealing focus.
- Error, empty, filtered-empty and loading states all live inside the center column — never in the nav or in the panel.
- The right-column panel stays intentionally minimal: device state + one primary CTA + a short, standardized reason when the CTA is disabled.
- No fourth column, no collapsible drawer, no hamburger.

## Device Detection Contract

Drives the right-column `Lunii Decision Panel`. The Rust core scans
mounted USB Mass Storage volumes for the canonical Lunii markers
(`.md` + `.pi` at the volume root — `.bt` is observed on some
generations but is **not** required, see
[device-support-profile.md](./device-support-profile.md)); the IPC
façade exposes a tagged-enum DTO; the panel maps each kind to one of
the labels below.

The frontend hook `useConnectedLunii` polls silently every 3 s so
plug / unplug events surface automatically without the user clicking
`Réessayer la détection`. The manual refresh button stays available
as a fallback.

| Internal State | Wire `kind` | Panel Label | Disabled Actions |
| --- | --- | --- | --- |
| no device | `none` | `Aucun appareil connecté` | Envoi |
| supported (≥ 1 activated capability) | `supported` | `Appareil prêt — {family} {cohort}` | Envoi (Phase 1 — wired Epic 3) |
| supported (zero activated capability) | `supported` | `Appareil reconnu — {famille}` | Envoi (capability-closed path) |
| unsupported | `unsupported` | `Profil non supporté` + standardized reason | Envoi |
| ambiguous | `ambiguous` | `Profil ambigu — {n} candidats` | Envoi |
| scanning | (transient) | `Détection en cours…` | Envoi, Réessayer |
| error | (`AppError DEVICE_SCAN_FAILED`) | `Détection indisponible` | Envoi |

**Recognized ≠ ready (general product rule).** `Appareil prêt — …` REQUIRES
at least one activated capability: the panel derives
`hasAnyCapability = readLibrary || inspectStory || importStory || writeStory`
from the authoritative DTO — never from the family name (no
`if family === "flam"` in state rendering; any future zero-capability
profile is honest by construction). A FLAM Gen1 now carries three
activated read-side capabilities (see the support matrix), so it renders
`Appareil prêt — FLAM` through this EXISTING rule — no family-conditional
code; its transfer capability line renders non-activated
(`— Transfert vers l'appareil`) and the send stays disabled through the
capability-closed path below.

A supported profile whose capabilities are ALL `false` — a DECLARED state
with **no live emitter** since the FLAM read capabilities activated (kept
in the closed set for any future zero-capability family, per the
declared-state discipline of this document) — renders:

- the STATIC chip `Appareil reconnu — {famille}` (a durable state, never
  `role="alert"`),
- its four capability lines as `—` (the standard non-activated rendering),
- a sober TEXT-ONLY support-profile explanation (`Appareil reconnu, aucune
  opération activée dans cette version. Consulte le profil de support pour
  comprendre ce qui est permis.`) — rendered in this
  recognized-without-capability idle state ONLY, never in a
  capability-bearing idle (Lunii, FLAM). The pointer stays informative
  text with NO navigation and NO network (NFR14); the internal target it
  speaks about now exists — the `Consulter le profil de support` CTA
  keeps its pre-existing scope (unsupported / ambiguous / error), is NOT
  offered here, and navigates IN-APP to `/settings` (see `Support
  Profile Screen Contract`) — no external browser, no network,
- the send disabled through the EXISTING capability-closed path
  (`Envoi indisponible: profil non supporté`, the V3 pattern). The idle copy
  `Envoi indisponible: transfert pas encore activé (MVP Phase 1)` stays
  EXCLUSIVE to write-planned Lunii cohorts and is never rendered for a
  zero-capability profile.

The send CTA label is family-correct: a Lunii panel keeps
`Envoyer vers la Lunii` VERBATIM; any other family renders
`Envoyer vers l'appareil` (Change Control,
[product-language.md](./product-language.md)). The FLAM send stays
disabled through the capability-closed reason
(`Envoi indisponible: profil non supporté`) — `writeStory` is `false`
on the FLAM Gen1 matrix line.

The metadata format version NEVER appears in a FLAM rendering — the wire
omits the `metadataFormatVersion` key entirely for a profile that has no
metadata version (never `null`, never an invented `0`).

Error payloads under `DEVICE_SCAN_FAILED` carry a stable
`details.source` discriminator so support can triage without parsing
the user-facing message:

- `fs_read` — per-mount filesystem read failure. `details.kind` is one of `permission_denied`, `not_found`, `timeout`, `interrupted`, `other`.
- `os_enum` — failure to enumerate the host disk list. `details.kind` mirrors the I/O kind label set above.
- `scan_timeout` — wall-clock budget exhausted before a candidate emerged. Carries `details.elapsed_ms`.
- `other` — fallback when the upstream source token does not match a known set.

Diagnostic events for the scan flow live in
`{app_data_dir}/diagnostics/device.jsonl` (rotation cap 10 MB,
identical to recovery.jsonl). Each event carries `elapsed_ms` (the
wall-clock duration of the surrounding scan pipeline) so support can
spot slow scans approaching the NFR4 5 s budget. The closed `category`
set is: `device_absent`, `device_detected_supported`,
`device_detected_unsupported`, `device_scan_failed`,
`device_automounted` (Linux only, when Rustory mounts a plugged
Lunii via udisks2 D-Bus), `device_automount_failed` (Linux only,
when the D-Bus mount call is refused), `device_library_read` (the
inventory was read — carries `story_count` and `hidden_count`, never
the raw pack UUIDs), `device_library_read_failed` (carries a
closed-set `source` and the upstream `kind`),
`device_story_imported` (a device pack was copied into the local
library — carries `short_id`, `story_id`, `elapsed_ms`,
`bytes_copied` and `file_count`; NEVER the full pack UUID nor any
absolute path), and `device_story_import_failed` (carries the
closed-set `source` from the import taxonomy and the upstream
`kind`). Each line is one JSON object — `device_identifier` is
always the SHA-256 hash of `.pi` + volume serial, never the raw
payload.

## Device Library Contract

After a device is detected as `supported`, Rustory may read its
installed-story inventory and show it as a **distinct section in the
center column**, below the local collection (separated by a rule). The
device library is never merged into the local collection and is never
hosted by the right decision panel (which stays "device state + send
CTA"). This keeps local truth and device truth distinguishable at all
times.

This contract is FAMILY-NEUTRAL: it lights up for ANY supported profile
whose `readLibrary` capability is `true` on the support matrix (every
supported Lunii cohort, FLAM Gen1) — same states, same chips, same error
surface, no per-family rendering. The wire DTO carries no family field;
the capability matrix alone decides.

The frontend hook `useDeviceLibrary(deviceIdentifier)` reads the
inventory through the `read_device_library` command. It is orthogonal to
`useLibraryOverview`: a device-read failure never alters the LOCAL
library. There is no polling of the inventory — device PRESENCE is polled
by `useConnectedLunii`; the heavier inventory read fires when the
identifier changes (a different device) and on a manual retry.

Scope: listing + recognition. Each entry shows its recognized `title` when a
local index covers the pack (composed Rust-side — see
[device-support-profile.md#Title Recognition & Catalog Policy](./device-support-profile.md)),
otherwise the opaque `shortId` under `Histoire non reconnue`. No asserted
content quality (the device stores none). Device entries are
single-selectable for inspection (see `Device Story Inspection Contract`)
but never editable here; that selection is separate from the local
`selectedStoryIds` and is not persisted.

| Internal State | Center-column rendering | Notes |
| --- | --- | --- |
| idle | nothing | No readable device (none / unsupported / not read-authorized). |
| loading | `ProgressIndicator` + `Lecture de la bibliothèque de l'appareil…` | "État non encore chargé" — must never read as "aucune histoire". |
| ready (n > 0) | list of entries + count | Each entry: the recognized `title` (or `Histoire non reconnue`) + `Identifiant: <shortId>` + a provenance chip when recognized. |
| ready (n = 0) | `Aucune histoire sur l'appareil` | Distinct from the loading state. Its hint is family-neutral: `L'appareil connecté ne contient aucune histoire lisible.` |
| error | `LibraryErrorBanner` (`role="alert"`) titled `Bibliothèque de l'appareil indisponible` + `Réessayer` | Recoverable, in-context, never a toast. The local library stays intact. |

Provenance is explicit: the section heading is `Histoires sur l'appareil`
(plus the device label when known) with a `Sur l'appareil` chip. Per
entry, a title-provenance chip (`Titre officiel` / `Titre non-officiel` /
`Titre saisi`) when recognized, plus `Masquée` (`.pi.hidden`) and `Contenu
incomplet` (no `.content/<shortId>` folder) chips surface structural facts —
all non-color (ASCII-glyph chips). The recognized title and its provenance
are also folded into each card's accessible name.

Error payloads under the read carry a stable `details.source`:

- `fs_read` — index-file read failure. `details.kind` ∈ `permission_denied`, `not_found`, `timeout`, `interrupted`, `other` (a mid-read unplug typically surfaces as `not_found`).
- `pack_index` — the story index is unreadable: it exceeded the 64 KB inventory bound (`details.kind = "oversize"` — `.pi` or FLAM `etc/library/list`), or the FLAM index entry is not a regular file (`details.kind = "not_a_regular_file"` — symlink/special file, refused no-follow).
- `read_timeout` — the read budget was exhausted (`details.elapsed_ms`).
- `device_changed` — the live re-scan no longer resolves to the requested device (swapped / unplugged-and-replaced).
- `scan_timeout` / `os_enum` / `mount_unavailable` / `spawn_blocking_join` / `other` — re-scan transport failures, mirroring the detection set.

## Device Story Inspection Contract

A device story listed by the `Device Library Contract` can be SELECTED to
inspect it before any import. Selection drives a contextual inspector in the
right column. Inspection is read-only: it consults the inventory snapshot
already in memory, never re-reads or mutates the device, and never imports
anything (the import flow is a later story).

| Aspect | Value |
| --- | --- |
| Source of truth | A single selected device-story `uuid`, held by `LibraryRoute` as local UI state. Separate from the local `selectedStoryIds`: selecting a device story never touches the local selection, and vice-versa. |
| Cardinality | Single selection — inspection (and the later import) target one device story at a time. |
| Persistence | None. Never survives a restart, and is dropped when the readable device changes or the selected entry is absent from a fresh inventory read — no silent stale target (a `uuid` that resolves to no current entry renders no inspector for that render). |
| Capability gate | Offered only when the detected profile authorizes `inspectStory` (✅ for every supported Lunii cohort — V3 included — AND for FLAM Gen1; distinct from `importStory`, which is ❌ for V3). When `inspectStory` is false, the device cards stay non-interactive. Family-neutral: the matrix decides, never the family name. |
| Non-color signal | A selected device card renders a colored border, a visible `✓` prefix in the DOM and `aria-pressed="true"` — the selection survives grayscale / color-blindness, exactly like a local `Story Card`. |
| Interaction | A selectable device card is a `role="button" aria-pressed={selected}` focus stop. Click (or `Space` / `Enter` on the focused card) toggles its selection. There is no double-click / open: a device story is not editable. |

The inspector lives in the right column ABOVE the `Lunii Decision Panel`
and renders only when a device story is selected:

| Inspector element | Content |
| --- | --- |
| Heading | `Histoire sélectionnée`. |
| Provenance | A `Sur l'appareil` chip plus the note `Cette histoire vit sur l'appareil, pas encore dans ta bibliothèque locale.` — or, when `alreadyImported`, `Cette histoire vit sur l'appareil et une copie existe déjà dans ta bibliothèque locale.` (the note must never claim "pas encore" about a story whose local copy exists). Keeps local truth and device truth distinguishable. |
| Identity | The recognized `title` when a local index covers the pack, otherwise `Histoire non reconnue`; always `Identifiant: <shortId>` and `UUID: <uuid>`. No asserted content quality. |
| Title provenance | When recognized, a provenance chip states the source honestly: `Titre officiel` (info tone), `Titre non-officiel` (neutral), `Titre saisi` (neutral). A user-typed or community title is NEVER labelled "officiel". Resolution priority (Rust-owned): `user > official > unofficial > none`. |
| Naming | For a genuinely unrecognized pack, a `Nommer cette histoire` action opens an inline editor (same title rules as a local story: NFC + trim + denylist + ≤120, Rust-authoritative). A name the user typed earlier can be edited via `Renommer cette histoire`. A user title is `source = user`, so it is never silently overwritten by a later official/community match; it survives unplug/replug. Naming is not offered for official/community titles (not the user's to overwrite here). A rejected title surfaces in-context next to the field, never as a toast. |
| Honest triage (before any copy) | The verified facts from the in-memory inventory snapshot are grouped under three honest headers so nothing is imported blindly (no device re-read, no catalog lookup): **`Ce que Rustory reconnaît`** — the identifiers and, when `contentPresent`, a `Contenu présent` chip; **`Ce qui bloque la copie`** — `Contenu incomplet` (`!contentPresent`) and `Dans ta bibliothèque` (`alreadyImported`), each with a short honest note; **`À revoir avant de copier`** — `Masquée` (`hidden`). A group is shown only when it has a fact. Fail-closed: a fact that is absent/unknown is never asserted positively. These headers are distinct from the Post-MVP `reconnu` / `partiel` / `à revoir` import-state labels — a device copy is binary (full success or `IMPORT_FAILED`), so this triage is a pre-decision, never a partial-import report. |
| Ambiguity flags | `Masquée` (`.pi.hidden`) and `Contenu incomplet` (no `.content/<shortId>` folder) chips, plus a short honest note when content is missing — surfaced, never hidden, never called "corrupt". |
| Copy affordance | A `Copier dans ma bibliothèque` button (device → local library), ACTIVE when `importStory === true && contentPresent && !alreadyImported` and the device is readable (see `Device Story Import Contract`). Otherwise it stays soft-disabled with a standardized, capability-aware reason picked fail-closed in this priority order: (1) `story.alreadyImported` ⇒ `Copie indisponible: déjà dans ta bibliothèque` (the most useful fact — no copy is needed); (2) `importStory !== true` or operations matrix absent ⇒ `Copie indisponible: profil non supporté` (fail-closed default, V3 included); (3) `!story.contentPresent` ⇒ `Copie indisponible: contenu incomplet sur l'appareil`. The internal capability flag stays `importStory`; only the user-facing verb is `Copier` (Importer / Exporter are reserved for file artifacts). |
| Next gesture on a profile refusal | When the copy is refused for an unsupported profile (the V3 case: inspectable but not importable), the inspector shows a discreet `Consulter le profil de support` affordance (`Button variant="quiet"`, `aria-label="Consulter le profil de support officiel"`) wired to the same `openSupportProfile` action as the detection panel — so a refusal carries a next gesture, never an opaque grayed-out CTA. The affordance is shown ONLY for the profile refusal: not for `déjà dans ta bibliothèque` (a copy is simply not needed) nor `contenu incomplet` (the honest note already tells the user to check the device), and it is hidden entirely in listing/inspection-only contexts where the route wires no handler. |

The inspector is the only place the right column reflects a device story; it
must never merge with the panel's local-selection layer, and the panel
itself stays "device state + send CTA".

## Device Story Import Contract

Activating the `Copier dans ma bibliothèque` affordance starts a raw,
structurally validated acquisition of the selected device pack into the
local library (see
[device-support-profile.md#Story Import Contract](./device-support-profile.md)
for the Lunii pack format, and "FLAM library inventory & story import"
for the opaque FLAM pack — same UI machine, same taxonomy, family-neutral
rendering; the created story's default title is family-correct:
`Histoire de ma Lunii (XXXXXXXX)` / `Histoire de mon FLAM (XXXXXXXX)`).
This contract is **distinct from the `Post-MVP Import State Contract`**
(local structured imports, Epic 4): the `reconnu` / `partiel` / `à
revoir` labels MUST NOT be reused here — a device copy either fully
succeeds or explicitly fails.

UI state machine (owned by `useDeviceStoryImport`, surfaced in the
inspector):

| State | Rendering | Announcement |
| --- | --- | --- |
| `idle` | no status content | none (the polite region stays mounted, empty) |
| `importing` | indeterminate `ProgressIndicator` labelled `Copie en cours…` (calm, neutral), CTA soft-disabled + `aria-busy` | deliberately NOT announced (ephemeral noise) |
| `imported` | `Histoire copiée dans ta bibliothèque` (success chip) + the created local title + explicit `Fermer` dismiss | `aria-live="polite"` region, mounted permanently, `aria-atomic` |
| `failed` | `Copie impossible` block with the canonical `message` + `userAction`, buttons `Réessayer` THEN `Fermer` in tab order. EXCEPTION: a profile refusal (`DEVICE_UNSUPPORTED` — the live device turned out non-importable, e.g. after a stale snapshot) is not retryable, so the alert offers `Consulter le profil de support` in place of `Réessayer` (parity with the pre-click affordance), THEN `Fermer` | `role="alert"` |

Surface rules:

- All import feedback lives in the inspector (right column). Never a
  toast for the failure, never a tunnel modal, never a navigation.
- No pre-copy confirmation dialog: the copy is non-destructive (the
  device is read-only end to end).
- The success does not auto-hide — dismiss is explicit, the user reads
  the outcome at their own pace.
- The `failed` state is never wiped IMPLICITLY (no auto-hide, no
  navigation side effect) — but the alert's own explicit `Fermer`
  button DOES dismiss it back to idle, exactly like `Réessayer` or a
  fresh copy trigger leave it. An inoperative close button is a bug,
  not a preservation feature.
- The status surface is attached to the pack that was copied: selecting
  another device story shows THAT story's (idle) status, never the
  previous card's success or failure; re-selecting the copied story
  surfaces its status again.
- A critical copy failure NEVER lives only in a toast (UX-DR15): the
  `failed` alert is `role="alert"` inside the inspector, and the `Toast`
  primitive excludes the `error` tone at the type level so the compiler
  refuses an error-only toast. The failure stays consultable as long as
  the user is on the library, re-displayed by re-selecting its pack.
- Residual limit (assumed for the MVP): the `failed` alert lives in
  route-local state (the hook + the selected-pack gate). It survives
  selection changes WITHIN the library (re-displayed by re-selecting its
  pack), but NOT a full departure from the library context — leaving the
  route unmounts the hook and the alert is gone on return (the pack shows
  idle again). The canonical state stays coherent regardless (Rust
  finished or refused the copy, the overview cache is reconciled), so a
  fresh copy trigger on return simply re-surfaces the honest refusal
  (e.g. `already_imported`). A global, cross-route notifier that would
  carry the failure across a route departure is a deliberately deferred,
  separate concern.

Post-success traces (AC: trace on BOTH sides):

- Local: `invalidateLibraryOverviewCache()` + authoritative overview
  re-read — the new story card appears, titled with the family-correct
  default (`Histoire de ma Lunii (XXXXXXXX)` / `Histoire de mon FLAM
  (XXXXXXXX)` — the provenance is carried by the title).
- Device: `deviceLibrary.refresh()` re-reads the inventory; the device
  card now renders a `Dans ta bibliothèque` chip (tone success, glyph,
  folded into the card's `aria-label`) and the inspector CTA flips back
  to soft-disabled with `Copie indisponible: déjà dans ta bibliothèque`.
- The device selection (and keyboard focus) is PRESERVED across the
  re-read: the story still lives on the device — a copy is not a move.

Actionability rule (never an opaque refusal): EVERY `IMPORT_FAILED`
`details.source` below — AND the `DEVICE_UNSUPPORTED` /
`capability_gate` copy refusal (V3) — carries a non-empty `message`
(cause + impact) AND a non-empty `userAction` (the next gesture). The
Rust side is the authority for both strings; the frontend renders them
verbatim (it branches on `code` + `details.source` only to choose the
surface/affordance, never to compose the text). Backend tests lock this
invariant per refusal constructor; the inspector adds the
`Consulter le profil de support` next gesture for the profile refusal
(see `Device Story Inspection Contract`).

Error taxonomy — rejections cross the boundary as `AppError { code:
"IMPORT_FAILED" }` with a stable `details.source` from this closed set:

- `already_imported` — a `story_imports` row already links this `pack_uuid` to a local story.
- `pack_missing` — the pack UUID is no longer in the device index, or its `.content/<SHORT_ID>` folder is absent.
- `pack_invalid` — the pack content violates its family's structural contract. Lunii (known format): missing/empty required entry, unknown entry, non-regular file, depth exceeded. FLAM (opaque format): non-regular entry (symlink/special file), empty story folder (`details.cause = "empty_pack"`), empty directory (`"empty_directory"` — not round-trippable), depth exceeded. Both families: a staging path collision (`"staging_path_collision"` — exclusive staging writes, never a silent truncation).
- `pack_oversize` — the pack exceeds `MAX_IMPORT_PACK_BYTES` or `MAX_IMPORT_PACK_FILES`.
- `device_changed` — the live re-scan no longer resolves to the requested device (swapped / unplugged), or no supported device answers.
- `fs_read` — reading the pack from the device failed mid-copy. `details.kind` reuses the export I/O set (`permission_denied`, `no_space`, `read_only_filesystem`, `not_found`, `already_exists`, `io`).
- `staging_write` — writing into the local staging area failed (same `details.kind` set).
- `promote` — the atomic staging → `imports/<story_id>` rename failed.
- `db_commit` — the final SQLite transaction failed; the promoted folder is removed by compensation.
- `read_timeout` — the import budget (300 s) was exhausted. Carries `details.elapsed_ms`.
- `spawn_blocking_join` — the worker task could not be joined.
- `other` — fallback for unmapped causes.

A `DEVICE_UNSUPPORTED` rejection with `details.source =
"capability_gate"` (V3 profile) and the `DEVICE_SCAN_FAILED` re-scan
failures keep their existing taxonomies — the UI branches on `code` +
`details.source`, never on free-form strings.

## Transfer Decision / Comparison Contract

Before any send, the right-column decision panel shows a **read-only**
comparison between the selected local story and the live device inventory,
so the user understands the impact of a transfer *before* launching one.
The comparison is **composed in Rust** (command `read_transfer_preview`,
returning a `TransferPreviewDto`) and only **presented** by the panel — the
frontend never recomputes device truth. It is a snapshot read: the device is
re-scanned authoritatively at the moment of the decision, never mirrored.

The comparison block lives **inside** the decision panel, in a
`<section aria-label="Comparaison avant envoi" aria-live="polite">` between the
`Sélection courante` and `État de l'appareil` regions. The polite live region
lets the async `loading`→`ready` verdict be announced to screen readers (the
`error` case additionally carries `role="alert"`). It never competes with the
center-column collection as the main work surface, and it never hosts the
device library (which stays a distinct center-column section).

Each "no comparison" cause renders a **distinct, actionable** hint — the panel
never collapses them into one generic line. The route owns the cause (the hook
cannot tell a selection gap from a missing device apart):

| Comparison state | When | Panel rendering |
| --- | --- | --- |
| no comparison — no selection | No local story selected | `Sélectionne une histoire locale pour comparer avant l'envoi.` |
| no comparison — multi selection | More than one local story selected (multi-transfer is out of scope) | `Sélectionne une seule histoire locale pour comparer (le transfert multiple n'est pas encore disponible).` |
| no comparison — no readable device | Exactly one story selected, but no read-authorized device is connected | `Branche une Lunii lisible pour comparer l'histoire sélectionnée avant l'envoi.` |
| loading | The preview read is in flight | `ProgressIndicator` (calm) + `Comparaison en cours…` |
| ready — new | The selected story's pack is NOT on the device (no `pack_uuid`, or `pack_uuid` absent from the inventory) | `Nouvelle sur l'appareil` chip + `Cette histoire serait ajoutée à l'appareil.` + the unchanged-count line |
| ready — replace | The selected story's pack IS on the device (`pack_uuid` present in the inventory) | `Déjà présente sur l'appareil` chip + `Déjà présente sur l'appareil — un envoi la remplacerait.` + the unchanged-count line |
| device changed during comparison | A readable device WAS detected, but the authoritative re-read no longer resolves to it (unplugged / swapped, or the `ready` payload's identifiers don't match the request) | Recoverable `role="alert"` — `L'appareil a changé pendant la comparaison.` + next gesture + a `Réessayer` CTA. NOT the "branche une Lunii" hint, which would contradict the just-detected device. |
| error | The preview read failed (FS error, timeout, local store unavailable, selected story vanished) | In-context message (cause + impact + next gesture), `role="alert"` + `Réessayer`, **never a toast**. The local library and the device section stay intact. |

The unchanged-count line states what stays untouched:
`Aucune autre histoire de l'appareil ne sera modifiée.` (0),
`1 autre histoire de l'appareil restera inchangée.` (1), or
`{n} autres histoires de l'appareil resteront inchangées.` (n > 1).

Rules:

- The comparison is keyed on the pack identity (`pack_uuid`, the
  `story_imports` join key), never on the title or the device identifier — the
  same pack seen from another Lunii resolves to the same identity.
- It shows ONLY what changes the decision. **No size / volume metric** is
  shown in this contract (UX-DR37 — there is no decisional volume before the
  media-preparation step). A useful size reappears later, when preparation
  produces one.
- The send CTA (`Envoyer vers la Lunii`) stays **disabled with the standardized
  reason** `Envoi indisponible: transfert pas encore activé (MVP Phase 1)` in
  every state — the comparison informs, it never enables a write. The send
  capability is governed by the `WriteStory` gate (always `false` in MVP).
- The comparison never asserts a validation verdict: a local story remains a
  `brouillon local`. The `présumée transférable` / `bloquée` states belong to
  the compatibility-validation flow, not to this comparison.
- Errors carry the same closed `details.source` taxonomy as the device-library
  read it reuses (`device_changed`, `fs_read`, `pack_index`, `read_timeout`,
  `scan_timeout`, `os_enum`, `mount_unavailable`, `spawn_blocking_join`,
  `other`), plus `transfer_preview` for the local-store / missing-story cases
  (`LIBRARY_INCONSISTENT` when the selected story vanished,
  `LOCAL_STORAGE_UNAVAILABLE` on a local read failure). No new error code is
  introduced.

## Story Validation / Preflight Contract

Before any send, the right-column decision panel can run a **read-only**
`preflight` that composes a per-story validation verdict, so the user learns
whether an issue must be fixed *before* a transfer is attempted. Like the
comparison above, the verdict is **composed in Rust** (command
`read_story_validation`, returning a `StoryValidationDto`) and only
**presented** by the panel — the frontend never decides "valide / prête /
bloquée". It is an authoritative snapshot: the device profile is re-read at the
moment of the decision, never mirrored, and nothing is persisted (no
`validation_status` row — freshness beats caching on a decision surface).

**Two orthogonal axes (AC1).** The verdict distinguishes:

- **Rustory canonical validity** — is the LOCAL data sound? (axes `structure` /
  `media` / `filesystem`)
- **Lunii compatibility** — would the detected device profile accept this
  story? (axis `deviceProfile`)

Each blocker carries its `axis`, so the panel groups blockers under the two
headers (canonical vs Lunii) and the AC1 distinction stays visible.

**Verdict (a successful read, never an error).** The verdict is one of:

| Internal verdict | UI Label | Chip tone | Meaning |
| --- | --- | --- | --- |
| `presumed_transferable` | `présumée transférable` | success | No real block found (canonically valid + a recognized, supported profile). NOT a transfer success. |
| `to_fix` | `à corriger` | warning | A repairable block was found (e.g. an invalid title) — fixable before send. |
| `blocked` | `bloquée` | error | A hard canonical block was found (corrupt structure, checksum mismatch, unsupported schema). |

The verdict is derived in Rust: `bloquée` if any `Blocking` cause exists, else
`à corriger` if any `Fixable` cause exists, else `présumée transférable`. A
minimal story (a single empty start node) is canonically valid and is never a
block.

**Closed taxonomy `axis × cause` (AC2).** Every blocker is a closed
`(axis, cause)` PAIR — never a free-form string, never an independent axis and
cause, never two wordings for one cause. The runtime guard validates the pair
against this closed set (an impossible couple like `deviceProfile ×
checksumMismatch` is rejected, not grouped under the wrong heading). Severity is
a fixed property of the cause, and the verdict is DERIVED from the blockers
(blocking > fixable > none) — the guard also rejects a verdict that contradicts
its blockers (e.g. `bloquée` with no blocker). Each cause maps to one canonical
`message` (cause + impact) and one `userAction` (the next gesture); Rust owns
both strings, React renders them verbatim.

| Axis | Cause | Severity | Canonical `message` | Canonical `userAction` |
| --- | --- | --- | --- | --- |
| `structure` | `titleInvalid` | fixable | Le titre enregistré de l'histoire n'est pas valide. | Renomme l'histoire avec un titre valide puis relance la vérification. |
| `structure` | `schemaUnsupported` | blocking | Cette histoire utilise un format plus récent que celui pris en charge par cette version de Rustory. | Mets à jour Rustory pour transférer cette histoire. |
| `structure` | `structureCorrupt` | blocking | La structure interne de l'histoire est illisible ou incohérente. | Restaure une version saine de l'histoire puis relance la vérification. |
| `structure` | `checksumMismatch` | blocking | Les données locales de l'histoire ont changé de façon inattendue (corruption détectée). | Restaure une sauvegarde saine de l'histoire avant de la transférer. |
| `structure` | `duplicateNodeId` | blocking | Deux nœuds de l'histoire portent le même identifiant interne. | Restaure une version saine de l'histoire puis relance la vérification. |
| `structure` | `startNodeInvalid` | blocking | Le nœud de départ de l'histoire est introuvable. | Restaure une version saine de l'histoire puis relance la vérification. |
| `structure` | `brokenOptionLink` | fixable | Une option pointe vers un nœud qui n'existe plus. | Relie l'option vers un nœud existant ou retire-la ; les autres éléments de l'histoire restent valides. |
| `media` | `mediaUnsupported` | blocking | Ce média utilise un format non pris en charge. | Choisis une image PNG ou JPEG, ou un son MP3, WAV ou OGG. |
| `media` | `mediaUnreadable` | blocking | Ce média est illisible ou dépasse la taille autorisée. | Choisis un fichier plus léger et lisible puis réessaie. |
| `media` | `mediaSourceMissing` | fixable | Le fichier d'un média associé n'est plus accessible. | Ré-associe le média ou retire-le ; le reste du nœud reste modifiable. |
| `deviceProfile` | `metadataUnsupported` | blocking | Le profil de l'appareil connecté n'est pas pris en charge. | Consulte le profil de support pour voir les appareils compatibles. |
| `deviceProfile` | `metadataCorrupt` | blocking | Les marqueurs de l'appareil connecté sont incomplets ou illisibles. | Rebranche l'appareil puis relance la vérification. |
| `deviceProfile` | `familyUnknown` | blocking | La famille de l'appareil connecté n'est pas reconnue. | Branche une Lunii prise en charge puis relance la vérification. |
| `deviceProfile` | `multipleCandidates` | blocking | Plusieurs appareils compatibles sont connectés en même temps. | Ne garde qu'un seul appareil branché puis relance la vérification. |
| `deviceProfile` | `firmwareUnsupported` | blocking | Le firmware de la Lunii connectée n'est pas pris en charge. | Consulte le profil de support pour les firmwares compatibles. |
| `deviceProfile` | `operationNotAuthorized` | blocking | Le profil détecté n'autorise pas la lecture de la bibliothèque de l'appareil. | Consulte le profil de support pour comprendre ce qui est permis. |

**The `media` axis now has a LIVE emitter — the `Story Node Editor`** (see
`Story Node Editor Contract`), which is where a parent attaches and validates a
node's source media. That is where the `(axis = media, cause)` blockers above
are produced and surfaced inline, with the `needs attention` vs `blocked`
distinction. The TRANSFER preflight panel itself does not re-emit media blockers
for a native story: such a story is structurally coherent for the
canonical checks, and converting its source media to a device pack is a
preparation/transfer concern, not a canonical-validity one. The `media` causes
are nonetheless part of the closed wire taxonomy so the verdict surface stays
consistent across both contexts.

**`filesystem` AND `deviceProfile` are still declared without a live emitter
here** (the `deviceProfile` causes above are part of the closed wire taxonomy,
ready for a future device-format validation): filesystem failures surface as
transport `AppError`s; and the `deviceProfile` axis only ever applies to a
CONFIRMED readable supported device — which is compatible by construction in MVP
(a supported profile passes by definition, see
[device-support-profile.md](./device-support-profile.md)). A verdict is composed
ONLY when the live re-scan resolves to the requested readable supported device
(its identity matched). If the re-scan finds no device, an unsupported device, or
more than one (ambiguous), the present device's identity cannot be confirmed as
the one the UI asked about, so the read surfaces a recoverable `device_changed`
(see "No new error code") rather than a compatibility verdict on an unconfirmed
device — never false coverage, never a stale verdict.

**Verdict ⟂ send gate.** The verdict answers "did this story pass the canonical
+ profile checks?"; it is INDEPENDENT of whether a transfer is enabled. The
send CTA (`Envoyer vers la Lunii`) stays disabled with its standardized phase
reason `Envoi indisponible: transfert pas encore activé (MVP Phase 1)` in EVERY
verdict — even `présumée transférable` — because the send capability is
governed by the `WriteStory` gate (always `false` in MVP), never by the
verdict. Folding the gate into the verdict would make every story `bloquée` and
`présumée transférable` unreachable. A `bloquée` verdict does NOT change the CTA
reason: the gate reason is about the phase, while the verdict's block is
surfaced in the validation section itself.

**Surface.** The validation block lives **inside** the decision panel, in a
`<section aria-label="Validation avant envoi" aria-live="polite">`, sibling to
`Comparaison avant envoi`. The polite live region announces the async
`loading`→verdict transition; a transport `error` additionally carries
`role="alert"` and a `Réessayer` CTA, never a toast. The panel renders:
`loading` → a calm `ProgressIndicator`; a verdict → the verdict `StateChip`
(glyph + text, never color alone) + the blockers grouped by axis (`Validité
Rustory` / `Compatibilité Lunii`), each showing its cause copy and `userAction`;
`error` → the in-context recoverable message. Transitions follow the
`State Transition Rules`: `brouillon local → en vérification → {bloquée | à
corriger | présumée transférable}`.

**No new error code.** The verdict is a successful read (`Ok`, a tagged union).
Only transport failures become `AppError`: the selected story vanished
(`LIBRARY_INCONSISTENT`, `details.source = "story_validation"`), the local store
read failed (`LOCAL_STORAGE_UNAVAILABLE`, same source), or the device changed /
became unreadable mid-read (`DEVICE_SCAN_FAILED`, `details.source =
"device_changed"` and the rest of the device-library read taxonomy). The
`device_changed` case ALSO covers a re-scan that resolves to an unsupported or
ambiguous device (the requested readable device is no longer confirmable). The
only non-error read outcomes are `noDevice` (no device at all — the hook folds it
to the same recoverable "device changed" surface) and `ready` (the verdict). The
triggering command is `read_story_validation`; it emits no `device_log` event (a
light read-only snapshot).

## Story Preparation Contract

Once a story is `présumée transférable` (or its only blockers were fixable and
are now repaired) and a readable supported device is connected, Rustory can
**prepare** the artifacts a transfer would need. Preparation is a **purely local**
operation that produces **derived** artifacts: it never writes to the device and
does NOT depend on the `WriteStory` gate. Like the comparison and the validation
it sits beside, the preparation state is **composed in Rust** and only
**presented** by the panel — the frontend never recomposes phases or outcome from
raw events.

**Local ⟂ send gate.** Preparation only assembles, locally, *what a transfer
would need*, so it can be genuinely active in MVP Phase 1 even while the send CTA
(`Envoyer vers la Lunii`) stays disabled with its standardized reason
`Envoi indisponible: transfert pas encore activé (MVP Phase 1)`. The `Préparer`
action is gated on the validation verdict (`présumée transférable`), NEVER on
`WriteStory`; reaching the rest state never enables the send. This mirrors the
preview's `transferable = false` and the verdict ⟂ gate rule above. Folding the
send gate into preparation would make `Préparer` unreachable.

**Observable phases.** Preparation exposes the transfer machine's first two
phases; the rest/failure outcomes reuse the existing MVP states:

| Internal phase / state | UI Label | Notes |
| --- | --- | --- |
| `preflight` | `en vérification` | Re-runs the read-only validation + authoritative device re-scan before any assembly. |
| `preparing` | `en préparation` | Local artifact assembly + integrity re-check. May coexist with preserved local work. |
| `prepared` | rest state: keeps `présumée transférable` + a discreet `Préparée` indicator | Artifacts assembled and fresh. NOT a transfer success — it never enables the send. |
| `retryable` | `échec récupérable` | Keep enough context for `Relancer`. |

The `transfer` and `verify` phases of the machine are **never emitted by the
preparation flow** — they belong to the device-write flow (see the `Story
Transfer Contract` and the `Story Verification Contract` below, both implemented).
In MVP there is **no media transcoding** to perform (the available stories are
raw imported packs already in device format, or minimal native stories with an
empty `nodes`): the media transformer is declared but has no live implementation,
exactly like the `media` / `filesystem` validation axes. The real, testable work
is the observable phase progression plus a genuine local assembly — re-`preflight`
then enumerating and re-checksumming the required artifacts (the imported pack's
files, or the canonical structure) into an ephemeral transfer-artifact descriptor
stamped with a pipeline version.

**Long-running, non-blocking (AC2).** While preparation runs, the **library stays
usable** (navigation, search, selection, sort): the preparation lives in the
right-column panel and never becomes the visual center, never a modal, never a
tunnel, never a toast. Progress is **honest**: the current phase is always named;
a percentage appears ONLY when it is reliable, otherwise the named-phase state
stands on its own (no fake percentage). At minimum the `preparing` and `preflight`
states, and the still-blocking or ready elements, are shown.

**Failure is a state, never a faked success (AC3).** If preparation fails or is
interrupted (error, device unplugged during `preflight`, window close, missing /
corrupt artifact), Rustory **preserves the local draft in full** — the canonical
story is **never** mutated by preparation (only derived artifacts are produced;
in MVP the descriptor stays in memory, so there is not even a derived write to
compensate). The outcome is marked `échec récupérable` with the cause and the
next gesture (`Relancer`) **consultable in the current context** (`role="alert"`,
never a toast), never disguised as success and never promising a hidden partial
resume. The closed failure causes are: `preflightNotPassing` (the verdict is not
`présumée transférable` — the offending blockers are reported, reusing the
validation blocker grouping), `artifactMissing`, `artifactCorrupt`,
`deviceChanged` (the live re-scan no longer resolves to the requested device
during `preflight`), and `interrupted` (deadline / close). Each cause carries one
canonical `message` (cause + impact) and one `userAction`; Rust owns both
strings, React renders them verbatim.

**Surface.** The preparation block lives **inside** the decision panel, in a
`<section aria-label="Préparation" aria-live="polite">`, sibling to
`Comparaison avant envoi` and `Validation avant envoi`. The polite live region
announces each phase transition; a transport `error` additionally carries
`role="alert"` and a `Réessayer` CTA, never a toast. The panel renders:
`preflight` → a `StateChip` `en vérification`; `preparing` → `en préparation` +
calm progress (named phase; a `%` bar only when progress is reliable); `prepared`
→ the discreet `Préparée` indicator (the send stays disabled); `retryable` → the
in-context recoverable message (cause + `userAction`) + a `Relancer` button. Each
state uses a non-color signal (glyph + text). The story-card badge in the library
reflects `en préparation` / `échec récupérable` through the existing `StateChip`
(derived from the panel state, never a competing source of truth).

**Authoritative re-read, not event reconstruction.** Preparation is the first
long-running flow: a `start_prepare_story` command returns an acceptance
immediately and the work continues in the background, reporting through typed
`job:progress` / `job:completed` / `job:failed` events correlated by `job_id`,
each carrying a monotonic `sequence` so consumers stay idempotent and tolerate
late / duplicate delivery. On a terminal event the UI performs an **authoritative
re-read** (`read_preparation_state`) rather than rebuilding truth from the events
alone. The `preflight` follows the same I/O-first / scoped-DB-lock pattern as the
comparison and the validation; a device change during `preflight` surfaces the
recoverable `deviceChanged` outcome. The `preparing` phase is local and does not
require the device to stay plugged in.

**No new persistence.** The transfer-artifact descriptor is **ephemeral and
re-derivable**, like the preview and the verdict: no migration, no
`preparation_outputs` row, no job record in MVP (freshness beats caching; the
transfer is not resumable, so each attempt re-prepares). AC3's "consultable
detail" is satisfied by the in-context state (the hook / panel keeps the last
result + error), exactly like the recoverable errors of the comparison and the
validation. Durable derived artifacts + their table are deferred to the real
transfer step.

**Error contract.** The preparation states (`preflight` / `preparing` /
`prepared` / `retryable`) are outcomes of a **successful** read (a tagged union +
event payloads), never an error. Only a **transport** failure that prevents even
producing a terminal job outcome becomes an `AppError` — a new code
`PREPARATION_FAILED`, reserved for transport. A **functional** preparation failure
(missing / corrupt artifact, `preflight` not passing, interruption) is the
terminal `retryable` state of the job, not a raw `AppError`.

## Story Transfer Contract

Once a story is **`Préparée`** (a transfer-artifact descriptor was assembled — so
an **imported** story carrying device-format pack files) and a **write-authorized**
Lunii is connected, Rustory can run the real **transfer** — the FIRST device write.
The transfer state is **composed in Rust** and only **presented** by the panel.

**Gate before mutation, fail-closed (AC2 / FR34).** The `WriteStory` capability is
checked **before any write I/O**: the send is allowed only on a write-authorized
cohort. In MVP Phase 1 the matrix wires writes for **Lunii Origine v1** and
**Mid-Gen v2**; **V3 stays read-only** (active reverse-engineering, same rationale
as import) and FLAM Gen1 is recognized with zero capability (its write stays ❌
through the same gate). The `Envoyer vers la Lunii` CTA is therefore
**activable** — active ONLY on (write-authorized cohort + `Préparée` story + a
single clear target), disabled everywhere else with a standardized
`Envoi indisponible: …` reason. **No confirmation modal** is shown when the
context and target are unambiguous (single local selection + one writable device).

**Observable phase.** The transfer drives the machine's `transfer` then `verify`
phases on the same job (the `verify` phase is the FINAL phase — see the `Story
Verification Contract` below):

| Internal phase / state | UI Label | Notes |
| --- | --- | --- |
| `transferring` | `en transfert` + honest progress | The write runs in the background; the library stays usable. A `%` bar appears ONLY during the measurable content copy — never a fake value nor 100 % before the terminal; a named phase otherwise. A non-destructive `Consulter le détail` discloses the phase / progress in-context (no cancel — out of scope). |
| `verify` (TRANSIENT, not a resting terminal) | `écriture effectuée — vérification à venir` | The write is done; the read-only verification re-read is running. It settles to one of the verdicts below. |
| `verified` (terminal) | `transférée et vérifiée` | Verification PROVED the write (indexed + content present + byte-faithful). The sober success + the AC2 summary (what changed / stayed unchanged / final state). The chip NEVER varies; only the summary's `changed` line names the write outcome the writer constated — first send / update / already up to date (see the three frozen variants in the Story Verification Contract below). |
| `partial` (terminal) | `état partiel` | Verification re-read the device but confirmed only an incoherent/incomplete result. Never collapsed into success. Distinct from `transfert incomplet`. `Relancer` / `Abandonner`. |
| `retryable` (`échoué`) | `échec récupérable` | The device was left UNTOUCHED (a write-phase refusal), OR the verification could not confirm the result (device gone / unreadable during `verify`). Keep enough context for `Relancer` / `Abandonner`. The local draft is preserved in full. |
| `incomplete` | `transfert incomplet` | The write STARTED then was interrupted (device mutated): the Lunii may hold a partial copy; a relaunch (full cycle) restores a safe state. Distinct from `état partiel`. `Relancer` / `Abandonner`. |

**No false success (AC3).** No success is communicated until BOTH the write AND the
verification have completed. After a successful write the job enters the TRANSIENT
`verify` phase (`écriture effectuée — vérification à venir`), which settles to
`transférée et vérifiée` (proof passed), `état partiel` (re-read incoherent) or
`échec récupérable` (re-read could not confirm) — `transférée et vérifiée` is
**never** shown without the verification proof. An interruption / failure (device unplugged mid-write,
`.content` not writable, no space, a stale / corrupt descriptor, a native story
with no device-format pack) is the terminal `retryable` state: the canonical story
is **never** mutated (FR18), there is **no partial resume** (a failed transfer
requires a fresh full cycle), and the recoverable detail is shown **in context**
(`role="alert"`, never a toast) with `Relancer`.

**Échoué vs incomplet (AC2).** An interruption resolves to one of two honest
terminals, classified by a property of the DEVICE — whether the write reached the
device mutation — not by the cause: **`échoué`** (`échec récupérable`) when the
device was left untouched (the failure happened before the atomic promotion), and
**`incomplet`** (`transfert incomplet`) when the mutation had started (a promoted
folder may exist before the index update). The writer reports a
`reached_device_mutation` signal; the closed cause taxonomy stays orthogonal.
Neither is a success nor a false failure; a relaunch is always a full cycle (never
a hidden partial resume), and the writer PROVES the state of an existing target
pack — identical → idempotent reuse, divergent-but-sound → atomic replacement,
unprovable → the dedicated refusal below — so a relaunch converges safely (the
full three-outcome contract lives in
[device-support-profile.md#Capability Gate Contract](./device-support-profile.md)).

**Update without a modal (FR23, explicit UX decision).** Updating a story already
present on the device introduces NO confirmation modal, NO new CTA and NO consent
flag: `Envoyer vers la Lunii` stays the single action (the Confirmation Rules —
`Exporter` and `Envoyer` ask no default confirmation — hold). AC1's "never
silently" is realized by three existing mechanisms instead of friction: (a) the
pre-send comparison already says `Déjà présente sur l'appareil — un envoi la
remplacerait.` BEFORE the send (its presence-only contract and copy stay
VERBATIM); (b) an unprovable device state is REFUSED with zero byte modified;
(c) the terminal summary NAMES what really happened (update vs first send).

**Unprovable device pack (dedicated refusal, honest copy).** When the write-job
state proof meets a state it cannot vouch for (a symlinked or non-directory
target root, a symlink / unplanned EMPTY directory / special file inside — a
non-empty out-of-plan directory is a container whose files decide, see the
device-support profile —, an entry whose bytes could not be read, an unreadable
I/O during the proof, or a divergent folder that cannot be ATTRIBUTED to the
target UUID: not referenced by the device index, or another indexed UUID shares
the target SHORT_ID — either way the folder would be clobbered blindly), the job
ends `retryable` with the dedicated cause `devicePackUnprovable` and this frozen
copy — message: `Envoi
interrompu : la copie présente sur l'appareil est dans un état que Rustory ne
reconnaît pas, rien n'a été modifié.`; next gesture: `Vérifie l'appareil,
débranche-le puis rebranche-le, puis relance l'envoi.` The copy says RUSTORY is
protecting the present content — never that the device refused the write
(`writeRejected` keeps its existing copy for the pure write-I/O failures it
still owns). Rendered by the existing `retryable` path (`role="alert"`,
in-context, never a toast).

**Context preserved in-session (AC3).** The `échoué` / `incomplet` outcome (cause
+ message + next action) lives in the CURRENT context — the live state of the
transfer hook, kept sticky (a late `job:progress` never regresses it) — until the
user chooses `Relancer` (full cycle) or `Abandonner` (back to a stable library,
draft intact). It is NOT carried by the job-shell store (which holds only
phase / progress).

**Durable cross-session memory (AC2).** The LAST terminal outcome ALSO survives an
app restart and a later return to the library / device panel: it is persisted to a
minimal `transfer_jobs` table (one row per story, PK `story_id`, FK `stories(id) ON
DELETE CASCADE`, UPSERT "latest wins" — the exact shape of `story_drafts`). Only
TERMINALS are written (`verified` / `partial` / `retryable` / `incomplete`), never
an in-flight `transferring` / `verifying` phase (it would be a lie after a restart —
the job died with the app). The full rules — reconciliation, consumption / purge,
boot probe — live in the **Transfer Resume Contract** below.

**Closed write-error taxonomy.** `fs_write` (write / space failure on the device),
`device_changed` (the live re-scan no longer resolves to the requested device),
`checksum_mismatch` (the assembled artifact no longer matches its stored
reference), `timeout` (budget exceeded), `interrupted` (deadline / close /
unplugged), `device_pack_unprovable` (the already-present pack under the target
folder is in a state the write-job proof cannot vouch for — Rustory refuses
protectively, zero device byte modified). Each maps to one canonical `message` +
`userAction` — Rust owns both strings, React renders them verbatim.

**Safe, atomic, offline write.** The write reuses the safe-write pattern: stage on
the device volume → promote atomically (`rename`) → `fsync` the promoted tree +
parent → update the device pack index atomically (files first, index after — a
pack is never indexed without its content present). Zero network I/O (USB only —
FR19). On failure the staging is swept; the canonical draft is untouched.

**Authoritative re-read, not event reconstruction.** A `start_transfer_story`
command returns an acceptance immediately and the write runs in the background,
reporting through the shared typed `job:progress` / `job:completed` / `job:failed`
events correlated by `job_id` (`jobType = "transfer_story"`, monotonic `sequence`).
On a terminal event the UI performs an **authoritative re-read**
(`read_transfer_state`) rather than rebuilding truth from the events alone.

**Surface.** The transfer block lives **inside** the decision panel, in a
`<section aria-label="Transfert" aria-live="polite">`, sibling to the preparation
region. It renders: `transferring` → a `StateChip` `en transfert` + honest
progress + a non-destructive `Consulter le détail` disclosure; the TRANSIENT
`verify` phase → the factual `écriture effectuée — vérification à venir` line;
`verified` → a `success` `StateChip` `transférée et vérifiée` + the sober summary
(`« <Titre> » est maintenant sur la Lunii` + how many other stories stayed
unchanged), `aria-live="polite"`, never an alert; `partial` → a `warning`
`StateChip` `état partiel` (distinct text from `transfert incomplet`) in
`role="alert"`; `retryable` (`échoué`) → `échec récupérable`; `incomplete` → the
distinct `transfert incomplet` chip (its own glyph) + the partial-copy message;
the non-success failure terminals offer `Relancer` AND `Abandonner`; transport
`error` → the in-context message + `Réessayer`; never a toast. Each state uses a
non-color signal (glyph + text). The story-card badge reflects `en transfert` /
`transférée et vérifiée` / `état partiel` / `échec récupérable` / `transfert
incomplet` through the existing `StateChip`.

**Durable resume memory.** Unlike the preview / verdict / preparation, the LAST
terminal outcome IS persisted (the minimal `transfer_jobs` table, migration
`0005_transfer_jobs.sql`), so the panel can re-offer `Relancer` / `Abandonner`
after an app restart — see the **Transfer Resume Contract**. The live appliance
stays the truth for a `verified` re-derivation (re-scan + re-checksum via
`read_transfer_state`); the durable memory only fills the gap the appliance cannot
reproduce (the non-success terminals), and a `Relancer` is always a full fresh
cycle (never a hidden partial resume).

**Error contract.** The transfer states (`transferring` / `verify` / `verified` /
`partial` / `retryable`) are outcomes of a **successful** read, never an error.
Only a **transport** failure that prevents even producing a terminal job outcome
becomes an `AppError` — `TRANSFER_FAILED`, reserved for transport (the exact
parallel of `PREPARATION_FAILED`). A **functional** transfer/verify failure is a
terminal job state (`retryable` / `partial`), not a raw `AppError`; the verify
verdicts are job states too, never new error codes.

## Story Verification Contract

The **`verify` phase is the FINAL phase of the same `transfer_story` job**, emitted
automatically after a successful write — never a separate command or job. It is the
explicit proof the reliability NFR requires ("no success shown before the required
verification step completes"): it PROVES what the write CLAIMS.

**Read-only re-read, gated `ReadLibrary`.** Verification re-scans the device, then
reads its inventory through the proven `read_library` path. It reuses the
`ReadLibrary` capability (true for every MVP cohort) — there is **no new
`SupportedOperation`**. Because the write itself is gated `WriteStory` (V1/V2 ✅,
V3 ❌), `verify` only ever runs after a write on a write-authorized cohort, so the
success path (`verified`) is demonstrable on V1/V2 or a fake mount; V3 keeps
blocking **before** the write, never reaching `verify`. It writes nothing — the
device and the canonical draft are never mutated (FR18).

**Three honest verdicts.** From the re-read facts:

| Verdict | UI Label | When |
| --- | --- | --- |
| `verified` | `transférée et vérifiée` | The UUID is indexed in `.pi`, the `.content/<SHORT_ID>` folder is present, AND the device bytes re-checksum to the prepared baseline (byte fidelity). |
| `partial` | `état partiel` | The device was mutated and the pack is present but NOT fully coherent (e.g. indexed but the bytes diverge, or content present but not indexed). A non-success, never a silent success. |
| `failed` | `échec récupérable` | The re-read PROVES the write did not land (pack absent) OR cannot confirm it (device gone / unreadable during `verify`). A reconnected relaunch re-verifies. |

**What is really verifiable.** On an opaque imported pack, verification proves —
offline, key-free — that the UUID is indexed, the content folder is present, and
the written bytes re-checksum to the prepared artifact's baseline (the EXACT import
aggregation: `rel_path` + NUL + bytes, in manifest order). It does **not** decrypt,
inspect media, or validate the internal pack structure. `transférée et vérifiée`
means **byte fidelity + indexing confirmed — nothing more**; it never implies a
semantic content validation.

**Continuity, not pre-write identity.** A successful write mutates `.pi`, so the
device identity Rustory derives from it legitimately CHANGES across the write —
the pre-write identifier can no longer be re-pinned. The in-job verify therefore
binds to the device it WROTE TO via a STABLE proof that survives the `.pi`
mutation: the USB **volume serial** (falling back to the written **mount path**
when no serial is available). A Lunii swapped after the write for ANOTHER
supported device — even one that already holds the same pack + bytes — fails this
continuity check and ends `failed`, so `verified` is never attributed to the wrong
device; a vanished / ambiguous device is `failed` too. Content presence is probed
on `.content/<SHORT_ID>` INDEPENDENTLY of the `.pi` index (a promoted-but-unindexed
pack reads as present ⇒ `état partiel`, not a false `failed`); a readable byte
DIVERGENCE is `état partiel`, while an ABSENT pack or an UNCONFIRMABLE re-checksum
is `failed`. The later authoritative `read_transfer_state` is pinned to the
(re-detected) target device.

**Confirmation summary (FR15/FR23), composed in Rust, carried on the terminal.**
On `verified` the panel shows a sober confirmation summarizing what changed and
what stayed unchanged (the N other device stories — reusing the comparison's
`unchanged_count`). The `changed` line NAMES the outcome the writer CONSTATED
(never deduced from a pre-write state), bifurcated by the FRESH preflight
profile's family (the Lunii wording stays VERBATIM; the generic variants are
declared-without-a-live-emitter until a non-Lunii write exists — the
`formatSendCtaLabel` pattern). Three frozen variants:

| Write outcome | Lunii panel (VERBATIM) | Non-Lunii family panel |
| --- | --- | --- |
| `created_new` (first send) | `« <Titre> » est maintenant sur la Lunii.` | `« <Titre> » est maintenant sur l'appareil.` |
| `replaced_divergent` (update) | `« <Titre> » a été mise à jour sur la Lunii.` | `« <Titre> » a été mise à jour sur l'appareil.` |
| `reused_identical` (already up to date) | `« <Titre> » était déjà à jour sur la Lunii.` | `« <Titre> » était déjà à jour sur l'appareil.` |

The state chip stays `transférée et vérifiée` in ALL three cases (controlled
vocabulary unchanged) — only the summary line distinguishes; `reused_identical`
is named honestly (a send that changed nothing never claims a replacement). Both
lines are **composed in Rust** and travel READY-MADE on the `job:completed`
event, so the UI renders the success straight from the terminal — never via a
re-read with the now-stale pre-write identifier — and never recomposes the text
in React. The passive `read_transfer_state` re-derivation keeps the
present-state wording (`est maintenant sur…`): a passive re-read proves PRESENCE,
never which write outcome produced it. `aria-live="polite"`, never a toast /
modal. `état partiel` and `échec récupérable` are shown `role="alert"` in-context
with `Relancer` / `Abandonner`.

**Durable resume memory.** The verdict lives in the live transfer-hook state (sticky
via the same settle/teardown discipline); the `verified` success is settled from
the `job:completed` summary, and `read_transfer_state` can also re-derive `verified`
on demand (same re-scan + re-checksum) for a re-mount with a freshly detected id. A
transient `partial` / `failed` verdict is NOT reproduced by the passive re-read — it
belongs to the live session, but it IS now remembered across an app restart by the
durable `transfer_jobs` memory (migration `0005_transfer_jobs.sql`): on re-mount the
panel re-hydrates the last `état partiel` / `échec récupérable` / `transfert
incomplet` terminal so the user can still `Relancer` / `Abandonner`. The
reconciliation rule still forbids promoting an `idle` live read to `verified` from
memory (no false success) — see the **Transfer Resume Contract**.

## Transfer Resume Contract

A transfer's last TERMINAL outcome can survive an app restart (or a later return to
the library / device panel). When the story is selected again, the panel re-hydrates
the surviving outcome so the parent can still `Relancer` (a full fresh cycle) or
`Abandonner` (purge the memory, library intact). The canonical story is never touched
(FR18); a relaunch is never a hidden partial resume.

| Aspect | Value |
| --- | --- |
| Trigger rule | When a story is selected, `useStoryTransfer` calls `read_transfer_outcome({ storyId })` and seeds its sticky state with a remembered NON-success terminal (`partial` / `retryable` / `incomplete`) so the panel re-shows `état partiel` / `échec récupérable` / `transfert incomplet` + `Relancer` / `Abandonner`, exactly as if the `job:failed` had just fired. `settledRef` is preserved, so a remembered terminal is never regressed by a late live re-read. |
| Reconciliation rule | The live `read_transfer_state` ALWAYS wins for `verified`: a connected device that proves the pack present + byte-faithful is `transférée et vérifiée`, whatever the memory says. The memory only supplies what the live read cannot reproduce: the non-success terminals, and — when NO device is connected — the last known result, presented as a RECALL of the last transfer, never as a live device truth. It is FORBIDDEN to promote an `idle` live read to `verified` from memory (that would be a false success). |
| Detection rule | At boot, `lib.rs::run().setup` queries `SELECT story_id FROM transfer_jobs WHERE terminal_kind <> 'verified' ORDER BY recorded_at DESC` (capped) and emits a single `interrupted_transfer_detected` event into `{app_data_dir}/diagnostics/transfer.jsonl`. `verified` rows are EXCLUDED — a quit-after-success has nothing pending to acknowledge (it is never re-surfaced by `hydrate`), so it must not raise a false interruption; the `verified` row is kept only for the "latest wins" overwrite of a failure by a successful relaunch. The boot probe never blocks on a log write failure. The per-story re-hydration is driven by the route-level read, not the probe. |
| Surface | The remembered terminal is rendered in the panel's anchored transfer region (`role="alert"` for a non-success, `aria-live="polite"` for a recalled `verified`), never a toast / modal; the story-card badge reflects the remembered outcome through the existing `StateChip`. No UI jargon (`job` / `write` / `staging` / `.pi` / `checksum` / `verify` / `transfer_jobs`). |
| Badge derivation (scope) | The story-card badge is derived from the SINGLE tracked transfer-hook state and is re-hydrated PER SELECTION (the route effect keyed on the selected story), NOT seeded at boot. So after a restart, a story with a remembered recoverable terminal shows its badge only once it has been selected at least once (re-visiting the story restores it — AC2). The boot probe only emits one `interrupted_transfer_detected` trace; it does not push per-story badges to the front. Seeding every visible story's badge at boot is a deliberate non-goal (it would mean reading the whole table eagerly for a cosmetic anchor). |
| Relaunch rule | `Relancer` re-runs the WHOLE cycle (`preflight → prepare → transfer → verify`) from the preserved local draft via the existing send path, with the CURRENT/fresh `writableDeviceId` — never the stored `device_identifier` (a write mutated `.pi`, so the stored identity is stale by construction). Gated on `writableDeviceId !== null`; otherwise the panel shows `Rebranche la Lunii pour relancer` (the fail-closed mirror of the send gate) while keeping `Abandonner`. Single-flight: `Relancer` is disabled while a job is in flight. |
| Consumption / auto-clear rule | A terminal UPSERTs the row (`BEGIN IMMEDIATE`, the DB lock released BEFORE any diagnostics write — the `recovery.rs` discipline). `Abandonner` purges the row (idempotent DELETE) and returns to a stable library, draft intact. A successful relaunch ending `verified` sets the row to `verified` (the last useful result becomes the success). Quitting without acting keeps the row for the next session. |
| Persisted terminals | `verified` (carries `summary_changed` / `summary_unchanged`), `partial` (`verify_verdict = "partial"`), `retryable` (`cause` + `completeness = "failed"`, plus `verify_verdict = "failed"` when the verify re-read could not confirm), `incomplete` (`cause` + `completeness = "incomplete"`). An in-flight `transferring` / `verifying` phase is NEVER persisted. |
| Stable diagnostic categories | The transfer log adds `interrupted_transfer_detected` to its closed category set (alongside `transfer_started` / `transfer_completed` / `transfer_failed`). The category is the stable identifier — never a localized message, never a free-form string. |
| Degradation rule | Reading the memory on mount is BEST-EFFORT: a read failure is logged and treated as "no memory" (it never breaks the flow or blocks the UI — this is operational observability). A write / purge failure (and the explicit `Abandonner`, where a purge failure must be visible) surfaces the new `TRANSFER_OUTCOME_UNAVAILABLE` error code, RESERVED for the persistence transport (SQLite / diagnostics) — never for a functional transfer failure (which stays a job terminal). |
| Forbidden | No modal. No toast. No automatic relaunch without a user choice. No promoting an `idle` live read to `verified` from memory. No mutation of the canonical story or its draft by the memory or the relaunch. |

### Error payloads — transfer resume flow

- `record_transfer_outcome` UPSERT fails — `TRANSFER_OUTCOME_UNAVAILABLE`, `details.source = "sqlite_upsert"`. A FK violation (the story vanished) re-maps to `LIBRARY_INCONSISTENT` with `details.source = "story_missing"`.
- `read_transfer_outcome` SELECT fails — best-effort on mount: the command LOGS a `transfer_outcome_unavailable` trace (`source = "sqlite_select"`) and returns `null` (the hook treats it as "no memory"); the read never rejects for a transport failure.
- `discard_transfer_outcome` DELETE fails — `TRANSFER_OUTCOME_UNAVAILABLE`, `details.source = "sqlite_delete"`.
- A blocking worker that cannot be joined (panic / cancel) — `TRANSFER_OUTCOME_UNAVAILABLE`, `details.source = "spawn_blocking_join"` (the read additionally logs the trace and resolves to `null`).
- `transfer_log` write fails — `TRANSFER_OUTCOME_UNAVAILABLE`, `details.source ∈ { diagnostics_dir, diagnostics_open, diagnostics_write, diagnostics_serialize, diagnostics_clock, diagnostics_rotate, diagnostics_path_invalid, diagnostics_app_data_dir }`.

## Official Catalog Contract

The right column hosts a quiet `Catalogue officiel` panel that manages the
local commercial-catalog cache used for title recognition (see
[device-support-profile.md#Title Recognition & Catalog Policy](./device-support-profile.md)).
It is global, not device-specific — caching the catalog recognizes packs even
before a device is plugged in.

| Panel element | Content / behavior |
| --- | --- |
| Status | `N titre(s) officiel(s) en cache` (or `Aucun titre officiel en cache`). Read on mount via `get_official_catalog_status` — the ONLY automatic call (a bounded count query, no network). |
| Offline-first note | States plainly that Rustory contacts no server without a deliberate action. |
| `Récupérer / mettre à jour` | The ONLY networked action: guest auth → catalog download → EAGER cover download into the local cache, on explicit click. Soft-disabled + `aria-busy` while in flight. |
| `Importer depuis un fichier` | 100%-offline alternative: a native open-file dialog picks a catalog file Rust reads/parses/caches (titles only — no cover download on the offline path). A cancelled dialog is a silent no-op. |
| Failure | An in-context `role="alert"` (never a toast) with the normalized message + next gesture and an explicit `Fermer`. |

Covers are cached LOCALLY during the refresh and rendered from that cache:
a recognized entry's `thumbnail` carries the local cache file name (never a
remote URL), and the UI loads the image through the `read_pack_cover` command
(a local read returning a `data:` URL — no network on display). Covers are
decorative (`aria-hidden`); the title carries the accessible name. The offline
file import caches no cover.

Catalog failures carry `code = OFFICIAL_CATALOG_UNAVAILABLE` with
`details.source` ∈ `network` (stage `auth_request` / `packs_request` /
`auth_token` / …), `parse` (stage `json` / `shape` / `empty` / …), `import`
(stage `metadata` / `oversize` / `read` / …), `cover` (stage `not_an_image` /
`oversize` / …) or `storage`. The UI branches on `code`, never on free-form
strings. A missing cover is NOT an error — `read_pack_cover` resolves `null`
and the title shows alone.

## UI Foundation Components

The MVP design system ships a closed core of foundation primitives in `src/shared/ui/`. Feature code consumes them through the barrel export `src/shared/ui/index.ts` — no direct file-by-file import path needed.

| Primitive | Responsibility | Key contract |
| --- | --- | --- |
| `Button` | Interactive action | `variant: "primary" \| "secondary" \| "quiet" \| "destructive"`; when `aria-disabled="true"`, the button stays focusable and clicks are swallowed (keyboard users must reach the reason) |
| `Field` | Labelled text input | Visible `label`, stable `id`, `onChange` receives the next string (never the raw event) |
| `SurfacePanel` | Neutral container surface | `elevation: 0 \| 1 \| 2`, `as`, `ariaLabelledBy` — no business logic |
| `StateChip` | Status pill | `tone: "neutral" \| "info" \| "success" \| "warning" \| "error"`; every tone ships with an ASCII glyph so the signal survives grayscale / color-blindness |
| `ProgressIndicator` | Long-operation feedback | `mode: "indeterminate" \| "determinate"`, always a visible `label`, respects `prefers-reduced-motion` |
| `Dialog` | Modal surface | Native `<dialog>` (free focus trap + Escape); `open`, `onClose`, `title`, `ariaDescribedBy` |
| `Toast` | Lightweight confirmation | `tone` typed to exclude `"error"` at compile time — critical errors never live in a toast alone |

New shared primitives land here only when a motif has appeared at least three times with stable behavior (see UX spec — Core Set Boundary).

## Library Selection Contract

The library supports single and multi selection from the story collection. The contract below is authoritative — feature code MUST NOT invent alternative selection models.

| Aspect | Value |
| --- | --- |
| Source of truth | `selectedStoryIds: ReadonlySet<string>` on the `libraryShell` store (`src/shell/state/library-shell-store.ts`) |
| Persistence | None — selection never survives a restart (avoids stale truth after the app relaunches). |
| Visibility vs selection | Filtering and sorting NEVER change selection. An off-screen selected id stays selected; the counter makes the distinction explicit (`X sur Y — Z sélectionnée(s)`). |
| Purge rule | On every successful `getLibraryOverview` read, the route calls `pruneSelection(presentIds)` to drop ids absent from the fresh overview. The cause is always explicable ("l'histoire a disparu de la bibliothèque locale"). |
| Non-color signal | Selected cards render a colored border, a visible textual prefix (`✓`) in the DOM and `aria-pressed="true"`. Selection survives grayscale and color-blindness checks. |
| Wording | `Aucune histoire sélectionnée` · `1 histoire sélectionnée` · `N histoires sélectionnées`. Strict singular/plural — never `picked`, `checked`, `active`. |

## Story Card Interaction Contract

A `Story Card` is a `role="button" aria-pressed={selected}` focus stop. Interaction maps to:

| Input | Effect |
| --- | --- |
| Click (no modifier) | `replace` — the card becomes the unique selection. |
| `Ctrl+click` / `Cmd+click` | `toggle` — add or remove the card from the current selection. |
| `Shift+click` | Not implemented in the MVP (deferred). |
| Double-click | Open the edit route (`/story/:storyId/edit`). |
| `Enter` on the focused card | Open the edit route. |
| `Space` on the focused card | `toggle` the selection for that card. |
| `Tab` / `Shift+Tab` | Linear traversal through the collection (no composite listbox keyboard model at this stage). |

A click that lands outside a card (header, controls, empty space, other columns) MUST NOT modify the selection — UX-DR7 forbids silent disappearance of selection.

## Library Routing Contract

`React Router` is the sole source of truth for navigation; the Zustand shell store never mirrors the active route.

| Path | Route | Purpose |
| --- | --- | --- |
| `/` | redirect to `/library` | Default entry point. |
| `/library` | `LibraryRoute` | Three-column library context (see `Library Layout Contract`). |
| `/story/:storyId/edit` | `StoryEditRoute` | Resume a `brouillon local` for a library-owned story. No device call, no mutation at this stage. |
| `/settings` | `SettingsRoute` | Read-only `Profil de support` screen (see `Support Profile Screen Contract`). Standalone single-column view; zero network, zero mutation. |
| `*` | redirect to `/library` | Unknown paths bounce back to the library. |

Rules:

- Returning to `/library` from `/story/:storyId/edit` preserves shell continuity (selection, filters) through the Zustand store — the URL does not carry that state.
- New routes land here only when a real dominant-context switch appears. The `settings` context is wired by the `Profil de support` screen — a real dominant-context switch, read-only by contract.
- The library left column carries the permanent navigation entry to `/settings` (below the filters block, no business state — see `Support Profile Screen Contract`); the column's `<nav>` label is the generalized `Navigation bibliothèque`.

## Story Creation Contract

A new `brouillon local` is created through a single modal dialog in the `library` context. No other surface exposes a creation entry point.

| Aspect | Value |
| --- | --- |
| Entry points | Header CTA `Créer une histoire` inside `Story Collection`, plus the same CTA inside the `loaded-empty` region. Both are active in parallel whenever the route wires a handler; they dispatch the exact same flow. |
| Input | A single `Titre` field. No `description`, `genre` or `cover image` at this stage — richer story inputs are deferred. After a successful creation the router lands on the `Story Editor Shell` (see `Story Editor Shell Contract`). |
| UI validation | Mirrors the Rust domain rules so `aria-disabled` flips at typing speed: the normalized title (NFC + trim) must be non-empty, at most `120` Unicode code points, and contain no C0 / C1 control characters nor any code point from the Unicode denylist below. |
| Unicode denylist | Beyond C0 (`U+0000..U+001F`) and C1 (`U+007F..U+009F`) controls, the following `Cf` and line-separator code points are rejected because they would make a title hidden, bidirectionally ambiguous, or carry embedded line breaks: `U+FEFF` (BOM / ZWNBSP), `U+202A..U+202E` (LRE / RLE / PDF / LRO / RLO bidi overrides), `U+2066..U+2069` (LRI / RLI / FSI / PDI bidi isolates), `U+200E` (LRM), `U+200F` (RLM), `U+061C` (ALM), `U+2028` (LINE SEPARATOR), `U+2029` (PARAGRAPH SEPARATOR). ZWJ (`U+200D`) and ZWNJ (`U+200C`) are deliberately allowed — they are load-bearing for many scripts and emoji sequences. |
| Authoritative validation | Rust re-validates on every `create_story` call; a title that slipped past the UI is refused via `AppError { code: "INVALID_STORY_TITLE" }` and no row is inserted. |
| Canonical model | Persisted with the current canonical schema (`schema_version = 3`) and the minimal `CanonicalStructure` — a single empty start node: `{ "schemaVersion": 3, "startNodeId": "n1", "nodes": [{ "id": "n1", "text": "", "label": "", "imageAssetId": null, "audioAssetId": null, "options": [] }] }`. Any future extension of the canonical shape MUST bump `schema_version` and ship a migration. |
| Integrity | `content_checksum` is the SHA-256 hex digest of the exact `structure_json` bytes written to disk. |
| Timestamps | `created_at` equals `updated_at` on first insert; both are ISO-8601 UTC at millisecond precision (`YYYY-MM-DDTHH:MM:SS.sssZ`). |
| Ordering | Library default sort is `ORDER BY created_at ASC, id ASC`. UUIDv7 keeps the ordering stable without an extra secondary key. |
| Post-success flow | The module-local SWR cache for `useLibraryOverview` is invalidated, a fresh fetch is triggered, and the router navigates to `/story/:storyId/edit` with `replace: true` so the history stack stays flat. |
| Failure recovery | On rejection, the dialog stays open, the typed title survives, the focus returns to the field, and the Rust-supplied `message` + `userAction` are rendered inside a `role="alert"` region below the field. |

The dialog is the creation CHOICE: the interactive path (title → `Créer`)
stays primary, and up to two OPTIONAL secondary entries hand over to the
other creation flows — `Ou démarre depuis un dossier préparé hors de
Rustory` + `Choisir un dossier…` (see `Structured Folder Creation
Contract`) and `Démarrer depuis une source externe (RSS)` (see `External
Source Creation Contract (RSS)`). Each secondary entry renders ONLY when
the route wires its handler, closes the dialog first, then delegates; both
are gated by the same cross-flow busy exclusivity (two review surfaces /
native dialogs must never stack). No extra button lands in the library bar.

The CONTENT-SOURCE section of the dialog (the RSS entry plus the known
non-activated kinds) is additionally DRIVEN by the distribution policy
(see `Content Source Activation Contract`): the route reads
`read_content_source_policy` when the dialog opens (a pure, point-in-time
read — no cache, no authoritative frontend state) and hands the result to
the dialog. An `enabled` kind renders its active entry plus the frozen
entry-level activation marker (`Activée par la distribution officielle`);
a non-enabled kind renders VISIBLE but DISABLED (`aria-disabled`, the
reason keyboard-reachable) with the Rust-carried frozen reason (the
`Disabled Actions and Reasons` pattern); a missing or failed policy read
renders EVERY external-source entry disabled with the fail-closed reason
`Sources externes indisponibles pour l'instant.` — never
active-by-default. The title path and the structured-folder entry are
NEVER policy-gated: they are local flows, not "additional content
sources", and a policy read failure must never block them.

## Story Editor Shell Contract

The `/story/:storyId/edit` route renders the `Story Editor Shell` — the
dedicated screen, separate from the library, where a parent resumes a story
without losing the global context. It is the same route the library opens on
double-click / `Éditer` (see `Story Card Interaction Contract`) and leaves
through `Retour à la bibliothèque` (see `Library Routing Contract`, which
already preserves selection and filters across the round trip). The shell
opens for a native story and for an imported one — both are canonical
`stories` rows read by `get_story_detail`. What may be edited follows the
story's DECLARED EDIT SCOPE, derived from its import provenance (see
`Imported Story Edit Scope Contract`): a native story and a `.rustory` import
carry the `full` scope (complete editor); a device-pack story carries
`titleOnly` (named pack states replace the content zones), so the shell never
shows an editable control that cannot be saved.

This contract owns the SHELL: the three-zone frame, its entry/exit, and its
keyboard model. The behaviors hosted inside it keep their own contracts — the
title field (`Story Autosave Contract`), the current-node editor
(`Story Node Editor Contract`), export (`Story Export Contract`), and draft
recovery (`Story Recovery Contract`). Editing the **content** of the current
node (its text, metadata, image and audio) lives in the shell through the
`Story Node Editor Contract`; reorganizing the **structure** (adding, moving,
deleting nodes) and the **option links** between nodes lives there too, through
the `Story Structure Editing Contract` and the `Option Link Editor Contract`.

| Aspect | Value |
| --- | --- |
| Three coexisting zones | The shell shows, at once: the global structure (`Story Structure Navigator`), the current node (host zone), and the story state + actions. None is hidden behind a tab or a click — all three are visible together so the global context is never lost while editing. |
| Content states | The canonical model carries an ordered node graph (one or more nodes, a designated start node, option links). Each zone renders honest, NAMED states (`Structure de l'histoire` shows the story root + the ordered node list; `Nœud courant` shows the editor for the selected node, with named empty states for an empty field or an absent optional media), never blank, never disguised, never a fake node. A new story (or a migrated one) starts with a single empty start node — that is a valid starting state, not an error. |
| Structure projection (from Rust) | The navigator consumes the node graph PROJECTED by Rust (`detail.structure`, see `Story Structure Editing Contract`): the start node id plus the ordered list of nodes, each with its stable id, label, option links and a localized issue flag. It NEVER re-serializes or reformats `structureJson` (covered byte-for-byte by `content_checksum`); every mutation goes through a Rust command. When Rust cannot project the graph (a corrupt / drifted structure — near-impossible since Rust is authoritative and checksum-guarded) the zone falls back to a NAMED degraded state (`Structure illisible`), never a crash, never a fake node. |
| Current-node zone | The `Story Node Editor` (see `Story Node Editor Contract`): the labelled text and metadata fields plus the image and audio media slots of the current node. For a `full`-scope story (native or `.rustory` import) the fields and media actions are active; for a device-pack story (`titleOnly`) the zone renders the named pack state INSTEAD of the controls (see `Imported Story Edit Scope Contract`). |
| Story state + actions | The persisted title (`<h1>`, mirrors `detail.title`), the editable title field + autosave chip (`Story Autosave Contract`), the draft recovery banner when present (`Story Recovery Contract`, which keeps priority over the field), and the global actions `Retour à la bibliothèque` (+ `Exporter l'histoire`). |
| Global actions scope | `Retour` and `Exporter` only. Sending a story to a device stays ANCHORED IN THE LIBRARY — it is never an editor action. No node-level or preflight validation lives in the shell (preflight is a transfer flow). |
| Keyboard & focus | Stable focus order, EXPLICITLY: the story state zone OPENS the tab order (its editable title field is the editing entry point, and the recovery banner keeps priority above it), then structure → current node → terminal global actions (`Exporter`, `Retour`). The CONTENT zones and the terminal actions therefore follow structure → current node → global actions; the state/title block sitting first is deliberate (it has opened the shell since its first iteration and anchors the recovery decision before any content edit). Focus is visible on every zone (the global `:focus-visible` ring). Meaning is never carried by color alone (glyph + text). A problem is never carried in a toast alone. |
| Context separation | Editing is a SEPARATE dominant context (its own route): the shell never renders the library collection / filters. Returning restores the library with its useful card traces intact (preparation / transfer badges, import provenance) — and never silently erases the selection or filters when they stay coherent. |
| Read rule & performance | The shell consumes the single authoritative `get_story_detail` read already owned by `useStoryEditor` (title + `structureJson` + timestamps) — no extra read, no device call, fully offline. Re-open stays well within NFR2 (one small read + a tiny JSON parse). |
| No regression | Title autosave, draft recovery, and export keep their exact behavior; `flushAutoSave` still fires on `Retour` and on unmount so a keystroke typed mid-debounce is never lost. |

## Story Autosave Contract

A persisted story is editable from the `/story/:storyId/edit` route, inside
the `Story Editor Shell` (see `Story Editor Shell Contract`). The **title** is
autosaved by this contract; the **content of the current node** (text,
metadata, image and audio) is autosaved by the `Story Node Editor Contract`,
which reuses this exact engine (the `500 ms` debounce, the `150 ms` recovery
buffer, `flushAutoSave`, the call-correlation guards, and the overview
invalidation). Reorganizing the structure and the option links is NOT an
autosave concern: structural mutations are explicit, acknowledged actions owned
by the `Story Structure Editing Contract` and the `Option Link Editor
Contract`, and the pending content is flushed before any of them runs.

| Aspect | Value |
| --- | --- |
| Read rule | `useStoryEditor` calls `get_story_detail` on mount and on every `storyId` change. The overview cache never substitutes for this authoritative read. A `null` return maps to the `Histoire introuvable` surface; a rejection maps to `Reprise indisponible`. |
| Write rule | Each `setDraftTitle` plans a single autosave `500 ms` after the last keystroke. The debounce cancels on every new keystroke so only the latest value survives. The save fires `update_story({ id, title })` in a `BEGIN IMMEDIATE` SQLite transaction. The **title** path never modifies `structure_json` or `content_checksum`. Node **content** (text / metadata) takes a SEPARATE path — `update_node_content`, owned by the `Story Node Editor Contract` — which DOES re-serialize `structure_json` and recompute `content_checksum` in Rust, behind the `schema_version` v2 model. The two write paths never share a command: the title-only `update_story` is forbidden from touching the canonical body. |
| No-op rule | Typing a value that normalizes (NFC + trim) back to the persisted title cancels any pending save, clears any stale failure alert, and settles the chip back to `Brouillon local`. |
| Authoritative validation | Rust re-validates on every `update_story` call with the same rules as `create_story`; an invalid title is refused via `AppError { code: "INVALID_STORY_TITLE" }` and leaves the row untouched. |
| State chip mapping | `idle → Brouillon local (info)`, `pending → Modifications en attente (neutral)`, `saving → Enregistrement… (neutral)`, `saved → Enregistré (success)`, `failed → Enregistrement en échec (error)`. The `saved` state is announced via an `aria-live="polite"` region; transient states stay silent to avoid AT chatter. |
| Failure recovery (AC3) | On rejection, `detail.title` keeps the previous value — never "Enregistré". The typed draft is preserved in the Field. A `role="alert"` region renders the Rust-supplied `message` + `userAction`, with a `Réessayer l'enregistrement` button that re-fires the save using the attempted title. The library overview cache is NOT invalidated on failure — the UPDATE is atomic (NFR9), so the persisted state is unchanged and the overview already reflects the truth. |
| Library coherence | After every successful save, `invalidateLibraryOverviewCache()` drops the module-local SWR snapshot so the next mount of `LibraryRoute` observes the updated title. The invalidation runs BEFORE the `mountedRef` guard so an ACK that arrives after the route unmounted (navigate-away race) still refreshes the overview. No explicit `retry()` is chained — the fetch happens naturally at next mount. |
| Stale success rule | If the save ACK arrives for a title the user has already typed past (e.g. "A" was in flight while the user typed "B"), the route commits the ACK'd value to `detail` (it IS persisted) but re-plans a debounced save for the newer draft. The chip transitions `saving → pending → saving → saved` without ever falsely painting "Enregistré" over a value the field has moved past. |
| H1 source | The visible `<h1>` mirrors `detail.title` (the last-committed title), not `draftTitle` — the H1 must not re-announce at every keystroke nor misrepresent what is actually saved. The editable Field carries the live draft. |
| Missing id rule | `update_story` on an id no longer in `stories` returns `AppError { code: "LIBRARY_INCONSISTENT" }` with `details.source = "story_missing"`. The UI surfaces it through the same `role="alert"` flow as any other save failure. |
| Flush rule | Clicking `Retour à la bibliothèque` (or any route-level unmount) calls `flushAutoSave()`: a pending debounce is cancelled and the save is fired before the synchronous `navigate(...)`. The IPC call itself still resolves asynchronously — the UI will have unmounted before the Rust response arrives, so the save is "fire and commit in Rust, forget in UI". A success is observed on the next `/library` read; a failure after unmount is not re-surfaced to the user (the route is gone) and the prior persisted state remains the source of truth. Durable recovery of a draft that never flushed (crash, kill -9) is a separate feature and not covered by this contract. |
| Persisted-vs-draft invariant | `detail.title` is the source of truth, refreshed from Rust on mount. `draftTitle` is the live Field value. They diverge only while `saveStatus.kind === "failed"`; any other transition reconciles them. |

## Story Node Editor Contract

The `Story Node Editor` fills the current-node zone of the `Story Editor Shell`
(see `Story Editor Shell Contract`). It edits the **content of the current
node** — the node selected in the `Story Structure Navigator`: its narrative
**text**, its **metadata** (a human-readable label), and two optional
**media** — an **image** and an **audio**. It does NOT add, move, delete or
relink nodes: structural mutations are owned by the `Story Structure Editing
Contract`, and the node's option links by the `Option Link Editor Contract`
(hosted below the content, in this same zone). This contract edits the selected
node in place.

**Projection from Rust (AC3).** The node is PROJECTED by the Rust core inside
`get_story_detail` as `detail.node` (a `NodeContentDto`) — the SELECTED node,
the start node by default (see `Story Structure Editing Contract` for the
selection rule), never recomposed in
React from `structureJson`. The projection carries the node's stable `id`, its
`text`, its `label`, and a resolved state for each media slot (see below). The
frontend consumes the projection; it never parses or rewrites `structureJson`
(those bytes stay opaque and are covered by `content_checksum`). The stable
`id` is what keeps the current node clearly identified across a long edit
session — no identity drift after N edits. When Rust cannot project a node
(corrupt / drifted structure) `detail.node` is `null` and the zone renders the
named degraded state (`Structure illisible`), never a fabricated node.

**Editability.** The node editor follows the story's declared edit scope (see
`Imported Story Edit Scope Contract`): a `full`-scope story (native or
`.rustory` import) edits its node the exact same way; a device-pack story
(`titleOnly`) renders the named pack state INSTEAD of the fields — the editor
NEVER shows a text field or a media action that cannot be saved.

| Aspect | Value |
| --- | --- |
| Supported fields | The narrative **text** (a multi-line field) and the node **metadata label** (a single-line field). Both may be empty — an empty node is a valid starting state, not an error. |
| Text / metadata autosave | Reuses the `Story Autosave Contract` engine verbatim: a `500 ms` debounce after the last keystroke, the `150 ms` recovery buffer, `flushAutoSave` on `Retour` / unmount, the call-correlation guards, and the overview invalidation on a terminal save. The save fires `update_node_content({ storyId, nodeId, text, label })`, which re-validates the canonical structure, re-serializes `structure_json` and recomputes `content_checksum` in a single `BEGIN IMMEDIATE` transaction. It NEVER reuses the title-only `update_story`. |
| Media machine | Each media slot (image, audio) is an explicit action set — `Ajouter` / `Aperçu` / `Remplacer` / `Retirer` — persisted IMMEDIATELY on the action (not debounced like text). `Ajouter` / `Remplacer` open a native file picker (non-blocking), validate the chosen file in Rust, store its bytes under Rust infrastructure, and write the node's asset reference. `Retirer` clears the reference. `Aperçu` reads the stored bytes back through a Rust command for presentation only — the frontend never owns the media bytes. |
| Acknowledgement < 1 s (NFR3) | Adding or replacing a media produces a VISIBLE acknowledgement (a preview / a named state) in under one second; any heavier work (hashing a large file) continues in the background without freezing the UI (NFR5). |
| Supported source formats | Images **PNG / JPEG**; audio **MP3 / WAV / OGG**. Recognized by magic bytes (signature), never by file extension. The set is declared in [device-support-profile.md](./device-support-profile.md). No transcoding happens here — converting to a device pack stays a transfer/preparation concern. |
| Validation — attention vs blocking (AC2) | Rust is the authority (the UI validates only for ergonomics). The node editor is the FIRST living emitter of the `media` axis of the validation taxonomy (see `Story Validation / Preflight Contract`). Two distinct levels: **`needs attention`** (`à corriger`, repairable — e.g. a referenced media whose source file is no longer readable; does NOT block autosaving the rest of the node) and **`blocked`** (`bloquée`, a real block — e.g. an unsupported / unreadable / oversize file at attach time; that media slot is NOT saved until corrected). Each level names the cause, the impact, and the next gesture. |
| Localized errors | An error lives INLINE next to the field / media slot it concerns, in a `role="alert"` surface — NEVER a lone toast, NEVER color alone (glyph + text). A media block lives at its slot; a text validation issue lives at the text field. A partial state is never disguised as a success. |
| Error codes | A media attach / read failure rejects with `MEDIA_INVALID` (unsupported format, unreadable, oversize — a real block surfaced at the slot) or `MEDIA_PROCESSING_FAILED` (a transport failure of the media store: staging / promotion / read). Both carry a closed `details.source` and are PII-safe (no absolute path, no raw OS message). |
| State chip mapping | The node editor reuses the autosave chip for text / metadata (`idle → Brouillon local`, `pending`, `saving`, `saved → Enregistré`, `failed`). A media slot carries its own named state: `Aucune image` / `Aucun audio` (empty), a preview/identity when present, `Média à corriger` (attention), `Média bloqué` (blocked). |
| Recovery (NFR8) | The `150 ms` recovery buffer extends to the node's in-progress text so a hard kill (kill -9) mid-edit does not lose the typed value, mirroring the title recovery. The buffered node text is offered back on the next open through the `Story Recovery Contract` surface. |
| Atomicity (NFR9) | The editor never paints `Enregistré` over an unsaved change: the same source-of-truth reconciliation as the title autosave applies. A failed node save leaves the previous canonical body untouched (the `BEGIN IMMEDIATE` transaction commits or rolls back whole). The canonical body of ANY OTHER story is never touched. |
| Keyboard & focus | Stable tab order: structure → node fields / media slots → terminal global actions (the state/title block opens the shell's overall order — see the `Story Editor Shell Contract`). A disabled media action carries a standardized reason (see `Disabled Actions and Reasons`). Focus is visible everywhere (the global `:focus-visible` ring). |

## Story Structure Editing Contract

The `Story Structure Navigator` (the `Structure de l'histoire` zone of the
`Story Editor Shell`) renders the story's node graph and owns the STRUCTURAL
mutations: adding, reordering and deleting nodes, and selecting the current
node. It stays an ordered hierarchical LIST — the story root followed by the
nodes in their canonical order — never a free-form 2D canvas. Branches remain
readable through each node's option links (see `Option Link Editor Contract`);
the graph is never manipulated spatially.

**Projection (from Rust).** The navigator consumes `detail.structure` (a
`StoryStructureDto`): the start node id plus the ordered node list, each node
carrying its stable `id`, its `label`, an `isStart` flag, a localized
`hasIssue` flag, and its option links with a Rust-derived state. The frontend
NEVER parses, re-serializes or reformats `structureJson` (covered
byte-for-byte by `content_checksum`) and never re-derives a link state. When a
BLOCKING canonical issue exists (unsupported schema, corrupt structure,
checksum mismatch, duplicate node id, invalid start node), Rust projects
`structure = null` and the zone renders the named degraded state
(`Structure illisible`) — never a crash, never a fabricated graph. A FIXABLE
issue (an option whose destination no longer exists) does NOT unmount the
graph: the structure stays projected and editable so the user can SEE and fix
the flagged spot.

| Aspect | Value |
| --- | --- |
| Rendering | Story root (the persisted title) + the ordered node list (order = canonical `nodes[]` order). Each entry shows the node label — or its stable id when the label is empty (a NAMED state, never a blank row) — the textual `Départ` mark on the start node, a short summary of its options, and a glyph + text `à corriger` mark when `hasIssue` is true. A localized issue NEVER hides or collapses the rest of the list. |
| Selection | Click / `Entrée` selects a node as the current node. The selection is LOCAL UI state (component/route state, never the Zustand shell store). Selecting re-reads `get_story_detail(storyId, nodeId)` (the authoritative read) so the `Nœud courant` zone re-seeds from Rust. The pending node content is flushed BEFORE the selection changes — a keystroke typed mid-debounce is never lost. `aria-current="true"` marks the selected entry. |
| Selection fallback | Deleting the currently selected node moves the selection back to the START node (structure and current node stay coherent). A stale selection (a node id no longer in a healthy graph) also falls back to the start node — never a blank editor over a healthy structure. |
| Keyboard | Roving tabindex over the node list: `↑` / `↓` move focus between nodes, `Entrée` selects the focused node. Inter-zone focus order is UNCHANGED (see the `Story Editor Shell Contract` for the explicit full order — the state/title block opens the tab order, then structure → current node → terminal global actions). Focus is visible everywhere (the global `:focus-visible` ring). |
| Structural actions | Per-node, discreet (quiet/tertiary) actions, rendered ONLY for a `full`-scope story (native or `.rustory` import — see `Imported Story Edit Scope Contract`): `Ajouter un nœud` (appends an empty node at the end of the list), `Monter` / `Descendre` (swap the node with its neighbor — display order only; the start node is designated by `startNodeId`, NOT by position, so moving it is allowed), `Supprimer le nœud`. A device-pack story (`titleOnly`) mounts NO navigator at all — the named pack state replaces the zone (never a control that cannot be saved, and never the misleading placeholder graph). |
| Delete confirmation | `Supprimer le nœud` is destructive (the node's text and media are lost). It requires TWO explicit gestures, INLINE and localized inside the node's entry: the first gesture swaps the action row for a confirmation block naming the impact (the node and its media are removed; options pointing at it will be flagged `destination à corriger`), with `Confirmer la suppression` + `Annuler`. Never a modal, never a single-gesture delete. Deleting the START node is refused by Rust (the entry point must exist); the UI does not offer it. |
| Mutation rule | Every structural mutation is an EXPLICIT, ACKNOWLEDGED action (never debounced, never optimistic): it calls a dedicated Rust command, which re-validates the persisted facts, applies the mutation to the whole graph, re-serializes the canonical structure and recomputes `content_checksum` in ONE `BEGIN IMMEDIATE` transaction. The UI reconciles from the ACK's re-projected graph — never from a locally recomposed state. The pending node content is flushed BEFORE any structural mutation. |
| Deletion side effects | Deleting a node removes its media assets (with the reference-counted file GC) and its recovery buffer entry for THAT node only. Options on OTHER nodes that pointed at the deleted node KEEP their destination value and become `destination à corriger` — visible, localized, repairable — never silently unlinked (the trace survives). |
| States | The degraded `Structure illisible` state (blocking issue) and the honest empty states are preserved. A story with a single start node is a valid minimal graph, not an error. |
| Errors | A refused mutation (deleting the start node, an unknown node id, a stale index) surfaces INLINE near the acted-on entry in a `role="alert"` region with the canonical `message` + `userAction` — never a lone toast, never color alone. Acknowledgements stay quiet (`aria-live="polite"` at most). |
| No canvas | The zone never becomes a free-form canvas: no drag-and-drop graph, no spatial layout, no zoom surface. The hierarchy stays visible in place, permanently, whatever the graph size. |

## Option Link Editor Contract

The `Option Link Editor` lives INSIDE the current-node zone (below the node's
content fields, hosted by the `Story Node Editor` zone) and edits the CHOICES
of the selected node: the list of its options and the destination each option
points at. It is the only surface that creates or repairs links between nodes.

**Link states (truth table, Rust-derived).** Each option carries a wire
`state` DERIVED BY RUST from its persisted `target` — the frontend never
re-derives it:

| Persisted `target` | Wire `state` | UI label (product language) |
| --- | --- | --- |
| absent (`null`) | `unlinked` | `non liée` — a normal authoring state, not an error |
| a node id present in the graph | `linked` | `liée` + the destination's label |
| a node id ABSENT from the graph | `broken` | `destination à corriger` — repairable, flagged in place |

Forbidden ambiguities: an option with a missing destination is NEVER rendered
as linked; a partially valid state is NEVER rendered as a success; the words
`broken` / `lien cassé` NEVER appear on screen (`destination à corriger` is
the only user-facing wording); an `unlinked` option is never conflated with a
`broken` one (not linked yet ≠ points at a ghost).

| Aspect | Value |
| --- | --- |
| Prevent vs flag (write vs read) | At WRITE time Rustory PREVENTS creating an invalid link: linking an option to a node id that does not exist in the graph is refused by Rust (typed error, inline alert). At READ time an already-broken link (its destination was deleted later) is FLAGGED `destination à corriger` — persisted, visible, repairable — never silently dropped. Self-reference (an option pointing back at its own node) is a legitimate narrative loop and is allowed. |
| Actions | Per option: `Lier` (opens a FLAT selector listing the graph's nodes by label/id — never a canvas), `Créer et lier un nouveau nœud` (creates an empty node AND links the option to it in ONE atomic Rust transaction — no intermediate half-state), `Délier` (back to `non liée`), `Retirer l'option` (removes the option). Per node: `Ajouter une option` (with its label typed at creation). Labels are bounded by the same cap as the node metadata label. |
| Acknowledged mutations | Same mutation rule as the structure contract: explicit acknowledged Rust commands, one transaction each, UI reconciled from the re-projected graph, pending content flushed first. Never debounced, never optimistic. |
| Out-of-scope rule | For a device-pack story (`titleOnly` scope) the Option Link Editor is NOT MOUNTED at all — the named pack state replaces the content zones (see `Imported Story Edit Scope Contract`); its defensive non-editable projection is kept as defense in depth only, never as a reachable screen. A `.rustory` import carries the `full` scope and edits its links exactly like a native story. |
| Errors | A refused link (unknown destination, stale option index) surfaces INLINE at the option row in a `role="alert"` region naming the cause, the impact and the next gesture. Acknowledgements are `aria-live="polite"`. Never a toast alone, never color alone (glyph + text). |
| A11y | The option list and the flat node selector are keyboard-reachable in the normal tab order of the current-node zone; every action is a real button with an accessible name naming its option (e.g. `Lier — {option label}`). |

## Story Export Contract

A persisted story can be exported to a user-chosen file from the
`/story/:storyId/edit` route. MVP Phase 1 exports exactly one story at a
time; batch export from the library is Post-MVP.

| Aspect | Value |
| --- | --- |
| Trigger rule | A `Exporter l'histoire` button lives in the route's action row next to `Retour à la bibliothèque`. The library context menu / batch action flow is Post-MVP; single-story from the edit surface is the only entry point in Phase 1. |
| Dialog rule | The frontend opens a native save dialog via `@tauri-apps/plugin-dialog::save` with `defaultPath = "{sanitizedTitle}.rustory"` and `filters = [{ name: "Artefact Rustory", extensions: ["rustory"] }]`. Cancel returns `null` and is a silent no-op — never an error surface, never an alert. |
| Atomic write rule | Rust stages the artifact through a `NamedTempFile` co-located with the destination, `write_all` + `flush` + `sync_all` the bytes, then `persist()` performs an atomic `rename(2)`. Any intermediate failure drops the `NamedTempFile`, leaving zero residual `.tmp*` files in the target directory. |
| Invariant canonical rule | `save_story_export` is strictly read-only on the `stories` table — `title`, `structure_json`, `content_checksum`, `created_at`, `updated_at` are byte-for-byte invariant across an export. Integration tests assert this row-equality before and after the call. |
| Cache rule | A successful export does NOT call `invalidateLibraryOverviewCache()`. A failed export does NOT call it either. Export never mutates canonical state, so the overview already reflects the truth. |
| State chip mapping | `idle → no chip`, `exporting → Exportation en cours… (neutral)`, `exported → Exporté (success)` inside an `aria-live="polite"` region that auto-hides after `3 s`, `failed → role="alert"` with the canonical `message` + `userAction` plus a `Choisir un autre emplacement` button (relaunches the dialog) and a `Fermer` button (dismiss to idle). The `exporting` transition is NOT announced to AT to avoid noise. |
| Error taxonomy rule | Rust may reject with `EXPORT_DESTINATION_UNAVAILABLE` (`details.source ∈ { invalid_path, parent_missing, temp_create, write_temp, rename, dialog_failed }` and `details.kind ∈ { permission_denied, no_space, io, read_only_filesystem, not_found, invalid_input, other }`, with `details.cause` naming the specific boundary-level reason when `source = "invalid_path"` — e.g. `not_absolute`, `too_long`, `empty`, `trailing_whitespace`, `empty_file_stem`, `symlink_destination`, `internal_app_directory`, `non_filesystem_path`), `LIBRARY_INCONSISTENT` with `details.source = "story_missing"` when the story was deleted between load and export, or `LOCAL_STORAGE_UNAVAILABLE` with `details.source ∈ { sqlite_select, artifact_serialization }` (the latter guards an unreachable serializer failure; an occurrence is a bug, not a user-recoverable state). User-facing messages are picked from a closed canonical table — the raw OS message is never forwarded (PII). |
| Artifact format rule | UTF-8 JSON, pretty-printed with a trailing `\n`, envelope `{ "rustoryArtifact": { "formatVersion": 1, "exportedAt", "exportedBy" }, "story": { "schemaVersion", "title", "structureJson", "contentChecksum", "createdAt", "updatedAt" } }`. `contentChecksum` is recopied byte-for-byte from the row — never recomputed. Forward-compatibility: a future importer MUST refuse `formatVersion != 1` until a new variant is introduced. |
| Default filename rule | `sanitizeFilename(persistedTitle) + ".rustory"`. Sanitization applies NFC, trims whitespace, replaces filesystem-unsafe characters (`\x00-\x1f`, `\x7f`, `/ \\ : * ? " < > |`) with `_`, collapses runs of whitespace/underscore, truncates at 80 code points, and falls back to `histoire` when the result is empty. |
| H1 rule | The visible `<h1>` continues to mirror `detail.title` — export does NOT change the title, so the heading is unaffected regardless of export outcome. |
| Accessibility | The success region is `aria-live="polite"` with `aria-atomic="true"`. The failure region is `role="alert"`. The "Choisir un autre emplacement" button is reachable via keyboard and appears before `Fermer` in tab order so a keyboard user can retry with one keystroke after reading the alert. |

## Local Artifact Import Contract

Importing a supported local artifact (`Importer une histoire`) is the **inverse
of the export flow**: it brings a `.rustory` file from the computer into the
local library as a canonical, re-openable story. It is **distinct from the
`Device Story Import Contract`** (`Copier dans ma bibliothèque`, device → library)
— that flow either fully succeeds or explicitly fails, while this one produces a
typed recognition verdict that may be `Partiellement exploitable`. The supported
artifact set for THIS flow is **the `.rustory` v1 artifact only** (see
[device-support-profile.md#Local Artifact Import Contract](./device-support-profile.md)).
The **structured folder** is supported by its own CREATION flow (see
`Structured Folder Creation Contract` below — a different door into the same
canonical model, never an implicit extension of this one); structured archives
(zip…) remain out of scope (no archive reader — zero-dependency rule).

The flow is **two-phase, with no mutation before acceptance (AC1)**:

| Phase | Command | Effect |
| --- | --- | --- |
| Analyze | `analyze_artifact_for_import` | Opens a native file picker (`.rustory`), reads the chosen file bounded, parses + validates every aspect, returns a typed recognition verdict DTO. NO row written, no file promoted. A cancelled dialog returns `{ kind: "cancelled" }` (never an error). A read/transport failure rejects with `IMPORT_FAILED`. |
| Accept | `accept_artifact_import` | On the explicit `Importer ce qui est reconnu` action: re-validates the canonical content from zero (never trusts the frontend), then commits one `stories` row + one `story_local_imports` row in a single `BEGIN IMMEDIATE` transaction. Returns the created `StoryCardDto`. |
| Abandon | — (pure frontend) | `Abandonner` drops the verdict. Nothing was mutated, so no command is needed. |

Recognition model (a typed verdict, NEVER an `AppError` — a partially usable or
functionally blocked artifact is a result state, not a transport error):

- **Recognition quality** (global): `Clean` → `Propre`, `Partial` →
  `Partiellement exploitable`, `Unusable` → `Inexploitable`.
- **Recognition aspect** (per finding): `Envelope`, `FormatVersion`,
  `SchemaVersion`, `Structure`, `Integrity`, `Title`, `Timestamps`.
- **Recognition category** (per finding): `Recognized` → `reconnu`, `Ambiguous`
  → `ambiguïté`, `Missing` → `information manquante`, `Blocking` → `blocage
  réel`. `Missing` is **never emitted by the `.rustory` flow** — a negative
  test locks this (mirrors `Axis::Media` / `Axis::Filesystem` in the preflight
  contract); its first real emitter is the structured-folder creation flow
  (see `Structured Folder Creation Contract` — a referenced media absent from
  the folder). The UX `dupliqué` stays a `Blocking` finding of the `Structure`
  aspect (a duplicate node id), never a category of its own.
- **Import state** (per story, durable, surfaced as a Story Card chip):
  `recognized` / `partial` / `needs_review` / `blocked` / `resolved`. `resolved`
  is emitted by the write-path review resolution ONLY (see `Import Review
  Resolution Contract`) and renders NO chip; `blocked` is never persisted.

Recognition truth table for a `.rustory` artifact:

| Case | Quality | Import state | Result |
| --- | --- | --- | --- |
| Envelope valid, `formatVersion == 1`, canonical structure valid, recomputed checksum == declared, title normalizable and non-empty, canonical timestamps | `Propre` | `recognized` | Importable → canonical story, **no marker** (AC3) |
| Same, but the title had to be normalized (`original != normalize_title(original)`) OR a carried timestamp is not the expected ISO-8601 UTC ms shape | `Partiellement exploitable` | `needs_review` | Importable **with** a durable `Import Issue Marker` + on-demand report (AC2) |
| Malformed JSON / unknown field / `formatVersion != 1` / non-canonical structure / **checksum divergent** / title empty after normalization | `Inexploitable` | `blocked` | **Not importable** → clear error + abandon, **no mutation** (AC1) |

The only real `partial` trigger for a Rustory-exported `.rustory` is a
**hand-edited** title (Rustory always exports a normalized title and canonical
timestamps, and the title never enters the `content_checksum` digest — adjusting
it never diverges the checksum). Timestamps are **preserved** on the imported
row (fidelity of the AC3 "re-openable" story) — never silently rewritten to
`now`; a malformed carried timestamp is preserved AND flagged `needs_review`.
`story_local_imports.imported_at = now`.

UI state machine (owned by `useStoryImport`, surfaced in the library):

| State | Rendering | Announcement |
| --- | --- | --- |
| `idle` | no status content (the polite region stays mounted, empty) | none |
| `analyzing` | indeterminate `ProgressIndicator` labelled `Analyse de l'artefact…` (calm, neutral) | deliberately NOT announced |
| `review` | the recognition report in-context: the quality chip (`Propre` / `Partiellement exploitable` / `Inexploitable`), the per-aspect findings, and — when importable — `Importer ce qui est reconnu` THEN `Abandonner` in tab order. A `blocked` verdict is `role="alert"` (no accept button, only `Abandonner`); an importable verdict is `aria-live="polite"` | `role="alert"` if blocked, else `aria-live="polite"` |
| `importing` | indeterminate `ProgressIndicator` labelled `Import en cours…` | deliberately NOT announced |
| `imported` | `Histoire importée dans ta bibliothèque` (success chip) + the created local title + explicit `Fermer` dismiss; no auto-hide | `aria-live="polite"`, mounted, `aria-atomic` |
| `failed` | `Import impossible` block with the canonical `message` + `userAction`, buttons `Réessayer` THEN `Fermer` in tab order | `role="alert"` |

`Import Issue Marker` (durable, AC2): a story imported as `partial` /
`needs_review` carries a discreet chip on its library card (`partiel` / `à
revoir`), derived from `story_local_imports.import_state` exposed by
`read_stories` / `StoryCardDto` — so it **survives an app restart**. It is
distinct from (and must coexist with) the transfer/preparation `StoryPreparationBadge`,
whose `partial` means a verification verdict: this marker uses its own dedicated
labels/tone/glyph and never reuses the transfer `partial` value or sense. A
`resolved` story carries NO chip and NO report on its card — provenance
`Importée` only (see `Import Review Resolution Contract`).

`Import Review Flow` (on-demand, AC2): clicking the marker opens a simple
in-context report — the global outcome (`Ce que Rustory a reconnu`) + the
recognized aspects + the `Points d'attention`. The single source of truth is
`story_local_imports.findings_summary`. Never a toast / modal to carry a problem
alone. The review is SETTLED by editing — a real write that leaves the
canonical story fully sound flips the durable state to `resolved` (see
`Import Review Resolution Contract`); a GUIDED repair flow remains deferred
and is never imposed.

Invariants (locked by tests):

- **No mutation before acceptance (AC1)**: until `accept_artifact_import` runs,
  no `stories` / `story_local_imports` row exists. `Abandonner` and the
  `Unusable` / `blocked` case return to an unchanged library.
- **Canonical data never mutated by another import (FR18)**: an import creates a
  NEW story (fresh UUIDv7); it never touches an existing row.
- **No false success / honesty**: `recognized` is shown only when every aspect
  passes; a `partial` is named as such, never disguised as success.
- **Atomicity**: the commit is one transaction; any failure rolls back fully
  (never a half-imported story).
- Offline, zero dependency, zero network; color is never the sole carrier of
  meaning; a problem is never carried in a toast alone.

Error taxonomy — analyze/accept reject only on TRANSPORT failure, as `AppError {
code: "IMPORT_FAILED" }` with a stable `details.source` from this closed set
(the functional verdict is the typed DTO, never an error):

- `file_read` — reading the chosen file failed (unreadable, oversize beyond `MAX_ARTIFACT_BYTES`, a non-regular file, a non-filesystem path). `details.stage` ∈ `metadata`, `open`, `not_regular_file`, `oversize`, `read`, `non_filesystem_path`.
- `db_commit` — the final SQLite transaction failed; nothing is committed (atomic rollback). `details.stage` ∈ `begin_transaction`, `insert_story`, `insert_provenance`, `commit`; `details.kind` ∈ `busy`, `locked`, `constraint_violation`, `other`.
- `spawn_blocking_join` — the worker task could not be joined.
- `app_data_unavailable` — the managed local store has no resolvable home.
- `dialog_failed` — the native file dialog backend could not open.
- `other` — fallback for unmapped causes; `details.cause` names the specific reason (`revalidation`, `invalid_provenance` with `details.field`, `system_clock_invalid`).

Every refusal constructor carries a non-empty `message` (cause + impact) AND a
non-empty `userAction` (next gesture); the frontend renders both verbatim and
branches on `code` + `details.source` only to choose the surface, never to
compose the text.

## Structured Folder Creation Contract

Creating a story from a **structured folder** (`Créer une histoire` → `Choisir
un dossier…`) is a CREATION flow, not an import bis: the folder carries an
**author manifest** (`histoire.json` + referenced media, see
[device-support-profile.md#Structured folder v1 format contract](./device-support-profile.md))
and the accepted story is BORN like an interactive creation (fresh UUIDv7,
`created_at = updated_at = now`). It reuses the import machinery — two phases,
typed verdict, durable marker — but owns its aspect set, its state derivation
and its copy.

The flow is **two-phase, with no mutation before acceptance (AC4)**:

| Phase | Command | Effect |
| --- | --- | --- |
| Analyze | `analyze_structured_folder_for_creation` | Opens a native FOLDER picker, reads `histoire.json` bounded, probes each referenced media (`symlink_metadata`, bounded read, magic-byte sniff — never the extension), returns a typed verdict DTO. NO row written, NO file promoted, the folder is never listed. A cancelled dialog returns `{ kind: "cancelled" }`. A read/transport failure rejects with `IMPORT_FAILED`. |
| Accept | `accept_structured_creation` | On the explicit `Créer l'histoire` action: RE-ANALYZES the folder from zero (the disk may have changed — the re-analysis is authoritative; a verdict turned blocking refuses), promotes the retained media into the node-media store OUTSIDE the DB lock, then commits `stories` + `story_local_imports` (`source_format = 'structured-folder'`) + `assets` rows in ONE `BEGIN IMMEDIATE` transaction. A transaction failure compensates the promoted files (GC). Returns the created `StoryCardDto`. |
| Abandon | — (pure frontend) | `Abandonner` drops the verdict. Nothing was mutated, so no command is needed. |

The analysis DTO carries the absolute `folderPath` returned by the system
dialog, ONLY so the accept phase can re-read the same folder. It is NEVER
rendered, NEVER persisted (provenance stores the validated basename only),
NEVER logged and never part of an error payload (PII). The accept phase grants
it no authority: everything is re-derived from the disk; a forged path is
equivalent to "the user picked this folder" and opens no new capability.

Recognition model (typed verdict, NEVER an `AppError`): the folder flow
analyzes `Envelope`, `FormatVersion`, `Title`, `Structure` and `Media` — its
OWN aspect set (no `SchemaVersion` / `Integrity` / `Timestamps`: an author
manifest has no declared schema, no checksum, no timestamps). The `Media`
aspect is analyzed ONLY when the declared `formatVersion` is the listed one:
an unlisted format never triggers a single media read (AC2 — no implicit /
partial support) and its verdict carries no `Media` finding. The full
aspect × category matrix, the named bounds and the manifest schema live in the
support profile contract. State derivation (folder flow):

| Findings | Quality | Durable state |
| --- | --- | --- |
| any `Blocking` | `Inexploitable` | `blocked` — nothing created, never persisted |
| else any `Missing` (a referenced media absent) | `Partiellement exploitable` | `partial` — the FIRST real emitter of this state |
| else any `Ambiguous` | `Partiellement exploitable` | `needs_review` |
| none of the above | `Propre` | `recognized` (no report, no marker) |

A discarded media (`Missing` / `Ambiguous`) never blocks the creation: the node
is born with the empty slot, visible in the editor (node-media controls) — that IS the
"à corriger" of AC1. The persisted findings stay aggregated `(aspect, category)`
pairs (per-file detail is deferred); the report names the groups, the editor
shows the empty slots.

Per-pair FR copy: the folder flow owns the FR message of every pair of its
matrix (fixed in `product-language.md#Change Control`). Every `Envelope`,
`FormatVersion`, `Title` and `Structure` pair carries a FOLDER wording (the
folder speaks of a manifest and a creation, not of an artifact and an import)
— except the shared `Title × reconnu` line, whose copy is identical to the
`.rustory` one. The folder wording renders everywhere the folder flow appears,
INCLUDING the durable card report (the projection branches on the provenance's
`source_format`); the `Media` pairs exist only in this flow. Every BLOCKING
copy of the matrix names the corrective gesture (fix the folder/manifest, then
re-run the analysis) — the blocked surface itself only offers `Abandonner`.

UI state machine (owned by `use-structured-creation`, mounted in the library):

| State | Rendering | Announcement |
| --- | --- | --- |
| `idle` | no status content | none |
| `analyzing` | indeterminate `ProgressIndicator` labelled `Analyse du dossier…` | deliberately NOT announced |
| `review` | the recognition report in-context (surface `Création depuis un dossier`): the quality chip, the folder basename, the per-aspect findings grouped `Ce que Rustory a reconnu` / `Points d'attention`, the `Ce qui sera créé` group (the normalized title, the node count, the retained media and the discarded ones BY BASENAME — the per-file detail lives here, never in the persisted findings), and — when creatable — the UNIQUE CTA `Créer l'histoire` THEN `Abandonner` in tab order (the report already says what will be discarded — no second CTA). A `blocked` verdict is `role="alert"` (only `Abandonner`, no summary — nothing will be created); a creatable one is `aria-live="polite"` | `role="alert"` if blocked, else `aria-live="polite"` |
| `creating` | indeterminate `ProgressIndicator` labelled `Création en cours…` | deliberately NOT announced |
| `created` | `Histoire créée dans ta bibliothèque` (success chip) + the created title + explicit `Fermer`; the library reloads (authoritative re-read); the editor is NOT auto-opened — the fresh card (with its possible marker) IS the sober success feedback | `aria-live="polite"`, mounted, `aria-atomic` |
| `failed` | `Création impossible` block with the canonical `message` + `userAction`, buttons `Réessayer` THEN `Fermer` | `role="alert"` |

Entry point: the library bar keeps ONE primary CTA (`Créer une histoire`,
UX-DR26). The `CreateStoryDialog` is the creation CHOICE: the interactive path
(title → `Créer`) stays primary; a secondary entry `Ou démarre depuis un
dossier préparé hors de Rustory` + `Choisir un dossier…` closes the dialog and
starts this flow. No third button in the bar.

Inherited behavior (documented honestly): the created story carries the FULL
edit scope by construction and joins the `Import Review Resolution Contract`
unchanged — a real write that leaves the canonical story fully sound settles a
pending `partial` / `needs_review` review (media slots are NEVER part of that
oracle: the `partiel` marker is settled by a sound canonical write even while
media slots stay empty — the empty slots remain visible in the editor, which is
the intended durable state). A `structured-folder` story with no device pack
stays `NotTransferable` at the write plan, exactly like a native story.

Invariants (locked by tests):

- **No mutation before acceptance (AC4)**: analysis alone leaves zero row and
  zero store file; a `blocked` verdict creates NOTHING; `Abandonner` is a pure
  frontend reset.
- **Bounded I/O**: manifest read `is_file`-gated then capped
  (`MAX_MANIFEST_BYTES`); every referenced media `symlink_metadata`-gated,
  capped per file and in count/total (`MAX_FOLDER_MEDIA_FILES`,
  `MAX_FOLDER_TOTAL_MEDIA_BYTES`); basenames validated BEFORE any path join;
  the folder is never walked.
- **Atomicity**: ONE transaction for `stories` + provenance + `assets`; a
  failure rolls back fully and compensates the promoted media files (locked by
  fault injection on both insert stages).
- **Authority**: the accept phase re-analyzes from zero; the frontend verdict
  is never trusted; provenance fields are re-validated before INSERT.
- **Orthogonality**: the `.rustory` import flow is untouched (its negative
  tests still hold: it never emits `Missing` nor `partial`); `scope.rs` /
  `review.rs` / the editor spines are inherited WITHOUT modification.
- Offline, zero dependency, zero network; color is never the sole carrier;
  a problem is never carried in a toast alone.

Error taxonomy: transport failures reuse the `IMPORT_FAILED` closed set of the
Local Artifact Import Contract (`file_read`, `db_commit`, `spawn_blocking_join`,
`app_data_unavailable`, `dialog_failed`, `other`) — the functional verdict is
the typed DTO, never an error. A folder whose CONTENT exceeds a bound is a
`Blocking` FINDING (typed verdict), never a transport error. The folder flow
ADDS these documented sub-values to the reused set:

- `file_read.stage` gains `folder_name` (the chosen folder's name cannot be
  carried as a provenance source — no real UTF-8 basename, or outside the
  sobriety rules; an honest refusal, never disguised as a manifest problem),
  `invalid_path` (a forged accept pointer: empty / relative — the system
  dialog never produces one) and `oversize_total` (the bytes actually read at
  promotion exceed `MAX_FOLDER_TOTAL_MEDIA_BYTES` — probe sizes may be stale).
- `db_commit.stage` gains `insert_assets` (the third insert stage of the
  atomic commit).
- `other.cause` gains `media_promotion` (a retained media could not be
  promoted into the managed store, or its bytes changed kind since the
  re-analysis).

## Content Source Activation Contract

Starting a story from an ADDITIONAL content source (an RSS feed today) is
governed by the OFFICIAL CONTENT SOURCE REGISTRY: a distribution-owned
matrix decided in Rust, line by line (the exact pattern of the device
support matrix — activated line by line, never wholesale), that says which
source kinds this distribution activates. Activating a source is a
DISTRIBUTION decision, never a user setting: no table, no migration, no
settings surface, no persistence — an alternative distribution edits the
matrix itself, and the visible "default configuration" required by the
distribution policy IS this code plus its frozen, tested copies.

Closed source kinds (wire tags): `rss` / `atom` / `jsonFeed`, with the
frozen labels `Flux RSS` / `Flux Atom` / `Flux JSON Feed`. Closed
activation states (wire tags):

| Activation (wire) | Product wording | Meaning |
| --- | --- | --- |
| `enabled` | `activée par la distribution officielle` | The kind may be used to create a story; its creation entry is active and carries the frozen entry-level activation marker |
| `notActivated` | `non activée dans la distribution actuelle` | The kind is KNOWN but not activated by this distribution (not implemented / not validated by the support policy); its creation entry renders visible but DISABLED with the frozen short reason |
| `blockedByPolicy` | `bloquée par la politique de distribution` | The kind is deliberately blocked (protected-content-oriented flows are never activated by default); same disabled rendering with its own frozen reason. NO line of the CURRENT matrix carries this state — the variant, its copies and its mappings exist and are tested so that the day a blocked source appears is a re-scope, never an invention |

Current official matrix (frozen by tests, one test per line): `rss` →
`enabled` (its ingestion mechanism is shipped); `atom` → `notActivated`;
`jsonFeed` → `notActivated` (both are known kinds whose ingestion is not
implemented and whose activation the support policy has not validated). A
kind absent from the matrix is fail-closed `notActivated` — never a panic,
never enabled-by-default.

Where the policy is decided, enforced and surfaced:

- **Rust alone decides.** The registry lives in the pure domain; the
  `read_content_source_policy` command serializes it (kind, frozen label,
  activation, the frozen entry-level activation marker
  `activationMarker` on an enabled line — the exact complement of the
  frozen full `reason` carried by the non-enabled lines: each line
  carries exactly one of the two copies, the other key stays absent) as
  a PURE, synchronous read: zero network, zero DB, zero lock. The
  frontend NEVER hardcodes the source list, the labels, the marker or
  the reasons: every consuming surface (the creation dialog, the
  support-profile screen) renders what Rust declares, verbatim.
- **The application facades refuse BEFORE any I/O.** Both RSS facades
  (preview AND accept) consult the matrix they receive as a parameter and
  refuse a non-enabled kind with the dedicated
  `CONTENT_SOURCE_UNAVAILABLE` error BEFORE the address validation and
  BEFORE any network dispatch — zero fetch on a policy refusal, proven by
  the recording mock. The message + gesture are frozen
  (`product-language.md`). Fail-closed everywhere: a kind missing from the
  received matrix refuses exactly like a `notActivated` one.
- **The creation dialog renders the policy.** See `Story Creation
  Contract`: the content-source entries are DRIVEN by the read policy —
  `enabled` → active entry with the frozen activation marker;
  `notActivated` / `blockedByPolicy` → visible but disabled with the
  frozen reason (the `Disabled Actions and Reasons` pattern); policy read
  failed or absent → every external-source entry disabled with the
  fail-closed reason (`Sources externes indisponibles pour l'instant.`) —
  never active-by-default, never blocking the primary title path.
- **The RSS surface keeps the activation visible.** See `External Source
  Creation Contract (RSS)`: the frozen mention `Source activée par la
  distribution officielle.` renders from the surface's opening, DISTINCT
  from (and next to) the content-rights posture line — the posture speaks
  of the CONTENT the user feeds in, the mention speaks of the SOURCE KIND
  the distribution activates; the two coexist. A
  `CONTENT_SOURCE_UNAVAILABLE` refusal renders the dedicated calm
  `unavailable` surface state (defence in depth — nominally unreachable,
  since the dialog never activates a non-enabled source).

Three sealed regimes — never merged, each with its own code, state and
vocabulary (`indisponible` ≠ `échoué`, the Product Glossary distinction):

| Regime | Carrier | Surface state | Gesture |
| --- | --- | --- | --- |
| POLICY refusal (requested source kind not enabled) | `AppError` code `CONTENT_SOURCE_UNAVAILABLE` | `unavailable` — calm (`role="status"`), frozen message + gesture, NO `Réessayer` (a retry never changes a distribution policy) | close / pick an enabled source |
| TRANSPORT failure (network) | `AppError` code `RSS_SOURCE_UNREACHABLE` | `failed` — `role="alert"`, the address field stays editable, `Réessayer` | correct the address, then retry |
| CONTENT verdict (unreadable / non-RSS / empty / diverged) | typed DTO verdict, never an error | `review` (blocked verdict) | `Relance la récupération du flux.` |

Never requalify across regimes: an Atom URL pasted into the RSS surface
stays the FORMAT verdict `Ce flux n'est pas au format RSS supporté.` (the
user used the RSS source — an ENABLED kind — on non-RSS content); the
policy refusal concerns the requested source KIND only, never the fetched
bytes, never the network.

Convergence (the governance proof): a story created from an enabled source
is an ORDINARY canonical story — it enters the EXISTING `Story Validation
/ Preflight Contract`, `Story Preparation Contract`, `Story Transfer
Contract` and `Story Verification Contract` with NO new state, no special
path, no duplicated command, and receives EXACTLY the verdicts of its
native twin at every stage. In the CURRENT distribution a locally-born
story (title path, structured folder and external source alike) carries no
device-format pack, so the shared transfer gate refuses it honestly
(`NotTransferable`, zero device byte — see `Disabled Actions and Reasons`:
`Envoi indisponible: histoire native non transférable (pas de pack
appareil)`); the native-story writer is a deferred capability tracked in
the deferred work, NOT a special path of this flow. An end-to-end
integration test keeps the claim falsifiable on real I/O: the REAL
creation, validation, preparation and transfer facades run against a
writable mount; the ingested story and a byte-identical native twin must
stay indistinguishable stage by stage (the honest refusal included, the
device bytes proven untouched), while an imported-pack witness driven
through the SAME transfer facade reaches the VERIFIED terminal — proving
the shared pipeline runs to verification whenever a pack exists, with no
source-specific branch anywhere.

Diagnostics: a policy refusal appends the dedicated closed category
`content_source_blocked` (the KIND only — never a URL, never a host) to
`import.jsonl`; it is NEVER counted as `rss_source_unreachable` (network)
nor `rss_creation_failed` (local).

## External Source Creation Contract (RSS)

Starting a story from an **external source** (`Créer une histoire` →
`Démarrer depuis une source externe (RSS)`) is a CREATION flow, the exact
sibling of the structured-folder one: the user provides the address of an
RSS feed they follow, Rustory fetches it ON EXPLICIT ACTION ONLY, and the
accepted episode becomes an ORDINARY canonical local draft (fresh UUIDv7,
`created_at = updated_at = now` — a birth), fully editable (`Full` scope by
construction — the provenance lives in `story_local_imports`, which the
edit-scope derivation never consults) and reviewable through the EXISTING
import chip / report / resolution machinery.

Scope declaration: RSS 2.0 is a **content source kind governed by the
`Content Source Activation Contract`** — the distribution-owned registry
whose current official matrix enables `rss` and leaves `atom` / `jsonFeed`
not activated; the support-policy screen stays its own capability. This
flow persists NO source configuration, NO subscription: the feed address
is provided at creation time and never stored (the provenance keeps the
HOST only). An Atom or JSON feed pasted into this surface receives an
honest typed verdict (`format non supporté`) — no implicit partial
support, and NEVER a policy requalification (the policy governs the
requested source KIND, not the fetched bytes).

Offline-first guardrail: the fetch runs ONLY on the explicit `Récupérer le
flux` action (the exact discipline of the official-catalog refresh — no
implicit traffic, ever). The core product flows stay 100% offline; a user
who never opens this surface never generates a byte of RSS traffic.

The flow is **two-phase, with no mutation before acceptance**:

| Phase | Command | Effect |
| --- | --- | --- |
| Preview | `fetch_rss_source_preview` | Validates the address (Rust-authoritative), fetches the feed bounded, parses it (bounded, event-driven) and returns a typed preview DTO: the source HOST, the exploitable items (bounded list), the flow-level findings and the derived state — or a BLOCKED verdict. The preview is PURE: zero byte written, zero DB row, zero store file. |
| Accept | `accept_rss_story_creation` | On the explicit `Créer le brouillon` action: RE-FETCHES and RE-PARSES the feed from zero (**the source is the authority** — the network equivalent of "the disk is the authority"; the frontend never re-submits content). The chosen item is resolved by STRICT `guid` when present, else by exact (`title`, `link`), THEN re-proven against the previewed-content FINGERPRINT carried by the reference (a canonical hash of title/text/guid/link/enclosure — the wire is a pointer + a PROOF): a missing/ambiguous item, a feed turned blocked, or a resolvable item whose CONTENT diverged since the preview is the honest recoverable refusal `La source a changé depuis la récupération.` with ZERO mutation — a creation can never ingest content the user never reread. Otherwise ONE `BEGIN IMMEDIATE` transaction inserts `stories` (canonical v3 minimal structure whose start node carries the cleaned item text; normalized title with the `Histoire de {hôte}` fallback) + `story_local_imports` (`source_format = 'rss'`, `source_name` = host, `artifact_checksum` = SHA-256 of the SECOND fetch's bytes — the bytes actually ingested). A transaction failure rolls back fully: nothing remains. |
| Abandon | — (pure frontend) | `Abandonner` drops the preview. Nothing was mutated, so no command is needed. |

Error taxonomy (the central discipline): **transport is an `AppError`,
content is a typed VERDICT** — never the other way around.

- Transport (`code = RSS_SOURCE_UNREACHABLE`): unreachable host, timeout,
  budget exhausted, response over the byte cap, invalid address
  (`details.stage` closed set: `url_invalid` / `request` / `read` /
  `response_oversize` / `budget`). PII-free: a stable stage token — never
  the URL, never the host in `details`, never a raw network message.
  STRICTLY network: a LOCAL failure of the accept (DB commit, system
  clock, worker join) reuses the closed `IMPORT_FAILED` taxonomy of the
  sibling creation flows (`details.source` = `db_commit` /
  `spawn_blocking_join` / `other`) — exactly like the folder flow; the
  diagnostics follow the same boundary (`rss_source_unreachable` vs
  `rss_creation_failed` categories).
- Content verdicts (typed, in the DTO, NEVER an error): unreadable XML →
  `Ce contenu n'est pas un flux RSS lisible.`; a non-RSS-2.0 root (Atom,
  anything else) → `Ce flux n'est pas au format RSS supporté.`; zero
  exploitable item → `Ce flux ne contient aucun épisode exploitable.`; and
  at accept time, a diverged source → `La source a changé depuis la
  récupération.` Every verdict carries the same next gesture: `Relance la
  récupération du flux.`

Bounds (all named, all tested): response cap 8 MiB read cap+1; parse depth
32; at most 100 items retained (beyond: ignored, documented here); item
text cleaned (HTML tags stripped, whitespace collapsed) and truncated at
65 536 chars; feed address at most 2048 chars, `http`/`https` only, no
userinfo, non-empty host; at most 5 redirects; fetch budget 30 s shared
across the whole request. The RSS network client is DEDICATED (its own
`reqwest` blocking client behind the `RssFeedSource` trait) — the
official-catalog source is a NEIGHBOR, not a base class; the duplication
of its disciplines (shared budget, cap+1 read, PII-free stages) is
deliberate.

Recognition model: the ingestion NEVER produces a `recognized` story. The
nominal provenance finding `(source, ambiguïté)` — `Contenu ingéré depuis
une source externe (RSS).` — is emitted for EVERY ingestion: external
content that was not reread is never "clean", so the durable state floor
is `needs_review` (the dedicated per-flow derivation: any `Blocking` →
`blocked`, nothing created; else any `Missing` → `partial`; else →
`needs_review`). The DB invariant (`recognized` ⟺ `findings_summary IS
NULL`) holds by construction — an `rss` provenance row ALWAYS carries a
summary. A referenced enclosure (podcast audio…) is NOT downloaded: it
becomes the `(media, information manquante)` finding → state `partial` —
that is the honest "qualité partielle" and a named review step. The
existing review resolution (a sound write from the editor settles
`needs_review` / `partial` → `resolved`) applies UNCHANGED — the
resolution query is format-agnostic.

Per-item findings vs flow findings: the PREVIEW carries the flow-level
findings (`envelope` / `formatVersion` recognized + the nominal `source`
ambiguity — floor state `needsReview`); the per-item facts surface in the
item list itself (title, truncated summary, `hasEnclosure`). The findings
PERSISTED at accept time are derived for the CHOSEN item (envelope +
format + source + the item's title/text adjustments + its enclosure), so
the created story's chip, report and durable state speak of what was
actually ingested.

UI state machine (owned by `use-rss-creation`, mounted in the library;
surface `Création depuis une source externe`; seven states — `idle →
fetching → review → creating → created → failed | unavailable`). The
surface is OPENED by the creation dialog's third entry and renders NOTHING
while closed; unlike the folder flow (whose input is a native picker), the
address input lives IN the surface, so the open surface shows the frozen
activation mention (`Source activée par la distribution officielle.` —
see `Content Source Activation Contract`; it coexists with the posture
line, distinct lines, both visible), the posture line, the
`Adresse du flux RSS` field and `Récupérer le flux` in every non-terminal
state — with ONE deliberate exception: the `unavailable` policy refusal
renders NONE of these (no activation mention — it would contradict the
refusal —, no posture line, no address field, no fetch CTA; only the calm
status block and `Abandonner`, because no retry gesture exists against a
distribution decision):

| State | Rendering | Announcement |
| --- | --- | --- |
| closed | nothing | none |
| `idle` (open) | the visible content-rights posture line (`Utilise uniquement des contenus dont tu as les droits : tes contenus personnels ou des contenus libres.`), the `Adresse du flux RSS` field, the `Récupérer le flux` CTA (soft-disabled while the address is empty) and `Abandonner` (closes the surface — a pure frontend reset, available in every non-terminal state) | none |
| `fetching` | indeterminate `ProgressIndicator` labelled `Récupération du flux…` (Long Operation Rule); `Abandonner` stays reachable (the in-flight result is then ignored — the surface never resurrects) | deliberately NOT announced |
| `review` (exploitable) | the source HOST (never the full address), the flow findings (existing category chips + messages), the BOUNDED selectable item list (title + truncated summary + the `Média distant non récupéré` note when `hasEnclosure`), then the UNIQUE CTA `Créer le brouillon` (aria-disabled until an item is selected AND while the typed address diverges from the reviewed one — the accept must never silently target the OLD source) THEN `Abandonner` in tab order. The field + `Récupérer le flux` stay available (re-fetch replaces the preview) | `aria-live="polite"` |
| `review` (blocked verdict) | the verdict findings in a `role="alert"` block (message + gesture — nothing will be created), only `Abandonner`; the field + `Récupérer le flux` stay available to correct and retry | `role="alert"` |
| `review` (source changed, after a refused accept) | the frozen verdict `La source a changé depuis la récupération.` + gesture in a `role="alert"` block; the stale items are DROPPED (never re-proposed); the field + `Récupérer le flux` stay available; `Abandonner` closes | `role="alert"` |
| `creating` | indeterminate `ProgressIndicator` labelled `Création en cours…`; `Abandonner` stays reachable (the UI stops listening; an accept that already reached Rust still settles atomically — the fresh card appears on the next authoritative overview read) | deliberately NOT announced |
| `created` | `Histoire créée dans ta bibliothèque` (success chip) + the created title + explicit `Fermer`; the library reloads (authoritative re-read); the editor is NOT auto-opened — the fresh card with its `à revoir` / `partiel` chip IS the sober success feedback. The address form is dropped (the activation mention, a surface-level line, STAYS rendered); closing forgets the typed address (a feed URL can carry a private token) | `aria-live="polite"`, mounted, `aria-atomic` |
| `failed` (transport) | the canonical `message` + `userAction` of the `AppError` in a `role="alert"` block, buttons `Réessayer` THEN `Fermer`. The `Adresse du flux RSS` field STAYS visible and editable (the gesture is "correct the address, then retry" — in-context, never close/reopen); `Réessayer` re-runs the fetch with the CURRENT field value, and the form's own fetch CTA yields to it | `role="alert"` |
| `unavailable` (policy refusal — defence in depth, nominally unreachable) | the frozen `message` + `userAction` of the `CONTENT_SOURCE_UNAVAILABLE` refusal (`Cette source de contenu n'est pas activée dans la distribution officielle.` + `Utilise une source activée ou consulte le profil de support de ta version.`) in a CALM `role="status"` block (never `role="alert"` — a distribution policy is not a breakage), NO `Réessayer` (a retry cannot change the policy; retry actions are no-ops in this state), only `Abandonner` (the gesture: close, then pick an enabled source); the deliberate exception to the always-visible surface elements — NO activation mention, NO posture line, NO address field, NO fetch CTA; never confused with `failed` (which keeps the field + `Réessayer`) | `aria-live="polite"` — routed through the PERSISTENT live region (mounted before the transition, like `created`; the visual `role="status"` block mounts already filled, which screen readers do not reliably vocalize) |

Entry point: the library bar keeps ONE primary CTA (`Créer une histoire`).
The `CreateStoryDialog` gains a THIRD optional entry `Démarrer depuis une
source externe (RSS)` (the exact pattern of the folder entry: not rendered
when the route wires no handler, closes the dialog then delegates, gated
by the same cross-flow busy exclusivity). The title path stays primary;
the folder entry is untouched.

Invariants (locked by tests):

- **No mutation before acceptance**: the preview writes zero byte and zero
  row; a blocked verdict creates NOTHING; `Abandonner` is a pure frontend
  reset; a refused accept (`La source a changé`) mutates NOTHING.
- **Re-proven accept**: the accept re-fetches and re-parses from zero;
  the item is resolved by strict `guid` (else exact `title`+`link`) AND
  its fresh content must match the previewed-content fingerprint; any
  divergence refuses honestly — NEVER a creation from the stale preview
  data, NEVER an approximate match, NEVER content the user did not
  reread. The persisted checksum fingerprints the SECOND fetch's bytes.
- **Atomicity**: ONE transaction for `stories` + provenance; a failure
  rolls back fully (no media is ever downloaded, so there is nothing to
  compensate).
- **Floor**: an `rss` story is never `recognized`; its summary is never
  NULL; the `(source, ambiguïté)` finding is always present.
- **Orthogonality**: the `.rustory` import, the structured-folder creation,
  the official catalog and every device/transfer flow are untouched; the
  review resolution and the card chip/report are REUSED without
  modification.
- **PII hygiene**: the provenance, the diagnostics and every error carry
  the HOST at most — never the full address (feed query strings can carry
  private tokens), never the feed content.
- Offline-first: no implicit fetch, ever; color is never the sole carrier;
  a problem is never carried in a toast alone.

## Imported Story Edit Scope Contract

An imported story is editable within the EDIT SCOPE DECLARED for its import
format (FR21) — never "imported = read-only" as a block. The scope is derived
in Rust ONLY (`story_edit_scope`), from the story's import provenance, and the
frontend never recomposes it:

| Provenance | Scope | Meaning |
| --- | --- | --- |
| no import row (native story) | `full` | the complete editor — content, media, structure, option links |
| `story_local_imports` row (`.rustory` import or structured-folder creation) | `full` | the imported/created canonical structure is the SAME v3 model as a native story; it edits exactly like one (the node/media/structure/option-link edit paths, unchanged) |
| `story_imports` row (device pack) | `titleOnly` | the content is carried by the binary pack copied from the device (the local canonical row is a placeholder); only the TITLE — a local Rustory metadata, packs store none — is editable |
| provenance query error | `titleOnly` | fail-closed: a read hiccup must never let a write slip through |
| forged rows in BOTH tables | `titleOnly` | the pack takes precedence (its placeholder content must never be edited) |

Scope × surface (the authoritative truth table):

| Surface | `full` (native or `.rustory` import) | `titleOnly` (device pack) |
| --- | --- | --- |
| `get_story_detail` projection | `editScope: "full"`, `editable: true`, `importState` present (4 states or `null`) | `editScope: "titleOnly"`, `editable: false`, `importState: null` (never projected outside `full`) |
| Node + structure write spines | accepted (the node/structure write paths, unchanged) | refused authoritatively — `LIBRARY_INCONSISTENT`, `details.source` `node_not_editable` / `structure_not_editable`, the revised pack messages (no promise of a future version) |
| Title (`update_story`) | accepted — a local metadata | accepted — the SAME local metadata (renaming a pack locally is the established local rename behavior, contractualized) |
| Editor zones | full editor | both content zones render a NAMED pack state; the controls are ABSENT, not disabled; the navigator is NOT mounted (a pack's placeholder graph is a lying projection) |

Frozen refusal copy (same error code and `details.source` as before — only the
text changed; no version promise):

- message: `Le contenu de cette histoire est porté par le pack copié depuis l'appareil et ne peut pas être modifié ici.`
- userAction: `Tu peux modifier le titre depuis l'éditeur ; le contenu du pack reste celui de l'appareil.`

Invariants:

- `editable` stays on the wire as a DERIVED compatibility flag — always equal
  to `editScope === "full"`; the TS guard refuses any divergence (drift error).
- `importState` is projected ONLY for a `full`-scope story, on the detail AND
  on every write acknowledgement — one shared Rust derivation, so the two can
  never diverge, even on forged data.
- The edit scope changes NOTHING for the transfer write-plan gate: a `.rustory`
  import stays `NotTransferable` (no pack files), a device pack stays
  transferable — verdict and gate remain orthogonal.

## Import Review Resolution Contract

The import review of a file-provenance story (`needs_review` / `partial` — a
`.rustory` import or a structured-folder creation) is SETTLED BY EDITING — no
button, no ceremony, no guided flow (AC3). The durable marker resolves when a
REAL write leaves the canonical story fully sound:

| Aspect | Value |
| --- | --- |
| One-way transition | `UPDATE story_local_imports SET import_state = 'resolved' WHERE story_id = ? AND import_state IN ('needs_review','partial')` — conditional by construction, so a `resolved` story NEVER regresses to `needs_review` (the living validation owns the present). |
| Oracle | The COMPLETE `validate_canonical` blocker list over the post-mutation facts must be EMPTY — any blocker of ANY severity (a still-broken option link is Fixable) prevents resolution. The spines already compute that list; no extra I/O. Node MEDIA are NEVER part of the oracle (a media `attention` slot is not a `.rustory` import finding; its per-slot marker lives its own life). |
| Write sites | Inside the SAME write transaction of: `apply_node_mutation`, `apply_structure_mutation`, `update_story` (title — after a cheap early-out so a native autosave never pays a structure parse), `apply_recovery` (a title recovery is a real write; a node recovery goes through the node spine and is covered there). |
| What does NOT resolve | Reading (never write-on-read); an acknowledged structural no-op (no real write happened — the ACK still carries the current state); any write on a non-`full` story (the forged two-table case re-checks the scope). |
| Findings trace | `findings_summary` is KEPT in base forever (the review's trace) — never rendered for a `resolved` story. |
| Timestamps | The state transition alone NEVER bumps `stories.updated_at` (a card must not resurface in a recency sort because a chip went out). |
| Acknowledgements | The three write outputs (`UpdateStoryOutputDto`, `NodeWriteOutputDto`, `StructureWriteOutputDto`) carry `importState` (required key, explicit `null`), read POST-UPDATE in the same transaction through the same None-unless-`full` derivation as the detail. The frontend reconciles with LOCAL MONOTONICITY: a local `resolved` is never overwritten by a stale in-flight `needsReview`/`partial` acknowledgement. |
| Editor surface | A static review chip in the editor shell banner for `needsReview` / `partial` (the card labels `à revoir` / `partiel`, warning tone, NEVER `role="alert"` — a durable state, not an action error). NOTHING is rendered for `recognized` / `resolved` / `null`: the chip's disappearance IS the feedback, no success announcement. |
| Library surface | A `resolved` card renders EXACTLY like a `recognized` one: provenance `Importée` only, no chip, no report (`read_stories` projects `importState: "resolved"` WITHOUT `importReport`). |

## Story Recovery Contract

A buffered keystroke value can survive an unexpected app shutdown
(crash, kill -9, power loss). On the next mount of `/story/:storyId/edit`
the route detects the surviving draft and proposes a recovery banner
above the editable Field. The user must commit a decision (Apply or
Discard) before resuming editing.

| Aspect | Value |
| --- | --- |
| Trigger rule | At every mount of `/story/:storyId/edit`, `useStoryRecovery(storyId)` calls `read_recoverable_draft({ storyId })`. The banner mounts only when the response is `{ kind: "recoverable" }`. |
| Detection rule | At boot, `lib.rs::run().setup` queries `SELECT story_id FROM story_drafts ORDER BY draft_at DESC` and emits a single `interrupted_session_detected` event into `{app_data_dir}/diagnostics/recovery.jsonl` with the full list of story_ids. The boot probe never blocks on a log write failure. |
| Banner UX | A `<section role="region" aria-label="Brouillon récupéré">` rendered ABOVE the `<h1>`, containing: heading `Brouillon récupéré`, two-line diff `Tu avais tapé : "X"` / `Dernier état enregistré : "Y"`, relative timestamp, two buttons `Restaurer le brouillon` (primary) and `Conserver l'état enregistré` (secondary), and a conditional `role="alert"` block on apply / discard failure. |
| Field locking | While the banner is on screen (`kind ∈ { recoverable, applying, error }`), the title `Field` is `disabled` and the export button is soft-disabled (`aria-disabled="true"`). The user must commit a decision before resuming any editing. |
| Apply rule | `applyRecovery({ storyId })` runs an atomic transaction in Rust: `UPDATE stories SET title = ? + DELETE FROM story_drafts WHERE story_id = ?`. The frontend receives a `UpdateStoryOutput` and patches its in-memory `useStoryEditor` snapshot via `reloadDetailFromOutput` — no follow-up `get_story_detail` round-trip is required. |
| Discard rule | `discardDraft({ storyId })` removes the row without touching `stories`. Idempotent: a second call on an already-empty row resolves silently. |
| Auto-clear rule | `update_story` (autosave) deletes the `story_drafts` row for the same `story_id` inside its existing `BEGIN IMMEDIATE` transaction. A successful autosave consumes the buffer; a failed autosave preserves it for the next session. |
| Validation rule | At apply time, `validate_title(normalize_title(draft_title))` re-validates authoritatively. A draft with control chars / blank-after-trim / > 120 chars is rejected with `INVALID_STORY_TITLE`. The draft row is NOT consumed on rejection so the UI can offer Discard explicitly. |
| Stable error categories | The recovery log produces only five categories: `interrupted_session_detected`, `recovery_draft_proposed`, `recovery_draft_applied`, `recovery_draft_discarded`, `recovery_draft_unavailable`. The category is the NFR24 stable identifier — never a localized message, never a free-form string. |
| Persistence cadence | `useStoryEditor` schedules a `record_draft` 150 ms after each keystroke. The autosave (`update_story`) runs on a 500 ms debounce in parallel. A keystroke between the two windows is captured by the recovery buffer even though no autosave has fired yet — that is the purpose of the shorter window. |
| Persistence cap | The `draft_title` column carries a SQLite `CHECK (length(draft_title) <= 4096)` clause. The Rust application service mirrors the cap by char count and rejects oversize inputs with `RECOVERY_DRAFT_UNAVAILABLE` (`details.source = "draft_too_long"`) before the SQL ever runs. |
| Forbidden | No modal. No toast. No automatic recovery without user choice. No generic "Erreur de récupération" copy — the closed message table from `product-language.md` is the only allowed phrasing. |

### Error payloads — recovery flow

The recovery commands surface failures with a stable `details.source`
discriminator so support can triage without parsing the user-facing
message:

- `record_draft` UPSERT fails — `RECOVERY_DRAFT_UNAVAILABLE`, `details.source = "sqlite_upsert"`. `details.kind` ∈ `busy`, `locked`, `other`.
- `read_recoverable_draft` SELECT fails — `RECOVERY_DRAFT_UNAVAILABLE`, `details.source = "sqlite_select"`.
- `apply_recovery` UPDATE+DELETE atomic fails — `RECOVERY_DRAFT_UNAVAILABLE`, `details.source = "sqlite_apply"`. `details.stage` ∈ `begin_transaction`, `read_draft`, `update`, `delete`, `commit`. A `constraint_violation` re-maps to `INVALID_STORY_TITLE`.
- `discard_draft` DELETE fails — `RECOVERY_DRAFT_UNAVAILABLE`, `details.source = "sqlite_delete"`.
- `recovery_log` write fails — `RECOVERY_DRAFT_UNAVAILABLE`, `details.source ∈ { diagnostics_dir, diagnostics_open, diagnostics_write, diagnostics_flush, diagnostics_serialize, diagnostics_app_data_dir, diagnostics_path_invalid, diagnostics_rotate }`. `details.kind` ∈ `permission_denied`, `storage_full`, `read_only_filesystem`, `not_found`, `other`.
- Draft input exceeds 4096 chars — `RECOVERY_DRAFT_UNAVAILABLE`, `details.source = "draft_too_long"`.
- Draft row vanished between propose and apply — `RECOVERY_DRAFT_UNAVAILABLE`, `details.source = "draft_missing_in_transaction"`.
- Draft fails authoritative validation at apply time — `INVALID_STORY_TITLE` with `details.source = "recovery_draft_invalid"`. The draft row is preserved so Discard remains available.
- Story disappeared (FK violation or rows_affected == 0) — `LIBRARY_INCONSISTENT` with `details.source = "story_missing"`.

## Support Profile Screen Contract

The read-only screen where the user consults what the OFFICIAL
DISTRIBUTION supports: device families and firmware cohorts, local
artifact types, content sources, and the distribution posture. It is
the navigable in-app face of the documented support profile
([device-support-profile.md](./device-support-profile.md) stays the
detailed developer reference) and the internal target the existing
`Consulter le profil de support` gesture navigates to.

| Aspect | Value |
| --- | --- |
| Route | `/settings` — the `settings` dominant context. The screen is a standalone single-column view (the `StoryEditRoute` pattern), NOT the three-column library grid. |
| Screen title | `<h1>` `Profil de support` (canonical term — never `matrice` in user copy). The `<main>` carries `aria-label="Profil de support"`. |
| Version header | The app version renders in the header as `Version {version}` via `getVersion()` (`@tauri-apps/api/app`, covered by `core:default`). The `consulte le profil de support de ta version` gesture copy becomes literal. If the version read fails, the line is omitted — never an invented value. |
| Entry points | (a) A permanent navigation entry in the library left column (below the filters block): a light `SurfacePanel` + quiet `Button` labeled `Profil de support` that navigates to `/settings` — no business state in the column. (b) The existing `Consulter le profil de support` gesture (detection panel, device story inspector, device import surface) navigates IN-APP to `/settings` — no external browser, no network. |
| Exit | A `Retour à la bibliothèque` button navigating to `/library`. |
| Read model | Content is ENTIRELY Rust-driven through two independent pure reads at route entry: `read_support_profile` (devices + local artifacts, frozen labels and reasons) and `read_content_source_policy` (content sources, reused VERBATIM from the Content Source Activation Contract — the screen is a second consumer, never a second truth). NO hardcoded TS list of families, cohorts, kinds, labels or reasons. Reads are pinned by a per-mount token (the `policyReadTokenRef` pattern) so a stale resolution never applies. |
| Screen states | `loading` (accessible via `aria-busy`) → `loaded` \| `unavailable` PER SECTION: a failed read renders the affected section(s) in a calm `unavailable` state (honest frozen copy, `role="status"`, NO `Réessayer` — a failed pure read is a contract drift, not a transient failure) while the sections whose read succeeded stay fully served (fail-closed per section, never invented content). |
| Sections | Four `<h2>` sections, in order: `Appareils` (the device support matrix grouped by family — every cohort line with its metadata format label and its four capability lines), `Artefacts locaux` (the three artifact registry lines plus the node-media formats line `Formats acceptés : images PNG, JPEG ; sons MP3, WAV, OGG` VERBATIM), `Sources de contenu` (the read policy rendered as-is: an enabled kind with its Rust-carried frozen entry-level marker `Activée par la distribution officielle` — the DTO's `activationMarker` — non-enabled kinds with their Rust-carried frozen reasons), `Politique de distribution` (the frozen posture copy derived from the PRD distribution policy). |
| Capability rendering | An available capability renders StateChip `success` + `Disponible`; a non-available one renders StateChip `neutral` + `Non disponible dans cette version` PLUS its frozen Rust reason — NEVER a bare ✗, NEVER tone `error`/`warning`, NEVER `role="alert"`: a distribution limit is durable calm information, not a runtime error (the fourth vocabulary — see the non-collision rule in [product-language.md](./product-language.md)). Chips carry glyphs; color alone never carries the distinction. |
| Read-only | NO setting, NO toggle, NO persistence: activation is a DISTRIBUTION decision (Content Source Activation Contract). `/settings` is the route context; `Profil de support` is the screen. |
| Offline | The screen triggers ZERO network request (NFR14). Repatriating the support-profile gesture REMOVES the only external-browser call of that path and introduces none. |
| Forbidden | No modal, no toast, no new design-system component (the matrix composes SurfacePanel / StateChip / Button), no runtime non-support copy reused for a matrix line (the detection panel's `raison de non-support` vocabulary stays its own). |

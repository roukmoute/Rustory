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
| `presumed_transferable` | `présumée transférable` | Valid for decision surfaces before send |
| `preparing` | `en préparation` | May coexist with preserved local work |
| `transferring` | `en transfert` | Must stay visibly in-context in the library |
| `verified` | `transférée et vérifiée` | Only after explicit confirmation |
| `partial` | `état partiel` | Never collapse into success wording |
| `retryable` | `échec récupérable` | Keep enough context for `Relancer` |

## Post-MVP Import State Contract

These states are for post-MVP local structured import flows and must not be mistaken for MVP transfer states:

| Internal Contract State | UI Label | Scope |
| --- | --- | --- |
| `recognized` | `reconnu` | Import analysis found usable material |
| `partial` | `partiel` | Some content is usable, some is not |
| `needs_review` | `à revoir` | The user must inspect before accepting |
| `blocked` | `bloqué` | Import cannot continue safely |
| `resolved` | `résolu` | The import issue has been handled |

## State Transition Rules

- `brouillon local` may lead to `en vérification` or remain local.
- `en vérification` may lead to `bloquée` or `présumée transférable`.
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
- `Création impossible: titre trop long (120 caractères maximum)`
- `Création impossible: titre contient des caractères non autorisés`

Avoid:

- `Action impossible`
- `Erreur`
- `Not ready`

## Screen Ownership

- `bibliothèque` is the stable home for selection, transfer visibility, and recovery traces
- `édition` is a separate context and must not silently replace library state vocabulary
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
| Left column | Global filters / navigation entry points | `<nav aria-label="Filtres bibliothèque">` | `minmax(240px, 280px)` | `200px` |
| Center column | Story collection — the main work surface | `<main aria-label="Collection d'histoires">` | `1fr` | `1fr` |
| Right column | Decision panel (device state + send CTA) | `<aside aria-label="Panneau de décision">` | `minmax(320px, 360px)` | `300px` |

Rules:

- The center column is the only working surface; the two side columns support decision without stealing focus.
- Error, empty, filtered-empty and loading states all live inside the center column — never in the nav or in the panel.
- The right-column panel stays intentionally minimal: device state + one primary CTA + a short, standardized reason when the CTA is disabled.
- No fourth column, no collapsible drawer, no hamburger.

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
| `*` | redirect to `/library` | Unknown paths bounce back to the library. |

Rules:

- Returning to `/library` from `/story/:storyId/edit` preserves shell continuity (selection, filters) through the Zustand store — the URL does not carry that state.
- New routes land here only when a real dominant-context switch appears. The `settings` route is not wired at this stage; add it when a specific need emerges.

## Story Creation Contract

A new `brouillon local` is created through a single modal dialog in the `library` context. No other surface exposes a creation entry point.

| Aspect | Value |
| --- | --- |
| Entry points | Header CTA `Créer une histoire` inside `Story Collection`, plus the same CTA inside the `loaded-empty` region. Both are active in parallel whenever the route wires a handler; they dispatch the exact same flow. |
| Input | A single `Titre` field. No `description`, `genre` or `cover image` at this stage — the editor surface is deferred. |
| UI validation | Mirrors the Rust domain rules so `aria-disabled` flips at typing speed: the normalized title (NFC + trim) must be non-empty, at most `120` Unicode code points, and contain no C0 / C1 control characters nor any code point from the Unicode denylist below. |
| Unicode denylist | Beyond C0 (`U+0000..U+001F`) and C1 (`U+007F..U+009F`) controls, the following `Cf` and line-separator code points are rejected because they would make a title hidden, bidirectionally ambiguous, or carry embedded line breaks: `U+FEFF` (BOM / ZWNBSP), `U+202A..U+202E` (LRE / RLE / PDF / LRO / RLO bidi overrides), `U+2066..U+2069` (LRI / RLI / FSI / PDI bidi isolates), `U+200E` (LRM), `U+200F` (RLM), `U+061C` (ALM), `U+2028` (LINE SEPARATOR), `U+2029` (PARAGRAPH SEPARATOR). ZWJ (`U+200D`) and ZWNJ (`U+200C`) are deliberately allowed — they are load-bearing for many scripts and emoji sequences. |
| Authoritative validation | Rust re-validates on every `create_story` call; a title that slipped past the UI is refused via `AppError { code: "INVALID_STORY_TITLE" }` and no row is inserted. |
| Canonical model | Persisted with `schema_version = 1` and a minimal `CanonicalStructure` `{ "schemaVersion": 1, "nodes": [] }`. Any future extension of the canonical shape MUST bump `schema_version` and ship an SQL migration. |
| Integrity | `content_checksum` is the SHA-256 hex digest of the exact `structure_json` bytes written to disk. |
| Timestamps | `created_at` equals `updated_at` on first insert; both are ISO-8601 UTC at millisecond precision (`YYYY-MM-DDTHH:MM:SS.sssZ`). |
| Ordering | Library default sort is `ORDER BY created_at ASC, id ASC`. UUIDv7 keeps the ordering stable without an extra secondary key. |
| Post-success flow | The module-local SWR cache for `useLibraryOverview` is invalidated, a fresh fetch is triggered, and the router navigates to `/story/:storyId/edit` with `replace: true` so the history stack stays flat. |
| Failure recovery | On rejection, the dialog stays open, the typed title survives, the focus returns to the field, and the Rust-supplied `message` + `userAction` are rendered inside a `role="alert"` region below the field. |

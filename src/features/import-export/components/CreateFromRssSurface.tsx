import type React from "react";
import { useEffect, useId, useState } from "react";

import {
  Button,
  Field,
  ProgressIndicator,
  StateChip,
} from "../../../shared/ui";
import type {
  ImportFinding,
  RssItemRef,
  RssPreviewItem,
} from "../../../shared/ipc-contracts/import-export";
import { rssItemRefKey } from "../../../shared/ipc-contracts/import-export";
import { categoryLabel, categoryTone } from "../lib/recognition-labels";
import type { RssCreationStatus } from "../hooks/use-rss-creation";

import "./CreateFromRssSurface.css";

export interface CreateFromRssSurfaceProps {
  /** The surface renders NOTHING while closed. Opened by the creation
   *  dialog's third entry; `Abandonner` / `Fermer` close it. */
  open: boolean;
  status: RssCreationStatus;
  /** Fetch the feed at the typed address (`Récupérer le flux`) — also the
   *  `Réessayer` action after a transport failure. */
  onFetch: (url: string) => void;
  /** Tick / untick one previewed item. */
  onToggleItem: (ref: RssItemRef) => void;
  /** Tick every previewed item (`Tout sélectionner`). */
  onSelectAll: () => void;
  /** Untick every previewed item (`Tout désélectionner`). */
  onSelectNone: () => void;
  /** Commit the ticked items as ONE story (`Créer l'histoire`). */
  onAccept: () => void;
  /** Abandon the flow (pure frontend, no mutation) and close the surface. */
  onAbandon: () => void;
  /** Dismiss a terminal status (`created` / `failed`) and close the surface. */
  onDismiss: () => void;
}

/**
 * In-context surface for the RSS external-source creation flow (`Création
 * depuis une source externe`), mirroring the `CreateFromFolderSurface`
 * discipline: never a toast for a problem, never a modal, `role="alert"`
 * for a blocked / diverged / failed state, `aria-live="polite"` for the
 * report + success. Renders nothing while closed. The feed address lives
 * IN the surface (unlike the folder flow, whose input is a native picker)
 * and only its HOST ever renders back from Rust.
 */
export function CreateFromRssSurface({
  open,
  status,
  onFetch,
  onToggleItem,
  onSelectAll,
  onSelectNone,
  onAccept,
  onAbandon,
  onDismiss,
}: CreateFromRssSurfaceProps): React.JSX.Element | null {
  const addressFieldId = useId();
  const [feedUrl, setFeedUrl] = useState<string>("");

  // A closed surface forgets the typed address: a full feed URL can carry
  // a private token in its query string — it must never resurface (nor be
  // re-fetchable by mistake) on the next opening.
  useEffect(() => {
    if (!open) {
      setFeedUrl("");
    }
  }, [open]);

  if (!open) return null;

  const isBusy = status.kind === "fetching" || status.kind === "creating";
  const canFetch = feedUrl.trim().length > 0 && !isBusy;
  // The field stays visible on a transport failure too (the gesture is
  // "correct the address, then retry" — in-context, never close/reopen);
  // the success terminal drops the form, and so does the policy refusal
  // (`unavailable`): no retry can change a distribution decision, so
  // keeping the field would promise a gesture that does not exist.
  const showAddressForm =
    status.kind !== "created" && status.kind !== "unavailable";
  // The failed block owns the retry gesture — the form's own fetch CTA
  // would be a duplicate there.
  const showFetchCta = status.kind !== "failed";

  return (
    <section
      className="create-from-rss"
      aria-label="Création depuis une source externe"
    >
      {/* Polite region mounted while the surface is shown so AT picks up
          the terminal announcements atomically: the success chip AND the
          policy refusal route through THIS persistent region (a live
          region inserted into the DOM already filled — like the visual
          `role="status"` block below — is not reliably announced; only
          CHANGES of an existing region are). */}
      <div
        className="create-from-rss__live"
        aria-live="polite"
        aria-atomic="true"
      >
        {status.kind === "created"
          ? "Histoire créée dans ta bibliothèque"
          : status.kind === "unavailable"
            ? status.error.message
            : ""}
      </div>

      {status.kind !== "unavailable" ? (
        // The frozen activation mention (Content Source Activation
        // Contract), visible from the surface's opening — DISTINCT from
        // the content-rights posture line below (the mention speaks of the
        // SOURCE KIND the distribution activates, the posture of the
        // CONTENT the user feeds in; both coexist). Deliberately NOT
        // rendered on the policy refusal: the mention would contradict it.
        <p className="create-from-rss__activation">
          Source activée par la distribution officielle.
        </p>
      ) : null}

      {showAddressForm ? (
        <>
          <p className="create-from-rss__posture">
            Utilise uniquement des contenus dont tu as les droits : tes
            contenus personnels ou des contenus libres.
          </p>
          <Field
            id={addressFieldId}
            label="Adresse du flux RSS"
            value={feedUrl}
            onChange={setFeedUrl}
          />
          <div className="create-from-rss__actions">
            {showFetchCta ? (
              canFetch ? (
                <Button
                  variant="primary"
                  onClick={() => onFetch(feedUrl.trim())}
                >
                  Récupérer le flux
                </Button>
              ) : (
                <Button variant="primary" aria-disabled="true">
                  Récupérer le flux
                </Button>
              )
            ) : null}
            {status.kind === "idle" || isBusy ? (
              <Button variant="quiet" onClick={onAbandon}>
                Abandonner
              </Button>
            ) : null}
          </div>
        </>
      ) : null}

      {status.kind === "fetching" ? (
        <div className="create-from-rss__pending">
          <ProgressIndicator
            mode="indeterminate"
            label="Récupération du flux…"
          />
        </div>
      ) : null}

      {status.kind === "review" ? (
        status.sourceChanged ? (
          <div className="create-from-rss__alert" role="alert">
            <p className="create-from-rss__alert-message">
              La source a changé depuis la récupération.
            </p>
            <p className="create-from-rss__alert-action">
              Relance la récupération du flux.
            </p>
            <div className="create-from-rss__actions">
              <Button variant="quiet" onClick={onAbandon}>
                Abandonner
              </Button>
            </div>
          </div>
        ) : (
          <ReviewPreview
            sourceHost={status.preview.sourceHost}
            channelTitle={status.preview.channelTitle}
            findings={status.preview.findings}
            items={status.preview.items}
            blocked={status.preview.blocked}
            selectedKeys={status.selectedKeys}
            addressDiverged={feedUrl.trim() !== status.feedUrl}
            onToggleItem={onToggleItem}
            onSelectAll={onSelectAll}
            onSelectNone={onSelectNone}
            onAccept={onAccept}
            onAbandon={onAbandon}
          />
        )
      ) : null}

      {status.kind === "creating" ? (
        // The accept downloads every ticked episode: the bar follows the
        // settled episodes (Rust streams the percent) so a long series
        // never looks frozen.
        <div className="create-from-rss__pending">
          {status.progress != null ? (
            <ProgressIndicator
              mode="determinate"
              label={`Création en cours… ${status.progress} %`}
              value={status.progress}
            />
          ) : (
            <ProgressIndicator mode="indeterminate" label="Création en cours…" />
          )}
        </div>
      ) : null}

      {status.kind === "created" ? (
        <div className="create-from-rss__success">
          <StateChip
            tone="success"
            label="Histoire créée dans ta bibliothèque"
          />
          <p className="create-from-rss__success-title">
            {status.story.title}
          </p>
          <Button variant="quiet" onClick={onDismiss}>
            Fermer
          </Button>
        </div>
      ) : null}

      {status.kind === "failed" ? (
        <div className="create-from-rss__alert" role="alert">
          <p className="create-from-rss__alert-message">
            {status.error.message}
          </p>
          {status.error.userAction ? (
            <p className="create-from-rss__alert-action">
              {status.error.userAction}
            </p>
          ) : null}
          <div className="create-from-rss__actions">
            <Button
              variant="secondary"
              onClick={() => onFetch(feedUrl.trim())}
            >
              Réessayer
            </Button>
            <Button variant="quiet" onClick={onDismiss}>
              Fermer
            </Button>
          </div>
        </div>
      ) : null}

      {status.kind === "unavailable" ? (
        // The POLICY refusal (defence in depth): a CALM status region —
        // never `role="alert"`, a distribution decision is not a breakage
        // — with the frozen Rust message + gesture and NO `Réessayer` (a
        // retry cannot change the policy; the way out is `Abandonner`).
        // Never confused with the transport `failed` block above, which
        // keeps the field and the retry. This block is the VISUAL face
        // only: the audible announcement travels through the persistent
        // live region above (this block mounts already filled, which
        // screen readers do not reliably vocalize).
        <div
          className="create-from-rss__unavailable"
          role="status"
          aria-live="polite"
        >
          <p className="create-from-rss__unavailable-message">
            {status.error.message}
          </p>
          {status.error.userAction ? (
            <p className="create-from-rss__unavailable-action">
              {status.error.userAction}
            </p>
          ) : null}
          <div className="create-from-rss__actions">
            <Button variant="quiet" onClick={onAbandon}>
              Abandonner
            </Button>
          </div>
        </div>
      ) : null}
    </section>
  );
}

/** The fetched-feed review. A blocked verdict is a `role="alert"` block
 *  (its findings ARE the verdict + gesture; only `Abandonner` — the field
 *  above stays available to correct and re-fetch); an exploitable one is a
 *  calm `aria-live` region with the host, the feed's name, the flow
 *  findings, the bounded TICK LIST of its episodes (every one ticked by
 *  default — the whole podcast is the story) and the unique `Créer
 *  l'histoire` CTA. */
function ReviewPreview({
  sourceHost,
  channelTitle,
  findings,
  items,
  blocked,
  selectedKeys,
  addressDiverged,
  onToggleItem,
  onSelectAll,
  onSelectNone,
  onAccept,
  onAbandon,
}: {
  sourceHost: string;
  channelTitle: string | null;
  findings: ImportFinding[];
  items: RssPreviewItem[];
  blocked: boolean;
  selectedKeys: ReadonlySet<string>;
  /** The typed address no longer matches the reviewed one: the accept is
   *  refused (it would silently target the OLD source) until a re-fetch
   *  replaces the preview or the address is restored. */
  addressDiverged: boolean;
  onToggleItem: (ref: RssItemRef) => void;
  onSelectAll: () => void;
  onSelectNone: () => void;
  onAccept: () => void;
  onAbandon: () => void;
}): React.JSX.Element {
  const selectedCount = items.filter((item) =>
    selectedKeys.has(rssItemRefKey(item.itemRef)),
  ).length;
  const canAccept = selectedCount > 0 && !addressDiverged;

  return (
    <div
      className="create-from-rss__review"
      role={blocked ? "alert" : undefined}
      aria-live={blocked ? undefined : "polite"}
    >
      <p className="create-from-rss__source-host">{sourceHost}</p>
      {!blocked && channelTitle !== null ? (
        <p className="create-from-rss__channel-title">{channelTitle}</p>
      ) : null}

      <ul className="create-from-rss__findings">
        {findings.map((finding) => (
          <li
            key={`${finding.aspect}-${finding.category}`}
            className="create-from-rss__finding"
          >
            <StateChip
              tone={categoryTone(finding.category)}
              label={categoryLabel(finding.category)}
              className="create-from-rss__finding-chip"
            />
            <span className="create-from-rss__finding-message">
              {finding.message}
            </span>
          </li>
        ))}
      </ul>

      {!blocked ? (
        <>
          <div className="create-from-rss__selection">
            <p className="create-from-rss__selection-count">
              {selectionCountLabel(selectedCount, items.length)}
            </p>
            <div className="create-from-rss__selection-actions">
              <Button
                variant="quiet"
                onClick={onSelectAll}
                disabled={selectedCount === items.length}
              >
                Tout sélectionner
              </Button>
              <Button
                variant="quiet"
                onClick={onSelectNone}
                disabled={selectedCount === 0}
              >
                Tout désélectionner
              </Button>
            </div>
          </div>
          <ul
            className="create-from-rss__items"
            aria-label="Épisodes du flux"
          >
            {items.map((item, index) => {
              const key = rssItemRefKey(item.itemRef);
              const selected = selectedKeys.has(key);
              const name =
                item.title.length > 0 ? item.title : `Épisode ${index + 1}`;
              return (
                <li key={key} className="create-from-rss__item">
                  <label
                    className={[
                      "create-from-rss__item-label",
                      selected ? "create-from-rss__item-label--selected" : null,
                    ]
                      .filter(Boolean)
                      .join(" ")}
                  >
                    <input
                      type="checkbox"
                      className="create-from-rss__item-check"
                      checked={selected}
                      onChange={() => onToggleItem(item.itemRef)}
                      aria-label={name}
                    />
                    <span className="create-from-rss__item-body">
                      <span className="create-from-rss__item-title">{name}</span>
                      {item.summary.length > 0 ? (
                        <span className="create-from-rss__item-summary">
                          {item.summary}
                        </span>
                      ) : null}
                      <span className="create-from-rss__item-media">
                        {item.hasEnclosure ? (
                          <span className="create-from-rss__item-audio">
                            Média audio
                          </span>
                        ) : (
                          <span className="create-from-rss__item-no-audio">
                            Sans média audio
                          </span>
                        )}
                        {item.hasImage ? (
                          <span className="create-from-rss__item-image">
                            Image
                          </span>
                        ) : null}
                      </span>
                    </span>
                  </label>
                </li>
              );
            })}
          </ul>
        </>
      ) : null}

      <div className="create-from-rss__actions">
        {!blocked ? (
          canAccept ? (
            <Button variant="primary" onClick={onAccept}>
              Créer l'histoire
            </Button>
          ) : (
            <Button variant="primary" aria-disabled="true">
              Créer l'histoire
            </Button>
          )
        ) : null}
        <Button variant="quiet" onClick={onAbandon}>
          Abandonner
        </Button>
      </div>
    </div>
  );
}

/** `N épisodes sélectionnés sur M` — singular below two. */
function selectionCountLabel(selected: number, total: number): string {
  const noun = selected > 1 ? "épisodes sélectionnés" : "épisode sélectionné";
  return `${selected} ${noun} sur ${total}`;
}

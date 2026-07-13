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
  /** Select one previewed item. */
  onSelectItem: (ref: RssItemRef) => void;
  /** Commit the selected item (`Créer le brouillon`). */
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
  onSelectItem,
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
  // only the success terminal drops the form.
  const showAddressForm = status.kind !== "created";
  // The failed block owns the retry gesture — the form's own fetch CTA
  // would be a duplicate there.
  const showFetchCta = status.kind !== "failed";

  return (
    <section
      className="create-from-rss"
      aria-label="Création depuis une source externe"
    >
      {/* Polite region mounted while the surface is shown so AT picks up
          the success announcement atomically. */}
      <div
        className="create-from-rss__live"
        aria-live="polite"
        aria-atomic="true"
      >
        {status.kind === "created" ? "Histoire créée dans ta bibliothèque" : ""}
      </div>

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
            findings={status.preview.findings}
            items={status.preview.items}
            blocked={status.preview.blocked}
            selectedItemRef={status.selectedItemRef}
            addressDiverged={feedUrl.trim() !== status.feedUrl}
            onSelectItem={onSelectItem}
            onAccept={onAccept}
            onAbandon={onAbandon}
          />
        )
      ) : null}

      {status.kind === "creating" ? (
        <div className="create-from-rss__pending">
          <ProgressIndicator mode="indeterminate" label="Création en cours…" />
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
    </section>
  );
}

/** The fetched-feed review. A blocked verdict is a `role="alert"` block
 *  (its findings ARE the verdict + gesture; only `Abandonner` — the field
 *  above stays available to correct and re-fetch); an exploitable one is a
 *  calm `aria-live` region with the host, the flow findings, the bounded
 *  selectable item list and the unique `Créer le brouillon` CTA. */
function ReviewPreview({
  sourceHost,
  findings,
  items,
  blocked,
  selectedItemRef,
  addressDiverged,
  onSelectItem,
  onAccept,
  onAbandon,
}: {
  sourceHost: string;
  findings: ImportFinding[];
  items: RssPreviewItem[];
  blocked: boolean;
  selectedItemRef: RssItemRef | null;
  /** The typed address no longer matches the reviewed one: the accept is
   *  refused (it would silently target the OLD source) until a re-fetch
   *  replaces the preview or the address is restored. */
  addressDiverged: boolean;
  onSelectItem: (ref: RssItemRef) => void;
  onAccept: () => void;
  onAbandon: () => void;
}): React.JSX.Element {
  const selectedKey =
    selectedItemRef === null ? null : itemRefKey(selectedItemRef);

  return (
    <div
      className="create-from-rss__review"
      role={blocked ? "alert" : undefined}
      aria-live={blocked ? undefined : "polite"}
    >
      <p className="create-from-rss__source-host">{sourceHost}</p>

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
        <ul className="create-from-rss__items">
          {items.map((item) => {
            const key = itemRefKey(item.itemRef);
            const selected = key === selectedKey;
            return (
              <li key={key} className="create-from-rss__item">
                <button
                  type="button"
                  className={[
                    "create-from-rss__item-button",
                    selected ? "create-from-rss__item-button--selected" : null,
                  ]
                    .filter(Boolean)
                    .join(" ")}
                  aria-pressed={selected}
                  onClick={() => onSelectItem(item.itemRef)}
                >
                  {selected ? (
                    <span
                      className="create-from-rss__item-check"
                      aria-hidden="true"
                    >
                      ✓{" "}
                    </span>
                  ) : null}
                  {item.title.length > 0 ? (
                    <span className="create-from-rss__item-title">
                      {item.title}
                    </span>
                  ) : null}
                  {item.summary.length > 0 ? (
                    <span className="create-from-rss__item-summary">
                      {item.summary}
                    </span>
                  ) : null}
                  {item.hasEnclosure ? (
                    <span className="create-from-rss__item-enclosure">
                      Média distant non récupéré
                    </span>
                  ) : null}
                </button>
              </li>
            );
          })}
        </ul>
      ) : null}

      <div className="create-from-rss__actions">
        {!blocked ? (
          selectedKey !== null && !addressDiverged ? (
            <Button variant="primary" onClick={onAccept}>
              Créer le brouillon
            </Button>
          ) : (
            <Button variant="primary" aria-disabled="true">
              Créer le brouillon
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

/** A stable render key for an item reference — JSON-encoded so no field
 *  separator can collide with (or corrupt) the key content. */
function itemRefKey(ref: RssItemRef): string {
  return ref.kind === "guid"
    ? JSON.stringify(["guid", ref.guid])
    : JSON.stringify(["titleLink", ref.title, ref.link ?? ""]);
}

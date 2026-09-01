import type React from "react";
import { useEffect, useId, useState } from "react";

import {
  Button,
  Field,
  ProgressIndicator,
  StateChip,
} from "../../../shared/ui";
import type { WebPreviewItem } from "../../../shared/ipc-contracts/import-export";
import type { WebCreationStatus } from "../hooks/use-web-creation";

import "./CreateFromWebSurface.css";

export interface CreateFromWebSurfaceProps {
  /** The surface renders NOTHING while closed. Opened by the creation
   *  dialog's web entry; `Abandonner` / `Fermer` close it. */
  open: boolean;
  status: WebCreationStatus;
  /** Fetch the page at the typed address (`Récupérer la page`) — also
   *  the `Réessayer` action after a motivated failure. */
  onFetch: (url: string) => void;
  /** Commit the FULL previewed page (`Créer l'histoire` — every
   *  episode, page order, no single-episode selection). */
  onAccept: () => void;
  /** Abandon the flow (pure frontend, no mutation) and close the surface. */
  onAbandon: () => void;
  /** Dismiss a terminal status (`created` / `failed`) and close the surface. */
  onDismiss: () => void;
}

/**
 * In-context surface for the web external-source creation flow (`Page
 * web`), mirroring the `CreateFromRssSurface` discipline: never a toast
 * for a problem, never a modal, `role="alert"` for a diverged / failed
 * state, `aria-live="polite"` for the success. Renders nothing while
 * closed. The page address lives IN the surface and only its HOST ever
 * renders back from Rust.
 *
 * Unlike the RSS review, the web review is a READ-ONLY confirmation of
 * the FULL page: every extracted episode (title, media audio, optional
 * image) is listed in page order, and the single CTA creates the story
 * with ALL of them (S2: exactly N episodes, page order, each with its
 * own title).
 */
export function CreateFromWebSurface({
  open,
  status,
  onFetch,
  onAccept,
  onAbandon,
  onDismiss,
}: CreateFromWebSurfaceProps): React.JSX.Element | null {
  const addressFieldId = useId();
  const [pageUrl, setPageUrl] = useState<string>("");

  // A closed surface forgets the typed address: a full page URL can
  // carry a private token in its query string — it must never resurface
  // (nor be re-fetchable by mistake) on the next opening.
  useEffect(() => {
    if (!open) {
      setPageUrl("");
    }
  }, [open]);

  if (!open) return null;

  const isBusy = status.kind === "fetching" || status.kind === "creating";
  const canFetch = pageUrl.trim().length > 0 && !isBusy;
  // The field stays visible on a motivated failure too (the gesture is
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
      className="create-from-web"
      aria-label="Création depuis une page web"
    >
      {/* Polite region mounted while the surface is shown so AT picks up
          the terminal announcements atomically: the success chip AND the
          policy refusal route through THIS persistent region (a live
          region inserted into the DOM already filled is not reliably
          announced; only CHANGES of an existing region are). */}
      <div
        className="create-from-web__live"
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
        // the content-rights posture line below. Deliberately NOT
        // rendered on the policy refusal: the mention would contradict
        // it.
        <p className="create-from-web__activation">
          Source activée par la distribution officielle.
        </p>
      ) : null}

      {showAddressForm ? (
        <>
          <p className="create-from-web__posture">
            Utilise uniquement des contenus dont tu as les droits : tes
            contenus personnels ou des contenus libres.
          </p>
          <Field
            id={addressFieldId}
            label="Adresse de la page web"
            value={pageUrl}
            onChange={setPageUrl}
          />
          <div className="create-from-web__actions">
            {showFetchCta ? (
              canFetch ? (
                <Button
                  variant="primary"
                  onClick={() => onFetch(pageUrl.trim())}
                >
                  Récupérer la page
                </Button>
              ) : (
                <Button variant="primary" aria-disabled="true">
                  Récupérer la page
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

      {status.kind === "review" ? (
        status.sourceChanged ? (
          <div className="create-from-web__alert" role="alert">
            <p className="create-from-web__alert-message">
              La page a changé depuis la récupération.
            </p>
            <p className="create-from-web__alert-action">
              Relance la récupération de la page.
            </p>
            <div className="create-from-web__actions">
              <Button variant="quiet" onClick={onAbandon}>
                Abandonner
              </Button>
            </div>
          </div>
        ) : (
          <ReviewPreview
            sourceHost={status.preview.sourceHost}
            items={status.preview.items}
            addressDiverged={pageUrl.trim() !== status.webUrl}
            onAccept={onAccept}
            onAbandon={onAbandon}
          />
        )
      ) : null}

      {status.kind === "fetching" ? (
        <div className="create-from-web__pending">
          <ProgressIndicator
            mode="indeterminate"
            label="Récupération de la page…"
          />
        </div>
      ) : null}

      {status.kind === "creating" ? (
        <div className="create-from-web__pending">
          <ProgressIndicator mode="indeterminate" label="Création en cours…" />
        </div>
      ) : null}

      {status.kind === "created" ? (
        <div className="create-from-web__success">
          <StateChip
            tone="success"
            label="Histoire créée dans ta bibliothèque"
          />
          <p className="create-from-web__success-title">
            {status.story.title}
          </p>
          <Button variant="quiet" onClick={onDismiss}>
            Fermer
          </Button>
        </div>
      ) : null}

      {status.kind === "failed" ? (
        // The motivated refusals (S4 / S5 / S6) land here: the message
        // IS the reason (malformed address, unreachable page, no audio
        // media) — rendered verbatim, never rephrased.
        <div className="create-from-web__alert" role="alert">
          <p className="create-from-web__alert-message">
            {status.error.message}
          </p>
          {status.error.userAction ? (
            <p className="create-from-web__alert-action">
              {status.error.userAction}
            </p>
          ) : null}
          <div className="create-from-web__actions">
            <Button
              variant="secondary"
              onClick={() => onFetch(pageUrl.trim())}
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
        // never `role="alert"`, a distribution decision is not a
        // breakage; the only gesture is `Abandonner` (closing the
        // surface).
        <div className="create-from-web__unavailable">
          <p className="create-from-web__unavailable-message">
            {status.error.message}
          </p>
          {status.error.userAction ? (
            <p className="create-from-web__unavailable-action">
              {status.error.userAction}
            </p>
          ) : null}
          <div className="create-from-web__actions">
            <Button variant="quiet" onClick={onAbandon}>
              Abandonner
            </Button>
          </div>
        </div>
      ) : null}
    </section>
  );
}

/** The fetched-page review: the reviewed host leads, then the FULL
 *  episode list in page order (title, media-audio marker, optional
 *  image marker) and the unique `Créer l'histoire` CTA — the review is
 *  a read-only confirmation, never a picker. */
function ReviewPreview({
  sourceHost,
  items,
  addressDiverged,
  onAccept,
  onAbandon,
}: {
  sourceHost: string;
  items: WebPreviewItem[];
  /** The typed address no longer matches the reviewed one: the accept
   *  is refused (it would silently target the OLD source) until a
   *  re-fetch replaces the preview or the address is restored. */
  addressDiverged: boolean;
  onAccept: () => void;
  onAbandon: () => void;
}): React.JSX.Element {
  return (
    <div className="create-from-web__review" aria-live="polite">
      <p className="create-from-web__source-host">{sourceHost}</p>

      <ul className="create-from-web__items">
        {items.map((item, index) => (
          <li
            key={`${index}-${item.title}`}
            className="create-from-web__item"
          >
            <span className="create-from-web__item-title">
              {item.title}
            </span>
            {item.summary.length > 0 ? (
              <span className="create-from-web__item-summary">
                {item.summary}
              </span>
            ) : null}
            <span className="create-from-web__item-media">
              <span className="create-from-web__item-audio">
                Média audio
              </span>
              {item.imageUrl !== null ? (
                <span className="create-from-web__item-image">Image</span>
              ) : null}
            </span>
          </li>
        ))}
      </ul>

      <div className="create-from-web__actions">
        {!addressDiverged ? (
          <Button variant="primary" onClick={onAccept}>
            Créer l'histoire
          </Button>
        ) : (
          <Button variant="primary" aria-disabled="true">
            Créer l'histoire
          </Button>
        )}
        <Button variant="quiet" onClick={onAbandon}>
          Abandonner
        </Button>
      </div>
    </div>
  );
}

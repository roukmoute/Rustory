import type React from "react";

import type { AppError } from "../../../shared/errors/app-error";
import { Button } from "../../../shared/ui";

import "./LibraryErrorBanner.css";

export interface LibraryErrorBannerProps {
  error: AppError;
  onRetry: () => void;
  title?: string;
}

/**
 * Error surface reused by the library route and the story-edit route. Keeps
 * a non-color signal (the `!` badge) so the alert is legible under grayscale
 * and color-blindness renderings, and exposes a keyboard-reachable retry.
 */
export function LibraryErrorBanner({
  error,
  onRetry,
  title = "Bibliothèque indisponible",
}: LibraryErrorBannerProps): React.JSX.Element {
  // `role="alert"` already implies `aria-live="assertive"` — mixing it with
  // `polite` would produce contradictory behavior on several screen readers.
  return (
    <section className="library-error-banner" role="alert">
      <p className="library-error-banner__badge" aria-hidden="true">
        !
      </p>
      <div className="library-error-banner__body">
        <h1 className="library-error-banner__title">{title}</h1>
        <p className="library-error-banner__message">{error.message}</p>
        {error.userAction ? (
          <p className="library-error-banner__action">{error.userAction}</p>
        ) : null}
        <Button variant="secondary" onClick={onRetry}>
          Réessayer
        </Button>
      </div>
    </section>
  );
}

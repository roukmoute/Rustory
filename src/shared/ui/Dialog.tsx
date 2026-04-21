import type React from "react";
import { useEffect, useId, useRef } from "react";

import "./Dialog.css";

export interface DialogProps {
  open: boolean;
  onClose: () => void;
  title: string;
  ariaDescribedBy?: string;
  children?: React.ReactNode;
}

/**
 * Minimal dialog built on the native `<dialog>` element — the browser
 * provides focus trap, Escape-to-close, and modal semantics for free.
 */
export function Dialog({
  open,
  onClose,
  title,
  ariaDescribedBy,
  children,
}: DialogProps): React.JSX.Element {
  const ref = useRef<HTMLDialogElement | null>(null);
  const titleId = useId();

  useEffect(() => {
    const node = ref.current;
    if (!node) return;
    if (open && !node.open) {
      node.showModal();
    } else if (!open && node.open) {
      node.close();
    }
  }, [open]);

  return (
    <dialog
      ref={ref}
      className="ds-dialog"
      aria-labelledby={titleId}
      aria-describedby={ariaDescribedBy}
      onClose={onClose}
    >
      <h2 id={titleId} className="ds-dialog__title">
        {title}
      </h2>
      <div className="ds-dialog__body">{children}</div>
    </dialog>
  );
}

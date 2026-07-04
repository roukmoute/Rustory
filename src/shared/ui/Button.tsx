import type React from "react";

import "./Button.css";

export type ButtonVariant = "primary" | "secondary" | "quiet" | "destructive";

export interface ButtonProps
  extends Omit<React.ButtonHTMLAttributes<HTMLButtonElement>, "type"> {
  variant?: ButtonVariant;
  type?: "button" | "submit" | "reset";
  /** Ref to the underlying `<button>` — for managed focus flows (mirrors
   *  `Field.inputRef`). */
  buttonRef?: React.Ref<HTMLButtonElement>;
}

/**
 * Primitive button. A primary variant that needs a visible disabled reason
 * should pass `aria-disabled="true"` + `aria-describedby` rather than the
 * native `disabled` attribute — keyboard users must be able to reach the
 * element and read the reason.
 */
export function Button({
  variant = "primary",
  type = "button",
  className,
  children,
  onClick,
  buttonRef,
  ...rest
}: ButtonProps): React.JSX.Element {
  const ariaDisabled = rest["aria-disabled"] === true || rest["aria-disabled"] === "true";
  const handleClick: React.MouseEventHandler<HTMLButtonElement> = (event) => {
    if (ariaDisabled) {
      // Stop the native click from bubbling up to an ancestor handler that
      // would otherwise treat the disabled button as an intentional action.
      event.preventDefault();
      event.stopPropagation();
      return;
    }
    onClick?.(event);
  };

  return (
    <button
      ref={buttonRef}
      type={type}
      className={["ds-button", `ds-button--${variant}`, className]
        .filter(Boolean)
        .join(" ")}
      onClick={handleClick}
      {...rest}
    >
      {children}
    </button>
  );
}

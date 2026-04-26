import type React from "react";

import "./Field.css";

export interface FieldProps {
  id: string;
  label: string;
  type?: React.HTMLInputTypeAttribute;
  value: string;
  onChange: (next: string) => void;
  placeholder?: string;
  "aria-describedby"?: string;
  autoFocus?: boolean;
  onKeyDown?: React.KeyboardEventHandler<HTMLInputElement>;
  inputRef?: React.Ref<HTMLInputElement>;
  /** When true, the input is non-editable. The label, layout and tab
   *  stop are preserved (the input keeps its native `disabled` semantic
   *  rather than `aria-disabled`) so the user knows the control is
   *  intentionally locked rather than missing. */
  disabled?: boolean;
}

/**
 * Labelled input primitive. The label is visible — placeholder-as-label is
 * not accepted. `id` is required so consumers get a stable `htmlFor` target.
 */
export function Field({
  id,
  label,
  type = "text",
  value,
  onChange,
  placeholder,
  autoFocus,
  onKeyDown,
  inputRef,
  disabled,
  ...rest
}: FieldProps): React.JSX.Element {
  return (
    <div className="ds-field">
      <label className="ds-field__label" htmlFor={id}>
        {label}
      </label>
      <input
        id={id}
        ref={inputRef}
        className="ds-field__input"
        type={type}
        value={value}
        placeholder={placeholder}
        autoFocus={autoFocus}
        disabled={disabled}
        onChange={(event) => onChange(event.target.value)}
        onKeyDown={onKeyDown}
        aria-describedby={rest["aria-describedby"]}
      />
    </div>
  );
}

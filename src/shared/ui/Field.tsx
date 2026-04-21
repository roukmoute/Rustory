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
  ...rest
}: FieldProps): React.JSX.Element {
  return (
    <div className="ds-field">
      <label className="ds-field__label" htmlFor={id}>
        {label}
      </label>
      <input
        id={id}
        className="ds-field__input"
        type={type}
        value={value}
        placeholder={placeholder}
        autoFocus={autoFocus}
        onChange={(event) => onChange(event.target.value)}
        aria-describedby={rest["aria-describedby"]}
      />
    </div>
  );
}

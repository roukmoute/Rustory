import { render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { Toast } from "./Toast";

describe("<Toast />", () => {
  beforeEach(() => {
    vi.useFakeTimers();
  });
  afterEach(() => {
    vi.useRealTimers();
  });

  it("renders as a polite status region (never as an alert carrying a critical error alone)", () => {
    render(<Toast tone="success" message="Action confirmée" onDismiss={() => {}} />);
    const region = screen.getByRole("status");
    expect(region).toHaveAttribute("aria-live", "polite");
    expect(region).toHaveTextContent(/action confirmée/i);
  });

  it("auto-dismisses after the configured duration", () => {
    const onDismiss = vi.fn();
    render(
      <Toast
        tone="neutral"
        message="x"
        durationMs={1000}
        onDismiss={onDismiss}
      />,
    );
    expect(onDismiss).not.toHaveBeenCalled();
    vi.advanceTimersByTime(1000);
    expect(onDismiss).toHaveBeenCalledTimes(1);
  });

  // Compile-time invariant documented here: `tone` is typed as
  // Exclude<StateChipTone, "error"> so the TS compiler refuses any attempt
  // to ship an error-only toast. UX-DR15 — critical errors never live in a
  // toast alone; they must appear inline in their context.
  it("TypeScript forbids tone=error at compile time (UX-DR15 guard)", () => {
    // @ts-expect-error — tone="error" is intentionally excluded from the union
    const _invalid = <Toast tone="error" message="x" onDismiss={() => {}} />;
    expect(_invalid).toBeTruthy();
  });
});

import { act, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { DropAnalyzingOverlay } from "./DropAnalyzingOverlay";

describe("<DropAnalyzingOverlay />", () => {
  beforeEach(() => {
    vi.useFakeTimers();
  });
  afterEach(() => {
    vi.useRealTimers();
  });

  it("renders nothing while inactive", () => {
    render(<DropAnalyzingOverlay active={false} />);
    expect(screen.queryByRole("status")).toBeNull();
  });

  it("reveals an announced analysis overlay after the delay while active", () => {
    render(<DropAnalyzingOverlay active={true} />);
    // Not shown immediately — a fast analysis must never flash it.
    expect(screen.queryByRole("status")).toBeNull();
    act(() => {
      vi.advanceTimersByTime(200);
    });
    const overlay = screen.getByRole("status");
    expect(overlay).toBeInTheDocument();
    expect(screen.getByText(/analyse en cours/i)).toBeInTheDocument();
    expect(screen.getByRole("progressbar")).toBeInTheDocument();
  });

  it("never flashes when the analysis settles before the delay", () => {
    const { rerender } = render(<DropAnalyzingOverlay active={true} />);
    act(() => {
      vi.advanceTimersByTime(150);
    });
    rerender(<DropAnalyzingOverlay active={false} />);
    act(() => {
      vi.advanceTimersByTime(200);
    });
    expect(screen.queryByRole("status")).toBeNull();
  });
});

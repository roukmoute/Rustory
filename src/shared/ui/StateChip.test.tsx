import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";

import { StateChip } from "./StateChip";

describe("<StateChip />", () => {
  it("renders the label and an ASCII glyph alongside — color is never the only signal", () => {
    render(<StateChip tone="error" label="Indisponible" />);
    // Non-color channel: glyph is present and aria-hidden so SR reads the label
    // while sighted users in grayscale still see the pictogram.
    const glyph = screen.getByText("×");
    expect(glyph).toHaveAttribute("aria-hidden", "true");
    expect(screen.getByText(/indisponible/i)).toBeInTheDocument();
  });

  it("exposes a distinct glyph per tone so grayscale readers can still differentiate", () => {
    const { rerender } = render(<StateChip tone="success" label="ok" />);
    expect(screen.getByText("✓")).toBeInTheDocument();

    rerender(<StateChip tone="warning" label="à corriger" />);
    expect(screen.getByText("!")).toBeInTheDocument();

    rerender(<StateChip tone="info" label="info" />);
    expect(screen.getByText("i")).toBeInTheDocument();

    rerender(<StateChip tone="neutral" label="neutre" />);
    expect(screen.getByText("•")).toBeInTheDocument();
  });

  it("applies the tone class", () => {
    const { container } = render(
      <StateChip tone="warning" label="à corriger" />,
    );
    expect(container.querySelector(".ds-chip--warning")).not.toBeNull();
  });
});

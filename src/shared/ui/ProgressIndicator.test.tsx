import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";

import { ProgressIndicator } from "./ProgressIndicator";

describe("<ProgressIndicator />", () => {
  it("renders a visible label (not only aria-label) and wires it as the accessible name", () => {
    render(<ProgressIndicator mode="indeterminate" label="Chargement en cours" />);
    expect(screen.getByText(/chargement en cours/i)).toBeInTheDocument();
    // The progressbar must have an accessible name — resolved via
    // aria-labelledby pointing at the visible label node.
    expect(
      screen.getByRole("progressbar", { name: /chargement en cours/i }),
    ).toBeInTheDocument();
  });

  it("exposes role=progressbar with aria-valuemin/max", () => {
    render(<ProgressIndicator mode="indeterminate" label="X" />);
    const bar = screen.getByRole("progressbar");
    expect(bar).toHaveAttribute("aria-valuemin", "0");
    expect(bar).toHaveAttribute("aria-valuemax", "100");
  });

  it("omits aria-valuenow in indeterminate mode (screen readers treat it as busy)", () => {
    render(<ProgressIndicator mode="indeterminate" label="X" />);
    const bar = screen.getByRole("progressbar");
    expect(bar).not.toHaveAttribute("aria-valuenow");
  });

  it("drops a non-finite value back to an unknown determinate (no aria-valuenow='NaN' leak)", () => {
    render(
      <ProgressIndicator
        mode="determinate"
        label="Préparation"
        value={Number.NaN}
      />,
    );
    expect(screen.getByRole("progressbar")).not.toHaveAttribute(
      "aria-valuenow",
    );
  });

  it("clamps determinate values outside [0, 100]", () => {
    const { rerender } = render(
      <ProgressIndicator mode="determinate" label="Préparation" value={-10} />,
    );
    expect(screen.getByRole("progressbar")).toHaveAttribute(
      "aria-valuenow",
      "0",
    );

    rerender(
      <ProgressIndicator mode="determinate" label="Préparation" value={150} />,
    );
    expect(screen.getByRole("progressbar")).toHaveAttribute(
      "aria-valuenow",
      "100",
    );
  });
});

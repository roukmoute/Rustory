import { render, screen } from "@testing-library/react";
import { afterAll, beforeAll, describe, expect, it, vi } from "vitest";

import { Dialog } from "./Dialog";

// happy-dom does not implement HTMLDialogElement.showModal by default —
// spy on the prototype (restored in afterAll) so this file never leaks
// into sibling tests via a permanent prototype patch.
const showModalSpy = vi.spyOn(
  HTMLDialogElement.prototype,
  "showModal" as never,
);
const closeSpy = vi.spyOn(HTMLDialogElement.prototype, "close" as never);

beforeAll(() => {
  showModalSpy.mockImplementation(function (this: HTMLDialogElement) {
    this.setAttribute("open", "");
  });
  closeSpy.mockImplementation(function (this: HTMLDialogElement) {
    this.removeAttribute("open");
  });
});

afterAll(() => {
  showModalSpy.mockRestore();
  closeSpy.mockRestore();
});

describe("<Dialog />", () => {
  it("renders title and children when open", () => {
    render(
      <Dialog open title="Confirmer la suppression" onClose={() => {}}>
        <p>Êtes-vous sûr&nbsp;?</p>
      </Dialog>,
    );
    expect(
      screen.getByRole("heading", { name: /confirmer la suppression/i }),
    ).toBeInTheDocument();
    expect(screen.getByText(/êtes-vous sûr/i)).toBeInTheDocument();
  });

  it("is not open initially when open=false", () => {
    const { container } = render(
      <Dialog open={false} title="X" onClose={() => {}}>
        body
      </Dialog>,
    );
    const el = container.querySelector("dialog") as HTMLDialogElement;
    expect(el.hasAttribute("open")).toBe(false);
  });

  it("wires the title as the dialog's accessible name via aria-labelledby", () => {
    const { container } = render(
      <Dialog open title="Confirmer la suppression" onClose={() => {}}>
        body
      </Dialog>,
    );
    const el = container.querySelector("dialog") as HTMLDialogElement;
    const labelId = el.getAttribute("aria-labelledby");
    expect(labelId).toBeTruthy();
    const titleNode = document.getElementById(labelId as string);
    expect(titleNode).toHaveTextContent(/confirmer la suppression/i);
  });

  it("forwards aria-describedby to the dialog element", () => {
    const { container } = render(
      <Dialog
        open
        title="X"
        ariaDescribedBy="dialog-desc"
        onClose={() => {}}
      >
        body
      </Dialog>,
    );
    const el = container.querySelector("dialog") as HTMLDialogElement;
    expect(el).toHaveAttribute("aria-describedby", "dialog-desc");
  });
});

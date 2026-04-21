import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";

import { Field } from "./Field";

describe("<Field />", () => {
  it("renders a visible label associated with the input via id/htmlFor", () => {
    render(
      <Field
        id="search-library"
        label="Rechercher une histoire"
        value=""
        onChange={() => {}}
      />,
    );
    const input = screen.getByLabelText(/rechercher une histoire/i);
    expect(input).toHaveAttribute("id", "search-library");
  });

  it("calls onChange with the next string value (not the event)", async () => {
    const user = userEvent.setup();
    const onChange = vi.fn();
    render(
      <Field id="q" label="Recherche" value="" onChange={onChange} />,
    );
    await user.type(screen.getByLabelText(/recherche/i), "ab");
    expect(onChange).toHaveBeenCalledTimes(2);
    expect(onChange).toHaveBeenLastCalledWith("b");
  });

  it("forwards aria-describedby to the input", () => {
    render(
      <Field
        id="q"
        label="Recherche"
        value=""
        onChange={() => {}}
        aria-describedby="q-help"
      />,
    );
    expect(screen.getByLabelText(/recherche/i)).toHaveAttribute(
      "aria-describedby",
      "q-help",
    );
  });
});

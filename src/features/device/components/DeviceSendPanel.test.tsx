import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";

import { DeviceSendPanel } from "./DeviceSendPanel";

const PACK_UUID = "abababab-abab-abab-abab-ababfac5562d";

describe("<DeviceSendPanel />", () => {
  it("offers the picker CTA when idle and fires onSend", async () => {
    const user = userEvent.setup();
    const onSend = vi.fn();
    render(
      <DeviceSendPanel
        status={{ kind: "idle" }}
        onSend={onSend}
        onDismissStatus={vi.fn()}
      />,
    );
    await user.click(
      screen.getByRole("button", { name: "Envoyer un pack (.zip)…" }),
    );
    expect(onSend).toHaveBeenCalledTimes(1);
  });

  it("shows an indeterminate busy state and blocks the CTA while sending", async () => {
    const user = userEvent.setup();
    const onSend = vi.fn();
    render(
      <DeviceSendPanel
        status={{ kind: "sending" }}
        onSend={onSend}
        onDismissStatus={vi.fn()}
      />,
    );
    expect(
      screen.getByText("Envoi du pack vers l'appareil…"),
    ).toBeInTheDocument();
    const cta = screen.getByRole("button", {
      name: "Envoyer un pack (.zip)…",
    });
    expect(cta).toHaveAttribute("aria-disabled", "true");
    await user.click(cta);
    expect(onSend).not.toHaveBeenCalled();
  });

  it("announces a settled success with the asset counts and dismisses", async () => {
    const user = userEvent.setup();
    const onDismissStatus = vi.fn();
    render(
      <DeviceSendPanel
        status={{
          kind: "sent",
          packUuid: PACK_UUID,
          imageCount: 117,
          audioCount: 223,
        }}
        onSend={vi.fn()}
        onDismissStatus={onDismissStatus}
      />,
    );
    expect(screen.getByText("Pack envoyé")).toBeInTheDocument();
    expect(
      screen.getByText("Pack envoyé sur l'appareil (117 images, 223 audios)."),
    ).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Fermer" }));
    expect(onDismissStatus).toHaveBeenCalledTimes(1);
  });

  it("renders an actionable failure as an alert and dismisses", async () => {
    const user = userEvent.setup();
    const onDismissStatus = vi.fn();
    render(
      <DeviceSendPanel
        status={{
          kind: "failed",
          error: {
            code: "DEVICE_WRITE_FAILED",
            message: "Envoi impossible: l'appareil a refusé l'écriture.",
            userAction:
              "Vérifie que l'appareil est bien connecté puis réessaie.",
            details: { source: "device_write" },
          },
        }}
        onSend={vi.fn()}
        onDismissStatus={onDismissStatus}
      />,
    );
    const alert = screen.getByRole("alert");
    expect(alert).toHaveTextContent(
      "Envoi impossible: l'appareil a refusé l'écriture.",
    );
    expect(alert).toHaveTextContent(
      "Vérifie que l'appareil est bien connecté puis réessaie.",
    );
    await user.click(screen.getByRole("button", { name: "Fermer" }));
    expect(onDismissStatus).toHaveBeenCalledTimes(1);
  });
});

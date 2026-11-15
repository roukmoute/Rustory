import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

vi.mock("../hooks/use-pack-cover", () => ({ usePackCover: () => null }));

import { DeviceScreen, type DeviceScreenProps } from "./DeviceScreen";
import type { DeviceStoryCollectionProps } from "./DeviceStoryCollection";

function props(overrides: Partial<DeviceScreenProps> = {}): DeviceScreenProps {
  return {
    deviceState: "absent",
    onRetryDetection: vi.fn(),
    onConsultSupportProfile: vi.fn(),
    collectionProps: {
      state: { kind: "idle" },
      isRefreshing: false,
      onRetry: vi.fn(),
    },
    ...overrides,
  };
}

function readyCollection(): DeviceStoryCollectionProps {
  return {
    state: {
      kind: "ready",
      deviceIdentifier: "0123456789abcdef0123456789abcdef",
      stories: [{
        uuid: "story-1",
        shortId: "12345678",
        hidden: false,
        contentPresent: true,
        alreadyImported: false,
        title: "Le voyage de Lila",
        titleSource: "user",
        thumbnail: null,
      }],
    },
    isRefreshing: false,
    onRetry: vi.fn(),
    onSelectStory: vi.fn(),
  };
}

describe("DeviceScreen", () => {
  it("distinguishes an absent device from a readable empty library", () => {
    const initial = props();
    const { rerender } = render(<DeviceScreen {...initial} />);
    expect(screen.getByRole("heading", { level: 1, name: "Sur l'appareil" })).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "Aucun appareil connecté" })).toBeInTheDocument();
    expect(screen.queryByText("Aucune histoire sur l'appareil")).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Réessayer la détection de l'appareil" }));
    expect(initial.onRetryDetection).toHaveBeenCalledOnce();

    rerender(<DeviceScreen {...initial} deviceState="idle" collectionProps={{
      ...initial.collectionProps,
      state: { kind: "ready", deviceIdentifier: "0123456789abcdef0123456789abcdef", stories: [] },
    }} />);
    expect(screen.getByText("Aucune histoire sur l'appareil")).toBeInTheDocument();
    expect(screen.queryByText("Aucun appareil connecté")).not.toBeInTheDocument();
  });

  it("keeps the existing inventory and keyboard selection during detection refresh", () => {
    const collectionProps = readyCollection();
    const initial = props({ deviceState: "idle", deviceLabel: "Lunii V3", collectionProps });
    const { rerender } = render(<DeviceScreen {...initial} />);
    const card = screen.getByRole("button", { name: /Le voyage de Lila/ });
    card.focus();
    rerender(<DeviceScreen {...initial} deviceState="scanning" />);
    expect(card).toHaveFocus();
    expect(screen.getByRole("progressbar", { name: "Détection en cours…" })).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Réessayer la détection de l'appareil" }));
    expect(initial.onRetryDetection).not.toHaveBeenCalled();
    fireEvent.keyDown(card, { key: " " });
    expect(collectionProps.onSelectStory).toHaveBeenCalledWith("story-1", "toggle");
  });

  it.each(["unsupported", "ambiguous", "error"] as const)(
    "preserves the real detection reason and support action for %s",
    (deviceState) => {
      const initial = props({ deviceState, deviceReason: "Raison fournie par la détection." });
      render(<DeviceScreen {...initial} />);
      expect(screen.getByText(initial.deviceReason!)).toBeInTheDocument();
      fireEvent.click(screen.getByRole("button", { name: "Consulter le profil de support" }));
      expect(initial.onConsultSupportProfile).toHaveBeenCalledOnce();
      expect(screen.queryByText("Aucune histoire sur l'appareil")).not.toBeInTheDocument();
    },
  );

  it("does not invent an empty inventory for a connected but unreadable device", () => {
    render(<DeviceScreen {...props({ deviceState: "idle" })} />);
    expect(screen.getByRole("heading", { name: "Bibliothèque de l'appareil indisponible" })).toBeInTheDocument();
    expect(screen.queryByText("Aucune histoire sur l'appareil")).not.toBeInTheDocument();
  });

  it("delegates inventory failures to the existing read retry", () => {
    const initial = props({
      deviceState: "idle",
      collectionProps: {
        state: { kind: "error", error: { code: "DEVICE_SCAN_FAILED", message: "Lecture interrompue.", userAction: "Relance la lecture.", details: null } },
        isRefreshing: false,
        onRetry: vi.fn(),
      },
    });
    render(<DeviceScreen {...initial} />);
    fireEvent.click(screen.getByRole("button", { name: "Réessayer" }));
    expect(initial.collectionProps.onRetry).toHaveBeenCalledOnce();
    expect(initial.onRetryDetection).not.toHaveBeenCalled();
  });
});

import { fireEvent, render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("../../../ipc/commands/presentation", () => ({
  readAnnouncementVoices: vi.fn(),
  setAnnouncementVoice: vi.fn(),
  previewAnnouncementVoice: vi.fn(),
  installEmbeddedVoice: vi.fn(),
}));

import { readAnnouncementVoices, setAnnouncementVoice } from "../../../ipc/commands/presentation";
import type { DeviceStoryDto } from "../../../shared/ipc-contracts/device-library";
import { DeviceDiscoveryScreen, type DeviceDiscoveryScreenProps } from "./DeviceDiscoveryScreen";
import { WelcomeScreen, type WelcomeScreenProps } from "./WelcomeScreen";

const VOICE = { id: "system:actual", name: "Voix réellement installée", language: "fr-FR", engine: "system" as const };
const VOICES = {
  voices: [VOICE],
  selectedVoiceId: VOICE.id,
  selectedIsStored: true,
  embedded: { state: "notInstalled" as const, downloadBytes: 89_667_631, voiceId: "embedded:actual", voiceName: "Voix neuronale" },
};

function welcomeProps(overrides: Partial<WelcomeScreenProps> = {}): WelcomeScreenProps {
  return { deviceState: "absent", onContinue: vi.fn(), onRetryDetection: vi.fn(), ...overrides };
}

function discoveryProps(overrides: Partial<DeviceDiscoveryScreenProps> = {}): DeviceDiscoveryScreenProps {
  return { state: { kind: "loading" }, onRetry: vi.fn(), onRecover: vi.fn(), onContinue: vi.fn(), ...overrides };
}

function story(overrides: Partial<DeviceStoryDto> = {}): DeviceStoryDto {
  return { uuid: "one", shortId: "12345678", hidden: false, contentPresent: true, alreadyImported: false, title: null, titleSource: null, thumbnail: null, ...overrides };
}

describe("WelcomeScreen", () => {
  beforeEach(() => {
    vi.mocked(readAnnouncementVoices).mockReset().mockResolvedValue(VOICES);
    vi.mocked(setAnnouncementVoice).mockReset().mockResolvedValue(VOICES);
  });

  it("uses real voices and download size while allowing continuation without a device", async () => {
    const props = welcomeProps();
    render(<WelcomeScreen {...props} />);
    expect(screen.getByRole("heading", { level: 1, name: "Bienvenue sur Rustory" })).toBeInTheDocument();
    expect(await screen.findByRole("radio", { name: /Voix réellement installée/ })).toBeChecked();
    expect(screen.getByRole("button", { name: "Télécharger · 90 Mo" })).toBeInTheDocument();
    expect(screen.queryByText("Hortense")).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Continuer sans Lunii" }));
    expect(props.onContinue).toHaveBeenCalledOnce();
    fireEvent.click(screen.getByRole("button", { name: "Réessayer la détection" }));
    expect(props.onRetryDetection).toHaveBeenCalledOnce();
    expect(screen.queryByRole("navigation")).not.toBeInTheDocument();
  });

  it("reflects a connection and guards continuation while persistence is pending", async () => {
    const props = welcomeProps({ deviceState: "idle", deviceLabel: "Lunii V3", busy: true });
    const { rerender } = render(<WelcomeScreen {...props} />);
    await screen.findByRole("radio");
    expect(screen.getByText("Ta Lunii est reconnue")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Continuer" }));
    expect(props.onContinue).not.toHaveBeenCalled();
    rerender(<WelcomeScreen {...props} busy={false} />);
    fireEvent.click(screen.getByRole("button", { name: "Continuer" }));
    expect(props.onContinue).toHaveBeenCalledOnce();
  });

  it("does not offer another detection during scanning and preserves an actual failure", async () => {
    const props = welcomeProps({ deviceState: "scanning" });
    const { rerender } = render(<WelcomeScreen {...props} />);
    await screen.findByRole("radio");
    fireEvent.click(screen.getByRole("button", { name: "Réessayer la détection" }));
    expect(props.onRetryDetection).not.toHaveBeenCalled();
    rerender(<WelcomeScreen {...props} deviceState="error" deviceReason="Le périphérique ne répond pas." />);
    expect(screen.getByText("Le périphérique ne répond pas.")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Réessayer la détection" }));
    expect(props.onRetryDetection).toHaveBeenCalledOnce();
  });

  it("lets a failed welcome preference read leave for the library without completing the guide", async () => {
    const props = welcomeProps({ onOpenLibrary: vi.fn(), error: { code: "LOCAL_STORAGE_UNAVAILABLE", message: "Préférences indisponibles.", userAction: null, details: null } });
    const { rerender } = render(<WelcomeScreen {...props} />);
    await screen.findByRole("radio");
    fireEvent.click(screen.getByRole("button", { name: "Ouvrir ma bibliothèque" }));
    expect(props.onOpenLibrary).toHaveBeenCalledOnce();
    expect(props.onContinue).not.toHaveBeenCalled();
    rerender(<WelcomeScreen {...props} error={null} />);
    expect(screen.queryByRole("button", { name: "Ouvrir ma bibliothèque" })).not.toBeInTheDocument();
  });
});

describe("DeviceDiscoveryScreen", () => {
  it("shows real read progress without a fabricated percentage, even before a capability refusal", () => {
    render(<DeviceDiscoveryScreen {...discoveryProps({ importUnavailableReason: "Copie non disponible pour ce profil." })} />);
    expect(screen.getByRole("progressbar")).not.toHaveAttribute("aria-valuenow");
    expect(screen.getByRole("heading", { level: 1 })).toHaveTextContent("Je regarde ce qu'il y a sur ta Lunii");
    expect(screen.getByText("Rien n'est modifié sur ta Lunii.")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Récupérer mes histoires" })).not.toBeInTheDocument();
  });

  it("counts all found stories and names the recoverable subset honestly", () => {
    const props = discoveryProps({ state: { kind: "ready", deviceIdentifier: "device", stories: [story(), story({ uuid: "two", alreadyImported: true }), story({ uuid: "three", contentPresent: false })] } });
    render(<DeviceDiscoveryScreen {...props} />);
    expect(screen.getByRole("heading", { level: 1 })).toHaveTextContent("3 histoires trouvées sur ta Lunii");
    expect(screen.getByText(/1 histoire à récupérer/)).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Récupérer mes histoires" }));
    expect(props.onRecover).toHaveBeenCalledOnce();
    fireEvent.click(screen.getByRole("button", { name: "Pas maintenant" }));
    expect(props.onContinue).toHaveBeenCalledOnce();
  });

  it("retains a V3 inventory count but never offers an unavailable import", () => {
    const props = discoveryProps({ state: { kind: "ready", deviceIdentifier: "device", stories: [story()] }, importUnavailableReason: "La copie depuis cette Lunii V3 n'est pas disponible." });
    render(<DeviceDiscoveryScreen {...props} />);
    expect(screen.getByRole("heading", { level: 1 })).toHaveTextContent("1 histoire trouvée sur ta Lunii");
    expect(screen.getByText(props.importUnavailableReason!)).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Récupérer mes histoires" })).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Continuer vers ma bibliothèque" }));
    expect(props.onContinue).toHaveBeenCalledOnce();
  });

  it.each([true, false])("does not offer an empty recovery when alreadyImported is %s", (alreadyImported) => {
    const props = discoveryProps({ state: { kind: "ready", deviceIdentifier: "device", stories: [story({ alreadyImported, contentPresent: alreadyImported })] } });
    render(<DeviceDiscoveryScreen {...props} />);
    expect(screen.queryByRole("button", { name: "Récupérer mes histoires" })).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Continuer vers ma bibliothèque" })).toBeInTheDocument();
  });

  it("distinguishes an actual empty inventory from a failed read", () => {
    const props = discoveryProps({ state: { kind: "ready", deviceIdentifier: "device", stories: [] } });
    const { rerender } = render(<DeviceDiscoveryScreen {...props} />);
    expect(screen.getByRole("heading", { level: 1 })).toHaveTextContent("Ta Lunii ne contient aucune histoire");
    fireEvent.click(screen.getByRole("button", { name: "Continuer" }));
    expect(props.onContinue).toHaveBeenCalledOnce();
    rerender(<DeviceDiscoveryScreen {...props} state={{ kind: "error", error: { code: "DEVICE_SCAN_FAILED", message: "Lecture interrompue.", userAction: "Réessaie la lecture.", details: null } }} />);
    expect(screen.getByRole("alert")).toHaveTextContent("Lecture interrompue. Réessaie la lecture.");
    expect(screen.queryByText("Ta Lunii ne contient aucune histoire")).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Réessayer" }));
    expect(props.onRetry).toHaveBeenCalledOnce();
  });

  it("guards recovery and skip while the route saves the choice", () => {
    const props = discoveryProps({ state: { kind: "ready", deviceIdentifier: "device", stories: [story()] }, busy: true });
    render(<DeviceDiscoveryScreen {...props} />);
    fireEvent.click(screen.getByRole("button", { name: "Récupérer mes histoires" }));
    fireEvent.click(screen.getByRole("button", { name: "Pas maintenant" }));
    expect(props.onRecover).not.toHaveBeenCalled();
    expect(props.onContinue).not.toHaveBeenCalled();
  });

  it("uses the connected family and lets preferences retry through the route", () => {
    const props = discoveryProps({ deviceFamily: "flam", onRetryPreferences: vi.fn(), error: { code: "LOCAL_STORAGE_UNAVAILABLE", message: "Préférences indisponibles.", userAction: null, details: null } });
    render(<DeviceDiscoveryScreen {...props} />);
    expect(screen.getByRole("heading", { level: 1 })).toHaveTextContent("Je regarde ce qu'il y a sur ton FLAM");
    expect(screen.getByText("Rien n'est modifié sur ton FLAM.")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Réessayer l'accueil" }));
    expect(props.onRetryPreferences).toHaveBeenCalledOnce();
  });

  it("lets a failed discovery preference save leave without dismissing or recovering stories", () => {
    const props = discoveryProps({ state: { kind: "ready", deviceIdentifier: "device", stories: [story()] }, onOpenLibrary: vi.fn(), error: { code: "LOCAL_STORAGE_UNAVAILABLE", message: "Préférences non enregistrées.", userAction: null, details: null } });
    const { rerender } = render(<DeviceDiscoveryScreen {...props} />);
    fireEvent.click(screen.getByRole("button", { name: "Ouvrir ma bibliothèque" }));
    expect(props.onOpenLibrary).toHaveBeenCalledOnce();
    expect(props.onContinue).not.toHaveBeenCalled();
    expect(props.onRecover).not.toHaveBeenCalled();
    rerender(<DeviceDiscoveryScreen {...props} error={null} />);
    expect(screen.queryByRole("button", { name: "Ouvrir ma bibliothèque" })).not.toBeInTheDocument();
  });
});

import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";

import { DeviceBulkImportPanel } from "./DeviceBulkImportPanel";
import type { DeviceStoryDto } from "../../../shared/ipc-contracts/device-library";
import type { DeviceBulkImportStatus } from "../hooks/use-device-bulk-import";

function makeStory(overrides: Partial<DeviceStoryDto> = {}): DeviceStoryDto {
  return {
    uuid: "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa",
    shortId: "0000ABCD",
    hidden: false,
    contentPresent: true,
    alreadyImported: false,
    title: null,
    titleSource: null,
    thumbnail: null,
    ...overrides,
  };
}

const IDLE: DeviceBulkImportStatus = { kind: "idle" };

function renderPanel(overrides?: {
  stories?: DeviceStoryDto[];
  canImport?: boolean;
  status?: DeviceBulkImportStatus;
  onImport?: (uuids: string[]) => void;
  onClearSelection?: () => void;
  onDismissStatus?: () => void;
}) {
  const onImport = overrides?.onImport ?? vi.fn();
  const onClearSelection = overrides?.onClearSelection ?? vi.fn();
  const onDismissStatus = overrides?.onDismissStatus ?? vi.fn();
  render(
    <DeviceBulkImportPanel
      stories={
        overrides?.stories ?? [
          makeStory({ uuid: "u1", shortId: "0000AAAA" }),
          makeStory({ uuid: "u2", shortId: "0000BBBB" }),
        ]
      }
      canImport={overrides?.canImport ?? true}
      status={overrides?.status ?? IDLE}
      onImport={onImport}
      onClearSelection={onClearSelection}
      onDismissStatus={onDismissStatus}
    />,
  );
  return { onImport, onClearSelection, onDismissStatus };
}

describe("<DeviceBulkImportPanel />", () => {
  it("names how many are selected", () => {
    renderPanel();
    expect(
      screen.getByRole("region", { name: /2 histoires sélectionnées/i }),
    ).toBeInTheDocument();
  });

  it("imports only the importable subset, excluding blocked packs", async () => {
    const user = userEvent.setup();
    const { onImport } = renderPanel({
      stories: [
        makeStory({ uuid: "u1" }),
        makeStory({ uuid: "u2", alreadyImported: true }),
        makeStory({ uuid: "u3", contentPresent: false }),
      ],
    });
    // Triage counts add up: 1 importable, 1 already there, 1 incomplete.
    expect(screen.getByText(/1 importable/i)).toBeInTheDocument();
    expect(screen.getByText(/1 déjà dans ta bibliothèque/i)).toBeInTheDocument();
    expect(screen.getByText(/1 au contenu incomplet/i)).toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: /importer/i }));
    expect(onImport).toHaveBeenCalledWith(["u1"]);
  });

  it("disables the action when nothing is importable", () => {
    renderPanel({
      stories: [
        makeStory({ uuid: "u1", alreadyImported: true }),
        makeStory({ uuid: "u2", contentPresent: false }),
      ],
    });
    expect(screen.getByRole("button", { name: /importer/i })).toBeDisabled();
    expect(
      screen.getByText(/aucune des histoires sélectionnées n'est importable/i),
    ).toBeInTheDocument();
  });

  it("explains a profile that blocks import and disables the action", () => {
    renderPanel({ canImport: false });
    expect(
      screen.getByText(/l'import n'est pas disponible pour le profil/i),
    ).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /importer/i })).toBeDisabled();
  });

  it("shows determinate progress while a batch runs and hides the actions", () => {
    renderPanel({
      status: { kind: "running", total: 4, done: 1, succeeded: 1, failed: 0 },
    });
    const bar = screen.getByRole("progressbar", { name: /import en cours/i });
    expect(bar).toHaveAttribute("aria-valuenow", "25");
    expect(
      screen.queryByRole("button", { name: /importer/i }),
    ).not.toBeInTheDocument();
  });

  it("summarizes a finished batch and dismisses it", async () => {
    const user = userEvent.setup();
    const { onDismissStatus } = renderPanel({
      status: {
        kind: "done",
        total: 3,
        succeeded: 3,
        failed: 0,
        firstError: null,
      },
    });
    expect(screen.getByText(/3 importées/i)).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: /fermer/i }));
    expect(onDismissStatus).toHaveBeenCalledTimes(1);
  });

  it("surfaces a partial-failure tally with the first cause", () => {
    renderPanel({
      status: {
        kind: "done",
        total: 3,
        succeeded: 2,
        failed: 1,
        firstError: {
          code: "IMPORT_FAILED",
          message: "Copie impossible: lecture interrompue.",
          userAction: "Réessaie.",
          details: null,
        },
      },
    });
    expect(screen.getByText(/2 importées, 1 en échec/i)).toBeInTheDocument();
    expect(screen.getByText(/lecture interrompue/i)).toBeInTheDocument();
  });

  it("clears the selection on demand", async () => {
    const user = userEvent.setup();
    const { onClearSelection } = renderPanel();
    await user.click(
      screen.getByRole("button", { name: /effacer la sélection/i }),
    );
    expect(onClearSelection).toHaveBeenCalledTimes(1);
  });
});

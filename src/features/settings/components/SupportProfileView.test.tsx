import { render, screen, within } from "@testing-library/react";
import { describe, expect, it } from "vitest";

import type { ContentSourcePolicy } from "../../../shared/ipc-contracts/import-export";
import type { SupportProfile } from "../../../shared/ipc-contracts/settings";
import type { SectionRead } from "./SupportProfileView";
import { SupportProfileView } from "./SupportProfileView";

/** The EXACT official profile Rust serializes (mirror of the contract
 *  test `the_official_device_matrix_serializes_exactly`). */
function officialProfile(): SupportProfile {
  return {
    devices: [
      {
        family: "lunii",
        familyLabel: "Lunii",
        cohort: "origineV1",
        cohortLabel: "Origine v1",
        metadataFormatLabel: "Format métadonnées v3",
        capabilities: [
          {
            operation: "readLibrary",
            label: "Lecture bibliothèque appareil",
            available: true,
          },
          {
            operation: "inspectStory",
            label: "Inspection d'histoire",
            available: true,
          },
          {
            operation: "importStory",
            label: "Copie dans la bibliothèque locale",
            available: true,
          },
          {
            operation: "writeStory",
            label: "Transfert vers la Lunii",
            available: true,
          },
        ],
      },
      {
        family: "lunii",
        familyLabel: "Lunii",
        cohort: "midGenV2",
        cohortLabel: "Mid-Gen v2",
        metadataFormatLabel: "Format métadonnées v6",
        capabilities: [
          {
            operation: "readLibrary",
            label: "Lecture bibliothèque appareil",
            available: true,
          },
          {
            operation: "inspectStory",
            label: "Inspection d'histoire",
            available: true,
          },
          {
            operation: "importStory",
            label: "Copie dans la bibliothèque locale",
            available: true,
          },
          {
            operation: "writeStory",
            label: "Transfert vers la Lunii",
            available: true,
          },
        ],
      },
      {
        family: "lunii",
        familyLabel: "Lunii",
        cohort: "v3",
        cohortLabel: "V3",
        metadataFormatLabel: "Format métadonnées v7",
        capabilities: [
          {
            operation: "readLibrary",
            label: "Lecture bibliothèque appareil",
            available: true,
          },
          {
            operation: "inspectStory",
            label: "Inspection d'histoire",
            available: true,
          },
          {
            operation: "importStory",
            label: "Copie dans la bibliothèque locale",
            available: false,
            reason: "Rétro-ingénierie du format en cours",
          },
          {
            operation: "writeStory",
            label: "Transfert vers la Lunii",
            available: false,
            reason: "Rétro-ingénierie du format en cours",
          },
        ],
      },
      {
        family: "flam",
        familyLabel: "FLAM",
        cohort: "flamGen1",
        cohortLabel: "Gen1",
        capabilities: [
          {
            operation: "readLibrary",
            label: "Lecture bibliothèque appareil",
            available: true,
          },
          {
            operation: "inspectStory",
            label: "Inspection d'histoire",
            available: true,
          },
          {
            operation: "importStory",
            label: "Copie dans la bibliothèque locale",
            available: true,
          },
          {
            operation: "writeStory",
            label: "Transfert vers l'appareil",
            available: false,
            reason: "Écriture non prouvée sur matériel réel",
          },
        ],
      },
    ],
    localArtifacts: [
      {
        kind: "rustoryArtifact",
        label: "Artefact d'histoire Rustory (.rustory)",
        formatLabel: "Format v1",
        available: true,
        capabilitiesLabel: "Import et export",
      },
      {
        kind: "structuredFolder",
        label: "Dossier structuré",
        formatLabel: "Format v1",
        available: true,
        capabilitiesLabel: "Création d'une histoire",
      },
      {
        kind: "structuredArchive",
        label: "Archive structurée",
        available: false,
        reason: "Lecture d'archives non prise en charge",
      },
    ],
    fileAssociation: {
      extensionLabel: ".rustory",
      channels: [
        {
          channel: "linuxSystemPackage",
          label: "Paquet Linux (.deb / .rpm)",
          registered: true,
          statusLabel: "Enregistrée à l'installation",
          detail:
            "L'association est déclarée par le paquet et active dès l'installation.",
        },
        {
          channel: "linuxAppImage",
          label: "AppImage (Linux)",
          registered: false,
          statusLabel: "Non enregistrée d'office",
          detail:
            "Une AppImage ne modifie pas ton système : rien n'est enregistré automatiquement.",
          reason:
            "Tu peux ajouter l'association avec un outil d'intégration AppImage ou une entrée d'application manuelle.",
        },
        {
          channel: "windowsInstaller",
          label: "Installeur Windows (.msi / .exe)",
          registered: true,
          statusLabel: "Enregistrée à l'installation",
          detail:
            "L'installeur déclare l'association. Windows peut te demander de confirmer et respecte ton choix existant.",
        },
        {
          channel: "macosAppBundle",
          label: "Application macOS (.dmg)",
          registered: true,
          statusLabel: "Enregistrée par le système",
          detail:
            "macOS enregistre l'association quand l'application est déposée dans Applications.",
        },
      ],
    },
  };
}

/** The official profile with the Linux install probe's AppImage
 *  verdict attached (the only case where the notice renders). */
function profileWithCurrentInstall(): SupportProfile {
  const profile = officialProfile();
  return {
    ...profile,
    fileAssociation: {
      ...profile.fileAssociation,
      currentInstall: {
        kind: "appImage",
        notice:
          "Ton installation actuelle est une AppImage : l'association n'est pas enregistrée d'office.",
      },
    },
  };
}

function officialPolicy(): ContentSourcePolicy {
  return {
    sources: [
      {
        kind: "rss",
        label: "Flux RSS",
        activation: "enabled",
        activationMarker: "Activée par la distribution officielle",
      },
      {
        kind: "atom",
        label: "Flux Atom",
        activation: "notActivated",
        reason:
          "Source indisponible: non activée dans la distribution officielle",
      },
      {
        kind: "jsonFeed",
        label: "Flux JSON Feed",
        activation: "notActivated",
        reason:
          "Source indisponible: non activée dans la distribution officielle",
      },
    ],
  };
}

function loaded<T>(data: T): SectionRead<T> {
  return { kind: "loaded", data };
}

function renderView(
  profileRead: SectionRead<SupportProfile> = loaded(officialProfile()),
  policyRead: SectionRead<ContentSourcePolicy> = loaded(officialPolicy()),
) {
  return render(
    <SupportProfileView profileRead={profileRead} policyRead={policyRead} />,
  );
}

describe("<SupportProfileView />", () => {
  it("renders the five sections as h2 headings in the contract order", () => {
    renderView();
    const headings = screen
      .getAllByRole("heading", { level: 2 })
      .map((h) => h.textContent);
    expect(headings).toEqual([
      "Appareils",
      "Artefacts locaux",
      "Association de fichiers",
      "Sources de contenu",
      "Politique de distribution",
    ]);
  });

  it("renders the full device matrix grouped by family (4 cohort lines)", () => {
    renderView();
    const familyHeadings = screen
      .getAllByRole("heading", { level: 3 })
      .map((h) => h.textContent);
    expect(familyHeadings).toEqual(["Lunii", "FLAM"]);
    for (const cohort of ["Origine v1", "Mid-Gen v2", "V3", "Gen1"]) {
      expect(screen.getByText(cohort)).toBeInTheDocument();
    }
    // The frozen metadata-format lines render verbatim; FLAM has none.
    expect(screen.getByText("Format métadonnées v3")).toBeInTheDocument();
    expect(screen.getByText("Format métadonnées v6")).toBeInTheDocument();
    expect(screen.getByText("Format métadonnées v7")).toBeInTheDocument();
    // 4 lines × 4 capabilities render the Rust-carried labels verbatim.
    expect(screen.getAllByText("Lecture bibliothèque appareil")).toHaveLength(
      4,
    );
    expect(screen.getAllByText("Inspection d'histoire")).toHaveLength(4);
    expect(
      screen.getAllByText("Copie dans la bibliothèque locale"),
    ).toHaveLength(4);
    expect(screen.getAllByText("Transfert vers la Lunii")).toHaveLength(3);
    expect(screen.getAllByText("Transfert vers l'appareil")).toHaveLength(1);
  });

  it("renders a non-available capability as a calm neutral chip plus its frozen reason — never a bare ✗", () => {
    renderView();
    // 13 available capabilities, 3 non-available ones (V3 ×2, FLAM ×1).
    expect(screen.getAllByText("Disponible")).toHaveLength(13 + 2); // +2 artifacts
    expect(
      screen.getAllByText("Non disponible dans cette version"),
    ).toHaveLength(3 + 1); // +1 deferred artifact
    expect(
      screen.getAllByText("Rétro-ingénierie du format en cours"),
    ).toHaveLength(2);
    expect(
      screen.getByText("Écriture non prouvée sur matériel réel"),
    ).toBeInTheDocument();
    // The fourth vocabulary stays calm: neutral chips, never error /
    // warning tones, never an alert region.
    const chips = document.querySelectorAll(".ds-chip");
    expect(chips.length).toBeGreaterThan(0);
    expect(document.querySelectorAll(".ds-chip--error")).toHaveLength(0);
    expect(document.querySelectorAll(".ds-chip--warning")).toHaveLength(0);
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
  });

  it("locks the tone couples POSITIVELY: a closed cell renders a neutral chip, an open cell a success chip", () => {
    // AC2's colour contract, asserted on precise lines (not global
    // counts): a regression swapping the two tones (a limit rendered
    // green, an availability rendered grey) must fail here.
    renderView();
    // The V3 import line: closed → the chip inside THIS list item is
    // `neutral` and carries the not-available label.
    const v3ImportReason = screen.getAllByText(
      "Rétro-ingénierie du format en cours",
    )[0];
    const closedItem = v3ImportReason.closest("li") as HTMLElement;
    const closedChip = closedItem.querySelector(".ds-chip");
    expect(closedChip).not.toBeNull();
    expect(closedChip).toHaveClass("ds-chip--neutral");
    expect(closedChip).toHaveTextContent("Non disponible dans cette version");
    expect(closedItem.querySelector(".ds-chip--success")).toBeNull();
    // An open line (the FLAM import cell — its label is unique to one
    // capability per line): the chip inside THIS list item is
    // `success` and carries the available label.
    const flamImportLabel = screen.getAllByText(
      "Copie dans la bibliothèque locale",
    )[3]; // wire order: origineV1, midGenV2, v3, flamGen1
    const openItem = flamImportLabel.closest("li") as HTMLElement;
    const openChip = openItem.querySelector(".ds-chip");
    expect(openChip).not.toBeNull();
    expect(openChip).toHaveClass("ds-chip--success");
    expect(openChip).toHaveTextContent("Disponible");
    expect(openItem.querySelector(".ds-chip--neutral")).toBeNull();
    // The deferred artifact line follows the same couple.
    const archiveReason = screen.getByText(
      "Lecture d'archives non prise en charge",
    );
    const archiveItem = archiveReason.closest("li") as HTMLElement;
    expect(archiveItem.querySelector(".ds-chip")).toHaveClass(
      "ds-chip--neutral",
    );
  });

  it("renders the three artifact registry lines plus the node-media formats line verbatim", () => {
    renderView();
    expect(
      screen.getByText("Artefact d'histoire Rustory (.rustory)"),
    ).toBeInTheDocument();
    expect(screen.getByText("Dossier structuré")).toBeInTheDocument();
    expect(screen.getByText("Archive structurée")).toBeInTheDocument();
    expect(screen.getAllByText("Format v1")).toHaveLength(2);
    expect(screen.getByText("Import et export")).toBeInTheDocument();
    expect(screen.getByText("Création d'une histoire")).toBeInTheDocument();
    expect(
      screen.getByText("Lecture d'archives non prise en charge"),
    ).toBeInTheDocument();
    expect(
      screen.getByText(
        "Formats acceptés : images PNG, JPEG ; sons MP3, WAV, OGG",
      ),
    ).toBeInTheDocument();
  });

  it("renders the content sources from the reused policy — marker on rss, Rust reasons verbatim on the others", () => {
    renderView();
    expect(screen.getByText("Flux RSS")).toBeInTheDocument();
    expect(screen.getByText("Flux Atom")).toBeInTheDocument();
    expect(screen.getByText("Flux JSON Feed")).toBeInTheDocument();
    expect(
      screen.getByText("Activée par la distribution officielle"),
    ).toBeInTheDocument();
    expect(
      screen.getAllByText(
        "Source indisponible: non activée dans la distribution officielle",
      ),
    ).toHaveLength(2);
  });

  it("renders the frozen distribution posture", () => {
    renderView();
    expect(
      screen.getByText(
        "La distribution officielle autorise par défaut les histoires créées dans Rustory, tes contenus personnels et les contenus explicitement libres. Elle n'active jamais de flux orientés vers des contenus protégés non autorisés et n'intègre aucun contournement de protections techniques.",
      ),
    ).toBeInTheDocument();
  });

  it("fails closed per section: a failed profile read never takes down the sources or the posture", () => {
    renderView({ kind: "unavailable" }, loaded(officialPolicy()));
    // Devices + artifacts + file association render the calm honest
    // copy, role="status".
    const statuses = screen.getAllByRole("status");
    expect(statuses).toHaveLength(3);
    for (const status of statuses) {
      expect(status).toHaveTextContent(
        "Le profil de support n'a pas pu être lu.",
      );
    }
    // No retry gesture: a failed pure read is a contract drift.
    expect(
      screen.queryByRole("button", { name: /réessayer/i }),
    ).not.toBeInTheDocument();
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
    // The sources section stays fully served.
    expect(screen.getByText("Flux RSS")).toBeInTheDocument();
    expect(
      screen.getByText("Activée par la distribution officielle"),
    ).toBeInTheDocument();
    // The static posture always renders.
    expect(
      screen.getByText(/La distribution officielle autorise/),
    ).toBeInTheDocument();
  });

  it("fails closed per section: a failed policy read never takes down the profile sections", () => {
    renderView(loaded(officialProfile()), { kind: "unavailable" });
    const status = screen.getByRole("status");
    expect(status).toHaveTextContent(
      "Les sources de contenu n'ont pas pu être lues.",
    );
    expect(screen.queryByText("Flux RSS")).not.toBeInTheDocument();
    // Devices and artifacts stay fully served.
    expect(screen.getByText("Origine v1")).toBeInTheDocument();
    expect(screen.getByText("Dossier structuré")).toBeInTheDocument();
  });

  it("marks loading sections with aria-busy and renders no invented content", () => {
    const { container } = renderView({ kind: "loading" }, { kind: "loading" });
    const busySections = container.querySelectorAll('[aria-busy="true"]');
    expect(busySections).toHaveLength(4);
    expect(screen.queryByText("Origine v1")).not.toBeInTheDocument();
    expect(screen.queryByText("Flux RSS")).not.toBeInTheDocument();
    // The static posture still renders (it depends on no read).
    expect(
      screen.getByText(/La distribution officielle autorise/),
    ).toBeInTheDocument();
  });

  it("renders the four file-association channel lines verbatim with the extension label", () => {
    renderView();
    expect(screen.getByText(".rustory")).toBeInTheDocument();
    for (const label of [
      "Paquet Linux (.deb / .rpm)",
      "AppImage (Linux)",
      "Installeur Windows (.msi / .exe)",
      "Application macOS (.dmg)",
    ]) {
      expect(screen.getByText(label)).toBeInTheDocument();
    }
    // The Rust-carried details render verbatim under each line.
    expect(
      screen.getByText(
        "L'association est déclarée par le paquet et active dès l'installation.",
      ),
    ).toBeInTheDocument();
    expect(
      screen.getByText(
        "Une AppImage ne modifie pas ton système : rien n'est enregistré automatiquement.",
      ),
    ).toBeInTheDocument();
    expect(
      screen.getByText(
        "L'installeur déclare l'association. Windows peut te demander de confirmer et respecte ton choix existant.",
      ),
    ).toBeInTheDocument();
    expect(
      screen.getByText(
        "macOS enregistre l'association quand l'application est déposée dans Applications.",
      ),
    ).toBeInTheDocument();
  });

  it("renders a registered channel as a success chip and the AppImage line as a neutral chip plus its frozen reason", () => {
    renderView();
    // The status labels are the chips themselves (Rust-carried).
    expect(screen.getAllByText("Enregistrée à l'installation")).toHaveLength(
      2,
    );
    expect(screen.getByText("Enregistrée par le système")).toBeInTheDocument();
    expect(screen.getByText("Non enregistrée d'office")).toBeInTheDocument();
    // Tone couples asserted on precise lines: a registered channel
    // renders success, the non-registered one neutral + its reason.
    const debLabel = screen.getByText("Paquet Linux (.deb / .rpm)");
    const debItem = debLabel.closest("li") as HTMLElement;
    const debChip = debItem.querySelector(".ds-chip");
    expect(debChip).toHaveClass("ds-chip--success");
    expect(debChip).toHaveTextContent("Enregistrée à l'installation");
    const appImageLabel = screen.getByText("AppImage (Linux)");
    const appImageItem = appImageLabel.closest("li") as HTMLElement;
    const appImageChip = appImageItem.querySelector(".ds-chip");
    expect(appImageChip).toHaveClass("ds-chip--neutral");
    expect(appImageChip).toHaveTextContent("Non enregistrée d'office");
    expect(
      within(appImageItem).getByText(
        "Tu peux ajouter l'association avec un outil d'intégration AppImage ou une entrée d'application manuelle.",
      ),
    ).toBeInTheDocument();
    // Never an error tone, never an alert region.
    expect(appImageItem.querySelector(".ds-chip--error")).toBeNull();
    expect(appImageItem.querySelector(".ds-chip--warning")).toBeNull();
  });

  it("renders no current-install notice when the probe did not speak", () => {
    renderView();
    // The only status regions of a fully-loaded screen are none at
    // all: the notice is ABSENT, never an invented claim.
    expect(screen.queryByRole("status")).not.toBeInTheDocument();
    expect(
      screen.queryByText(/Ton installation actuelle/),
    ).not.toBeInTheDocument();
  });

  it("renders the current-install notice first, role=status, when the probe spoke", () => {
    renderView(loaded(profileWithCurrentInstall()), loaded(officialPolicy()));
    const notice = screen.getByRole("status");
    expect(notice).toHaveTextContent(
      "Ton installation actuelle est une AppImage : l'association n'est pas enregistrée d'office.",
    );
    // Rendered at the head of the section body, before the channel
    // list.
    const body = notice.closest(".support-profile__section-body");
    expect(body).not.toBeNull();
    expect(
      notice.compareDocumentPosition(
        (body as HTMLElement).querySelector(
          ".support-profile__associations",
        ) as HTMLElement,
      ) & Node.DOCUMENT_POSITION_FOLLOWING,
    ).toBeTruthy();
  });

  it("adds no interactive element to the file-association section (read-only, no toggle)", () => {
    renderView(loaded(profileWithCurrentInstall()), loaded(officialPolicy()));
    const heading = screen.getByRole("heading", {
      level: 2,
      name: "Association de fichiers",
    });
    const section = heading.closest("section") as HTMLElement;
    expect(section.querySelector("button")).toBeNull();
    expect(section.querySelector("input")).toBeNull();
    expect(section.querySelector("a")).toBeNull();
    expect(section.querySelector('[role="switch"]')).toBeNull();
  });

  it("keeps the reason keyboard-reachable next to its capability line", () => {
    renderView();
    // The V3 import capability line carries label + chip + reason in
    // the same list item, so a keyboard/screen-reader user reading the
    // line gets the limit and its reason together.
    const v3ImportReason = screen.getAllByText(
      "Rétro-ingénierie du format en cours",
    )[0];
    const listItem = v3ImportReason.closest("li");
    expect(listItem).not.toBeNull();
    expect(
      within(listItem as HTMLElement).getByText(
        "Copie dans la bibliothèque locale",
      ),
    ).toBeInTheDocument();
    expect(
      within(listItem as HTMLElement).getByText(
        "Non disponible dans cette version",
      ),
    ).toBeInTheDocument();
  });
});

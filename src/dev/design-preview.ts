/** Development-only browser entry. All stories are fictional and every
 * mutation lives in memory until reload. Production uses index.html only. */
import { mockIPC, mockWindows } from "@tauri-apps/api/mocks";
import { version } from "../../package.json";
import {
  isLibraryOverviewDto,
  type StoryCardDto,
} from "../shared/ipc-contracts/library";
import {
  isStoryDetailDto,
  type NodeContentDto,
  type StoryDetailDto,
} from "../shared/ipc-contracts/story";
import {
  isConnectedDeviceDto,
  type ConnectedDeviceDto,
} from "../shared/ipc-contracts/device";
import {
  isDeviceLibraryDto,
  type DeviceLibraryDto,
} from "../shared/ipc-contracts/device-library";
import {
  isSupportProfile,
  isUpdateApplyPlan,
  isUpdateAvailability,
} from "../shared/ipc-contracts/settings";
import {
  isAnnouncementVoicesDto,
  isStoryPresentationDto,
  type StoryPresentationDto,
} from "../shared/ipc-contracts/presentation";
import { isContentSourcePolicy } from "../shared/ipc-contracts/import-export";

if (!import.meta.env.DEV) {
  throw new Error("The design preview is available only in development.");
}

const params = new URLSearchParams(location.search);
const scenario = params.get("scenario") ?? "offline";
const screen = params.get("screen") ?? "library";
document.title = "Rustory — aperçu — données de démonstration";
const timestamp = "2026-11-15T10:00:00.000Z";
const deviceIdentifier = "0123456789abcdef0123456789abcdef";
const titles = [
  "La forêt des petits secrets",
  "Le voyage de la petite baleine",
  "Un dragon dans mon jardin",
  "Les étoiles de Lila",
  "Le train des animaux",
  "La cabane au bout du monde",
  "Le renard et la lune",
  "Une journée chez les nuages",
];
let stories: StoryCardDto[] =
  scenario === "empty"
    ? []
    : titles.map((title, index) => ({
        id: `s${index + 1}`,
        title,
        sendable: false,
        sendBlocker: "missingAudio",
        ...(index === 1 ? { importState: "recognized" as const } : {}),
      }));
const nodes = new Map<string, NodeContentDto[]>();
for (const story of stories) {
  nodes.set(story.id, [
    {
      id: "n1",
      label: "Le début de l'aventure",
      text: "Au bord de la forêt, une petite lumière brillait entre les feuilles. Léonie prit son sac et suivit le sentier, curieuse de découvrir qui l'attendait.",
      image: null,
      audio: null,
    },
    {
      id: "n2",
      label: "Une rencontre inattendue",
      text: "Un renard apparut au détour du chemin. Il portait une écharpe couleur de soleil et cherchait la clé de sa cabane.",
      image: null,
      audio: null,
    },
    {
      id: "n3",
      label: "Le chemin du retour",
      text: "Quand les étoiles s'allumèrent, Léonie rentra chez elle avec une nouvelle histoire à raconter.",
      image: null,
      audio: null,
    },
  ]);
}
const drafts = new Map<string, unknown>();
const presentations = new Map<string, StoryPresentationDto>();

function detailFor(id: string, selectedNodeId?: string): StoryDetailDto | null {
  const story = stories.find((item) => item.id === id);
  if (!story) return null;
  const content = nodes.get(id) ?? [];
  const structure = {
    startNodeId: content[0].id,
    nodes: content.map((node, index) => ({
      id: node.id,
      label: node.label,
      isStart: index === 0,
      hasIssue: false,
      options: [],
    })),
  };
  return {
    id,
    title: story.title,
    schemaVersion: 3,
    structureJson: JSON.stringify({ schemaVersion: 3, ...structure }),
    contentChecksum: "a".repeat(64),
    createdAt: timestamp,
    updatedAt: timestamp,
    editable: true,
    editScope: "full",
    importState:
      story.importState === "blocked" ? null : (story.importState ?? null),
    structure,
    node: content.find((node) => node.id === selectedNodeId) ?? content[0],
  };
}

function presentationFor(id: string): StoryPresentationDto {
  let value = presentations.get(id);
  if (!value) {
    const story = stories.find((item) => item.id === id);
    value = {
      layout: "sequential",
      autoContinue: false,
      archiveRetained: false,
      linear: false,
      linearBlocker: {
        reason: "missingAudio",
        nodeId: "n1",
        label: "Le début de l'aventure",
      },
      title: {
        spokenText: story?.title ?? "Nouvelle histoire",
        status: "missing",
      },
      question: {
        spokenText: "Quelle histoire veux-tu écouter ?",
        status: "missing",
      },
      chapters: (nodes.get(id) ?? []).map((node) => ({
        nodeId: node.id,
        label: node.label,
        spokenText: node.label,
        status: "missing",
      })),
    };
    presentations.set(id, value);
  }
  return value;
}

const device: ConnectedDeviceDto =
  scenario === "connected"
    ? {
        kind: "supported",
        family: "lunii",
        firmwareCohort: "midGenV2",
        metadataFormatVersion: 6,
        deviceIdentifier,
        supportedOperations: {
          readLibrary: true,
          inspectStory: true,
          importStory: true,
          writeStory: true,
          deleteStory: true,
          sendArchive: false,
          reorderStories: true,
        },
      }
    : { kind: "none" };
const deviceLibrary: DeviceLibraryDto =
  scenario === "connected"
    ? {
        kind: "readable",
        deviceIdentifier,
        stories: stories.slice(0, 3).map((story, index) => ({
          uuid: `00000000-0000-0000-0000-${String(index + 1).padStart(12, "0")}`,
          shortId: String(index + 1).padStart(8, "0"),
          hidden: false,
          contentPresent: true,
          alreadyImported: true,
          localStoryId: story.id,
          title: story.title,
          titleSource: "user",
          thumbnail: null,
        })),
      }
    : { kind: "none" };
const voices = {
  voices: [
    {
      id: "preview:fr",
      name: "Camille (aperçu)",
      language: "fr-FR",
      engine: "system",
    },
  ],
  selectedVoiceId: "preview:fr",
  selectedIsStored: false,
  embedded: {
    state: "notInstalled",
    downloadBytes: 89667631,
    voiceId: "embedded:fr_FR-siwis-medium",
    voiceName: "Voix neuronale française (Siwis)",
  },
};
const updateAvailability = {
  status: "checkNotRun",
  currentVersion: version,
  headline: "La vérification de version n'est pas exécutée pour cette copie.",
  notice:
    "Cette copie de Rustory ne provient pas d'un canal de distribution officiel : aucune vérification réseau n'est effectuée.",
};
const updatePlan = {
  mode: "manual",
  reason: "development_build",
  headline:
    "La mise à jour intégrée n'est pas disponible pour un build de développement.",
  guidance:
    "Reconstruis Rustory depuis les sources pour obtenir la dernière version.",
};

function assertFixture(valid: boolean, name: string): void {
  if (!valid) throw new Error(`Invalid design-preview fixture: ${name}`);
}
assertFixture(isLibraryOverviewDto({ stories }), "library");
assertFixture(isConnectedDeviceDto(device), "device");
assertFixture(isDeviceLibraryDto(deviceLibrary), "device library");
assertFixture(isAnnouncementVoicesDto(voices), "voices");
assertFixture(isSupportProfile(officialProfile()), "support profile");
assertFixture(isContentSourcePolicy(officialPolicy()), "source policy");
assertFixture(isUpdateAvailability(updateAvailability), "update availability");
assertFixture(isUpdateApplyPlan(updatePlan), "update plan");
for (const story of stories) {
  assertFixture(isStoryDetailDto(detailFor(story.id)), story.id);
  assertFixture(
    isStoryPresentationDto(presentationFor(story.id)),
    `presentation ${story.id}`,
  );
}

function unsupported(command: string): never {
  throw {
    code: "UNKNOWN",
    message: "Cette action n'est pas disponible dans l'aperçu navigateur.",
    userAction: "Utilise l'application Rustory pour cette action.",
    details: { command },
  };
}

mockWindows("main");
mockIPC(
  (command, payload) => {
    const args = (payload ?? {}) as Record<string, unknown>;
    const input = (args.input ?? {}) as Record<string, unknown>;
    switch (command) {
      case "get_library_overview":
        return structuredClone({ stories });
      case "read_connected_lunii":
        return structuredClone(device);
      case "read_device_library":
        return structuredClone(deviceLibrary);
      case "get_official_catalog_status":
        return { count: 0 };
      case "analyze_os_open_request":
      case "analyze_drop_request":
        return { kind: "none" };
      case "read_transfer_outcome":
        return null;
      case "read_transfer_state":
        return { kind: "idle" };
      case "read_transfer_preview":
      case "read_story_validation":
        return scenario === "connected"
          ? unsupported(command)
          : { kind: "noDevice" };
      case "read_content_source_policy":
        return officialPolicy();
      case "read_support_profile":
        return officialProfile();
      case "plugin:app|version":
        return version;
      case "read_update_availability":
        return updateAvailability;
      case "read_update_apply_plan":
        return updatePlan;
      case "read_update_apply_state":
        return { status: "idle" };
      case "read_announcement_voices":
        return structuredClone(voices);
      case "set_announcement_voice": {
        if (input.voiceId !== voices.selectedVoiceId)
          return unsupported(command);
        voices.selectedIsStored = true;
        return structuredClone(voices);
      }
      case "get_story_detail":
        return structuredClone(
          detailFor(
            String(args.storyId),
            typeof args.nodeId === "string" ? args.nodeId : undefined,
          ),
        );
      case "read_story_presentation":
        return structuredClone(presentationFor(String(args.storyId)));
      case "set_story_layout": {
        if (input.layout !== "sequential" && input.layout !== "menu")
          return unsupported(command);
        const value = presentationFor(String(input.storyId));
        value.layout = input.layout;
        return structuredClone(value);
      }
      case "set_story_auto_continue": {
        const value = presentationFor(String(input.storyId));
        value.autoContinue = input.autoContinue === true;
        return structuredClone(value);
      }
      case "read_recoverable_draft":
      case "read_recoverable_node_draft":
        return { kind: "none" };
      case "record_draft":
      case "record_node_draft":
        drafts.set(
          `${command}:${String(input.storyId)}`,
          structuredClone(input),
        );
        return;
      case "discard_draft":
        drafts.delete(`record_draft:${String(input.storyId)}`);
        return;
      case "discard_node_draft":
        drafts.delete(`record_node_draft:${String(input.storyId)}`);
        return;
      case "create_story": {
        const title = typeof input.title === "string" ? input.title.trim() : "";
        if (!title) return unsupported(command);
        const story: StoryCardDto = {
          id: `preview-${crypto.randomUUID()}`,
          title,
          sendBlocker: "missingAudio",
        };
        stories.push(story);
        nodes.set(story.id, [
          { id: "n1", label: "", text: "", image: null, audio: null },
        ]);
        return structuredClone(story);
      }
      case "update_story": {
        const story = stories.find((item) => item.id === input.id);
        if (!story || typeof input.title !== "string" || !input.title.trim())
          return unsupported(command);
        story.title = input.title.trim();
        return {
          id: story.id,
          title: story.title,
          updatedAt: new Date().toISOString(),
          importState: story.importState ?? null,
        };
      }
      case "update_node_content": {
        const storyId = String(input.storyId);
        const node = nodes
          .get(storyId)
          ?.find((item) => item.id === input.nodeId);
        if (
          !node ||
          typeof input.text !== "string" ||
          typeof input.label !== "string"
        )
          return unsupported(command);
        node.text = input.text;
        node.label = input.label;
        return {
          id: storyId,
          updatedAt: new Date().toISOString(),
          contentChecksum: "a".repeat(64),
          node: structuredClone(node),
          importState:
            stories.find((item) => item.id === storyId)?.importState ?? null,
        };
      }
      case "delete_stories": {
        if (
          !Array.isArray(input.ids) ||
          !input.ids.every(
            (id) =>
              typeof id === "string" &&
              stories.some((story) => story.id === id),
          )
        )
          return unsupported(command);
        const ids = input.ids as string[];
        stories = stories.filter((story) => !ids.includes(story.id));
        ids.forEach((id) => {
          nodes.delete(id);
          presentations.delete(id);
        });
        return { deletedIds: ids };
      }
      default:
        return unsupported(command);
    }
  },
  { shouldMockEvents: true },
);

const route =
  screen === "settings"
    ? "/settings"
    : screen === "editor"
      ? "/story/s1/edit"
      : "/library";
history.replaceState(null, "", `${route}?${params.toString()}`);
document.documentElement.dataset.designPreview = scenario;
export const previewReady = import("../main");

// Support fixtures mirror the current Rust matrices and settings DTO.
// That DTO serializes four operations; sendArchive is exposed separately
// by read_connected_lunii, not invented in this support-profile payload.
function officialProfile() {
  const cap = (
    operation: string,
    label: string,
    available: boolean,
    reason?: string,
  ) => ({ operation, label, available, ...(reason ? { reason } : {}) });
  const readCaps = (importAvailable = true) => [
    cap("readLibrary", "Lecture bibliothèque appareil", true),
    cap("inspectStory", "Inspection d'histoire", true),
    importAvailable
      ? cap("importStory", "Copie dans la bibliothèque locale", true)
      : cap(
          "importStory",
          "Copie dans la bibliothèque locale",
          false,
          "Rétro-ingénierie du format en cours",
        ),
  ];
  return {
    devices: [
      {
        family: "lunii",
        familyLabel: "Lunii",
        cohort: "origineV1",
        cohortLabel: "Origine v1",
        metadataFormatLabel: "Format métadonnées v3",
        capabilities: [
          ...readCaps(),
          cap("writeStory", "Transfert vers la Lunii", true),
        ],
      },
      {
        family: "lunii",
        familyLabel: "Lunii",
        cohort: "midGenV2",
        cohortLabel: "Mid-Gen v2",
        metadataFormatLabel: "Format métadonnées v6",
        capabilities: [
          ...readCaps(),
          cap("writeStory", "Transfert vers la Lunii", true),
        ],
      },
      {
        family: "lunii",
        familyLabel: "Lunii",
        cohort: "v3",
        cohortLabel: "V3",
        metadataFormatLabel: "Format métadonnées v7",
        capabilities: [
          ...readCaps(false),
          cap(
            "writeStory",
            "Transfert vers la Lunii",
            false,
            "Rétro-ingénierie du format en cours",
          ),
        ],
      },
      {
        family: "flam",
        familyLabel: "FLAM",
        cohort: "flamGen1",
        cohortLabel: "Gen1",
        capabilities: [
          ...readCaps(),
          cap(
            "writeStory",
            "Transfert vers l'appareil",
            false,
            "Écriture non prouvée sur matériel réel",
          ),
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
        formatLabel: "Format v1",
        available: true,
        capabilitiesLabel: "Création d'une histoire",
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

function officialPolicy() {
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
      {
        kind: "web",
        label: "Page web",
        activation: "enabled",
        activationMarker: "Activée par la distribution officielle",
      },
    ],
  };
}

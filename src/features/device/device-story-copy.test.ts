import { describe, expect, it } from "vitest";

import {
  getDeviceStoryCopyAvailability,
  type DeviceStoryCopyAvailabilityInput,
} from "./device-story-copy";

const available: DeviceStoryCopyAvailabilityInput = {
  story: { contentPresent: true, alreadyImported: false },
  supportedOperations: { importStory: true },
  hasImportHandler: true,
};

describe("getDeviceStoryCopyAvailability", () => {
  it("allows an idle copy only from the verified inventory facts", () => {
    expect(getDeviceStoryCopyAvailability(available)).toEqual({
      enabled: true,
      refusalKind: null,
      reason: null,
      isImporting: false,
      isImported: false,
    });
  });

  it("prioritizes an existing local copy over every other refusal", () => {
    expect(
      getDeviceStoryCopyAvailability({
        story: { contentPresent: false, alreadyImported: true },
        hasImportHandler: false,
      }),
    ).toMatchObject({
      enabled: false,
      refusalKind: "alreadyImported",
      reason: "Copie indisponible: déjà dans ta bibliothèque",
    });
  });

  it.each([undefined, { importStory: false }])(
    "refuses absent or closed capabilities before an incomplete payload: %s",
    (supportedOperations) => {
      expect(
        getDeviceStoryCopyAvailability({
          ...available,
          supportedOperations,
          story: { contentPresent: false, alreadyImported: false },
        }),
      ).toMatchObject({
        enabled: false,
        refusalKind: "unsupportedProfile",
        reason: "Copie indisponible: profil non supporté",
      });
    },
  );

  it("names incomplete content before a missing handler on an authorized profile", () => {
    expect(
      getDeviceStoryCopyAvailability({
        ...available,
        story: { contentPresent: false, alreadyImported: false },
        hasImportHandler: false,
      }),
    ).toMatchObject({
      enabled: false,
      refusalKind: "incompleteContent",
      reason: "Copie indisponible: contenu incomplet sur l'appareil",
    });
  });

  it("distinguishes a missing handler from a genuine profile refusal despite the shared label", () => {
    expect(
      getDeviceStoryCopyAvailability({
        ...available,
        hasImportHandler: false,
      }),
    ).toMatchObject({
      enabled: false,
      refusalKind: "handlerUnavailable",
      reason: "Copie indisponible: profil non supporté",
    });
  });

  it("blocks re-entry during copying and before the post-success inventory refresh", () => {
    expect(
      getDeviceStoryCopyAvailability({
        ...available,
        status: { kind: "importing" },
      }),
    ).toMatchObject({
      enabled: false,
      reason: null,
      isImporting: true,
      isImported: false,
    });
    expect(
      getDeviceStoryCopyAvailability({
        ...available,
        status: {
          kind: "imported",
          story: { id: "local", title: "Histoire copiée" },
          packShortId: "12345678",
        },
      }),
    ).toMatchObject({
      enabled: false,
      reason: null,
      isImporting: false,
      isImported: true,
    });
  });

  it("permits retry after failure only while the current snapshot still allows copying", () => {
    const status = {
      kind: "failed" as const,
      error: {
        code: "UNKNOWN" as const,
        message: "Copie interrompue.",
        userAction: "Réessaie.",
        details: null,
      },
    };
    expect(
      getDeviceStoryCopyAvailability({ ...available, status }).enabled,
    ).toBe(true);
    expect(
      getDeviceStoryCopyAvailability({
        ...available,
        status,
        supportedOperations: { importStory: false },
      }),
    ).toMatchObject({ enabled: false, refusalKind: "unsupportedProfile" });
  });
});

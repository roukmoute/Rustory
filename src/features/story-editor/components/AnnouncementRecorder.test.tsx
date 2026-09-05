import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";

import type { RecordedTake, Recorder, RecordingSession } from "../lib/microphone-recorder";
import { RecorderError } from "../lib/microphone-recorder";

import { AnnouncementRecorder } from "./AnnouncementRecorder";

function take(ms = 1500): RecordedTake {
  return { wav: new Uint8Array(44 + 100), durationMs: ms, sampleRate: 22050 };
}

function fakeRecorder(result: RecordedTake | Error = take()): {
  recorder: Recorder;
  session: RecordingSession;
} {
  const session: RecordingSession = {
    stop: vi.fn(async () => {
      if (result instanceof Error) throw result;
      return result;
    }),
    cancel: vi.fn(),
  };
  return { recorder: { start: vi.fn(async () => session) }, session };
}

describe("AnnouncementRecorder", () => {
  beforeEach(() => {
    vi.spyOn(HTMLMediaElement.prototype, "play").mockImplementation(() => Promise.resolve());
    vi.spyOn(HTMLMediaElement.prototype, "pause").mockImplementation(() => undefined);
    vi.stubGlobal("URL", {
      ...URL,
      createObjectURL: vi.fn(() => "blob:take"),
      revokeObjectURL: vi.fn(),
    });
  });

  it("records, reviews, then hands the take over on « Utiliser »", async () => {
    const user = userEvent.setup();
    const { recorder, session } = fakeRecorder();
    const onUse = vi.fn(async () => undefined);
    render(
      <AnnouncementRecorder
        spokenText="Épisode 1. Un."
        hasClip={false}
        recorder={recorder}
        onUse={onUse}
        rowName="Épisode 1"
      />,
    );
    await user.click(screen.getByRole("button", { name: /enregistrer — épisode 1/i }));
    expect(await screen.findByText(/dis : « Épisode 1\. Un\. »/i)).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Arrêter" }));
    expect(session.stop).toHaveBeenCalled();
    expect(await screen.findByText(/prise de 2 s/i)).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Écouter la prise" }));
    await waitFor(() => expect(HTMLMediaElement.prototype.play).toHaveBeenCalled());
    await user.click(screen.getByRole("button", { name: "Utiliser" }));
    expect(onUse).toHaveBeenCalledWith(expect.objectContaining({ durationMs: 1500 }));
    expect(await screen.findByRole("button", { name: /enregistrer — épisode 1/i })).toBeInTheDocument();
  });

  it("offers « Réenregistrer » when a clip exists and « Refaire » restarts a take", async () => {
    const user = userEvent.setup();
    const { recorder } = fakeRecorder();
    render(
      <AnnouncementRecorder
        spokenText="Q ?"
        hasClip
        recorder={recorder}
        onUse={vi.fn(async () => undefined)}
        rowName="Question"
      />,
    );
    await user.click(screen.getByRole("button", { name: /réenregistrer — question/i }));
    await user.click(await screen.findByRole("button", { name: "Arrêter" }));
    await user.click(await screen.findByRole("button", { name: "Refaire" }));
    expect(recorder.start).toHaveBeenCalledTimes(2);
    expect(await screen.findByRole("button", { name: "Arrêter" })).toBeInTheDocument();
  });

  it("words a refused microphone and an empty take, and stays idle", async () => {
    const user = userEvent.setup();
    const denied: Recorder = {
      start: vi.fn(async () => {
        throw new RecorderError("denied", "refused");
      }),
    };
    render(
      <AnnouncementRecorder
        spokenText="x"
        hasClip={false}
        recorder={denied}
        onUse={vi.fn(async () => undefined)}
        rowName="Titre de la série"
      />,
    );
    await user.click(screen.getByRole("button", { name: /enregistrer — titre/i }));
    expect(await screen.findByRole("alert")).toHaveTextContent(/accès au micro a été refusé/i);
    expect(screen.getByRole("button", { name: /enregistrer — titre/i })).toBeInTheDocument();

    const { recorder } = fakeRecorder(take(0));
    render(
      <AnnouncementRecorder
        spokenText="y"
        hasClip={false}
        recorder={recorder}
        onUse={vi.fn(async () => undefined)}
        rowName="Épisode 2"
      />,
    );
    await user.click(screen.getByRole("button", { name: /enregistrer — épisode 2/i }));
    await user.click(await screen.findByRole("button", { name: "Arrêter" }));
    expect(await screen.findByText(/la prise est vide/i)).toBeInTheDocument();
  });

  it("keeps the take for another try when saving fails", async () => {
    const user = userEvent.setup();
    const { recorder } = fakeRecorder();
    const onUse = vi.fn(async () => {
      throw new Error("L'enregistrement n'a pas pu être lu. Réessaie.");
    });
    render(
      <AnnouncementRecorder
        spokenText="z"
        hasClip={false}
        recorder={recorder}
        onUse={onUse}
        rowName="Épisode 3"
      />,
    );
    await user.click(screen.getByRole("button", { name: /enregistrer — épisode 3/i }));
    await user.click(await screen.findByRole("button", { name: "Arrêter" }));
    await user.click(await screen.findByRole("button", { name: "Utiliser" }));
    expect(await screen.findByRole("alert")).toHaveTextContent(/n'a pas pu être lu/i);
    expect(screen.getByRole("button", { name: "Utiliser" })).toBeInTheDocument();
  });

  it("releases the microphone when unmounted mid-take", async () => {
    const user = userEvent.setup();
    const { recorder, session } = fakeRecorder();
    const view = render(
      <AnnouncementRecorder
        spokenText="w"
        hasClip={false}
        recorder={recorder}
        onUse={vi.fn(async () => undefined)}
        rowName="Épisode 4"
      />,
    );
    await user.click(screen.getByRole("button", { name: /enregistrer — épisode 4/i }));
    await screen.findByRole("button", { name: "Arrêter" });
    view.unmount();
    expect(session.cancel).toHaveBeenCalled();
  });
});

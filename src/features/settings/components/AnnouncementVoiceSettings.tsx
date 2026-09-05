import type React from "react";
import { useEffect, useRef, useState } from "react";

import {
  installEmbeddedVoice,
  previewAnnouncementVoice,
  readAnnouncementVoices,
  setAnnouncementVoice,
} from "../../../ipc/commands/presentation";
import { toAppError } from "../../../shared/errors/app-error";
import type { AnnouncementVoicesDto } from "../../../shared/ipc-contracts/presentation";
import { Button, ProgressIndicator, StateChip, SurfacePanel } from "../../../shared/ui";

import "./AnnouncementVoiceSettings.css";

// Frozen frontend copies (product-language: `Voix des annonces`).
const SECTION_TITLE = "Voix des annonces";
const SECTION_LEAD =
  "La voix qui annonce les histoires et les épisodes sur la Lunii quand une histoire est présentée au choix. Par défaut, une voix française installée sur cet ordinateur.";
const NO_VOICE_NOTE =
  "Aucune voix française n'est disponible sur cet ordinateur. Installe une voix française dans les réglages du système, ou télécharge la voix neuronale ci-dessous.";
const LOADING_LABEL = "Lecture des voix…";
const UNAVAILABLE_NOTE = "Les voix n'ont pas pu être lues. Réessaie plus tard.";
const PREVIEW_LABEL = "Écouter";
const PREVIEWING_LABEL = "Lecture…";
const EMBEDDED_TITLE = "Voix neuronale embarquée";
const EMBEDDED_LEAD =
  "Une voix française de bonne qualité, identique sur tous les ordinateurs. Téléchargée une fois, utilisée hors ligne ensuite.";
const INSTALL_LABEL = "Télécharger la voix neuronale";
const INSTALLING_LABEL = "Téléchargement de la voix neuronale…";
const INSTALLED_NOTE = "Voix neuronale installée.";
const UNSUPPORTED_NOTE =
  "La voix neuronale n'est pas disponible pour cet ordinateur.";

type VoicesRead =
  | { kind: "loading" }
  | { kind: "loaded"; data: AnnouncementVoicesDto }
  | { kind: "unavailable" };

function formatMegabytes(bytes: number): string {
  return `${Math.max(1, Math.round(bytes / 1_000_000))} Mo`;
}

/**
 * The `Voix des annonces` settings section: the French voices available
 * now (system first, the embedded voice once installed), the selection, a
 * listen button per voice, and the embedded voice's download. Every fact
 * comes from Rust (`read_announcement_voices`); this renders and triggers.
 * A failed read is the calm `unavailable` state, never invented voices.
 */
export function AnnouncementVoiceSettings(): React.JSX.Element {
  const [read, setRead] = useState<VoicesRead>({ kind: "loading" });
  const [previewing, setPreviewing] = useState<string | null>(null);
  const [installProgress, setInstallProgress] = useState<number | null>(null);
  const [note, setNote] = useState<string | null>(null);
  const readTokenRef = useRef(0);
  const audioRef = useRef<HTMLAudioElement | null>(null);

  useEffect(() => {
    readTokenRef.current += 1;
    const token = readTokenRef.current;
    setRead({ kind: "loading" });
    void readAnnouncementVoices().then(
      (data) => {
        if (readTokenRef.current === token) setRead({ kind: "loaded", data });
      },
      () => {
        if (readTokenRef.current === token) setRead({ kind: "unavailable" });
      },
    );
    return () => {
      readTokenRef.current += 1;
      audioRef.current?.pause();
    };
  }, []);

  const select = async (voiceId: string): Promise<void> => {
    setNote(null);
    try {
      const data = await setAnnouncementVoice({ voiceId });
      setRead({ kind: "loaded", data });
    } catch (err) {
      setNote(toAppError(err).message);
    }
  };

  const preview = async (voiceId: string): Promise<void> => {
    if (previewing !== null) return;
    setNote(null);
    setPreviewing(voiceId);
    try {
      const sample = await previewAnnouncementVoice({ voiceId });
      audioRef.current?.pause();
      const audio = new Audio(sample.dataUrl);
      audioRef.current = audio;
      await audio.play().catch(() => undefined);
    } catch (err) {
      setNote(toAppError(err).message);
    } finally {
      setPreviewing(null);
    }
  };

  const install = async (): Promise<void> => {
    if (installProgress !== null) return;
    setNote(null);
    setInstallProgress(0);
    try {
      const data = await installEmbeddedVoice((percent) =>
        setInstallProgress(percent),
      );
      setRead({ kind: "loaded", data });
    } catch (err) {
      const error = toAppError(err);
      setNote(
        error.userAction ? `${error.message} ${error.userAction}` : error.message,
      );
    } finally {
      setInstallProgress(null);
    }
  };

  return (
    <SurfacePanel
      as="section"
      elevation={1}
      className="announcement-voices"
      ariaLabelledBy="announcement-voices-title"
    >
      <h2 id="announcement-voices-title" className="announcement-voices__title">
        {SECTION_TITLE}
      </h2>
      <p className="announcement-voices__lead">{SECTION_LEAD}</p>

      {read.kind === "loading" && (
        <p className="announcement-voices__state" role="status">
          {LOADING_LABEL}
        </p>
      )}
      {read.kind === "unavailable" && (
        <p className="announcement-voices__state" role="status">
          {UNAVAILABLE_NOTE}
        </p>
      )}
      {read.kind === "loaded" && (
        <>
          {read.data.voices.length === 0 ? (
            <p className="announcement-voices__state" role="status">
              {NO_VOICE_NOTE}
            </p>
          ) : (
            <ul className="announcement-voices__list" aria-label="Voix disponibles">
              {read.data.voices.map((voice) => {
                const selected = voice.id === read.data.selectedVoiceId;
                return (
                  <li key={voice.id} className="announcement-voices__item">
                    <label className="announcement-voices__choice">
                      <input
                        type="radio"
                        name="announcement-voice"
                        value={voice.id}
                        checked={selected}
                        onChange={() => void select(voice.id)}
                      />
                      <span className="announcement-voices__name">{voice.name}</span>
                      <span className="announcement-voices__meta">
                        {voice.language}
                        {voice.engine === "embedded" ? " · neuronale" : " · système"}
                      </span>
                    </label>
                    <Button
                      variant="quiet"
                      onClick={() => void preview(voice.id)}
                      aria-label={`${PREVIEW_LABEL} — ${voice.name}`}
                      disabled={previewing !== null}
                    >
                      {previewing === voice.id ? PREVIEWING_LABEL : PREVIEW_LABEL}
                    </Button>
                  </li>
                );
              })}
            </ul>
          )}

          <div className="announcement-voices__embedded">
            <h3 className="announcement-voices__subtitle">{EMBEDDED_TITLE}</h3>
            <p className="announcement-voices__lead">{EMBEDDED_LEAD}</p>
            {read.data.embedded.state === "unsupported" && (
              <StateChip tone="neutral" label={UNSUPPORTED_NOTE} />
            )}
            {read.data.embedded.state === "installed" && (
              <StateChip tone="success" label={INSTALLED_NOTE} />
            )}
            {(read.data.embedded.state === "notInstalled" ||
              read.data.embedded.state === "installing") &&
              (installProgress !== null ||
              read.data.embedded.state === "installing" ? (
                <ProgressIndicator
                  mode={installProgress === null ? "indeterminate" : "determinate"}
                  label={INSTALLING_LABEL}
                  value={installProgress ?? undefined}
                />
              ) : (
                <Button variant="secondary" onClick={() => void install()}>
                  {INSTALL_LABEL} ({formatMegabytes(read.data.embedded.downloadBytes)})
                </Button>
              ))}
          </div>
        </>
      )}
      {note !== null && (
        <p className="announcement-voices__note" role="alert">
          {note}
        </p>
      )}
    </SurfacePanel>
  );
}

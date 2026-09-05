import type React from "react";
import { useEffect, useRef, useState } from "react";

import { Button } from "../../../shared/ui";
import type {
  RecordedTake,
  Recorder,
  RecordingSession,
} from "../lib/microphone-recorder";
import { RecorderError } from "../lib/microphone-recorder";

// Frozen frontend copies (product-language: recording an announcement).
const RECORD_LABEL = "Enregistrer";
const RERECORD_LABEL = "Réenregistrer";
const STOP_LABEL = "Arrêter";
const USE_LABEL = "Utiliser";
const RETAKE_LABEL = "Refaire";
const CANCEL_LABEL = "Annuler";
const LISTEN_TAKE_LABEL = "Écouter la prise";
const STARTING_LABEL = "Micro…";
const SAVING_LABEL = "Enregistrement…";
const EMPTY_TAKE_NOTE =
  "La prise est vide : rien n'a été capté. Vérifie le micro et réessaie.";

function failureNote(err: unknown): string {
  if (err instanceof RecorderError) {
    switch (err.failure) {
      case "denied":
        return "L'accès au micro a été refusé. Autorise Rustory à utiliser le micro dans les réglages du système, puis réessaie.";
      case "noDevice":
        return "Aucun micro n'a été trouvé. Branche un micro puis réessaie.";
      case "unsupported":
        return "Le micro n'est pas disponible dans cette version.";
      default:
        return "Le micro n'a pas pu démarrer. Réessaie.";
    }
  }
  return "Le micro n'a pas pu démarrer. Réessaie.";
}

type Phase =
  | { kind: "idle" }
  | { kind: "starting" }
  | { kind: "recording"; session: RecordingSession; since: number }
  | { kind: "review"; take: RecordedTake }
  | { kind: "saving" };

export interface AnnouncementRecorderProps {
  /** What to read aloud — shown while recording. */
  spokenText: string;
  /** `true` when a clip already exists (the label says « Réenregistrer »). */
  hasClip: boolean;
  disabled?: boolean;
  recorder: Recorder;
  /** Hand the accepted take over (the caller attaches it through Rust). */
  onUse: (take: RecordedTake) => Promise<void>;
  /** Accessible suffix naming the row (« — Épisode 1 »). */
  rowName: string;
}

/**
 * One announcement's microphone gesture: Enregistrer → (micro) → prise en
 * cours avec compteur → Arrêter → réécoute, Utiliser / Refaire / Annuler.
 * The microphone is released the moment the take ends or the row unmounts.
 */
export function AnnouncementRecorder({
  spokenText,
  hasClip,
  disabled = false,
  recorder,
  onUse,
  rowName,
}: AnnouncementRecorderProps): React.JSX.Element {
  const [phase, setPhase] = useState<Phase>({ kind: "idle" });
  const [elapsed, setElapsed] = useState(0);
  const [note, setNote] = useState<string | null>(null);
  const phaseRef = useRef<Phase>(phase);
  phaseRef.current = phase;
  const audioRef = useRef<HTMLAudioElement | null>(null);

  useEffect(() => {
    if (phase.kind !== "recording") return undefined;
    const timer = setInterval(() => {
      setElapsed(Math.floor((Date.now() - phase.since) / 1000));
    }, 250);
    return () => clearInterval(timer);
  }, [phase]);

  useEffect(
    () => () => {
      const current = phaseRef.current;
      if (current.kind === "recording") current.session.cancel();
      audioRef.current?.pause();
    },
    [],
  );

  const start = async (): Promise<void> => {
    setNote(null);
    setPhase({ kind: "starting" });
    try {
      const session = await recorder.start();
      setElapsed(0);
      setPhase({ kind: "recording", session, since: Date.now() });
    } catch (err) {
      setNote(failureNote(err));
      setPhase({ kind: "idle" });
    }
  };

  const stop = async (): Promise<void> => {
    if (phase.kind !== "recording") return;
    try {
      const take = await phase.session.stop();
      if (take.wav.length <= 44 || take.durationMs === 0) {
        setNote(EMPTY_TAKE_NOTE);
        setPhase({ kind: "idle" });
        return;
      }
      setPhase({ kind: "review", take });
    } catch {
      setNote("La prise n'a pas pu être terminée. Réessaie.");
      setPhase({ kind: "idle" });
    }
  };

  const listen = (take: RecordedTake): void => {
    audioRef.current?.pause();
    const blob = new Blob([take.wav], { type: "audio/wav" });
    const url = URL.createObjectURL(blob);
    const audio = new Audio(url);
    audio.onended = () => URL.revokeObjectURL(url);
    audioRef.current = audio;
    void audio.play().catch(() => undefined);
  };

  const use = async (take: RecordedTake): Promise<void> => {
    setPhase({ kind: "saving" });
    try {
      await onUse(take);
      setPhase({ kind: "idle" });
    } catch (err) {
      setNote(err instanceof Error ? err.message : "L'enregistrement n'a pas pu être rangé.");
      setPhase({ kind: "review", take });
    }
  };

  return (
    <div className="announcement-recorder">
      {phase.kind === "idle" && (
        <Button
          variant="quiet"
          onClick={() => void start()}
          disabled={disabled}
          aria-label={`${hasClip ? RERECORD_LABEL : RECORD_LABEL} — ${rowName}`}
        >
          {hasClip ? RERECORD_LABEL : RECORD_LABEL}
        </Button>
      )}
      {phase.kind === "starting" && (
        <span className="announcement-recorder__state" role="status">
          {STARTING_LABEL}
        </span>
      )}
      {phase.kind === "recording" && (
        <div className="announcement-recorder__live" role="status">
          <span className="announcement-recorder__dot" aria-hidden="true" />
          <span className="announcement-recorder__prompt">
            Dis : « {spokenText} » — {elapsed} s
          </span>
          <Button variant="primary" onClick={() => void stop()}>
            {STOP_LABEL}
          </Button>
        </div>
      )}
      {phase.kind === "review" && (
        <div className="announcement-recorder__review">
          <span className="announcement-recorder__state">
            Prise de {Math.max(1, Math.round(phase.take.durationMs / 1000))} s
          </span>
          <Button variant="quiet" onClick={() => listen(phase.take)}>
            {LISTEN_TAKE_LABEL}
          </Button>
          <Button variant="primary" onClick={() => void use(phase.take)}>
            {USE_LABEL}
          </Button>
          <Button variant="quiet" onClick={() => void start()}>
            {RETAKE_LABEL}
          </Button>
          <Button variant="quiet" onClick={() => setPhase({ kind: "idle" })}>
            {CANCEL_LABEL}
          </Button>
        </div>
      )}
      {phase.kind === "saving" && (
        <span className="announcement-recorder__state" role="status">
          {SAVING_LABEL}
        </span>
      )}
      {note !== null && (
        <p className="announcement-recorder__note" role="alert">
          {note}
        </p>
      )}
    </div>
  );
}

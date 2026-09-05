import type React from "react";
import { useEffect, useRef, useState } from "react";

import {
  generateStoryAnnouncements,
  readStoryPresentation,
  setStoryLayout,
} from "../../../ipc/commands/presentation";
import { readNodeMedia } from "../../../ipc/commands/story";
import { toAppError } from "../../../shared/errors/app-error";
import type {
  AnnouncementStatus,
  StoryLayout,
  StoryPresentationDto,
} from "../../../shared/ipc-contracts/presentation";
import {
  Button,
  ProgressIndicator,
  StateChip,
  SurfacePanel,
} from "../../../shared/ui";
import type { StateChipTone } from "../../../shared/ui";

import "./StoryPresentationPanel.css";

// Frozen frontend copies (product-language: `Présentation sur la Lunii`).
const PANEL_TITLE = "Présentation sur la Lunii";
const LOADING_LABEL = "Lecture de la présentation…";
const UNAVAILABLE_NOTE =
  "La présentation n'a pas pu être lue. Réessaie plus tard.";
const ARCHIVE_NOTE =
  "Cette histoire est envoyée telle quelle depuis son archive d'origine : la présentation ci-dessous ne s'applique pas à l'envoi.";
const NOT_LINEAR_NOTE =
  "La présentation au choix demande un audio sur chaque épisode et aucun choix dans l'histoire.";
const LAYOUT_LEGEND = "Comment la Lunii joue cette histoire";
const SEQUENTIAL_LABEL = "À la suite";
const SEQUENTIAL_HINT = "Les épisodes s'enchaînent dans l'ordre.";
const MENU_LABEL = "Au choix";
const MENU_HINT =
  "L'enfant choisit l'épisode sur la molette, puis revient au menu à la fin.";
const ANNOUNCEMENTS_TITLE = "Annonces";
const ANNOUNCEMENTS_LEAD =
  "Les annonces sont lues par la voix des réglages : le titre de la série sur la molette, la question, puis le titre de chaque épisode.";
const GENERATE_LABEL = "Générer les annonces";
const REGENERATE_LABEL = "Régénérer toutes les annonces";
const GENERATING_LABEL = "Génération des annonces…";
const LISTEN_LABEL = "Écouter";
const TITLE_ROW = "Titre de la série";
const QUESTION_ROW = "Question";

type PresentationRead =
  | { kind: "loading" }
  | { kind: "loaded"; data: StoryPresentationDto }
  | { kind: "unavailable" };

function statusChip(status: AnnouncementStatus): {
  tone: StateChipTone;
  label: string;
} {
  switch (status) {
    case "ready":
      return { tone: "success", label: "prête" };
    case "stale":
      return { tone: "warning", label: "à régénérer" };
    case "missing":
      return { tone: "neutral", label: "manquante" };
  }
}

export interface StoryPresentationPanelProps {
  storyId: string;
  /** `false` while the story is not editable (a pending recovery). */
  editable: boolean;
  /** Changes when the structure (labels, nodes) changes: re-reads. */
  structureKey: string;
}

/**
 * The `Présentation sur la Lunii` zone of the editor: the layout (episodes
 * in sequence, or a spoken menu) and, for the menu, the spoken
 * announcements — their state per Rust, one generation gesture, a listen
 * button per generated clip. Every fact comes from
 * `read_story_presentation`; a failed read is the calm unavailable state.
 */
export function StoryPresentationPanel({
  storyId,
  editable,
  structureKey,
}: StoryPresentationPanelProps): React.JSX.Element {
  const [read, setRead] = useState<PresentationRead>({ kind: "loading" });
  const [generating, setGenerating] = useState<number | null>(null);
  const [note, setNote] = useState<string | null>(null);
  const readTokenRef = useRef(0);
  const audioRef = useRef<HTMLAudioElement | null>(null);

  useEffect(() => {
    readTokenRef.current += 1;
    const token = readTokenRef.current;
    void readStoryPresentation({ storyId }).then(
      (data) => {
        if (readTokenRef.current === token) setRead({ kind: "loaded", data });
      },
      () => {
        if (readTokenRef.current === token) setRead({ kind: "unavailable" });
      },
    );
    return () => {
      readTokenRef.current += 1;
    };
  }, [storyId, structureKey]);

  useEffect(() => () => audioRef.current?.pause(), []);

  const changeLayout = async (layout: StoryLayout): Promise<void> => {
    setNote(null);
    try {
      const data = await setStoryLayout({ storyId, layout });
      setRead({ kind: "loaded", data });
    } catch (err) {
      setNote(toAppError(err).message);
    }
  };

  const generate = async (force: boolean): Promise<void> => {
    if (generating !== null) return;
    setNote(null);
    setGenerating(0);
    try {
      const outcome = await generateStoryAnnouncements(
        { storyId, force },
        (percent) => setGenerating(percent),
      );
      setRead({ kind: "loaded", data: outcome.presentation });
    } catch (err) {
      const error = toAppError(err);
      setNote(
        error.userAction
          ? `${error.message} ${error.userAction}`
          : error.message,
      );
    } finally {
      setGenerating(null);
    }
  };

  const listen = async (assetId: string): Promise<void> => {
    setNote(null);
    try {
      const preview = await readNodeMedia({ storyId, assetId });
      audioRef.current?.pause();
      const audio = new Audio(preview.dataUrl);
      audioRef.current = audio;
      await audio.play().catch(() => undefined);
    } catch (err) {
      setNote(toAppError(err).message);
    }
  };

  const busy = generating !== null || !editable;

  return (
    <SurfacePanel
      as="section"
      elevation={1}
      className="story-presentation"
      ariaLabelledBy="story-presentation-title"
    >
      <h2 id="story-presentation-title" className="story-presentation__title">
        {PANEL_TITLE}
      </h2>
      {read.kind === "loading" && (
        <p className="story-presentation__state" role="status">
          {LOADING_LABEL}
        </p>
      )}
      {read.kind === "unavailable" && (
        <p className="story-presentation__state" role="status">
          {UNAVAILABLE_NOTE}
        </p>
      )}
      {read.kind === "loaded" && read.data.archiveRetained && (
        <p className="story-presentation__state" role="status">
          {ARCHIVE_NOTE}
        </p>
      )}
      {read.kind === "loaded" && !read.data.archiveRetained && (
        <>
          <fieldset className="story-presentation__layout" disabled={busy}>
            <legend className="story-presentation__legend">{LAYOUT_LEGEND}</legend>
            <label className="story-presentation__choice">
              <input
                type="radio"
                name="story-layout"
                value="sequential"
                checked={read.data.layout === "sequential"}
                onChange={() => void changeLayout("sequential")}
              />
              <span>
                <span className="story-presentation__choice-label">
                  {SEQUENTIAL_LABEL}
                </span>
                <span className="story-presentation__choice-hint">
                  {SEQUENTIAL_HINT}
                </span>
              </span>
            </label>
            <label className="story-presentation__choice">
              <input
                type="radio"
                name="story-layout"
                value="menu"
                checked={read.data.layout === "menu"}
                onChange={() => void changeLayout("menu")}
                disabled={!read.data.linear}
              />
              <span>
                <span className="story-presentation__choice-label">{MENU_LABEL}</span>
                <span className="story-presentation__choice-hint">{MENU_HINT}</span>
              </span>
            </label>
            {!read.data.linear && (
              <p className="story-presentation__state" role="status">
                {NOT_LINEAR_NOTE}
              </p>
            )}
          </fieldset>

          {read.data.layout === "menu" && read.data.linear && (
            <div className="story-presentation__announcements">
              <h3 className="story-presentation__subtitle">{ANNOUNCEMENTS_TITLE}</h3>
              <p className="story-presentation__lead">{ANNOUNCEMENTS_LEAD}</p>
              <ul className="story-presentation__list" aria-label="Annonces">
                {[
                  { key: "title", row: TITLE_ROW, announcement: read.data.title },
                  { key: "question", row: QUESTION_ROW, announcement: read.data.question },
                  ...read.data.chapters.map((chapter, index) => ({
                    key: `chapter-${chapter.nodeId}`,
                    row: `Épisode ${index + 1}`,
                    announcement: chapter,
                  })),
                ].map(({ key, row, announcement }) => {
                  const chip = statusChip(announcement.status);
                  return (
                    <li key={key} className="story-presentation__item">
                      <div className="story-presentation__item-text">
                        <span className="story-presentation__item-row">{row}</span>
                        <span className="story-presentation__item-spoken">
                          « {announcement.spokenText} »
                        </span>
                      </div>
                      <div className="story-presentation__item-actions">
                        <StateChip tone={chip.tone} label={chip.label} />
                        {announcement.assetId !== undefined && (
                          <Button
                            variant="quiet"
                            onClick={() => void listen(announcement.assetId as string)}
                            aria-label={`${LISTEN_LABEL} — ${row}`}
                          >
                            {LISTEN_LABEL}
                          </Button>
                        )}
                      </div>
                    </li>
                  );
                })}
              </ul>
              {generating !== null ? (
                <ProgressIndicator
                  mode="determinate"
                  label={GENERATING_LABEL}
                  value={generating}
                />
              ) : (
                <div className="story-presentation__actions">
                  <Button
                    variant="primary"
                    onClick={() => void generate(false)}
                    disabled={!editable}
                  >
                    {GENERATE_LABEL}
                  </Button>
                  <Button
                    variant="quiet"
                    onClick={() => void generate(true)}
                    disabled={!editable}
                  >
                    {REGENERATE_LABEL}
                  </Button>
                </div>
              )}
            </div>
          )}
        </>
      )}
      {note !== null && (
        <p className="story-presentation__note" role="alert">
          {note}
        </p>
      )}
    </SurfacePanel>
  );
}

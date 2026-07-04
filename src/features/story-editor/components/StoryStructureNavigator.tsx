import type React from "react";
import { useEffect, useId, useRef, useState } from "react";

import { Button } from "../../../shared/ui";
import type { AppError } from "../../../shared/errors/app-error";
import type { StoryStructure } from "../../../shared/ipc-contracts/story";

import "./StoryStructureNavigator.css";

export interface StoryStructureNavigatorProps {
  /** Persisted story title, shown as the structure root. */
  title: string;
  /** The node graph PROJECTED BY RUST (`detail.structure`), or `null` when a
   *  blocking canonical issue degrades the zone (`Structure illisible`). */
  structure: StoryStructure | null;
  /** The id of the node currently shown in the editor zone (AC3). */
  currentNodeId: string | null;
  /** `false` for an imported story: NO structural action is rendered. */
  editable: boolean;
  /** A structural mutation is in flight — actions are disabled. */
  busy: boolean;
  /** A refused NODE mutation, surfaced inline at the acted-on entry. */
  nodeError: { nodeId: string; error: AppError } | null;
  /** A refused GLOBAL mutation (add node / a failed targeted re-read),
   *  surfaced inline near the add action. */
  globalError: AppError | null;
  onSelectNode: (nodeId: string) => void;
  onAddNode: () => void;
  onMoveNode: (nodeId: string, direction: "up" | "down") => void;
  onDeleteNode: (nodeId: string) => void;
}

function optionSummary(count: number): string {
  if (count === 0) return "Aucune option";
  if (count === 1) return "1 option";
  return `${count} options`;
}

/**
 * `Story Structure Navigator` — the editor's global-structure zone (AC1).
 *
 * Renders the graph PROJECTED FROM RUST (never re-parsing `structureJson`)
 * as an ordered hierarchical LIST — the story root followed by the nodes in
 * their canonical order. Never a free-form canvas. Keyboard: roving tabindex
 * over the node list (`↑` / `↓` move focus, `Entrée` selects). The start
 * node carries a textual `Départ` mark; a node whose option points at a
 * vanished destination carries a localized glyph + text `à corriger` mark
 * that never hides the rest of the list. Structural actions (add / move /
 * delete, with a two-gesture inline delete confirmation) are rendered ONLY
 * for an editable (native) story. When Rust could not project the graph the
 * zone degrades to the NAMED `Structure illisible` state — never a crash,
 * never a fabricated node. Meaning is carried by glyph + text, never color
 * alone.
 */
export function StoryStructureNavigator({
  title,
  structure,
  currentNodeId,
  editable,
  busy,
  nodeError,
  globalError,
  onSelectNode,
  onAddNode,
  onMoveNode,
  onDeleteNode,
}: StoryStructureNavigatorProps): React.JSX.Element {
  const headingId = useId();
  // Roving tabindex over the node entries: exactly one entry is tabbable;
  // ↑/↓ move the focus between entries without leaving the zone.
  const [focusIndex, setFocusIndex] = useState(0);
  const entryRefs = useRef<Array<HTMLButtonElement | null>>([]);
  // Two-gesture delete confirmation, inline and localized on ONE node at a
  // time. Any other structural action cancels it.
  const [confirmingDeleteId, setConfirmingDeleteId] = useState<string | null>(
    null,
  );
  // Managed focus for the two-step delete: swapping the action row for the
  // confirmation block unmounts the button holding the focus — without a
  // hand-off the keyboard user lands on `body` and falls out of the flow.
  const confirmButtonRef = useRef<HTMLButtonElement | null>(null);
  const deleteButtonRefs = useRef(new Map<string, HTMLButtonElement | null>());
  const [restoreDeleteFocusId, setRestoreDeleteFocusId] = useState<
    string | null
  >(null);
  // Set when a deletion was CONFIRMED: once the node vanishes from the
  // re-projected list, the focus lands on the entry at the re-clamped index.
  const deletePendingRef = useRef(false);
  const lastNodeCountRef = useRef(0);

  const nodes = structure?.nodes ?? [];

  // Keep the roving index within bounds when the list shrinks, and drop a
  // confirmation aimed at a node that no longer exists. After a CONFIRMED
  // deletion, hand the focus to the entry at the re-clamped index so the
  // keyboard user stays in the list.
  useEffect(() => {
    const clamped =
      focusIndex >= nodes.length && nodes.length > 0
        ? nodes.length - 1
        : focusIndex;
    if (clamped !== focusIndex) setFocusIndex(clamped);
    if (
      confirmingDeleteId !== null &&
      !nodes.some((node) => node.id === confirmingDeleteId)
    ) {
      setConfirmingDeleteId(null);
    }
    if (deletePendingRef.current && nodes.length < lastNodeCountRef.current) {
      deletePendingRef.current = false;
      entryRefs.current[clamped]?.focus();
    }
    lastNodeCountRef.current = nodes.length;
  }, [nodes, focusIndex, confirmingDeleteId]);

  // Opening the confirmation moves the focus onto its first button.
  useEffect(() => {
    if (confirmingDeleteId !== null) confirmButtonRef.current?.focus();
  }, [confirmingDeleteId]);

  // Cancelling restores the focus on the logical trigger (`Supprimer le
  // nœud` of the same entry).
  useEffect(() => {
    if (restoreDeleteFocusId === null) return;
    deleteButtonRefs.current.get(restoreDeleteFocusId)?.focus();
    setRestoreDeleteFocusId(null);
  }, [restoreDeleteFocusId]);

  const moveFocus = (next: number): void => {
    if (next < 0 || next >= nodes.length) return;
    setFocusIndex(next);
    entryRefs.current[next]?.focus();
  };

  const handleEntryKeyDown = (
    event: React.KeyboardEvent<HTMLButtonElement>,
    index: number,
  ): void => {
    if (event.key === "ArrowUp") {
      event.preventDefault();
      moveFocus(index - 1);
    } else if (event.key === "ArrowDown") {
      event.preventDefault();
      moveFocus(index + 1);
    }
    // `Entrée` activates the <button> natively → onSelectNode.
  };

  return (
    <section className="story-structure-navigator" aria-labelledby={headingId}>
      <h2 id={headingId} className="story-structure-navigator__heading">
        Structure de l'histoire
      </h2>
      {structure === null ? (
        <p className="story-structure-navigator__degraded" tabIndex={0}>
          <span
            className="story-structure-navigator__glyph"
            aria-hidden="true"
          >
            !
          </span>
          Structure illisible.
        </p>
      ) : (
        <div className="story-structure-navigator__tree">
          <div className="story-structure-navigator__root">
            <span
              className="story-structure-navigator__glyph"
              aria-hidden="true"
            >
              •
            </span>
            <span className="story-structure-navigator__root-label">
              {title}
            </span>
          </div>
          <ul className="story-structure-navigator__list">
            {nodes.map((node, index) => {
              const isCurrent = node.id === currentNodeId;
              const displayLabel =
                node.label.trim().length > 0 ? node.label : node.id;
              const confirming = confirmingDeleteId === node.id;
              const entryError =
                nodeError !== null && nodeError.nodeId === node.id
                  ? nodeError.error
                  : null;
              return (
                <li
                  key={node.id}
                  className="story-structure-navigator__item"
                >
                  <button
                    type="button"
                    ref={(el) => {
                      entryRefs.current[index] = el;
                    }}
                    className={
                      isCurrent
                        ? "story-structure-navigator__node story-structure-navigator__node--current"
                        : "story-structure-navigator__node"
                    }
                    tabIndex={index === focusIndex ? 0 : -1}
                    aria-current={isCurrent ? "true" : undefined}
                    onFocus={() => setFocusIndex(index)}
                    onKeyDown={(event) => handleEntryKeyDown(event, index)}
                    onClick={() => onSelectNode(node.id)}
                  >
                    <span
                      className="story-structure-navigator__glyph"
                      aria-hidden="true"
                    >
                      ›
                    </span>
                    <span className="story-structure-navigator__node-label">
                      {displayLabel}
                    </span>
                    {node.isStart ? (
                      <span className="story-structure-navigator__start-mark">
                        {" "}
                        — Départ
                      </span>
                    ) : null}
                    <span className="story-structure-navigator__options-summary">
                      {" "}
                      · {optionSummary(node.options.length)}
                    </span>
                    {node.hasIssue ? (
                      <span className="story-structure-navigator__issue">
                        <span
                          className="story-structure-navigator__glyph"
                          aria-hidden="true"
                        >
                          !
                        </span>
                        à corriger
                      </span>
                    ) : null}
                    {isCurrent ? (
                      <span className="story-structure-navigator__node-marker">
                        {" "}
                        — en cours d'édition
                      </span>
                    ) : null}
                  </button>
                  {editable ? (
                    confirming ? (
                      <div className="story-structure-navigator__confirm">
                        <p className="story-structure-navigator__confirm-impact">
                          Le nœud et ses médias seront supprimés. Les options
                          qui pointent vers lui resteront à corriger.
                        </p>
                        <Button
                          variant="secondary"
                          disabled={busy}
                          buttonRef={confirmButtonRef}
                          onClick={() => {
                            setConfirmingDeleteId(null);
                            deletePendingRef.current = true;
                            onDeleteNode(node.id);
                          }}
                        >
                          Confirmer la suppression
                        </Button>
                        <Button
                          variant="quiet"
                          disabled={busy}
                          onClick={() => {
                            setConfirmingDeleteId(null);
                            setRestoreDeleteFocusId(node.id);
                          }}
                        >
                          Annuler
                        </Button>
                      </div>
                    ) : (
                      <div className="story-structure-navigator__actions">
                        <Button
                          variant="quiet"
                          disabled={busy || index === 0}
                          aria-label={`Monter — ${displayLabel}`}
                          onClick={() => {
                            setConfirmingDeleteId(null);
                            onMoveNode(node.id, "up");
                          }}
                        >
                          Monter
                        </Button>
                        <Button
                          variant="quiet"
                          disabled={busy || index === nodes.length - 1}
                          aria-label={`Descendre — ${displayLabel}`}
                          onClick={() => {
                            setConfirmingDeleteId(null);
                            onMoveNode(node.id, "down");
                          }}
                        >
                          Descendre
                        </Button>
                        <Button
                          variant="quiet"
                          disabled={busy || node.isStart}
                          buttonRef={(el) => {
                            deleteButtonRefs.current.set(node.id, el);
                          }}
                          aria-label={`Supprimer le nœud — ${displayLabel}`}
                          onClick={() => setConfirmingDeleteId(node.id)}
                        >
                          Supprimer le nœud
                        </Button>
                      </div>
                    )
                  ) : null}
                  {entryError ? (
                    <div
                      className="story-structure-navigator__error"
                      role="alert"
                    >
                      <p className="story-structure-navigator__error-message">
                        {entryError.message}
                      </p>
                      {entryError.userAction ? (
                        <p className="story-structure-navigator__error-action">
                          {entryError.userAction}
                        </p>
                      ) : null}
                    </div>
                  ) : null}
                </li>
              );
            })}
          </ul>
          {editable ? (
            <div className="story-structure-navigator__add">
              <Button variant="quiet" disabled={busy} onClick={onAddNode}>
                Ajouter un nœud
              </Button>
            </div>
          ) : null}
          {globalError ? (
            <div className="story-structure-navigator__error" role="alert">
              <p className="story-structure-navigator__error-message">
                {globalError.message}
              </p>
              {globalError.userAction ? (
                <p className="story-structure-navigator__error-action">
                  {globalError.userAction}
                </p>
              ) : null}
            </div>
          ) : null}
        </div>
      )}
    </section>
  );
}

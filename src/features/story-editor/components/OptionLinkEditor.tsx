import type React from "react";
import { useEffect, useId, useRef, useState } from "react";

import { Button, Field } from "../../../shared/ui";
import type { AppError } from "../../../shared/errors/app-error";
import type {
  NodeGraph,
  OptionLink,
} from "../../../shared/ipc-contracts/story";

import "./OptionLinkEditor.css";

export interface OptionLinkEditorProps {
  /** The SELECTED node's graph entry (its options carry the Rust-derived
   *  link states), or `null` when no node is selected / projected. */
  node: NodeGraph | null;
  /** Every node of the graph — the FLAT destination selector (never a
   *  canvas). Self-reference is a legitimate narrative loop, so the current
   *  node is listed too. */
  nodes: NodeGraph[];
  /** `false` for an imported story: the options render read-only with their
   *  states — NO link action. */
  editable: boolean;
  /** A structural mutation is in flight — actions are disabled. */
  busy: boolean;
  /** A refused OPTION mutation, surfaced inline at the acted-on option. */
  optionError: {
    nodeId: string;
    optionIndex: number;
    error: AppError;
  } | null;
  onAddOption: (label: string) => void;
  onLink: (optionIndex: number, target: string) => void;
  onCreateAndLink: (optionIndex: number) => void;
  onUnlink: (optionIndex: number) => void;
  onRemoveOption: (optionIndex: number) => void;
}

function displayName(node: NodeGraph): string {
  return node.label.trim().length > 0 ? node.label : node.id;
}

function linkStateLabel(option: OptionLink, nodes: NodeGraph[]): string {
  switch (option.state) {
    case "unlinked":
      return "non liée";
    case "linked": {
      const destination = nodes.find((n) => n.id === option.target);
      return destination !== undefined
        ? `liée → ${displayName(destination)}`
        : "liée";
    }
    case "broken":
      // Product language: the words « lien cassé » / « broken » never reach
      // the screen — the repairable state is named `destination à corriger`.
      return "destination à corriger";
    default: {
      const exhaustive: never = option.state;
      return exhaustive;
    }
  }
}

/**
 * `Option Link Editor` — the choices of the selected node (AC2), hosted
 * inside the current-node zone below the content fields. Lists the options
 * with their Rust-derived state (`non liée` / `liée` / `destination à
 * corriger`, glyph + text, never color alone) and offers the link gestures:
 * `Lier` (flat node selector), `Créer et lier un nouveau nœud` (atomic),
 * `Délier`, `Retirer l'option`, `Ajouter une option` (label typed at
 * creation). An invalid destination is PREVENTED by Rust at write time; an
 * already-broken link stays visible and repairable in place. Errors surface
 * inline at the option row in a `role="alert"` region — never a toast.
 */
export function OptionLinkEditor({
  node,
  nodes,
  editable,
  busy,
  optionError,
  onAddOption,
  onLink,
  onCreateAndLink,
  onUnlink,
  onRemoveOption,
}: OptionLinkEditorProps): React.JSX.Element | null {
  const headingId = useId();
  const selectIdBase = useId();
  // Which option row has its flat destination selector open.
  const [linkingIndex, setLinkingIndex] = useState<number | null>(null);
  const [selectedTarget, setSelectedTarget] = useState<string>("");
  // The new-option label, typed at creation.
  const [newOptionLabel, setNewOptionLabel] = useState("");
  // Focus management for the two-step link gesture: the open form focuses
  // its selector; closing it restores the focus on the triggering `Lier`
  // button (the keyboard user never lands on `body`).
  const selectRef = useRef<HTMLSelectElement | null>(null);
  const linkTriggerRefs = useRef<Array<HTMLButtonElement | null>>([]);
  const [restoreFocusIndex, setRestoreFocusIndex] = useState<number | null>(
    null,
  );

  const nodeId = node?.id ?? null;
  // Re-seed the transient UI state when the SELECTED node changes — an open
  // selector must not survive onto another node's options.
  useEffect(() => {
    setLinkingIndex(null);
    setSelectedTarget("");
    setNewOptionLabel("");
  }, [nodeId]);

  // The link form is INDEX-based: any change to the option LIST (a removal,
  // an addition, a link landing) slides the indexes — an open form kept
  // alive would submit against the WRONG option. Close it whenever the
  // list's fingerprint moves (the typed new-option label survives: it is
  // not index-addressed).
  const optionsFingerprint = JSON.stringify(node?.options ?? []);
  const lastFingerprintRef = useRef(optionsFingerprint);
  useEffect(() => {
    if (lastFingerprintRef.current === optionsFingerprint) return;
    lastFingerprintRef.current = optionsFingerprint;
    setLinkingIndex(null);
    setSelectedTarget("");
  }, [optionsFingerprint]);

  // Opening the form moves the focus onto its selector.
  useEffect(() => {
    if (linkingIndex !== null) selectRef.current?.focus();
  }, [linkingIndex]);

  // Closing the form restores the focus on the logical trigger.
  useEffect(() => {
    if (restoreFocusIndex === null) return;
    linkTriggerRefs.current[restoreFocusIndex]?.focus();
    setRestoreFocusIndex(null);
  }, [restoreFocusIndex]);

  if (node === null) return null;

  const options = node.options;

  return (
    <section className="option-link-editor" aria-labelledby={headingId}>
      <h3 id={headingId} className="option-link-editor__heading">
        Options du nœud
      </h3>
      {options.length === 0 ? (
        <p className="option-link-editor__empty">
          Aucune option pour l'instant.
        </p>
      ) : (
        <ul className="option-link-editor__list">
          {options.map((option, index) => {
            const rowError =
              optionError !== null &&
              optionError.nodeId === node.id &&
              optionError.optionIndex === index
                ? optionError.error
                : null;
            const stateLabel = linkStateLabel(option, nodes);
            return (
              <li
                key={`${node.id}-${index}`}
                className="option-link-editor__item"
              >
                <div className="option-link-editor__summary">
                  <span className="option-link-editor__label">
                    {option.label.trim().length > 0
                      ? option.label
                      : `Option ${index + 1}`}
                  </span>
                  <span
                    className={
                      option.state === "broken"
                        ? "option-link-editor__state option-link-editor__state--issue"
                        : "option-link-editor__state"
                    }
                  >
                    {option.state === "broken" ? (
                      <span
                        className="option-link-editor__glyph"
                        aria-hidden="true"
                      >
                        !
                      </span>
                    ) : null}
                    {stateLabel}
                  </span>
                </div>
                {option.state === "broken" ? (
                  // A persistent STATE note (not an action error): role
                  // "status" — announcing it as an alert on every mount
                  // would spam AT users.
                  <p className="option-link-editor__issue-note" role="status">
                    La destination de cette option n'existe plus : le choix ne
                    mènera nulle part tant qu'il n'est pas corrigé. Relie
                    l'option vers un nœud existant ou retire-la.
                  </p>
                ) : null}
                {editable ? (
                  linkingIndex === index ? (
                    <div className="option-link-editor__link-form">
                      <label
                        className="option-link-editor__select-label"
                        htmlFor={`${selectIdBase}-${index}`}
                      >
                        Destination
                      </label>
                      <select
                        id={`${selectIdBase}-${index}`}
                        ref={selectRef}
                        className="option-link-editor__select"
                        value={selectedTarget}
                        onChange={(e) => setSelectedTarget(e.target.value)}
                      >
                        <option value="">Choisis un nœud…</option>
                        {nodes.map((candidate) => (
                          <option key={candidate.id} value={candidate.id}>
                            {displayName(candidate)}
                            {candidate.isStart ? " — Départ" : ""}
                          </option>
                        ))}
                      </select>
                      <Button
                        variant="secondary"
                        disabled={
                          busy ||
                          !nodes.some((candidate) => candidate.id === selectedTarget)
                        }
                        onClick={() => {
                          setLinkingIndex(null);
                          setRestoreFocusIndex(index);
                          onLink(index, selectedTarget);
                        }}
                      >
                        Lier
                      </Button>
                      <Button
                        variant="quiet"
                        disabled={busy}
                        onClick={() => {
                          setLinkingIndex(null);
                          setRestoreFocusIndex(index);
                          onCreateAndLink(index);
                        }}
                      >
                        Créer et lier un nouveau nœud
                      </Button>
                      <Button
                        variant="quiet"
                        disabled={busy}
                        onClick={() => {
                          setLinkingIndex(null);
                          setRestoreFocusIndex(index);
                        }}
                      >
                        Annuler
                      </Button>
                    </div>
                  ) : (
                    <div className="option-link-editor__actions">
                      <Button
                        variant="quiet"
                        disabled={busy}
                        buttonRef={(el) => {
                          linkTriggerRefs.current[index] = el;
                        }}
                        aria-label={`Lier — ${option.label.trim().length > 0 ? option.label : `option ${index + 1}`}`}
                        onClick={() => {
                          // Pre-select the current destination ONLY when it
                          // still exists in the graph: a broken target would
                          // otherwise sit invisible in the <select> (whose
                          // options don't contain it) yet re-submittable.
                          const current = option.target;
                          setSelectedTarget(
                            current !== null &&
                              nodes.some((candidate) => candidate.id === current)
                              ? current
                              : "",
                          );
                          setLinkingIndex(index);
                        }}
                      >
                        Lier
                      </Button>
                      {option.state !== "unlinked" ? (
                        <Button
                          variant="quiet"
                          disabled={busy}
                          aria-label={`Délier — ${option.label.trim().length > 0 ? option.label : `option ${index + 1}`}`}
                          onClick={() => onUnlink(index)}
                        >
                          Délier
                        </Button>
                      ) : null}
                      <Button
                        variant="quiet"
                        disabled={busy}
                        aria-label={`Retirer l'option — ${option.label.trim().length > 0 ? option.label : `option ${index + 1}`}`}
                        onClick={() => onRemoveOption(index)}
                      >
                        Retirer l'option
                      </Button>
                    </div>
                  )
                ) : null}
                {rowError ? (
                  <div className="option-link-editor__error" role="alert">
                    <p className="option-link-editor__error-message">
                      {rowError.message}
                    </p>
                    {rowError.userAction ? (
                      <p className="option-link-editor__error-action">
                        {rowError.userAction}
                      </p>
                    ) : null}
                  </div>
                ) : null}
              </li>
            );
          })}
        </ul>
      )}
      {editable ? (
        <div className="option-link-editor__add">
          <Field
            id="option-link-editor-new-label"
            label="Libellé de la nouvelle option"
            value={newOptionLabel}
            onChange={setNewOptionLabel}
          />
          <Button
            variant="secondary"
            disabled={busy || newOptionLabel.trim().length === 0}
            onClick={() => {
              const label = newOptionLabel.trim();
              setNewOptionLabel("");
              onAddOption(label);
            }}
          >
            Ajouter une option
          </Button>
        </div>
      ) : null}
    </section>
  );
}

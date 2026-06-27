import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";

import {
  StoryStructureNavigator,
  parseStoryStructure,
} from "./StoryStructureNavigator";

const V1_STRUCTURE = '{"schemaVersion":1,"nodes":[]}';

describe("parseStoryStructure", () => {
  it("recognizes the canonical v1 structure", () => {
    expect(parseStoryStructure(V1_STRUCTURE)).toEqual({ kind: "ok" });
  });

  it("marks malformed JSON as unreadable instead of throwing", () => {
    expect(parseStoryStructure("{not json")).toEqual({ kind: "unreadable" });
  });

  it("marks a payload without a nodes array as unreadable", () => {
    expect(parseStoryStructure('{"schemaVersion":1}')).toEqual({
      kind: "unreadable",
    });
    expect(parseStoryStructure('{"schemaVersion":1,"nodes":"oops"}')).toEqual({
      kind: "unreadable",
    });
    expect(parseStoryStructure("null")).toEqual({ kind: "unreadable" });
  });

  it("rejects a drifted schemaVersion instead of masking it as empty", () => {
    // Missing, future and non-numeric versions are all drift the v1 shell
    // cannot honestly project — never a silent "normal empty structure".
    expect(parseStoryStructure('{"nodes":[]}')).toEqual({ kind: "unreadable" });
    expect(parseStoryStructure('{"schemaVersion":2,"nodes":[]}')).toEqual({
      kind: "unreadable",
    });
    expect(parseStoryStructure('{"schemaVersion":"x","nodes":[]}')).toEqual({
      kind: "unreadable",
    });
  });

  it("rejects a non-empty node list as unreadable (v1 carries no node)", () => {
    expect(parseStoryStructure('{"schemaVersion":1,"nodes":[{}]}')).toEqual({
      kind: "unreadable",
    });
  });
});

describe("<StoryStructureNavigator />", () => {
  it("renders the named structure zone with the story as its root", () => {
    render(
      <StoryStructureNavigator
        title="Le soleil couchant"
        structureJson={V1_STRUCTURE}
      />,
    );

    expect(
      screen.getByRole("region", { name: "Structure de l'histoire" }),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("heading", { name: "Structure de l'histoire" }),
    ).toBeInTheDocument();
    // The story is shown as the structure root.
    expect(screen.getByText("Le soleil couchant")).toBeInTheDocument();
  });

  it("names the v1 empty state instead of hiding it (UX-DR38)", () => {
    render(
      <StoryStructureNavigator title="Histoire" structureJson={V1_STRUCTURE} />,
    );
    expect(
      screen.getByText("Aucune saison ni nœud pour l'instant."),
    ).toBeInTheDocument();
  });

  it("exposes the structure root as a keyboard focus stop", () => {
    render(
      <StoryStructureNavigator title="Histoire" structureJson={V1_STRUCTURE} />,
    );
    const root = screen.getByText("Histoire").closest("div");
    expect(root).not.toBeNull();
    expect(root).toHaveAttribute("tabindex", "0");
    root?.focus();
    expect(root).toHaveFocus();
  });

  it("falls back to a named degraded state on a malformed payload, never a crash", () => {
    render(
      <StoryStructureNavigator title="Histoire" structureJson="{broken" />,
    );
    expect(screen.getByText("Structure illisible.")).toBeInTheDocument();
    // No fabricated empty-node line when the payload could not be read.
    expect(
      screen.queryByText("Aucune saison ni nœud pour l'instant."),
    ).not.toBeInTheDocument();
  });

  it("shows the degraded state for a drifted schema, never a silent empty view", () => {
    render(
      <StoryStructureNavigator
        title="Histoire"
        structureJson='{"schemaVersion":2,"nodes":[]}'
      />,
    );
    expect(screen.getByText("Structure illisible.")).toBeInTheDocument();
    expect(
      screen.queryByText("Aucune saison ni nœud pour l'instant."),
    ).not.toBeInTheDocument();
  });

  it("keeps the degraded state a keyboard focus stop (AC3)", () => {
    render(
      <StoryStructureNavigator title="Histoire" structureJson="{broken" />,
    );
    const degraded = screen.getByText("Structure illisible.");
    expect(degraded).toHaveAttribute("tabindex", "0");
    degraded.focus();
    expect(degraded).toHaveFocus();
  });

  it("never echoes the raw structureJson bytes (display only, no reserialization)", () => {
    const { container } = render(
      <StoryStructureNavigator title="Histoire" structureJson={V1_STRUCTURE} />,
    );
    // The component reads the bytes to decide what to show; it must never
    // print them back (that would be the first step toward reformatting a
    // checksum-covered payload).
    expect(container.textContent ?? "").not.toContain("schemaVersion");
    expect(container.textContent ?? "").not.toContain("nodes");
  });
});

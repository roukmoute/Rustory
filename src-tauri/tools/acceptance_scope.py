#!/usr/bin/env python3
"""Restrict a frozen Gherkin feature to a scenario subset (APS scope).

Usage:
    acceptance_scope.py <frozen-feature> <out-feature> [scenario-name]...

Without scenario names the feature is copied unchanged (full scope). The
feature header (language, tags, Feature line, description, Rule line) is
always preserved; only the requested scenario blocks (tags, steps,
Examples table) are kept, in their original order. The scope copy is a
WORKING COPY: the APS mutator stamps its manifest into it, so the frozen
feature must never be scoped in place.

Exit codes: 0 ok, 1 usage error, 2 missing input or unknown scenario name.
"""
from __future__ import annotations

import sys
from pathlib import Path


def scenario_heading(line: str) -> str | None:
    stripped = line.strip()
    for prefix in ("Scenario Outline:", "Scenario:"):
        if stripped.startswith(prefix):
            return stripped[len(prefix):].strip()
    return None


def main(argv: list[str]) -> int:
    if len(argv) < 3:
        print(__doc__, file=sys.stderr)
        return 1
    frozen = Path(argv[1])
    out = Path(argv[2])
    wanted = set(argv[3:])
    if not frozen.is_file():
        print(f"acceptance_scope: no such feature file: {frozen}", file=sys.stderr)
        return 2
    lines = frozen.read_text(encoding="utf-8").splitlines()
    seen: list[str] = []
    header: list[str] = []
    blocks: list[tuple[str, list[str]]] = []
    pending: list[str] = []
    in_header = True
    for line in lines:
        name = scenario_heading(line)
        if name is not None:
            # Trailing tag/blank lines of the pending buffer decorate THIS
            # scenario; the rest belongs to the section before it.
            tags: list[str] = []
            while pending and (pending[-1].strip().startswith("@") or pending[-1].strip() == ""):
                tags.insert(0, pending.pop())
            if in_header:
                header.extend(pending)
                in_header = False
            elif blocks:
                blocks[-1][1].extend(pending)
            else:
                header.extend(pending)
            blocks.append((name, tags + [line]))
            seen.append(name)
            pending = []
        else:
            pending.append(line)
    if in_header:
        header.extend(pending)
    elif blocks:
        blocks[-1][1].extend(pending)
    # The first scenario's tag rode along in the pending buffer; drop the
    # dangling tag line(s) so scoped copies carry no orphaned tags.
    while header and header[-1].strip().startswith("@"):
        header.pop()
    if wanted:
        unknown = sorted(wanted.difference(seen))
        if unknown:
            print(
                "acceptance_scope: unknown scenario name(s): " + ", ".join(unknown),
                file=sys.stderr,
            )
            return 2
        if not (wanted & set(seen)):
            print("acceptance_scope: no scenario selected", file=sys.stderr)
            return 2
        selected = [(name, block) for name, block in blocks if name in wanted]
    else:
        selected = blocks
    out_lines = header
    for _name, block in selected:
        out_lines.extend(block)
    while out_lines and out_lines[-1].strip() == "":
        out_lines.pop()
    out.parent.mkdir(parents=True, exist_ok=True)
    out.write_text("\n".join(out_lines) + "\n", encoding="utf-8")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))

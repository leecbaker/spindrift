#!/usr/bin/env python3
"""Inventory Flex scalar geometry candidates for semantic-type review.

This intentionally reports candidates rather than attempting to decide whether a
value is valid.  The reviewer records that decision in
`doc/flex_semantic_geometry_audit.md`, using the same stable candidate key that
this program prints.

Run:
    python3 tools/audit_flex_semantic_geometry.py
    python3 tools/audit_flex_semantic_geometry.py --format markdown
"""

from __future__ import annotations

import argparse
import json
import pathlib
import re
import sys
from dataclasses import dataclass


ROOT = pathlib.Path(__file__).resolve().parents[1]
FLEX_ROOT = ROOT / "src" / "layout" / "flex"
FLEX_ENTRY = ROOT / "src" / "layout" / "flex.rs"
LEDGER = ROOT / "doc" / "flex_semantic_geometry_ledger.json"

# Type-position only: `::` paths and `as f32` expressions are scalar uses,
# not raw geometry API boundaries. Related scalar collections are reported by
# SCALAR_COMPOSITE below.
API_F32 = re.compile(r"(?:->\s*(?:Option\s*<\s*)?f32\b|:\s*(?:Option\s*<\s*)?f32\b)")
TUPLE_FIELD_F32 = re.compile(
    r"\b(?:struct|enum)\s+[A-Za-z_][A-Za-z0-9_]*(?:<[^>{}]*>)?\s*\([^;{}]*\bf32\b"
)
TYPE_ALIAS_F32 = re.compile(r"\btype\s+[A-Za-z_][A-Za-z0-9_]*\s*=\s*f32\b")
SCALAR_COMPOSITE = re.compile(r"\([^\)]*\bf32\s*,\s*f32[^\)]*\)|\[f32;\s*2\]|Vec<f32>")
POINTS = re.compile(r"\.points\(\)")
TUPLE_BINDING_START = re.compile(r"\blet\s+\(")
SCALAR_BINDING = re.compile(r"\blet\s+(?:mut\s+)?([A-Za-z_][A-Za-z0-9_]*)\s*=")
IDENTIFIER = re.compile(r"\b[A-Za-z_][A-Za-z0-9_]*\b")
OWNER = re.compile(
    r"^\s*(?:pub(?:\([^)]*\))?\s+)?(?:const\s+|async\s+|unsafe\s+)*"
    r"(?:fn|struct|enum|impl)\b"
)


@dataclass
class Candidate:
    """One reviewable scalar boundary or compound geometry candidate."""

    kind: str
    path: pathlib.Path
    line: int
    owner: str
    source: str
    occurrence: int = 0

    @property
    def key(self) -> str:
        """Stable review key that distinguishes repeated source occurrences."""
        relative = self.path.relative_to(ROOT)
        normalized_owner = " ".join(self.owner.split())
        normalized_source = " ".join(self.source.split())
        return (
            f"{self.kind}|{relative}|{normalized_owner}|{normalized_source}|{self.occurrence}"
        )


def flex_sources() -> list[pathlib.Path]:
    """Return the Flex module entry point and all nested Rust sources."""
    return [FLEX_ENTRY, *sorted(FLEX_ROOT.rglob("*.rs"))]


def enclosing_owner(lines: list[str], line_index: int) -> str:
    """Return the nearest preceding declaration for an inventory entry."""
    for previous in range(line_index, -1, -1):
        text = lines[previous].strip()
        if OWNER.match(text):
            return text
    return "<module scope>"


def test_module_lines(lines: list[str]) -> set[int]:
    """Return lines compiled only by the unit-test configuration.

    Assertions often project a typed value to a scalar solely to compare it
    with a literal. That is a test observation, not a production geometry
    conversion, so it does not belong in the runtime semantic-boundary ledger.
    Keep detection deliberately narrow: only a `mod` immediately gated by
    `#[cfg(test)]` is excluded.
    """
    excluded: set[int] = set()
    index = 0
    while index < len(lines):
        if lines[index].strip() != "#[cfg(test)]":
            index += 1
            continue
        module_index = index + 1
        while module_index < len(lines) and not lines[module_index].strip():
            module_index += 1
        if module_index == len(lines) or not re.match(r"\s*mod\s+\w+\s*\{", lines[module_index]):
            index += 1
            continue
        depth = 0
        end_index = module_index
        while end_index < len(lines):
            depth += lines[end_index].count("{") - lines[end_index].count("}")
            if depth == 0:
                break
            end_index += 1
        excluded.update(range(index, min(end_index + 1, len(lines))))
        index = end_index + 1
    return excluded


def scalar_tuple_projections(
    lines: list[tuple[int, str]],
) -> list[tuple[int, tuple[str, ...], str]]:
    """Return raw local tuple projections built from `.points()` values.

    The scan deliberately follows one semicolon-terminated `let (...) = ...`
    expression. It does not treat every tuple in a large helper as a funnel;
    the right-hand side must itself discard typed values to scalar points.
    """
    projections: list[tuple[int, tuple[str, ...], str]] = []
    index = 0
    while index < len(lines):
        line_number, source = lines[index]
        if not TUPLE_BINDING_START.search(source):
            index += 1
            continue
        expression = source
        end_index = index
        while ";" not in expression and end_index + 1 < len(lines):
            end_index += 1
            expression += " " + lines[end_index][1]
        if ".points()" not in expression:
            index = end_index + 1
            continue
        binding, _, _ = expression.partition("=")
        right_hand_side = expression.partition("=")[2].lstrip()
        # A tuple returned by an established scalar table/block/inline API is
        # an adapter candidate, not evidence that the Flex helper itself
        # projected typed values into scalar locals. Funnel detection is for
        # branches or tuple expressions built in this helper.
        if not right_hand_side.startswith(("if ", "match ", "(")):
            index = end_index + 1
            continue
        names = tuple(
            name
            for name in IDENTIFIER.findall(binding)
            if name not in {"let", "mut"}
        )
        if names:
            projections.append((line_number, names, expression))
        index = end_index + 1
    return projections


def scalar_local_projections(
    lines: list[tuple[int, str]],
) -> list[tuple[int, str, str, list[tuple[int, str]]]]:
    """Return named scalar locals that later cross back into typed geometry.

    A common funnel is less visually obvious than a tuple: a typed size is
    assigned to `let width = size.points()`, used throughout an estimator, and
    eventually wrapped in `content_box_pt(width)` or `layout_pt(width)`. The
    individual extraction report catches the first line, but this data-flow
    finding makes the lost semantic identity impossible to classify as a
    harmless local adapter.
    """
    projections: list[tuple[int, str, str, list[tuple[int, str]]]] = []
    for index, (line_number, source) in enumerate(lines):
        binding = SCALAR_BINDING.search(source)
        if binding is None or ".points()" not in source:
            continue
        name = binding.group(1)
        reconstruction = re.compile(
            rf"\b(?:content_box_pt|border_box_pt|layout_pt|non_content_pt|margin_box_pt)\(\s*{name}\b"
        )
        reconstruction_lines = [
            candidate
            for candidate in lines[index + 1 :]
            if reconstruction.search(candidate[1])
        ]
        # A one-off reconstruction can be a deliberately named legacy
        # adapter, for example a CSS scalar constraint routine. Multiple
        # typed re-entries show that the scalar local escaped that boundary
        # and is being used as an untyped geometry carrier instead.
        if len(reconstruction_lines) >= 2:
            projections.append((line_number, name, source, reconstruction_lines))
    return projections


def inventory() -> list[Candidate]:
    """Find every scalar API, scalar composite, and scalar extraction candidate."""
    candidates: list[Candidate] = []
    point_groups: dict[tuple[pathlib.Path, str], list[tuple[int, str]]] = {}
    owner_sources: dict[tuple[pathlib.Path, str], list[tuple[int, str]]] = {}
    for path in flex_sources():
        lines = path.read_text().splitlines()
        test_lines = test_module_lines(lines)
        for index, line in enumerate(lines):
            if index in test_lines:
                continue
            source = line.strip()
            # Documentation and line comments describe scalar types but do not
            # introduce a geometry boundary themselves.
            if source.startswith("//"):
                continue
            owner = enclosing_owner(lines, index)
            owner_sources.setdefault((path, owner), []).append((index + 1, source))
            if API_F32.search(line):
                candidates.append(Candidate("api-f32", path, index + 1, owner, source))
            if TUPLE_FIELD_F32.search(line):
                candidates.append(Candidate("tuple-field-f32", path, index + 1, owner, source))
            if TYPE_ALIAS_F32.search(line):
                candidates.append(Candidate("type-alias-f32", path, index + 1, owner, source))
            if SCALAR_COMPOSITE.search(line):
                candidates.append(Candidate("scalar-composite", path, index + 1, owner, source))
            for extraction_index, _ in enumerate(POINTS.finditer(line), start=1):
                point_groups.setdefault((path, owner), []).append((index + 1, source))
                candidates.append(
                    Candidate(
                        "points-extraction",
                        path,
                        index + 1,
                        owner,
                        f"{source} [points extraction {extraction_index} on this line]",
                    )
                )
    # Several scalar extractions can be a legitimate Taffy or paint adapter,
    # but not when the same helper projects typed values into raw locals and
    # immediately constructs typed metrics again.  Record that data-flow shape
    # independently so it cannot be accepted merely because one extraction in
    # the helper reaches a legacy call.
    for key, extractions in point_groups.items():
        path, owner = key
        for projection_line, names, projection in scalar_tuple_projections(owner_sources[key]):
            reconstruction_lines = [
                (line, source)
                for line, source in owner_sources[key]
                if any(
                    re.search(rf"\b(?:content_box_pt|border_box_pt|layout_pt)\(\s*{name}\b", source)
                    for name in names
                )
            ]
            if not reconstruction_lines:
                continue
            reconstruction_summary = "; ".join(
                f"line {line}: {source}" for line, source in reconstruction_lines
            )
            candidates.append(
                Candidate(
                    "scalar-funnel",
                    path,
                    projection_line,
                    owner,
                    (
                        f"raw tuple projection `{', '.join(names)}` discards typed values: "
                        f"{projection}; typed reconstruction: {reconstruction_summary}"
                    ),
                )
            )
        for projection_line, name, projection, reconstruction_lines in scalar_local_projections(
            owner_sources[key]
        ):
            reconstruction_summary = "; ".join(
                f"line {line}: {source}" for line, source in reconstruction_lines
            )
            candidates.append(
                Candidate(
                    "scalar-funnel",
                    path,
                    projection_line,
                    owner,
                    (
                        f"raw local `{name}` discards typed value: {projection}; "
                        f"typed reconstruction: {reconstruction_summary}"
                    ),
                )
            )
    # Line numbers make a report easy to navigate but must not invalidate a
    # completed decision when a preceding unrelated line is reformatted.  The
    # occurrence index still keeps identical extractions in one API distinct.
    occurrences: dict[tuple[str, pathlib.Path, str, str], int] = {}
    for candidate in candidates:
        identity = (candidate.kind, candidate.path, candidate.owner, candidate.source)
        candidate.occurrence = occurrences.get(identity, 0)
        occurrences[identity] = candidate.occurrence + 1
    return candidates


def print_text(candidates: list[Candidate]) -> None:
    """Print a grep-friendly inventory for triage and review."""
    for candidate in candidates:
        relative = candidate.path.relative_to(ROOT)
        print(
            f"{candidate.kind}\t{relative}:{candidate.line}\t{candidate.owner}\t{candidate.source}"
        )


def print_markdown(candidates: list[Candidate]) -> None:
    """Print a Markdown inventory suitable for review notes."""
    print("| Kind | Location | Enclosing API | Source |")
    print("| --- | --- | --- | --- |")
    for candidate in candidates:
        relative = candidate.path.relative_to(ROOT)
        owner = candidate.owner.replace("|", "\\|")
        source = candidate.source.replace("|", "\\|")
        print(f"| {candidate.kind} | {relative}:{candidate.line} | `{owner}` | `{source}` |")


def print_summary(candidates: list[Candidate]) -> None:
    """Print review progress grouped by source file and candidate kind."""
    decisions = json.loads(LEDGER.read_text()) if LEDGER.exists() else {}
    groups: dict[tuple[str, str], list[Candidate]] = {}
    for candidate in candidates:
        relative = str(candidate.path.relative_to(ROOT))
        groups.setdefault((relative, candidate.kind), []).append(candidate)

    print("| Source | Kind | Candidates | Unreviewed | Reviewed requires refactor | Accepted |")
    print("| --- | --- | ---: | ---: | ---: | ---: |")
    for (path, kind), group in sorted(groups.items()):
        group_decisions = [decisions.get(candidate.key, {}) for candidate in group]
        unreviewed = sum(decision.get("reviewed") is not True for decision in group_decisions)
        requires_refactor = sum(
            decision.get("reviewed") is True
            and decision.get("outcome") == "requires-refactor"
            for decision in group_decisions
        )
        accepted = sum(
            decision.get("reviewed") is True
            and decision.get("outcome") not in (None, "requires-refactor")
            for decision in group_decisions
        )
        print(
            f"| {path} | {kind} | {len(group)} | {unreviewed} | "
            f"{requires_refactor} | {accepted} |"
        )


def check_ledger(candidates: list[Candidate]) -> int:
    """Require an explicit semantic decision for every current candidate."""
    decisions = json.loads(LEDGER.read_text())
    valid_outcomes = {
        "typed",
        "scalar-factor",
        "adapter",
        "wrapper-storage",
        "requires-refactor",
    }
    candidate_keys = {candidate.key for candidate in candidates}
    missing = sorted(candidate_keys - decisions.keys())
    stale = sorted(set(decisions) - candidate_keys)
    invalid = sorted(
        key
        for key, decision in decisions.items()
        if not isinstance(decision, dict)
        or decision.get("outcome") not in valid_outcomes
        or decision.get("reviewed") is not True
        or not isinstance(decision.get("reason"), str)
        or not decision["reason"].strip()
    )
    if not missing and not stale and not invalid:
        return 0
    for key in missing:
        print(f"missing decision: {key}", file=sys.stderr)
    for key in stale:
        print(f"stale decision: {key}", file=sys.stderr)
    for key in invalid:
        print(f"invalid decision: {key}", file=sys.stderr)
    return 1


def check_clean_ledger(candidates: list[Candidate]) -> int:
    """Fail until all evaluated candidates are either typed or intentionally scalar."""
    if check_ledger(candidates):
        return 1
    decisions = json.loads(LEDGER.read_text())
    outstanding = sorted(
        key
        for key, decision in decisions.items()
        if decision["outcome"] == "requires-refactor"
        or (key.startswith("scalar-funnel|") and decision["outcome"] != "typed")
    )
    for key in outstanding:
        print(f"requires refactor: {key}", file=sys.stderr)
    return int(bool(outstanding))


def check_no_scalar_funnels(candidates: list[Candidate]) -> int:
    """Fail when typed geometry is projected through a scalar funnel.

    This is the incremental regression gate. Unlike the clean-ledger check,
    it remains useful while the pre-existing individual adapter queue is being
    reviewed, because it rejects only the data-flow shape that cannot be a
    legitimate local legacy boundary.
    """
    funnels = [candidate for candidate in candidates if candidate.kind == "scalar-funnel"]
    for candidate in funnels:
        relative = candidate.path.relative_to(ROOT)
        print(
            f"scalar funnel: {relative}:{candidate.line}: {candidate.source}",
            file=sys.stderr,
        )
    return int(bool(funnels))


def write_ledger_template(candidates: list[Candidate]) -> None:
    """Write missing candidates as explicit review work without discarding decisions."""
    decisions = json.loads(LEDGER.read_text())
    candidate_keys = {candidate.key for candidate in candidates}
    retained = {key: decision for key, decision in decisions.items() if key in candidate_keys}
    for candidate in candidates:
        placeholder = {
            "outcome": "requires-refactor",
            "reviewed": False,
            "reason": "Unreviewed candidate; classify before accepting this scalar boundary.",
        }
        decision = retained.get(candidate.key)
        if decision is None:
            retained[candidate.key] = placeholder
        elif (
            isinstance(decision, dict)
            and decision.get("outcome") == "requires-refactor"
            and decision.get("reason") == placeholder["reason"]
            and "reviewed" not in decision
        ):
            # Upgrade templates generated before the reviewed flag existed,
            # without changing any deliberate reviewer decision.
            decision["reviewed"] = False
    LEDGER.write_text(json.dumps(retained, indent=2, sort_keys=True) + "\n")


def main() -> int:
    """Run the selected inventory output format."""
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--format", choices=("text", "markdown"), default="text")
    parser.add_argument(
        "--summary",
        action="store_true",
        help="show deterministic review progress by file and candidate kind",
    )
    parser.add_argument(
        "--check-ledger",
        action="store_true",
        help="fail unless doc/flex_semantic_geometry_ledger.json decides every candidate",
    )
    parser.add_argument(
        "--write-ledger-template",
        action="store_true",
        help="add newly discovered candidates to the ledger as explicit review work",
    )
    parser.add_argument(
        "--check-clean-ledger",
        action="store_true",
        help="fail unless every evaluated candidate is typed or intentionally scalar",
    )
    parser.add_argument(
        "--check-no-scalar-funnels",
        action="store_true",
        help="fail when typed geometry is projected through a scalar funnel",
    )
    arguments = parser.parse_args()
    candidates = inventory()
    if arguments.check_no_scalar_funnels:
        return check_no_scalar_funnels(candidates)
    if arguments.write_ledger_template:
        write_ledger_template(candidates)
    if arguments.summary:
        print_summary(candidates)
    elif arguments.format == "markdown":
        print_markdown(candidates)
    else:
        print_text(candidates)
    print(f"# {len(candidates)} Flex semantic-geometry candidates", file=sys.stderr)
    if arguments.check_clean_ledger:
        return check_clean_ledger(candidates)
    return check_ledger(candidates) if arguments.check_ledger else 0


if __name__ == "__main__":
    raise SystemExit(main())

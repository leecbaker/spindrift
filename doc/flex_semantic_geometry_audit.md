# Flex semantic-geometry audit

Run the inventory from the repository root:

```sh
python3 tools/audit_flex_semantic_geometry.py --format markdown
python3 tools/audit_flex_semantic_geometry.py --summary
```

The scan covers `src/layout/flex.rs` and every Rust source below
`src/layout/flex/`. It reports the following candidate kinds:

Unit-test modules gated by `#[cfg(test)]` are deliberately excluded: an
assertion can observe a typed value as a scalar literal without introducing a
production geometry conversion.

- `api-f32`: a raw `f32` in a Flex field, parameter, or return type (the
  requested `-> f32` / `: f32` type positions, not casts or `::` paths);
- `tuple-field-f32` and `type-alias-f32`: scalar storage that the requested
  type-position regex cannot see, including a semantic wrapper's private
  tuple field. This makes wrapper representation explicit in the review
  rather than accidentally treating it as invisible;
- `scalar-composite`: a related scalar pair or collection that may need a
  semantic composite;
- `points-extraction`: one individual extraction which discards a typed value
  to scalar points. Every occurrence is independently reviewed; an adapter
  may not approve unrelated scalar work in the same helper.
- `scalar-funnel`: a helper with a raw tuple, or a named scalar projection
  that re-enters typed geometry in multiple places. This is a mandatory
  refactor finding: it cannot be accepted as an adapter merely because the
  helper also calls one legacy API. A single immediate re-entry is retained as
  an individual `points-extraction` candidate so a documented legacy adapter
  is not falsely reported as a funnel.

The machine-readable decision ledger is
`doc/flex_semantic_geometry_ledger.json`. Its key is the candidate kind, path,
enclosing API, whitespace-normalized source, and occurrence index. The report
retains source line numbers for navigation, but harmless line movement or
formatting does not invalidate a decision; repeated `.points()` calls still
cannot share one accidentally. Each entry has this form:

```json
{
  "api-f32|src/layout/flex/example.rs|fn example(...)|size: f32,|0": {
    "outcome": "typed",
    "reviewed": true,
    "reason": "Flex cross-axis extent; it crosses a Flex helper boundary."
  }
}
```

Start or refresh the review queue without overwriting existing decisions:

```sh
python3 tools/audit_flex_semantic_geometry.py --write-ledger-template
```

New candidates are deliberately written as `requires-refactor` with
`"reviewed": false`. Replace that placeholder only after reviewing the
enclosing API and set `"reviewed": true`; it must never be accepted as an
intentional scalar.

There is deliberately no bulk approval command. Refreshing the ledger always
adds new candidates as unreviewed `requires-refactor` entries. A reviewer must
classify every individual extraction and name its immediate scalar consumer;
folder, filename, or enclosing-function category is never an approval reason.

The focused regression gate is green as soon as no helper projects typed
geometry to a scalar local or tuple and then reconstructs typed geometry:

```sh
python3 tools/audit_flex_semantic_geometry.py --check-no-scalar-funnels
```

Run it in every Flex-focused CI/test command. The broader
`--check-clean-ledger` gate remains intentionally stricter and becomes green
only after each individual adapter candidate has a reviewed decision.

Each candidate must be evaluated in source order and assigned exactly one of
the following outcomes in the review change that touches it:

| Outcome | Requirement |
| --- | --- |
| `typed` | Replace the boundary with an existing semantic type, or introduce a narrowly scoped Flex type when none matches. |
| `scalar-factor` | Keep `f32` only for a ratio, algorithm factor, count, or tolerance. Name the factor if it could be confused with another scalar. |
| `adapter` | Keep the scalar only at an immediate CSS-value, Taffy, inline-layout, paint/PDF, or other external/legacy boundary. The conversion must be named and local. |
| `wrapper-storage` | Keep the scalar private to a semantic wrapper's constructor, operations, and `points()` accessor. It must not cross a Flex API unwrapped. |
| `requires-refactor` | The occurrence has been reviewed and is a real semantic leak. Record the intended target type and keep the clean-ledger check failing until the code is changed. |

The following are never sufficient justifications: a variable name such as
`width`, a nearby comment saying it is a size, or an extraction that travels
through another Flex helper. Related start/end values require typed offsets or
a named bounds composite; widths/heights require physical-content, logical, or
Flex main/cross types according to their coordinate system.

## Review procedure

1. Start with `api-f32` and `scalar-composite` entries; they are API leaks.
   Use `--summary` before each batch to choose the next file and to make the
   review queue and completed decisions visible.
2. For each entry, answer the Agent guidelines' questions: position or extent,
   signed or non-negative, axis, physical/logical coordinate system, and
   box-model space.
3. Change the callee signature first. Let the compiler identify every caller,
   then move `.points()` extraction to the real boundary.
4. Review every reported `points-extraction` after the signature change. An
   adapter is accepted only when its ledger reason names the immediate scalar
   consumer (for example, `taffy_layout::Dimension`, a CSS resolver, or a
   paint primitive). A `scalar-funnel` must be refactored to `typed`; it is
   never an adapter.
5. Add or update a focused test whenever a new Flex wrapper or conversion has
   non-trivial operations.
6. Re-run the inventory. A candidate is complete only when it has a recorded
   outcome and its changed location is no longer an untyped geometry boundary.

## Required verification

After each reviewed batch run:

```sh
python3 tools/audit_flex_semantic_geometry.py > /tmp/flex-semantic-geometry.tsv
python3 tools/audit_flex_semantic_geometry.py --check-no-scalar-funnels
python3 tools/audit_flex_semantic_geometry.py --check-ledger
python3 tools/audit_flex_semantic_geometry.py --check-clean-ledger
cargo test layout::flex --quiet
cargo test intrinsic --quiet
cargo +nightly fmt --check
cargo clippy --all-targets --all-features -- -D warnings
```

Before declaring the module complete, review the complete inventory generated
by the first command, ensure the clean-ledger check succeeds, then run `cargo test` for the full suite. Do not use a
reduced candidate count as proof: every remaining candidate must be an
intentional factor, wrapper implementation, or documented immediate adapter.

# PDF Rendering Parity

Last updated: 2026-07-05

This note tracks PDF content-stream rendering behavior that is separate from
font embedding and from CSS layout conformance. Visual output should be driven
by the paint tree produced by layout; flat primitive vectors are retained only
as an internal compatibility/debug representation while tests and callers move
to the canonical paint model.

## Current Behavior

- HTML/CSS rendering records a paint tree with stacking contexts, paint bands,
  clips, transforms, opacity groups, blend modes, and links before PDF
  serialization.
- Non-stacking paint effect scopes represent clips that must be emitted into
  PDF graphics state without changing CSS paint-band ordering.
- PDF serialization uses the paint tree as the authoritative paint order when
  one is present. The older flat operation writer remains as an internal
  fallback for pages without a paint tree.
- Consecutive compatible fill rectangles in the same paint-tree band and
  stacking context are merged before emission. This is limited to same-fill
  axis-aligned rectangles without stroke, radius, or alpha, and does not cross
  text, image, path, stroke, rounded-rectangle, link, nested stacking-context,
  clip, transform, opacity, or blend boundaries.
- Fully covered opaque rectangle underpaint can be omitted in both the
  paint-tree writer and the remaining flat writer. This preserves final
  compositing while avoiding PDF viewer rasterization artifacts where
  antialiasing at a later fill boundary samples hidden colors underneath.
- `Page` exposes page geometry and read-only primitive slices for inspection.
  External construction through mutable primitive vectors is not a supported
  API goal.

## Remaining Work

- Finish migrating internal tests and debug utilities away from manual flat
  page construction so the fallback flat writer can be deleted or reduced to a
  crate-local test helper.
- Add visual comparison coverage for representative paint-tree output with
  clipping, transforms, opacity groups, blends, images, vector paths, and
  text, using the repo-local PDF comparison workflow.
- Expand PDF/A and PDF/UA validation beyond current structural hooks, including
  tagged PDF structure, output intents, conformance metadata, and external
  validator runs.

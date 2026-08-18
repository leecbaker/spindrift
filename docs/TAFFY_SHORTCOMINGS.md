# Taffy Shortcomings

Last updated: 2026-08-13

This document tracks limitations and impedance mismatches around Quire's use
of Taffy 0.13.0, as resolved by `Cargo.lock`. Entries here are implementation
notes for adapter work and future dependency audits; they are not automatically
spec divergences. Actual CSS/PDF conformance gaps should still be recorded in
`SPEC_DIVERGENCES.md`.

## Entry Criteria

- Include behavior that Taffy cannot currently model directly, or behavior
  where Taffy's public API shape requires Quire to adapt CSS semantics.
- Include Quire post-processing that exists specifically to correct or enrich
  Taffy output.
- Exclude ordinary Quire bugs, even when they occur near Taffy-backed layout.

## Known Limitations and Workarounds

- Mixed length-percentage dimensions: the `TaffyTree` style interface Quire
  uses accepts a length, percentage, or (when enabled) an opaque `calc`
  handle. It cannot carry Quire's owned CSS math representation or its
  percentage-definiteness semantics. Quire resolves mixed values at the
  adapter boundary when the relevant percentage basis is definite.
- CSS Align cross-axis placement and baseline sharing: Taffy 0.13 resolves
  `self-start`/`self-end` for its horizontal-tb Grid model, but cannot express
  a vertical-writing subject or Quire's paged final placement. Its Flex model
  also has no baseline callback. Quire therefore keeps vertical-writing,
  baseline, safe-overflow, and final flex-line placement in its own typed
  post-layout phase.
- `align-content: baseline`: Taffy's public `AlignContentKeyword` has no
  baseline keywords, so Quire maps them to start packing, records flex line
  metadata, and applies baseline packing in post-processing.
- `align-content: stretch` overflow fallback: CSS Align says stretch falls
  back to `flex-start` when stretched flex lines overflow. Taffy's flex
  algorithm applies its generic distribution fallback, so Quire corrects the
  overflow case after recovering flex line metadata.
- Flex item `aspect-ratio`: Taffy's generic flex item `aspect_ratio` field can
  impose final main/cross geometry that does not match CSS Flexbox's interaction
  between stretched cross sizes, transferred flex-basis suggestions, and
  automatic minimum main sizes. Quire therefore keeps authored non-replaced flex
  item ratios out of Taffy's final item geometry and applies them in the flex
  basis and auto-minimum adapter logic. That adapter uses semantic content-box
  and non-content typed lengths before converting to Taffy scalars, so padding
  and borders are not counted twice.
- Leaf measurement baselines: Taffy internally carries first baselines for
  layouts, but `TaffyTree::compute_layout_with_measure` accepts a callback
  that returns only a size. It cannot receive Quire's first/last baseline
  metadata, so Quire computes and stores that information separately for flex
  items and nested flex containers.
- Physical row/column model: Taffy operates in physical row/column axes plus
  an LTR/RTL switch. Quire adapts CSS flex directions, writing modes, logical
  dimensions, and gaps into that model, then converts raw Taffy coordinates
  back to Quire's layout coordinate spaces.
- Rounding: Taffy's default layout rounding is appropriate for screen-pixel
  UI layout, but PDF output must preserve real-valued CSS lengths. Quire
  disables Taffy rounding for flex and Grid layout.

## Not Taffy

- Auto-height row flex containers with `min-height` larger than `max-height`
  should resolve to the minimum size. Taffy 0.12.2 has explicit flex logic for
  `max <= min` and chooses the min size. If Quire paints or returns the smaller
  height for this case, treat it as a Quire adapter or final-height
  preservation bug rather than a Taffy shortcoming.

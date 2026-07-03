# Taffy Shortcomings

Last updated: 2026-07-07

This document tracks limitations and impedance mismatches around Quire's use
of Taffy 0.11.0. Entries here are implementation notes for adapter work and
future dependency audits; they are not automatically spec divergences. Actual
CSS/PDF conformance gaps should still be recorded in `SPEC_DIVERGENCES.md`.

## Entry Criteria

- Include behavior that Taffy cannot currently model directly, or behavior
  where Taffy's public API shape requires Quire to adapt CSS semantics.
- Include Quire post-processing that exists specifically to correct or enrich
  Taffy output.
- Exclude ordinary Quire bugs, even when they occur near Taffy-backed layout.

## Known Limitations and Workarounds

- Mixed length-percentage dimensions: Taffy 0.11 exposes length-only or
  percentage-only dimensions in places where CSS accepts mixed
  `<length-percentage>` math. Quire resolves mixed values at the adapter
  boundary when the relevant percentage basis is definite.
- `self-start` and `self-end` alignment: CSS Box Alignment maps these keywords
  through the alignment subject's own writing mode. Taffy's flex alignment
  model only carries the container-axis keyword, so Quire lets Taffy perform
  sizing and line construction, then corrects final cross-axis offsets.
- `align-content: baseline`: Taffy 0.11 maps baseline content alignment to
  start packing. Quire records flex line metadata and applies baseline packing
  in post-processing.
- `align-content: stretch` overflow fallback: CSS Align says stretch falls
  back to `flex-start` when stretched flex lines overflow. Taffy 0.11 applies
  an older generic distribution fallback, so Quire corrects the overflow case
  after recovering flex line metadata.
- Flex item `aspect-ratio`: Taffy's generic flex item `aspect_ratio` field can
  impose final main/cross geometry that does not match CSS Flexbox's interaction
  between stretched cross sizes, transferred flex-basis suggestions, and
  automatic minimum main sizes. Quire therefore keeps authored non-replaced flex
  item ratios out of Taffy's final item geometry and applies them in the flex
  basis and auto-minimum adapter logic. That adapter uses semantic content-box
  and non-content typed lengths before converting to Taffy scalars, so padding
  and borders are not counted twice.
- Leaf measurement baselines: Taffy's public measure callback returns sizes
  but not first/last baseline metadata. Quire computes and stores baseline
  information separately for flex items and nested flex containers.
- Physical row/column model: Taffy operates in physical row/column axes plus
  an LTR/RTL switch. Quire adapts CSS flex directions, writing modes, logical
  dimensions, and gaps into that model, then converts raw Taffy coordinates
  back to Quire's layout coordinate spaces.
- Rounding: Taffy's default layout rounding is appropriate for screen-pixel
  UI layout, but PDF output must preserve real-valued CSS lengths. Quire
  disables Taffy rounding for flex layout.

## Not Taffy

- Auto-height row flex containers with `min-height` larger than `max-height`
  should resolve to the minimum size. Taffy 0.11 has explicit flex logic for
  `max <= min` and chooses the min size. If Quire paints or returns the smaller
  height for this case, treat it as a Quire adapter or final-height
  preservation bug rather than a Taffy shortcoming.

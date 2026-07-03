# CSS Positioning Parity

Last updated: 2026-07-08

This document tracks current CSS positioning and stacking behavior in Quire.
Known unresolved divergences belong in `SPEC_DIVERGENCES.md`; this file is a
working parity snapshot for implemented behavior and nearby follow-up areas.

## Current Support

- Absolutely and fixed positioned block boxes are laid out out of flow and
  replayed into the containing stacking context by `z-index` level.
- Auto-width absolutely positioned non-replaced blocks use CSS 2.2
  shrink-to-fit sizing, including zero-width results for empty content when no
  inset pair or explicit width fills the containing block.
- Auto-sized absolutely positioned replaced images use their intrinsic
  dimensions and aspect ratio before resolving absolute insets.
- Relative and sticky normal-flow blocks preserve their flow position while
  translating their visual paint by the resolved inset offsets.
- Relative normal inline boxes preserve line metrics and inline advance while
  translating text, inline backgrounds, decorations, links, inline edge atoms,
  and atomic inline boxes by the resolved inset offsets.
- Positioned boxes with integer `z-index`, flex items with `z-index`, page
  margin boxes, and effect-created stacking contexts are represented as nested
  paint contexts before PDF emission.
- Transform translations and transform origins keep typed length components
  until used-value preparation, including `ch` resolved through the selected
  font metric and viewport units resolved against the page area.
- Inline boxes split around in-flow block descendants preserve positioned
  inline visual effects for the split block segment, including relative offsets
  and integer `z-index` stacking.
- Inline-level absolutely positioned descendants use a non-painting
  hypothetical inline placeholder for auto static-position line selection and
  horizontal placement, so forced breaks, wrapping, and RTL alignment choose
  the rectangle the box would have occupied in normal flow. Atomic inline
  sources such as inline-blocks also use the placeholder margin-box top for
  auto vertical placement, preserving explicit top margins and used heights.
- Inline-level absolutely positioned descendants before a terminal generated
  line break, such as HTML `br`, do not collapse the following line onto their
  static-position line; the generated break still creates the normal in-flow
  line advance.
- Block-level absolutely positioned descendants encountered after inline text
  use the buffered inline line sequence for their auto static-position block
  offset, so later floats do not shift or expose the positioned box.
- Block-level absolutely positioned descendants with auto horizontal insets
  honor the static-position containing block's direction, including RTL cases
  that seed `right` from the hypothetical normal-flow static position.
- Absolutely positioned descendants inside inline floats under a single-line
  positioned inline ancestor resolve explicit insets against that inline
  ancestor's generated padding-box containing block instead of the outer block
  or page.
- Inline-block pseudo stacking contexts paint their own background/border
  before atom-owned block content, and non-stacking inline-blocks let
  absolutely positioned descendants escape to the parent stacking context at
  their atom-local auto static position, including block-level absolute
  descendants whose containing block is outside the inline-block. Explicit
  insets remain page-resolved when the inline-block is not a containing block,
  and remain atom-local when the inline-block itself is positioned or
  transformed and establishes the containing block.
- Same-page non-positioned overflow clips apply to descendant paint without
  creating an atomic stacking context, so later normal-flow block backgrounds
  do not cover earlier clipped inline foregrounds.

## Follow-Up Areas

- Broaden WPT coverage for positioned inline ancestors combined with nested
  absolute descendants, transforms, opacity, and fragmentation.
- Audit sticky positioning beyond current static-page behavior once scrolling
  and viewport-relative sticky constraints are represented more fully.
- Continue expanding CSS Positioned Layout Level 3 coverage for edge cases in
  static-position rectangles and writing-mode-specific inset resolution.

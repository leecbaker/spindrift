# CSS Positioning Parity

Last updated: 2026-07-18

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
- Positioned layout prepares a zoom-normalized used style exactly once for
  relative, absolute, and fixed boxes. Fixed inset terms scale with the
  effective zoom, while percentages resolve against the zoomed containing
  block; transformed containing-block translations are carried into captured
  positioned descendants in PDF paint coordinates.
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
  font metric and viewport units resolved against the immutable initial page
  viewport rather than a later named page's used area.
- CSS Transforms Level 2's independent 2D `translate`, `rotate`, and `scale`
  properties compose before the legacy `transform` list, establish the same
  stacking-context and containing-block effects, and preserve typed
  translate-length components through used-value resolution. Scale factors
  accept numbers and percentages in both independent and legacy syntax.
- Legacy `matrix()` values retain a dedicated CSS transform coordinate space.
  Their numeric translation is projected from CSS pixels to PDF-point paint
  coordinates only at the normal-box paint boundary; inline SVG transforms
  instead remain in SVG source space until SVG parsing.
- HTML `transform-box: border-box` and `content-box` select the 2D reference
  rectangle for transform origins and percentage translations. The
  content-box path uses the laid-out border geometry to retain percentage
  padding where that basis is reconstructible.
- Non-invertible 2D transform matrices suppress the transformed subtree and
  its link annotations, as required by CSS Transforms rendering.
- Legacy 3D transform functions are represented with typed Euclid homogeneous
  matrices. Affine projections of `matrix3d`, three-axis translate/scale, and
  axis-angle rotation feed the existing PDF CTM; singular matrices and hidden
  backfaces suppress their subtree. Projective perspective and preserve-3D
  scene composition remain deliberately unsupported.
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
- Block-level absolutely positioned descendants with orthogonal writing modes,
  auto physical vertical insets, and both physical horizontal insets set
  preserve their resolved physical static top edge instead of translating by
  their own content height.
- Nested absolutely positioned boxes that overflow page areas extend the final
  page sequence from their resolved absolute offsets without leaking temporary
  positioned-subtree pagination into normal flow; fixed descendants discovered
  inside those subtrees still replay on every generated page. Nested absolute
  layers retain their independently resolved destination-page ownership rather
  than being remapped again by an ancestor, and the final page-span requirement
  is merged across the nested subtree.
- Fixed descendants replay after the final page sequence is known. Their
  initial containing-block geometry is retained while each output page supplies
  its own media clip, including later named pages with a different used page
  box.
- When an absolutely positioned box prebreaks its first in-flow child, a
  background-only source slice is discarded before page ownership is remapped;
  later child fragments therefore begin on the intended destination page
  without materializing an empty intermediate page.
- Absolute-position fragment remapping moves the principal scratch fragments
  to their destination page while nested positioned stacking contexts retain
  their independently resolved destination ownership. Page-local paint
  fragments drain image patterns with their other resources, so negative-z
  backgrounds and replaced images cannot remain as orphaned source-page
  resources.
- Auto-height absolutely positioned boxes measure descendant forced page breaks
  as continuous crossed page areas, so their page span and page-margin counters
  reflect the actual fragment sequence rather than a synthetic measurement
  cursor.
- Absolutely positioned descendants inside inline floats under a single-line
  positioned or transformed inline ancestor resolve explicit insets against
  that inline ancestor's generated padding-box containing block instead of the
  outer block or page, including edge-only inline fragments and nested
  positioned inline ancestors with identical styles.
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
- Implement projective perspective, preserve-3D depth ordering, SVG
  stroke/view transform boxes, and transform animations before treating the
  CSS Transforms WPT directory as broadly covered.
- Audit sticky positioning beyond current static-page behavior once scrolling
  and viewport-relative sticky constraints are represented more fully.
- Continue expanding CSS Positioned Layout Level 3 coverage for edge cases in
  static-position rectangles and writing-mode-specific inset resolution.

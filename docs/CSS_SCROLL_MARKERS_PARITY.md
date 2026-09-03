# CSS Scroll Markers parity

This document tracks Spindrift's static implementation of the current CSS Overflow
Level 5 scroll-marker draft. The relevant source is
<https://drafts.csswg.org/css-overflow-5/#scroll-markers>; the draft is not a
stable Recommendation, so implementation is deliberately pinned to that
section's current terminology.

## Implemented static foundation

- `scroll-target-group` and `scroll-marker-group` have typed computed values,
  CSS-wide keyword handling, and cascade participation.
- `::scroll-marker` and `::scroll-marker-group` use normal pseudo-element
  cascading and generated-content evaluation. A marker is generated only when
  its final `content` is not `none`.
- Enabled scroll containers collect eligible descendant automatic markers in
  tree order. Nested scroll containers form a collection boundary, so their
  markers cannot become members of an outer group's list.
- `::scroll-marker-group` is emitted as an external sibling before or after
  its owning scroll container and therefore participates in the real parent
  flex/grid/table normalization path. Marker boxes remain children of that
  group while retaining their source element for generated content and counter
  evaluation.
- The UA sheet applies `contain: size !important` to generated groups and a
  link-like baseline presentation to automatic markers.
- `:target-current`, `:target-before`, and `:target-after` parse in selector
  positions following `::scroll-marker`. In static fragment navigation,
  `:target-current` matches the document-indicated target; the other relative
  states require the unimplemented geometry pass below.

## Remaining work

- Build immutable render-scoped topology for explicit `scroll-target-group`
  anchors, full flat-tree ordering, `display: contents`, root placement, and
  same-document HTML/SVG anchor resolution.
- Extract scroll-navigation geometry from `scroll_snap` and perform the
  CSS Overflow 5 active-marker selection algorithm, including writing modes,
  nested scrollers, tie-breaking, and unreachable-target redistribution.
- Re-cascade marker state through a bounded convergence driver. The current
  one-pass static build does not re-layout when target-state rules change
  geometry.
- Attach automatic marker paint to same-document PDF destinations. The current
  generated marker boxes are visual content only; `links` and `tabs` modes are
  retained in the computed model but do not create PDF tab/focus semantics.
- Run the listed non-script scroll-marker WPT reftests and add PDF destination
  fixtures once topology and active-state selection are available.

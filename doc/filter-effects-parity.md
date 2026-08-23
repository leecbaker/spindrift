# Filter Effects parity

For CSS `filter`, Quire proves visual identity for the exact lowering's
identity result (`grayscale(0)`, `saturate(1)`, `brightness(1)`, and
`opacity(1)`) and for `blur(0)`, `contrast(1)`, `invert(0)`, `sepia(0)`, and
whole-turn `hue-rotate()` values. These non-`none` filters retain their CSS
stacking behavior, but do not create a redundant PDF transparency group.

Quire has no raster SVG filter backend. Most SVG filter graphs are therefore
suppressed rather than painted as unfiltered source content, because a
partial vector substitute would not be equivalent to the filtered result.

There is one exact vector lowering. Filter Effects requires an
`feDisplacementMap` whose `in2` is tainted to act as a pass-through. Quire
proves that the complete normalized filter sequence ends in that exact
`SourceGraphic` result, then paints the original vector scene clipped to the
intersection of the filter region and every exact pass-through primitive
subregion. The rectangular clip follows the normal SVG-to-paint transform;
it is not replaced by an axis-aligned output bound.

Taint provenance crosses the inline-SVG presentation bridge as a typed
computed `flood-color` or `lighting-color` value. A concrete RGBA value is
provided to `usvg`, while the separate `currentColor` dependency records the
Filter Effects taint rule. External SVG styles, unknown image taint state,
masks, malformed inputs, and all non-identity graphs remain on the unsupported
path.

Relevant specifications: [Filter Effects §9](https://drafts.csswg.org/filter-effects/#FilterPrimitiveSubregion), [§15.1–15.2](https://drafts.csswg.org/filter-effects/#tainted-filter-primitives), and [CSS Color 4 currentColor](https://drafts.csswg.org/css-color-4/#currentcolor-color).

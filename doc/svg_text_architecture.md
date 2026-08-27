# SVG text architecture

## Current implementation status

Quire now retains normalized inline SVG `<text>`/`<tspan>` chunks and shapes
their visible spans through the owning document's
`FontSystem`. The resulting glyph runs are recorded as ordinary PDF text, so
they share document-font IDs, embedding, subsetting, and ToUnicode maps with
HTML text. Native output retains SVG affine transforms, `text-anchor`,
`textLength`/`lengthAdjust`, normalized letter/word spacing, and character
`dx`/`dy` positioning in the shared glyph stream and PDF text matrix. The
same route is used for inline SVG roots and replaced SVG images.

SVG `dominant-baseline`, `alignment-baseline`, and inherited numeric,
`super`, and `sub` `baseline-shift` values adapt to the existing document
font baseline-table APIs. This reuses the selected font's BASE data,
variation instance, and metric synthesis instead of keeping an SVG-specific
baseline-metric path. SVG text shadows are retained as normalized style data,
then replayed from the already-shaped glyph outlines; blurred shadows pass
through a bounded `tiny-skia` raster surface and the source text remains the
sole selectable PDF text item.

When an inline SVG subtree is styled by the host HTML/CSS document, Quire
records a typed typography projection for every SVG scene element after the
ordinary host cascade has resolved SVG presentation attributes, embedded SVG
styles, inheritance, and `!important`. For shaping, the serialized SVG carries
only an opaque key; the pinned parser retains that key on each normalized text
span and Quire reconstructs its shared-font `ComputedStyle` from the private
table. This includes font matching, variation and OpenType feature controls, font
synthesis, palette, language, and bidi/writing settings. `usvg` remains the
authority for SVG geometry and chunk construction, including the font-size and
spacing values it needs for `em` coordinates and `textLength` normalization.

Solid-fill spans are native PDF text. Gradients and text strokes lower the
same selected Quire glyphs to vector outlines; per-character SVG `rotate`
also lowers to outlines because a PDF text run cannot rotate one character
without rotating later advances. Every outline fallback is emitted inside one
tagged PDF `/ActualText` span for its source text, never as a duplicate
invisible text layer. SVG stroke cap/join/miter/dash and fill/stroke paint
order are retained by the vector path realization.

The initial `<textPath>` adapter samples the retained normalized path by arc
length and tangent after Quire shaping, then uses the same semantic-outline
fallback. It covers ordinary contour placement and `startOffset`; complete
text-path character-list, baseline, side/method, and path-layout edge cases
remain outstanding.

Quire uses a pinned parser-only `usvg` fork. Its one documented patch retains
normalized text before upstream font lookup/layout, so the parser neither
loads system fonts nor produces a competing glyph layout. See
[`third_party/usvg/QUIRE_PATCHES.md`](../third_party/usvg/QUIRE_PATCHES.md).
An upstream retained-text API remains desirable so this narrow fork can be
removed.

Outstanding work includes complete character-list edge cases, complete SVG
vertical-writing coordinate and baseline behavior, complete text-path layout, exact
SVG pattern text paint, SVG text processing that changes parser chunk
construction (`text-transform`, `white-space`, `tab-size`, and line-breaking/
line-height behavior), `@font-face` within external SVG documents,
mixed-orientation/path-text decoration geometry, masks,
filters, and an exact
offscreen blur/effects compositor.

## Decision

SVG text and HTML text use one Quire font-selection, shaping, document-font,
and PDF-font-subsetting pipeline. They do not use one layout algorithm.

### Coordinate-system boundary

SVG text positioning and font glyph geometry cross the SVG/PDF coordinate
boundary through different typed transforms. `SvgTextPosition` and
`SvgTextUserDisplacement` remain in the current SVG element's y-down user
space until the text adapter maps their origins through
`SvgTextUserToPaintTransform`. Shaped glyphs remain in `TextRunSpace`, whose
font/PDF convention is y-up, and cross through `SvgGlyphToPaintTransform`.
That latter transform composes the SVG viewport/element transform with one
glyph-local Y reflection. Consequently an ordinary inline SVG retains SVG's
top-left, y-down positioning while its PDF glyphs remain upright; an authored
SVG reflection still affects glyphs exactly once.

This follows SVG 2's requirement that the initial SVG user space be
top-left/y-down while text is upright, and that viewport and element
transforms apply to descendant text.
<https://www.w3.org/TR/SVG2/coords.html#InitialCoordinateSystem>
<https://www.w3.org/TR/SVG2/coords.html#ComputingAViewportsTransform>

HTML inline layout and SVG text layout have incompatible source geometry:
HTML constructs line boxes, while SVG text has independently positioned text
chunks, `x`/`y`/`dx`/`dy`/`rotate` character lists, `text-anchor`,
`textLength`, baseline controls, and text on a path. An SVG-specific layout
adapter must therefore resolve those semantics, then submit each text span to
the shared font system.

SVG text is normally emitted as native PDF text so that it remains selectable,
searchable, extractable, and uses the same embedded and subsetted font program
as HTML text. The fallback is glyph-outline lowering, derived from the same
already selected and shaped glyph stream, when the SVG result cannot be
represented faithfully by a PDF text operation.

This follows SVG 2's model: text is graphical content subject to SVG
transforms, painting, clipping, masking, and compositing, while SVG 2 relies
on CSS typography for much of its text layout.

Relevant specifications:

- [SVG 2 text](https://www.w3.org/TR/SVG2/text.html)
- [SVG 2 painting](https://www.w3.org/TR/SVG2/painting.html)
- [CSS Fonts Level 4 font matching](https://www.w3.org/TR/css-fonts-4/#font-matching-algorithm)
- [CSS Text Level 3 text processing](https://www.w3.org/TR/css-text-3/#text-processing-order)
- ISO 32000-2, sections 9.4 (text), 9.10.3 (ToUnicode CMaps), and 14.9.4.4
  (ActualText).

## Boundaries

```text
HTML inline layout  -- HTML text adapter --+
                                           |
SVG text layout     -- SVG text adapter ---+--> FontSystem
                                                DocumentFontRegistry
                                                shaped document-font glyph runs
                                                            |
                                      +---------------------+---------------------+
                                      |                                           |
                            native PDF text                          SVG glyph outlines
                         (preferred, selectable)           (complex-paint/geometry fallback)
```

### Shared font and glyph infrastructure

The shared layer is responsible for:

- CSS/SVG font-family matching, fallback, `@font-face` sources, embedding
  permissions, synthetic faces, variation instances, and OpenType features.
- Shaping into glyph IDs, advances, offsets, source text, and ToUnicode or
  ActualText information.
- Registering exactly one document-local font program/face/variation instance
  for equivalent uses, then subsetting that program once for the PDF.
- Color-glyph and bitmap-glyph handling where the chosen program requires
  non-text paint primitives.

The reusable input is the concrete, source-independent `TextShapingRequest`.
It carries exactly one resolved typography context and line-height into the
document `FontSystem`; HTML adapts its `ComputedStyle` at its existing layout
boundary and SVG adapts its normalized span at its SVG boundary. It has no
HTML line-box or SVG user-coordinate state. The current implementation keeps
the complete resolved style as the concrete typography carrier so existing
font selection, fallback, features, variations, synthesis, and spacing remain
lossless. This is deliberately not a broad `TextStyle` trait or generic
layout API, and it must not become a second SVG font renderer.

The reusable result is a document-font glyph run: selected document-font ID,
glyph IDs, advances, offsets, and exact logical-text mapping. HTML's line-box
record remains HTML-specific; SVG adds its own text-position and transform
information around the shared glyph run.

### SVG-specific text layout

The SVG adapter owns the operations that are not CSS inline-box layout:

- SVG white-space and text-chunk construction.
- Character-addressable absolute and relative positions, rotation, and
  `text-anchor`. Normalized absolute `x`/`y` chunks and relative `dx`/`dy`
  are implemented. Relative lists remain in SVG user axes through vertical
  sideways-run matrices; mixed-list and all chunk-boundary edge cases remain
  pending.
- `textLength`/`lengthAdjust`, baseline properties, vertical writing, and
  text-on-path placement. `vertical-rl` and `vertical-lr` remain distinct in
  the retained tree and use Quire's vertical glyph orientation/PDF matrices.
  The inherited SVG `text-orientation` values `mixed`, `upright`, and
  `sideways` are normalized in the same retained node and mapped to Quire's
  orientation-aware shaping request. Vertical `textLength`, both
  `lengthAdjust` modes, and `text-anchor` use the same typed vertical inline
  axis for pen progression; upright glyphs scale their vertical text-space
  axis rather than their horizontal one. Complete mixed-run vertical anchors
  and baseline behavior remain pending. Horizontal baseline selection and
  inherited baseline-shift are implemented.
- SVG text decorations and span paint order. Underline, overline, and
  line-through use the selected document font's decoration metrics and SVG
  fill/stroke paint. Upright vertical decorations follow the vertical inline
  axis; mixed-orientation and path-text decoration geometry remains pending.
- Mapping SVG local user coordinates through the existing typed SVG-to-paint
  transform boundary.

The SVG layer must retain semantic text and placement until it decides how to
paint. Do not flatten it into paths at parse time.

## Paint realization

Native PDF text is used only when all of the following are representable by
Quire's PDF text emitter:

- The chosen font program is embeddable as a PDF outline font.
- The SVG paint is a supported solid text fill and has no stroke, gradient,
  per-character rotation, or other paint-server behavior requiring outlines.
- The positioned glyph transforms can be represented without changing glyph
  appearance or extraction order.
- The run does not require a color/bitmap-glyph realization that is already
  represented by paths or images.

All other cases lower the same shaped glyphs to paths/images inside the SVG
paint group. This includes gradient text, stroked text, and per-glyph rotation
that the PDF text emitter cannot express faithfully. Pattern text remains
unsupported rather than being approximated. The fallback must not reselect or
reshape fonts through another engine.

Text-shadow uses this same fallback rule even when the source span is native
PDF text. Its decorative replay is made of ordinary glyph-outline paths when
unblurred, or a bounded alpha-bearing raster image when blurred; neither gets
`ActualText`. The original text run is the only semantic/extractable copy.
The raster path consumes the exact selected Quire outlines, is capped at 16
million pixels, works in premultiplied-alpha sRGB, and has no font lookup or
shaping dependency. It is the first client of the approved offscreen-effects
boundary. A filtered solid-path/text subtree supports one
`feGaussianBlur(in="SourceGraphic")`, `feDropShadow(in="SourceGraphic")`, `feOffset(in="SourceGraphic")`, or
`feColorMatrix(in="SourceGraphic")`/`feComponentTransfer(in="SourceGraphic")`
or `feMorphology(in="SourceGraphic")` with an explicit `userSpaceOnUse`
filter region. It also supports a strictly linear sequence of
`feGaussianBlur`, `feDropShadow`, `feOffset`, `feColorMatrix`, `feComponentTransfer`,
`feMorphology`, and `feConvolveMatrix` primitives when every primitive consumes the immediately
previous named result; it is emitted as that same raster image with one
`/ActualText` string in SVG source order.
The common binary `feFlood` followed by `feComposite operator="in"` with
`SourceAlpha` is also exact: it recolors the same premultiplied shaped-glyph
coverage without creating a second text or font-rendering path.
The canonical named blur/offset/flood/composite/merge shadow graph is likewise
recognized exactly and lowered to `feDropShadow`'s single source-surface
operation; arbitrary `feComposite` and `feMerge` graphs remain pending.
An explicit named linear result may also be composited with immutable
`SourceGraphic` as `in2`; both surfaces stay in the bounded compositor until
the final image encoding step.
The same boundary derives `SourceAlpha` directly from the immutable source
coverage for a named result's `in2`, without rerasterizing glyphs.
Color matrices and component-transfer functions correctly unpremultiply and
re-premultiply pixels and observe
`color-interpolation-filters`; convolution matrices do the same while honoring
their target point, divisor, bias, `edgeMode`, and `preserveAlpha`. Flat
solid-path alpha/luminance masks with an
explicit `userSpaceOnUse` region now use the same bounded raster boundary,
including shaped SVG text with one `/ActualText` replacement. Nested,
paint-server/image, and object-bounding-box masks, object-bounding-box filters
on retained text, and the remaining filter graph primitives are still pending
rather than routed through an approximation.

Outline fallback is visual fallback, not permission to drop semantics. When
the tagged-PDF path can represent it, retain the source string as accessible
text (for example through ActualText) rather than emitting an invisible,
duplicate text layer that would distort selection and extraction.

## `usvg`'s role

`usvg` remains useful for SVG XML parsing, CSS/presentation normalization, and
non-text SVG geometry. Quire's pinned fork uses its text feature only as an
input-normalization aid; it does not invoke upstream text layout or outline
flattening.

It must not be the authoritative visible-text shaper in the completed design:
its text feature owns a separate font database and shaping implementation.
Making that a peer rendering path would permit different font fallback,
variation selection, glyph IDs, and PDF font programs from Quire's HTML text.

The desired parser boundary is retained normalized SVG text content and style,
not `usvg`'s already flattened glyph paths. If upstream cannot expose that
boundary, prefer an upstream extension or a narrowly maintained adapter over
a bespoke SVG parser.

## Rust representation guidance

Use concrete semantic records at the font boundary, such as
`TextShapingRequest` and a document-font glyph-run record. Broad generic
traits such as `TextLayout<T: TextStyle>` are not appropriate: they would hide
the real difference between HTML line layout and SVG text placement, complicate
stateful font caches, and encourage an invalid one-algorithm abstraction.

Rust type parameters are useful for geometric invariants. A glyph placement
may be parameterized by a coordinate-space marker, or represented with
distinct concrete SVG-user-space and paint-space types, so an SVG text
position cannot accidentally be used as a page paint point. Follow the
existing explicit SVG-to-paint conversion boundary and the semantic-unit
guidance in `AGENTS.md`.

## Implementation order

1. Retain normalized SVG text nodes and identify the parser boundary needed to
   do so without a bespoke parser.
2. Extract the source-independent request/result layer from `FontSystem` and
   `DocumentFontRegistry`, keeping HTML behavior unchanged.
3. Add typed SVG text/chunk/glyph-placement scene records and shape them via
   the shared system.
4. Teach SVG scene recording and PDF emission to carry simple native SVG text.
5. Add outline/image realization from the same glyph stream for unsupported
   SVG paint and geometry.
6. Add tagging/ActualText behavior for fallback content, then expand SVG text
   conformance coverage.

At every stage, test that identical HTML and SVG font requests choose the same
document font program and variation instance, and compare SVG visual output
against an independent renderer for SVG-specific positioning behavior.

The current host-CSS bridge serializes a computed `text-shadow` and changed
font-family/font-size/font-weight/font-style/font-stretch as SVG presentation
values, plus `direction` and `unicode-bidi`, for inline SVG descendants.
It is a serialization boundary, not a second cascade: Quire still owns the
host CSS cascade and document font registry. It must expand to the remaining
SVG text-relevant CSS properties before host styling can be considered
complete.

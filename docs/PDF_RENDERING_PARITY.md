# PDF Rendering Parity

Last updated: 2026-07-18

This note tracks PDF content-stream rendering behavior that is separate from
font embedding and from CSS layout conformance. Visual output is driven by the
paint tree produced by layout. Typed primitive stores remain page-local backing
storage for paint-tree operation nodes, not an alternate paint-order or PDF
serialization representation.

## Current Behavior

- HTML/CSS rendering records a paint tree with stacking contexts, paint bands,
  clips, transforms, opacity groups, blend modes, and links before PDF
  serialization.
- Non-stacking paint effect scopes represent clips that must be emitted into
  PDF graphics state without changing CSS paint-band ordering.
- PDF serialization uses the paint tree as the sole authoritative paint order.
- Consecutive compatible fill rectangles in the same paint-tree band and
  stacking context are merged before emission. This is limited to same-fill
  axis-aligned rectangles without stroke, radius, or alpha, and does not cross
  text, image, path, stroke, rounded-rectangle, link, nested stacking-context,
  clip, transform, opacity, or blend boundaries.
- After a rectangle batch is flushed, PDF serialization always queues the next
  rectangle in every build profile; debug assertions validate that invariant
  without controlling the required mutation. This preserves the first fill in
  a new batch, including flex-item backgrounds in
  `css/css-flexbox/flex-align-content-end.html`.
- Fully covered opaque rectangle underpaint can be omitted by the paint-tree
  writer. This preserves final
  compositing while avoiding PDF viewer rasterization artifacts where
  antialiasing at a later fill boundary samples hidden colors underneath.
- Repeated raster CSS background layers are emitted as native PDF tiling
  patterns instead of expanded per-tile image draws. The tiling path preserves
  used CSS tile size, repeat steps for `repeat`, `space`, `round`, `repeat-x`,
  and `repeat-y`, image interpolation, and rectangular or rounded background
  clipping while sharing the tile image XObject through the normal image
  resource planner.
- Repeated URL SVG backgrounds use the same resolved-tile geometry, but paint
  their vector cell once in a page-local Form XObject and invoke it from a
  `/PatternType 1` tiling pattern. This avoids page-content growth with the
  number of CSS tiles while keeping the outer CSS background clip outside the
  reusable cell.
- SVG linear and radial gradients retain their stop geometry through the paint
  tree. Padded endpoint colors and coincident hard stops are emitted as PDF
  stitching functions, and the paint-server matrix is composed with the SVG
  path viewport transform so shading coordinates match transformed geometry.
- Uniform CSS linear/radial background gradients emit ordinary clipped solid
  paths; a fully repeating uniform layer covers its resolved positioning area
  with one path rather than enumerating tiles. CSS linear/radial gradients,
  including repeating color lines, transition hints, hard stops, and alpha,
  emit `/PatternType 2` axial/radial shadings. Repeating color lines use one
  periodic Type 4 calculator function per line, with a paired Type 4 soft-mask
  function for varying opacity, so internal repeat count does not grow the
  PDF. Zero-period and physically unresolvable repeats use their CSS
  gradient-average color.
  Repeated background layers use a
  `/PatternType 1` cell; radial ellipses use the shading matrix rather than a raster
  approximation. Generated-image raster fallbacks are materialized
  at the resolved CSS tile size and both RGB and alpha streams use
  `/FlateDecode`.
- Page content, Form XObjects, tiling patterns, embedded font programs,
  ToUnicode CMaps, CIDSets, XMP metadata, and decoded raster-image streams use
  `/FlateDecode` by default. Decoded raster image and soft-mask streams retain
  their source 8- or 16-bpc integer precision. Eligible unchanged 8-bit RGB JPEG sources retain
  their original `/DCTDecode` streams instead; PDF/A limits passthrough to
  untagged sRGB sources. Cropped, oriented, generated, and tagged or
  color-converted images remain decoded samples. `PdfCompression::Uncompressed`
  disables the filter for generated streams to support PDF debugging.
- Direct vector fills, strokes, text, native linear/radial shadings, and
  generated CSS-gradient images use generated ICCBased color-space resources
  for retained CSS Color 4 spaces. PDF/A converts vector paint and generated
  gradients to tagged sRGB and supplies an sRGB `/OutputIntent`. Ordinary PDF
  preserves embedded RGB ICC profiles from PNG/JPEG images; PDF/A transforms
  those samples to sRGB. SVG image decoding and CMYK/non-RGB source profiles
  remain color-management gaps.
- Successful PDF output only references non-empty, embeddable font programs.
  When compact subsetting is unsuitable but outline embedding is permitted,
  Quire embeds the compatible full program; otherwise serialization reports a
  font-embedding error rather than emitting a rejected font resource.
- Empty normal-flow and out-of-flow geometry does not materialize blank PDF
  pages. Backgrounds, borders, outlines, descendant paint, anchors, bookmarks,
  and explicit page ownership still retain each affected page fragment.
- Transparency Form XObjects reserve unique `/FmN` names before recursively
  serializing their content. Their `/XObject` resources contain only the Forms
  directly invoked by that Form, while page resources retain the complete
  page-level Form set.
- Replayed flex and grid items apply their principal compositing and stacking
  effects once at the item context. Nested descendants retain independent
  effect scopes, and formatting-context overflow clipping remains content-only.
- Page geometry and paint primitives remain renderer-private.
  External construction through mutable primitive vectors is not a supported
  API goal.

## Remaining Work

- Make Quire-authored ICC profile resources byte-deterministic. Direct
  `moxcms` 0.9 currently ignores `ColorProfile::creation_date_time` and writes
  the current UTC time during encoding. Adopt an upstream or pinned moxcms fix
  that preserves the supplied date, assign Quire's built-in profiles a
  source-controlled fixed date, then add ICC/PDF byte-identity coverage and
  re-evaluate the four `css3-text-line-break-opclns` WPTs.
- Add visual comparison coverage for representative paint-tree output with
  clipping, transforms, opacity groups, blends, images, vector paths, and
  text, using the repo-local PDF comparison workflow.
- Expand PDF/A and PDF/UA validation beyond current structural hooks, including
  tagged PDF structure, output intents, conformance metadata, and external
  validator runs.

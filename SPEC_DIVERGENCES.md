# reasyprint Spec Divergences

Last updated: 2026-06-26

This document tracks known places where `reasyprint` is broken, incomplete, or
otherwise needs future work to match the relevant CSS, HTML, SVG, and PDF
specifications. Resolved behavior and parity wins belong in `PROGRESS.md` or
the parity notes, not here.

Treat every entry as future work unless it is removed from this file.

Primary references:

- HTML rendering and presentational hints:
  <https://html.spec.whatwg.org/multipage/rendering.html>
- CSS 2.2 visual formatting model, tables, generated content, and paged media:
  <https://www.w3.org/TR/CSS22/>
- CSS Display Module Level 3:
  <https://www.w3.org/TR/css-display-3/>
- CSS Lists and Counters Module Level 3:
  <https://www.w3.org/TR/css-lists-3/>
- CSS Fragmentation Module Level 3:
  <https://www.w3.org/TR/css-break-3/>
- CSS Text Module Level 3:
  <https://www.w3.org/TR/css-text-3/>
- CSS Fonts Module Level 4:
  <https://www.w3.org/TR/css-fonts-4/>
- CSS Backgrounds and Borders Module Level 3:
  <https://www.w3.org/TR/css-backgrounds-3/>
- CSS Box Model Module Level 4:
  <https://drafts.csswg.org/css-box-4/>
- CSS Paged Media Module Level 3:
  <https://www.w3.org/TR/css-page-3/>
- SVG 2:
  <https://www.w3.org/TR/SVG2/>
- PDF/A:
  <https://www.iso.org/standard/79428.html>

## Open Divergences

### HTML User-Agent Stylesheet

- Spec area: HTML rendering defaults, presentational hints, CSS Cascade,
  CSS Generated Content, CSS Lists, CSS Tables.
- Divergence: HTML generated-content defaults for `br` and `wbr` now route
  through pseudo-element content, but broader edge cases around generated
  content, presentational hints, and legacy HTML defaults have not been
  exhaustively audited against the WHATWG rendering section.
- Divergence: HTML presentational hints have not been exhaustively audited
  against the WHATWG rendering section.
- Needed work: broaden WPT coverage for HTML rendering defaults that combine
  generated content, author overrides, counters, text capture, and legacy
  presentational hints.
- Needed work: audit presentational hints element-by-element, including table
  and legacy attributes, and either implement the spec behavior or record a
  narrower divergence.

### Lists and Markers

- Spec area: CSS Lists, CSS Counter Styles, generated content, tagged PDF.
- Divergence: SVG/vector `list-style-image` values are not implemented.
- Divergence: image markers do not yet implement full CSS image marker sizing,
  baseline, and alignment semantics.
- Divergence: nested counter behavior across fragmentation and re-layout still
  has unverified edge cases.
- Divergence: vertical writing-mode outside marker geometry is incomplete.
- Divergence: PDF output paints marker glyphs as normal text runs rather than
  exposing tagged marker semantics.
- Divergence: custom counter-style `speak-as` is parsed but has no effect
  because speech output is not generated.
- Needed work: implement image marker sizing/alignment from CSS Lists, extend
  marker geometry to vertical writing modes, verify counters through
  fragmentation, and map markers into tagged PDF when accessibility support is
  added.

### CSS Display Model

- Spec area: CSS Display and CSS Lists.
- Divergence: `run-in`, ruby display values, and several authored
  table-internal values remain unsupported or only covered in table-specific
  paths.
- Divergence: table-internal display fixup and anonymous box construction are
  not fully spec-complete for every malformed or unusual authored tree.
- Needed work: expand parsing and layout for the remaining CSS Display values,
  including layout participation, box tree fixup, pagination behavior, and WPT
  coverage.

### CSS Content

- Spec area: CSS Content Level 3, generated content, accessibility metadata.
- Divergence: unsupported CSS Images values used in `content`, such as
  gradients and image-set forms, remain under the broader CSS Images gap.
- Divergence: generated image alt text is not emitted into tagged PDF/PDF-UA
  structure.
- Divergence: broader target cross-reference behavior outside the currently
  supported text-capture paths remains incomplete.
- Divergence: `leader()` and generated content layout still need broader WPT
  coverage for edge cases involving fragmentation, line fitting, and replaced
  generated content.
- Needed work: share complete CSS Images support with generated content,
  connect generated/replacement alt text to tagged PDF, and finish target
  cross-reference evaluation for all generated-content contexts.

### CSS Box Model `margin-trim`

- Spec area: CSS Box Model Level 4.
- Divergence: `margin-trim: block-end` is not implemented.
- Divergence: inline-axis trimming is not implemented.
- Divergence: trim application is not fully writing-mode aware.
- Divergence: interactions outside normal block formatting contexts need
  spec-aligned coverage.
- Needed work: implement block-end and inline-axis trimming, logical side
  mapping, fragmentation interactions, and tests for non-block formatting
  contexts.

### Inline Formatting and CSS Text

- Spec area: CSS Inline Layout, CSS Text, CSS Text Decoration.
- Divergence: `::first-line` and `::first-letter` parse, cascade, and affect
  basic inline text painting with allowed-property filtering, but nested
  first-line inheritance, drop-cap layout, generated-content first-letter
  interactions, fragmentation behavior, and the complete CSS Pseudo-Elements
  first-letter typographic-unit model are incomplete.
- Divergence: some diagnostic grouping and fragmented-context estimates still
  diverge from final graph-selected line layout in less common nested
  formatting contexts.
- Divergence: `word-break: keep-all` min-content behavior across mixed
  CJK/Latin text remains suspect in less common contexts.
- Divergence: nested flex/table layout inside atomic inline boxes is partial.
- Divergence: full `vertical-align` behavior and exact browser-level mixed-run
  pixel positioning are incomplete.
- Divergence: `text-justify: inter-character` and vertical/complex-script
  justification are not fully script-sensitive.
- Divergence: `text-autospace` is incomplete for punctuation-specific spacing,
  `replace` semantics, vertical text-orientation interactions, `text-spacing`
  shorthand integration, and dynamic DOM cases from the CSS Text Level 4 draft.
- Divergence: `hanging-punctuation` is incomplete for vertical writing and
  complex bidi fragment-edge placement.
- Divergence: vertical writing underline side placement is not fully
  implemented.
- Divergence: text emphasis marks are painted as independent text runs but are
  not yet laid out as ruby-positioned annotation fragments; full
  ruby collision handling, vertical line boxes, and mixed-script
  typographic-unit placement remain incomplete.
- Divergence: CSS Text Decoration `text-shadow` supports Level 3 zero-blur
  vector painting and bounded translucent vector replay for blurred shadows,
  but blur/spread/inset shadows are not yet rasterized as grouped
  text/decorator alpha masks.
- Divergence: `text-decoration-skip-box` and complete
  `text-decoration-skip-self` behavior across atomic inline boxes, inline-block
  fragments, generated content, and fragmented lines are incomplete.
- Divergence: CSS `text-transform: math-auto` and remaining locale-specific
  CSS Text Level 4 transforms are not implemented.
- Divergence: auto hyphenation, emergency wrapping, and intrinsic sizing still
  have residual discrepancies in some styled inline and fragmented contexts.
- Needed work: drive every intrinsic-sizing, fragmentation, and paint path from
  the same durable inline fragments; then complete the script-,
  writing-mode-, and language-sensitive CSS Text features above.

### Text Shaping and Font Selection

- Spec area: CSS Fonts, CSS Text, OpenType shaping, PDF font embedding.
- Divergence: synthetic bold and oblique are not applied to emitted PDF glyph
  outlines.
- Divergence: variable-font axis instantiation and descriptor range matching
  are incomplete.
- Divergence: `unicode-range` handling is incomplete.
- Divergence: per-glyph synthetic fallback for missing small-caps and
  petite-caps feature glyphs is not implemented.
- Divergence: CFF/OpenType collection embedding is incomplete.
- Divergence: baseline and line-box metric mapping still diverges from
  WeasyPrint/Pango in some contexts.
- Divergence: residual complex-script shaping mismatches remain, including the
  Arabic presentation-form reference mismatch tracked by WPT shaping cases.
- Needed work: add focused tests and implementation for `unicode-range`, font
  synthesis, variable fonts, missing-glyph fallback, CFF/collection embedding,
  per-glyph caps fallback, and residual complex-script mismatches.

### Tables

- Spec area: CSS Tables, CSS 2.2 table layout, HTML table semantics.
- Divergence: anonymous table object construction is not complete for all
  malformed edge cases.
- Divergence: rare auto table width edge cases still need broader WPT and
  WeasyPrint coverage, especially complex colspans combined with column
  groups and separated-border spacing.
- Divergence: CSS Overflow 3 behavior for table cells and block descendants is
  incomplete, including clipping, scrollbar painting, and overflow
  propagation.
- Divergence: table fragmentation is incomplete for full cloned decoration
  semantics, flex-specific descendant link handling inside planned nested flex
  fragments, and rare rowspans/collapsed-track border conflict resolution
  across fragment boundaries.
- Divergence: collapsed row/column clipping has remaining edge cases involving
  partial glyph clipping, spanning borders, and fragmented table pieces.
- Divergence: table height distribution and baseline behavior still need
  broader coverage against CSS Tables 3 and WeasyPrint edge cases.
- Needed work: port WeasyPrint table tests incrementally, finish table fixup
  edge cases, extend table sizing coverage for complex spans and column
  groups, and complete cloned decoration and collapsed-border behavior across
  fragmentation.

### Flex Layout and Fragmentation

- Spec area: CSS Flexible Box Layout, CSS Box Alignment, CSS Fragmentation.
- Divergence: `align-content` baseline packing falls back to start-side line
  packing.
- Divergence: some flex reference cases still expose child text/content
  placement mismatches unrelated to alignment keyword parsing.
- Divergence: preserved-newline auto bases and near-exact line fits still rely
  on guards because intrinsic sizing, line breaking, and PDF glyph placement
  use separate measurement paths.
- Divergence: full paged flex fragmentation is not implemented.
- Divergence: vertical-writing baseline sets, multi-line collapsed-item strut
  rewrapping, remaining indefinite percentage cross-size interactions, and
  nested flex edge cases are incomplete.
- Divergence: exact child-height estimates for flex items with column or
  multicolumn descendants are incomplete.
- Needed work: add flex fragment objects for paged layout, implement the
  remaining Box Alignment cases, and remove duplicate measurement paths that
  cause intrinsic-size and final-layout disagreement.

### Float Layout

- Spec area: CSS 2.2 floats and block formatting contexts.
- Divergence: complex float fragmentation interactions still need broader
  coverage, including anchors/bookmarks inside broken floats, future-page line
  reflow around fragmented float shapes, and interactions with table/replaced
  effect contexts.
- Divergence: float exclusion and clearance behavior inside less common nested
  formatting contexts has not been exhaustively validated against CSS 2.2.
- Needed work: expand WPT float-fragmentation coverage and tighten remaining
  paged float interactions around generated content, anchors, and replaced
  descendants.

### Pagination and Fragmentation

- Spec area: CSS Fragmentation and CSS Paged Media.
- Divergence: pagination is still cursor-oriented rather than driven by
  fragment objects with available block size.
- Divergence: general fragmentainer layout, cloned decorations, multi-pass
  generated content, rare complex table-cell child/effect fragmentation, and
  robust flex fragmentation are incomplete.
- Divergence: full forced/avoid break precedence and break selection remain
  incomplete for cell-internal effect fragmentation, flex/grid fragmentation,
  and other non-block fragment paths.
- Divergence: class A/B/C break handling is not yet uniformly represented
  across block, inline, table, flex, grid, floats, and generated content.
- Needed work: move pagination to durable fragment objects with available
  block-size negotiation, cloned decorations, and uniform break precedence.

### Page Margin Boxes and Generated Content

- Spec area: CSS Paged Media and Generated Content for Paged Media.
- Divergence: named-string boundaries are not yet durable fragments across all
  formatting contexts, especially table and fragmented flex pagination.
- Divergence: `string(..., start)` uses a page-start assignment marker rather
  than inspecting the first generated page fragment's exact border box.
- Divergence: `element(..., start)` has the same exact-fragment inspection gap.
- Divergence: running elements are limited to text-oriented replay and do not
  preserve the actual source box tree, dimensions, backgrounds, borders,
  images, or tables.
- Divergence: full `string-set` content lists still omit image items and
  non-text target fragments.
- Divergence: negative `z-index` page-margin boxes preserve below-document
  paint ordering through primitive prepend replay, but that fallback does not
  yet preserve opacity/overflow effect groups the way normal margin-box
  stacking replay does.
- Divergence: page-name edge cases across table and flex fragmentation remain
  incomplete.
- Divergence: multi-layer page backgrounds and some page background image
  clipping cases remain incomplete.
- Needed work: make named strings and running elements fragment-backed, then
  extend the typed paged-media generated-content pipeline with box-preserving
  running elements, full `string-set` content lists, cross-references, exact
  `start` retrieval, and effect-preserving negative `z-index` replay.

### Bookmarks and PDF Outlines

- Spec area: CSS Generated Content bookmarks and PDF outlines.
- Divergence: bookmark labels do not support full generated-content list
  evaluation.
- Divergence: pseudo-element label content, counters, named strings, and
  transformed bookmark coordinates are incomplete.
- Needed work: implement full bookmark label evaluation and port the remaining
  WeasyPrint bookmark tests.

### Borders

- Spec area: CSS Backgrounds and Borders, CSS Color, CSS Images,
  fragmentation.
- Divergence: CSS Color parsing does not cover all color functions or
  non-sRGB color spaces.
- Divergence: rounded dashed and dotted borders do not yet have exact
  phase distribution along curved corner arcs.
- Divergence: antialias-equivalent side transitions and full corner conflict
  behavior remain incomplete.
- Divergence: border images do not support all CSS image sources, including
  gradients and SVG.
- Divergence: border-image repeat centering and partial-tile behavior are not
  exact at WPT level.
- Divergence: decoration fragmentation for borders is incomplete.
- Needed work: finish the standardized border model described in
  `BORDER_ARCHITECTURE.md`, including color, rounded stroke, border-image, and
  fragmented-decoration coverage.

### Backgrounds, Images, and SVG

- Spec area: CSS Backgrounds and Borders, CSS Images, SVG, compositing.
- Divergence: general SVG parsing and rendering are incomplete.
- Divergence: CSS Images behavior remains incomplete for object sizing,
  fallback, transforms, opacity, compositing, and broader image source types.
- Divergence: multi-layer backgrounds, full clipping/origin integration,
  interpolated and angled gradients, corner gradients, and percentage or
  omitted gradient stop positions are incomplete.
- Needed work: integrate a richer image/SVG rendering path and expand WPT
  coverage for CSS Images, backgrounds, SVG, transforms, opacity, and
  compositing.

### Positioning and Stacking

- Spec area: CSS Positioned Layout, CSS 2.2 Appendix E painting order,
  transforms, fragmentation.
- Divergence: CSS 2.2 Appendix E painting order is incomplete for some
  fragmented and nested formatting-context edge cases, including rare spanning
  table decoration/border conflicts and flex-specific nested descendant links.
- Divergence: opacity, transforms, overflow clips, links, and effect groups are
  incomplete for some fragmented stacking-context combinations.
- Divergence: transformed containing-block behavior is incomplete in less
  common formatting-context and fragmentation cases.
- Divergence: exact static-position placeholders are incomplete for multiline
  inline, table, and other remaining formatting-context cases.
- Divergence: containing block rules are incomplete for positioned,
  transformed, table, and fragmented contexts.
- Divergence: positioned fragmentation behavior is incomplete.
- Needed work: finish oversized/spanning table fragmentation and remaining
  fragmented effect paint contexts so table pieces, normal-flow sibling overlap,
  and nested stack levels flatten exactly from the durable tree before final PDF
  emission.

### Units and Value Computation

- Spec area: CSS Values and Units, CSS Cascade computed-value processing.
- Divergence: container query units (`cqw`, `cqh`, `cqi`, `cqb`, `cqmin`,
  `cqmax`) are not implemented, and additional font/line metric units such as
  `ex`, `cap`, `ic`, `lh`, and `rlh` still need property-wide support.
- Divergence: mixed length/percentage `min()`, `max()`, and `clamp()`
  comparisons that depend on a layout basis remain incomplete.
- Divergence: many property-specific keywords and computed-value models remain
  incomplete.
- Divergence: multi-layer background lists and full generated image values are
  incomplete.
- Divergence: invalid-at-computed-value-time behavior is incomplete for
  not-yet-modeled shorthands and cross-property dependencies.
- Divergence: property dependency invalidation beyond exact property names is
  incomplete.
- Divergence: `ComputedStyle` still carries some layout-oriented used-value
  projections that should eventually move to a layout-only used-style cache.
- Needed work: broaden typed specified/computed value representation, preserve
  source-unit details where required by the specs, and migrate remaining
  layout projections out of computed style.

### CSS Cascade and Selectors

- Spec area: CSS Cascade, Selectors, CSS Nesting, CSS Conditional Rules,
  Media Queries.
- Divergence: CSS-wide defaulting is incomplete in page-context structural
  declarations.
- Divergence: Media Queries support does not cover the full feature grammar or
  all context-dependent features.
- Divergence: `@supports` support does not cover all CSS Conditional
  feature-query functions.
- Divergence: form/input pseudo-classes cover local attributes, disabled
  fieldset inheritance, option/optgroup/select disabled propagation, default
  selected option fallback, and simple string/email/url/numeric constraint
  validation, but custom element state, complete selectedness normalization,
  pattern/date/time validation, and full HTML constraint validation remain
  incomplete.
- Divergence: namespaced selector matching is implemented for preserved element
  and attribute namespace URLs, but XML-specific namespace edge cases and the
  legacy no-namespace HTML compatibility behavior need a broader spec audit.
- Divergence: shadow/tree-scoped selectors and unmodeled UI/highlight
  pseudo-elements are not implemented.
- Divergence: CSS nesting is handled by a narrow flattener rather than a full
  nesting implementation.
- Needed work: complete media/supports parsing and replace the narrow nesting
  preprocessor with a full CSS Nesting implementation or library-backed parser
  path.

### PDF Conformance and Accessibility

- Spec area: PDF, PDF/A, PDF/UA, PDF/X, tagged PDF.
- Divergence: output is not tagged, structured, PDF/A, PDF/UA, or PDF/X
  conformant.
- Divergence: conformance metadata, structure trees, semantic roles,
  annotations, color profile handling, and related archival/accessibility
  constraints are not implemented.
- Divergence: the renderer still needs a final decision on whether to adopt
  WeasyPrint's page-level coordinate transform model or an equivalent
  text-emission transform if future samples expose transform-related raster
  deltas.
- Needed work: add conformance modes after layout fidelity stabilizes, starting
  with metadata, color/profile handling, tagging, and structure-tree support.

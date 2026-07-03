# Quire Spec Divergences

This document is the central inventory of known places where `quire` diverges
from the relevant CSS, HTML, SVG, and PDF specifications.

Only unresolved divergences belong here. Do not record implementation history,
recent fixes, parity status, or completed behavior. When behavior changes,
remove or narrow the affected entry instead of adding a progress note.

Each entry should identify the relevant spec area and the specific behavior
that remains non-conformant or insufficiently audited. Prefer W3C, WHATWG, and
PDF specifications as the source of truth; use WeasyPrint only as a
compatibility reference when the relevant spec is ambiguous.

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
- CSS Grid Layout Module Level 1:
  <https://www.w3.org/TR/css-grid-1/>
- CSS Flexible Box Layout Module Level 1:
  <https://www.w3.org/TR/css-flexbox-1/>
- CSS Box Alignment Module Level 3:
  <https://www.w3.org/TR/css-align-3/>
- CSS Writing Modes Level 4:
  <https://www.w3.org/TR/css-writing-modes-4/>
- SVG 2:
  <https://www.w3.org/TR/SVG2/>
- PDF/A:
  <https://www.iso.org/standard/79428.html>

## Open Divergences

### HTML User-Agent Stylesheet

- Spec area: HTML rendering defaults, presentational hints, CSS Cascade, CSS
  Generated Content, CSS Lists, CSS Tables.
- Divergence: HTML generated content defaults, author overrides, counters, text
  capture, and legacy presentational hints have not been exhaustively audited
  against the WHATWG rendering section.
- Divergence: presentational hints need element-by-element auditing, especially
  for table attributes and legacy HTML attributes whose cascade behavior affects
  computed styles.

### Lists and Markers

- Spec area: CSS Lists and Counters, CSS Counter Styles, generated content,
  tagged PDF.
- Divergence: SVG/vector `list-style-image` values are not implemented.
- Divergence: image marker sizing, baseline alignment, and marker box geometry
  do not fully follow CSS Lists.
- Divergence: nested counters across fragmentation and re-layout have
  unverified edge cases.
- Divergence: vertical writing-mode geometry for outside markers is incomplete.
- Divergence: PDF marker output lacks tagged marker semantics for PDF/UA.
- Divergence: custom counter-style `speak-as` has no observable effect because
  speech output is not generated.

### CSS Display Model

- Spec area: CSS Display, CSS Lists, CSS Tables, CSS Ruby.
- Divergence: `run-in` display behavior is unsupported.
- Divergence: ruby display values and ruby formatting context behavior are
  unsupported.
- Divergence: authored table-internal display values are not fully handled
  outside table-specific paths.
- Divergence: table-internal display fixup and anonymous box construction are
  not spec-complete for every malformed or unusual authored tree.
- Divergence: grid display values do not yet cover the full CSS Grid formatting
  context, layout participation, pagination behavior, and box tree fixup.

### CSS Float Layout

- Spec area: CSS 2.2 floats and float-adjacent formatting contexts.
- Divergence: table wrappers, flex/grid roots, and block-level replaced boxes
  need a complete audit against the CSS 2.2 border-box float avoidance rule.
- Divergence: some float-adjacent root placement paths still risk using
  margin-box placement where CSS 2.2 requires border-box collision behavior.

### CSS Writing Modes

- Spec area: CSS Writing Modes, CSS Sizing, CSS Fragmentation.
- Divergence: orthogonal-flow available inline sizing is incomplete for
  scrollport-derived available sizes.
- Divergence: orthogonal-flow available-size negotiation is incomplete across
  fragmentation.
- Divergence: orthogonal available-size behavior for grid containers, table
  cells, absolutely positioned descendants, and deeply nested mixed
  writing-mode flex descendants is not exhaustively audited.

### CSS Grid Layout

- Spec area: CSS Grid Layout Levels 1 and 2, CSS Box Alignment, CSS Writing
  Modes, CSS Sizing, CSS Fragmentation.
- Divergence: full CSS Grid placement behavior is incomplete, including unusual
  `grid`, `grid-template`, and named-line parser edge cases.
- Divergence: full grid track sizing is incomplete, including intrinsic tracks,
  flexible tracks in all sizing contexts, spanning item contributions, and
  auto-repeat behavior beyond simple cases.
- Divergence: full grid intrinsic sizing is incomplete, including min-content
  and max-content contributions from complex grid items and indefinite
  container sizes.
- Divergence: grid baseline alignment and exported baseline synthesis are
  incomplete, including writing-mode-specific self-start/self-end behavior.
- Divergence: grid-aware absolute static positions are incomplete for complex
  placement, intrinsic tracks, flexible tracks, template areas, writing modes,
  and fragmented grids.
- Divergence: grid child collection and anonymous grid item construction need
  broader coverage for generated content, `display: contents`, out-of-flow
  descendants, ordering, and text-node edge cases.
- Divergence: grid fragmentation in paged media is not implemented.
- Divergence: `subgrid` is not implemented.
- Divergence: masonry layout is not implemented.

### CSS Content and Generated Content

- Spec area: CSS Content Level 3, Generated Content for Paged Media, CSS
  Images, accessibility metadata.
- Divergence: generated content cannot use all CSS Images values, including
  conic gradients, SVG image sources, and `image-set()` forms.
- Divergence: generated image alt text is not emitted into tagged PDF/PDF-UA
  structure.
- Divergence: target cross-reference behavior is incomplete outside the
  text-capture subset.
- Divergence: `leader()` and generated content layout need broader conformance
  coverage for unusual fragmentation, target cross-reference, and replaced
  generated-content combinations.

### CSS Box Model `margin-trim`

- Spec area: CSS Box Model Level 4.
- Divergence: `margin-trim: block-end` is not implemented.
- Divergence: inline-axis trimming is not implemented.
- Divergence: trim application is not fully writing-mode aware.
- Divergence: `margin-trim` interactions outside normal block formatting
  contexts are incomplete.
- Divergence: `margin-trim` fragmentation interactions are incomplete.

### CSS Inline `text-box-trim` and `text-box-edge`

- Spec area: CSS Inline Layout Level 3, CSS Fragmentation, CSS Multi-column
  Layout.
- Divergence: `text-box-trim` behavior is incomplete for general multi-column
  containers whose content includes block children, margins, floats,
  positioning, or forced breaks.
- Divergence: `text-box-trim` behavior is incomplete for full vertical
  block-flow placement of block children.
- Divergence: fragmented-container cloned decoration replay for
  `text-box-trim` is incomplete.
- Divergence: `box-decoration-break`-dependent first/last fragment policy is
  incomplete outside collected inline line-sequence fragment paths.
- Divergence: broader fragmentation interactions for `text-box-trim` outside
  collected inline line sequences are incomplete.

### CSS Gaps Decorations

- Spec area: CSS Gaps Level 1, CSS Box Alignment, CSS Grid Layout, CSS
  Flexible Box Layout, CSS Multi-column Layout, CSS Fragmentation.
- Divergence: exact CSS Gaps intersection behavior is incomplete for broader
  empty-area cases.
- Divergence: non-grid complex endpoint classification is incomplete because
  the shared painter lacks a complete layout-mode-owned segment endpoint graph.
- Divergence: paged grid gap decoration behavior is incomplete across
  named-page changes, page-size changes, and complex item fragmentation.
- Divergence: multicolumn fragmentation does not slice gap decorations per
  fragmentainer with full CSS Gaps and CSS Fragmentation behavior.
- Divergence: remaining flex fragmentation edge cases lack exact gap decoration
  clipping and replay semantics.

### Inline Formatting and CSS Text

- Spec area: CSS Inline Layout, CSS Text, CSS Text Decoration, CSS
  Pseudo-Elements.
- Divergence: nested `::first-line` inheritance is incomplete.
- Divergence: drop-cap layout for `::first-letter` is incomplete.
- Divergence: generated-content interactions with `::first-letter` are
  incomplete.
- Divergence: `::first-line` and `::first-letter` fragmentation behavior is
  incomplete.
- Divergence: the full CSS Pseudo-Elements first-letter typographic-unit model
  is incomplete.
- Divergence: nested flex and table layout inside atomic inline boxes is
  partial.
- Divergence: CSS Inline 3 baseline-table precision for ideographic,
  mathematical, hanging, and central baselines uses approximations rather than
  complete baseline tables.
- Divergence: the aligned-subtree model for deeply nested mixed inline runs
  needs broader conformance coverage.
- Divergence: complex-script justification expansion rules are incomplete.
- Divergence: fallback transformed vertical glyph forms for Unicode
  `Vertical_Orientation` classes `Tr` and `Tu` are not implemented when font
  alternates are unavailable or incomplete.
- Divergence: `text-autospace` is incomplete for punctuation-specific spacing,
  `replace` semantics, vertical text-orientation interactions, `text-spacing`
  shorthand integration, and dynamic DOM cases from the CSS Text Level 4 draft.
- Divergence: fallback transformed vertical glyph forms and vertical or deeply
  fragmented complex-bidi fragment-edge placement remain incomplete for
  `hanging-punctuation`.
- Divergence: `text-decoration-skip-box`, complete
  `text-decoration-skip-self` edge cases, decoration collision refinements, and
  rare fragmented vertical decoration cases are incomplete.
- Divergence: text emphasis mark collision handling, vertical line-box
  expansion, and annotation overlap avoidance are incomplete.
- Divergence: CSS Text Decoration `text-shadow` blur, spread, and inset
  shadows are not rasterized as grouped text/decorator alpha masks.
- Divergence: CSS `text-transform: math-auto` is not implemented.
- Divergence: remaining locale-specific CSS Text Level 4 transforms are not
  implemented.
- Divergence: residual CSS Text discrepancies remain in fragmented contexts
  that combine nested formatting contexts, generated inline content,
  page-margin inline content, and complex visual effects.

### Text Shaping and Font Selection

- Spec area: CSS Fonts, CSS Text, OpenType shaping, PDF font embedding.
- Divergence: synthetic bold and oblique are not applied to emitted PDF glyph
  outlines.
- Divergence: variable-font axis instantiation and descriptor range matching
  are incomplete.
- Divergence: `unicode-range` behavior has not been exhaustively audited
  against the full CSS Fonts descriptor matching model, invalid descriptors,
  collections, and variable-font instances.
- Divergence: per-glyph synthetic fallback for missing small-caps and
  petite-caps feature glyphs is not implemented.
- Divergence: PDF font subsetting retains glyph IDs for standalone font
  programs, so generated PDFs can be larger than engines that remap subsets
  densely.
- Divergence: CFF/CFF2 embedding and subsetting are incomplete.
- Divergence: variable-font instance embedding and broad font collection
  coverage are incomplete.
- Divergence: TTC/OTC extraction needs repo-local conformance fixtures beyond
  optional system-font smoke tests.
- Divergence: OS/2 embedding permission failures are not exposed through a
  strict public PDF/A/PDF/UA render option.
- Divergence: baseline and line-box metric mapping can diverge from
  browser/Pango behavior in multi-font and fallback-font runs because Quire
  relies on an em-box content-area policy in contexts CSS 2.2 leaves
  underspecified.
- Divergence: residual complex-script shaping mismatches may remain outside
  known join-control and tatweel boundary cases.

### Tables

- Spec area: CSS Tables, CSS 2.2 table layout, HTML table semantics, CSS
  Overflow, CSS Fragmentation.
- Divergence: anonymous table object construction is incomplete for malformed
  edge cases beyond common fixup cases, authored empty rows, inline wrappers,
  `display: contents` descendants inside table rows, and HTML span parsing.
- Divergence: rare auto table width edge cases need broader conformance
  coverage, especially column groups, collapsed-column interactions, and less
  common multi-span combinations.
- Divergence: CSS Overflow 3 behavior for table cells and block descendants is
  incomplete, including scrollbar painting and viewport scrolling.
- Divergence: table fragmentation is incomplete for full cloned decoration
  semantics.
- Divergence: flex-specific descendant link handling inside nested table/flex
  fragments is incomplete.
- Divergence: collapsed-track border conflict resolution across fragment
  boundaries has rare unresolved cases.
- Divergence: collapsed row/column clipping has remaining edge cases involving
  partial glyph clipping, rare fragmented table pieces, and complex nested
  formatting contexts.
- Divergence: full horizontal-axis table baseline positioning for mixed writing
  modes needs broader conformance coverage.
- Divergence: table height distribution remains incomplete for complex floats
  inside cells, large percentage/min-max matrices, and complex nested
  formatting contexts.

### Box Alignment

- Spec area: CSS Box Alignment, CSS Display, CSS Writing Modes, CSS
  Fragmentation.
- Divergence: `align-content` is incomplete for fragmented block containers and
  fragmented table cells.
- Divergence: `align-content` is incomplete for general multi-column
  containers.
- Divergence: grid alignment is incomplete wherever grid layout itself is
  incomplete.
- Divergence: baseline content-alignment is incomplete for indefinite
  percentage nested vertical exports.
- Divergence: column-axis baseline sharing and remaining column-axis baseline
  edge cases are incomplete.
- Divergence: block containers outside flex/table alignment paths use baseline
  fallback behavior rather than full baseline sharing semantics.

### Flex Layout and Fragmentation

- Spec area: CSS Flexible Box Layout, CSS Box Alignment, CSS Fragmentation.
- Divergence: paged flex fragmentation is partial and lacks complete cloned
  decoration slicing.
- Divergence: flex fragment metadata is incomplete for links, running
  assignments, named pages, and other PDF side effects.
- Divergence: `visibility: collapse` flex-item behavior needs broader
  conformance coverage for column-axis cases, `wrap-reverse`, collapsed
  replaced items, and rare writing-mode combinations.
- Divergence: true column-axis baseline sharing is incomplete.
- Divergence: indefinite percentage nested exported baselines are incomplete.
- Divergence: remaining indefinite percentage cross-size interactions outside
  definite stretch sizing are incomplete.
- Divergence: nested flex edge cases remain incomplete.
- Divergence: exact child-height estimates are incomplete for flex items with
  column, complex multicolumn, or deeply nested descendants.
- Divergence: flex intrinsic sizing may diverge from browser behavior if the
  CSS Flexbox draft's web-compatible intrinsic sizing algorithm differs from
  the concrete ideal max-content flex-fraction algorithm.

### Pagination and Fragmentation

- Spec area: CSS Fragmentation, CSS Paged Media.
- Divergence: pagination is cursor-oriented rather than driven by durable
  fragment objects with available block-size negotiation.
- Divergence: general fragmentainer layout is incomplete.
- Divergence: cloned decorations are incomplete across fragmented layout modes.
- Divergence: multi-pass generated content is incomplete.
- Divergence: rare complex table-cell child/effect fragmentation is incomplete.
- Divergence: robust flex and grid fragmentation are incomplete.
- Divergence: full forced/avoid break precedence and break selection are
  incomplete for cell-internal effect fragmentation, flex/grid fragmentation,
  and other non-block fragment paths.
- Divergence: class A/B/C break handling is not uniformly represented across
  block, inline, table, flex, grid, floats, and generated content.

### Page Margin Boxes and Generated Content

- Spec area: CSS Paged Media, Generated Content for Paged Media, CSS
  Fragmentation.
- Divergence: named-string and running-element boundaries are not represented
  as durable fragments across all formatting contexts.
- Divergence: rare rowspan, collapsed-track, repeated table copy, and deeply
  fragmented flex pagination cases can lose or misplace named-string and
  running-element assignments.
- Divergence: `element()` replay is incomplete for complex table-root
  descendants.
- Divergence: `element()` replay is incomplete for flex-fragment edge cases.
- Divergence: `string-set` content lists omit box-preserving generated
  fragments beyond the inline quote, leader, marker, target
  cross-reference, URL image, and Level 3 linear/radial gradient image cases.
- Divergence: page-name behavior remains incomplete for complex repeated table
  copy interactions and deeply fragmented flex layouts.
- Divergence: background layer support for normal boxes, page boxes, and margin
  boxes lacks unsupported CSS Images features beyond URL raster images and
  Level 3 linear/radial gradients.

### Bookmarks and PDF Outlines

- Spec area: CSS Generated Content bookmarks, PDF outlines.
- Divergence: bookmark labels do not support full generated-content list
  evaluation.
- Divergence: pseudo-element bookmark label content is incomplete.
- Divergence: counters, named strings, and transformed bookmark coordinates are
  incomplete.

### Borders

- Spec area: CSS Backgrounds and Borders, CSS Color, CSS Images, CSS
  Fragmentation.
- Divergence: CSS Color parsing does not cover all color functions or
  non-sRGB color spaces.
- Divergence: CSS Borders 4 `corner-shape` exact inset border offset behavior
  is incomplete.
- Divergence: `corner-shape` integration with `box-shadow`, overflow clipping,
  logical shorthands, and side shorthands is incomplete.
- Divergence: background clipping follows `border-radius` curves rather than
  `corner-shape` contours.
- Divergence: rounded dashed and dotted borders do not have exact phase
  distribution along curved corner arcs.
- Divergence: antialias-equivalent side transitions and full corner conflict
  behavior are incomplete.
- Divergence: border images do not support all CSS image sources, including
  gradients and SVG.
- Divergence: border-image repeat centering and partial-tile behavior are not
  exact against the CSS Backgrounds and Borders requirements.
- Divergence: decoration fragmentation for borders is incomplete.

### Backgrounds, Images, and SVG

- Spec area: CSS Backgrounds and Borders, CSS Images, SVG, compositing.
- Divergence: general SVG parsing and rendering are incomplete.
- Divergence: CSS Images object sizing, fallback, transforms, opacity,
  compositing, and broader image source behavior are incomplete.
- Divergence: CSS Images Level 4 gradient color spaces and hue interpolation
  are unsupported.
- Divergence: conic gradients are unsupported.
- Divergence: `image-set()` is unsupported.
- Divergence: SVG image sources are unsupported in CSS image-consuming
  properties.
- Divergence: generated gradients are not shared across all image-consuming
  properties, including `border-image`, markers, and masks.
- Divergence: `box-shadow` blur rasterization, rounded-corner shadow shaping,
  complete spread behavior, and fragmentation are incomplete.

### Positioning and Stacking

- Spec area: CSS Positioned Layout, CSS 2.2 Appendix E painting order,
  transforms, CSS Fragmentation, compositing.
- Divergence: CSS 2.2 Appendix E painting order is incomplete for fragmented
  and nested formatting-context edge cases.
- Divergence: rare spanning table decoration and border conflicts remain
  unresolved in fragmented painting order.
- Divergence: flex-specific nested descendant links are incomplete in
  fragmented paint/effect contexts.
- Divergence: opacity, transforms, links, and effect groups are incomplete for
  some fragmented stacking-context combinations.
- Divergence: fragmented overflow clips need broader conformance coverage.
- Divergence: fragmented floats do not fully let descendant stacking contexts
  escape into the ancestor context while preserving fragment-local coordinates.
- Divergence: `position: fixed` replay is not fully page-context-specific when
  later named pages use different page boxes.
- Divergence: transformed ancestors capturing fixed containing blocks across
  fragments are incomplete.
- Divergence: transformed containing-block behavior is incomplete in less
  common formatting-context and fragmentation cases.
- Divergence: exact static-position placeholders are incomplete for multiline
  inline, table, fragmented, and other remaining formatting-context cases.
- Divergence: containing block rules are incomplete for multi-line positioned
  inlines, transformed contexts, table contexts, and fragmented contexts.
- Divergence: positioned fragmentation behavior is incomplete.
- Divergence: `isolation`, blend modes, filters, masks, `clip-path`,
  containment-triggered paint isolation, `content-visibility`, and
  `will-change` lack full visual semantics.
- Divergence: filter functions are retained but not rendered.
- Divergence: masks and `clip-path` geometry are not shape-aware.
- Divergence: `will-change` is only a pre-isolation trigger.

### Units and Value Computation

- Spec area: CSS Values and Units, CSS Cascade, CSS Sizing.
- Divergence: container query units (`cqw`, `cqh`, `cqi`, `cqb`, `cqmin`,
  `cqmax`) are not implemented.
- Divergence: additional font/line metric units such as `ex`, `cap`, `ic`,
  `lh`, and `rlh` need property-wide support.
- Divergence: many property-specific keywords and computed-value models are
  incomplete.
- Divergence: CSS Sizing Level 4 `aspect-ratio` integration is incomplete
  across block, table, grid, absolute positioning, and remaining non-raster
  replaced-element edge cases.
- Divergence: CSS Sizing Level 4 `stretch` integration is incomplete across
  block, table, grid, absolute positioning, and intrinsic sizing contexts.
- Divergence: multi-layer background lists are incomplete.
- Divergence: full generated image values are incomplete.
- Divergence: invalid-at-computed-value-time behavior is incomplete for
  not-yet-modeled shorthands and cross-property dependencies.
- Divergence: property dependency invalidation beyond exact property names is
  incomplete.
- Divergence: `ComputedStyle` carries layout-oriented used-value projections
  that should be layout-only used-style data.

### CSS Cascade and Selectors

- Spec area: CSS Cascade, Selectors, CSS Nesting, CSS Conditional Rules, Media
  Queries, HTML form state.
- Divergence: CSS-wide defaulting is incomplete in page-context structural
  declarations.
- Divergence: Media Queries support does not cover the full feature grammar or
  all context-dependent features.
- Divergence: `@supports` support does not cover all CSS Conditional
  feature-query functions.
- Divergence: form/input pseudo-classes are incomplete for custom element
  state, complete selectedness normalization, pattern/date/time validation, and
  full HTML constraint validation.
- Divergence: element display state pseudo-classes do not model dynamic picker,
  popover, modal, fullscreen, and picture-in-picture states.
- Divergence: XML namespace selector edge cases need broader conformance
  auditing.
- Divergence: shadow/tree-scoped selectors are not implemented.
- Divergence: unmodeled UI/highlight pseudo-elements are not implemented.
- Divergence: CSS nesting uses a narrow flattener rather than a full CSS
  Nesting implementation.

### PDF Conformance and Accessibility

- Spec area: PDF, PDF/A, PDF/UA, PDF/X, tagged PDF.
- Divergence: output is not tagged or structured.
- Divergence: output is not PDF/A-conformant.
- Divergence: output is not PDF/UA-conformant.
- Divergence: output is not PDF/X-conformant.
- Divergence: PDF/UA and PDF/X conformance identification metadata is not
  emitted.
- Divergence: structure trees, semantic roles, annotations, color profile
  handling, output intents, and related archival/accessibility constraints are
  incomplete.
- Divergence: PDF file trailer `/ID` arrays and PDF/A identification metadata
  alone are insufficient for PDF/A conformance.
- Divergence: the renderer needs a conformance decision for page-level
  coordinate transforms or an equivalent text-emission transform if future
  samples expose transform-related raster deltas.

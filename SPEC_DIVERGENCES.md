# Quire Spec Divergences

Last updated: 2026-06-30

This document is the central inventory of known places where `quire` is
divergent from the relevant CSS, HTML, SVG, and PDF specifications. It is
intended to be exhaustive for all known divergences. It should describe the
current unresolved gaps only: this is not a change log, progress report, or
list of implementation wins.

Treat every entry as future work unless it is removed from this file. When a
specific parity plan or implementation note is useful, put that detail under
`docs/`, but keep the actual divergence listed here until the behavior is
implemented or the entry is narrowed by a spec audit.

Entry requirements:

- Each entry should identify the spec area, the current divergence, and the
  feature work needed to reach conformance.
- Entries should be specific enough to guide implementation; avoid broad
  "needs audit" language unless the unknown behavior itself is the divergence.
- Do not record resolved behavior, recent changes, or comparisons as history.
  Remove or narrow entries when the implementation changes.
- Prefer W3C/WHATWG/PDF specifications as the source of truth. Use WeasyPrint
  only as a compatibility reference when the relevant spec is ambiguous.

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
- Divergence: edge cases around HTML generated content, presentational hints,
  and legacy HTML defaults have not been exhaustively audited against the
  WHATWG rendering section.
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
- Divergence: grid display values are parsed and routed to a dedicated layout
  entrypoint, including a first same-page atomic `inline-grid` path, but the
  grid path is still a partial same-page implementation and does not cover the
  full CSS Grid formatting context.
- Divergence: table-internal display fixup and anonymous box construction are
  not fully spec-complete for every malformed or unusual authored tree.
- Needed work: expand parsing and layout for the remaining CSS Display values,
  including layout participation, box tree fixup, pagination behavior, and WPT
  coverage.

### CSS Grid Layout

- Spec area: CSS Grid Layout Levels 1 and 2, CSS Box Alignment, CSS Writing
  Modes, CSS Sizing, CSS Fragmentation.
- Divergence: Grid Level 1 computed values and a first parser pass exist for
  explicit tracks, auto tracks, template areas, auto-flow, and basic item
  placement. Simple same-page normal-flow grid containers use a Taffy-backed
  layout path with Quire-measured basic grid-item leaf contributions and a
  first intrinsic-width estimate for simple explicit column tracks, including
  simple per-track contributions for non-spanning placed items, simple
  positive/negative numeric and named-line column starts/ends, equal
  distribution of simple explicit positive spanning-item contributions after
  crossed gaps, and one fixed-size auto-repeat copy for indefinite intrinsic
  width queries, but Quire does not yet implement the full CSS Grid placement,
  intrinsic sizing, baseline alignment, and track-sizing behavior. Same-page `inline-grid`
  boxes reuse the grid adapter inside an atomic inline fragment and can export
  a baseline from rendered grid item text, but full grid baseline synthesis,
  writing-mode-specific self-start/self-end details, and fragmentation
  interactions remain incomplete. Same-page grid `align-content: space-evenly`
  and `justify-content: space-evenly` are covered for fixed track
  distribution through the Taffy-backed path.
- Divergence: `grid-row` and `grid-column` shorthand parsing exists for
  explicit and omitted-end forms, but unusual `grid`, `grid-template`, and
  named-line parser edge cases, `subgrid`, masonry, full grid baseline
  alignment/export, complete grid-aware absolute static positions, and paged
  grid fragmentation are not implemented. `grid-area` shorthand expansion for
  one- through four-value forms, practical `grid-template` and `grid`
  shorthand forms, covered invalid grid placement custom-ident tokens,
  covered invalid bracketed track line-name tokens, covered auto-repeat parser
  validation, named-line occurrence placement, named-area placement,
  template-area generated
  `area-start`/`area-end` line placement, rectangular `grid-template-areas`
  validation, same-page atomic `inline-grid` layout, and same-page
  row/column/dense auto-placement basics are implemented. Same-page
  child collection covers ordinary in-flow children, anonymous non-whitespace
  text grid items, whitespace-only text suppression, ordering, basic
  out-of-flow splitting, `display: contents` flattening, and tree-abiding
  `::before`/`::after` generated grid items participating in order-modified
  auto-placement. Same-page spanning over definite fixed tracks includes row
  and column gaps in replayed item geometry, and horizontal RTL
  auto-placement starts at the inline-start/rightmost column. Definite
  same-page flexible `fr` tracks distribute available inline size for ordinary
  in-flow grid items, fixed-size same-page `repeat(auto-fill, ...)` expands in
  definite inline sizes with empty-track `repeat(auto-fit, ...)` collapse, and
  `grid-auto-rows`/`grid-auto-columns` track lists cycle across simple
  same-page implicit tracks. Same-page abspos
  positive/negative numeric explicit-line and
  positive/negative named explicit-line static positions over fixed explicit
  tracks and gaps are implemented. Same-page abspos template-area placement,
  generated template-area line placement, and simple flexible-track,
  intrinsic-track, and fixed-size `auto-fill` repeated-track static positions
  are covered through Taffy
  non-participating probe layout.
- Needed work: complete grid parsing, broaden Quire-native grid child
  collection and the Taffy adapter, complete Quire intrinsic contribution
  measurement, complete grid exported baselines and abspos static positions,
  and add page-fragment-aware grid fragmentation. See
  `docs/CSS_GRID_PARITY.md`.

### CSS Content

- Spec area: CSS Content Level 3, generated content, accessibility metadata.
- Divergence: unsupported CSS Images values used in `content`, such as
  gradients and image-set forms, remain under the broader CSS Images gap.
- Divergence: generated image alt text is not emitted into tagged PDF/PDF-UA
  structure.
- Divergence: broader target cross-reference behavior outside the currently
  supported text-capture paths remains incomplete.
- Divergence: `leader()` and generated content layout need broader WPT
  coverage for unusual fragmentation, target cross-reference, and replaced
  generated-content combinations.
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
- Divergence: nested flex/table layout inside atomic inline boxes is partial.
- Divergence: CSS Inline 3 baseline-table precision for ideographic,
  mathematical, hanging, and central baselines falls back to available font
  metrics/content-area approximations, and the full aligned-subtree model for
  deeply nested mixed inline runs still needs broader WPT/browser audits.
- Divergence: justification opportunities are policy-owned and suppress current
  joining/control gaps, but broader complex-script expansion rules remain
  incomplete.
- Divergence: `text-orientation` is parsed, cascaded, and applied to vertical
  glyph placement for `mixed`, `upright`, and `sideways` using Unicode
  `Vertical_Orientation`, and upright units request OpenType `vert`/`vrt2`
  alternates, vertical inline advances, and vertical `ch` metrics, but fallback
  transformed glyph forms for `Tr`/`Tu` classes are not implemented when
  font-provided alternates are unavailable or incomplete.
- Divergence: `text-autospace` is incomplete for punctuation-specific spacing,
  `replace` semantics, vertical text-orientation interactions, `text-spacing`
  shorthand integration, and dynamic DOM cases from the CSS Text Level 4 draft.
- Divergence: `hanging-punctuation` has logical inline-axis line-edge
  accounting, but fallback transformed vertical glyph-form details and complex
  bidi fragment-edge placement remain incomplete.
- Divergence: text decoration strokes are prepared as logical inline-line
  annotations with vertical underline side placement and matrix-aware
  skip-space positioning, but full `text-decoration-skip-box`, complete
  `text-decoration-skip-self` edge cases, decoration collision refinements, and
  any remaining rare fragmented vertical decoration cases are incomplete.
- Divergence: text emphasis marks are prepared as typographic-unit annotations
  and painted through normal shaped text emission, but ruby-style collision
  handling, vertical line-box expansion, and annotation overlap avoidance remain
  incomplete.
- Divergence: CSS Text Decoration `text-shadow` support is limited to Level 3
  zero-blur vector painting and bounded translucent vector replay for blurred
  shadows; blur/spread/inset shadows are not yet rasterized as grouped
  text/decorator alpha masks.
- Divergence: CSS `text-transform: math-auto` and remaining locale-specific
  CSS Text Level 4 transforms are not implemented.
- Divergence: residual CSS Text discrepancies remain in Text Level 4 autospace
  edge cases and fragmented contexts that combine nested formatting contexts,
  generated inline content, page-margin inline content, and complex visual
  effects.
- Needed work: broaden WPT coverage for shared graph/line-sequence paths,
  shared typographic-unit policy, and fragmented nested formatting contexts;
  then complete the script-, writing-mode-, and language-sensitive CSS Text
  features above.

### Text Shaping and Font Selection

- Spec area: CSS Fonts, CSS Text, OpenType shaping, PDF font embedding.
- Divergence: synthetic bold and oblique are not applied to emitted PDF glyph
  outlines.
- Divergence: variable-font axis instantiation and descriptor range matching
  are incomplete.
- Divergence: `unicode-range` support is limited to scalar ranges, wildcard
  ranges, and range-limited shaping spans for registered `@font-face` rules;
  it has not been exhaustively audited against the full CSS Fonts descriptor
  matching model, invalid descriptors, collections, and variable-font
  instances.
- Divergence: per-glyph synthetic fallback for missing small-caps and
  petite-caps feature glyphs is not implemented.
- Divergence: PDF font subsetting uses retained glyph IDs for standalone font
  programs, emits spec-shaped subset names and audited descriptors, and falls
  back to full standalone font embedding for unsupported or failed subset
  cases, so some generated PDFs remain larger than engines such as WeasyPrint
  that can remap subsets densely.
- Divergence: CFF/CFF2, variable-font instances, and broad collection coverage
  are incomplete. TTC/OTC faces are extracted to standalone sfnt data before
  embedding or subsetting, but this still needs repo-local fixture coverage
  beyond optional system-font smoke tests.
- Divergence: OS/2 embedding permissions are audited and logged in the default
  PDF profile, but strict PDF/A/PDF/UA failure modes are not exposed through a
  public render option yet.
- Divergence: baseline and line-box metric mapping still diverges from
  WeasyPrint/Pango in some contexts. Mixed-weight generic-family inline text
  now uses shaped-font metrics for fragment baselines so normal and strong runs
  selected from different concrete faces share the same text baseline.
- Divergence: residual complex-script shaping mismatches may remain outside
  the covered CSS Text join-control and tatweel boundary cases.
- Needed work: add focused tests and implementation for font synthesis,
  variable fonts, missing-glyph fallback, CFF/collection embedding, denser
  PDF font subsetting where safe, per-glyph caps fallback, full `unicode-range`
  descriptor auditing, and residual complex-script mismatches.

### Tables

- Spec area: CSS Tables, CSS 2.2 table layout, HTML table semantics.
- Divergence: anonymous table object construction is not complete for all
  malformed edge cases beyond the covered common fixup, authored empty-row,
  inline-wrapper, consecutive non-cell whitespace, and HTML span-parsing
  cases.
- Divergence: rare auto table width edge cases still need broader WPT and
  WeasyPrint coverage, especially complex colspans combined with column
  groups and collapsed-column interactions.
- Divergence: CSS Overflow 3 behavior for table cells and block descendants is
  incomplete, including clipping, scrollbar painting, and overflow
  propagation.
- Divergence: table fragmentation is incomplete for full cloned decoration
  semantics, flex-specific descendant link handling inside planned nested flex
  fragments, and rare collapsed-track border conflict resolution across
  fragment boundaries.
- Divergence: collapsed row/column clipping has remaining edge cases involving
  partial glyph clipping and rare fragmented table pieces.
- Divergence: table height distribution and baseline behavior are implemented
  for common CSS Tables 3 cases, and vertical-writing table-cell baselines no
  longer inflate physical row height. Full horizontal-axis baseline positioning
  for mixed writing modes still needs broader WPT and WeasyPrint coverage, as
  do floats inside cells, large percentage/min-max matrices, and complex nested
  formatting contexts. Table-cell content relayout now resolves common
  percentage-height replaced descendants when the cell or table root is
  explicitly height-sized.
- Needed work: port WeasyPrint table tests incrementally, finish table fixup
  edge cases, extend table sizing coverage for complex spans and column
  groups, broaden height/baseline coverage, and complete cloned decoration
  and collapsed-track behavior across fragmentation. See
  `docs/CSS_TABLES_PARITY.md` for the current parity breakdown.

### Box Alignment

- Spec area: CSS Box Alignment, CSS Display, CSS Writing Modes, CSS
  Fragmentation.
- Divergence: `align-content` is implemented for flex containers, table cells,
  same-page ordinary block containers with definite used block sizes, and
  Quire's current definition-list column layout, including default/safe/unsafe
  overflow-position handling, single-subject distribution fallback, and the
  required independent formatting context for non-`normal` block values. It is
  still incomplete for fragmented block containers and table cells, general
  multi-column containers, and the future grid layout mode.
- Divergence: baseline content-alignment is implemented for horizontal row
  flex line packing and compatible table-cell row groups, with orthogonal table
  cells using baseline fallback and row-spanning baseline cells participating
  in their start-most or end-most spanned row as required. Row-axis nested flex
  containers export first/last intrinsic baselines including definite wrapped
  `row`, `row-reverse`, `wrap-reverse`, and definite `max-width` constrained
  auto lines plus `fit-content(<length>)` constrained lines.
  `width:max-content` row flex baseline exports include main-axis gap lengths
  and definite flex-basis contributions, and vertical-writing row flex
  content-alignment can align physical x-axis line baselines. Nested
  vertical-writing row flex containers export physical x-axis first/last
  baselines for parent row flex baseline-sharing groups, including
  `wrap-reverse` line packing, when their wrapped line estimates have a
  definite physical cross size or an auto cross size resolved from the wrapped
  line stack, or when percentage physical cross sizes resolve against a
  definite parent cross size. Block containers use baseline fallback
  alignment. Column flex baseline content-alignment values use the required
  safe start/end fallback when compatible baseline sharing cannot apply, and
  column flex baseline self-alignment applies the matching first/last-baseline
  fallback sides. Indefinite percentage nested vertical exports and true
  column-axis baseline sharing remain incomplete.
- Needed work: complete fragmented block alignment semantics, audit
  multi-column alignment subjects, add grid alignment when grid layout exists,
  and broaden WPT coverage. See `docs/CSS_BOX_ALIGNMENT_PARITY.md`.

### Flex Layout and Fragmentation

- Spec area: CSS Flexible Box Layout, CSS Box Alignment, CSS Fragmentation.
- Divergence: paged flex fragmentation is partial; normal-flow block flex
  containers can break at row line and column item boundaries and carry
  page-local fragment-plan metadata for those boundary fragments. Oversized
  row-line and column-item fragments now split into page-local item slices and
  replay split item visual content from the source item layout, but full cloned
  decoration slicing and complete page-fragment metadata for links, running
  assignments, named pages, and every PDF side effect are incomplete.
- Divergence: `visibility: collapse` flex-item behavior uses a visible-layout
  probe, source-line struts, and collapsed relayout, but column-axis,
  `wrap-reverse`, collapsed replaced-item, and rare writing-mode combinations
  still need broader conformance coverage.
- Divergence: horizontal row flex line packing supports
  `align-content: baseline` and `align-content: last baseline`, and
  baseline fallback is writing-mode aware for the covered self- and
  content-alignment cases, including column flex content-alignment fallback,
  and vertical-writing row flex content-alignment supports physical x-axis
  baseline packing. Nested vertical-writing row flex containers export
  horizontal baselines for definite wrapped rows, auto-width wrapped rows, and
  definite-parent percentage wrapped rows, including `wrap-reverse`. Column
  flex baseline self-alignment fallback applies to first/last-baseline
  participants when true baseline sharing is unavailable. True column-axis
  baseline sharing, indefinite percentage nested exported baselines, remaining
  indefinite percentage cross-size interactions outside definite stretch
  sizing, and nested flex edge cases are incomplete.
- Divergence: exact child-height estimates for flex items with column or
  multicolumn descendants are incomplete.
- Divergence: flex intrinsic sizing implements the concrete ideal
  max-content flex-fraction algorithm from CSS Flexbox 9.9.1.1, but the
  draft's web-compatible algorithm in 9.9.1.2 is still unresolved and may
  require a future compatibility decision if WPT/browser behavior diverges.
- Needed work: complete flex fragment side-effect metadata, broaden remaining
  flex baseline alignment cases, finish collapsed-item edge-case audits, and
  broaden sequence-backed estimate coverage for generated content inside
  anonymous flex text items, deeply nested flex/table/multicolumn descendants,
  rare orthogonal-flow intrinsic cases, and exact child block-size estimates.
  See `docs/CSS_FLEXBOX_PARITY.md`.

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
  formatting contexts; table row/cell replay and deeply fragmented flex
  pagination still need durable assignment propagation.
- Divergence: `element()` replay remains incomplete for table-root and
  flex-fragment replay edge cases.
- Divergence: `string-set` content lists still omit richer non-text target
  fragments and box-preserving generated fragments.
- Divergence: page-name edge cases across table and flex fragmentation remain
  incomplete.
- Divergence: background layer support for normal boxes, page boxes, and
  margin boxes remains limited by unsupported CSS Images features and
  incomplete rounded-corner clipping for layered backgrounds.
- Needed work: broaden table/flex replay and named-page coverage, and extend
  the typed paged-media generated-content pipeline with full `string-set`
  target fragments and cross-references.

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
- Divergence: `box-shadow` supports rectangular zero-blur shadows for basic
  outer and inset cases, but blur rasterization, rounded-corner shadow
  shaping, complete spread behavior, and fragmentation are incomplete.
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
  incomplete for some fragmented stacking-context combinations. Fragmented
  floats still do not fully let descendant stacking contexts escape into the
  ancestor context while preserving fragment-local coordinates.
- Divergence: `position: fixed` replay is not fully page-context-specific when
  later named pages use different page boxes or transformed ancestors capture
  the fixed containing block across fragments.
- Divergence: transformed containing-block behavior is incomplete in less
  common formatting-context and fragmentation cases.
- Divergence: exact static-position placeholders are incomplete for multiline
  inline, table, fragmented, and other remaining formatting-context cases.
  Inline-level absolutely positioned boxes now use their prepared hypothetical
  placeholder rectangle for auto horizontal and vertical static position,
  including forced-break and RTL inline alignment cases.
  Block-level absolutely positioned boxes after inline content, including
  block-level absolute descendants encountered inside inline boxes, now use
  the preceding inline line boxes for their auto vertical static position,
  including split inline fragments that only contribute inline-start or
  inline-end edge decoration.
- Divergence: containing block rules are incomplete for positioned,
  transformed, table, and fragmented contexts. Positioned table wrappers now
  cover absolute descendants inside captions, but remaining edge cases exist
  for transformed and fragmented table contexts.
- Divergence: positioned fragmentation behavior is incomplete.
- Divergence: `isolation`, blend modes, filters, masks, `clip-path`,
  containment-triggered paint isolation, `content-visibility`, and
  `will-change` are parsed and participate in coarse stacking-context policy,
  but their visual semantics are incomplete: filter functions are retained but
  not rendered, masks and `clip-path` geometry are not shape-aware, and
  `will-change` is only a pre-isolation trigger.
- Needed work: finish oversized/spanning table fragmentation and remaining
  fragmented effect paint contexts so table pieces, normal-flow sibling overlap,
  and nested stack levels flatten exactly from the durable tree before final PDF
  emission.

### Units and Value Computation

- Spec area: CSS Values and Units, CSS Cascade computed-value processing.
- Status: `ch` resolves through selected font metrics across render/layout
  typed length consumers, including vertical-upright inline advances, table grid sizing,
  separated-table `border-spacing`, multicolumn widths, rounded radii,
  page-context margins, padding, borders, and sizes, border and outline widths,
  border-image outsets, outline offsets, transform translations and origins,
  shadows, background gradient stop positions, baseline shifts, text decoration
  lengths, and `font-size` during initial box-tree construction, rebuilt child-box
  construction, builder-owned estimate probes, DOM flow-helper probes,
  generated and typographic pseudo-element styles, and table
  row/column/cell/caption helper style reconstruction. CSS math comparisons
  such as `min(1ch, 2ch)` and `min(calc(1ch + 1pt), calc(2ch + 1pt))`
  reduce when the unknown components cancel or differ in only one
  non-negative-basis unit, preserving the unresolved `ch` component.
  `min()`/`max()`/`clamp()` comparisons such as `min(10pt, 1ch)` and
  `min(10ch, 50%)` are deferred through nested CSS math until font-metric,
  viewport-unit, or percentage-basis resolution can choose the used branch.
- Divergence: container query units (`cqw`, `cqh`, `cqi`, `cqb`, `cqmin`,
  `cqmax`) are not implemented, and additional font/line metric units such as
  `ex`, `cap`, `ic`, `lh`, and `rlh` still need property-wide support.
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
- Divergence: PDF/A identification metadata is emitted for supported PDF/A
  variants, but PDF/UA and PDF/X conformance identification metadata, structure
  trees, semantic roles, annotations, color profile handling, and related
  archival/accessibility constraints are not implemented. PDF file trailer
  `/ID` arrays and PDF/A identification metadata alone do not make output
  PDF/A-conformant.
- Divergence: the renderer still needs a final decision on whether to adopt
  WeasyPrint's page-level coordinate transform model or an equivalent
  text-emission transform if future samples expose transform-related raster
  deltas.
- Needed work: complete conformance modes after layout fidelity stabilizes,
  starting with output intents, color/profile handling, tagging, structure-tree
  support, and external validator coverage.

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
- CSS Fonts Module Level 5:
  <https://www.w3.org/TR/css-fonts-5/>
- CSS Backgrounds and Borders Module Level 3:
  <https://www.w3.org/TR/css-backgrounds-3/>
- CSS Box Model Module Level 4:
  <https://drafts.csswg.org/css-box-4/>
- CSS Shapes Module Level 1:
  <https://drafts.csswg.org/css-shapes-1/>
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

### Wasm platform integration

- The `wasm32-unknown-unknown` library target is compile-compatible only.
  It does not implement browser or WASI resource loading, filesystem output,
  system-font discovery, or a host-language binding. Consequently, HTTP(S)
  and `file:` resource URLs cannot be rendered on that target.

### CSS Viewport `zoom`

- CSS `zoom` is not yet applied consistently in nested-frame layout. Background
  and border-image intrinsic
  sizing, shadows, filters, masks, transforms, and scrolling also remain
  outside the current zoomed used-value boundary.
  <https://drafts.csswg.org/css-viewport/#zoom-property>

### CSS Text White-space and Wrapping

- Spec area: CSS Text Level 3 white-space processing and line-edge processing.
- Divergence: selected dictionary and authored discretionary breaks carry
  source-boundary replacements and language-sensitive markers. A selected
  marker is now shaped with its source edge before it is materialized as a
  separate item, but the final UAX #9 visual-slice handoff still cannot retain
  non-painting source context across every Arabic/Uyghur edge. In particular,
  a marker and its source may retain the right advances while losing a
  contextual glyph form after visual fragment splitting.
  <https://www.w3.org/TR/css-text-3/#hyphenation>
  <https://drafts.csswg.org/css-text-4/#hyphenate-character>
- Divergence: line-edge effects for `pre-wrap` leading spaces, bidi visual line
  ends, floats, justification, and intrinsic sizing are incomplete. The graph
  now records selected multi-run Unicode-space suffixes by source range and
  preserves their paint ownership, but PDF extraction still needs a dedicated
  source-range emission path and decoration ownership across complex
  bidi/fragmented inline boundaries remains incomplete.
  <https://drafts.csswg.org/css-text-3/#white-space-phase-2>
- Divergence: tab stops use a shared logical cursor through text and atomic
  inline content, but float-displaced lines do not yet carry their block
  content-edge coordinate through graph fitting and painting.
  <https://drafts.csswg.org/css-text-3/#tab-size-property>
- Divergence: textarea and carriage-return transitions across mixed
  white-space descendants do not yet use fully conformant shared CSS Text
  whitespace processing.

- Divergence: `word-space-transform` resolves explicit U+200B and `<wbr>`
  separators after transparent-edge and forced-boundary context is known, but
  does not yet preserve a non-selectable virtual-source range or implement
  language-sensitive `auto-phrase` segmentation and placement.
  <https://drafts.csswg.org/css-text-4/#word-space-transform>
- Divergence: generated Type 0 font `/ToUnicode` mappings are not reliably
  extractable for macOS CJK system fonts. In the Taiwanese numeral comparison
  fixture, `pdftotext` drops the CJK title and cell content and corrupts some
  marked Latin transliteration even though the painted glyphs are correct.
  This prevents faithful text extraction and accessibility.
  <https://www.w3.org/TR/REC-PDF-ISO32000-1-200807/#sec-9.10.3>

### CSS Text Level 4 Wrapping

- Spec area: CSS Text Level 4 `text-wrap-style` and line clamping.
- Divergence: `text-wrap: balance` uses the shared graph for float-free groups
  of up to ten lines, but source-position inline floats are not yet modeled in
  balance-candidate selection. Independently balanced pseudo-element block
  groups and fragmentation boundaries remain incomplete. `line-clamp:auto`,
  positioned/floated clamp boundaries, and clamp-before-balance handling at
  pseudo-element and fragmentation boundaries remain incomplete.
  <https://drafts.csswg.org/css-text-4/#text-wrap-style>

### HTML User-Agent Stylesheet

- Spec area: HTML rendering defaults, presentational hints, CSS Cascade, CSS
  Generated Content, CSS Lists, CSS Tables.
- Divergence: HTML generated content defaults, author overrides, counters, text
  capture, and legacy presentational hints have not been exhaustively audited
  against the WHATWG rendering section.
- Divergence: presentational hints need element-by-element auditing, especially
  for table attributes and legacy HTML attributes whose cascade behavior affects
  computed styles.
- Divergence: XML/XHTML parsing rejects a valid element carrying both `xml:lang`
  and `lang` as a duplicate attribute. This prevents
  `html/rendering/non-replaced-elements/tables/table-align-float.xhtml` from
  rendering; XML namespace parsing remains limited by the current `xml5ever`
  integration.
  <https://www.w3.org/TR/xml-names/#defaulting>

### HTML Embedded Browsing Contexts

- Spec area: HTML iframe element and nested browsing contexts.
- Divergence: iframe rendering is limited to static `src` and `srcdoc`
  documents. It does not implement scripting, navigation, sandboxing,
  permissions policies, lazy loading, or other browsing-context lifecycle and
  security behavior. Embedded documents are composed as a single clipped PDF
  page rather than a live scrolling viewport.
  <https://html.spec.whatwg.org/multipage/iframe-embed-object.html#the-iframe-element>

### Lists and Markers

- Spec area: CSS Lists and Counters, CSS Counter Styles, generated content,
  tagged PDF.
- Divergence: image marker sizing, baseline alignment, and marker box geometry
  do not fully follow CSS Lists. In particular, an outside marker for an
  atomic `flow-root list-item` does not yet retain the first descendant line's
  final block position when that line follows a block-start margin.
- Divergence: inline generated-content evaluation still consumes the runtime
  counter scope because normalized generated boxes do not yet retain a precise
  logical counter origin for every nested-scope shape.
- Divergence: vertical writing-mode geometry for outside markers is incomplete.
- Divergence: PDF marker output lacks tagged marker semantics for PDF/UA.
  Bitmap marker glyphs retain replacement text through PDF `/ActualText`, but
  Quire does not yet emit the structure tree needed to associate them with a
  PDF/UA list-label element.
- Divergence: custom counter-style `speak-as` has no observable effect because
  speech output is not generated.

### CSS Custom Properties

- Spec area: CSS Custom Properties, CSS Properties and Values API, CSS Color.
- Divergence: registered custom properties (`@property`) are not modeled, so
  descriptor syntax, inheritance, initial values, and typed substitution are
  unavailable.
- Divergence: `light-dark()` resolves only Quire's fixed light print scheme;
  `color-scheme`, dark-scheme selection, and interaction with registered
  custom-property values are not implemented.
- Divergence: variable substitution in complex `background` and `font`
  shorthands is incomplete.
- Divergence: `@font-face` descriptor handling still needs full CSS Variables
  and CSS Fonts conformance coverage, including invalid descriptor recovery and
  font matching parity.

### CSS Display Model

- Spec area: CSS Display, CSS Lists, CSS Tables, CSS Ruby.
- Divergence: `run-in` display behavior is unsupported.
- Divergence: ruby display values and ruby formatting context behavior are
  unsupported.
- Divergence: authored table-internal display values are not fully handled
  outside table-specific paths.
- Divergence: table-internal display fixup and anonymous box construction are
  not spec-complete for every malformed or unusual authored tree.
- Divergence: fieldset/legend rendering is still ordinary block layout rather
  than the HTML rendered-legend model. In particular, a flattened `legend`
  cannot yet participate in the fieldset border's rendered-legend state.
- Divergence: grid display values do not yet cover the full CSS Grid formatting
  context, layout participation, pagination behavior, and box tree fixup.

### CSS Scroll Snap

- Spec area: CSS Scroll Snap Level 1 and CSSOM View scrolling.
- Divergence: Quire's static scroll-snap model covers container/area geometry,
  target navigation, root handling, and iframe subdocuments, but atomic inline
  and writing-mode replay still have two visual failures in the local
  non-script `css/css-scroll-snap/` WPT selection.
- Divergence: live DOM scrolling APIs, JavaScript-driven scrolling,
  re-snapping after interactive relayout, and scroll-snap events are not
  implemented in the static PDF renderer.
  <https://www.w3.org/TR/css-scroll-snap-1/>

### CSS Float Layout

- Spec area: CSS 2.2 floats and float-adjacent formatting contexts.
- Divergence: table wrappers, flex/grid roots, and block-level replaced boxes
  need a complete audit against the CSS 2.2 border-box float avoidance rule.
- Divergence: non-block float-adjacent root placement paths still risk using
  margin-box placement where CSS 2.2 requires border-box collision behavior.
  Normal-flow horizontal block formatting context roots, including
  `overflow:hidden` roots, use border-box fixed-point float avoidance,
  including the resolved normal-flow border-box inline start. The remaining
  audit still covers negative-margin, nested, and non-block root paths.
- Divergence: zero-width float exclusions do not yet consistently distinguish
  block-axis clearance from inline collision. This remains visible in the
  `zero-width-floats*` CSS2 reftests, especially with negative BFC margins.
  Negative physical margins otherwise preserve their legal border-box overflow
  while float avoidance tests the actual collision rectangle, including BFC
  roots whose positive adjoining start margins cross a float.
- Divergence: split-flow floats normalized from inline ancestors still need an
  audit of source-boundary ownership and do not replay the following inline
  continuation against the source line's float exclusion. The inline graph now
  keeps float-placement checkpoints separate from legal CSS Text soft-wrap
  opportunities, including inside descendant `white-space: nowrap` islands.
  The remaining CSS2 failures include `float-nowrap-4.html`,
  `float-nowrap-hyphen-rewind-1.html`, `float-no-content-beside-001.html`,
  and `floats-line-wrap-shifted-001.html`.

### CSS Shapes

- Spec area: CSS Shapes Module Level 1 float-area shapes.
- Divergence: `shape-outside` supports shape boxes and the `inset()`,
  `circle()`, `ellipse()`, and `polygon()` basic shapes for floated boxes.
  Polygons retain `nonzero` and `evenodd` fill rules and use analytical
  scanline intersections. `shape-margin` offsets circle, ellipse,
  rounded-rectangle, and polygon contours before the float margin-box clip;
  concave polygon offsets still need broader conformance coverage.
  Selected inline candidates now refine their float band against the full used
  line slab, but vertical-writing contour placement still fails representative
  WPTs. `shape-image-threshold`, decoded bitmap alpha sources,
  and generated linear gradients now use page-local raster contours; their
  `shape-margin` is clipped to the float margin box. SVG alpha sources,
  several gradient directions in vertical writing, `path()`, CSS Shapes 2
  `shape()`, and initial-letter boxes remain unsupported. Rounded shape-box
  contours and basic-shape contours are queried for line wrapping, but
  remaining BFC-root avoidance paths need broader conformance coverage.
  <https://drafts.csswg.org/css-shapes-1/#shape-outside-property>
  <https://drafts.csswg.org/css-shapes-1/#shape-margin-property>
  <https://drafts.csswg.org/css-shapes-1/#shape-image-threshold-property>
- Divergence: vertical-writing float bands and inline line placement still
  disagree for shaped float contours. The used-line slab is now passed to the
  vertical band query, but the remaining physical/logical placement and retry
  projection must be unified with horizontal-writing behavior.
  <https://drafts.csswg.org/css-shapes-1/#relation-to-box-model-and-float-behavior>
- Divergence: CSS Shapes Level 2 `shape()` values are not represented for
  float-area geometry. The existing unrelated CSS Borders `border-shape`
  subset cannot supply CSS Shapes semantics, including fill rules, curves,
  arcs, percentage resolution, or line-intersection queries.
  <https://drafts.csswg.org/css-shapes-2/#shape-function>

### CSS Writing Modes

- Spec area: CSS Writing Modes, CSS Sizing, CSS Fragmentation.
- Divergence: orthogonal-flow available inline sizing across fragmented page
  and column continuations is incomplete. Normal block, flex/grid item,
  table-cell, inline-block, and positioned scopes keep an indefinite
  percentage basis distinct from their tagged nearest-scroll-container or
  initial-containing-block fallback.
- Divergence: orthogonal-flow available-size negotiation is incomplete across
  fragmentation.
- Divergence: orthogonal available-size behavior for deeply nested mixed
  writing-mode formatting contexts is not exhaustively audited under
  fragmentation and repeated replay.
- Divergence: principal-flow propagation from `body` to the initial
  containing block does not yet preserve the root canvas's anonymous-inline
  versus block-child advancement behavior in sideways writing modes. A
  block-level first body child can consume a physical block track before a
  following paragraph or root generated inline where the propagated canvas
  flow must retain the corresponding anonymous-inline placement; this is
  exposed by `wm-propagation-body-042.html`,
  `wm-propagation-body-047.html`, `wm-propagation-body-049.html`, and
  `wm-propagation-body-054.html`.
  <https://www.w3.org/TR/css-writing-modes-4/#principal-flow>

### CSS Grid Layout

- Spec area: CSS Grid Layout Levels 1 and 2, CSS Box Alignment, CSS Writing
  Modes, CSS Sizing, CSS Fragmentation.
- Spec area: CSS Grid Layout Level 3 Grid Lanes.
- Divergence: Grid Lanes supports establishing `grid-lanes` and
  `inline-grid-lanes` formatting contexts, basic fixed-track placement,
  `flow-tolerance`, order-modified placement, simple dense backfilling,
  `grid-lanes-direction` axis selection/reversal, basic grid-axis
  self-alignment, and stacking-axis and track-axis content alignment
  (including positional and space-distribution values). It does not yet fully
  implement track stretching, safe-overflow, or writing-mode portions of
  Level 3 alignment. It applies hypothetical min-content contributions and
  positive free-space stretching for simple all-auto column lanes, but does
  not yet generalize that track-sizing step to mixed, spanning, nested, or
  non-column cases.
  A single intrinsic auto-repeat track supports simple hypothetical
  max/min-content contributions and spans, but mixed/multi-track repeats,
  subgrids, fragmentation, and complete writing-mode behavior remain
  incomplete.
- Divergence: Grid Lanes-specific track and flow-tolerance quantities are not
  yet included in the CSS `zoom` used-value boundary.
  <https://drafts.csswg.org/css-viewport/#zoom-property>
- Divergence: full CSS Grid placement behavior is incomplete, including unusual
  `grid`, `grid-template`, and named-line parser edge cases beyond covered
  escaped custom-ident, bracketed line-name tokenization and non-empty
  validation, non-negative track breadth validation, and template-area string
  decoding. Same-page placement for simple backward named spans from a
  definite end line and negative named line placements into startward implicit
  tracks works for non-repeat and finite numbered-repeat column and row grids,
  including area-created explicit tracks, and for fixed-size definite
  `auto-fill` column and row grids. Simple backward named spans and negative
  named line placements also work before fixed-size definite `auto-fit`
  column and row grids, including empty-track collapse after startward
  implicit-track expansion. Positive named implicit line placement after
  fixed-size definite `auto-fill` and `auto-fit` repeats is covered on both
  axes with cycled auto tracks, including multi-track `auto-fill` repeat
  fragments and forward named spans after single-track `auto-fill` fragments,
  but writing modes, broader auto-repeat combinations, and broader
  implicit-grid combinations remain incomplete.
- Divergence: full grid track sizing is incomplete, including intrinsic tracks,
  flexible tracks in all sizing contexts, spanning item contributions, and
  auto-repeat behavior beyond simple cases.
- Divergence: full grid intrinsic sizing is incomplete, including min-content
  and max-content contributions from complex grid items, broader row-axis
  spanning effects, and indefinite container sizes beyond simple cyclic
  percentage explicit-column-as-auto and column-gap intrinsic-width handling,
  simple cyclic percentage row-gap intrinsic sizing and final fixed-track
  same-page layout handling, plus
  simple indefinite percentage grid-item block-size handling for min-content rows.
  Covered column intrinsic placement includes simple all-auto, numeric and
  named-line definite-row constrained auto-flow, positive numeric and positive
  named implicit column starts/ends, forward named implicit column spans on the
  after-explicit side, simple backward named implicit spans from a definite end
  line into the startward side, authored-grid extension, and area-created
  explicit column sizing.
- Divergence: grid baseline alignment, exported baseline synthesis, and
  grid-specific self-alignment are incomplete beyond covered simple same-page
  horizontal first- and last-baseline self-alignment for items in the same row,
  same-page horizontal `justify-self`/`justify-items` `self-start`/`self-end`
  physical inline placement for LTR/RTL grid items, same-page horizontal
  `align-self`/`align-items` `self-start`/`self-end` physical block-axis
  placement for LTR/RTL grid items, same-page horizontal
  `justify-self` and `justify-items` `left`/`right` physical placement in
  LTR/RTL grids, and
  same-page horizontal grid container first/last exported baselines from
  occupied grid rows for inline-grid, nested grid, and simple spanning row-edge
  baseline-sharing cases, including fragmented baseline-sharing groups,
  broader spanning synthesis, orthogonal writing modes, and broader
  writing-mode-specific behavior.
- Divergence: grid-aware absolute static positions are incomplete for complex
  placement beyond covered same-page fixed explicit lines, simple template
  areas, simple intrinsic/flexible tracks, fixed-size `auto-fill`,
  start-aligned, positional content-aligned, and distributed content-aligned
  numeric/named `auto-fit` collapsed repeated lines on both axes, and
  template-area generated lines. Covered fixed-track explicit lines include
  content alignment, and covered numeric/named implicit lines on either side
  of the explicit grid can use definite cycled `grid-auto-*` tracks, including
  after-explicit row and column lines outside the grid container with
  line-edge offsets and covered horizontal static-rectangle end edges that do
  not include the following implicit gutter.
  Named explicit lines inside finite numbered repeats are covered on both
  axes, and named fixed-size `auto-fill` and collapsed `auto-fit` repeated
  lines, including multi-track repeated fragments and before-/after-explicit
  named implicit-line offsets with definite `grid-auto-*` tracks, are covered
  on both axes. Descendants whose effective containing block is the grid
  container use the resolved grid area for covered explicit placement, while
  all-auto grid lines retain the grid container's CSS containing block. The
  covered orthogonal positioned-item automatic-height cases use the logical
  inline measure rather than physical text advance; broader implicit-line
  combinations, writing-mode alignment, and fragmented grid cases remain
  incomplete.
- Divergence: grid fragmentation in paged media is not implemented.
- Divergence: Grid scroll-container geometry resolves axis longhands and
  clips stretched item paint in the paged backend, but native scrollbar
  tracks/thumbs and scrolling interaction are not painted or exposed by PDF.
- Divergence: the legacy WPT references for `display-grid.html` and
  `display-inline-grid.html` distribute CSS table row heights rather than
  preserving the authored fixed Grid tracks; Quire intentionally keeps Grid
  track sizing spec-conformant instead of matching that reference behavior.
- Divergence: unfragmented in-flow subgrids parse the full local
  `<line-name-list>` grammar (including name repeats). Their layout context
  carries nested parent-track slices, inherited and local names, actual gutter
  geometry, clamped explicit placement, and the Grid Lanes fixed-axis geometry
  before lane packing. Explicit normal-flow descendants are recursively
  projected through nested subgrids after preliminary placement and supplied
  as compact parent track-sizing proxies; outer border/padding/margin edges
  are retained in the proxy constraint. Full shared track sizing is still
  incomplete: local-gap differences within an inherited gutter and positioned
  descendants remain unsupported, as do page/column fragmentation. Layout or
  paint containment still resolves
  `subgrid` to `none` as required.
  <https://drafts.csswg.org/css-grid-2/#subgrids>
- Divergence: masonry layout is not implemented.

### CSS Content and Generated Content

- Spec area: CSS Content Level 3, Generated Content for Paged Media, CSS
  Images, accessibility metadata.
- Divergence: generated content cannot use all CSS Images values, including
  conic gradients.
- Divergence: generated image alt text is not emitted into tagged PDF/PDF-UA
  structure. Bitmap OpenType glyph images carry PDF `/ActualText` for text
  extraction, but they are not full tagged-PDF image structure elements.
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

### CSS Multi-column Layout

- Spec area: CSS Multi-column Layout Level 1 and CSS Fragmentation.
- Divergence: column balancing uses iterative snapshot layout and one committed
  fragment pass, and deferred descendant/positioned fragments participate in
  speculative used-column counts. Atomic inline boxes also impose their
  monolithic outer block-size bound, while consecutive floats use
  width-constrained shelf estimates and monolithic floats retain collapsed
  sibling-margin offsets through transparent wrappers. More complex nested
  nested formatting contexts, parallel flows, and complex forced-break
  interactions do not yet converge on the specification's optimal height.
- Divergence: `column-span: all` splits direct and eligible descendant boxes
  into balanced column sets, slices ordinary intervening wrappers, and keeps a
  fitting definite principal box's flow end independent from visible
  descendant overflow. More complex parallel-flow boundaries, full spanner
  margin collapsing, fragment-local wrapper decoration, and some spanner
  continuations across page fragments are incomplete.
- Divergence: vertical spanner auto sizing and single-root orthogonal overflow
  replay are logical-axis aware, but general vertical column placement,
  multi-fragment orthogonal replay, and column-rule geometry still use a
  physically horizontal block-child canvas.
- Divergence: future fragments captured by relative/effect containers retain
  their fragmentainer assignment and positioned principal decoration spans are
  materialized, but fixed-position, static-position, transformed-containing-
  block, and spanner cases do not yet share one durable multicol containing
  block across every nested fragment.
- Divergence: auto-height sequential overflow rows can continue onto later
  pages, and definite-height inline column rows can continue through an outer
  column fragmentainer with final-row balancing. General block/flex/grid/table
  nested rows, balanced `balance` versus `balance-all` page continuation,
  spanner continuation, and named-page geometry changes are incomplete.
- Divergence: named strings, running elements, anchors, links, bookmarks, and
  other side effects produced inside speculative columns need complete
  fragment-local remapping. Conceptual anonymous columns beyond the bounded
  off-canvas replay horizon are represented arithmetically, so side effects
  originating only in that unmaterialized tail are not yet remapped.

### CSS Containment Level 1

- Spec area: CSS Containment Level 1, CSS Sizing, CSS Positioned Layout, and CSS
  Fragmentation.
- Divergence: size containment implements empty intrinsic/used sizing, retains
  authored empty-grid and empty-multicol geometry, applies the internal-table/
  ruby/non-atomic-inline applicability exceptions, and separates a principal
  box's definite fragmentainer consumption from visible descendant overflow.
  Form controls, fieldsets, grid, and table descendants follow the
  empty-principal-box sizing rule. Monolithic overflow through flex, table, and
  positioned contexts plus remaining replaced/table sizing matrices is still
  incomplete.
- Divergence: size-contained principal block boxes and their descendant flow
  retain monolithic placement in ordinary multicol flow while visible overflow
  remains attached without contributing to the principal used size. Equivalent
  behavior through flex, table, and positioned fragmentation contexts is still
  incomplete.
- Divergence: layout containment establishes an independent formatting context,
  positioning containing blocks (including final table-cell grid geometry),
  and a stacking context, and suppresses forced break propagation across the
  containment boundary. Explicit fragmented-flow trapping beyond that
  break-propagation rule is incomplete.
- Divergence: paint containment clips at the used padding edge and establishes
  formatting/positioning/stacking isolation, including degenerate zero-area
  padding-box clips, rounded padding-edge clips, and resolved
  table/table-cell/caption clip geometry. Fragmented effect-group semantics
  remain incomplete.
- Divergence: containment on `html` or `body` suppresses background, overflow,
  and principal writing-mode propagation to the canvas. Viewport propagation
  paths beyond the HTML root/first eligible body rules remain incomplete.

### CSS Multi-column Layout Level 2

- Spec area: CSS Multi-column Layout Level 2.
- Divergence: `column-height` and `column-wrap` parsing, computed values, row
  planning, row gaps, and nested row continuation are not implemented. This
  includes the nested row behavior exercised by `column-height-008`; any prior
  pass came from the old unbalanced Level 1 fallback rather than Level 2
  semantics.

### CSS Containment Level 2

- Spec area: CSS Containment Level 2.
- Divergence: `inline-size` containment has logical-axis intrinsic suppression
  in normal-flow, Flexbox, Grid, and table measurement, but remaining
  replaced-content, multicolumn, and fragmentation interactions are
  incomplete. `style`, `content`, and `strict` parse with style containment;
  style containment scopes counter mutation and generated-quote depth, while
  its remaining fragmentation and generated-content edge cases are incomplete.
  `content-visibility: hidden` skips HTML descendant layout and paint, while
  `auto` is conservatively visible in static paged output; viewport lifecycle
  skipping, SVG text, and full accessibility-tree behavior remain incomplete.
  Container queries and later containment-level features are outside the
  current Level 1 implementation.

### Inline Formatting and CSS Text

- Spec area: CSS Inline Layout, CSS Text, CSS Text Decoration, CSS
  Pseudo-Elements.
- Divergence: nested `::first-line` inheritance is incomplete.
- Divergence: CSS Inline 3 `initial-letter` is incomplete beyond first-letter
  graph splitting, used-size calculation, line-height isolation, and a
  page-local exclusion participant that is distinct from CSS
  floats. Missing areas include
  `initial-letter-align` baseline precision, non-rectangular
  `initial-letter-wrap` contours. `grid` wrapping rounds the rectangular
  exclusion to a shaped containing-text character cell, but does not yet
  integrate glyph contours. Explicit
  length/percentage offsets resolve against the final margin-box logical
  inline width, but do not yet combine with glyph contours,
  vertical writing modes, complex bidi start-position rejection, short
  paragraph page-local replay, and clearing at independent formatting-context
  roots and in vertical writing modes. In horizontal writing modes, ordinary
  short following blocks retain the page-local exclusion, and a subsequent
  initial letter clears prior initial-letter exclusions at the used margin-box
  end (not the strut-leading-expanded wrapping end), without invoking CSS
  float `clear` semantics. In horizontal text, preserved leading whitespace
  before the typographic initial retains its resolved tab advance and the
  combined pseudo's background extent without inflating the ordinary line box.
  Equivalent vertical-writing-mode placement remains incomplete.
- Divergence: generated-content interactions with `::first-letter` and
  `initial-letter` need broader auditing for markers, nested pseudo-elements,
  and fragmented generated inline content.
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
  `replace` semantics, vertical text-orientation interactions, and dynamic DOM
  cases from the CSS Text Level 4 draft.
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
  beyond fixed `@font-face` `font-weight`/`font-stretch` bindings are
  incomplete.
- Divergence: `@font-face size-adjust` scales selected shaping runs, but its
  interactions with metric overrides and fallback faces remain incomplete.
- Divergence: COLR v0 solid-layer glyphs and ordinary CPAL palette selection
  are painted as PDF paths, but COLR v1 paint graphs (gradients, clips,
  composites, and transforms), all `override-colors` edge cases, and palette
  selection across separately styled prepared inline runs remain incomplete.
- Divergence: `unicode-range` behavior has not been exhaustively audited
  against the full CSS Fonts descriptor matching model, invalid descriptors,
  collections, and variable-font instances.
- Divergence: `@font-face` feature defaults are currently looked up by CSS
  family before final face selection. Families with distinct feature
  descriptors across weight, style, stretch, or unicode-range faces can apply
  defaults from a sibling rather than the selected face.
- Divergence: per-glyph synthetic fallback for missing small-caps and
  petite-caps feature glyphs is not implemented.
- Divergence: CFF2 embedding and subsetting are incomplete.
- Divergence: variable-font instance embedding and broad font collection
  coverage are incomplete.
- Divergence: TTC/OTC extraction needs repo-local conformance fixtures beyond
  optional system-font smoke tests.
- Divergence: OS/2 embedding permission failures are not exposed through a
  strict public PDF/A/PDF/UA render option.
- Divergence: inline content-area metric mapping can still diverge from
  browser/Pango behavior in underspecified multi-font cases because Quire
  intentionally uses an em-box policy for non-replaced inline backgrounds and
  borders.
- Divergence: residual complex-script shaping mismatches may remain outside
  retained selected-source ranges and known join-control/tatweel boundary
  cases.

### Tables

- Spec area: CSS Tables, CSS 2.2 table layout, HTML table semantics, CSS
  Overflow, CSS Fragmentation.
- Divergence: anonymous table object construction is incomplete for malformed
  edge cases beyond common fixup cases, authored empty rows, inline wrappers,
  and HTML span parsing.
- Divergence: rare auto table width edge cases need broader conformance
  coverage, especially column groups, collapsed-column interactions, and less
  common multi-span combinations.
- Divergence: CSS Overflow 3 behavior for table cells and block descendants is
  incomplete, including scrollbar painting and viewport scrolling. Collapsed
  row/column tracks retain a union-of-regions clip, but complex nested and
  fragmented descendants still need broader conformance coverage.
- Divergence: CSS Tables 3 row-group structural backgrounds, and structural
  backgrounds in fragmented tables, do not yet consistently use the required
  cell-derived positioning areas and clips for spans. Vertical root column and
  column-group gradients retain their logical positioning area through the
  final writing-mode projection, but fragmented/repeating structural layers
  still need the same retained-grid treatment.
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
  modes needs broader conformance coverage beyond the root vertical-table
  placement and baseline cases.
- Divergence: repeating row/row-group structural layers, alignment, and rare
  fragmented paths still need broader logical-axis coverage for `vertical-rl`,
  `vertical-lr`, and sideways table roots. Normal table-cell placement,
  collapsed-border line segments, and column background gradients retain the
  table root's logical placement through final projection.
- Divergence: CSS 2.2 table-row and table-row-group margins, borders, and
  padding are not yet consistently ignored as required by the table model.
  The local `row-margin-border-padding.html` and
  `row-group-margin-border-padding.html` reftests retain small raster
  differences.
- Divergence: table height distribution remains incomplete for complex floats
  inside cells, multi-level percentage/min-max matrices beyond direct replaced
  descendants, and complex nested formatting contexts. In particular,
  multi-level percentage-height descendants whose resolved block size transfers
  through an aspect ratio do not yet feed their final inline contribution back
  into auto-column sizing.
- Divergence: `text-combine-upright` forms normalized tate-chu-yoko atoms from
  contiguous compatible text words and replays their horizontally shaped
  content through one compressed paint subtree. It does not yet track explicit
  nested-inline scopes, apply the exact vertical baseline/alignment rules, or
  preserve bidi isolation and source ranges through every synthetic atomic
  boundary.

### Box Alignment

- Spec area: CSS Box Alignment, CSS Display, CSS Writing Modes, CSS
  Fragmentation.
- Divergence: `align-content` is incomplete for fragmented block containers and
  fragmented table cells.
- Divergence: `align-content` is incomplete for general multi-column
  containers.
- Divergence: grid alignment is incomplete wherever grid layout itself is
  incomplete; writing-mode-sensitive `self-start`/`self-end` is only covered
  for same-page horizontal grid `justify-self`/`justify-items` inline-axis and
  `align-self`/`align-items` block-axis placement of LTR/RTL items, and
  physical `left`/`right` justify self-positioning from `justify-self` and
  `justify-items` is only covered on the same-page horizontal grid path.
- Divergence: block containers outside flex/table alignment paths use baseline
  fallback behavior rather than full baseline sharing semantics.

### Flex Layout and Fragmentation

- Spec area: CSS Flexible Box Layout, CSS Box Alignment, CSS Fragmentation.
- Divergence: flex fragmentation does not yet apply the complete
  `box-decoration-break: slice` model to padding, borders, backgrounds, and
  outlines for complex line/item continuations, particularly in nested column
  fragmentainers.
- Divergence: split flex-item replay has durable table paint state, but not a
  general source-continuation contract for ordinary block and inline child
  formatting contexts. A descendant that spans a flex source slice can
  therefore be replayed from its beginning rather than its committed child
  continuation, notably for wrapped row lines and zero-capacity multicolumn
  fragmentainers.
- Divergence: vertical-writing multicolumn destination projection handles a
  single committed flex continuation, but complex multi-line and nested
  fragment sequences do not yet map every committed flex fragment to its final
  physical column interval.
  <https://www.w3.org/TR/css-flexbox-1/#pagination>
  <https://www.w3.org/TR/css-multicol-1/#pagination-and-overflow-outside-multicol>
  <https://www.w3.org/TR/css-writing-modes-4/#abstract-box>
- Divergence: positioned descendants of flex items retain their static
  rectangle across a temporary multicolumn pass. Physical-column flex static
  intervals replay through each intersected committed source fragment before
  projection. Physical-row descendants with a resolved block-start inset beyond
  the first committed source fragment select and replay the matching candidate
  fragment, but auto/block-end inset ownership, nested column/page containing
  blocks, clipping, and stacking order are still incomplete.
- Divergence: flex fragment metadata is incomplete for links, running
  assignments, named pages, and other PDF side effects.
- Divergence: split flex-item replay still lacks a general child continuation
  contract. Nested tables and other independently fragmenting formatting
  contexts can require their own resumable state rather than off-page
  source-layout clipping, particularly for repeated table headers and footers.
- Divergence: remaining indefinite percentage cross-size interactions outside
  definite stretch sizing are incomplete.
- Divergence: flex container baseline export remains incomplete for the
  `flexbox-baseline-multi` matrix. Final layout resolves each line's
  measured/synthesized first and last baseline participants once after final
  remeasurement, including CSS Align fallback for incompatible or sole
  participants. Intrinsic nested-flex estimates honor a line's shared
  baseline before startmost-item fallback, and that fallback follows
  order-modified line order without reversing it again for `*-reverse`.
  Remaining multi-line cases still differ in final line-content/inline
  placement after normal or stretch packing, and parent inline layout still
  lacks transport for exported physical-horizontal baseline coordinates.
  <https://www.w3.org/TR/css-flexbox-1/#flex-baselines>
  <https://drafts.csswg.org/css-align-3/#baseline-align-self>
- Divergence: automatic vertical-writing flex container sizing and pagination
  still need broader conformance coverage for physical margins, mixed
  row/column wrap modes, and percentage gaps. The top-margin prebreak loop
  and nested bottom-to-top float anchoring are resolved, but normal-flow
  vertical reference sizing can still create incorrect physical fragment
  geometry where an auto physical height should be content-sized; final
  fragment geometry for these combinations is not yet exhaustively verified.
  <https://www.w3.org/TR/css-flexbox-1/#layout-algorithm>
  <https://www.w3.org/TR/css-writing-modes-4/#abstract-box>
- Divergence: intrinsic flex-item block-size estimation does not yet project
  every orthogonal descendant's physical outer extent onto its parent's logical
  block axis. The empty-inline fallback preserves common definite descendant
  extents, but mixed-writing-mode nested block stacks and multiline orthogonal
  items still require a typed logical-axis contribution walk.
  <https://www.w3.org/TR/css-flexbox-1/#algo-cross-item>
  <https://www.w3.org/TR/css-writing-modes-4/#orthogonal-flows>
- Divergence: the Flexbox Level 2 `flex-wrap: balance` algorithm is only
  partially implemented. It still needs complete intrinsic cross-size handling
  for balanced column containers and full `flex-line-count` cross-axis
  measurement/reflow behavior, including percentage-sized descendants and
  available-size cases:
  <https://drafts.csswg.org/css-flexbox-2/#algo-balance>.
- Divergence: nested flex layout remains incomplete where cyclic percentage
  cross sizes or fragmented nested line replay require the parent to preserve
  unresolved line constraints across formatting-context boundaries.
  <https://www.w3.org/TR/css-flexbox-1/#algo-cross-item>
- Divergence: exact child-height estimates are incomplete for flex items with
  column, complex multicolumn, or deeply nested descendants.
  In particular, a content-based row flex item whose inline descendants are
  entirely atomic can retain an inherited line-strut contribution after its
  used main size is established, even when ordinary block/float replay has a
  shorter used block size. This affects the remaining
  `flexbox-flex-basis-content-003*` reftests; the flex sizing path must share
  the final inline line-box construction rather than approximate it from the
  intrinsic sequence.
  <https://www.w3.org/TR/css-flexbox-1/#algo-cross-item>
  <https://www.w3.org/TR/css-inline-3/#line-layout>
- Divergence: flex intrinsic sizing may diverge from browser behavior if the
  CSS Flexbox draft's web-compatible intrinsic sizing algorithm differs from
  the concrete ideal max-content flex-fraction algorithm.

### Pagination and Fragmentation

- Spec area: CSS Fragmentation, CSS Paged Media.
- Reference-only compatibility: CSS2
  `row-page-break-inside-avoid-2-print.html` is assigned
  `rowgroup-page-break-inside-avoid-5-print-ref.html` as its reference. The
  test source has no table header, while that reference adds a `<thead>` with
  `page-break-after: always`; the resulting output cannot be made equivalent
  by conforming pagination of the test input. Quire intentionally does not
  synthesize the reference's header or add a test-specific fragmentation
  exception.
- Divergence: pagination is cursor-oriented rather than driven by durable
  fragment objects with available block-size negotiation.
- Divergence: some non-block continuation paths (notably complex flex/grid
  descendants) can still retain a preceding page's used percentage basis when
  the destination page has different geometry.
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
- Divergence: root and body auto-height block flow can overestimate the final
  in-flow extent after nested inline-block and flex descendants, causing a
  fitting page-area sequence to fragment onto an extra page.
- Divergence: oversized atomic line boxes consume page fragmentainers and
  project continuous paint, but monolithic glyph/inline-atom ink that crosses a
  slice boundary is not yet uniformly prebroken or kept atomic in every inline
  formatting path.

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
- Divergence: background layer support for normal boxes, page boxes, and margin
  boxes lacks unsupported CSS Images features beyond URL raster images and
  Level 3 linear/radial gradients.
- Divergence: page-margin variable-dimension sizing does not yet fully solve
  genuinely indefinite or interdependent orthogonal cross-axis constraints, so
  some center/middle distribution cases remain incorrect.
- Divergence: CSS Page 3 leaves non-CSS2 properties, including
  `writing-mode`, undefined in margin contexts. Quire preserves ordinary CSS
  Writing Modes rendering there; three imported page-margin reftests
  (`dimensions-004`, `dimensions-013`, and `dimensions-014`) expect a distinct
  compatibility behavior for vertical generated content. The WPT runner
  records their known visual differences through exact-path expected-difference
  thresholds; this does not change Quire output.
  <https://www.w3.org/TR/css-page-3/#page-properties>

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
- Divergence: direct vector paint and CSS gradients use CSS Color 4
  predefined RGB ICCBased PDF color spaces. Direct paint representable in
  sRGB is canonically emitted as sRGB; all other direct RGB and PCS paint is
  converted to Display-P3. Same-space gradients retain their interpolation
  space. PDF/A converts all paint to its tagged sRGB output condition.
  PNG/JPEG raster images preserve valid embedded RGB ICC profiles in ordinary
  PDF and convert them to sRGB for PDF/A. Non-gradient patterns, SVG image
  decoding, and unsupported/non-RGB image profiles remain incomplete. CSS
  Color 4 local-MINDE gamut mapping is not implemented. Relative colors are
  limited to `currentcolor` origins and the channel references presently
  modeled by the CSS Color WPT coverage.
- Divergence: CSS Color 5 `color-mix()` supports the built-in interpolation
  spaces, polar hue methods, ordered color lists, literal percentages,
  `currentcolor` at a concrete property-resolution boundary, and analogous
  missing-component replacement. Percentage `calc()` expressions, custom
  `@color-profile` spaces, and retaining an unresolved `currentcolor` mix in
  reusable generated-image values remain unsupported. `contrast-color()`
  currently accepts one concrete color or `currentcolor` and chooses black or
  white with WCAG relative luminance; configurable user-agent contrast
  algorithms and wider argument support remain. Print CMYK/spot output,
  non-RGB image-profile conversion, and full system-color integration with the
  host platform are not implemented.
- Divergence: CSS Borders 4 `corner-shape` exact inset border offset behavior
  is incomplete.
- Divergence: `corner-shape` integration with logical and side shorthands is
  incomplete. Non-blurred outer and inset shadows retain rounded and
  corner-shaped contours, but blur rasterization, negative-spread edge cases,
  and fragmentation remain incomplete.
- Divergence: CSS Borders 4 `border-shape` supports one or two `circle()` or
  `ellipse()` paths with typed geometry-box resolution, relevant-side color
  selection, outlines, non-blurred shadows, and descendant overflow clipping
  for normal-flow, positioned, and atomic replaced content. Collapsed resolved
  contours suppress descendant content rather than reverting to a rectangular
  clip.
  The line-only absolute subset of `shape()`, non-rounded `inset()`, and the
  default-fill-rule `polygon()` form are also retained as typed path vertices;
  two basic shapes may use heterogeneous primitives and geometry boxes.
  Uniform rounded `inset()` radii are retained through paint-time resolution.
  Multi-value/slash `inset()` radii, custom polygon fill rules,
  `rect()`/`xywh()`, curves/arcs and relative commands in `shape()`,
  interpolation, non-linear or repeating gradient path clipping, and complete
  clipping composition for unsupported shape forms remain incomplete.
- Divergence: rounded dashed and dotted borders do not have exact phase
  distribution along curved corner arcs.
- Divergence: antialias-equivalent side transitions and full corner conflict
  behavior are incomplete.
- Divergence: border-image sources support URL images, configured-decoder
  `image-set()` selection, and generated CSS color, linear, radial, and conic
  gradients. Unsupported CSS image functions and exact vector-gradient
  sampling in nine-slice tiles remain incomplete.
- Divergence: border-image `round` mode still has incomplete tile sizing in
  some mixed-axis and subpixel cases.
- Divergence: decoration fragmentation for borders is incomplete.

### Backgrounds, Images, and SVG

- Spec area: CSS Backgrounds and Borders, CSS Images, SVG, compositing.
- Divergence: inline and URL SVG supports normalized path geometry, solid
  fills/strokes, `pad` linear/radial gradients (including coincident hard
  stops and stop opacity), path-based `clipPath`, group opacity, isolation,
  blend modes, and normalized solid-vector SVG pattern tiles. Pattern tiles
  preserve their affine placement and can contain supported solid vector paths.
  `repeat`/`reflect` spread methods, pattern tiles with nested paint servers,
  transparency/effects, images, or text, SVG images, text, masks, filters,
  nested SVG resource loading, SVG fonts, `<object>`, and `<embed>` remain
  unsupported; affected SVG subtrees are omitted rather than approximated.
- Divergence: inline SVG presentation-attribute `transform-origin`,
  CSS-sourced affine transforms, and inherited host-CSS `fill`, `stroke`, and
  non-percentage `stroke-width` cross into the SVG scene before construction.
  General SVG fill/stroke/view-box selection (especially paths, strokes, and
  nested viewports), CSS paint servers/context paints/percentages, full
  host-CSS overflow semantics beyond root `overflow: visible`, and
  font-relative, viewport-relative, or math CSS
  transform values remain incomplete.
- Divergence: the host-CSS-to-inline-SVG display bridge handles `none` and the
  eligible `contents` container/text/use cases, but not the full SVG display,
  rendering-tree, and shadow-tree rules.
- Divergence: SVG path rendering does not yet preserve `stroke-linejoin:
  miter-clip`, marker cases that depend on unsupported paints/effects, or SVG
  text accessibility/selectability.
- Divergence: URL SVG is supported by existing image consumers (`<img>`,
  backgrounds, border images, list markers, and generated content), but only
  for preloaded file/HTTP or `data:` resources; stylesheet and inline-style
  background URLs are preloaded before layout. SVG resources nested through
  SVG `<image>` are denied rather than recursively preloaded.
- Divergence: SVG backgrounds retain omitted, percentage, and `viewBox` root
  intrinsic-dimension metadata for `background-size`, including finite
  painting of opaque uniform regions from extreme-ratio `cover`/`contain`
  images. SVG `<view>` fragments select a viewBox for both sizing and vector
  painting when the root has no viewBox. Replacing an existing root viewBox
  can still rasterize differently from the corresponding CSS reference
  construction.
- Divergence: raster and URL-SVG replaced images support the `fill`,
  `contain`, `cover`, `none`, and `scale-down` concrete-object sizing modes
  and a shared `object-position` model, but embedded-document fallback
  behavior, transforms, opacity, compositing, and fragmentation remain
  incomplete.
- Divergence: animated GIF and WebP image sources render only their first
  composited frame on the logical screen. Frame timing, looping, disposal, and
  later-frame compositing are not supported in static PDF output.
- Divergence: animated JPEG XL sources render only their first decoded
  keyframe. Frame timing, looping, blending, disposal, and later frames are
  not supported in static PDF output.
- Divergence: PNG images without an `iCCP` or `sRGB` chunk preserve their
  color encoding only when both `gAMA` and `cHRM` are present. Images that
  declare only one of those chunks still fall back to sRGB rather than
  retaining their declared gamma or chromaticities.
  <https://www.w3.org/TR/png-3/#11gAMA>
  <https://www.w3.org/TR/png-3/#11cHRM>
- Divergence: JPEG XL decoding is not yet visually equivalent for the WPT
  `bitdepth-16bpc-reftest`, `cmyk-basic-conversion-reftest`,
  `{hdr,sdr}-alpha-reftest`, `patches-reftest`,
  `progressive-{1,2,3}.html`, `progressive-dc-observable-reftest`, and
  `vardct-large-blocks-reftest` reftests. Their remaining deltas cover
  high-bit-depth sample values, CMYK and alpha conversion, patches,
  progressive codestream rendering, and large VarDCT blocks.
- Divergence: physical `contain-intrinsic-size` and its logical inline/block
  longhands supply fallback sizes for size-contained normal blocks, basic
  intrinsic contributions, and basic replaced-image sizing without inventing
  a natural aspect ratio. `auto`/remembered-size syntax and propagation
  through all flex/grid/table, max-content, and normal-flow writing-mode
  sizing paths remain incomplete.
- Divergence: `image-orientation: from-image` and `none` select distinct raw
  or EXIF-oriented raster resources for replaced elements, ordinary raster
  backgrounds, border images, list markers, and generated-content URL images.
  Legacy EXIF metadata in PNG `zTXt` `Raw profile type exif` chunks is ignored.
  Orientation handling remains incomplete for masks, SVG, and image documents.
- Divergence: CSS linear/radial backgrounds use native PDF shadings, including
  repeated color lines, transition hints, hard stops, alpha masks, and
  zero-period or physically unresolvable gradient-average colors; uniform
  gradients use solid paths.
  Conic gradients still use tile-sized raster fallbacks because a native
  two-dimensional PDF shading representation is not yet implemented.
- Divergence: conic gradients support ordinary/repeating color stops, `from`
  angles, `at` positions, and interpolation methods, but do not yet support
  mixed angle/percentage `calc()` expressions or all relative-length positions
  required by CSS Images Level 4.
- Divergence: `image-set()` selects a supported 1dppx candidate for
  background, generated-content, border-image, and URL-backed list-marker
  sources; it applies the
  selected candidate's intrinsic-resolution scaling and discards MIME types
  unsupported by Quire's enabled decoders. Nested image values, dynamic
  output-resolution changes, complete `calc()` resolution arithmetic, and the
  remaining Level 4 option grammar remain incomplete.
- Divergence: CSS Values 5 request URL modifiers are retained and CORS and
  integrity checks are enforced for decoded CSS URL images, but
  `referrer-policy` request semantics and modifier enforcement for non-image
  URL consumers remain incomplete.
- Divergence: generated gradients are not shared across all image-consuming
  properties, including `border-image`, markers, and masks.
- Divergence: `box-shadow` blur rasterization, negative-spread corner
  normalization, and fragmentation are incomplete. Non-blurred rounded,
  corner-shaped, circular, and elliptical outer/inset shadows are supported.
- Divergence: repeated raster background images are not yet raster-equivalent
  to equivalent replaced-image painting at all scaled tile sizes.

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
- Divergence: HTML `transform-box: content-box` and `border-box` resolve the
  2D transform reference rectangle, including reconstructible percentage
  padding. SVG fill/stroke/view boxes, nested viewports, and non-reconstructible
  layout percentage-padding cases remain incomplete.
- Divergence: CSS `matrix3d`, three-axis translate/scale, axis-angle rotation,
  z transform origins, and hidden-backface suppression use typed Euclid
  homogeneous matrices when their flattened result is affine. Projective
  `perspective`, `transform-style: preserve-3d` depth composition/sorting,
  parent-derived backface orientation, and transform animations remain
  unsupported. Independent transforms still support only their 2D forms.
- Divergence: fragmented overflow clips need broader conformance coverage.
- Divergence: `overflow-clip-margin` supports the CSS Overflow 3 shorthand
  for non-negative lengths and visual boxes, including rounded and
  corner-shaped contour expansion. The Level 4 logical/physical longhands and
  independent per-side offsets are incomplete. Axis-selective deferred paint effects and several
  SVG/replaced-element clip paths still use rectangular two-axis scopes.
  <https://www.w3.org/TR/css-overflow-3/#overflow-clip-margin>
- Divergence: fragmented floats do not fully let descendant stacking contexts
  escape into the ancestor context while preserving fragment-local coordinates.
- Divergence: transformed ancestors capturing fixed containing blocks across
  fragments are incomplete.
- Divergence: transformed containing-block behavior is incomplete in less
  common formatting-context and fragmentation cases.
- Divergence: exact static-position placeholders are incomplete for multiline
  inline, table, fragmented, and other remaining formatting-context cases;
  simple single-line inline static rectangles use the prepared hypothetical
  placeholder geometry.
- Divergence: positioned inline containing blocks use collected first/last
  edge markers for ordinary single-line horizontal block-level absolute
  descendants, but multi-line, bidi, vertical-writing-mode, transformed,
  table, and fragmented inline contexts do not yet retain one final
  fragmentainer-aware rectangle.
- Divergence: flex static-position rectangles supply the CSS Alignment
  centered automatic-size constraint on the physical horizontal axis, but
  orthogonal and physical-vertical automatic sizing still uses the block-flow
  minimum-content measurement rather than the required fit-content extent.
  <https://drafts.csswg.org/css-align-3/#abspos-sizing>
- Divergence: positioned fragmentation behavior is incomplete.
- Divergence: `isolation`, blend modes, filters, masks, and `clip-path`,
  containment-triggered paint isolation, `content-visibility`, and
  `will-change` lack full visual semantics.
- Divergence: filter functions are retained but not rendered.
- Divergence: masks and `clip-path` geometry beyond comma-delimited
  nonzero-fill `polygon()` values in the default border box are not
  shape-aware. In particular, other basic shapes, `evenodd`, geometry-box
  selection, and URL references are not rendered.
- Divergence: `will-change` is only a pre-isolation trigger.

### Units and Value Computation

- Spec area: CSS Values and Units, CSS Cascade, CSS Sizing.
- Divergence: container query units (`cqw`, `cqh`, `cqi`, `cqb`, `cqmin`,
  `cqmax`) parse, survive CSS math, and use the required small-viewport
  fallback when no container is available, but layout does not yet select an
  eligible ancestor container for them.
- Divergence: `ex` and `cap` resolve from the selected font for box-model
  `<length-percentage>` values (including CSS math), and `ic` resolves from
  the WATER glyph advance for direct box-model values. `lh` resolves against
  the inherited line height in `font-size` and `line-height`. These metric
  units are not yet supported property-wide or throughout deferred math; `rlh`
  remains incomplete.
- Divergence: `rem` uses the document root's resolved font size for CSS sizing
  properties, but this root-relative basis is not yet applied property-wide to
  every computed `<length-percentage>` value.
- Divergence: typed CSS `attr()` resolves basic `raw-string`, `type(*)`,
  `<color>`, and `<length>` substitutions (including prefixed XML attributes),
  but its remaining type grammars, unit keywords, tainting, and animation
  behavior are not implemented.
- Divergence: CSS Animations parses named `@keyframes` and snapshots one
  negative-delay animation for interpolable box-size values. Multiple
  animations, timing functions other than linear, fill/direction/iteration
  behavior, animation events, compositor-time animation, and interpolation of
  other property types remain incomplete.
- Divergence: CSS Values Level 5 `calc-size()` retains only affine arithmetic
  in `size` (`multiplier × size + <length-percentage>`) and has no complete
  expression tree for nested `calc-size()`, `min()`, `max()`, `clamp()`, or
  other non-linear CSS math. `interpolate-size` is also not implemented.
- Divergence: many property-specific keywords and computed-value models are
  incomplete.
- Divergence: CSS Sizing Level 4 `aspect-ratio` transfers definite normal-flow
  block dimensions (including constrained ratio-derived block sizes), handles
  basic absolute-positioned non-replaced axes, and resolves basic grid
  track/item dimensions after grid area sizing (including normal versus
  explicit stretch alignment). Intrinsic inline collection also distinguishes
  a cyclic percentage inline size from a definite block-size basis for atomic
  canvas sizing, including explicitly-sized float intrinsic measurement.
  Integration remains incomplete for wrapped/intrinsic grid and flex sizing,
  table, nested percentage-height absolute containing blocks, and remaining
  non-raster replaced-element edge cases.
- Divergence: CSS Sizing Level 4 `stretch` integration is incomplete across
  block-height, table, grid, absolute positioning, and intrinsic sizing
  contexts. Normal-flow width min/max constraints now preserve margin-box
  stretch-fit semantics.
- Divergence: multi-layer backgrounds preserve and normalize the ordinary
  image-layer count, but painting remains incomplete for `background-clip:
  text`, per-layer fixed/local attachment geometry, and several table and
  fragmented-box painting contexts. CSS Backgrounds and Borders Level 3
  layering: <https://www.w3.org/TR/css-backgrounds-3/#layering>.
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
- Divergence: Media Queries support captures a static renderer environment at
  parse time; dynamic environment recascading, custom media queries, and the
  remaining Media Queries feature set are incomplete.
- Divergence: `@supports` declaration conditions share the cascade's canonical
  modeled declaration-operation parser. CSS Conditional feature-query
  functions beyond `selector()` (including `font-format()` and `font-tech()`)
  remain incomplete. Container-query evaluation is retained for a later
  layout-time implementation.
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
- Divergence: HTML form-control rendering semantics are incomplete. In
  particular, `button { display: grid }` with `button::first-letter` does not
  yet model the control-specific anonymous display and pseudo-element behavior
  required by CSS Display, CSS Pseudo-Elements, and HTML form controls.

### CSS Color Adjustment

- Spec area: CSS Color Adjustment Level 1 forced colors.
- Divergence: script-driven forced-colors and backplate reftests are not
  supported because Quire has no JavaScript runtime or platform text backplate
  compositor.
- Divergence: `color-scheme`, `print-color-adjust`, and the remaining CSS Color
  Adjustment properties do not yet provide full used-value behavior.

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
# CSS Generated Content for Paged Media: footnotes

GCPM footnotes are partially implemented. `float: footnote`, the generated
call/marker pseudo-elements, source-order `footnote` counter, a dedicated
`@footnote` area, and page-local fixed-point reservation are supported.

The following requirements remain divergent from
<https://www.w3.org/TR/css-gcpm-3/#footnotes>:

- `@footnote` sizing constraints (including `max-height`) do not yet
  participate in the footnote area's used geometry. Its margins, borders, and
  padding reserve space, and the area paints its background, borders, and
  images around the measured body reservation.
- `footnote-display: inline` and `compact` are parsed but bodies currently
  stack as block footnotes.
- A footnote call is generated at the source position, but the current inline
  collector may place it in a distinct line item rather than preserving the
  surrounding source text run.
- `footnote-policy: line` and `block` are parsed but do not yet constrain the
  break chosen for a deferred footnote body; page-local reservation currently
  uses the `auto` behavior.
- Oversized footnote bodies do not yet use the GCPM overflow/defer algorithm;
  their normal block fragmentation is used as a fallback.
- A page-local queue is used to defer body painting until page commit. Its
  cross-page continuation handling is incomplete: an overflowing note can
  corrupt the first footnote area's content or fail to paint a later body.

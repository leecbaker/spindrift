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

### CSS Fonts language-system overrides

- `font-language-override` cannot yet preserve a raw, case-sensitive OpenType
  language-system tag through Parley's BCP-47 locale API to HarfBuzz shaping.
  This leaves `css/css-fonts/font-language-override-03.html` divergent. See
  <https://drafts.csswg.org/css-fonts/#font-language-override-prop> and
  `docs/PARLEY_SHORTCOMINGS.md`.

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
- Divergence: line-edge effects for `pre-wrap` leading spaces, floats, and
  intrinsic sizing are incomplete. The graph now records selected multi-run
  Unicode-space suffixes by source range and preserves their paint ownership;
  selected lines also retain their UAX #9 visual sequence, transparent
  inline-box edge ownership, and resolved indent/justification span. PDF
  extraction still needs a dedicated source-range emission path, and
  decoration ownership across complex fragmented bidi boundaries remains
  incomplete.
  The `trailing-space-and-text-alignment-002`, `004`, and `rtl-002` WPT
  references use an `X` glyph as a visible proxy for a blank preserved U+0020
  at a scrollport edge. Quire keeps the source space and its no-ink glyph, but
  PDF rasterization anti-aliases the coincident source-coverage and
  overflow-clip edges differently from the reference's extending `X`
  coverage. This leaves 82/82/2 pixels despite identical line measures and
  positions; it is an exact-raster interoperability gap, not line-breaking or
  font-subsetting substitution.
  <https://drafts.csswg.org/css-text-3/#white-space-phase-2>
- Divergence: `trailing-other-space-separators-break-spaces-013` still differs
  from its Ahem-font reference when U+202F is followed by CJK text. Quire
  preserves U+202F's ordinary `GL` protection at text and atomic boundaries
  and keeps its `break-spaces` advance, but fallback-glyph line widths select
  different CJK line packing. This is a remaining font-fallback/line-measure
  interoperability divergence, not permission to split French U+202F
  punctuation pairs.
  <https://www.w3.org/TR/css-text-3/#white-space-phase-2>
  <https://www.unicode.org/reports/tr14/#GL>
- Divergence: tab stops use a shared logical cursor through text and atomic
  inline content, but float-displaced lines do not yet carry their block
  content-edge coordinate through graph fitting and painting.
  <https://drafts.csswg.org/css-text-3/#tab-size-property>
- Divergence: textarea and carriage-return transitions across mixed
  white-space descendants do not yet use fully conformant shared CSS Text
  whitespace processing.
- Divergence: `word-break: auto-phrase` has source-faithful phrase analyzers
  only for declared Thai (Kham dictionary tokens plus named entities) and
  Japanese (particle tailoring), rather than full language-specific phrase
  analysis for all languages and scripts.
  <https://drafts.csswg.org/css-text-4/#valdef-word-break-auto-phrase>

- Divergence: `word-space-transform` does not yet implement language-sensitive
  `auto-phrase` segmentation or its outermost inline-boundary placement.
  Explicit U+200B and `<wbr>` separators preserve their layout-only source
  ownership after transparent-edge and forced-boundary context is known.
  <https://drafts.csswg.org/css-text-4/#word-space-transform>
- Divergence: generated Type 0 font `/ToUnicode` mappings are not reliably
  extractable for macOS CJK system fonts. In the Taiwanese numeral comparison
  fixture, `pdftotext` drops the CJK title and cell content and corrupts some
  marked Latin transliteration even though the painted glyphs are correct.
  This prevents faithful text extraction and accessibility.
  <https://www.w3.org/TR/REC-PDF-ISO32000-1-200807/#sec-9.10.3>

### CSS Text Level 4 Wrapping

- Spec area: CSS Text Level 4 `text-wrap-style`.
- Divergence: `text-wrap: balance` retains selected source endpoints through
  source-order float placement and clamp marker fitting. Independently
  balanced pseudo-element block groups and fragmentation boundaries remain
  incomplete.
  <https://drafts.csswg.org/css-text-4/#text-wrap-style>

### CSS Overflow Line Clamping

- Spec area: CSS Overflow Level 4 `line-clamp`, its longhands, and legacy
  `-webkit-line-clamp` compatibility.
- Divergence: Automatic line-clamp selection covers direct inline formatting
  contexts whose used block-size constraint has a definite absolute,
  line-height-relative, or containing-block percentage basis, plus eligible
  in-flow descendant blocks under an absolute or line-height-relative typed
  remaining content-box allowance. Constraints that depend on the enclosing
  block's final percentage basis through mixed/nested block flow and
  reevaluation through multicol balancing remain unimplemented. `continue:
  discard` captures direct-inline cutoffs and direct multicolumn child-prefix
  region breaks (including later spanners) locally, but its complete Category-3
  region-fragmentation remainder model across mixed and nested formatting
  contexts remains unimplemented. Quire also has no JavaScript/CSSOM runtime,
  so script-driven mutation tests for legacy clamping are unsupported.
  <https://drafts.csswg.org/css-overflow-4/#line-clamp>
  <https://drafts.csswg.org/css-overflow-4/#continue>

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
- Divergence: image marker sizing and general marker box geometry do not fully
  follow CSS Lists. Horizontal outside textual markers now use the first
  accepted in-flow line's baseline (and images its block-start edge), including
  normalized block-in-inline and inline-block paths; fragmented, atomic
  `flow-root list-item` descendant-line cases still need broader auditing.
- Divergence: counter snapshots preserve counter creators for source-order
  planning, but normalized generated boxes do not yet retain a complete
  logical counter origin for every fragmentation and replay shape.
- Divergence: vertical writing-mode geometry for outside markers is incomplete.
- Divergence: PDF marker output lacks tagged marker semantics for PDF/UA.
  Bitmap marker glyphs retain replacement text through PDF `/ActualText`, but
  Quire does not yet emit the structure tree needed to associate them with a
  PDF/UA list-label element.
- Divergence: custom counter-style `speak-as` has no observable effect because
  speech output is not generated.

### CSS Custom Properties

- Spec area: CSS Custom Properties, CSS Properties and Values API, CSS Color.
- Divergence: `@property` supports only the exact `<color>` syntax. Valid
  registrations using the universal syntax or any other CSS Properties and
  Values grammar are not computed, and JavaScript `CSS.registerProperty()` is
  unavailable because Quire has no DOM runtime.
- Divergence: registered `<color>` values are serialized at `var()` boundaries,
  but typed computation for every other registered syntax, animation behavior,
  and relative-unit dependency cycles is not implemented.
- Divergence: variable substitution in complex `background` and `font`
  shorthands beyond token-aware `font-family` lists is incomplete.
- Divergence: `@font-face` descriptor handling still needs full CSS Variables
  and CSS Fonts conformance coverage, including invalid descriptor recovery and
  font matching parity.

### CSS Display Model

- Spec area: CSS Display, CSS Lists, CSS Tables, CSS Ruby.
- Divergence: `run-in` display behavior is unsupported.
- Divergence: Ruby display roles, the HTML UA defaults, anonymous role
  normalization (including authored source intra-level whitespace
  counterparts),
  out-of-flow exclusion, default first-level interlinear placement, inherited
  `ruby-position: alternate | over | under`, originating-block `::first-line`
  propagation, non-transformable ruby role handling, base-only
  `::first-letter`, and default paired justification are
  implemented. Annotated ruby columns are still represented by coupled inline
  atoms rather than a base-level inline opportunity range; their base source
  text participates in UAX #14 boundary selection, but full paired
  fragmentation and line metrics remain incomplete (notably the residual
  `ruby-line-breaking-001` mismatch). Generated-role whitespace pairing,
  improper-parent anonymous wrappers, multi-level span sizing, and
  vertical-writing-mode placement remain incomplete.
- Divergence: Ruby normalizes `rbc`/`rtc` styles separately from segment
  styles and resolves `ruby-align` plus `ruby-overhang: spaces` after visual
  line materialization. Legacy `ruby-overhang: none` aliases `spaces`.
  `spaces` supports preserved document spaces/tabs, U+00A0, Unicode `Zs`, and
  the eligible untrimmed fullwidth punctuation shares. `auto` has Quire's
  deterministic UA policy of at most `0.5ic` borrowed from each immediately
  adjacent non-atomic visual text item. This is intentionally not a complete
  glyph-ink collision analysis; complete vertical ruby paint projection is
  still unfinished.
- Divergence: The remaining CSS Ruby behaviors are unsupported or incomplete:
  `ruby-position: inter-character`, `ruby-merge`, auto-hide/collapse, detailed
  bidi isolation/reordering (including ruby-specific reordering), a complete
  `auto` collision policy, vertical ruby placement, and fragmentation within a
  base/annotation pair.
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
- Divergence: table wrappers and block-level replaced boxes still need a
  complete audit against the CSS 2.2 border-box float avoidance rule.
  Flex/grid item replay, table-cell inline planning, multicolumn child flow,
  and captured ruby levels carry explicit float-scope data, but the remaining
  non-block root paths have not yet been audited against that shared boundary.
- Divergence: non-block float-adjacent root placement paths still risk using
  margin-box placement where CSS 2.2 requires border-box collision behavior.
  The remaining audit covers nested and non-block root paths.
- Divergence: zero-width float exclusions do not yet consistently distinguish
  block-axis clearance from inline collision. This remains visible in the
  `zero-width-floats*` CSS2 reftests.
- Divergence: split-flow floats normalized from inline ancestors still need an
  audit across fragmentation and complex float-exclusion replay. The
  `float-no-content-beside-001.html` line-selection case now moves an
  unbreakable line to the first fitting slab, but its full reftest still
  diverges when an explicit `<br>` and `clear: both` replay the following
  paragraph. The remaining CSS2 failures include `float-nowrap-4.html`,
  `float-no-content-beside-001.html`, and
  `floats-line-wrap-shifted-001.html`.

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
- Divergence: `astral-bidi/adlam.html` now resolves and shapes a strong Adlam
  RTL run equivalently to the RTL reference. However,
  `astral-bidi/adlam-anti-ref.html` still fails because an explicit LTR
  `unicode-bidi: isolate-override` run is not distinguished from the
  intrinsic RTL run. CSS requires the inline scope's override controls to
  determine that sequence's UAX #9 ordering.
  <https://drafts.csswg.org/css-writing-modes-4/#unicode-bidi>

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
  Intrinsic `auto-fill`/`auto-fit` syntax is retained for `auto`,
  min-content, max-content, and fit-content tracks. Grid Lanes materializes
  a finite intrinsic auto-repeat before final placement, preserves repeated
  track provenance for `auto-fit` collapse, and sizes end implicit `auto`
  tracks from their own contributions. Explicitly placed Grid Lanes subgrids
  preserve their final grid-axis line topology through nested replay,
  including local `repeat(auto-fill, <line-name-list>)` expansion, empty name
  slots, RTL, and orthogonal writing modes. More complex implicit
  `grid-auto-*` sizing functions, auto-placed or stacking-axis Grid Lanes
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
  same-page layout handling, plus simple indefinite percentage grid-item
  block-size handling for min-content rows. Cyclic final-area
  preferred/minimum/maximum resolution, explicit multi-track flexible-span
  automatic minima, and replaced automatic used sizes in `minmax(auto, 0)`
  tracks are covered; broader automatic-minimum eligibility and
  replaced-element sizing interactions remain incomplete.
  Covered column intrinsic placement includes simple all-auto, numeric and
  named-line definite-row constrained auto-flow, positive numeric and positive
  named implicit column starts/ends, forward named implicit column spans on the
  after-explicit side, simple backward named implicit spans from a definite end
  line into the startward side, authored-grid extension, and area-created
  explicit column sizing.
- Divergence: grid baseline alignment, exported baseline synthesis, and
  grid-specific self-alignment are incomplete beyond covered simple same-page
  horizontal first- and last-baseline self-alignment for items in the same row,
  including the Grid-required exclusion of baseline requests whose item sizing
  depends on an intrinsic track (with first/last fallback alignment and no
  group or exported-baseline participation),
  same-page horizontal `justify-self`/`justify-items` `self-start`/`self-end`
  physical inline placement for LTR/RTL grid items, same-page horizontal
  `align-self`/`align-items` `self-start`/`self-end` physical block-axis
  placement for LTR/RTL grid items, same-page horizontal
  `justify-self` and `justify-items` `left`/`right` physical placement in
  LTR/RTL grids, and
  same-page horizontal grid container first/last exported baselines from
  occupied grid rows for inline-grid, nested grid, and simple spanning row-edge
  baseline-sharing cases. Layout-contained inline-grid baseline suppression and
  the synthesized border-box fallback are covered; full baseline shims during intrinsic track sizing,
  fragmented baseline-sharing groups, broader spanning synthesis, orthogonal
  baseline sharing, and broader writing-mode-specific behavior remain
  incomplete.
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
  all-auto grid lines retain the grid container's CSS containing block. A
  direct child whose actual containing block is outside the grid instead uses
  the grid content box as its static-position rectangle, independent of
  grid-placement. The covered orthogonal positioned-item automatic-height
  cases use the logical inline measure rather than physical text advance;
  broader implicit-line combinations, writing-mode alignment, and fragmented
  grid cases remain incomplete.
- Divergence: grid fragmentation in paged media is not implemented.
- Divergence: Quire's static-PDF user-agent policy uses overlay scrollbars, so
  scrollport geometry, clipping, intrinsic sizing, and final track space do
  not reserve or paint native scrollbar chrome. This applies to all scroll
  containers and the viewport; it is distinct from CSS Overflow clipping and
  layout behavior. Native tracks/thumbs and scrolling interaction are not
  exposed by PDF, and `overflow: auto` does not yet perform the post-layout
  scrollbar-triggered re-sizing required when automatic overflow becomes
  scrollable.
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
- Divergence: generated content supports URL raster/SVG images and Level 3
  linear/radial gradients (with native PDF shading where representable), but
  cannot use all CSS Images values, including conic gradients.
- Divergence: generated image alt text is not emitted into tagged PDF/PDF-UA
  structure. Bitmap OpenType glyph images carry PDF `/ActualText` for text
  extraction, but they are not full tagged-PDF image structure elements.
- Divergence: generated-content cross references resolve same-document
  `target-counter()` and `target-text()` values through bounded fresh layout
  passes, but `target-counters()`, external-document targets, and cyclic
  page-number references that do not converge within the bounded pass budget
  remain unsupported.
  <https://www.w3.org/TR/css-content-3/#cross-references>
- Divergence: `leader()` and generated content layout need broader conformance
  coverage for unusual fragmentation, target cross-reference, and replaced
  generated-content combinations.

### CSS Box Model `margin-trim`

- Spec area: CSS Box Model Level 4.
- Divergence: adjacent opaque flex-item backgrounds made contiguous by
  `margin-trim` can retain a one-device-pixel PDF raster stitching seam when
  separate item paint scopes serialize as separate rectangle fills. Layout
  geometry and used margins are correct; PDF fill coalescing across those
  scopes remains incomplete.
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
- Divergence: non-grid complex endpoint classification is incomplete. Wrapped
  multicolumn rows provide an owned two-axis topology with abutting-gap
  junctions, but spanners, nested fragmentainers, and broader flex endpoint
  graphs do not yet provide one complete container-wide topology.
- Divergence: paged grid gap decoration behavior is incomplete across
  named-page changes, page-size changes, and complex item fragmentation.
- Divergence: multicolumn fragmentation carries row-local two-axis gap
  topology through committed replay, but does not yet retain one canonical
  decoration fragment map across spanners, nested fragmentainers, and page
  continuations.
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
- Divergence: a multicol list item's single marker/first-line ownership is not
  yet preserved when a `column-span: all` boundary partitions normalized
  inline and block content. The equivalent inline-block multicol root also
  needs its final principal block-size and baseline to come from the committed
  segment plan rather than its atomic-inline fallback.
- Divergence: vertical spanner auto sizing and single-root orthogonal overflow
  replay are logical-axis aware, but general vertical column placement,
  multi-fragment orthogonal replay, and column-rule geometry still use a
  physically horizontal block-child canvas.
- Divergence: direct and nested absolute positioned descendants resolve once in
  their continuous source containing block, then replay through the committed
  multicolumn source slices that clip their paint. Exact later-slice static
  positioning and paint order (including the `out-of-flow-in-multicolumn`
  WPT matrix), plus broader fixed-position, transformed-containing-block, and
  spanner combinations, still need nested interoperability coverage.
  The positioned continuation state retains a logical fragmentainer tail
  separately from a materialized destination prefix. A resolved ancestor
  `overflow: clip` now bounds scratch positioned fragmentation before page or
  paint allocation, including a containing block that crosses the clip edge.
  However, clip geometry that is not yet resolved still requires deferred
  positioned replay, and `out-of-flow-in-multicolumn-071` and `-107` retain
  visual conformance divergences in nested projection/stacking. `hidden`,
  `auto`, and `scroll` remain conservatively materialized because static-media
  behavior for their scrollable overflow is not yet modeled.
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
  empty-principal-box sizing rule. Monolithic overflow through complex flex
  child replay and positioned contexts, plus remaining replaced/table sizing
  matrices, is still incomplete.
- Divergence: size-contained principal block boxes and their descendant flow
  retain monolithic placement in ordinary multicol flow while visible overflow
  remains attached without contributing to the principal used size. Equivalent
  behavior through complex flex and positioned fragmentation contexts is still
  incomplete; the simple table page-span path is implemented.
- Divergence: layout containment establishes an independent formatting context,
  positioning containing blocks, and a stacking context (including final
  table-cell grid geometry), resolves forced breaks inside its active local
  fragmentainer, and exports descendant ink without exporting descendant
  scrollable overflow. Explicit fragmented-flow trapping beyond that break
  behavior is incomplete.
- Divergence: paint containment clips at the used padding edge and establishes
  formatting/positioning/stacking isolation, including degenerate zero-area
  padding-box clips, rounded padding-edge clips, and resolved
  table/table-cell/caption clip geometry. Fragmented effect-group semantics
  remain incomplete.
- Divergence: containment on `html` or the eligible `body` resolves the
  root/body `contain` value before principal-flow, background, and
  viewport-overflow selection, including `inline-size` and `style` but not
  `content-visibility` alone. A body whose propagation is disabled is not a
  document-canvas-flow source, but exact orthogonal descendant sizing and
  page-canvas edge coverage for its ordinary background remain incomplete.
  Viewport propagation paths beyond the HTML root/first eligible body rules
  also remain incomplete.

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
- Divergence: nested `::first-line` inheritance remains incomplete for
  fragmented generated content and complex ruby/positioned-inline replay;
  fragment-local `currentcolor` resolution for text, inline edges, border,
  outline, background, and shadow paint is implemented.
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
- Divergence: `::first-letter` selects Unicode `L*`, `N*`, and `S*`
  typographic units and associated punctuation within one source fragment, but
  language-specific first-letter tailoring and complete selection across
  independently collected inline fragments remain incomplete.
- Divergence: an absolutely positioned inline box's static-position rectangle
  and per-line background geometry are not yet derived from the same used
  first-letter line metrics as its final text fragments. This leaves the
  `first-letter-width` reftest with a small exposed background edge.
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
  `replace` semantics, and dynamic DOM cases from the CSS Text Level 4 draft.
- Divergence: fallback transformed vertical glyph forms and vertical or deeply
  fragmented complex-bidi fragment-edge placement remain incomplete for
  `hanging-punctuation`.
- Divergence: `text-decoration-skip-box`, complete
  `text-decoration-skip-self` edge cases, decoration collision refinements, and
  rare fragmented vertical decoration cases are incomplete.
- Divergence: `text-decoration-skip-spaces` does not yet normalize preserved
  `break-spaces` glyph-sequence edges across shaping boundaries that retain
  pair-positioning adjustments, so equivalent split and unsplit source can
  produce a sub-glyph inline coverage difference.
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
- Divergence: synthetic oblique is not applied to emitted PDF glyph outlines.
- Divergence: `font-optical-sizing` is not modeled, so the `font` shorthand
  cannot reset that CSS Fonts reset-only subproperty.
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
- Divergence: CSS 2.2 leaves the content-area metric unspecified when multiple
  fonts are used. Quire consistently uses the primary metric face's em box for
  non-replaced inline backgrounds, borders, and padding; other engines may
  choose a different multi-font measure. This policy is independent of
  `line-height`, as CSS 2.2 requires.
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
  common multi-span combinations. Absolutely positioned table roots now use
  the containing block's logical inline available size before auto-width
  calculation, and authored definite logical block sizes reach row
  distribution; remaining width divergence is outside that abspos handoff.
  Resolved collapsed outer half-borders are converted from table-wrapper
  border-box space to grid space exactly once on both axes, including
  automatic-height absolute tables; this divergence does not include that
  conversion.
- Divergence: CSS Overflow 3 behavior for table cells and block descendants is
  incomplete, including scrollbar painting and viewport scrolling. Collapsed
  row/column tracks retain a union-of-regions clip, but complex nested and
  fragmented descendants still need broader conformance coverage.
- Structural backgrounds for table, column, column-group, row, and row-group
  layers select originating cells and retain a paired source-grid/destination-
  fragmentainer projection with durable source-row slices. The complete
  reported fragmented table-paint matrix remains divergent in horizontal-tb
  LTR, vertical-lr RTL, and vertical-rl RTL: destination-local row/grid slices
  are not yet paired with a continuous table-wrapper border, trailing chrome,
  and caption replay placement across multicolumn fragmentainers. Antialiased
  edge alignment in a few collapsed or column-background comparisons, nested
  writing modes, collapsed borders, and complex repeated chrome/replay also
  remain incomplete.
- Divergence: table fragmentation still lacks complete cloned table-wrapper
  margin/background geometry, decoration truncation, and collapsed-border
  behavior at fragment edges.
- Divergence: flex-specific descendant link handling inside nested table/flex
  fragments is incomplete.
- Divergence: collapsed-track border conflict resolution across fragment
  boundaries has rare unresolved cases.
- Divergence: collapsed row/column clipping has remaining edge cases involving
  partial glyph clipping, rare fragmented table pieces, and complex nested
  formatting contexts.
- Divergence: collapsed-border painting-order cases that combine positioned
  tables, nested tables, and raster-sensitive edge overlaps still need broader
  coverage. Root-level `inline-table` boxes now retain their inline-level outer
  role during mixed block-flow classification and normalization, so their
  table-local collapsed border paint is replayed in source order; `display:
  table` remains block flow. Remaining failures must not be addressed by
  moving the global `TableCollapsedBorder` band.
  <https://drafts.csswg.org/css-display-3/#outer-role>
  <https://drafts.csswg.org/css-tables-3/#anonymous-boxes>
  <https://www.w3.org/TR/CSS22/visuren.html#anonymous-block-level>
  <https://drafts.csswg.org/css-position-4/#painting-order>
- Divergence: root inline-table baseline export uses the table box (rather
  than its wrapper) for ordinary horizontal and vertical parent lines, but
  orthogonal first-row cells and fragmented mixed-writing-mode tables still
  need a complete logical row-baseline source rather than the remaining
  physical-Y row-layout metric.
- Divergence: repeating row/row-group alignment and rare fragmented paths still
  need broader logical-axis coverage for mixed and sideways table roots.
  Normal table-cell placement, originating-cell structural clips, and column
  background gradients retain the table root's logical placement through final
  projection; collapsed-border combinations with inline-table replay remain
  open.
- Divergence: table height distribution remains incomplete for complex floats
  inside cells, multi-level percentage/min-max matrices beyond direct replaced
  descendants, and complex nested formatting contexts. In particular,
  multi-level percentage-height descendants whose resolved block size transfers
  through an aspect ratio do not yet feed their final inline contribution back
  into auto-column sizing.
- Divergence: `text-combine-upright` forms normalized tate-chu-yoko atoms from
  contiguous compatible text words, gives the measured one-em square the
  parent central baseline, reverses full-width forms in multi-unit
  compositions, and replays its horizontally shaped content without clipping
  ink to the measured square. Generated-content atoms do not yet raster-match
  equivalent authored inline content, and inherited compositions cannot carry
  run-boundary provenance through nested atomic formatting contexts. Full bidi
  isolation and source ranges through every synthetic atomic boundary also
  remain incomplete.

### Box Alignment

- Spec area: CSS Box Alignment, CSS Display, CSS Writing Modes, CSS
  Fragmentation.
- Divergence: `align-content` is incomplete for fragmented non-flex block
  containers and fragmented table cells. Wrapped flex lines retain their
  resolved definite cross-axis packing through fragment replay, but broader
  fragmented alignment cases remain unresolved.
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
- Divergence: a materialized flex-item continuation records its source-content
  interval, frozen Flexbox border-box geometry, and either source-slice replay
  or a nested child-fragment ordinal. This covers ordinary descendant-overflow
  replay and forced descendant breaks in an otherwise-unsplit item. A committed
  forced child page span supplies the enclosing flex fragment's first/end
  edges and destination-page flow cursor, including the final
  containing-block handoff for following normal-flow siblings. Complex
  zero-capacity and nested fragmentainer sequences still do not carry a
  complete child disposition through every continuation.
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
- Divergence: nested table and other independently fragmenting child replay
  retains per-item child-fragment ordinals, but does not yet carry a complete
  child layout-state object through every complex nested continuation.
- Divergence: remaining indefinite percentage cross-size interactions outside
  definite stretch sizing are incomplete.
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
  cross sizes or complex fragmented nested line replay require state to cross
  formatting-context boundaries.
  <https://www.w3.org/TR/css-flexbox-1/#algo-cross-item>
- Divergence: exact child-height estimates are incomplete for flex items with
  column, complex multicolumn, or deeply nested descendants.
  <https://www.w3.org/TR/css-flexbox-1/#algo-cross-item>
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
- Divergence: durable page-span records cover ordinary positioned and
  size-contained overflow, but complex nested flex/table/grid continuations
  still do not share one complete fragment-disposition model.
- Divergence: some complex flex/grid continuation paths can still retain a
  preceding fragmentainer's percentage basis when nested destination pages
  have different geometry.
- Divergence: general fragmentainer layout is incomplete.
- Divergence: cloned decorations are incomplete across fragmented layout modes.
- Divergence: multi-pass generated content is incomplete.
- Divergence: rare complex table-cell child/effect fragmentation is incomplete.
- Divergence: robust flex and grid fragmentation are incomplete.
- Divergence: full forced/avoid break precedence and break selection are
  incomplete for cell-internal effect fragmentation, flex/grid fragmentation,
  and other non-block fragment paths.
- Divergence: transformed or nested positioned descendants may still lose a
  required page span when a side-forced break is combined with complex
  fragmentainer ownership. Ordinary absolute and viewport-fixed descendants
  retain their finalized destination-page spans independently of paint.
  <https://www.w3.org/TR/css-position-3/#absolute-positioning>
  <https://www.w3.org/TR/css-break-3/#fragmentation-model>
- Divergence: class A/B/C break handling is not uniformly represented across
  block, inline, table, flex, grid, floats, and generated content.
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
  PDF and convert them to sRGB for PDF/A; eligible opaque uniform samples use
  the same calibrated PDF color space as vector fills. Non-gradient patterns, SVG image
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
  `image-set()` selection, generated CSS color, linear, radial, and conic
  gradients, and concretely sized SVG source viewports before nine-slice
  resolution. Unsupported CSS image functions and exact vector-gradient
  sampling in nine-slice tiles remain incomplete.
- Divergence: border-image `round` mode still has incomplete tile sizing in
  some mixed-axis and subpixel cases.
- Divergence: decoration fragmentation for borders is incomplete.

### Backgrounds, Images, and SVG

- Spec area: CSS Backgrounds and Borders, CSS Images, SVG, compositing.
- Divergence: an embedded SVG root is captured as an isolated, atomic
  compositing group with its CSS background/border before its SVG scene, but
  the PDF backend still serializes coextensive opaque vector edges separately
  inside that group. PDF rasterizers can therefore show one-device-pixel
  antialias seams where a same-color SVG descendant meets the root background;
  this leaves the `svg-scale-*` and `svg-skewy-*` transform reftests
  visually divergent despite correct geometry and root stacking order.
  <https://www.w3.org/TR/SVG2/render.html#EstablishingANewStackingContext>
  <https://www.w3.org/TR/css-backgrounds-3/#layering>
- Divergence: inline and URL SVG supports normalized path geometry, solid
  fills/strokes, `pad` linear/radial gradients (including coincident hard
  stops and stop opacity), path-based `clipPath`, group opacity, isolation,
  blend modes, and normalized solid-vector SVG pattern tiles. Pattern tiles
  preserve their affine placement and can contain supported solid vector paths.
  Supported marker `context-fill` and `context-stroke` preserve the context
  element's paint-server coordinates and object bounding box across affine
  marker placement. SVG strokes in this subset are materialized as SVG-space
  stroked outlines before PDF painting.
  `repeat`/`reflect` spread methods, pattern tiles with nested paint servers,
  transparency/effects, images, or text, SVG images, text, masks, filters,
  nested SVG resource loading, SVG fonts, `<object>`, and `<embed>` remain
  unsupported; affected SVG subtrees are omitted rather than approximated.
- Divergence: non-root inline SVG descendants cascade valid SVG `transform`
  presentation attributes at author origin/specificity zero with CSS
  `transform`, including invalid-CSS fallback, explicit `none`, SVG unitless
  angles, centered `rotate(angle cx cy)`, SVG unitless `transform-origin`
  lengths, and valid `transform-box`. The bridge folds supported static 2D
  transforms and origins into one scene-local affine matrix and strips
  transform declarations from overridden inline `style` attributes. Root
  `<svg>` presentation attributes and transform declarations in embedded SVG
  stylesheets have not yet entered that cascade. Reference-box selection is
  currently complete only for directly specified `<rect>` fill boxes and the
  local view-box origin needed for absolute origins; stroke boxes, exact
  viewport extents, paths and other graphics, transformed container
  descendants, nested SVG viewports, gradients, and patterns remain
  incomplete. Projective/3D, dynamic/script-driven, and unresolved relative
  CSS transforms remain incomplete. CSS paint servers, context-paint cases
  beyond the supported normalized marker subset, percentages outside the
  supported rect fill-box path, full host-CSS overflow semantics beyond root
  `overflow: visible`, and font-relative, viewport-relative, or math CSS
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
- Divergence: embedded SVG image stylesheets evaluate `:root` and matching
  `@media` blocks (including `prefers-color-scheme` from the embedding
  element's used scheme) before entering the SVG backend. The remaining SVG
  stylesheet cascade is limited by that backend's supported selector and
  property subset; unsupported selectors, at-rules, and CSS features are
  skipped rather than fully cascaded.
  <https://www.w3.org/TR/mediaqueries-5/#prefers-color-scheme>
  <https://www.w3.org/TR/SVG2/styling.html>
- Divergence: SVG backgrounds retain omitted, percentage, and `viewBox` root
  intrinsic-dimension metadata for `background-size`, including finite
  painting of opaque uniform regions from extreme-ratio `cover`/`contain`
  images. SVG `<view>` fragments select a viewBox for both sizing and vector
  painting when the root has no viewBox. Replacing an existing root viewBox
  can still rasterize differently from the corresponding CSS reference
  construction.
- Divergence: raster and URL-SVG replaced images support the `fill`,
  `contain`, `cover`, `none`, and `scale-down` concrete-object sizing modes
  and a shared `object-position` model. `<object>` selects its fallback
  subtree when its `data` resource is absent, unavailable, unsupported, or
  not a decodable static image, but Quire does not implement live plugin or
  child-navigable `<object>` representations, loading-state transitions,
  transforms, opacity, compositing, or fragmentation for embedded content.
  <https://html.spec.whatwg.org/multipage/iframe-embed-object.html#the-object-element>
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
- Divergence: unqualified linear, radial, and conic gradients use CSS Images
  Level 3 premultiplied sRGB interpolation. CSS Images Level 4 requires Oklab
  when `in <color-space>` is omitted; Quire retains explicit Level 4
  interpolation methods but does not yet apply that Level 4 default.
  <https://www.w3.org/TR/css-images-3/#coloring-gradient-line>
  <https://drafts.csswg.org/css-images-4/#coloring-gradient-line>
- Divergence: conic gradients support ordinary/repeating color stops, `from`
  angles, `at` positions, and interpolation methods, but do not yet support
  mixed angle/percentage `calc()` expressions or all relative-length positions
  required by CSS Images Level 4.
- Divergence: `image-set()` retains all options through cascade, negotiates
  MIME support and duplicate resolutions in source order, and selects a
  quality-first candidate for the configured device density in backgrounds,
  generated content, border images, and URL-backed list markers. Its
  resolution parser supports units plus `calc()`, `min()`, `max()`, `clamp()`,
  and statically computable `sign()` arithmetic, but the broader CSS Values
  Math function set and currently unsupported `<image>` functions remain
  incomplete.
- Divergence: CSS Values 5 request URL modifiers are retained and CORS and
  integrity checks are enforced for decoded CSS URL images, but
  `referrer-policy` request semantics and modifier enforcement for non-image
  URL consumers remain incomplete.
- Divergence: generated linear/radial gradients share their native shading
  program between backgrounds and `content` replacements, but generated
  gradients are not yet shared across all image-consuming properties,
  including `border-image`, markers, and masks.
- Divergence: `box-shadow` blur rasterization, negative-spread corner
  normalization, and fragmentation are incomplete. Non-blurred rounded,
  corner-shaped, circular, and elliptical outer/inset shadows are supported.
- Divergence: repeated raster background images are not yet raster-equivalent
  to equivalent replaced-image painting at all scaled tile sizes.

### Positioning and Stacking

- Spec area: CSS Positioned Layout, CSS 2.2 Appendix E painting order,
  transforms, CSS Fragmentation, compositing.
- Divergence: CSS Anchor Positioning is not implemented beyond the
  no-default-anchor `anchor-center` fallback to ordinary `center` alignment.
  `anchor-name`, `position-anchor`, anchor geometry and lookup, `anchor()`,
  `anchor-size()`, and `position-area` remain unsupported.
  <https://drafts.csswg.org/css-anchor-position-1/>
- Divergence: CSS 2.2 Appendix E painting order remains incomplete for
  fragmented and nested formatting-context edge cases. Non-fragmented root
  background/border, negative-z, in-flow, auto/zero, and positive paint bands
  are ordered correctly; atomic inline pseudo-contexts also export positioned
  descendants to their nearest real parent without changing stack level.
  Same-page static `z-index: auto` flex items retain their in-flow paint phase
  while auto-level positioned descendants escape to the ancestor stacking
  phase; final PDF realization also retains their resolved in-flow border
  decorations.
- Divergence: a block container with an inline run followed by block children
  can misplace a later negative-margin sibling after CSS block-in-inline
  anonymous-block splitting. The retained inline fragment is laid out one
  block lower instead of overlapping the preceding sibling.
  <https://www.w3.org/TR/CSS22/visuren.html#anonymous-block-level>
  <https://www.w3.org/TR/CSS22/box.html#collapsing-margins>
- Divergence: rare spanning table decoration and border conflicts remain
  unresolved in fragmented painting order.
- Divergence: flex-specific nested descendant links are incomplete in
  fragmented paint/effect contexts.
- Divergence: opacity, transforms, links, and effect groups are incomplete for
  some fragmented stacking-context combinations.
- Divergence: static 2D CSS transforms now carry exact used border boxes from
  ordinary block, atomic inline, positioned, float, and table-wrapper layout
  into transform-origin and percentage resolution; table roots use their
  wrapper border box for both `content-box` and `border-box`. SVG fill/stroke/
  view boxes, nested viewports, and fragmented principal-box reference
  geometry remain incomplete.
- Divergence: CSS Transforms Level 2 projective lowering currently projects
  retained rectangular paint, links, and scene-plane clips. Text, arbitrary
  paths and strokes, images, gradients, patterns, and SVG viewports still
  need their dedicated projective lowerers; SVG 3D integration remains
  deliberately unsupported. Projective scrolling, hit testing, CSS/script-
  driven animation, dynamic DOM mutation, and independent 3D transforms are
  also incomplete.
  <https://drafts.csswg.org/css-transforms-2/#3d-rendering-contexts>
- Divergence: fragmented overflow clips need broader conformance coverage.
- Divergence: `overflow-clip-margin` supports the CSS Overflow 3 shorthand
  for non-negative lengths and visual boxes, including rounded and
  corner-shaped contour expansion. The Level 4 logical/physical longhands and
  independent per-side offsets are incomplete. Axis-selective normal-flow
  deferred paint preserves an unbounded companion axis through transforms,
  but fragmented, SVG, and specialized replaced-element paths still need
  broader conformance coverage.
  <https://www.w3.org/TR/css-overflow-3/#overflow-clip-margin>
- Divergence: fragmented floats do not fully let descendant stacking contexts
  escape into the ancestor context while preserving fragment-local coordinates.
- Divergence: transformed ancestors capturing fixed containing blocks across
  fragments are incomplete.
- Divergence: transformed containing-block behavior is incomplete in less
  common formatting-context and fragmentation cases.
- Divergence: exact static-position placeholders are incomplete for multiline
  inline, table, fragmented, and other remaining formatting-context cases.
  Direct single-line inline abspos sources and collected inline descendants
  use the same prepared hypothetical placeholder. Non-atomic inline sources
  reset the subject to `position: static`, `float: none`, and `clear: none`;
  preceding physical floats, clearance, text alignment, indentation, and
  direction remain part of the surrounding line context. Atomic inline source
  display restoration is still incomplete.
- Divergence: positioned inline containing blocks use collected first/last
  edge markers, including zero-height phantom-line fragments, for ordinary
  single-line horizontal block-level absolute descendants. Multi-line, bidi,
  vertical-writing-mode, transformed, table, and fragmented inline contexts
  do not yet retain one final fragmentainer-aware rectangle.
- Divergence: ordinary block and inline abspos static-position alignment is
  covered for same-page, single-line source geometry, including degenerate
  block/inline rectangles, inherited `justify-items`/`align-items` defaults,
  direction, and vertical writing modes. Multiline, bidi, transformed,
  orthogonal, and fragmented source geometry remain incomplete, as does
  physical-vertical automatic sizing that still uses block-flow
  minimum-content measurement rather than the required fit-content extent.
  <https://drafts.csswg.org/css-align-3/#abspos-sizing>
- Divergence: positioned fragmentation remains incomplete for transformed,
  complex nested formatting-context, and fragmentainer-ownership cases.
  Ordinary transparent absolute paint retains its resolved destination page,
  and viewport-fixed layers replay across retained positive absolute spans
  independently of source order.
- Divergence: `isolation`, blend modes, filters, masks (including
  `mask-border-source`), and `clip-path`, containment-triggered paint
  isolation, `content-visibility`, and `will-change` lack full visual
  semantics. Legacy positioned `clip: rect()` is modeled as a typed border-box
  clip; `mask-border-source` is independently cascaded; both, together with
  the paint containment implied by `content-visibility: auto` and `hidden`,
  force the `transform-style: preserve-3d` used value to `flat` as CSS
  Transforms 2 requires, even though mask-border painting itself remains
  unsupported.
  <https://drafts.csswg.org/css-transforms-2/#grouping-property-values>
- Divergence: Filter Effects Level 1 is implemented only for a deliberately
  narrow exact lowering: `grayscale()` (with its required `[0, 1]` clamp),
  `saturate()` and `brightness()` when their resolved amount is in `[0, 1]`,
  and `opacity()`, for a subtree that the PDF backend can prove contains only
  normal source-over direct vector paints. The lowering uses an isolated sRGB
  PDF transparency group and applies filter opacity before CSS `opacity`.
  It deliberately declines to paint a partial result when any descendant is
  outside that audited subset. Contrast, invert, sepia, hue rotation,
  over-range saturation or brightness, spatial filters, SVG filter graphs,
  URL filters, backdrop filters, non-normal descendant blend modes, raster
  images, image patterns, and SVG paint servers still require a raster
  filter backend.
  <https://www.w3.org/TR/filter-effects-1/#FilterProperty>
- Divergence: masks and `clip-path` geometry beyond comma-delimited
  nonzero-fill `polygon()` values in the default border box are not
  shape-aware. In particular, other basic shapes, `evenodd`, geometry-box
  selection, and URL references are not rendered.
- Divergence: `will-change` is only a pre-isolation trigger.

### Units and Value Computation

- Spec area: CSS Values and Units, CSS Cascade, CSS Sizing.
- Divergence: `ex` and `cap` resolve from the selected font for box-model
  `<length-percentage>` values (including CSS math), and `ic` resolves from
  the WATER glyph advance for direct box-model values. Supported `font-size`
  paths also resolve `rex`, `rcap`, and `ric` from the document root's selected
  font. `lh` resolves against the inherited line height in `font-size` and
  `line-height`. These metric units are not yet supported property-wide or
  throughout deferred math; `rlh` remains incomplete.
- Divergence: `rem` uses the document root's resolved font size for CSS sizing
  properties, but this root-relative basis is not yet applied property-wide to
  every computed `<length-percentage>` value.
- Divergence: typed CSS `attr()` resolves basic `raw-string`, `type(*)`,
  `<color>`, and `<length>` substitutions (including prefixed XML attributes),
  but its remaining type grammars, unit keywords, tainting, and animation
  behavior are not implemented.
- Divergence: CSS Animations cascades `animation`, `animation-name`,
  `animation-duration`, and `animation-delay` for its one static snapshot,
  including CSS-wide keywords, rollback, variables, and explicit inheritance.
  Multiple
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
  popover, modal, fullscreen, and picture-in-picture states. Consequently the
  ten script-driven `top-layer-box-uses-icb-*` static-position WPTs remain
  out of scope until JavaScript and HTML top-layer state are modeled.
- Divergence: XML namespace selector edge cases need broader conformance
  auditing.
- Divergence: shadow/tree-scoped selectors are not implemented.
- Divergence: unmodeled UI/highlight pseudo-elements are not implemented.
- Divergence: HTML form-control rendering semantics are incomplete. In
  particular, `button { display: grid }` with `button::first-letter` does not
  yet model the control-specific anonymous display and pseudo-element behavior
  required by CSS Display, CSS Pseudo-Elements, and HTML form controls.

### CSS Color Adjustment

- Spec area: CSS Color Adjustment Level 1 forced colors.
- Divergence: script-driven forced-colors and backplate reftests are not
  supported because Quire has no JavaScript runtime or platform text backplate
  compositor.
- Divergence: `color-scheme` selects Quire's light/dark used scheme and drives
  `light-dark()`, but system-color palettes, form-control appearance,
  `print-color-adjust`, and the remaining CSS Color Adjustment used-value
  behavior are incomplete.

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

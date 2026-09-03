# CSS Writing Modes Parity

This note tracks writing-mode behavior that affects layout geometry rather
than text shaping alone.

## Current Coverage

- `writing-mode: horizontal-tb`, `vertical-rl`, and `vertical-lr` are parsed,
  cascaded, and used by block, inline, table, flex, floats, and painting paths.
- Logical side and axis projection is centralized in the computed-flow mapping
  used by CSS Logical Properties and layout adapters. This covers all five
  supported writing modes and both `direction` values, including the distinct
  `sideways-lr` inline progression.
- A box whose `writing-mode` differs from its nearest box-generating ancestor
  computes `flow` to `flow-root`. The comparison skips `display: contents`
  while ordinary inheritance still passes through it, covering parallel
  sideways/vertical as well as orthogonal writing-mode changes.
- Float sizing stays in the floated box's logical axes, while `float`,
  `clear`, exclusion bands, and placement resolve through the containing
  block's `FloatPlacementAxes`. Legacy `left`/`right` use line-relative sides;
  vertical block-track search starts at physical right for `vertical-rl` and
  physical left for `vertical-lr`. Vertical line boxes use the same typed
  block-start projection when selecting their float-exclusion slab, so a
  `vertical-rl` line no longer samples the containing box's leftmost column.
- Bottom-origin vertical and sideways floats use a typed replay projection
  from a stable page-top scratch border box to the selected physical border
  box. The projection is applied once to the complete ordinary float subtree,
  separately captured positioned descendants, and each fragmented page-local
  subtree; horizontal and top-origin flows retain direct destination replay.
  The replayed principal border box also supplies the final margin-box and
  exclusion extent. The exact WPT evaluation for
  `css/css-logical/logical-values-float-clear-4.html` is raster-exact.
- Bottom-origin anonymous-block scratch projection is confined to the
  document principal flow. Ordinary fixed-size nested vertical containers
  retain their local inline origin, so their descendants advance in physical
  X without translating local overflow into page-Y consumption.
- Inline `text-align: left` and `right` now resolve through the writing
  mode's line-left and line-right sides before physical placement, including
  vertical and sideways line directions.
- In vertical typographic modes, `text-orientation: upright` now forces the
  used inline direction to LTR without changing its cascaded `direction`.
  Normal block, float, grid, flex, table, and inline-replay containing-block
  scopes consume that used value while computed `direction` remains available
  for inheritance. Upright text also enters bidi ordering as strong LTR rather
  than merely as an LTR embedding.
- `::marker` text inherits and accepts the writing-mode text controls
  `direction`, `unicode-bidi`, `text-orientation`, and
  `text-combine-upright`, including upright marker punctuation in vertical
  lists.
- Mixed vertical text preserves OpenType vertical glyph forms through PDF
  emission. Transformed-rotated (`Tr`) typographic units select those forms
  and are typeset upright, matching CSS Writing Modes' mixed-orientation
  policy; only `R` units are sideways.
- Vertical paint placement groups glyphs by their preserved shaping source
  ranges, not their PDF-facing Unicode summaries. A glyph with an empty
  ToUnicode summary therefore remains in its resolved typographic unit and
  retains its advance and vertical origin. A shaping range that spans several
  units uses vertical metrics and placement when every covered unit resolves
  to the same mode; mixed upright/sideways ranges remain conservative.
- Mongolian and Phags-pa are resolved as complete sideways-horizontal units
  where their intrinsic vertical presentation requires that composition.
  Their feature selection, font metrics, and PDF matrix now share one
  per-unit plan, preventing vertical substitutions or vertical advances from
  leaking into sideways runs.
- Inline fragments with transparent source glyphs remain paintable when an
  explicit visible `text-shadow` needs their glyph outline. This keeps the
  shadow paint path independent of the source fill's alpha in horizontal,
  vertical, and sideways text.
- Upright text without OpenType vertical metrics synthesizes its vertical
  advance and origin from the face's ascender/descent and centered glyph
  bounds, rather than assuming a fixed em origin. This aligns upright glyph
  and physical `text-shadow` placement, including
  `text-shadow-orientation-upright-001.html`.
- Block layout now carries a logical inline content size separately from the
  physical content width, so orthogonal vertical-flow roots do not wrap text
  against the containing block's horizontal span.
- Parallel vertical normal-flow blocks retain their automatic logical inline
  size as their physical height after child layout. This prevents a block with
  a physical `width` but automatic `height` from collapsing to its inline
  content height, including the principal geometry exercised by
  `scrollbar-vertical-rl.html`.
- A definite vertical physical `height` remains the block's used logical
  inline size after selected inline lines are painted. The selected line
  sequence reports intrinsic occupancy for generated-box alignment and
  auto-sizing only; it cannot replace a specified height, including after a
  selected soft-hyphen break.
- Normal block intrinsic `width` sizing now keeps logical inline and block
  content-size contributions typed separately before projecting to physical
  width, so vertical writing-mode `width:min-content` and `width:max-content`
  use block-axis line-column size.
- Parallel vertical and sideways normal-flow blocks now resolve physical
  `width:auto` as their content-derived logical block-size, not as a
  fill-available horizontal width. The propagated `html`/principal-`body`
  canvas remains the intentional initial-containing-block exception.
- Orthogonal auto inline sizing uses the CSS Writing Modes fit-content rule
  against the containing block's available perpendicular size. For auto-height
  containing blocks, fixed `max-height` is floored by fixed `min-height` before
  falling back to the initial containing block, covering WPT
  `available-size-018.html`.
- Normal-flow orthogonal flex containers with `width:auto` use flex intrinsic
  sizing for their physical block axis, so vertical row flex containers can
  shrink-wrap their item cross-size while honoring a definite physical height.
- Flex cross-size remeasurement keeps the line-owned size of an automatic
  `align-self: stretch` item. This preserves a horizontal row flex line's
  assigned physical height when its items use orthogonal writing modes,
  covering `flexbox_align-items-stretch-writing-modes.html`.
- Block-level absolute-position replay retains the originating vertical block
  container's physical content span through anonymous inline wrappers. Fixed
  and automatic physical block axes retain their distinct static-position
  behavior.
- Auto-sized vertical absolute-positioned boxes now measure their physical
  height as a fit-content logical inline axis against the positioned
  containing block, rather than as a normal-flow stretching height. The
  measurement scope retains the containing block's physical height for that
  axis while horizontal auto-height measurement remains fragmentable. This
  covers `available-size-006.html` through `available-size-011.html` and
  `available-size-015.html` through `available-size-016.html`. Direct
  non-scrolling maximum constraints likewise contribute the physical width of
  the resulting wrapped vertical columns, rather than max-content width.
- A replaced source nested in a horizontal child of a vertical flow
  inherits that child's final hypothetical physical block position, rather
  than the temporary left-origin measurement span. The `vertical-rl` source
  needs that projection; a `vertical-lr` source instead retains the inline
  placeholder's own static rectangle. This covers
  `abs-pos-replaced-vrl-001.html`, `abs-pos-vlr-border-001.html`, and
  `abs-pos-vlr-padding-001.html` without changing block-source static
  positioning.
- Child available space keeps a percentage basis separate from its orthogonal
  layout fallback and projects both through the child writing mode. An
  indefinite containing physical height can therefore provide a usable
  orthogonal available size without incorrectly resolving vertical inline
  percentage margins. The tagged fallback either retains the initial
  containing block or records the nearest scroll container's capped used
  constraint; an unconstrained nearer scroll container correctly terminates
  lookup. A non-scrolling constraint remains direct-child-only rather than
  leaking through an intervening formatting context. Normal blocks, flex/grid
  item replay, table-cell, inline-block, and positioned scopes all use that
  policy.
- Orthogonal fit-content negotiation treats the selected available measure as
  an outer inline size, subtracting the vertical child’s inline margins,
  padding, and borders before line fitting. A vertical block's own physical
  min/max-height constraint also bounds that measure before line layout, so
  its content reflows rather than being laid out against the ICB and clipped.
  Principal vertical block flow also keeps physical inline-axis margins out of
  block-axis margin collapsing,
  including the document canvas's body start margin. This aligns the nested
  unfragmented normal-flow cases exercised by `available-size-020.html`
  without making the fallback a percentage basis.
- Intrinsic inline-size callers now avoid computing an unrelated logical
  block contribution. Likewise, a definite physical width on an orthogonal
  block child is used directly instead of recursively measuring its contents.
  Simple vertical inline streams retain their selected final sequence, so the
  final block geometry consumes the same line measurement rather than shaping
  it again. A release WPT run on 2026-08-12 measured renderer time of 1.648 s,
  1.421 s, 1.406 s, and 1.545 s for `available-size-020.html` through
  `available-size-023.html`, respectively. These are within the WPT's
  five-second limit, but remain above the one-second performance target.
- A resolved physical `height` is handed to vertical auto-width measurement as
  its logical inline-size before wrapped columns are counted. This prevents a
  height-constrained vertical block from measuring against the page-height
  fallback and then overflowing its too-narrow physical block-size, covering
  `text-combine-upright-line-breaking-rules-001.html`.
- Normal-flow blocks supply the page area's physical height as the initial
  orthogonal fallback. Vertical and sideways blocks therefore no longer use
  their logical block size, which is the page area's physical width, for a
  child physical-height constraint.
- Orthogonal float measurement and final float replay retain the parent
  writing-mode context until the floated root enters block layout. A vertical
  auto-height float therefore resolves its logical inline contribution into a
  physical height instead of being measured as zero-height.
- Atomic inline-block layout retains the physical extent and last compatible
  baseline of an in-flow horizontal line when a later orthogonal block advances
  only its own axis. Orthogonal descendants do not replace that exported
  baseline, covering `baseline-with-orthogonal-flow-001.html` for both visible
  and clipped descendants.
- Vertical flow roots with in-flow block children use block intrinsic
  contributions for their logical inline fit-content measure, projecting each
  child through its physical height. This preserves auto sizes through nested
  alternating writing modes, covering `nested-orthogonal-001.html`.
- Normal-flow child entry resolves all percentage margin and padding edges
  against the parent’s typed logical-inline basis before margin collapsing and
  placement consume the used physical edges. This covers the complete
  `sizing-orthogonal-percentage-margin-001.html` through `-008.html` series.
- Grid intrinsic probes preserve both logical track axes. A vertical grid's
  physical width is derived from its row-track contribution, including the
  grid item's logical block-axis border, padding, and margins. Replaced Grid
  items project their physical intrinsic size through their writing mode, and
  vertical replaced inline atoms use the central baseline, covering
  `img-intrinsic-size-contribution-001.html` and `-002.html`.
- Table cells now derive their row and column rectangles from logical table
  tracks and project those rectangles through the table root's flow axes. This
  keeps table slot progression distinct from the writing mode used to lay out
  a cell's own contents. Final vertical cell content uses that projected
  content box as its sole block-start origin, and table-cell alignment subjects
  resolve on the cell's logical block axis rather than its inline progression
  axis. The table wrapper enters row paint at the projected grid-content edge,
  so root border chrome is not applied again to floated vertical or sideways
  tables. Axis-aligned hard-stop cell backgrounds also retain vector edges,
  keeping them coincident with Ahem alignment masks. Verified coverage is the
  26-reftest `table-*` subset in `css/css-writing-modes`, including
  `table-cell-align-*`, `table-cell-valign-*`, and the horizontal, vertical,
  and sideways `table-progression-*` cases. This is scoped WPT coverage, not
  a claim of complete vertical-table behavior.
- Orthogonal table-cell row measurement preserves a definite logical
  `inline-size` as a horizontal table-row constraint, while a physical
  `height` derived from `ch` is resolved through the table's column track.
  The cascade retains that selected-font-metric provenance until table
  layout, covering the `ch-units-vrl` table-column and column-group cases.
  Sideways glyph runs inside a table cell also project their horizontal
  baseline into that cell's vertical line box, without changing ordinary
  vertical text outside table layout.
- Inline-table atoms export their first-row baseline as a logical block-axis
  offset where an eligible row baseline exists, and otherwise synthesize the
  table border-box block-end fallback. The enclosing line projects that
  baseline through the table box rather than reapplying the wrapper's
  block-start margin; `vertical-rl` no longer replaces the exported baseline
  with a block-end shortcut. Isolated vertical inline-tables also freeze their
  logical inline track through physical `height` and expose their logical block
  track as the parent-facing physical inline size. Speculative inline-table
  track probes now restore their page state before the retained atom fragment
  is built, so probe paint cannot escape into the parent line. Inline-table
  replay retains the table-cell writing-mode and direction context through
  final text paint, including reversed sideways RTL content.
- Vertical table roots now resolve their column-grid preferred size from the
  physical `height` property, and parent/float intrinsic sizing projects the
  table's logical block tracks to physical width. This prevents a vertical
  table's column extent from being replayed as a horizontal float width.
- Table-wrapper grid sizing now carries a `LogicalInlineContentSize` with its
  `TableAxes`. Its preferred, minimum, and maximum logical-inline constraints
  resolve through `height`/`min-height`/`max-height` and top/bottom decoration
  for vertical roots (rather than through physical-width constraints and
  left/right edges). The corresponding horizontal path continues to use width
  constraints and left/right decoration.
- Table-body layout now exports one typed physical parent-flow endpoint before
  wrapper trailing chrome is consumed. This prevents a completed vertical grid
  from applying its logical-to-physical projection twice before the following
  horizontal-flow sibling is placed; table-cell baseline sizing remains wholly
  within the logical table-row pass.
- Empty vertical float-avoidance scopes preserve the hypothetical physical
  top for both inline directions. In particular, a bottom-to-top nested BFC
  no longer jumps by the full available page-inline span before table layout.
- Principal-flow block advancement now consumes the painted fragment's actual
  physical block span without a right-to-left offset, so a later vertical-rl
  sibling starts after the full preceding block and its logical margin. The
  bottom-origin `sideways-lr` scratch/replay path is raster-exact for
  `wm-propagation-body-043.html` and `wm-propagation-body-051.html`. Inline root
  generated content uses the propagated principal axes, while a block-level
  root pseudo remains an independent orthogonal formatting context with its
  computed writing mode and its own computed block-start edge; this covers
  `wm-propagation-body-044.html`.
- Fixed normal-flow blocks in vertical paged roots retain their principal
  logical block fragments separately from visible descendant overflow. The
  overflow replays through later page areas on the root's horizontal block
  axis, with `vertical-rl` progressing right-to-left and `vertical-lr`
  left-to-right, without enlarging the fixed box or advancing later siblings.
- Fragment paint projection now records independent source and destination
  writing-mode axes. This keeps a source flow's logical slice selection
  separate from the destination fragmentainer's physical clip and translation.
  Positioned-tail clipping likewise selects the fragmentainer's logical block
  axis while retaining CSS overflow rectangles as physical geometry.
- Absolute-positioned page-span selection now resolves physical inset
  properties into a continuous margin box first, then measures that box along
  the principal fragmentainer's block-start side. This keeps `vertical-rl`
  right-to-left and `vertical-lr` left-to-right page selection independent of
  physical `top`/`bottom` positioning.
- Definite absolute blocks retain that continuous source extent while consuming
  every destination page's actual page-area capacity. A later differently
  sized page therefore changes only local fragmentainer capacity, not the
  absolute containing block or percentage basis.
- A propagated `sideways-lr` body now lays direct canvas children in a
  top-origin scratch coordinate system and projects the resulting paint to its
  bottom-origin principal inline edge. This makes
  `wm-propagation-body-051.html` raster-exact without changing computed root
  values.
- A vertical propagated root keeps the initial containing block's physical
  canvas width for `width:auto`; it is not shrink-wrapped from the body's
  content while establishing the principal writing mode. This covers the
  non-pseudo body propagation matrix entries.
- Nested orthogonal block roots preserve the initial available physical width
  as the percentage basis through the document-canvas and vertical auto-size
  handoff. Their intrinsic contribution measures an auto horizontal child at
  its shrink-to-fit line measure, preventing extra wrapped height.
- Intrinsic physical-width estimation for an auto-sized vertical block now
  wraps direct vertical child text at the parent's definite physical-height
  (logical inline) measure before accumulating its column contribution.
- Sideways glyph runs now align to every vertical line box's logical block
  span, rather than only table-cell projections. This keeps rotated Latin
  glyph ink inside correctly positioned vertical inline-block outlines.
- Upright vertical glyphs retain their OpenType vertical-origin offset on the
  glyph record while their run origin carries only normal-flow inline advance.
  PDF emission therefore applies that origin exactly once, keeping
  `text-align:center` labels centered in ordinary and row-spanning table
  cells.
- A definite-height horizontal child of a vertical flow is now statically
  anchored at the parent's logical block-start edge; auto-height percentage
  children retain ordinary static positioning. This preserves nested
  orthogonal block progression without moving percentage-based roots.

## Remaining Divergences

- Fragmented orthogonal available-size negotiation still needs a full audit
  across nested scroll containers and page/column continuations.
- Fragmented orthogonal flows still inherit Spindrift's cursor-oriented pagination
  limits; available block-size negotiation is not yet represented as durable
  fragment objects.
- Orthogonal available-size coverage for grid, table-cell, absolutely
  positioned, and deeply nested mixed-writing-mode descendants needs broader
  WPT coverage beyond the shared scope handoff.
- Propagated root pseudo-element inline layout still needs an audit of its
  available inline extent in vertical and sideways roots. In
  `wm-propagation-body-047.html`, the propagated body canvas is correctly
  bottom-originated, but `html::after` is still laid out as narrow
  one-character columns.
- Complex table-wrapper fragmentation, collapsed borders, alignment, and
  repeated-row replay still need broader mixed-writing-mode coverage. Normal
  table grid rectangles, structural paint clips, and row/row-group containing
  blocks project through the table root exactly once; the horizontal, vertical,
  and sideways `table-progression-*` WPT matrix passes.
- `text-combine-upright` now parses, cascades, and inherits `none`, `all`, and
  `digits 2..4`. A pure source-range analysis pass and a separate
  materialization pass form horizontal child sequences inside one-em atomic
  squares; intrinsic and final layout use the same formation path. Each atom
  retains its transformed boundary text, isolated nested bidi sequence, and
  square geometry separately. Text-combine and ruby bases expose one shared
  textual-boundary interface to UAX #14, while the atom remains indivisible to
  the parent graph. Used tracking is zero at the composition's outgoing edge
  without losing source-scope ownership. One resolver supplies both the
  parent-central line extents and paint placement, and the captured paint
  subtree is centered, compressed only in its horizontal axis, and replayed
  without an ink clip. The sequence applies `hwid`, `twid`, and `qwid`
  alternatives for two through four characters and reverses the full-width
  ASCII transform before multi-character compression.
- A complete release run on 2026-09-03 passes 33 of the 35 executed
  `text-combine-upright*` reftests. The remaining failures are the two
  single-character value tests, whose geometrically equivalent
  horizontal/vertical glyph paths differ at rasterized edges.
  Dynamic DOM tests and the currently skipped ruby and decoration tests remain
  outside this coverage.

# CSS Tables Parity

Last updated: 2026-07-27

CSS 2.2 table layout, CSS Tables Level 3, CSS Sizing Level 3, CSS
Fragmentation Level 3, and HTML table semantics are the conformance targets.
WeasyPrint tests remain useful comparison material for paged-output behavior,
but spec conformance takes priority when behavior differs.

## Current Level

- The latest full `css/css-tables` WPT report records **121 / 123**
  non-tentative tests passing. Exact re-evaluation passes the repaired
  content-box min/max wrapper, collapsed-row rowspan clipping, anonymous-cell
  whitespace, vertical cell baselines, positioned-cell static-position,
  vertical-rl RTL column backgrounds, and collapsed-border `width: 0` track
  allocation. The only remaining non-tentative failures are
  `row-group-margin-border-padding.html` and
  `row-margin-border-padding.html`, whose residual raster differences are
  isolated to table-internal box-model no-op handling.

- Table formatting is implemented under `src/layout/table/` with durable row,
  row-group, column, caption, span, and collapsed-border structures.
- HTML table span attributes use one shared parser for durable fragments and
  layout grids: leading ASCII digits are parsed, trailing junk is ignored,
  invalid values default to 1, `rowspan=0` spans to the row-group end, and
  column/row spans are clamped to the HTML table-model bounds.
- Enabled HTML presentational hints map table dimensions, border color,
  cell spacing, cell padding, and legacy structural `background` URLs through
  the ordinary author-origin cascade. Author CSS therefore overrides the hints,
  and resource URLs retain the document URL context.
- Authored empty table rows are preserved in durable table fragments and can
  contribute explicit row height to the grid.
- Orphan table-internal boxes generate anonymous table wrappers, including
  inline-table wrappers when the missing parent is generated inside an inline
  formatting context.
- Anonymous cell construction preserves whitespace between consecutive
  non-table-cell children, while still ignoring formatting whitespace adjacent
  to table-internal boxes.
- Generated anonymous table cells apply normal block-container normalization,
  so an inline descendant split by an in-flow block retains separate preceding
  and following block-flow content.
- Nested row groups inside a row group participate in the enclosing anonymous
  row fixup, and their misparented inner row groups generate nested anonymous
  tables so sibling cells remain in the same generated row.
- The table height path uses a `TableHeightPlan` with base row heights,
  reference heights, final distributed heights, collapsed-row state, and
  auto-row eligibility.
- Definite table heights distribute through row groups and rows. Percentage
  table root heights resolve against definite containing-block content height
  before empty-grid painting and row distribution, percentage row/row-group/cell
  heights resolve against definite table block size, and table-cell content
  relayout resolves percentage-height descendants when CSS Tables 3 treats the
  cell or table root as explicitly height-sized. The height plan carries that
  resolved definiteness into final cell relayout. A cyclic percentage-height
  scroll container with an independent minimum block size instead contributes
  that minimum during row sizing, then resolves against the finalized cell.
- First-pass table-cell row minimum sizing treats descendants whose block size
  depends on the parent cell height as auto. A percentage-height scroll
  container with an independent non-percentage `min-height` contributes that
  constraint rather than its overflowing descendants, then receives its final
  percentage height during the second pass.
  Intrinsic table-cell sizing now follows normal-flow descendants through those
  unresolved percentage-height wrappers for both row height and column width.
- The final table-cell relayout carries the committed, typed cell block-size
  percentage basis through intrinsic measurement, inline sequence planning,
  baseline alignment, and painting. Replaced canvas descendants therefore
  resolve percentage `height`/`min-height`/`max-height` constraints in the
  final pass, including block-level canvases preserved through block-in-inline
  normalization, without making those percentages definite during row-minimum
  sizing. When a cell has a definite height, automatic-width replaced
  descendants also contribute their percentage-resolved aspect-ratio width to
  intrinsic column sizing, including through inline wrappers.
- Table-cell row minimum sizing accounts for CSS 2.2 margin collapsing among
  normal-flow block descendants, including self-collapsing blocks inside
  anonymous table cells. Whitespace-only inline nodes between block children
  remain phantom line boxes and do not terminate those adjoining margins.
- Direct text children participate in that same table-cell block-flow height
  sequence in both the row-minimum and final percentage-relayout passes. Text
  following an oversized or size-contained block therefore extends the row and
  remains in the final table fragment instead of being painted beneath the
  following normal-flow box.
- Mixed inline and block descendants contribute in source order: an inline
  line preceding a block child expands the row before that child and any
  following row border are placed. An authored row `height` remains a minimum
  rather than clipping either in-flow segment.
- Explicit table-cell `height` and `min-height` participate as minimum row
  sizing inputs, while table-cell `max-height` does not clamp the final
  border-box below required in-flow content during row height distribution.
- Table wrapper `min-height` participates in grid height distribution after
  separated wrapper padding and borders are subtracted; `max-height` caps
  definite table height targets but does not shrink intrinsic auto-height rows
  by itself. A flex/grid-assigned wrapper border-box size now takes the same
  constraints before it becomes a row-distribution target.
- Rowspan constraints grow eligible visible rows and preserve collapsed-row
  clipping and fragmentation behavior.
- Table-cell row sizing now measures nested table fragments through the table
  measurement path, so nested table captions, spacing, row heights, and wrapper
  box sizing contribute to the containing row.
- Floated inline children inside table cells, including children wrapped by
  anonymous table-cell construction, contribute to auto row height and paint
  through the normal float placement path.
- Durable table fragments preserve authored row and cell computed styles and
  participate in font-metric length resolution before table layout. Row and
  column grid sizing resolves metric-dependent lengths in the owning
  row/column axis context, while cell content still uses the cell's own
  writing-mode and text-orientation. Separated-table `border-spacing` also
  keeps metric-dependent lengths until selected-font resolution before the
  table grid consumes physical spacing values. Table row, row-group, column,
  column-group, cell, and caption helper style reconstruction resolves
  descendant `font-size: <ch>` against the measured parent zero advance.
- Table root and reconstructed table-part styles cross an explicit used-value
  boundary for CSS `zoom`: fixed table dimensions, borders, padding, captions,
  cell content, separated `border-spacing`, and collapsed-border conflict
  geometry scale exactly once, while percentage components remain relative to
  their zoomed table or cell bases. Durable styles remain unscaled cascade
  parents for deferred and anonymous table-part reconstruction.
- Deferred font-metric resolution follows the table tree's inheritance order
  (row group, row, then cell), so anonymous wrappers retain inherited font
  sizes rather than resolving `inherit` against the table root.
- Baseline behavior covers direct text, multiline content, block children,
  nested table rows, non-text fallback, inline-table baselines from the first
  visual row, and vertical-writing table cells no longer inflate physical row
  height with horizontal-axis baselines. Row sizing and painting consume the
  same non-text content-bottom fallback, including below-baseline space from
  peer text cells.
- Committed table-row painting retains the table fragment's logical grid
  placement through the cell boundary. This prevents vertical tables from
  applying their block-direction projection twice, and keeps vertical root
  baseline/intrinsic-size layout on the table's logical axes.
- Column and column-group generated backgrounds retain a logical table-grid
  positioning area until the final root-writing-mode transform. In particular,
  hard-stop linear gradients rotate with their vertical/RTL grid clips instead
  of resolving against a reconstructed horizontal rectangle.
- Positioned table wrappers establish the containing block used by absolute
  descendants inside captions as well as the row grid.
- Auto-width captions use the table wrapper border box as their containing
  width, including separated borders and resolved collapsed outer insets,
  rather than the narrower grid content box.
- Inline tables contribute their full margin box to line layout, so vertical
  alignment and negative block-axis margins position nested table fragments
  without changing their internal collapsed-border paint order.
- Inline-table atom construction uses an unfragmented isolated canvas, so an
  oversized atomic table remains intact and overflows its containing line/page
  instead of producing discarded scratch page continuations.
- Positioned table rows establish their final row-piece containing block for
  absolute descendants inside cells, including descendants preserved through
  anonymous table-cell block construction. Static positions for those
  descendants use the unaligned cell-content origin, before baseline or other
  vertical alignment shifts in-flow cell content.
- Empty tables keep wrapper padding, borders, captions, definite grid size, and
  border-box sizing in separated-border layout and painting; collapsed empty
  tables ignore separated wrapper padding/borders and use the empty collapsed
  grid's zero outer insets.
- Table box `overflow:hidden` and `overflow:clip` clip to the table box rather
  than the table wrapper that contains captions, while table box
  `overflow:auto` and `overflow:scroll` behave as visible per the CSS 2.1
  errata. An HTML root table still propagates its overflow to the viewport, and
  the non-rendered HTML `head` cannot become a caption through author CSS.
- Table-cell `overflow:auto` and `overflow:scroll` establish a used
  padding-edge scrollport after row sizing, so overflowing cell content clips
  without allowing a specified cell height to override the row's intrinsic
  minimum. An explicit cell height also supplies the definite percentage basis
  while measuring its scroll-container descendants. Table-cell `overflow:hidden`
  remains visible when row layout grows the used cell height to fit in-flow
  content.
  The auto/scroll clip is retained as a PDF paint scope as well as an immediate
  layout culling boundary, so partially intersecting glyphs and positioned
  descendants do not escape it.
- Collapsed-border conflict resolution covers table, column-group, column,
  row-group, row, and cell origins, including column and column-group
  block-edge candidates and floored CSS-pixel width priority for subpixel
  borders; rowspans and colspans suppress internal grid edges so structural
  borders do not paint through spanning cells.
- Collapsed tables ignore table-root padding and `border-spacing`, derive
  wrapper insets from the widest resolved outer grid-edge half-width across
  all displayed row segments, derive cell content insets from resolved
  grid-edge border half-widths, subtract those resolved outer insets from
  border-box table height targets, and paint collapsed `inset`/`outset` as
  `ridge`/`groove` with side-aware two-tone 3D borders. Collapsed borders
  paint after table/cell and in-flow block background/border paint but before
  floating, inline, and positioned foreground phases, so collapsed borders can
  cover block child backgrounds while overflowing inline-block cell content can
  cover border paint as required by CSS 2.2 Appendix E. Collapsed-border
  emission honors table-level `visibility`, so a hidden table retains layout
  geometry without painting its resolved border grid.
- Every in-flow table body fragment preserves its local paint-band sequence at
  its source-order position. A collapsed border therefore remains below a
  following block sibling, while a nested block table remains below its
  ancestor table's collapsed borders; inline-table descendants continue to
  participate in the enclosing inline paint phase.
- Floated collapsed table wrappers, including floats collected through inline
  layout around clearing breaks, use durable table fragments for shrink-to-fit
  grid sizing and add resolved collapsed outer insets only to the float
  exclusion/visual margin box.
- Collapsed table-cell decorations use resolved collapsed-border half-insets
  for background and rectangular inset `box-shadow` geometry. Initial
  `background-clip:border-box` is resolved to the padding edge because the
  collapsed grid owns the intervening half-rule area; retained collapsed-grid
  rules are prepended before those decorations, while the grid remains the
  only border painter.
- Normal-flow atomic inline content in table cells, including inline-block
  spans, is routed through reusable inline line sequences before collapsed
  border paint, so `line-height:0` cells keep inline-block backgrounds inside
  the resolved border insets. Anonymous table-cell inline sequences resolve
  percentage inline-block widths against the final cell content width, and
  preserved whitespace between consecutive non-cell children participates in
  line wrapping.
- Table `direction: rtl` maps logical columns and collapsed-border candidates
  onto physical left-to-right grid positions for placement, backgrounds,
  conflict tie-breaking, and fragmented border painting, while separated
  column backgrounds stay inset from the outer `border-spacing`.
- In horizontal writing modes, table row, column, and column-group structural
  backgrounds use their complete structural box as the positioning area and
  each participating grid cell as its paint area. This preserves `colspan`,
  `rowspan`, collapsed borders, and separated `border-spacing` while routing
  image layers through the shared CSS background renderer. Synthetic columns
  created for a `colgroup` do not replay that group's background layer.
  Disjoint synthetic-group and explicit-column fills share the committed
  physical paint order, while overlapping column layers retain their CSS
  stacking order. This covers the writing-mode table progression matrix.
  Row-group and fragmented-span geometry remains incomplete.
- Collapsed columns are removed from collapsed-border column and column-group
  candidate painting when their entire candidate span is suppressed.
- Separated-border table fragments paint row-group backgrounds and outlines
  from the visible row fragment geometry, including after collapsed rows reduce
  their occupied track height to zero.
- Separated-table vertical `border-spacing` distinguishes participating
  zero-height rows from collapsed or hidden-empty rows, so rowspans crossing
  collapsed rows do not retain internal spacing from the suppressed tracks.
- Separated table wrapper borders paint after the table-root structural
  background in each body fragment, so an authored table background does not
  obscure the wrapper border.
- Separated-border table rows ignore row border, padding, and margin for row
  box painting and grid placement, while still preserving row styles as
  inheritance parents for cells. Collapsed row borders continue to participate
  through collapsed-border conflict resolution.
- Repeated table header/footer groups are relaxed on fragments where reserving
  them would prevent row-group `break-inside: avoid` or forward pagination
  progress; source `thead`/`tfoot` rows still lay out in table order.
- Table row fragments carry an explicit committed mode for whole rows, sliced
  rows, and row-group `break-inside: avoid` rows kept together with small
  separated-table chrome overflow. Table-cell flow descendants consume that
  committed row fragment through planned child replay. Table body pagination
  now builds a row-fragment decision before painting and records the same
  decision in the table fragment plan, so cell-internal block layout does not
  create a conflicting later page break after row-group pagination has kept
  the group together. Table-body page boundaries are committed through a
  table-local boundary decision that owns footer handling and paint
  finalization; intermediate boundaries replay repeated footer chrome into the
  enclosing table fragment's paint bands, while final boundaries only record
  footer rows already present in source order. New
  table-body fragments are likewise created from a table-local start decision
  that owns the break reason and repeated header replay for that fragment.
  Avoided row groups and fitting avoided rows are represented as explicit
  source ranges, and their keep decisions record measured group height,
  destination repeat policy, and whether optional chrome was suppressed to
  allow bounded overflow. An oversized avoided row instead falls back to
  ordinary row fragmentation rather than being clipped by that group-only
  chrome-overflow fallback. Row-group overflow, ordinary row overflow,
  repeated-header progress retries, and
  oversized-row pagination consume a table-local fragmentainer value. That
  value is built from the shared cursor-bounds fragmentainer capacity primitive
  while keeping empty fragmentainer block size, remaining body capacity, and
  repeated footer reservation local to table layout. Repeated header/footer
  fit decisions are routed through a table-local chrome context that owns the
  target fragmentainer block size, optional chrome heights, and repeat
  eligibility flags before row-group avoid, avoid-run rollback, ordinary row
  overflow, oversized-row slicing, forced row breaks, or named-page row
  transitions commits an incoming repeat policy. The resulting outgoing and
  incoming choices are paired into one committed
  table-fragment transition. Row-overflow advances use the shared fragment
  advance gate after table-local repeated-footer and oversized-row overflow
  checks determine whether the row overflows the current body fragment. The
  final source row reserves the table's typed trailing non-content (the
  separated-border edge spacing plus bottom padding and border) during that
  fit check, so closing table decoration cannot silently escape the selected
  fragmentainer.
  Avoid-run rollback and row-group keep-together moves use the shared
  whole-source prebreak decision with an explicit fresh body fragmentainer, so
  repeated table chrome can reduce next-fragment capacity and forward-progress
  checks stay in the common overflow-and-empty-fit rule.
  Oversized rows choose each fragment-local slice height from the table body
  capacity, even at the top of a page where advancing is not possible, before
  table-cell child fragments are replayed. Inline atomic descendants inside
  split row pieces now emit their opacity and transform effects per visible
  piece rather than only once for the source row.
  Avoid runs formed by row or row-group `break-before: avoid` and
  `break-after: avoid` record their rollback candidate, measured run height,
  and incoming repeat policy before restoring to the chosen row boundary; the
  row's pending, before, after, and next-row break values are taken through the
  shared adjacent-box fragmentation context before table avoid candidates
  consume them; row-start rollback candidate arming uses the same shared
  predicates as block-flow avoid runs, and row avoid-boundary checks now
  consume the shared target-aware fragmentation break-opportunity model used by
  grid and flex. Row-boundary rollback selection uses the shared boundary-side
  classifier so previous-row `break-after` and current-row `break-before`
  avoids retain distinct candidate targets. Post-row candidate updates consume
  target-scoped authored avoid values from the same context while
  table-specific rollback candidate selection remains local. The rolling
  candidate state carries the active fragmentainer kind and is updated after
  each source row in one table-local state object. Table body row pagination
  receives that active kind from the wrapper planning step before constructing
  row-group keep ranges, forced-break carry state, row break opportunities, and
  avoid rollback state. Forced `break-after` carry-over consumes the shared
  target-aware forced-break carry state atomically with that break context: it
  becomes the next row's pending break when a following row exists, or the
  outgoing table break when the source rows are exhausted. Forced table-row
  transition decisions retain the active fragmentainer kind before applying
  the shared `FragmentainerKind` page-cursor materialization gate. Ordinary
  table body transitions caused by overflow, avoid rollback, row-group
  keep-together moves, and oversized-row continuations now retain the same
  active kind in their committed boundary/start decision before the replay
  layer applies that current page-only cursor materialization.
  Row groups kept together by bounded chrome overflow likewise keep their
  committed source range in table-local state until pagination advances past the
  row-group end.
  Table wrapper `break-before`/`break-after` transitions consume the shared
  standalone box break context and `FragmentainerKind` page-cursor
  materialization gate with the same active fragmentainer kind before falling
  back to outgoing row or row-group forced breaks.
  Row-group avoid decisions use the shared fragment advance gate for the
  current-fragment overflow trigger, while repeated-header/footer fit and
  bounded chrome overflow remain table-local choices. Row-group `break-inside`
  keep-together planning consumes the constraint through the active
  fragmentainer kind, so page-only and column-only avoids can share the same
  table planning hook as column fragmentation is added.
  Forced row and row-group breaks likewise commit the outgoing boundary,
  authored break value, and incoming table chrome policy as one table-local
  decision before page selection advances. Named-page class-A row boundaries
  commit the outgoing fragment boundary, target page name, and incoming chrome
  policy before the paged-media page context switch adjusts table placement.
  Durable table fragment plans now retain the active fragmentainer kind,
  incoming start decision, and outgoing boundary decision, so later paint and
  metadata handling can consume the same committed fragmentation choices while
  current PDF output still uses page-backed fragment metadata.
- Table wrappers, including inline-table atoms, resolve `width:min-content`,
  `width:max-content`, and `width:fit-content()` from measured table
  min/max-content column contributions before final column planning.
- Block-level table wrappers, including `html { display: table }`, keep
  `width:auto` shrink-wrapped to the final table grid when the table algorithm
  resolves below the available page width, and horizontal `auto` margins are
  resolved after that final grid width is known.
- Auto-layout table wrappers clamp definite used content widths to the grid
  min-content width when the authored width is too small, and parent
  min-content sizing consumes that clamped wrapper/margin-box contribution.
- Declared table-cell widths contribute their content width plus padding and
  cell border insets to column sizing; collapsed-border cells use the resolved
  grid-edge half-insets for both auto and fixed table layout.
- Separated-border column width distribution subtracts displayed horizontal
  `border-spacing`, including the two edge gaps, before assigning column
  widths; spanning-cell width constraints account for internal horizontal
  gutters and are replayed after single-column constraints in increasing span
  order before distributing width to columns; row and row-group structural
  backgrounds/outlines use the occupied visible column span rather than the full
  spacing-inclusive table grid.
- Table-cell source `string-set` and `position: running()` capture from the
  final visible cell fragment, and running cells are removed before table grid
  construction. Running-only table rows still capture the removed cell
  assignment from the collapsed row source marker. Split table-cell child
  replay consumes page-local fragment metadata for direct child named-string
  and running-element assignments, so
  page-margin `string()` and `element()` values emitted by a moved or split
  cell child are sourced from that child's first visible table-cell fragment.
- Fragmented table rows capture `string-set` from the first visible row piece,
  including rows that move to a later page before their source fragment is
  emitted.
- Repeated table header/footer replay suppresses element side effects for the
  copied rows, so repeated visual copies do not emit duplicate page-margin
  assignments, anchors, or bookmarks.
- Table-root `string-set` uses the final moved table fragment for
  `string(..., start)`, and table-root `position: running()` can replay table
  text and cell background paint into page-margin boxes.
- Table-row `position: running()` is removed from table flow and assigned from
  a zero-size source marker after row forced-break handling, so
  `element(..., start)` resolves from the row's final source page. Running
  table rows replay nested table and flex descendants through the existing
  isolated `element()` path.
- Pre-rendered nested table/flex fragments inside split table-cell pieces
  replay descendant named-string and running-element assignments onto the
  visible table-cell fragment.
- Rowspanning table-cell child fragments, including nested table/flex
  descendants, use the first visible page fragment for `string(..., start)` and
  `element(..., start)` lookups.
- Fragmented table row layout switches named page context at row boundaries
  from row, cell, and cell-descendant `page` values, and preserves explicit
  `page:auto` as an exit from an enclosing named page group.
- Row-spanning cells with specified `page` values keep the named page context
  across spanned row boundaries, while explicit `page:auto` rows can leave that
  context before subsequent row content.
- Repeated table header and footer copies paint in the destination page
  context instead of re-entering the copied row group's source `page` value.

## Remaining Gaps

- The XHTML table reftest `table-align-float.xhtml` cannot currently render:
  the XML parser treats simultaneous `xml:lang` and `lang` attributes as a
  duplicate attribute. Parser replacement work is deliberately deferred.

- Absolutely positioned collapsed tables preserve their authored grid width
  while resolving the inset equation: collapsed grid-edge borders are not
  subtracted as ordinary wrapper insets. Broader table-wrapper sizing cases
  still need coverage.
- Anonymous table object construction still needs a malformed-markup audit
  beyond the covered common fixup, inline-wrapper, consecutive non-cell
  whitespace, empty-row, and span-parsing cases.
- Auto and intrinsic table width have residual edge cases around column groups,
  collapsed-column interactions, and less common multi-span combinations beyond
  ordered colspan distribution.
- Table-cell overflow behavior is incomplete for the full CSS Overflow 3
  surface, including scrollbar painting and propagation beyond the retained
  union-of-rectangles clipping used for collapsed tracks.
- Structural table background images remain incomplete for row groups and
  vertical-writing or fragmented tables. CSS Tables 3 requires their
  backgrounds to use cell-derived geometry and clipping across all spans and
  fragment boundaries.
- Fragmentation still needs full cloned decoration semantics and broader
  coverage for rare spanning-cell/collapsed-track combinations, complex
  repeated header/footer interactions, rare rowspanning assignment propagation
  interactions, complex table-root running-element descendants, and complex
  nested table/flex descendants across page boundaries.
- Table direction and writing-mode support still needs broader WPT coverage for
  full vertical table row/cell placement, mixed writing modes, and unusual
  column-group/span combinations.
- CSS 2.2 table-internal row and row-group margins, borders, and padding need
  a final no-op audit. The two remaining local non-tentative WPT failures are
  `row-margin-border-padding.html` and
  `row-group-margin-border-padding.html`.
- Height and baseline coverage should be broadened with WPT and local
  WeasyPrint cases for full horizontal-axis baseline positioning in mixed
  writing modes, complex floats inside cells, large percentage matrices,
  percentage min/max-size combinations, and complex nested formatting contexts.
  In particular, multi-level percentage-height descendants whose final height
  transfers through an aspect ratio need a second column-sizing contribution
  before final table-cell content relayout is painted.

## Test Backlog

- Port high-value height cases from `WeasyPrint/tests/layout/test_table.py`,
  especially row-height, vertical-align, wrapper sizing, malformed span
  placement combinations, and inline-table baseline scenarios.
- Add WPT-derived tests for CSS Tables 3 height distribution, row-group
  percentage resolution, spanning cells across collapsed rows, table-cell
  percentage min/max-size combinations, and cell baseline fallback.
- Expand visual PDF comparisons for fragmented collapsed-border tables with
  colspans, rowspans, repeated headers/footers, collapsed tracks, RTL mixed
  writing-mode cases, and nested block/flex/table content.

## Implementation Notes

- Keep durable table fragments as the source of truth for downstream
  measurement, layout, painting, and fragmentation.
- Anonymous table and row wrappers retain inherited deferred font-size state,
  so later font-metric resolution cannot reset generated table content to the
  initial font size. Orthogonal cell row sizing uses the resolved column
  allocation for an explicit physical height instead of treating that height
  as a direct horizontal-row constraint.
- Avoid estimate-only shortcuts for nested formatting contexts in table cells;
  prefer reusing the corresponding layout subsystem's measurement adapter.
- Update `SPEC_DIVERGENCES.md` whenever a remaining table gap is narrowed or a
  new spec divergence is identified.

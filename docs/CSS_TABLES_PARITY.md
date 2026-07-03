# CSS Tables Parity

Last updated: 2026-07-06

CSS 2.2 table layout, CSS Tables Level 3, CSS Sizing Level 3, CSS
Fragmentation Level 3, and HTML table semantics are the conformance targets.
WeasyPrint tests remain useful comparison material for paged-output behavior,
but spec conformance takes priority when behavior differs.

## Current Level

- Table formatting is implemented under `src/layout/table/` with durable row,
  row-group, column, caption, span, and collapsed-border structures.
- HTML table span attributes use one shared parser for durable fragments and
  layout grids: leading ASCII digits are parsed, trailing junk is ignored,
  invalid values default to 1, `rowspan=0` spans to the row-group end, and
  column/row spans are clamped to the HTML table-model bounds.
- Authored empty table rows are preserved in durable table fragments and can
  contribute explicit row height to the grid.
- Orphan table-internal boxes generate anonymous table wrappers, including
  inline-table wrappers when the missing parent is generated inside an inline
  formatting context.
- Anonymous cell construction preserves whitespace between consecutive
  non-table-cell children, while still ignoring formatting whitespace adjacent
  to table-internal boxes.
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
  cell or table root as explicitly height-sized.
- First-pass table-cell row minimum sizing treats descendants whose block size
  depends on the parent cell height as auto, including overflow scroll
  containers whose final percentage height is resolved during relayout.
  Intrinsic table-cell sizing now follows normal-flow descendants through those
  unresolved percentage-height wrappers for both row height and column width.
- Table wrapper `min-height` participates in grid height distribution after
  separated wrapper padding and borders are subtracted; `max-height` caps
  definite table height targets but does not shrink intrinsic auto-height rows
  by itself.
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
- Baseline behavior covers direct text, multiline content, block children,
  nested table rows, non-text fallback, inline-table baselines from the first
  visual row, and vertical-writing table cells no longer inflate physical row
  height with horizontal-axis baselines.
- Positioned table wrappers establish the containing block used by absolute
  descendants inside captions as well as the row grid.
- Empty tables keep wrapper padding, borders, captions, definite grid size, and
  border-box sizing in separated-border layout and painting; collapsed empty
  tables ignore separated wrapper padding/borders and use the empty collapsed
  grid's zero outer insets.
- Table box `overflow:hidden` and `overflow:clip` clip to the table box rather
  than the table wrapper that contains captions, while table box
  `overflow:auto` and `overflow:scroll` behave as visible per the CSS 2.1
  errata.
- Collapsed-border conflict resolution covers table, column-group, column,
  row-group, row, and cell origins, including column and column-group
  block-edge candidates and floored CSS-pixel width priority for subpixel
  borders; rowspans and colspans suppress internal grid edges so structural
  borders do not paint through spanning cells.
- Collapsed tables ignore table-root padding and `border-spacing`, derive
  wrapper insets from the widest resolved outer grid-edge half-width across
  all displayed row segments, derive cell content insets from resolved
  grid-edge border half-widths, and paint collapsed `inset`/`outset` as
  `ridge`/`groove` with side-aware two-tone 3D borders.
- Floated collapsed table wrappers, including floats collected through inline
  layout around clearing breaks, use durable table fragments for shrink-to-fit
  grid sizing and add resolved collapsed outer insets only to the float
  exclusion/visual margin box.
- Collapsed table-cell decorations use resolved collapsed-border half-insets
  for background and rectangular inset `box-shadow` geometry while the
  collapsed-border grid remains the only border painter.
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
- Table column and column-group structural backgrounds paint full background
  layers, including URL images and CSS Images Level 3 linear/radial gradients. Vertical
  `writing-mode` tables now consume column `height` as a column inline-size
  input and project `vertical-rl; direction: rtl` column backgrounds with the
  first logical column on the physical bottom.
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

- Anonymous table object construction still needs a malformed-markup audit
  beyond the covered common fixup, inline-wrapper, consecutive non-cell
  whitespace, empty-row, and span-parsing cases.
- Auto and intrinsic table width have residual edge cases around column groups,
  collapsed-column interactions, and less common multi-span combinations beyond
  ordered colspan distribution.
- Table-cell overflow behavior is incomplete for the full CSS Overflow 3
  surface, including scrollbar painting and propagation.
- Fragmentation still needs full cloned decoration semantics and broader
  coverage for rare spanning-cell/collapsed-track combinations, complex
  repeated header/footer interactions, rare rowspanning assignment propagation
  interactions, complex table-root running-element descendants, and complex
  nested table/flex descendants across page boundaries.
- Table direction and writing-mode support still needs broader WPT coverage for
  full vertical table row/cell placement, mixed writing modes, and unusual
  column-group/span combinations.
- Height and baseline coverage should be broadened with WPT and local
  WeasyPrint cases for full horizontal-axis baseline positioning in mixed
  writing modes, complex floats inside cells, large percentage matrices,
  percentage min/max-size combinations, and complex nested formatting contexts.

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
- Avoid estimate-only shortcuts for nested formatting contexts in table cells;
  prefer reusing the corresponding layout subsystem's measurement adapter.
- Update `SPEC_DIVERGENCES.md` whenever a remaining table gap is narrowed or a
  new spec divergence is identified.

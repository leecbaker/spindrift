# CSS Multi-column and Containment Parity

This note tracks Quire's implementation of CSS Multi-column Layout Level 1,
CSS Containment Level 1, and their shared CSS Fragmentation behavior.

## Current support

- `column-count`, `column-width`, `columns`, `column-gap`, column rules,
  `column-fill` (`auto`, `balance`, and `balance-all`), and `column-span`
  (`none` and `all`) have computed-value models and cascade support.
- CSS `zoom` derives a separate effective used style for multicol layout.
  Fixed `column-width`, Level 2 `column-height`, gaps, column-rule widths,
  and rule endpoint insets scale once; percentages and algorithmic keywords
  remain deferred to the zoomed column box. Frozen source styles continue to
  own descendant cascade and replay, so nested sets and spanners do not
  compound their ancestor scale.
- Normal-flow block children use finite anonymous column fragmentainers rather
  than index-based round-robin placement. Column transitions share the page
  fragmentation machinery, including forced breaks, target-specific avoid
  values, `break-inside`, widows/orphans, and rollback to an earlier class A
  boundary.
- One-column multicol containers and overflow columns progress in the inline
  direction. Column boxes use isolated float contexts and column-local
  containing widths. Auto-height sequential overflow column rows continue on
  later pages. Direct out-of-flow children are positioned against the multicol
  container after speculative column layout.
- Float block extents constrain balancing independently of normal-flow block
  extents, so a float can fragment and repaint through anonymous columns
  without being double-counted beside in-flow content. The structural estimate
  packs consecutive float margin boxes into width-constrained shelves, using
  the tallest float in each shared band rather than summing side-by-side
  left/right floats. Monolithic float bounds preserve collapsed sibling-margin
  state through transparent anonymous wrappers, so negative adjoining margins
  can place part of a float above the fragmentainer start. A spanner terminates
  the preceding float scope, and empty post-spanner column sets retain zero
  intrinsic block size instead of synthesizing an inherited line-height.
- Anonymous column paint preserves inline overflow across column gaps and
  neighboring columns, then intersects the finished fragment with its outer
  page area. This retains authored multicol overflow while enforcing the page
  fragment's initial-containing-block boundary.
- Sequential `column-fill:auto` treats max-height as a fragmentainer limit
  while deriving the column set's used block size from committed content.
  Column rules span only columns that actually receive inline content, and
  descendant inline-axis overflow remains visible across anonymous column
  boundaries until the multicol container's own overflow clips it.
- Page-constrained `column-fill:auto` rows preserve structurally occupied
  anonymous columns even when their source boxes emit no paint. An empty
  completed outer page is retained instead of being reused by the page-empty
  guard, and following flow starts after the actually consumed part of the
  final column row.
- Column-rule eligibility uses structural column occupancy rather than emitted
  ink, so non-painting overflow fragments still count as content. A page-local
  paint insertion cursor places each rule set immediately after its multicol
  container decoration and before that container's descendants without
  manufacturing a stacking context. Solid rules use filled areas, and
  consecutive same-paint PDF rectangles share a compound path to avoid
  antialias seams at mathematically coincident edges.
- Definite-height multicol containers nested in an outer column separate the
  principal box's authored block size from each outer-fragment-local column-set
  height. Completed rows of inner columns continue in the next outer
  fragmentainer, the final content row is balanced independently, and trailing
  decoration-only fragments preserve the principal box's remaining size.
- Ordinary block backgrounds, borders, and outlines are generated for every
  page or column fragment. Slice decoration owns only the first/last block
  edges, while cloned block-end decoration reduces the content capacity of
  every nested multicol fragment.
- Definite scroll containers with block descendants and unbreakable inline
  lines in a zero-height column are treated as monolithic column-flow subjects.
  Their padding-edge clips remain attached to the subject while the multicol
  replay avoids graphically slicing them into unrelated continuation columns.
- Balanced column sets use iterative snapshot probes over the real column
  fragmentation algorithm, with structural bounds for monolithic and
  avoid-connected content. Authored extreme lengths use a bounded structural
  estimate so speculative cost is independent of numeric magnitude. Committed
  definite-height overflow likewise materializes only the bounded anonymous
  column replay horizon and carries a conceptual off-canvas tail
  arithmetically, rather than allocating one temporary page per authored
  fragmentainer. One committed flow pass paints the selected fragments, so
  counters and generated content state are not advanced by balance probes.
  Horizontal balancing converges below the rasterizer's subpixel threshold.
  For a definite horizontal set, including ordinary sets and sets after a
  spanner, the anonymous column
  fragmentainer height is balanced independently from the set's allocated
  used block size, so rules and cursor consumption still reach the content
  edge.
- Intrinsic multicol sizing includes one max-content contribution per
  requested automatic-width column, counts gaps once, recursively measures
  nested block formatting contexts, treats eligible spanners as a single
  full-width contribution, and combines spanner and non-spanner min/max-content
  contributions independently.
- Flexbox cross-size estimation preserves the block/spanner structure of an
  auto-height multicol flex item instead of replacing it with a trailing inline
  line estimate. Atomic inline flow roots enter the shared column-set planner,
  so form controls can establish real multicol formatting contexts.
- Normalized text and forced-break inline boxes use one authoritative
  multicol line-sequence pass, avoiding an ordinary anonymous-block pass before
  column balancing. Atomic inline formatting contexts retain their
  fragment-owned replay path and resolve percentage sizes against the
  anonymous column box rather than the multicol principal box.
- Direct and eligible descendant `column-span: all` boxes split a multicol
  container into independent column sets. Ordinary intervening block and split
  inline wrappers are sliced around the promoted spanner; adjacent spanners
  remain in normal block flow. Empty self-collapsing wrapper fragments do not
  synthesize a line-height column before or after a promoted spanner, and a
  trailing normalized anonymous inline run remains in the final column set.
- Visible descendant overflow from a fitting definite-height box is projected
  into later anonymous columns independently from the principal box's
  normal-flow end. Following siblings therefore resume after the authored
  principal size rather than after the descendant overflow. Forced descendant
  breaks remain on the regular parallel-flow fragmentation path, preserving
  their real column boundaries before a following spanner.
- Balancing tracks normal-flow progression and parallel fragment reach as
  separate quantities. A definite principal contributes only its authored
  size to the following sibling cursor, while deferred descendant and
  positioned paint assignments count toward the number of anonymous columns
  required by speculative fit probes. One-column sets retain visible overflow
  without treating it as a competing balance height.
- Atomic inline-level boxes contribute their used outer block size to the
  monolithic balance lower bound. Inline-block sequences therefore select a
  column height that can contain each atomic box instead of falling back to
  the surrounding text line-height.
- Vertical multicol spanners resolve their auto logical inline size against
  the multicol content box. Orthogonal flow roots contribute physical width to
  shrink-to-fit sizing, keep physical-width and logical-inline percentage
  bases distinct, and preserve block-axis overflow across one unbroken column
  fragment. `sideways-lr` and `sideways-rl` retain their specified logical
  axes and use horizontally shaped glyph runs rotated in their specified
  line orientation.
- `contain` supports the Level 1 grammar and canonical effects: `none`, `size`,
  `layout`, `paint`, `content` (`layout paint`), and `strict`
  (`size layout paint`). The property is non-inherited.
- Size containment contributes intrinsic and automatic used sizes as if the
  principal box were empty, including flex-item estimates and replaced natural
  size suppression. Authored empty-grid tracks and empty-multicol column gaps
  still contribute their own formatting-context geometry. Descendants are laid
  out in place while the size-contained principal box retains its independently
  resolved used size. Definite principal boxes consume that size continuously
  through crossed fragmentainers, while visible descendant overflow can reach
  later fragmentainers without contributing to the principal used size. A box
  that fits is retained as one typed paint scope; oversized paint is projected
  into page/column-local slices, and clipped overflow remains monolithic inside
  its used padding-box clip.
  Content-sized buttons, empty-option selects, fieldsets, and table descendants
  use the same empty-principal-box sizing rule.
- Layout and paint containment establish independent formatting contexts,
  distinct absolute/fixed containing blocks, and stacking contexts across
  block, flex, grid, and table-wrapper paths. Paint containment clips
  normal-flow and captured positioned descendant paint at the used padding
  edge, including zero-area padding boxes. Definite principal block sizes are
  exported before inline-atom preparation so percentage-sized atomic
  descendants use the containing block's own definite basis rather than an
  unrelated indefinite replay basis. Table and table-cell padding-edge clips
  use resolved table geometry, and table captions are clipped across their
  caption block extent to the table wrapper's contained inline edge.
  Out-of-flow cell descendants are collected only after final table-grid
  geometry, preventing a provisional page-relative layer from escaping a
  layout-contained cell. Auto-width captions subtract their own padding and
  borders before freezing the used content-box width, so containment does not
  inflate the table wrapper measure.
  Containment applicability excludes non-atomic inline, ruby-internal, and
  non-cell internal-table boxes.
  Rounded padding-edge clips are emitted as PDF path clips for captured
  descendant paint scopes.
- Layout-containment baseline suppression preserves an earlier eligible line
  baseline in the surrounding atomic flow root while preventing the contained
  block's descendant baseline from replacing it.
- Contained `html`/`body` boxes no longer propagate their background or
  overflow to the document canvas. Ordinary root/body propagation remains
  unchanged.

The local `css/css-break/break-between-avoid-000.html` through `-014.html`
reftests currently pass as a group. In the July 2026 local non-script
`css/css-multicol` run, 275 of 366 renderable tests passed with no renderer
errors or timeouts. The corresponding `css/css-contain` run passes 270 of 302
renderable tests, also with no renderer errors or timeouts. The
remaining
failures are classified below rather than excluded.

## Remaining parity work

- Iterative balancing includes deferred descendant and positioned fragment
  assignments, but more complex nested parallel flows, inline formatting
  contexts, floats, and some widows/orphans and forced-break matrices still
  choose a non-optimal column block size.
- Balance probes reject geometric overflow from an unbreakable line even when
  that overflow has not yet allocated another anonymous column. This preserves
  a real lower bound for per-column `text-box-trim` planning.
- Descendant spanners pass through eligible ordinary wrappers, and definite
  principal boxes keep visible descendant overflow independent from their
  normal-flow end. More complex parallel-flow boundaries, full spanner margin
  collapsing, fragment-local wrapper decoration, and several nested
  spanner/page-continuation combinations remain incomplete.
- Vertical spanner auto sizing and single-root orthogonal overflow replay are
  logical-axis aware. General vertical column placement, multi-fragment
  orthogonal replay, and column-rule geometry still use a physically
  horizontal block-child canvas.
- Positioned descendants nested below fragmented relative/effect containers
  retain future fragmentainer assignments, and positioned principal decoration
  continues through its recorded span. Remaining fixed-position, static-
  position, transformed-containing-block, and spanner interactions still need
  one durable multicol containing-block model across every nested case.
- Tables, grids, flex containers, floats, and replaced content now consume an
  active column fragmentainer, but their column-local paint and side-effect
  replay needs more interoperability coverage. Repeated table chrome and
  nested fragmentation are especially sensitive.
- Definite-height continuous overflow columns progress in the inline direction,
  and auto-height sequential column rows can continue on later pages. Complex
  paged continuation must still rebalance according to `balance` versus
  `balance-all`, preserve named-page geometry, and carry spanners and nested
  positioned descendants across page rows.
- `box-decoration-break: clone` makes finite one-CSS-pixel content progress in
  a zero-height column, constructs the full border/padding geometry around each
  fragment, and retains that decoration as one fragment-local paint unit
  without reinterpreting it as additional flow columns.
- Speculative columns currently commit counters, quote depth, and assignment
  identifiers. Named strings, running elements, anchors, links, bookmarks, and
  other captured side effects need explicit fragment remapping.
- Size containment handles the internal table/ruby and non-atomic-inline
  applicability exceptions plus block-flow used sizing. In column flow, both
  the principal box and descendant layout form one monolithic fragmentation
  subject while visible descendant overflow remains attached to that subject.
  Remaining replaced flex/table edge cases, baseline export, positioned
  overflow, and monolithic overflow through every remaining formatting context
  still need broader coverage.
- Layout containment suppresses forced-break propagation across its boundary
  while preserving forced breaks inside a nested fragmentation context. The
  broader fragmented-flow trapping rule and principal writing-mode propagation
  from contained `html`/`body` elements remain incomplete.
- Fragmented paint-containment effect-group semantics and non-polygon
  `clip-path` shapes remain part of the separate paint-effects work.
- CSS Multicol Level 2 `column-height` and `column-wrap`, and CSS Containment
  Level 2 `inline-size`, style containment, and `content-visibility`, are not
  part of the Level 1 implementation and account for a visible subset of the
  unfiltered local directory failures. In particular, `column-height-008`
  previously passed accidentally through unbalanced Level 1 fallback behavior;
  definite Level 1 balancing now exposes the unsupported Level 2 row model.

## Specifications

- <https://www.w3.org/TR/css-multicol-1/>
- <https://www.w3.org/TR/css-contain-1/>
- <https://www.w3.org/TR/css-break-3/>

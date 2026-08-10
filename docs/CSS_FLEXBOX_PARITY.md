# CSS Flexbox Parity

Last updated: 2026-08-09

This document tracks Quire's implementation status for CSS Flexible Box
Layout. The normative references are CSS Flexible Box Layout Level 1, CSS Box
Alignment Level 3, CSS Writing Modes Level 4, CSS Sizing Level 3, and CSS
Fragmentation Level 3. WeasyPrint tests and behavior are useful compatibility
references, but spec conformance is the priority when the two disagree.

## Current Implementation

- `margin-trim` is applied as a container-owned used-margin plan. It covers
  every logical edge across row/column and reverse directions, uses the
  container writing mode for orthogonal items, and derives wrapped-line edge
  items from the order-modified non-collapsed flex line topology before final
  sizing and replay. Adjacent opaque item backgrounds can still expose a
  one-pixel PDF raster stitching seam because their independent paint scopes
  are not yet coalesced. Fragmentation-specific trimming remains deferred.

- The local renderable `css/css-flexbox/` WPT run currently passes 647 of 935
  tests (69.20%). This raw reftest baseline includes reference-side rendering
  gaps (notably CSS-versus-image color precision), so it
  is a triage input rather than a direct count of flex-algorithm defects. The
  largest material layout clusters are baseline/writing-mode layout, intrinsic
  sizing and automatic minimums, tables as flex items, absolute static
  positioning, and the remaining intrinsic/available-size `flex-wrap: balance`
  Level 2 cases.

- Flex layout is implemented under `src/layout/flex/`.
- The raster members of the non-script `flex-aspect-ratio-img` reftest family
  exercise correct flex/replaced-image geometry. Their former exact-PDF
  mismatch came from raster-image XObject edge coverage versus a vector
  reference fill, not from aspect-ratio transfer or flex sizing. Eligible
  opaque uniform raster images now serialize as calibrated vector fills at the
  PDF boundary, while flex layout retains its original image geometry.
- Flex containers and items consume an effective-zoom used style at the flex
  layout boundary. Fixed flex bases, gaps, box edges, margins, and intrinsic
  replaced sizes scale once; percentages resolve against the resulting zoomed
  containing geometry, while automatic and intrinsic sizing remain algorithmic.
- Taffy 0.12.2 remains the core engine for line formation, flexible length
  distribution, wrapping, gaps, auto margins, and common alignment.
- Taffy-specific limitations and Quire adapter workarounds are tracked in
  `docs/TAFFY_SHORTCOMINGS.md`.
- Quire supplies CSS/PDF-specific adapter behavior for flex child collection,
  anonymous text items, intrinsic estimates, `flex-basis` keyword resolution,
  automatic minimum sizes, replaced-item sizing, baseline correction, absolute
  flex child static positions, inline-flex atoms, and PDF painting.
- Flex child collection shares the blockified formatting-context itemization
  path with grid child collection. Normal-flow flex items establish an
  independent formatting context for intrinsic estimates and replay, so first
  and last child block margins stay contained by the flex item, floats do not
  intrude into sibling items or outside block flow, and blockified inline-source
  item paint is normalized before item layout. Anonymous flex text items are
  created for non-whitespace text runs, while CSS document-whitespace-only text
  runs are ignored even in preserved `white-space` modes.
- A flex container remains an ordinary in-flow block for its own external
  margin collapse with an eligible parent or sibling. Its formatting context
  only prevents its flex items' margins from collapsing through the container.
- Direct tree-abiding flex/grid children are blockified during box-tree
  construction, before CSS Tables anonymous-wrapper fixup. Consequently a
  `table-cell`, `table-row`, or row group directly inside a flex container is
  an independent block-level item instead of becoming part of a synthetic
  table fragment with its siblings.
- A normal-flow flex root shares the BFC float-band placement path with block
  layout when automatic sizing or a negative inline margin can fit beside an
  active float. This keeps ordinary fixed-width flex roots below an obstructing
  float while allowing a fitting negative-margin root to retain its resolved
  inline origin.
- Absolutely positioned flex children are collected out of flow. Their
  static-position probe uses fixed sole-item sizing and ignores authored
  flexing, including `flex-basis`, while normal absolute positioning computes
  the final used size. A typed static-geometry record keeps the hypothetical
  margin-box main-axis interval distinct from the flex content-box cross-axis
  edges, and does not override the real absolute-position containing block.
  Thus `auto` inset pairs use the flex static-position rectangle while final
  inset equations and safe overflow use the positioned-layout containing
  block, including horizontal and vertical writing modes.
  Distributed main-axis values are resolved for the hypothetical sole item:
  `space-between`/`stretch` use start fallback and
  `space-around`/`space-evenly` use center fallback.
- Flex container `width:min-content`, `width:max-content`, and
  `width:fit-content` route through explicit flex intrinsic contribution
  records rather than final Taffy layout. Item records carry min/max main and
  cross contributions, flex base size, hypothetical main size, grow/shrink
  factors, definite cross-size contributions, and gap handling. Wrapped row
  min-content uses the largest item contribution, nowrap rows keep all items
  on one line, and intrinsic item contributions use their intrinsic size plus
  a non-auto preferred main size before min/max clamping; an auto preferred
  size is not replaced by an automatic flex base size, while a definite basis
  caps non-growing item contributions. Cyclic percentage gaps contribute only
  their non-percentage length component, while used flex layout resolves percentage
  and mixed `calc()` gaps against a definite physical content-box axis when
  one is available. Block flex item intrinsic widths merge same-line floated
  children into the max-content inline contribution, so wrapped column
  `flex-basis:auto` items are remeasured against the max cross available width
  before line packing. Definite flex item widths contribute to column flex
  min-content inline sizing, including wrapped column containers
  with empty fixed-width items. Wrapped column flex min-content inline sizing
  uses the largest item cross contribution rather than summing columns, and
  auto-width floated/atomic wrapped column flex containers shrink-to-fit to
  that cross contribution unless a smaller containing block clamps between the
  min-content and wrapped max-content widths. Wrapped CSS column containers
  remeasure max-content item contributions with the largest max-content cross
  contribution as the available cross size, so percentage-width items and
  float-only block descendants participate in flex-basis calculation with the
  same available size used by the intrinsic cross-size algorithm.
- Nested percentage-width tables now expose their grid-based intrinsic minimum
  with an indefinite percentage basis while retaining their definite preferred
  flex base size. This prevents a percentage table wrapper from becoming an
  artificial automatic minimum for its flex-item ancestor.
- Table flex items consume a table-owned wrapper-sizing contract. It keeps
  grid min/max-content contributions distinct from wrapper preferred inline
  and caption-inclusive intrinsic block contributions, while wrapper borders,
  padding, and margins remain in their own box-model spaces. The intrinsic
  block probe clears authored `height` and `min-height`, so a specified table
  height is a flex preferred-size suggestion rather than a second automatic
  minimum. Speculative table-height caching includes resolved block
  constraints, preventing a stretched or specified wrapper measurement from
  leaking into an intrinsic flex probe.
- The flex/Taffy margin and padding adapters resolve percentage and mixed
  `calc()` values against the containing block's logical inline percentage
  basis before converting to physical Taffy edges. Replay uses the same used
  padding, avoiding a vertical container's inline basis being conflated with
  its physical width. Flex replay resolves that padding before freezing final
  main-axis bounds, so a percentage padding edge cannot be added a second time
  by the independently replayed formatting context.
- Flex item replay clears both cached and typed margins after Taffy positions
  the item's margin box, so normal-flow replay cannot reapply fixed margins.
- An auto-height physical-row flex container derives its final cross extent
  from reconciled item margin boxes, resolving percentage margins with the
  container's logical-inline basis. This prevents a provisional Taffy root
  height from surviving automatic-minimum or aspect-ratio correction.
- Horizontal auto-height row containers carry a typed content-based line
  constraint rather than treating a provisional Taffy root height as a
  definite cross size. Each auto-height item is remeasured at its post-flexing
  content width before line cross sizing; that update refreshes ordinary
  cross metrics and baselines while preserving replay-only fragmentable
  overflow. Stretch remeasurement therefore cannot overwrite the hypothetical
  contribution that establishes the content-based line.
- Flex intrinsic line measurement distinguishes a container content-box
  constraint from an item content-box constraint. A definite preferred cross
  size or post-flex main size is already the item's content box, so its
  padding is not removed a second time before selecting inline lines. This
  keeps the item’s hypothetical main extent, final line slot, and measured
  baseline aligned with the text that replay actually paints.
- A single final cross-axis placement phase runs after main-size and cross-size
  remeasurement, automatic-minimum correction, and line-slot refresh. Taffy
  continues to construct lines, size stretch items, and resolve auto cross
  margins; Quire resolves the final CSS Align side, center, safe-overflow, and
  first/last baseline placement from those final margin-box slots. Baseline
  metadata is then refreshed before `align-content`, RTL mirroring, and other
  whole-line translations move items and their exported baselines together.
- Flex item replay materializes each principal background and border from its
  frozen final physical border box before replaying the independent formatting
  context. The resulting decoration and descendant paint are retained as one
  inline-block-equivalent item paint unit in order-modified document order,
  while positioned descendants continue to escape unless the item establishes
  a real stacking context. This preserves empty-item decoration and makes
  orthogonal flex-item paint use the same physical geometry as line formation;
  a replaced content-box item on a physical-column main axis retains its
  resolved outer main size during replay, so that replay does not add its
  padding and border a second time.
  split-item decoration remains subject to the fragmentation limitations
  listed below.
- A wrapped physical-column flex container that remains in one fragmentainer
  replays one complete order-modified item sequence rather than synthetic
  physical-Y source slices. This preserves `column-reverse` and
  `wrap-reverse` item contents, while actual page/column continuations retain
  their separate interval planner. `justify-content: normal` and `stretch`
  also resolve as `flex-start` in reverse main axes, preserving the free-space
  offset before final replay.
- A size-contained flex item keeps its definite used main size while its
  intrinsic contribution is sized as empty. When that monolithic item spans
  pages, each continuation clips the corresponding block interval of one
  frozen source canvas rather than restarting its contents at the next page.
  This covers the direct and wrapped `monolithic-overflow-005/006-print.html`
  CSS Page reftests; it follows CSS Containment's size-containment and
  monolithic-box rules together with Flexbox pagination.
- Whole-flex prebreak decisions retain the distinction between a border-box
  start and its preceding physical top margin. A flex container whose margin
  box starts at a new page is laid out there rather than repeatedly advancing
  to another page; this keeps vertical-writing percentage-gap cases finite.
- Isolated float sizing and float replay suppress whole-flex prebreaks. This
  prevents a floated orthogonal flexbox from materializing page-height replay
  fragments merely while its auto height is being measured.
- A nested bottom-to-top vertical block retains its horizontal parent's
  physical block cursor. Only the principal body box is page-inline-end
  anchored, preventing RTL vertical float replays from converting a short
  logical inline extent into a page-height fragment.
- Flex containers retain first/last exported baseline sets separately for
  physical vertical and horizontal axes until the parent formatting context
  selects a compatible baseline. Final baseline export uses reconciled flex
  line and item geometry, so `order`, wrapping, `wrap-reverse`, and
  `align-content` placement affect the exported coordinate. The inline-flex
  atom no longer substitutes its captured paint fragment's first line for
  missing flex metadata. Intrinsic nested-flex estimates use the same
  shared-line-baseline-first selection rule as final layout, including
  order-modified fallback selection for reversed main axes. Remaining
  Baseline participant eligibility, synthesized-baseline selection, and CSS
  Align fallback are resolved together after final flex-item remeasurement,
  rather than in separate physical row/column correction passes. Inline-flex
  atoms convert final content-box baseline sets to physical border-box
  coordinates once, then the parent inline context projects the compatible
  physical axis through its logical block-start side. This covers horizontal,
  `vertical-lr`, and `vertical-rl` inline-flex baseline transport.
- Replayed flex and grid item formatting contexts now retain an explicit
  physical normal-flow containing-block scope for relative/sticky descendants.
  Block, nested flex/grid/table, and inline formatting contexts consume that
  scope, so percentage insets resolve against the item's used content box
  without falsely establishing an absolute-positioning containing block.
- Block-start parent/first-child margin collapse remains valid when the parent
  has a specified height; only block-end collapse depends on auto height. This
  keeps a definite-height document body from introducing an extra top margin
  before its first flow child.
- Grid's Taffy adapter maps CSS Grid's logical inline/block tracks, placement
  lines, auto tracks, alignment axes, and gaps to physical Taffy columns/rows
  in vertical writing modes. This lets a vertical-writing grid flex item size
  `grid-template-rows` along physical width rather than physical height.
- A grid container participating as a flex item now contributes its measured
  Grid track sizes to flex base and automatic-minimum sizing rather than using
  an inline text-line fallback. This preserves the zero intrinsic height of an
  empty grid item and keeps flex replay's temporary used height separate from
  Grid's percentage definiteness.
- Simple inline-content multi-column flex items measure their inline sequence at
  the resolved column width and use the balanced column block-size for
  auto-height hypothetical cross sizing, so flex replay paints all balanced
  column lines and column rules.
- Normal-flow block flex containers preserve definite used widths even when
  they overflow the containing block, and fixed-width block flex containers
  resolve horizontal auto margins with the same CSS 2.2 block-width equation as
  normal block boxes. Orthogonal normal-flow flex containers with
  `width:auto` route through flex intrinsic sizing so their physical block axis
  shrink-wraps rather than filling the containing block's horizontal span.
  Auto-height row flex containers apply CSS min/max-height constraints to the
  used container height, including the rule that a larger `min-height` wins
  over a smaller `max-height`. Flex containers with an automatic height and a
  preferred `aspect-ratio` transfer their definite used width into a definite
  content height for flex item cross-size resolution.
- Flex item margins preserve negative length and percentage values through the
  Taffy layout bridge, so overlapping flex items participate in
  order-modified layout and paint in the CSS Flexbox `order` sequence.
- Intrinsic `min-content` and `max-content` maximum cross sizes cap stretch
  targets before final item replay. Auto main sizes are then remeasured in the
  capped cross size, so floats and inline wrapping use the same dimensions as
  the final flex item.
- CSS math `calc(infinity)` is accepted for flex factors. Before adapting to
  Taffy, an infinite grow factor is normalized into the spec's finite used
  distribution: all infinite growers receive equal shares and finite growers
  receive none.
- Max-content flex container sizing uses the concrete ideal flex-fraction
  algorithm from CSS Flexbox 9.9.1.1. The draft's web-compatible intrinsic
  sizing algorithm remains unresolved, so any future browser-compatibility
  deltas should be tracked as spec divergences rather than encoded as
  undefined behavior.
- Flex item base-size estimates use a definite stretched cross size as the
  descendant containing-block inline size. This covers vertical-writing items
  whose descendants resolve percentage padding from the stretched physical
  height before the item's row-axis flex base size is finalized, and
  horizontal row flex items whose inline replaced descendants resolve
  `height:100%` against the stretched item before transferring width through
  an intrinsic aspect ratio. Percentage-height descendants of column flex
  items resolve against the item's definite post-flexing main size instead of
  the remaining fragmentainer height. Once an auto-height row flex
  container's line cross size is final, its items replay with that final
  content-box height as a definite descendant percentage basis, including
  lines clamped by `min-height` or `max-height`. Auto cross-axis margins
  suppress stretch and therefore retain an indefinite descendant percentage
  basis even when their content-derived used height matches the line size.
  Conversely, a percentage
  flex-basis that falls back to `auto` against an indefinite column main size
  does not gain a definite descendant percentage basis merely because replay
  freezes the item's final used height.
- Root/body overflow propagation now retains the propagated viewport clip when
  deciding whether descendants may fragment. This prevents an auto-height flex
  descendant with a definite max-clamped cross size from materializing extra
  pages after the document canvas has requested `overflow: hidden`.
- Flex item available-size and replay boundaries carry distinct physical
  content width/height quantities, while flex-line main/cross values remain
  axis-typed. This prevents a physical width from being silently reused as a
  logical inline size in orthogonal flex descendants. Split flex and grid item
  replay records likewise preserve used physical border-box dimensions as
  border-box quantities until their explicit layout boundary conversion.
- Flex item `stretch` min cross sizes resolve through CSS Sizing Level 4
  stretch-fit sizing when the flex line/container cross size is definite.
  Final flex item border boxes are floored by their padding and borders, so a
  zero stretched cross-size target cannot create a negative content box or
  clip away item decorations.
- Non-stretched auto cross-size flex items preserve their hypothetical
  cross-size during final line alignment, including shrinkwrapped anonymous
  text flex items. Column flex anonymous item replay scopes the placed item's
  own logical inline size, so inherited text alignment is applied inside the
  flex item rather than the flex container. Only stretched auto cross-size
  items relayout against the final flex line cross size for definite descendant
  sizing.
- Replaced flex items with an intrinsic aspect ratio transfer cross-axis
  min/max constraints into the content-basis candidate used by
  `flex-basis:auto`, while keeping main-axis min/max constraints out of the
  flex base size. Raster image natural dimensions are converted from source
  image pixels to CSS px and then to Quire layout points before they contribute
  to flex max-content main-size calculations, including vertical-writing row
  flex items.
- Automatic flex minimums use a preferred aspect ratio to transfer a definite
  cross-axis minimum even when the preferred cross size is automatic; that
  transfer remains separate from flex-base sizing. CSS Overflow's paired
  computed-value rule is applied after the cascade, so a scrollable or clipped
  axis makes the other axis's `visible`/`clip` value compute to `auto`/`hidden`
  before Flexbox selects its automatic minimum.
- Intrinsic flex item cross contributions apply definite preferred widths and
  heights through their used min/max constraints before a shrink-to-fit flex
  container consumes them. This keeps a replaced column flex item with
  `width` plus `max-width` from exporting the unconstrained preferred width
  after final flex layout has correctly selected the clamped cross size.
- Canvas, image-backed, and inline SVG share one replaced-element geometry
  path across flex, inline, block, grid, and table layout. Their content,
  border, and margin boxes remain distinct through flex sizing and replay, so
  padding and borders contribute exactly once to cross sizing and inline
  fallback references.
- Resource-less video and iframe elements now remain atomic replaced boxes for
  flex sizing and replay even when Quire has no frame/embed painter. Their
  unavailable content is transparent while author backgrounds and borders
  still paint; iframe fallback dimensions deliberately carry no preferred
  aspect ratio. Ratio-only inline SVG roots likewise use CSS's 300px default
  object width for sizing and are normalized to their final CSS viewport before
  percentage SVG geometry is painted.
- Floated flex containers resolve their clearance height from flex-line
  intrinsic sizing after their shrink-to-fit width is known, rather than from
  an ordinary block-child sum. This preserves one-line row height for later
  `clear` placement and downstream float exclusions. A narrow snapshot replay
  supplies the real line height for otherwise zero-height atomic-inline
  floats, so their overflow and clearance geometry remains materialized.
- When a content-based flex basis runs along an item's logical inline axis,
  Quire measures hypothetical cross-size line boxes at that resolved
  max-content base rather than at a narrower available container width. This
  avoids introducing soft wraps before the flex algorithm establishes the
  item's main size. Float-only block items re-enter block formatting with the
  resolved border-box containing span, rather than treating their resolved
  content width as a parent span and subtracting their own decoration twice.
  Once flexible lengths are known, automatic cross-size remeasurement carries
  a typed definite `PostFlexingMainSize` basis on either physical main axis;
  this also covers orthogonal items in physical column flex layout.
- Wrapped column flex items with automatic, non-stretched cross sizes retain
  their CSS fit-content used width after line-aware descendant measurement.
  Their intrinsic min/max-content contributions remain available for later
  sizing, but cannot overwrite the definite container-cross-size constraint
  during final replay or line packing.
- Replaced external SVG flex items retain a `viewBox` preferred aspect ratio
  even when the SVG declares an external DOCTYPE. This lets a definite
  cross-size suggestion participate in the replaced item’s automatic main
  minimum rather than preserving the generic 300×150 fallback object size.
- Ratio-only inline SVG roots use their available flex inline space, after
  margins, as the automatic size contribution; their `viewBox` ratio supplies
  the opposite axis. This keeps the generic default object dimensions from
  becoming an erroneous flex base size.
- Authored `aspect-ratio` values on non-replaced flex items participate in
  Quire's flex base-size and automatic minimum main-size calculations, including
  content-box transferred flex bases that Taffy expands through item padding
  and borders according to `box-sizing`. The flex/Taffy adapter carries those
  transferred sizes through semantic content-box and non-content typed lengths
  until the final Taffy scalar boundary. The adapter withholds Taffy's
  generic `aspect_ratio` field when the item's cross-size property is definite
  or resolved by stretch, so ratio transfer affects the flex base size without
  expanding the final cross-axis geometry.
- Raster image flex items resolve CSS Sizing `aspect-ratio` fallback before
  flex sizing, so bare authored ratios override the natural image ratio while
  `auto <ratio>` keeps the natural ratio when present. Auto cross sizes that
  depend on the final flexed main size are corrected before flex line metadata
  and replay are finalized.
- Percentage `flex-basis` values fall back to content sizing when the flex
  container's main size is indefinite, including authored `0%`; this content
  basis ignores the item's authored main-size property as required by the flex
  base-size algorithm.
- Flex item estimates map inline-content logical inline/block contributions
  through the item's writing mode, so vertical-writing row flex items with
  forced inline breaks expose the correct physical main and cross sizes.
- Element flex item estimates use the same logical-to-physical mapping for
  simple vertical-writing inline content, so inherited `vertical-rl`
  `inline-flex` column wrapping uses line-height-sized physical item widths
  and keeps definite logical inline sizes as definite physical heights.
- Flex layout now records explicit per-line metadata for item source ranges,
  main/cross extents, first/last baseline candidates, and collapsed-item
  struts. `FlexFragmentPlan` now carries page-local boundary fragments, line
  ranges, original and page-local item bounds, content/decoration slices, and
  fragment metadata for block flex replay.
- Normal-flow block flex containers can now fragment at flex boundaries in
  paged media: row containers break by flex line, column containers break by
  item progression, and forced item `break-before`/`break-after` values are
  consumed at the flex-container layer. Forced `break-after` propagation now
  takes the shared adjacent-box break context and target-aware forced-break
  carry state also used by table pagination, so a forced item break is either
  the next flex unit's pending break or the outgoing flex-container break when
  the item sequence ends. Flex page break units consume `break-inside` and
  carried `break-after` avoid values through the active fragmentainer kind,
  preserving the same page/column target split used by the shared CSS Break
  classifier. Flex break-unit construction now receives the active
  fragmentainer kind before aggregating line/item `break-before`,
  `break-after`, and `break-inside`, and flex line/item break-value combining
  uses the shared target-aware break combiner for forced and avoided breaks.
  Flex pre-unit overflow/avoid decisions now receive that same active
  fragmentainer kind before asking whether the committed break opportunity is
  avoided. Flex container wrapper `break-before`/`break-after` page
  transitions use the shared standalone box break context and
  `FragmentainerKind` page-cursor materialization gate before falling back to
  any outgoing forced item break.
  Flex fragment advances for forced item breaks, pre-unit overflow/avoid moves,
  and oversized-item slice continuation now flow through committed flex
  transition decisions that own the active fragmentainer kind and next source
  block offset before replay materializes any target-specific cursor mutation.
  Forced item transitions use the same shared fragmentainer materialization
  path as overflow/avoid and slice continuation transitions. In a multicolumn
  context that path advances the temporary column page selected by
  `FragmentainerOverride`, so each committed flex source slice has a concrete
  destination fragmentainer rather than metadata-only column transitions.
  Pre-unit avoid checks now
  consume the shared target-aware
  fragmentation break-opportunity model also used by grid row-boundary
  planning, while pre-unit overflow checks consume the shared fragmentainer
  capacity value built from cursor bounds and also used underneath table and
  grid fragmentation. Flex container definite/available-height setup also
  reserves block-axis margins through that shared fragmentainer capacity
  primitive.
  Oversized flex units choose a fragment-local source-slice decision before
  painting from the shared source-slice primitive also used by table row
  fragmentation; this includes the progress transition used when the current
  fragmentainer has no remaining block-size.
  Flex pre-unit overflow decisions use the shared fragment advance gate before
  constructing the flex-local fragment-transition metadata; `avoid` restricts
  a selected break after overflow rather than creating a transition by itself.
  Oversized row-line and column-item fragments split into page-local item
  slices and replay split visual content from the source item layout with
  page-local clipping. Fragmented block flex containers clone simple container
  backgrounds onto page-local fragments; empty flex containers now contribute
  a fragmentable source range, whose first and final slices carry the
  appropriate padding/border decoration, so its decoration can continue
  instead of disappearing. Fragment planning also carries a non-size-contained item's
  measured overflowing descendant block extent separately from its used flex
  border box, so that overflow can continue into later fragmentainers.
  Document-canvas flex boxes and floated/atomic replay contexts continue to
  use their existing specialized pagination behavior.
- `visibility: collapse` now uses a visible-layout probe to measure collapsed
  item struts and source-line placement, then relayouts with collapsed items
  omitted from main-axis distribution. Wrapped lines are repacked when a strut
  expands an affected line, including `wrap-reverse` line packing. Row- and
  column-axis collapsed replaced flex items preserve their cross-size struts
  without painting their images or consuming main-axis space, and
  vertical-writing row flex items preserve the same physical cross-size strut
  behavior.
- Wrapped row flex containers apply Quire post-Taffy baseline passes for
  `align-content: baseline` and `align-content: last baseline`, using recorded
  first/last line baseline sets and content baselines synthesized from flex
  item estimates when needed. Horizontal writing modes align physical y
  baselines, and vertical-writing CSS row containers align physical x baselines
  from horizontal baseline estimates that use the selected inline line-box
  baseline: central for `text-orientation:mixed`/`upright` and alphabetic for
  `text-orientation:sideways`. Missing row flex item
  baselines synthesize from flex border edges using the CSS Align alphabetic
  line-under baseline for sideways/horizontal typographic mode and the central
  baseline for vertical `text-orientation:mixed`/`upright` baseline-sharing
  groups. Wrapped row `align-items: baseline` self-alignment resolves each
  line's used cross size before final slot packing, then replaces Taffy's
  synthesized baselines with measured item baselines without rebuilding those
  slots. `wrap-reverse` reverses line stacking only; first/last baseline
  alignment inside each line remains on the ordinary cross-axis edges. Taffy receives a cross-start
  placeholder for first- and last-baseline alignment, so its missing baseline
  channel cannot alter a specified cross size when cross-axis margins are
  negative. Row flex item baseline estimates
  also walk normal-flow block
  descendants, so a block flex item can export the first/last baseline of its
  in-flow block content rather than falling back to a synthesized border edge.
  Empty `inline-flex` row containers do not export a flex baseline; their
  parent inline formatting context synthesizes the atom baseline from the
  margin box, so padding, borders, and margins align with adjacent text as
  CSS Inline requires.
  Baseline sharing groups only include flex items whose inline axis is
  parallel to the container main axis; orthogonal baseline-aligned items fall
  back through CSS Align first/last-baseline self-alignment. Baseline fallback
  paths preserve a `wrap-reverse` column line's packed cross-end slot and use CSS
  Align logical start/end sides through writing-mode-aware side mapping,
  including column flex content-alignment and column-axis self-alignment
  fallback when compatible baseline sharing cannot apply. Column flex
  containers share first and last vertical text baselines for vertical-writing
  items in the column cross axis.
  Row-axis nested flex containers export first and last baselines from their
  intrinsic estimates so they can join parent flex baseline-sharing groups; definite
  wrapped `row`, `row-reverse`, and `wrap-reverse` nested flex containers form
  estimated flex lines so their exported first and last baselines come from the
  first and last wrapped lines, using each line's startmost/endmost item and
  the correct reversed cross-axis line offsets. Auto-width nested row flex
  containers constrained by definite `max-width` or `fit-content(<length>)`
  also use that constraint when estimating wrapped exported baselines.
  `width:max-content` row flex containers include main-axis gap lengths and
  definite flex-basis contributions in their intrinsic main size, so parent
  baseline alignment observes the same line breaks as the nested flex layout.
  Wrapped flex lines resolve their cross-axis percentage gutter once from the
  definite flex content box before `align-content: normal`/`stretch` sizes
  and places line slots. This covers `gap-009-ltr.html` without turning a
  genuinely cyclic percentage gap into a definite length.
  An automatic physical-column `inline-flex` preserves the resolved main-axis
  gap between every adjacent final line member while retaining normal-flow
  line-box leading. This covers the fixed-gap `gap-004-{lr,rl}` and
  `gap-005-{ltr,rtl}` WPT cases without treating visual item overflow as the
  used line span. Final flex-item baseline coordinates use their placed
  border-box origins, so fixed item margins do not shift equivalent
  `vertical-rl` inline-flex gap layouts; this covers `gap-005-rl.html` and
  wrapped `gap-006-rl.html`.
  Vertical-writing nested row flex containers export physical x-axis
  first/last baselines for parent row flex baseline-sharing groups, including
  `wrap-reverse` line packing, when their wrapped line estimates have either a
  definite physical cross size or an auto cross size resolved from the wrapped
  line stack, and when percentage physical cross sizes resolve against a
  definite parent cross size or behave as auto because the parent cross size is
  indefinite. Overflowed `wrap-reverse` line estimates can export negative
  cross-start baselines so last-baseline alignment can target lines that fall
  outside the nested flex border box.
- Existing smoke coverage includes row/column and reverse directions, wrapping,
  `order`, `flex-grow`/`flex-shrink`, intrinsic `flex-basis` keywords,
  percentage and mixed math sizing, gaps, auto margins, baseline alignment,
  `self-start`/`self-end`, safe alignment fallback, absolute flex children,
  generated `::before`/`::after` flex items including order sorting,
  `display: contents` child flattening including mixed inline/block
  descendants exposed as flex items, inline-flex baselines including empty
  margin-box synthesis, replaced images, single-subject distributed
  `justify-content` fallback, post-minimum
  main-axis repacking, vertical-writing row flex items with forced inline
  breaks, and several nested flex/table cases.
- Generated pseudo flex items retain their frozen tree-abiding child content
  through item replay, so `::before` and `::after` contribute as ordinary
  order-sorted flex items rather than empty anonymous placeholders.
- The Taffy adapter maps vertical-writing row flex containers that use Taffy's
  physical column axis onto the correct horizontal cross-start side. This
  covers `vertical-rl` row wrapping, `wrap-reverse`, single-line cross-axis
  `flex-start` alignment, child writing-mode-independent explicit item sizes,
  avoids letting `direction` flip vertical-writing column block-axis flow, and
  covers `vertical-rl; direction:rtl` wrapped column line stacking.

## Remaining Gaps

- Paged flex fragmentation is partial. Boundary breaks between row lines and
  column items work for normal-flow block flex containers, and oversized
  row-line/column-item fragments now produce page-local item slices with visual
  content continuation. Split row and wrapped-column item replay translates
  its consumed source offset before clipping, so early descendant paint is not
  duplicated in a later fragmentainer. Automatic single-line column containers
  now extend the boundary-crossing item through the final continuation span and
  shift following main-axis items. A definite-height single-line column box
  also preserves its used height while extending an overflowing item's final
  continuation. A forced break between flex items extends the preceding
  container fragment through the remaining fragmentainer space. Wrapped-column
  boxes whose resolved layout has one line share those continuation rules;
  general multi-line and nested column continuations still need complete
  fragment-local main-size re-layout. At an exhausted multicolumn boundary,
  flex now lets its own fragment plan advance the container rather than
  recursively prebreaking it as a whole. Positioned flex descendants measured
  against temporary multicolumn pages now retain their final flex static
  rectangle and replay after the enclosing real containing block is restored.
  Wrapped-column fragmentation partitions overlapping item ranges into shared
  vertical intervals rather than serializing cross-axis flex lines. Flex line
  reconstruction also keeps a zero-width hypothetical stretched item with its
  source line, so the later cross-size pass stretches every item in that line.
  The latest full local `css/css-break/flexbox/` run passes 178 of 327
  reftests. The most recently measured focused `css/css-flexbox/flexbox/`
  run passes 325 of 427 tests. A final cross-size replay now reruns a
  single-line auto-height row flex container when a binding `min-height` or
  `max-height` establishes its used cross size, and fragment painting keeps
  that final used border box distinct from descendant source overflow. When the next
  whole flex line advances to a new fragmentainer, the preceding flex-box
  fragment now paints its own background and border through that boundary,
  including the source gap before the next line.
  The `single-line-row-flex-fragmentation-047.html` WPT now completes within
  the renderer timeout; its visual comparison remains unclassified.
  Fixed-height single-line row containers retain their declared source block
  extent instead of being extended to fill the final continuation
  fragmentainer; only auto-height row containers perform that continuation
  stretch.
  In orthogonal writing modes, a block-level flex container's automatic
  logical inline fill enters the physical Flex adapter as a numeric physical
  height while retaining an indefinite CSS percentage basis. This preserves
  the final background fragment without incorrectly resolving cyclic
  percentage gaps against that automatic used size. Its automatic physical
  width likewise remains an indefinite percentage basis when that width is
  the orthogonal logical block axis, while still constraining the physical
  solver; the final intrinsic main-size pass can then resolve a cyclic gap
  without recursive flex estimation.
  Column-targeted `break-inside: avoid` retries retain their temporary column
  fragmentainer context instead of rebuilding a document-page context. Direct
  positioned multicolumn descendants with definite insets remain anchored to
  the principal multicol containing block, while auto-inset descendants retain
  their final source-column static position. Physical-column flex descendants
  now also distinguish candidate-local static-position geometry from
  source-global definite block insets before their positioned layers are
  projected through a committed column fragment. This shares the committed
  positioned-fragment replay path with flex descendants.
  A shortened first multicolumn fragmentainer is now used only for the first
  column; all continuation columns use the nominal column height, preserving
  flex source slices across class-A avoided boundaries.
  Synthetic multicolumn fragmentainers now preserve their local containing
  block insets across a continuation instead of subtracting document-canvas
  insets a second time, which had shifted later column fragments horizontally.
  An automatic column flexbox now carries the final fragmentainer span of an
  auto-sized item that has fragmented, including trailing empty flex space
  after a later whole-item prebreak.
  Split flex-item replay now uses a zero-inset off-page page context, so nested
  multicolumn fragments retain their local origin when translated back to the
  live page. Positioned layers that escape a split flex item now retain their
  source-page inline coordinate during replay; only their off-page block slice
  is translated, avoiding a duplicate item-inline offset in multicolumns.
  Automatic
  single-line row flex containers that end at a multicolumn boundary now enter
  the fragment plan when their item subtree has no independently forced break.
  A materialized flex-fragment record now owns its source interval,
  destination border box, container-decoration ownership, item-local border
  boxes, continuation state, and local-to-page translation; normal and split
  item replay consume that record rather than independently reconstructing
  page-local item geometry. Each committed fragment also retains explicit
  per-line source intersections and visible item membership, so overlapping
  wrapped-column lines are not collapsed into one enclosing source range. A sole flex line/item at an exhausted
  fragmentainer now advances before replaying its source slice. Orthogonal
  multicolumn continuation pages retain their complete committed destination
  rectangle, preventing their cross-axis overflow from leaking past the
  container after page-local translation. Direct and flex-positioned
  descendants now both replay through the committed multicol fragment record,
  including its containing-block scope, translation, and optional clip.
  Physical-column flex static intervals now replay separately through every
  intersected committed source fragment before their layers are translated and
  clipped in the final multicolumn destination. A physical-row descendant with
  a resolved block-start inset beyond the first committed source fragment now
  retains candidate multicolumn fragments and replays only through its
  final-inset owner; auto insets and the remaining nested containing-block
  cases remain deferred.
  A physical-column descendant with a definite block-start inset now advances
  its source-global positioned layer by each committed source slice start
  during destination projection; this preserves both portions of an inset box
  that spans wrapped multicolumn fragments.
  Every materialized flex-item continuation now records its selected source
  interval, frozen Flexbox border-box geometry, and replay mode. Descendant
  overflow replays from that committed source slice without expanding a
  definite-height item, while independently fragmenting children retain a
  child-fragment ordinal. Forced descendant breaks are no longer suppressed by
  otherwise-unsplit item replay: their committed page span supplies the outer
  flex continuation edge and destination-page cursor. The item's child
  endpoint is captured before frozen Flexbox geometry is reapplied, so the
  following sibling inherits the same final-page flow context as the ordinary
  block-fragmentation path.
  Nested flex measurement now retains a separate fragmentable descendant-overflow
  extent instead of overloading the normal intrinsic content height.
  Column flex replay also preserves normal items' final border-box main-size
  constraints, avoiding an erroneous padding/border-sized gap. Fragmented row
  flex items retain fragment-local stretched cross sizes and replay positioned
  descendants against the flex container's fragment-local containing block.
  Table-item replay now retains the committed table fragments, including
  repeated headers and footers, rather than rerunning the table from its first
  row on every outer flex continuation. The remaining matrix shows that
  complex multi-line and column fragment-local relayout, padding/border decoration
  slicing, and
  positioned-descendant containing blocks inside nested multicol fragmentainers
  still need work. Complete fragment-plan metadata for links, running
  elements, named pages, and other PDF side effects also remains incomplete.
- Baseline handling now covers horizontal and vertical-writing row
  `align-content` baseline packing. Baseline fallback alignment is
  writing-mode aware for the covered self- and content-alignment cases,
  including column flex content-alignment fallback, and nested vertical-writing
  row flex containers export text-orientation-aware horizontal baselines for
  definite wrapped rows, auto-width wrapped rows, definite-parent percentage
  wrapped rows, indefinite-parent percentage wrapped rows, `wrap-reverse`, and
  first- and last-baseline `wrap-reverse` exports with indefinite percentage
  cross sizes.
  Row flex items with normal-flow block descendants export descendant text
  baselines for first/last baseline self-alignment. Column-axis baseline
  sharing covers first and last vertical text baselines for vertical-writing
  flex items. Both intrinsic estimation and post-Taffy reconciliation select
  first/last baseline contributors from the resolved physical main direction,
  so vertical rows with reversed inline progression no longer select an item
  from the authored logical direction. Rarer nested baseline edge cases still
  need more work.
- Intrinsic flex container sizing now has a dedicated contribution pipeline,
  but still needs WPT-backed auditing for the unresolved web-compatible
  algorithm, deeply nested flex/table descendants, complex block-child
  multicolumn descendants, rare
  orthogonal-flow combinations, and exact child block-size estimates for
  descendant formatting contexts beyond the covered wrapped-column float cases.
  Orthogonal descendants still need a complete logical block-axis contribution
  projection: the current empty-inline fallback preserves definite descendant
  extents, but it is not a substitute for recursively projecting mixed
  writing-mode block stacks.
- Flex layout still needs a WPT-backed audit for generated content inside
  anonymous flex text items, absolute static-position edge cases, and remaining
  reverse/wrap-reverse combinations outside the covered vertical-writing
  adapter tests.

## Test Backlog

- Import local WPT cases by behavior group: line breaking, flexible lengths,
  intrinsic sizing, auto minimums, alignment, collapsed items, writing modes,
  absolute children, and fragmentation.
- Port high-value local WeasyPrint tests from
  `WeasyPrint/tests/layout/test_flex.py`, especially column pagination,
  page-break interactions, replaced flex items, auto margins, and nested flex.
- Existing local conformance smoke coverage in
  `tests/smoke/flex_conformance.rs` and `tests/smoke/layout_inline_flex.rs`
  covers collapsed-item struts, flex intrinsic min/max-content widths,
  intrinsic percentage-gap behavior, baseline alignment, row-axis baseline content
  alignment, definite wrapped nested `row`/`row-reverse`/`wrap-reverse`
  baseline exports, max-constrained auto-width nested baseline exports,
  fit-content-constrained nested baseline exports, definite-parent percentage
  and indefinite-parent percentage nested vertical baseline exports, including
  `wrap-reverse` overflowed first/last baseline exports, column flex content
  alignment fallback, column-axis first/last vertical text baseline sharing,
  baseline fallback in vertical and `wrap-reverse` cases,
  vertical-writing row flex item line-under baseline synthesis,
  wrapped row `align-items: baseline` line-slot preservation,
  absolutely positioned flex static positions including vertical-lr/vertical-rl
  auto-position content-box coverage, `display: contents` flattening
  including WPT-shaped mixed inline/block flex items, generated `::before` and
  `::after` flex item generation/order sorting, `order`-modified paint order
  with overlapping negative-margin flex items, wrapped-line repacking after
  collapsed struts, row line fragmentation, column item fragmentation,
  oversized flex item slices, split-item visual continuation, and forced flex
  item breaks, including wrapped column flex min-content cross-size aggregation
  and max-content available cross-size propagation for percentage-width
  float-only flex items, plus stretched vertical flex-item descendant
  percentage padding, non-negative border-box sizing for `min-height: stretch`
  flex items, definite-cross-size raster image flex items using flex-resolved
  main sizes, and stretched column flex items whose aspect-ratio transferred
  main size is clamped by content auto-minimum sizing. Add
  Quire-specific regression tests for full PDF fragment
  metadata, running elements and links inside fragmented flex items, full
  cloned decorations, and Taffy adapter edge cases as fragmentation is
  completed. Current flex sizing coverage includes auto-height row containers
  with min/max-height constraints.

## Implementation Notes

- Keep Taffy as the primary algorithm unless a specific required CSS behavior
  cannot be represented through its public model.
- Prefer adding explicit adapter metadata, such as flex-line and baseline
  records, over re-inferring layout structure at each downstream use site.
- Update `SPEC_DIVERGENCES.md` whenever a remaining gap is implemented or
  narrowed.

# CSS Floats Parity

Last updated: 2026-08-15

CSS 2.2 is the conformance target for float placement, exclusion, and
clearance. WeasyPrint is used as a compatibility reference for paged-output
behavior where the specs leave implementation details ambiguous.

## Current Level

- `float` and `clear` parse the physical CSS 2.2 values plus logical
  `inline-start` and `inline-end`.
- Floated children are blockified, auto-width floats use shrink-to-fit sizing,
  intrinsic `width` keywords resolve from the float formatting context's
  min/max-content sizes, percentage and specified float widths are replayed
  from their resolved used sizes, and float margin boxes are recorded as
  page-local exclusions for later flow.
- Used-style replay resolves definite font-relative box lengths before
  intrinsic measurement and final layout, so a float descendant such as
  `width: 2em` cannot be widened by its overflowing max-content text.
- A float following an adjoining normal-flow block inherits that block's
  pending block-end margin in its hypothetical block-start position; this is
  additive, including negative margins, because the float itself is outside
  the preceding margin-collapse set.
- Empty auto-height floats record zero-height margin-box exclusions, so they
  preserve source-order float placement without shortening same-top line boxes.
- Auto-height float exclusions and page-prebreak decisions use an isolated
  replay of the final blockified flow-root style with its frozen used width,
  rather than a descendant-height estimate that can diverge from final layout.
- The isolated auto-height replay cache uses a typed key carrying the element,
  frozen used inline sizes, percentage-basis state, generated counter/quote
  state, and source page identity. Active measurement state is kept outside
  restored snapshots, so nested auto-height floats use a finite estimator
  instead of recursively starting an unbounded replay tree.
- Shrink-to-fit preferred width accounts for floats that share an inline run
  with following inline content, so the unconstrained line width includes the
  same-line float margin boxes plus the inline max-content contribution.
- Floats with no visible paint, including `visibility:hidden` placeholders,
  still record margin-box exclusions, so consecutive same-line floats reserve
  reference-grid cells before later floats are placed.
- Line boxes and normal-flow horizontal block formatting context roots avoid
  active floats through the shared float collision model. Table wrappers,
  flex/grid roots, replaced boxes, and orthogonal flows still need broader
  parity coverage.
- In-flow block fragments produced by block-in-inline splitting query active
  float exclusions in their relatively positioned inline ancestor's visual
  coordinate space. Their parent normal-flow cursor and interposed float's
  static placement remain unshifted.
- Floats inside self-collapsing blocks remain part of the open adjoining
  margin set, so a following BFC root that fits beside the float uses the
  resolved collapsed start margin for both float placement and the parent's
  painted border box.
- When a following BFC root cannot fit beside floats that would otherwise be
  adjoining, the BFC separates from those floats and its start margin is
  consumed as a clearance-style boundary. This prevents the collapsed margin
  from replaying the floats downward while preserving the BFC's normal-flow
  border position.
- BFC avoidance also probes the pre-margin block edge. When a positive start
  margin crosses a float that a full-width BFC root cannot occupy beside, the
  used margin is reduced to the float-clearance distance instead of skipping
  the collision after the border box has already moved below the float.
- Horizontal normal-flow block formatting context roots re-resolve their
  auto inline size and estimated auto block size against narrowed float bands
  before accepting placement, so internal floats can force a narrower same-top
  retry instead of exposing overlapped background.
- Horizontal normal-flow block formatting context roots use their border box,
  not their own margin box, for float-adjacent collision. Auto-width roots can
  narrow to the float-free band while horizontal margins remain outside the
  collision box.
- Final BFC placement preserves the original CSS containing span and
  percentage bases for width and margin resolution. The residual float band is
  used only for collision testing and auto-width measurement; the resolved
  border-box origin is then applied once, including RTL, negative margins,
  nested BFCs, and auto-width retries.
- Negative physical margins retain their normal-flow border-box origin during
  BFC fixed-point avoidance. A root that is disjoint from a float may extend
  beyond its containing inline span through that margin instead of being
  incorrectly moved below the float.
- Float-stacking placement and normal-flow BFC avoidance retain distinct
  hypothetical block positions for horizontal block formatting-context roots.
- Auto-layout table wrappers use the complete active float band for their
  shrink-to-fit input, and a cell's declared `width` contributes a preferred
  track size rather than incorrectly becoming its min-content floor.
- Independent block formatting contexts isolate their internal floats from
  parent flow. Auto-height block formatting context roots and inline-block
  formatting contexts now expand to include floats that belong to their own
  context.
- Overflow-clipped normal-flow BFC roots clip descendant contents while
  preserving their own background, border, and outline outside the overflow
  clip edge.
- Fragmented floats register page-local exclusion shapes, and following text
  reflows around continued float fragments on later pages.
- Definite descendants of fragmented floats take the ordinary class-A
  prebreak before their positioned descendants are laid out. The enclosing
  float retains those layers until fragment-local stacking contexts are
  assembled, so a relative containing block that starts on a continuation page
  cannot leave an absolute child on its empty source fragment.
- A terminal deferred float fragment materializes and commits its destination
  page even when no later normal-flow content reaches that page.
- Floats that are prebroken to the next page are placed from the new
  fragmentainer cursor, so `break-inside: avoid` floated blocks can stay
  together when they fit on an empty page.
- Fragmented float exclusions keep source and fragment identity internally, so
  clearance and replay order can distinguish same-source continuations from
  unrelated same-page floats.
- `clear` accounts for active same-page fragments of a broken float, including
  float fragments continued from earlier fragmentainers.
- An empty tree-abiding generated block with `clear` remains a normal-flow
  clearance boundary rather than collapsing through its parent. This lets the
  common `::after { content: ""; clear: both }` clearfix include preceding
  float rows in the parent's automatic height and background.
- Clearance keeps the `clear:none` hypothetical border edge distinct from the
  used post-margin border edge. An ordinary post-float hypothetical edge does
  not select that float, while a first in-flow cleared child that would adjoin
  its parent start margin queries the counterfactual parent-start edge. The
  latter can produce negative clearance when its actual top margin extends
  below the float, without extending the parent background.
- A cleared parent resolves its complete adjoining first-child start-margin
  set before calculating clearance, then consumes that descendant contribution
  during child layout so negative child margins are neither lost nor applied a
  second time.
- Float painting preserves per-fragment opacity, transform, and overflow clip
  effects.
- Bookmarks, anchors, named strings, and running elements produced while laying
  out fragmented float fragments survive snapshot restore and are replayed in
  page/source order.
- Generated pseudo text produced inside fragmented floats survives
  target-text/page-margin replay through the float side-effect capture path.
- Inline floats are preserved as zero-width graph markers during line
  selection. A marker reached at the start of a selected line is placed before
  following text is selected. A marker reached after preceding inline text is
  placed on that current line when its signed CSS2 outer margin extent fits the
  remaining band; suffix text is then reselected against the final,
  non-negative float-exclusion band. A negative end margin can therefore paint
  beyond the band while preserving its aligned outer margin edge and leaving
  the suffix on the same line. If an oversized tentative placement cannot keep
  visible suffix content on that completed line, the placement is rolled back
  so the marker defers with the suffix to the next line. Prefix text keeps its
  original position.
- Equal-width line break candidates advance over zero-width inline float
  markers, so an inline-block prefix does not force a following fitting
  right float onto the next line.
- Inline float markers inside unbreakable `white-space: nowrap` and `pre`
  lines use a source-order transaction rather than a synthetic CSS Text break.
  The transaction preserves the selected source range and real break metadata,
  line slab, physical placement floor, and committed exclusion. A right float
  that cannot fit after its prefix uses the earliest legal later float row
  without splitting or soft-wrapping the source line. This covers the current
  `float-nowrap-1.html` and `float-nowrap-hyphen-rewind-1.html` cases; broader
  fragmented and nested inline-float replay remains listed in
  `SPEC_DIVERGENCES.md`.
  Collapsible whitespace looks through those zero-width out-of-flow markers,
  so adjacent spaces collapse as if the float were not part of the inline
  text run.
- `clear` progresses across continued float fragments, applying pending
  page-local exclusions until the final matching continuation is cleared.
- Inline and block clearance use the exact cleared float edge for used
  geometry; comparison tolerance never adds visible clearance. A fragmented
  float's deferred continuation paint is marked as a parallel flow, so it
  cannot by itself extend an ordinary ancestor's background or border into
  later columns. Auto-height BFC float containment remains a separate
  used-height calculation.
- HTML `br` line breaks generated by `br::before` preserve the originating
  `br` element's computed `clear`, so inline `br { clear: both }` can clear
  preceding floats without creating an extra empty line when the break itself
  has no inline content. The same clear metadata is preserved through collected
  inline line sequences used by text-box trimming and fragmentation planning.
- Logical `float`/`clear` sides are stored internally as used physical
  exclusion sides. Horizontal writing keeps CSS 2.2 left/right behavior, while
  vertical `inline-start`/`inline-end` clear matching resolves to top/bottom
  rather than aliasing physical left/right.
- Float replay retains a page-local logical placement record alongside its
  projected margin rectangle. This keeps writing-mode/direction, logical
  inline and block spans, and fragment identity available to exclusion and
  replay code without reassembling independent physical x/y offsets.
- Vertical logical floats now expose top/bottom exclusion bands to vertical
  inline line selection, paint-time line adjustment, and basic BFC-root
  placement.
- Vertical inline line selection advances float-band queries through each
  physical block-axis slab. Source-order logical floats therefore move with
  their line in `vertical-rl`, `vertical-lr`, and sideways writing modes,
  rather than repeatedly excluding every later column from the first slab.
- Vertical BFC roots, table wrappers, flex containers, and orthogonal
  formatting-context roots move to the next physical block-axis slab when a
  top/bottom logical float leaves too little inline span in the current slab.
- Generated image content inside fragmented floats survives float replay along
  with generated text and page-scoped metadata.
- Nested multicolumn float estimates retain the float's source block offset,
  and committed float paint is projected over every required column slice
  rather than being clipped to a speculative occupied-column count.
- Float paint capture reserves the float's document source order before its
  isolated descendant replay, so later sibling floats retain their Appendix E
  ordering when captured paint contexts are committed.

## Needed for Parity

- Broaden WPT and WeasyPrint comparison coverage for nested formatting contexts,
  especially combinations of floats with tables, flex items, generated content,
  replaced descendants, and fragmented layout.
- Broaden coverage for page-scope metadata and paint effects inside fragmented
  floats across page boundaries.
- Continue reducing legacy float-row call sites as more inline float placement
  cases are migrated onto the shared collision model.
- Audit table wrappers, flex/grid roots, and block-level replaced boxes for the
  same border-box-versus-margin-box float-adjacent placement rule now covered
  for normal-flow block formatting context roots.
- Complete zero-width float exclusion semantics, including their interaction
  with negative BFC margins and `clear`.
- Complete split-flow inline continuation replay. A float normalized from an
  inline ancestor must still make its source line and its following inline
  continuation participate in the same float-excluded line-selection retry.

## CSS2 Float Benchmark

On 2026-07-24, `css/CSS2/floats/` ran **62/67 passing (92.5%)**, with
**5 raster failures**, **0 timeouts**, and **6.98s** total Quire render time.
This improves on the 49/67 baseline and remains below its 9.15s render time.
The former auto-height crashtest timeout completes without timing out. The
remaining failures are `float-no-content-beside-001.html`, the two nowrap
cases, `floats-line-wrap-shifted-001.html`, and
`zero-width-floats-positioning.tentative.html`. A fresh targeted run on
2026-08-02 confirms that its unbreakable lines select the first full-width
slab below the float; the reftest still differs in the separate explicit-break
and `clear: both` paragraph-flow replay.

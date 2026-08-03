# CSS Sizing Parity

This note tracks behavior from CSS Sizing where intrinsic sizes consume
computed CSS lengths, percentages, and sizing keywords.

## Current Support

- Intrinsic `min-content` and `max-content` inline-size calculations preserve
  fixed length components inside cyclic percentage margin and padding values.
  For example, `calc(0% + 30px)` contributes the fixed `30px` component when
  the percentage basis is zero during intrinsic sizing.
- Block child, inline atomic, flex intrinsic, grid intrinsic-probe, and table
  cell intrinsic contribution paths resolve cyclic percentage margin and
  padding edges against zero from computed values, instead of reusing cached
  used edges from a concrete containing block.
- Normal block containers resolve intrinsic `width` keywords through typed
  logical inline/block content-size contributions before projecting to physical
  width. This keeps `writing-mode: vertical-lr` and `vertical-rl`
  `width:min-content`/`width:max-content` on the logical block axis instead of
  reusing inline-size contributions.
- Normal block containers resolve intrinsic `height`, `min-height`, and
  `max-height` keywords against their laid-out content block size. Intrinsic
  block-axis constraints keep cyclic descendant percentage heights unresolved
  while measuring the content contribution, then use the resolved final height
  for painting and block extent.
- Normal-flow blocks transfer a definite content width or height through a
  non-replaced preferred `aspect-ratio` when the opposite preferred size is
  automatic. The shared transfer helper accounts for `box-sizing` before the
  formatting context applies its own min/max constraints.
- When a block-axis min/max constraint changes a ratio-derived automatic
  height, normal-flow block layout transfers that constrained result back to
  its dependent automatic width and re-runs the block-width equation. This
  preserves width constraints and auto margins while avoiding an unconstrained
  ratio-sized overflow.
- For ratio-derived automatic block dimensions, normal flow retains the
  content-based automatic minimum in the opposite axis. This uses the typed
  block intrinsic contribution rather than a formatting-context-specific
  child-size approximation.
- Grid item Taffy styles and intrinsic measurement callbacks carry a
  non-replaced preferred `aspect-ratio`, allowing a known grid-area axis to
  supply the automatic opposite axis during basic track sizing.
- After Grid resolves an item's area, final item sizing distinguishes `normal`
  alignment from explicit `stretch` for non-replaced aspect-ratio boxes.
  Definite fixed rows also provide the post-track percentage basis needed for
  percentage heights and their ratio-dependent inline size.
- Flex intrinsic estimates export a ratio-transferred automatic width when a
  definite height supplies it. This keeps `flex-basis: content` and intrinsic
  flex-container sizing on the same preferred aspect-ratio contribution.
- For one intrinsic flex line with a definite main size, flex grow resolution
  now precedes aspect-ratio transfer into an automatic cross contribution.
  This covers inline-flex and replaced items whose final cross size depends on
  their resolved, rather than base, main size.
- `contain-intrinsic-size: <width> <height>` is parsed as typed physical
  fallback sizes. For size-contained normal blocks it supplies the used
  automatic block size and basic content-ignored intrinsic contributions.
- Size-contained replaced images retain those fallback dimensions in the
  shared replaced-box sizing path, including when exactly one CSS axis is
  definite. The fallback does not synthesize a natural aspect ratio.
- Absolutely positioned non-replaced boxes resolve an automatic axis from a
  definite authored opposite axis through `aspect-ratio` before their inset
  equations run. When both axes are automatic, a definite horizontal inset
  fill size is used first, then ratio and min/max constraints select the final
  dependent axis.
- When an absolutely positioned box's automatic block size is changed by a
  ratio-relevant min/max constraint, its automatic inline size is re-solved
  from that final block size. This applies the CSS Sizing size transfer after
  the absolute-position block equation, including size-contained fallback
  content.
- Normal-flow `min-width: stretch` and `max-width: stretch` constraints are
  resolved against the available margin box rather than dropped during the
  final block-width constraint pass.
- Float shrink-to-fit measurement propagates a definite float block-size basis
  while measuring descendants. Replaced `height: 100%` content can therefore
  contribute its ratio-transferred intrinsic width to an explicitly sized
  float.
- Layout-time percentage bases are represented with typed `PercentageBasis`
  values instead of raw scalar options on the block-size stack and in flex
  available-space, flex item sizing, flex minimum sizing, flex gap, and
  flex-basis Taffy adapter paths. Numeric available-size constraints remain
  separate when they are not definite CSS percentage bases.
- Used-value helpers now take typed percentage bases directly; the former
  optional-basis compatibility wrappers have been removed.
- Grid item sizing, intrinsic item height constraints, and the Grid Taffy
  bridge use typed physical percentage bases. `GridPhysicalAvailableSpace`
  projects these once into `LogicalInlinePercentageBasis`; the shared edge and
  Taffy-bridge APIs require that type for item margin/padding percentages.
  This preserves indefinite row sizing while preventing vertical-writing
  physical edge percentages from resolving against the wrong physical axis.
- Flex item edges, block-flow child edges, table-cell padding, inline atom
  edges, and in-flow replaced-element edges use the same logical-inline edge
  API. The generic layout-length helper remains only for older callers whose
  containing-block writing-mode projection has not yet been carried through
  their layout input.
- The remaining generic edge-basis callers are deliberately retained physical
  boundaries: Grid container setup (including its used-style/zoom boundary),
  Grid and replaced-element absolute positioning, table and multicol track
  geometry, page-margin layout, and legacy block-estimation APIs. Migrating
  any of these requires a typed physical available-space record at its public
  boundary; a local `f32` to logical-inline conversion would hide rather than
  solve the missing projection.
- Table wrapper target heights, row/row-group percentage height distribution,
  table-cell content scopes, and table-cell final relayout paths use typed
  block-size percentage bases.
- Canvas replaced-element sizing receives a typed block-size percentage basis,
  keeping percentage heights and min/max-height constraints indefinite unless a
  real containing-block basis is available.
- Intrinsic inline collection carries a distinct typed inline percentage basis
  to atomic canvas and raster/SVG image sizing. A line-width constraint used
  to measure fragments no longer makes a cyclic `width: <percentage>`
  definite; an independently definite percentage height can instead transfer
  through the concrete object's preferred aspect ratio.
- Absolutely positioned boxes expose a definite content-height basis to
  intrinsic inline-size measurement when their own height is definite from an
  explicit `height` or from non-auto `top` and `bottom`, allowing percentage
  heights on replaced descendants to transfer through intrinsic aspect ratios.
- Orthogonal absolutely positioned boxes with both physical horizontal insets
  preserve static top-edge placement while vertical replaced descendants use
  percentage sizing and intrinsic aspect ratios to determine the box's physical
  extent.

## Remaining Gaps

- See `SPEC_DIVERGENCES.md` for known incomplete intrinsic sizing behavior in
  grid, flex, table, and replaced-element layout.
- Continue migrating remaining absolute/fixed positioning and
  replaced/aspect-ratio sizing calculations beyond canvas where
  containing-block sizes are optional percentage bases rather than merely
  optional geometry.
- Continue propagating contain-intrinsic fallback sizes through parent
  max-content estimation, normal-flow writing-mode sizing, and flex/grid/table
  contributions. Logical contain-intrinsic-size longhands already map through
  the cascade to the corresponding physical fallback axis.
- Gap helpers outside the flex/grid Taffy adapters should be audited for typed
  bases when they begin resolving CSS percentages from optional container sizes.

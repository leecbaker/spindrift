# CSS Writing Modes parity

## Verified behavior

- Vertical text resolves each typographic unit once into either upright vertical
  composition or sideways horizontal composition. Font features, vertical
  metrics, and PDF placement consume that shared plan, so Mongolian and
  Phags-pa retain their intrinsic vertical presentation without combining
  sideways placement with OpenType vertical substitutions.

- Vertical paint placement uses preserved shaped-glyph source provenance rather
  than PDF-facing Unicode summaries. Glyphs without a standalone ToUnicode
  value remain in their resolved typographic unit with their used advance and
  vertical origin. A shaping range spanning several units uses vertical
  metrics and placement when every covered unit resolves to the same mode;
  mixed upright/sideways ranges remain conservative.

- Replayed table-part styles cross the normal used-length boundary. Font-relative
  box-model lengths such as `td { width: 1em }` are therefore finalized before
  table track sizing and structural background painting.
- An orthogonal horizontal block in a vertical containing block is placed from
  the containing block's logical block-start edge even when it has an explicit
  CSS `width`. Its physical horizontal border-box span is the parent vertical
  flow's logical block span.
- Orthogonal sizing and normal-flow placement use separate axis contexts. A
  child selects its physical extent and line measure in its own logical axes,
  while its normal-flow physical position and margins are resolved in the
  containing block's axes; a vertical child of a horizontal block therefore
  remains at the containing block's inline-start edge.
- Isolated float replay records a typed source and destination border-box
  origin. Bottom-origin vertical and sideways flows lay out in a stable
  page-top scratch space, then project the complete ordinary float subtree and
  any separately captured positioned layers exactly once. Top-origin and
  horizontal flows replay directly at their destination. Final exclusion
  geometry comes from the replayed principal border box rather than its paint
  ink.
- A different `writing-mode` value from the nearest box-generating ancestor
  promotes a `flow` inner display type to `flow-root`; `display: contents`
  remains inheritance-only and is skipped for that comparison. This covers
  parallel sideways/vertical pairs as well as orthogonal flows, so the BFC
  float-avoidance rule applies to
  `slr-alongside-vlr-floats.html` and `srl-alongside-vrl-floats.html`.
- Orthogonal table-cell alignment measures the selected, constrained inline
  fragments. A vertical cell's `height` or `max-height` therefore wraps its
  contents before legacy `vertical-align` distributes logical block-axis free
  space.
- `@supports` recognizes top-level logical conjunctions between parenthesized
  declaration tests, including `(writing-mode: vertical-lr) and (direction:
  rtl)`, as required by CSS Conditional Rules.
- Orthogonal available-size selection keeps percentage definiteness separate
  from direct fixed/min/max constraints, the nearest scrollport fallback, and
  the initial-containing-block cap. Its initial containing-block measure is
  stable across page fragmentation, rather than being taken from the current
  fragmentainer; the typed selected measure now selects one final-equivalent,
  frozen ordinary inline stream for intrinsic sizing and final line layout.
  Positioned descendants are retained as deferred static-position recipes and
  materialized after that selected line stack establishes the containing box.
  The remaining visual mismatch in the ICB/scroller WPT cases is tracked in
  `SPEC_DIVERGENCES.md`.
- Mixed inline visual ordering resolves the selected line with CSS's explicit
  paragraph direction through UAX #9 rather than inserting an LRM/RLM into
  author text. CSS-generated embedding, isolate, and override controls remain
  part of the same source sequence for reordering. The bidi embedding,
  isolate, override, unset, plaintext, block-plaintext, and the positive
  astral Adlam reftest and anti-reference are raster-exact. Explicit LTR
  `isolate-override` retains its resolved visual sequence even for intrinsic
  RTL astral text. Final shaped visual-group advances now drive both line
  measurement and the cursor used for subsequent outer text, while preserving
  the individual background fragments owned by the reordered isolate. This is
  raster-exact for `bidi-isolate-override-003.html` and `004.html`.
- `available-size-018` is raster-exact in the current exact WPT run.
  `available-size-003` and `available-size-012` remain visual mismatches and
  are tracked in `SPEC_DIVERGENCES.md`; the remaining available-size cases
  require a fresh exact run before they can be claimed as passing. The current
  `020` run is a separate page-count mismatch (two actual pages versus five
  reference pages).
  The contained/scroller variants `021`–`023` remain tracked as visual
  mismatches in the current worktree; their renderer timings are within the
  evaluator's five-second budget but above the one-second performance target.
- Basic `text-combine-upright: all` and `digits` compositions use a horizontal
  one-em child line, central-baseline alignment, width-feature compression,
  full-width reversal for multi-unit compositions, preserved preformatted
  whitespace, unclipped paint ink, and a one-em effective footprint for
  orthogonal auto sizing. Generated-content and nested inherited run provenance,
  plus full bidi isolation across synthetic atomic boundaries, remain tracked in
  `SPEC_DIVERGENCES.md`.
- Paged root flows fragment in their logical block direction. Consequently,
  `vertical-rl` consumes the physical page area from right to left and
  `vertical-lr` from left to right, using page-area width as fragmentainer
  capacity while retaining physical-height percentage bases. Fragment slices
  own `box-decoration-break: slice` start/end decoration on their first/last
  logical page fragment.
- Fragment paint projection carries independent source and destination axes,
  so logical slice selection is not coupled to the destination fragmentainer's
  physical clip. Positioned-tail clipping selects the destination logical
  block axis but retains overflow rectangles in their specified physical axes.
- Absolutely positioned boxes resolve a continuous physical margin box before
  page selection. The selected page span then follows the principal
  fragmentainer block side, rather than assuming that physical Y is page
  progression.

The following local reftests pass with the debug renderer after these changes:

- `css/css-writing-modes/writing-mode-vertical-rl-003.htm`
- `css/css-writing-modes/table-progression-vrl-004.html`
- `css/css-writing-modes/table-progression-vlr-004.html`
- `css/css-writing-modes/sideways-lr-main-axis.html`
- `css/css-writing-modes/forms/range-input-vertical-rtl-painting.html`
- `css/css-break/block-001-wm-vrl-print.html`
- `css/css-break/block-001-wm-vlr-print.html`
- `css/css-logical/logical-values-float-clear-4.html`

## Current table projection

Fragmented table structural backgrounds now project from one logical source
grid into physical destinations. Column and row clips use originating-cell
areas, vertical block advancement uses the fragmentainer's logical block
origin, and each destination viewport owns the paired source-grid projection
and durable source row slices. Gradient phase is resolved before the
writing-mode transform. The reported fragmented table-paint matrix remains
failing in horizontal-tb LTR, vertical-lr RTL, and vertical-rl RTL: wrapper
edges and captions do not yet replay from one continuous wrapper placement
through all multicolumn destination fragments. The projection is therefore
not claimable as complete parity.

Collapsed-border conflict resolution now maps physical declaration sides to
the root table's logical inline/block edges before grid insertion and maps
resolved half-insets back to physical box edges only at the layout boundary.
This covers vertical and sideways roots; fragmented collapsed-border replay
remains separate work.

## Next work

- Continue the writing-modes sweep with principal-flow propagation, text
  orientation, form controls, and row/row-group structural table painting.
- Vertical propagated-body direct children now resolve against their current
  logical block track, including page-height inline percentage bases and
  default paragraph block margins. `sideways-lr` canvas projection uses its
  required bottom-origin inline edge; remaining principal-flow work is root
  pseudo-element continuation and vertical/sideways inline text extent.
- Buttons, inputs, and selects use the browser-compatible `border-box` UA
  sizing model; textarea retains CSS's default `content-box` model. The
  remaining zero-inline-size range mismatch is in the flex replay path, which
  still reintroduces an intrinsic main-axis extent after flex sizing has
  resolved that border-box dimension to zero.

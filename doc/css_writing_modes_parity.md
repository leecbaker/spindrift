# CSS Writing Modes parity

## Verified behavior

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
- Orthogonal table-cell alignment measures the selected, constrained inline
  fragments. A vertical cell's `height` or `max-height` therefore wraps its
  contents before legacy `vertical-align` distributes logical block-axis free
  space.
- `@supports` recognizes top-level logical conjunctions between parenthesized
  declaration tests, including `(writing-mode: vertical-lr) and (direction:
  rtl)`, as required by CSS Conditional Rules.
- Orthogonal available-size selection keeps percentage definiteness separate
  from direct fixed/min/max constraints, the nearest scrollport fallback, and
  the initial-containing-block cap. The selected logical inline measure is
  reused by intrinsic sizing and final line layout, including through
  containment and intervening parallel formatting contexts.
- Mixed inline visual ordering resolves the selected line with CSS's explicit
  paragraph direction through UAX #9 rather than inserting an LRM/RLM into
  author text. CSS-generated embedding, isolate, and override controls remain
  part of the same source sequence for reordering. The bidi embedding,
  isolate, override, unset, plaintext, block-plaintext, and the positive
  astral Adlam reftest are raster-exact. The Adlam anti-reference remains
  divergent because an explicit LTR isolate-override does not yet differ from
  the intrinsic RTL run.
- `available-size-003`, `005`, `012`–`014`, and `020`–`023` are
  raster-exact. The contained/scroller variants finish within the evaluator's
  five-second budget.
- Basic `text-combine-upright: all` and `digits` compositions use a horizontal
  one-em child line, central-baseline alignment, width-feature compression,
  full-width reversal for multi-unit compositions, preserved preformatted
  whitespace, unclipped paint ink, and a one-em effective footprint for
  orthogonal auto sizing. Generated-content and nested inherited run provenance,
  plus full bidi isolation across synthetic atomic boundaries, remain tracked in
  `SPEC_DIVERGENCES.md`.

The following local reftests pass with the debug renderer after these changes:

- `css/css-writing-modes/writing-mode-vertical-rl-003.htm`
- `css/css-writing-modes/table-progression-vrl-004.html`
- `css/css-writing-modes/table-progression-vlr-004.html`
- `css/css-writing-modes/forms/range-input-vertical-rtl-painting.html`

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

## Next work

- Continue the writing-modes sweep with principal-flow propagation, text
  orientation, form controls, and row/row-group structural table painting.
- Principal-flow body tests still double-advance a later block sibling after a
  vertical-flow child; the correction needs to distinguish normal child flow
  from the root/canvas paint projection.
- Buttons, inputs, and selects use the browser-compatible `border-box` UA
  sizing model; textarea retains CSS's default `content-box` model. The
  remaining zero-inline-size range mismatch is in the flex replay path, which
  still reintroduces an intrinsic main-axis extent after flex sizing has
  resolved that border-box dimension to zero.

# CSS Writing Modes parity

## Verified behavior

- Replayed table-part styles cross the normal used-length boundary. Font-relative
  box-model lengths such as `td { width: 1em }` are therefore finalized before
  table track sizing and structural background painting.
- An orthogonal horizontal block in a vertical containing block is placed from
  the containing block's logical block-start edge even when it has an explicit
  CSS `width`. Its physical horizontal border-box span is the parent vertical
  flow's logical block span.
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
- `available-size-001`, `003`, `005`, `012`–`014`, and `020`–`023` are
  raster-exact. The contained/scroller variants finish within the evaluator's
  five-second budget.

The following local reftests pass with the debug renderer after these changes:

- `css/css-writing-modes/writing-mode-vertical-rl-003.htm`
- `css/css-writing-modes/table-progression-vrl-004.html`
- `css/css-writing-modes/table-progression-vlr-004.html`
- `css/css-writing-modes/forms/range-input-vertical-rtl-painting.html`

## Next work

- Continue the writing-modes sweep with principal-flow propagation, text
  orientation, form controls, and row/row-group structural table painting.
- Principal-flow body tests still double-advance a later block sibling after a
  vertical-flow child; the correction needs to distinguish normal child flow
  from the root/canvas paint projection.
- Native form controls use the browser-compatible `border-box` UA sizing
  model. The remaining zero-inline-size range mismatch is in the flex replay
  path, which still reintroduces an intrinsic main-axis extent after flex
  sizing has resolved that border-box dimension to zero.

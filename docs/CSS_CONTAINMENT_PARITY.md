# CSS Containment parity

Last updated: 2026-08-16

Root/body containment is resolved before layout from the cascaded `html` and
first eligible `body` styles. Any active containment (a non-`none` used
`contain` value, including `inline-size` and `style`, or
`content-visibility: auto`/`hidden`) on either element disables body property
propagation to the principal flow, viewport overflow, and canvas background.
When propagation is disabled, the root supplies the document-canvas flow and
the body is an ordinary principal block; root/body principal boxes still
receive their normal containment effects.
This is not a statement about CSS Container Query evaluation.

## Implemented value surface

- `container-type`, `container-name`, and the `container` shorthand are parsed
  into typed computed-style fields.
- Logical `contain-intrinsic-inline-size` and
  `contain-intrinsic-block-size` resolve to their physical computed owner
  before CSS-wide defaulting and rollback, including vertical writing modes.
- `cqw`, `cqh`, `cqi`, `cqb`, `cqmin`, and `cqmax` are preserved through
  `calc()`, `min()`, `max()`, `clamp()`, and `calc-size()` length values.
- When no query container is available, these units resolve against the paged
  small viewport, as required by CSS Containment Level 3.
- `contain: style`, `content`, and `strict` compute their style-containment
  bit. Style containment prevents descendant counter increments and sets from
  mutating an enclosing counter, while preserving ordinary counter resets and
  keeping generated-quote nesting local to the containment scope.
- Layout and paint containment establish independent formatting contexts for
  block and atomic-inline layout, including local float and margin behavior.
  Captured atomic-inline paint retains an explicit scratch-to-border-box replay
  transform, so descendant margin placement, paint-containment clipping, and
  stacking-context bounds use one resolved border box without treating the
  parent line's margin-box geometry as fragment-local paint coordinates.
  Layout containment also establishes a stacking context. A layout-contained
  atomic inline suppresses every descendant baseline source—including Grid,
  multicolumn, and captured fragment line baselines—and uses the ordinary
  bottom-margin-edge fallback; forced breaks remain consumable by the active
  local fragmentainer.
- The used containment record is the layout and paint source of truth. It
  rejects non-atomic inline and excluded internal-table principal boxes while
  retaining effects for eligible table cells; layout/paint boundaries export
  descendant ink but not descendant scrollable overflow.
- Containment on an eligible `html` or `body` principal box uses the same
  document-level propagation resolver as background and viewport overflow.
  It preserves computed inherited writing-mode and text-orientation inside
  the subtree while preventing the body's values from becoming root used
  values. Vertical direct-child placement follows the actual document-canvas
  source, whether that is the root or an eligible propagated body.
- `contain: inline-size` is parsed separately from `size`; intrinsic inline
  contributions are suppressed by logical axis in normal-flow, Flexbox, Grid,
  and table measurement while orthogonal physical block contributions remain.
- HTML `content-visibility: hidden` retains its principal box and semantics
  while skipping descendant formatting and paint, using size containment for
  fallback geometry. `auto` remains conservatively visible in paged output
  while applying layout/style/paint containment. SVG text is excluded.
- `subgrid` track lists are parsed. A layout- or paint-contained subgrid axis
  resolves to its required used value of `none`; cyclic percentage items stay
  automatic for implicit-track sizing and resolve against their final area.

## Remaining work

The layout tree does not yet collect eligible container snapshots or select
ancestors for container units. `@container` size conditions and their
fixed-point recascade/layout loop are therefore not implemented. Style queries,
scroll-state queries, and their containment behavior remain out of scope.

General subgrid track inheritance, line-name propagation, and parent sizing
participation remain unimplemented. Remaining containment work is outside the
Level 1 acceptance set: `inline-size` containment still needs complete
replaced-content, table, multicolumn, and fragmentation coverage;
`content-visibility: auto` has no interactive viewport lifecycle in paged
output; and SVG text remains explicitly deferred.

Primary reference: <https://www.w3.org/TR/css-contain-3/>.

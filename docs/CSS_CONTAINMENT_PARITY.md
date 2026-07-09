# CSS Containment parity

The local CSS Containment reftest run passes **306 / 314** tests at 96 DPI
with the harness's configured comparison tolerance. The remaining failures are
concentrated in baseline synthesis, table-cell clipping, exact margin-edge
rasterization, size-contained caption/multicol layout, and the explicitly
deferred SVG-text `content-visibility:auto` case.

## Implemented value surface

- `container-type`, `container-name`, and the `container` shorthand are parsed
  into typed computed-style fields.
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
participation remain unimplemented. `inline-size` containment still needs
complete replaced-content, table, multicolumn, and fragmentation coverage.
`content-visibility: auto` has no interactive viewport lifecycle in paged
output, and SVG text remains explicitly deferred.

Primary reference: <https://www.w3.org/TR/css-contain-3/>.

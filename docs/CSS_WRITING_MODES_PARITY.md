# CSS Writing Modes Parity

This note tracks writing-mode behavior that affects layout geometry rather
than text shaping alone.

## Current Coverage

- `writing-mode: horizontal-tb`, `vertical-rl`, and `vertical-lr` are parsed,
  cascaded, and used by block, inline, table, flex, floats, and painting paths.
- Block layout now carries a logical inline content size separately from the
  physical content width, so orthogonal vertical-flow roots do not wrap text
  against the containing block's horizontal span.
- Orthogonal auto inline sizing uses the CSS Writing Modes fit-content rule
  against the containing block's available perpendicular size. For auto-height
  containing blocks, fixed `max-height` is floored by fixed `min-height` before
  falling back to the initial containing block, covering WPT
  `available-size-018.html`.
- Normal-flow orthogonal flex containers with `width:auto` use flex intrinsic
  sizing for their physical block axis, so vertical row flex containers can
  shrink-wrap their item cross-size while honoring a definite physical height.

## Remaining Divergences

- Scrollport-derived orthogonal available sizes still need a full audit across
  `overflow` combinations and nested scroll containers.
- Fragmented orthogonal flows still inherit Quire's cursor-oriented pagination
  limits; available block-size negotiation is not yet represented as durable
  fragment objects.
- Orthogonal available-size coverage is currently strongest for normal block
  containers and normal-flow flex container auto-width sizing. Grid,
  table-cell, absolutely positioned, and deeper mixed writing-mode descendant
  paths need broader WPT coverage.

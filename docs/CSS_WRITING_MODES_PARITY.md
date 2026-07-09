# CSS Writing Modes Parity

This note tracks writing-mode behavior that affects layout geometry rather
than text shaping alone.

## Current Coverage

- `writing-mode: horizontal-tb`, `vertical-rl`, and `vertical-lr` are parsed,
  cascaded, and used by block, inline, table, flex, floats, and painting paths.
- Logical side and axis projection is centralized in the computed-flow mapping
  used by CSS Logical Properties and layout adapters. This covers all five
  supported writing modes and both `direction` values, including the distinct
  `sideways-lr` inline progression.
- In vertical typographic modes, `text-orientation: upright` now forces the
  used inline direction to LTR without changing its cascaded `direction`.
  Table-root axes consume that used value, covering upright RTL table
  progression as well as the common flow-axis boundary.
- Block layout now carries a logical inline content size separately from the
  physical content width, so orthogonal vertical-flow roots do not wrap text
  against the containing block's horizontal span.
- Normal block intrinsic `width` sizing now keeps logical inline and block
  content-size contributions typed separately before projecting to physical
  width, so vertical writing-mode `width:min-content` and `width:max-content`
  use block-axis line-column size.
- Orthogonal auto inline sizing uses the CSS Writing Modes fit-content rule
  against the containing block's available perpendicular size. For auto-height
  containing blocks, fixed `max-height` is floored by fixed `min-height` before
  falling back to the initial containing block, covering WPT
  `available-size-018.html`.
- Normal-flow orthogonal flex containers with `width:auto` use flex intrinsic
  sizing for their physical block axis, so vertical row flex containers can
  shrink-wrap their item cross-size while honoring a definite physical height.
- Child available space keeps a percentage basis separate from its orthogonal
  layout fallback and projects both through the child writing mode.  An
  indefinite containing physical height can therefore provide a usable
  orthogonal available size without incorrectly resolving vertical inline
  percentage margins. This covers the normal-flow percentage-margin cases in
  `sizing-orthogonal-percentage-margin-003`, `-004`, `-007`, and `-008`.
- The orthogonal fallback now records whether it came from the initial
  containing block or the nearest constrained scroll container. It remains a
  layout-only value in both cases, so it never turns an indefinite percentage
  basis into a definite one.
- Table cells now derive their row and column rectangles from logical table
  tracks and project those rectangles through the table root's flow axes. This
  keeps table slot progression distinct from the writing mode used to lay out
  a cell's own contents.
- Vertical table roots now resolve their column-grid preferred size from the
  physical `height` property, and parent/float intrinsic sizing projects the
  table's logical block tracks to physical width. This prevents a vertical
  table's column extent from being replayed as a horizontal float width.
- Empty vertical float-avoidance scopes preserve the hypothetical physical
  top for both inline directions. In particular, a bottom-to-top nested BFC
  no longer jumps by the full available page-inline span before table layout.

## Remaining Divergences

- Scrollport-derived orthogonal available sizes still need a full audit across
  `overflow` combinations and nested scroll containers. The scoped fallback
  is currently threaded through normal block flow only.
- Fragmented orthogonal flows still inherit Quire's cursor-oriented pagination
  limits; available block-size negotiation is not yet represented as durable
  fragment objects.
- Orthogonal available-size coverage is currently strongest for normal block
  containers and normal-flow flex container auto-width sizing. Grid,
  table-cell, absolutely positioned, and deeper mixed writing-mode descendant
  paths need broader WPT coverage.
- Table wrapper origin placement for RTL vertical roots, structural
  backgrounds, collapsed borders, alignment, and fragmentation still retain
  physical horizontal-grid assumptions after the primary cell-grid projection;
  vertical and sideways table WPTs therefore remain incomplete.
- `text-combine-upright` now parses, cascades, and inherits `none`, `all`, and
  `digits 2..4`; normalized eligible runs form horizontal child sequences in a
  one-em atomic inline and their captured paint subtree is compressed and
  replayed as a unit. Contiguous normalized words now form one composition
  when their style, link, decoration, bidi, and source metadata match. Exact
  baseline alignment and explicit nested-inline scope tracking remain
  incomplete.

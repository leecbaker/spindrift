# Flex sizing and replay parity

This focused matrix records the Flex sizing/replay lifecycle. The broader
feature inventory is maintained in `docs/CSS_FLEXBOX_PARITY.md`.

| Lifecycle boundary | Implemented behavior | Exact WPT evidence |
| --- | --- | --- |
| Intrinsic descendant contribution | A block descendant applies its own intrinsic `min-width`/`max-width` constraints before becoming a flex item's automatic-minimum content suggestion. | `flex-item-content-is-min-width-max-content.html` |
| Automatic block minimum | An intrinsic `min-height:auto` probe suppresses its temporary percentage basis, so an enclosing definite flex basis cannot leak through the auto-sized item. | `flex-minimum-height-flex-items-014.html` |
| Block-axis intrinsic minimum | A logical-block-axis `min-content` minimum uses the content-based Flex floor, while retaining its authored origin so only a true `auto` minimum takes the scroll-container zero-minimum exception. | `flex-minimum-height-flex-items-023.html`, `flex-item-min-height-min-content-overflow.html` |
| Block-axis intrinsic preferred size | A flex container resolves `height:min-content`, `max-content`, and `fit-content` from its typed intrinsic block contributions before the final flexible-length pass, rather than asking Taffy for an unconstrained max-content layout. | `flex-container-min-content-001.html`, `flex-container-max-content-001.html` (remaining matrix cells noted below) |
| Content-derived flex basis | A measured content-box contribution is passed unchanged to `flex-basis:auto`; whitespace preservation affects text measurement, never the subsequent basis conversion. | `whitespace-in-flexitem-001.html` |
| Flexible-length allocation | Taffy owns the resolved main-size allocation; Quire records it with the corresponding intrinsic estimate in `FlexItemSizingState`. | `flexbox-definite-cross-size-constrained-percentage.html` |
| Final main-axis replay | After final item sizing, Quire repacks each line with CSS Align’s resolved free space, including distributed-alignment fallback to `flex-start` on a reverse main axis. | `justify-content_space-between-003.tentative.html` |
| Final line remeasurement | A final normal-flow measurement can update an item's cross contribution, but cannot replace the allocated main size with an unrelated intrinsic cursor span. | `flexbox-definite-cross-size-constrained-percentage.html` |
| Single-line min/max clamp | A container min/max cross-size creates a final line slot while remaining distinct from a definite percentage basis. | `flexbox-single-line-clamp-1.html`, `flexbox-single-line-clamp-3.html` |
| Replaced-control stretch eligibility | Range controls retain their automatic CSS preferred size, so the generic text-control UA dimensions do not suppress Flex's normal cross-axis stretch resolution. | `flexitem-stretch-range.html` |
| Final cross-axis auto margins | Automatic cross margins are distributed from the final line slot after stretch and line packing, instead of retaining a provisional Taffy placement. | `align-self-015.html` |
| Float-based reference overflow | The legacy LTR `flexbox-align-self-vert` references use right floats to emulate cross-end alignment. Overwide right floats retain their outer right edge and overflow left; a short atomic line commits one later float-band retry after materialization, before `text-align` resolves. The reference inline-block proxy also sizes its contents in its own BFC, without inheriting parent-float exclusions. | `flexbox-align-self-vert-003.xhtml`, `flexbox-align-self-vert-004.xhtml` |
| Reversed baseline line edge | A sole compatible baseline participant remains attached to the final Flex cross-start edge of a `wrap-reverse` line, rather than falling back to its own self-start edge. | `flexbox-align-self-baseline-horiz-003.xhtml` |
| Safe line packing | When an overflowing wrapped line falls back through `safe`, final repacking uses logical cross-start rather than the `wrap-reverse` flex cross-start edge. | `flexbox-safe-overflow-position-002.html`, `flexbox-safe-overflow-position-005.html` |
| Descendant replay basis | Explicitly definite and stretch-established final bases resolve descendant percentages; cyclic automatic row-line contributions remain indefinite. | `percentage-heights-010.html`, `percentage-heights-015.html`, `percentage-heights-019.html`, `percentage-heights-021.html`, `percentage-max-height-003.html`, `percentage-padding-002.html`, `percentage-padding-005.html` |
| Document-canvas paint ownership | A flex root or propagated body suppresses its local background only when CSS Backgrounds selected it as the canvas source; another canvas participant remains an ordinary principal-box paint subject. | `flexbox_quirks_body.html` |

## Remaining related divergences

- The exact sizing/replay census currently has 33 passing and 26 failing
  paths. The unresolved Flex-owned group is baseline and cross-axis alignment
  in horizontal, vertical, and RTL flows; its references also exercise the
  shared inline baseline and float formatting-context paths, so it requires
  a geometry audit across those boundaries rather than an item-offset
  correction.
- `flex-abspos-align-self-safe-outer-cb-001.tentative.html` and
  `flex-abspos-align-self-safe-outer-cb-002.tentative.html` cover the
  unresolved CSS Align safe-overflow/static-position interaction when the
  absolutely positioned item's containing block is outside its flex
  container. The corresponding inner-containing-block cases pass.
- `flex-container-min-content-001.html` and
  `flex-container-max-content-001.html` now resolve their block-axis intrinsic
  preferred size before final Flex layout. The current exact residuals are
  1,132 and 636 pixels respectively, concentrated in a small number of
  grid-hosted matrix cells; the remaining work is intrinsic item contribution
  selection rather than an unconstrained-container replay error.
- `grandchild-span-height.html` exposes a one-CSS-pixel excess in the shared
  inline line-box/baseline extent of an atomic inline. `table-with-float-paint`
  remains a float-formatting-context integration.
- `negative-margins-001.html` has only an 80-pixel raster difference after
  signed margin contribution handling; it is a baseline/paint-boundary issue,
  not a container intrinsic-width discrepancy.
- `flexbox-safe-overflow-position-006.html` exercises legacy `-webkit-box`
  layout rather than Flexbox and remains outside this sizing/replay lifecycle.

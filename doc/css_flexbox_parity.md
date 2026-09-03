# Flex sizing and replay parity

This focused matrix records the Flex sizing/replay lifecycle. The broader
feature inventory is maintained in `docs/CSS_FLEXBOX_PARITY.md`.

| Lifecycle boundary | Implemented behavior | Exact WPT evidence |
| --- | --- | --- |
| Intrinsic descendant contribution | A block descendant applies its own intrinsic `min-width`/`max-width` constraints before becoming a flex item's automatic-minimum content suggestion. | `flex-item-content-is-min-width-max-content.html` |
| Automatic block minimum | An intrinsic `min-height:auto` probe suppresses its temporary percentage basis, so an enclosing definite flex basis cannot leak through the auto-sized item. | `flex-minimum-height-flex-items-014.html` |
| Block-axis intrinsic minimum | A logical-block-axis `min-content` minimum uses the content-based Flex floor, while retaining its authored origin so only a true `auto` minimum takes the scroll-container zero-minimum exception. | `flex-minimum-height-flex-items-023.html`, `flex-item-min-height-min-content-overflow.html` |
| Block-axis intrinsic preferred size | A flex container resolves `height:min-content`, `max-content`, and `fit-content` from its typed intrinsic block contributions before the final flexible-length pass, rather than asking Taffy for an unconstrained max-content layout. | `flex-container-min-content-001.html`, `flex-container-max-content-001.html` (remaining matrix cells noted below) |
| Preferred aspect-ratio box space | One typed width/height conversion applies bare ratios in the `box-sizing` box and `auto <ratio>` in content-box space, including asymmetric padding. | `flex-aspect-ratio-025.html`, `flex-aspect-ratio-026.html`, `flex-aspect-ratio-031.html` |
| Aspect-ratio constraint transfer | Definite min/max constraints transfer in both directions once, with destination preferred/min/max caps and floors applied before each axis is independently constrained. Intrinsic content minimums remain available unless an effective maximum caps them. | `flex-aspect-ratio-025.html`, `flex-aspect-ratio-026.html`, `flex-aspect-ratio-039.html`, `flex-aspect-ratio-049.html` |
| Stretch-fit automatic minimum | An authored cross-axis `stretch` establishes a definite, constraint-clamped ratio input for both Flexbox's primary automatic minimum and its post-layout safeguard. Replaced items derive their content suggestion from that same used cross size rather than retaining a smaller natural-object contribution. | `flexbox-auto-minimum-001.html`, `flexbox-auto-minimum-002.html` |
| Auto/auto ratio flex basis | Flexbox 9.2 Part E derives the main flex basis from the fit-content cross contribution while ignoring main-axis min/max constraints for this basis calculation. | `flex-aspect-ratio-034.html`–`flex-aspect-ratio-038.html` |
| Content-derived flex basis | A measured content-box contribution is passed unchanged to `flex-basis:auto`; whitespace preservation affects text measurement, never the subsequent basis conversion. | `whitespace-in-flexitem-001.html` |
| Table block minimum | A column flex item's table grid/caption minimum remains a table-layout floor even when the author sets `min-height: 0`; Flexbox cannot replay a zero-height wrapper over an intrinsically taller grid. | `table-as-item-min-content-height-1.tentative.html` |
| Unbounded table max-content | CSS Tables' unbounded max-content state is represented explicitly at the table/Flex boundary and resolves against Flexbox's definite available main-size slot instead of becoming a scalar backend length. | `table-with-infinite-max-intrinsic-width.html` |
| Flexible-length allocation | Taffy owns the resolved main-size allocation; Spindrift records it with the corresponding intrinsic estimate in `FlexItemSizingState`. | `flexbox-definite-cross-size-constrained-percentage.html` |
| Final main-axis replay | After final item sizing, Spindrift repacks each line with CSS Align’s resolved free space, including distributed-alignment fallback to `flex-start` on a reverse main axis. | `justify-content_space-between-003.tentative.html` |
| Final line remeasurement | Once a flex line has its final cross size, each dependent item is remeasured with that used content-box size as a typed definite basis. Its final source extent is replaced by that replay (rather than retaining a cyclic intrinsic probe), while unchanged automatic items remain indefinite. | `flexbox-definite-cross-size-constrained-percentage.html`, `flexbox-definite-sizes-001.html` |
| Single-line min/max clamp | A container min/max cross-size creates a final line slot while remaining distinct from a definite percentage basis. | `flexbox-single-line-clamp-1.html`, `flexbox-single-line-clamp-3.html` |
| Replaced-control stretch eligibility | Range controls retain their automatic CSS preferred size, so the generic text-control UA dimensions do not suppress Flex's normal cross-axis stretch resolution. | `flexitem-stretch-range.html` |
| Final cross-axis auto margins | Automatic cross margins are distributed from the final line slot after stretch and line packing, instead of retaining a provisional Taffy placement. | `align-self-015.html` |
| Float-based reference overflow | The legacy LTR `flexbox-align-self-vert` references use right floats to emulate cross-end alignment. Overwide right floats retain their outer right edge and overflow left; a short atomic line commits one later float-band retry after materialization, before `text-align` resolves. The reference inline-block proxy also sizes its contents in its own BFC, without inheriting parent-float exclusions. | `flexbox-align-self-vert-003.xhtml`, `flexbox-align-self-vert-004.xhtml` |
| Reversed baseline line edge | A sole compatible baseline participant remains attached to the final Flex cross-start edge of a `wrap-reverse` line, rather than falling back to its own self-start edge. | `flexbox-align-self-baseline-horiz-003.xhtml` |
| Flex-container baseline export | The first/last order-modified line owns requested- and opposite-set sharing priority. If it has no sharing group, the measured-item and synthesized fallback is selected from the startmost/endmost finalized ordinary writing-mode line; `wrap-reverse` does not exchange those physical fallback edges. Empty flex containers export no fabricated baseline: their atomic-inline fallback is deferred to CSS Inline, which synthesizes it from the margin box. Explicit physical exports retain their named source baseline (central for mixed/upright vertical synthesis, alphabetic otherwise) until parent-line resolution selects its requested metric. | All 17 non-reference `flexbox-baseline-*` WPTs (17/17 exact on 2026-08-21); `flexbox-baseline-empty-001a.html`; `flexbox-baseline-empty-001b.html`; `synthesize-vrl-baseline.html` (exact on 2026-08-26) |
| Estimated baseline line extent | Flexbox 9.4 line estimation sums the greatest participant baseline-to-outer-start and baseline-to-outer-end distances, compares the sum with every non-participant outer cross size, and applies the result to one-line as well as multiline intrinsic flex sizing. | `flexbox-baseline-nested-001.html`; focused line-estimation unit coverage |
| Forced-break float reference geometry | An author-styled atomic `<br>` remains an HTML forced-break control. Once that break commits the preceding anonymous inline run, the adjoining-float origin advances to the source-order line floor, while the preceding in-flow line remains the inline-block baseline. | `flexbox-baseline-multi-line-vert-001.html`, `flexbox-baseline-multi-line-vert-002.html` |
| Safe line packing | When an overflowing wrapped line falls back through `safe`, final repacking uses logical cross-start rather than the `wrap-reverse` flex cross-start edge. | `flexbox-safe-overflow-position-002.html`, `flexbox-safe-overflow-position-005.html` |
| Descendant replay basis | Explicitly definite and stretch-established final bases resolve descendant percentages; cyclic automatic row-line contributions remain indefinite. | `percentage-heights-010.html`, `percentage-heights-015.html`, `percentage-heights-019.html`, `percentage-heights-021.html`, `percentage-max-height-003.html`, `percentage-padding-002.html`, `percentage-padding-005.html` |
| Ratio-derived definiteness | A container main size derived from a definite perpendicular axis is definite for column wrapping; an item flex basis transferred through a definite ratio input makes its post-flexing main size definite for descendant percentages. | `flex-aspect-ratio-007.html`, `flex-aspect-ratio-008.html`, `flex-aspect-ratio-032.html`, `flex-aspect-ratio-033.html` |
| Ratio-dependent container automatic minimum | Automatic block sizing first measures the content-based minimum without imposing the ratio preference, then floors the preferred height by that minimum and applies the effective maximum before final Flex layout. | `flex-aspect-ratio-040.html`, `flex-aspect-ratio-041.html`, `flex-aspect-ratio-043.html`, `flex-aspect-ratio-044.html` |
| Physical main-axis pagination | An overflowing wrapped logical-row flex container fragments at physical-Y item intervals in vertical and sideways writing modes. The outgoing fragment suppresses its cross-axis gutter at the break while retaining source-order gap-rule assignment for its continuation. | `css/css-gaps/flex/flex-gap-decorations-multi-value-writing-mode.html` |
| Nested size-contained overflow | Flex measurement preserves an in-flow item's used block extent separately from visible descendant source extent, including an ordinary wrapper around `contain:size`. Source-slice replay for that nested source canvas remains incomplete. | focused nested Flex source-extent coverage; `css/css-page/monolithic-overflow-005-print.html`–`monolithic-overflow-008-print.html` |
| Document-canvas paint ownership | A flex root or propagated body suppresses its local background only when CSS Backgrounds selected it as the canvas source; another canvas participant remains an ordinary principal-box paint subject. | `flexbox_quirks_body.html` |

## Aspect-ratio acceptance evidence

The exact `css/css-sizing/aspect-ratio/flex-aspect-ratio-*` non-script prefix
passes 54 of 54 tests. The lifecycle above covers the 17 formerly failing
paths (`007`, `008`, `025`, `026`, `031`–`037`, `039`–`041`, `043`, `044`, and
`049`) while retaining all 37 prior passes. Script-driven
`flex-aspect-ratio-042.html` is outside this non-script run.

## Flexbox baseline acceptance evidence

Exact evaluation on 2026-08-21 passes all 17 non-reference
`css/css-flexbox/flexbox-baseline-*` tests. The five formerly failing multiline
cases now compare either raster-exact or metadata-insensitive PDF-identical:
`multi-line-horiz-002`, `multi-line-horiz-003`, `multi-line-horiz-004`,
`multi-line-vert-001`, and `multi-line-vert-002`.

## Remaining related divergences

- The dedicated `flexbox-baseline-*` family is now exact. Remaining baseline
  and cross-axis alignment work belongs to other horizontal, vertical, RTL,
  orthogonal-flow, and fragmentation families; those paths still require
  geometry audits across Flex, inline baseline, and float formatting-context
  boundaries rather than item-offset corrections.
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

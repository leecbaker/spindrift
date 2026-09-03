# CSS Positioning parity

## Grid absolute positioning

An absolutely positioned Grid descendant uses its Grid positioning containing
block. When its grid placement is automatic, the containing-block edges are
the Grid container's padding edges; percentage dimensions resolve against
that resulting Grid area. Definite placement uses the first occupied track's
start edge and the final occupied track's end edge, so it includes crossed
interior gutters but excludes any gutter following the area. This geometry is
kept in physical order and applies equally in LTR and RTL. This follows
[CSS Grid §9.1](https://drafts.csswg.org/css-grid-1/#abspos)
and [CSS Positioned Layout §2.1](https://drafts.csswg.org/css-position-3/#abspos-containing-block).

## Compatibility outline paint ordering

CSS UI intentionally leaves the paint order of outlines and overlapping
outlines implementation-defined. Spindrift chooses the compatibility order in
which an ordinary normal-flow outline paints after inline content but before
auto/zero-z positioned descendants. This is compatible with CSS 2.2
Appendix E's optional per-box outline stage and matches the positioned Grid
WPT references.

The policy does not cross a stacking-context boundary: positioned,
transformed, opacity, and page-margin contexts retain their own final local
outline phase. Page-margin boxes remain independent stacking contexts above
document contents, as required by [CSS Paged Media §3.1](https://drafts.csswg.org/css-page-3/#page-painting-order).

References: [CSS UI §3](https://drafts.csswg.org/css-ui-4/#outline-painting),
[CSS 2.2 Appendix E](https://www.w3.org/TR/CSS22/zindex.html), and
[CSS Paged Media §3.1](https://drafts.csswg.org/css-page-3/#page-painting-order).

# CSS Overflow line-clamp parity

Quire resolves legacy `-webkit-line-clamp` only after the cascade has selected
the final display, orientation, and clamp declaration. A legacy clamp is
active only for specified `-webkit-box` or `-webkit-inline-box` with
`-webkit-box-orient: vertical`; its used layout display is a flow-root while
retaining the authored block versus inline outer display. Other legacy boxes
remain ordinary legacy flex boxes.

Clamp accounting is scoped to descendant in-flow line boxes in the same block
formatting context. Flow roots, scroll containers, flex and table formatting
contexts, and fieldsets neither receive nor debit their ancestor's line
budget. Reaching a clamp boundary suppresses later in-flow source and floats
whose source occurs after that boundary. Positioned descendants retain their
containing-block layout path; floats encountered before the boundary still
participate in normal float placement.

The terminal clamp decision is likewise scoped to that shared in-flow stream,
rather than to one inline opportunity graph. Preserved breaks, collected
inline paragraphs, and in-flow block descendants carry an explicit
`LaterInFlowContent` continuation into the final available line. This lets the
selector reserve and fit the block ellipsis before painting the line, even
when the overflow source occurs in a later graph or sibling. Out-of-flow
positioned and floated source does not create that continuation or consume a
line slot.

The following focused static WPTs pass with the current debug Quire binary:

- `webkit-line-clamp-001.html`
- `webkit-line-clamp-002.html`
- `webkit-line-clamp-015.html`
- `webkit-line-clamp-029.html`
- `webkit-line-clamp-046.html`
- `webkit-line-clamp-abspos-001.html`
- `webkit-line-clamp-block-in-inline-001.html`
- `line-clamp-with-abspos-001.html`
- `line-clamp-with-abspos-002.html`
- `line-clamp-with-fixed-pos-001.html`
- `line-clamp-with-fixed-pos-002.html`

The static sweep also exposes existing visual differences in several
ellipsis-fitting and line-clamp reference fixtures. Script-driven mutation
fixtures (017–023, 026, 048, and `dynamic-001`) are intentionally excluded:
Quire does not implement JavaScript or CSSOM mutation.

Remaining CSS Overflow Level 4 gaps are the independently cascaded
`max-lines`, `block-ellipsis`, and `continue` longhands, `line-clamp: auto`,
and `continue: discard`.

<https://drafts.csswg.org/css-overflow-4/#line-clamp>

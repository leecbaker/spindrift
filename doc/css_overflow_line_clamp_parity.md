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
budget. For an automatic block-size clamp, those independent normal-flow
children still debit their actual parent block-axis contribution, so they
remain legal boundaries even though their descendant line boxes are outside
the ancestor's `max-lines` stream. Reaching a clamp boundary suppresses later in-flow source and floats
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

The 2026-08-15 static sweep executed 256 tests (19 script-driven tests
excluded) and passed 198 (77.3%). In addition to
`line-clamp-auto-034.html`, the automatic nested-marker cases 023–025 and
036 now pass: a cached inline sequence carries the terminal block-boundary
continuation into marker fitting. Line-clamp containers now establish an
independent formatting context, as `continue: collapse` requires; this
prevents their child margins from collapsing through the container and makes
the automatic margin cases 027–029 pass. A shared traversal now retains its
remaining line budget through ordinary nested block flow and recognizes
trailing inline source after a block sibling; this makes
`block-ellipsis-003.html` and the nested legacy line-clamp cases 009–013 pass.
The sweep remains below the 90% target because it still lacks replay of an
earlier source endpoint after a later fixed-size or nested block proves not to
fit. Script-driven mutation fixtures (017–023, 026, 048, and `dynamic-001`)
are intentionally excluded: Quire does not implement JavaScript or CSSOM
mutation.

Quire cascades `max-lines`, inherited `block-ellipsis`, and `continue`
independently. The line-clamp shorthands expand into those longhands, and a
non-zero/exhausted layout budget prevents a zero available-line state. A
positive `max-lines` value also supplies the cutoff for `continue: discard`.

For a direct inline formatting context, Quire selects an automatic cutoff from
measured line-box block advances when a finite absolute, line-height-relative,
or definite containing-block percentage used block-size constraint overflows.
An absolute or line-height-relative constraint is also propagated as a typed
remaining content-box allowance through eligible in-flow descendant blocks;
their computed longhands remain independent, and a block-only cutoff cannot
create a marker. It then reselects the terminal line with the marker reserved.
The same local endpoint path handles an unforced `continue: discard` break; it
omits following in-flow source without materializing a page or column
fragmentainer. A direct multicolumn child traversal captures a non-empty opaque
source prefix at its first local region break, replays only that prefix for
balancing and box sizing, and suppresses later spanners and column sets.

Remaining CSS Overflow Level 4 gaps are automatic constraints which need to
replay an earlier inline or block-sibling candidate after later mixed/nested
block flow changes the used size, including constraints that depend on the
enclosing block's final percentage basis. Direct inline balancing now first
selects stable wrapping, then clamps, balances the retained source, and
monotonically reclamps to an earlier endpoint if required. Cross-block
balancing, complete block-ellipsis fitting and bidi isolation, and full
Category-3 region fragmentation through mixed or nested formatting contexts
remain incomplete. Those cases need a block-flow-local candidate and remainder
controller rather than the direct-inline and direct-child multicol cutoff
paths.

<https://drafts.csswg.org/css-overflow-4/#line-clamp>

# CSS Floats Parity

Last updated: 2026-07-18

CSS 2.2 is the conformance target for float placement, exclusion, and
clearance. WeasyPrint is used as a compatibility reference for paged-output
behavior where the specs leave implementation details ambiguous.

## Current Level

- `float` and `clear` parse the physical CSS 2.2 values plus logical
  `inline-start` and `inline-end`.
- Floated children are blockified, auto-width floats use shrink-to-fit sizing,
  intrinsic `width` keywords resolve from the float formatting context's
  min/max-content sizes, percentage and specified float widths are replayed
  from their resolved used sizes, and float margin boxes are recorded as
  page-local exclusions for later flow.
- A float following an adjoining normal-flow block inherits that block's
  pending block-end margin in its hypothetical block-start position; this is
  additive, including negative margins, because the float itself is outside
  the preceding margin-collapse set.
- Empty auto-height floats record zero-height margin-box exclusions, so they
  preserve source-order float placement without shortening same-top line boxes.
- Auto-height float exclusions and page-prebreak decisions use an isolated
  replay of the final blockified flow-root style with its frozen used width,
  rather than a descendant-height estimate that can diverge from final layout.
- Shrink-to-fit preferred width accounts for floats that share an inline run
  with following inline content, so the unconstrained line width includes the
  same-line float margin boxes plus the inline max-content contribution.
- Floats with no visible paint, including `visibility:hidden` placeholders,
  still record margin-box exclusions, so consecutive same-line floats reserve
  reference-grid cells before later floats are placed.
- Line boxes, normal-flow block formatting context roots, table wrappers,
  block-level replaced boxes, flex containers, and SVG/canvas blocks avoid
  active floats through the shared float collision model.
- Floats inside self-collapsing blocks remain part of the open adjoining
  margin set, so a following BFC root that fits beside the float uses the
  resolved collapsed start margin for both float placement and the parent's
  painted border box.
- When a following BFC root cannot fit beside floats that would otherwise be
  adjoining, the BFC separates from those floats and the collapsed start margin
  does not replay the floats downward.
- Horizontal normal-flow block formatting context roots re-resolve their
  auto inline size and estimated auto block size against narrowed float bands
  before accepting placement, so internal floats can force a narrower same-top
  retry instead of exposing overlapped background.
- Horizontal normal-flow block formatting context roots use their border box,
  not their own margin box, for float-adjacent collision. Auto-width roots can
  narrow to the float-free band while horizontal margins remain outside the
  collision box.
- Independent block formatting contexts isolate their internal floats from
  parent flow. Auto-height block formatting context roots and inline-block
  formatting contexts now expand to include floats that belong to their own
  context.
- Overflow-clipped normal-flow BFC roots clip descendant contents while
  preserving their own background, border, and outline outside the overflow
  clip edge.
- Fragmented floats register page-local exclusion shapes, and following text
  reflows around continued float fragments on later pages.
- Floats that are prebroken to the next page are placed from the new
  fragmentainer cursor, so `break-inside: avoid` floated blocks can stay
  together when they fit on an empty page.
- Fragmented float exclusions keep source and fragment identity internally, so
  clearance and replay order can distinguish same-source continuations from
  unrelated same-page floats.
- `clear` accounts for active same-page fragments of a broken float, including
  float fragments continued from earlier fragmentainers.
- Clearance candidates with adjoining collapsed top margins use the
  post-collapse hypothetical border edge, so a large collapsed top margin that
  already places the empty block below the matching float does not create
  spurious clearance or parent background height.
- Float painting preserves per-fragment opacity, transform, and overflow clip
  effects.
- Bookmarks, anchors, named strings, and running elements produced while laying
  out fragmented float fragments survive snapshot restore and are replayed in
  page/source order.
- Generated pseudo text produced inside fragmented floats survives
  target-text/page-margin replay through the float side-effect capture path.
- Inline floats are preserved as zero-width graph markers during line
  selection. A marker reached at the start of a selected line is placed before
  following text is selected. A marker reached after preceding inline text is
  placed on that current line when its margin box fits in the remaining band;
  suffix text is then reselected into the same line when there is post-float
  space. If an oversized tentative placement cannot keep visible suffix content
  on that completed line, the placement is rolled back so the marker defers
  with the suffix to the next line. Prefix text keeps its original position.
- Equal-width line break candidates advance over zero-width inline float
  markers, so an inline-block prefix does not force a following fitting
  right float onto the next line.
- Inline float markers inside unbreakable `white-space: nowrap` and `pre`
  lines no longer create artificial line breaks; floats are placed at the
  current line top while surrounding inline content remains on the overflowing
  visual line. Collapsible whitespace now looks through those zero-width
  out-of-flow markers, so spaces adjacent to the marker collapse as if the
  float were not part of the inline text run.
- `clear` progresses across continued float fragments, applying pending
  page-local exclusions until the final matching continuation is cleared.
- HTML `br` line breaks generated by `br::before` preserve the originating
  `br` element's computed `clear`, so inline `br { clear: both }` can clear
  preceding floats without creating an extra empty line when the break itself
  has no inline content. The same clear metadata is preserved through collected
  inline line sequences used by text-box trimming and fragmentation planning.
- Logical `float`/`clear` sides are stored internally as used physical
  exclusion sides. Horizontal writing keeps CSS 2.2 left/right behavior, while
  vertical `inline-start`/`inline-end` clear matching resolves to top/bottom
  rather than aliasing physical left/right.
- Vertical logical floats now expose top/bottom exclusion bands to vertical
  inline line selection, paint-time line adjustment, and basic BFC-root
  placement.
- Vertical inline line selection advances float-band queries through each
  physical block-axis slab. Source-order logical floats therefore move with
  their line in `vertical-rl`, `vertical-lr`, and sideways writing modes,
  rather than repeatedly excluding every later column from the first slab.
- Vertical BFC roots, table wrappers, flex containers, and orthogonal
  formatting-context roots move to the next physical block-axis slab when a
  top/bottom logical float leaves too little inline span in the current slab.
- Generated image content inside fragmented floats survives float replay along
  with generated text and page-scoped metadata.

## Needed for Parity

- Broaden WPT and WeasyPrint comparison coverage for nested formatting contexts,
  especially combinations of floats with tables, flex items, generated content,
  replaced descendants, and fragmented layout.
- Broaden coverage for page-scope metadata and paint effects inside fragmented
  floats across page boundaries.
- Continue reducing legacy float-row call sites as more inline float placement
  cases are migrated onto the shared collision model.
- Audit table wrappers, flex/grid roots, and block-level replaced boxes for the
  same border-box-versus-margin-box float-adjacent placement rule now covered
  for normal-flow block formatting context roots.

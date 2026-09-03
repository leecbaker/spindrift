# CSS Block Layout Parity

This note tracks normal-flow block layout behavior from CSS 2.2, especially
where block sizing interacts with margin collapse.

## Current Support

- Adjacent in-flow block sibling vertical margins collapse using CSS 2.2's
  adjoining-margin rules, including mixed positive/negative margin sets.
  Absolutely and fixed-positioned source children are transparent to the
  pending in-flow sibling margin set, so they cannot separate two adjoining
  normal-flow blocks.
- First-child top margins and last-child bottom margins can collapse through
  auto-height block containers, including preferred block sizes that behave as
  auto under an indefinite percentage basis, when no border, padding,
  clearance, line box, or formatting-context boundary separates the margins.
- Document-canvas start-margin collapse carries the complete adjoining set
  through transparent block wrappers. The body's canvas inset remains an
  inline-axis offset and is not reapplied when a wrapped first descendant
  continues that same block-start margin set.
- A first child that follows an earlier zero-margin in-flow sibling still
  carries its wrapper and first-descendant margins as one adjoining start set;
  the descendant margin is not subtracted a second time. CSS 2.2:
  <https://www.w3.org/TR/CSS22/box.html#collapsing-margins>.
- HTML document-canvas overflow follows CSS Overflow viewport propagation
  before block layout decides whether it establishes a formatting-context
  boundary. In particular, `body { overflow: hidden }` uses `visible` when it
  provides the root viewport's overflow, so it does not suppress normal
  parent/child margin collapse.
- The propagated overflow source is the exact root or first eligible `body`
  principal box. Other `body` boxes retain their local clips, and propagated
  `overflow-x`/`overflow-y` values preserve their independent viewport axes.
- Overflow clipping keeps physical axes separate through deferred paint and
  transformed stacking contexts. A `clip` axis therefore does not turn its
  `visible` companion axis into a rectangular two-axis clip.
- Phantom inline line boxes are ignored for margin-collapse adjacency, so empty
  inline boxes with only block-axis decoration do not block parent/child or
  self-collapsing margin collapse; non-zero inline-axis margin, border, or
  padding still creates a real separating line box.
- Self-collapsing empty block boxes keep adjoining parent and sibling margin
  sets open when their own height, min-height, border, padding, and in-flow
  contents allow it. Blocks containing atomic inline line-box participants,
  including inline-blocks, are not treated as self-collapsing solely because
  their own `line-height` is zero.
- Block-in-inline anonymous structural wrappers are transparent to adjoining
  margin propagation. Their self-collapsing descendants therefore collapse
  through the enclosing block exactly as ordinary block descendants do, except
  where the descendant establishes a block formatting context, which terminates
  the adjoining-margin set as required by CSS 2.2.
- A last child's bottom margin is measured outside the parent when it collapses
  through, but is excluded from the parent's min/max-height constraint
  calculation when `min-height` or `max-height` prevents that collapse.
- Forced page breaks between block descendants continue at the next page's
  block-start edge without cloning ancestor block-start margin, border, or
  padding, matching `box-decoration-break: slice`.
- `break-inside: avoid` retries preserve the root/body canvas's inline insets
  on their destination page, so the kept subtree shares the same containing
  block origin as an ordinary in-flow page continuation.
- Block sibling `break-before: avoid` and `break-after: avoid` rollback runs
  use the shared adjacent-box fragmentation decision to arm candidate starts,
  recognize current avoid boundaries through target-aware committed break
  opportunities, and preserve candidates for a later boundary while carrying
  both the previous sibling's authored `break-after` avoid value and the next
  sibling's authored `break-before` value instead of page-only booleans.
  Definite-height block pre-breaks, avoid-inside pre-breaks, and avoided
  sibling-run moves consume a shared whole-source prebreak decision built on
  the same cursor-bounds fragmentainer capacity value used by flex, grid, and
  table fragmentation.
  The block element path and normal block-child phase receive an explicit
  active fragmentainer kind and carry it into avoid-inside prebreak probes,
  avoid-boundary creation, avoid-run start decisions, and outgoing
  `break-after` carry state, while block-specific margin and float rollback
  state remains local to block flow.
- Multi-column block children use finite anonymous column fragmentainers and
  the shared adjacent-box break context, including parsed `column`, generic
  `avoid`, `avoid-page`, and `avoid-column` values. Modern
  `break-before`/`break-after` preserve generic avoid versus page-only and
  column-only avoid values, and effective `break-before` resolution ignores
  pending breaks for other fragmentainer kinds while letting the later
  `break-before` win over carried `break-after` at the same target boundary;
  `break-inside` likewise keeps separate page and column avoid state, and both
  column planning and block page fragmentation consume it through the active
  fragmentainer kind. Generated block-box forced `break-before`/`break-after`
  transitions now consume the same target-aware standalone box break context
  and shared `FragmentainerKind` page-cursor materialization gate used by flex,
  grid, and table wrappers.
  Avoid-run retries distinguish an occupied fragmentainer from an empty source
  fragmentainer. An empty temporary column retries only when its selected
  continuation has strictly greater block capacity; equal column geometry
  relaxes the unavoidable `avoid` preference and continues ordinary
  fragmentation rather than materializing identical anonymous columns.
  Page fragmentation still treats
  column-specific values as non-page-forcing constraints at both block
  descendants and top-level page boundary handling, and legacy
  `page-break-*` properties remain page-only.
- Same-page non-positioned `overflow:hidden`/`clip` block containers clip
  descendants to the used padding box, including auto-height blocks whose
  block-end clip edge is only known after child layout. Auto-height clipped
  blocks continue their normal-flow descendants through later pages; only a
  definite clipped block size establishes a monolithic fragmentation boundary.
  Nonzero border radii defer this clip until final paint, preserving descendant
  geometry for rounded, CSS Borders 4 `corner-shape`, and supported single
  circular `border-shape` contours. A circular `border-shape` clips
  descendants at the inner stroke edge while keeping the box's own border and
  background outside that overflow scope.
- A fixed normal-flow block in a paged vertical root retains its own logical
  block range while visible in-flow overflow continues through later page
  fragmentainers. Deferred paint is sliced in the root block direction, so a
  `vertical-rl` root progresses right-to-left and a `vertical-lr` root
  left-to-right without advancing following normal-flow siblings by the
  overflow extent.
- Normal-flow block formatting context roots next to active floats avoid by
  border box, not margin box. Auto-width roots can narrow to the float-free
  band without horizontal margins forcing placement below the float, and
  `overflow:hidden` roots are remeasured after narrowing so internal floats can
  change the root's auto height before final float-overlap validation.
- Inline-block shrink-to-fit sizing resolves percentage-height replaced
  descendants against the inline-block's own definite content height, including
  inline runs wrapped in anonymous blocks.
- Shared atomic-inline placement preserves signed logical block-start margins
  for baseline-participating inline-blocks. This keeps an all-negative margin
  set effective when the parent inline context normalizes its line paint
  anchor.
- Raster image natural dimensions are represented as CSS pixels (including
  HTML-validated EXIF density correction) before conversion to Spindrift's
  PDF-point layout unit for block replaced-element sizing.

## Remaining Gaps

- Ordinary horizontal, non-fragmented block containers trim the final
  in-flow block child's block-end adjoining margin, including a
  self-collapsing descendant margin set. Inline-axis trimming, fully logical
  writing-mode-aware trimming, and fragmentation remain incomplete; see
  `SPEC_DIVERGENCES.md`.
- Fragmented block layout still needs broader WPT coverage for margin collapse
  across page breaks, clearance, and nested formatting contexts.

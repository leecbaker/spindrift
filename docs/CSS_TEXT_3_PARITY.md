# CSS Text Level 3 Parity

This note tracks the renderer's current CSS Text Module Level 3 behavior. CSS
Text Decoration and CSS Text Level 4 features are intentionally out of scope
except where existing code already implements draft properties.

## Current Coverage

- Parser and computed-style support exists for the CSS Text 3 property surface
  used by layout: `text-transform`, `white-space`, `tab-size`, `word-break`,
  `overflow-wrap`/`word-wrap`, `line-break`, `hyphens`, `text-align`,
  `text-align-all`, `text-align-last`, `text-justify`, `text-indent`, and
  `hanging-punctuation`.
- Legacy `white-space` is represented as preservation/collapse behavior plus
  its wrapping-mode component. The inherited `text-wrap`, `text-wrap-mode`,
  and `text-wrap-style` declarations are also parsed, so an inline descendant
  can override an ancestor's `pre` or `nowrap` wrapping behavior without
  changing its whitespace preservation mode.
- Inline layout builds a shared opportunity graph for normal layout,
  fragmentation planning, page-margin/generated text, and intrinsic
  measurement.
- Text runs keep paint/background ownership while the graph records internal
  CSS Text break positions, including UAX #14 soft wraps, manual and automatic
  hyphenation, `wbr`/zero-width-space boundaries, `overflow-wrap:anywhere`, and
  `overflow-wrap:break-word` emergency opportunities.
- Min-content sizing is computed from graph opportunity metadata, preserving
  the CSS distinction between `anywhere` opportunities and `break-word`
  emergency breaks. Legacy `word-break: break-word` remains distinct from
  `overflow-wrap: break-word`: it contributes `anywhere` emergency
  opportunities to min-content sizing even when an authored `overflow-wrap`
  declaration says otherwise.
- Typographic character-unit policy is shared by graph break eligibility,
  min-content metadata, `word-break: keep-all` suppression, CJK unit-boundary
  handling, and prepared-line `text-justify: inter-character` gaps. The policy
  uses ICU grapheme clusters as the base unit and suppresses expansion inside
  joining/control sequences.
- Regular inline box edges are transparent to graph-backed CSS Text break
  discovery, so margin/border/padding edge atoms remain materialized for
  painting while line breaking sees the surrounding text.
- Cross-element text and atomic-inline boundary opportunities take their
  wrapping-mode policy from the nearest common lexical inline ancestor, rather
  than from either descendant's `white-space`. This keeps sibling `pre` spans
  from suppressing an ancestor-owned wrap, while retaining the participating
  text runs' local `line-break`, `word-break`, and `overflow-wrap` behavior.
  The typed boundary resolver uses U+FFFC only as transient UAX #14 input for
  atomics, preserves the NBSP compatibility opportunity, and suppresses GL,
  `WORD JOINER`, and ZWJ boundaries. Out-of-flow static-position placeholders
  and regular inline box edges are transparent to the surrounding text stream
  and never create U+FFFC-style opportunities.
- Horizontal mixed inline bidi reordering now operates on measured inline
  items, preserving neutral text fragments such as collapsed spaces and keeping
  regular inline box edge atoms attached to the adjacent visual content that
  owns their decoration.
- Non-text inline edges are transparent to UAX #9: both regular box edges and
  CSS Text autospace edges split the selected visual ranges and are reinserted
  at their owned visual boundary. Autospace therefore contributes its selected
  inline advance without becoming an object-replacement character that can
  alter the surrounding bidi resolution.
- Styled visual-line shaping applies the same font-neutral default-ignorable
  filtering as ordinary shaping. CGJ therefore remains available to line-break
  processing and source extraction without causing a visible Ahem or fallback
  glyph in the same cluster to select a different font.
- Compatibility-only glyph selection preserves the authored source stream and
  its byte ranges: U+2011 can select a face's U+2010 hyphen glyph without
  changing its CSS Text line-break class or PDF ToUnicode value. PDF emission
  also removes zero-used-advance, nonzero-nominal-advance shaping artifacts
  such as a font-internal ZWJ space glyph, while retaining positioned combining
  marks.
- Forced segment breaks now flush before following zero-width bidi controls,
  so an inline isolate or override begins on its own post-`br` line.
  Block-level `unicode-bidi: plaintext` instead keeps its inline stream free
  of synthetic FSI/PDI controls and resolves each selected bidi paragraph with
  UAX #9 P2/P3. This preserves contextual Arabic shaping through forced
  paragraph boundaries while `text-align: start/end` uses the same per-line
  direction. Once a mixed line has resolved UAX #9 visual order, final
  measurement and text paint shaping clear each fragment's `unicode-bidi`
  scope before applying one explicit visual-order guard. This preserves edge
  neutrals and UAX #9 L4 mirrored punctuation through empty inline spans,
  while joining scripts retain their logical shaping direction and formatting
  controls stay out of emitted glyph runs.
- Whitespace normalization is paragraph-scoped and runs before Text Level 4
  autospace insertion and graph construction. The processor owns segment-break
  transformation, cross-node collapse state, preserved tabs/spaces, forced
  breaks, visible control replacement, transparent inline edges, transparent
  page-scope markers, and segment-break transformation for the collected
  inline stream. Its typed neighboring context carries the declared writing
  system and the preceding currency-token state: Chinese, Japanese, and Yi
  source formatting joins without an inserted word separator, while currency
  amounts and Hangul retain their legal separators. Untagged text retains the
  conservative adjacent East Asian Width `F`/`W`/`H` fallback (including
  punctuation and ignoring variation selectors). The resolver finds those
  neighbors outside the complete collapsible whitespace/segment-break sequence,
  so preceding source spaces cannot mask CJK or U+200B removal context. When a
  break is removed, same-metadata runs are rejoined before shaping, preventing
  source formatting from changing kerning, wrapping, paint, or PDF extraction.
  UAX #14 `BK` and `NL` controls enter the same forced-break representation as
  explicit generated breaks, while LF and CR remain in CSS Text's separate
  segment-break transformation path.
- Language-sensitive text behavior resolves the content writing system once
  from BCP 47 language and ISO 15924 script subtags, with an explicit script
  overriding a language default. Segment-break removal and ICU line-breaking
  therefore agree that `ja-Hang` is Korean, while `en-Hrkt` is Japanese and
  `ko-Hani` is Chinese. The line-break adapter supplies ICU's canonical `ja`
  or `zh` locale only for Japanese/Chinese writing systems, preserving the
  CSS Text `normal`/`loose` U+301C and U+30A0 opportunities without extending
  them to Korean or untagged content. The targeted writing-system line-break
  and segment-break WPT regressions pass.
- CSS Tables anonymous-object fixup recognizes a whole whitespace-only inline
  sequence around table-internal children independently of its inherited
  `white-space` mode. Preserved source indentation (including indentation split
  by comments) therefore cannot create spurious anonymous table cells or row
  height in `break-spaces` tables.
- A non-percentage specified table-cell width now remains the preferred
  auto-table contribution unless the cell's min-content width is larger. This
  allows `break-spaces` to use its legal preserved-space breaks inside a fixed
  cell rather than widening an auto table to the cell's unwrapped max-content
  width.
- Preserved tab stops are re-shaped against the selected line's content edge
  before graph fitting and intrinsic measurement as well as during paint.
  Numeric `tab-size` values measure U+0020 using the nearest block container's
  font and text spacing, while the tab's own computed value selects the
  multiplier. Font matching treats glyph zero (`.notdef`) as missing coverage,
  and preserved tabs are advance-only records: they affect layout and PDF text
  positioning without painting or subsetting a synthetic missing glyph. Break
  selection therefore sees the same tab advances as the resulting line.
- Selected justified lines containing preserved tabs retain their tab-stop
  geometry rather than redistributing document spaces before those stops;
  source-range-level justification after a tab remains tracked as an open
  refinement.
- Inter-word justification records expansion eligibility per source fragment.
  An inline descendant with `text-justify: none` keeps its own separators at
  natural width while eligible separators on the parent justified line receive
  the line's remaining space.
- Unicode space separators at a selected line end retain source-range ownership
  in the graph while their advance hangs in every legacy white-space mode
  except `break-spaces`, including `pre` and `pre-wrap`. Their source remains
  in the selected paint sequence, so an inline background or decoration covers
  the hanging separator even though the line's fitting measure excludes its
  advance.
- Selected-line edge scanning treats an interleaved sequence of Unicode other
  space separators and document spaces as one Phase II sequence across
  transparent inline edges. This keeps the complete sequence out of fitting
  and intrinsic widths in legacy modes while `pre-wrap` records its preserved
  document-space advance separately; U+00A0 remains a no-break inter-word
  separator rather than a hanging edge space; `break-spaces` remains
  non-hanging.
- A collapsible document-space suffix is traversed as already removed before
  the same scan identifies a preceding Unicode other-space separator. That
  exposes the remaining visual line edge to Phase II hanging for fitting,
  alignment, and intrinsic measurement without charging the collapsed suffix
  to the separator sequence.
- Terminal preserved segment breaks stay in the shared whitespace stream for
  every newline-preserving white-space mode. The normalizer emits their forced
  empty line records after DOM and generated-content collection, so ordinary
  DOM text, generated content, intrinsic measurement, and painted lines agree
  on final `pre`, `pre-wrap`, `pre-line`, and `break-spaces` line boxes.
- Forced inline boundaries retain whether they came from a preserved segment
  break or an explicit/generated break element, so later Phase II processing
  can use source semantics rather than infer a boundary from visual position.
- Selected-line Phase II collapsing retains trailing collapsible source spaces
  through graph selection and bidi ordering, then removes only their used
  advance and visual paint geometry through transparent inline box edges.
  Decorative inline borders and padding remain materialized, while spaces in
  an otherwise-empty nested inline tail cannot shift those edges away from the
  final text content.
- When collapsible spaces from adjacent inline styles merge, their one retained
  advance retains every source style's legal wrap ownership. In particular, a
  `normal` descendant's separator can wrap after a preceding `nowrap` run
  without changing the whitespace glyph's shaping or paint ownership.
- `word-break: break-all` adds graph boundaries only between typographic letter
  units. It does not replace `break-spaces`' own after-space opportunities or
  manufacture a break beside a preserved separator; `line-break:anywhere` and
  `overflow-wrap:anywhere` retain their separate graph ownership.
- Unicode other-space separators remain visible source content. In
  `break-spaces`, their ordinary soft-wrap opportunity is after the separator;
  a before-separator break is available only when an explicit line-break or
  overflow-wrap policy supplies it. Legacy modes retain Phase II trailing
  hanging without moving the separator to a line of its own. The hanging
  suffix is excluded from fitting and alignment, but remains source for inline
  backgrounds, decorations, and extraction.
- Ordinary UAX #14 candidates are finalized only after ICU and Quire's CJK
  fallback candidates have been combined. This retains the LB13 prohibition
  on beginning a line with any `NS` Nonstarter, including ideographic and
  kana iteration marks, without weakening explicit or emergency breaks.
- Min-content graph segmentation treats ordinary UAX #14 ideographic breaks as
  eligible soft-wrap opportunities, while `word-break: keep-all` continues to
  suppress the CJK boundaries it forbids. Its UAX #14 tailoring retains the
  post-hyphen (`HY`) punctuation opportunity, which ICU's `keep-all` option
  otherwise suppresses together with word-unit boundaries.
- When `word-break: keep-all` leaves no acceptable fitting break,
  graph-emergency opportunities relax it at typographic-unit boundaries. This
  last-resort behavior remains distinct from `overflow-wrap` and does not
  contribute to min-content sizing.
- Terminal segment breaks generated by CSS Generated Content, including the
  HTML UA `br::before { content: "\A"; white-space: pre-line }` rule, use the
  same forced-empty-line representation as ordinary DOM text.
- Inline boundary semantics are centralized in an internal policy shared by
  whitespace normalization, intrinsic measurement splitting, and reusable line
  sequence construction. Bidi controls, inline box edges, and page-scope
  controls are transparent to text context, while real atoms, floats, and
  independent nested formatting contexts reset that context explicitly.
- Float markers are out-of-flow positioning participants rather than CSS Text
  soft-wrap opportunities. The graph records a distinct float-placement
  checkpoint, which lets mixed inline layout establish exclusions without
  splitting `white-space: nowrap` or an ordinary unbroken word at that source
  boundary. The selector consults legal soft-wrap opportunities separately
  before choosing its unbreakable-float path.
- When a temporary float band cannot contain a selected graph range that fits
  the unexcluded containing block, selection records each skipped physical
  line and retries that same range below the exclusion. Inline collection,
  intrinsic measurement, pagination, and paint therefore agree that the float
  consumes block-size instead of treating the range as overflow beside it.
  A skipped float row is not a formatted line: the retry retains first-line
  `text-indent` and first-line styling. This also keeps preserved `pre-wrap`
  line geometry on the common selected-line path.
- Absolutely positioned block containers isolate source float exclusions both
  while measuring their automatic block size and during final replay. Their
  own floats remain local, while a preceding normal-flow float cannot create
  zero-width line bands inside the out-of-flow formatting context.
- Float shrink-to-fit and auto-height measurement likewise run in the float's
  own formatting context before final replay. Earlier sibling exclusions
  therefore cannot inflate a later float's provisional height or leave gaps
  between consecutive inline floats.
- Selected graph ranges own CSS Text line-edge materialization for mixed inline
  layout, text-only/generated text layout, and intrinsic sizing: collapsed
  trailing spaces, `pre-wrap` hanging spaces, soft-hyphen visibility,
  zero-width-space stripping, trailing tracking, and hanging space separators
  are applied from one graph path.
- Language-resource discretionary spelling changes are resolved against both
  dictionary opportunities and authored U+00AD boundaries. The source stream
  remains unchanged until the selected line edge is materialized, so Dutch
  `cafee&shy;tje` can use `café-` / `tje` without changing unbroken text or
  extraction.
- Graph-backed `letter-spacing` retains shaper glyph selection while resolving
  all advances at final visual typographic boundaries. Terminal backend
  advances are removed from both fitting and durable glyph data; nested inline
  ownership uses the tracking-scope LCA, and UAX #9 visual order, controls,
  Arabic joining, and Indic grapheme clusters share the same boundary policy.
- Reused source-shaped slices validate that their glyph provenance covers every
  paintable selected character. A conditional-hyphen control can otherwise
  leave a truncated backend slice that under-measures the remaining source;
  incomplete slices fall back to shaping the selected range before line
  fitting and painting.
- Prepared lines retain selected source-edge metadata separately from fitting
  width. The paint copy retains both unconditional Unicode-separator suffixes
  and conditional `pre-wrap` tails, while alignment excludes their advances
  and distinguishes the two effects.
- A selected `pre-wrap` soft break retains its authored spaces in graph line
  records while excluding their advance at the visual line end after bidi
  ordering. RTL paint starts before aligned content when that logical suffix is
  first in visual order, so hanging spaces no longer shift centered, start,
  end, left, or right aligned content. This keeps ordinary alignment and RTL
  `pre-wrap` hanging-space geometry on the same graph-backed path.
- A preserved `pre-wrap` run immediately before an unconditionally hanging
  Unicode space separator now hangs with that separator even when the selected
  line ends at a forced break. The graph records this Phase II condition
  without deleting either source run.
- Terminal preserved `pre-wrap` tails are non-constraining for graph candidate
  fitting and intrinsic min-content measurement, while final-line
  materialization retains their alignment geometry. This prevents a
  shrink-to-fit float from acquiring a trailing-space-only line without
  changing right-aligned final lines.
- Graph-selected inline lines are collected into an internal
  `InlineLineSequence` that carries paragraph-local line metadata, forced
  empty lines, available widths, indents, hanging punctuation reserves, and line
  heights for fragmentation and slice painting. Normal text blocks, list item
  text, atomic inline text boxes, page-margin/generated text, inline-block
  slices, and table-cell slices now make page-fit and painting decisions from
  this shared sequence model. Split table-cell inline-like child slices also
  carry sequence-backed line records, so generated inline content and nested
  inline spans are not flattened during replay.
- Selected inline line records classify CSS Inline phantom line boxes from the
  materialized line items, preserving forced empty lines while letting phantom
  lines contribute no paint, baseline, or block-size.
- Typographic pseudo-element painting tracks the originating block container's
  first formatted line across CSS 2 block-in-inline anonymous block splits. Its
  initial anonymous inline sequence receives the originating pseudo style, and
  later anonymous inline runs do not restart the same `::first-line`.
- The originating block's first-formatted-line state is also carried into
  anonymous inline runs for `text-indent`: a later run after an in-flow block
  does not restart the parent's indent, while a real child block retains its
  own inherited indent. First-line indents contribute to max-content sizing
  but not min-content sizing, which preserves the CSS automatic minimum size
  of anonymous flex and grid items.
- Anonymous flex and grid item replay installs the assigned item content box
  as its inline formatting-context basis, so the same text-indent-aware line
  selection used for intrinsic sizing wraps against the resolved item width.
- Prepared inline static-position placeholders retain their hypothetical
  margin-box rectangle and prepared-line baseline in one shared line artifact.
  Margin-box static rectangles anchor positioned boxes directly; baseline-mode
  callers translate from the same prepared-line coordinate, keeping vertical
  and indented horizontal static positions consistent.
- Intrinsic sizing and layout estimates use sequence-backed measurements:
  inline measurements carry one `InlineLineSequence` plus graph min/max-content
  contributions, and block-size estimates, anonymous flex text, flex child
  hypothetical sizes, direct inline rows, inline-blocks, table cells, and
  shrink-to-fit calculations consume those records instead of separate line
  counters or max-content guard bands. Vertical-writing measurements expose
  physical inline/block extents from the same selected line records, including
  vertical-upright advances used by `ch`-sized reference boxes.
- Atomic inline formatting contexts, including inline-block, inline-flex, and
  inline-table atoms, resolve `width:min-content`, `width:max-content`, and
  `width:fit-content()` from their formatting-context min/max-content
  contributions while keeping `width:auto` on the shrink-to-fit path.
- Atomic inline boxes are measured and painted through the parent logical
  inline/block axes, so vertical-writing inline-block and inline-flex atoms use
  their physical height for inline progression and synthesize logical block-axis
  baselines from their margin boxes when needed. Forced inline breaks in
  vertical writing modes stack subsequent atomic-inline line boxes along the
  physical block axis, including `vertical-rl` right-to-left line progression.
- Mixed inline line metrics support CSS 2.2 `vertical-align` keywords plus
  CSS Inline 3 `dominant-baseline`, `alignment-baseline`, `baseline-source`,
  and `baseline-shift` longhands. Percentages resolve against the aligned
  element's own line-height, `middle` uses the parent x-height,
  `text-top`/`text-bottom` use parent content-area metrics, and `top`/`bottom`
  edge-aligned inline boxes contribute block-size without inflating baseline
  ascent/descent. Mixed negative-leading lines use signed baseline extents so
  the used `line-height` box can sit partly above or below the baseline without
  expanding the WPT line-box-edge cases, while `text-top`/`text-bottom` align
  child content edges to the parent content area and can increase line box
  block-size. Inline backgrounds and text paint origins use the same resolved
  parent content-edge anchor, and baseline-aligned siblings in those expanded
  lines resolve backgrounds and glyph origins from the same line placement.
  Baseline-relative shifts are applied once after baseline alignment for text
  and atomic inline boxes, so positive shifts increase the line-over extent and
  negative shifts increase the line-under extent while paint origins and line
  metrics stay in sync.
- Paint-time CSS Text adjustments are prepared through one inline line model
  for text-only, mixed, generated, page-margin, and fragmented sequence
  consumers. Alignment, `unicode-bidi: plaintext` direction state,
  justification gaps computed from boundary-shaped paint width, hanging
  punctuation reserves, trailing tracking exclusion, inline backgrounds, and
  shaped text groups are resolved before painting from the selected line
  record.
- Justification opportunities are policy-owned across mixed, text-only,
  generated, page-margin, fragmented, and vertical-writing prepared lines.
  Inter-word expansion uses CSS word separators, inter-character expansion uses
  shared typographic-unit gaps, consecutive atomic inline content is treated as
  one typographic unit, and suppressed script/control gaps are recorded by the
  policy instead of rediscovered by the painter.
- Document-font shaping applies `word-spacing` to the shared CSS word-separator
  set, including U+00A0 and other Unicode space separators, rather than only
  ASCII document spaces.
- Page-margin generated inline content is collected as structured `InlineItem`s
  and painted from fixed-box `InlineLineSequence` records with the shared
  prepared-line painter. Forced breaks, leaders, generated images, replayed
  running-element fragments, bidi/plaintext alignment, decoration, shadows,
  emphasis, writing-mode placement, and PDF text extraction now use the same
  selected-line artifact as normal inline content.
- Internal collapsible and preserved spaces adjacent to opaque atomic inline
  boxes remain prepared text fragments with normal layout advances and PDF text
  extraction summaries. Line-edge trimming, hanging spaces, soft hyphen,
  zero-width space, and trailing tracking stay graph-owned rather than being
  suppressed during paint preparation. Atomic inline break discovery also sees
  adjacent zero-advance separator text through `U+FFFC` graph context instead
  of relying on the separator's measured width. Zero-font separator fragments
  retain those break opportunities without contributing stale shaped-font
  baseline extents when their used line-height is zero.
- Prepared inline lines now use an internal logical inline-axis geometry before
  mapping fragments, text groups, atoms, backgrounds, and links to physical PDF
  coordinates. Horizontal LTR/RTL output remains the compatibility baseline,
  while vertical writing-mode indentation and line-edge placement no longer
  rely on physical-left assumptions.
- Non-replaced inline backgrounds and borders paint from the CSS inline content
  area, using selected-font content height plus owned padding and border
  extents for single-font and explicit-line-height text, and the union of
  shaped fallback-font content extents for normal multi-font runs. Inline line
  metrics keep `line-height` leading separate from that content area, including
  negative and asymmetric leading, so changing explicit `line-height` affects
  line box sizing without changing ordinary inline background height or the
  exported inline-block baseline for transparent text. Split inline edge atoms
  preserve start/end ownership, and
  ancestor inline box decorations are carried as paint-only metadata across
  descendant text fragments, so horizontal RTL bidi reordering does not collapse
  border ink into adjacent text content or leave gaps across nested inline
  children.
- Explicit `line-height` inline baselines are anchored to the style's first
  available font. Shaped fallback runs still provide glyph IDs, advances, and
  PDF font IDs, but a later fallback face selected by `unicode-range` no longer
  moves the CSS line box baseline or the text group's paint-origin adjustment.
  For `line-height: normal`, the inline line box uses the union of
  baseline-aligned selected and fallback font-run metrics, matching adjacent
  inline boxes that use those fonts as primary faces.
- Prepared text groups now place shaped glyph runs with writing-mode-aware PDF
  text matrices and computed `text-orientation`. Vertical writing maps logical
  inline advance to the physical vertical axis, supports `mixed`, `upright`,
  and `sideways` orientation policy, resolves default `mixed` placement from
  Unicode `Vertical_Orientation`, enables OpenType `vert`/`vrt2` features for
  upright vertical typographic units before placement, uses vertical advances
  for upright vertical runs, and preserves horizontal shaping and ToUnicode
  data. `writing-mode: sideways-rl` and `sideways-lr` instead force every
  typographic unit into a horizontally shaped run rotated clockwise or
  counter-clockwise respectively; they ignore `text-orientation` and do not
  select vertical forms, vertical kerning, upright `ch` metrics, or central
  vertical baselines.
- Text emphasis marks are prepared as annotations on typographic character
  units before painting. The annotation path uses Unicode skip policy,
  writing-mode-aware rendered-run placement, and normal shaped text emission
  for horizontal, vertical, generated, page-margin, and inline-block text.
- Text decoration strokes are prepared as inline-line annotations before PDF
  primitive emission. The stroke model uses logical inline axes, writing-mode
  side placement, rendered-run matrices for skip-space positioning, and shared
  solid/double/dotted/dashed/wavy emission for normal, generated, page-margin,
  inline-block, horizontal, and vertical text. Text decoration length fields
  such as thickness, underline offset, and inset preserve `ch` until selected
  font metric resolution.
- Text and box shadow geometry preserves metric-dependent lengths through
  computed style resolution, so `ch` offsets, blur radii, and spread distances
  use the selected font's zero advance before paint consumes absolute lengths.
- Background gradient stops, radii, and positions preserve `em`/`rem`, selected
  font metrics, and viewport-unit components until the owning style's computed
  font, metric, and page viewport resolution passes, leaving only percentages
  to resolve against the concrete gradient geometry.
- Inline `vertical-align` and `baseline-shift` length-percentage values preserve
  `ch` through cascade and resolve it through the selected font before inline
  baseline layout projects percentages against the element's line height.
- Initial formatting tree style construction resolves `font-size: <ch>` through
  the selected parent font's measured zero advance, so descendant font-relative
  sizing can depend on real font metrics before later layout length resolution.
  Rebuilt child-box construction, builder-owned estimate paths, DOM
  flow-helper probes, generated and typographic pseudo-element styles,
  running-element replay, and table helper probes use the same measured parent
  metric when they reconstruct descendant styles. Nested pseudo-element lengths,
  such as first-line line-height and first-letter margins, participate in the
  later selected-font metric resolution pass. The pre-font stylesheet option
  extraction path does not flatten `ch`-dependent `font-size` or `line-height`
  values into render defaults; those declarations stay on the normal measured
  cascade path. CSS math functions with comparable `ch` operands reduce while
  preserving the unresolved metric component when all other unknown components
  cancel. `min()`/`max()`/`clamp()` comparisons such as `min(10pt, 1ch)` and
  `min(10ch, 50%)` now defer branch selection through nested CSS math until
  font-metric, viewport-unit, or percentage-basis resolution.
- Arabic join controls are kept in the shaper input as invisible shaping
  controls and are stripped from visual glyph summaries. When a shaping
  backend nevertheless associates a painted glyph with a control-only or
  multi-glyph source cluster, PDF output encloses the visual glyph sequence in
  an `/ActualText` span containing the authored logical text. Synthetic join
  context remains excluded from that span. U+0640 ARABIC TATWEEL is treated as
  visible joining text, including tatweel-only inline fragments that cross
  style/font boundaries. Registered `@font-face unicode-range` descriptors
  participate in font matching for scalar, interval, and wildcard ranges while
  treating ZWJ/ZWNJ as font-neutral shaping controls. Already resolved visual
  bidi order never overrides the logical shaping direction for cursive scripts.
- Selected soft-wrap ranges retain private source-cluster provenance from the
  unbroken shaped run. Styled shapers map clusters from their synthetic
  join-context buffer back to authored source coordinates before this is
  retained. Bidi visual fragmentation composes safe scope-free glyph slices to
  preserve contextual Arabic forms; a line containing generated CSS bidi
  controls instead re-shapes its selected visual text under the internal
  unscoped paint style, preserving UAX #9 L4 punctuation mirroring without a
  second scope. This keeps source text and PDF extraction unchanged across
  transparent inline boundaries, `line-break:anywhere`, and `overflow-wrap`
  wraps. Normalized backend ranges that cannot index the source safely fall
  back to ordinary selected-range shaping.
- CSS Text edge-context smoke coverage includes inline `::before`/`::after`
  generated content with default inline display, inside text and image markers,
  generated marker segment-break transformation, page-margin forced breaks,
  page-margin `unicode-bidi: plaintext` alignment, zero-width spaces, and
  generated-content-only inline boxes preserved at split block-in-inline
  boundaries for forced-break handling, and inline-block text atoms with soft
  hyphen and zero-width-space breaks.
  Generated `leader()` content is resolved into sequence-owned measured text
  fragments before painting, including normal generated content, page-margin
  text, inline-block replacement content, RTL, and vertical-writing smoke
  coverage.
- Existing shaping, bidi, transform, justification, spacing, and hanging
  punctuation code covers common horizontal writing cases and many WPT-derived
  smoke cases. `word-spacing` accepts CSS Text Level 4 length-percentage
  values, preserving inherited percentages until they resolve against each
  element's used font size.

## Remaining Work

- Audit remaining edge-adjacent cases involving Text Level 4 autospace and
  fragmented inline boxes that combine true nested formatting contexts such as
  flex/table descendants with complex effects.
- Narrow remaining white-space divergences to verified CSS Text phase edge
  cases. The current local `css/css-text/white-space/` result is **355/412
  raw runner passes**; 9 raw failures have a matching alternative reference,
  leaving 48 cases with no matching reference. They group into preserved-space
  floats and intrinsic widths, textarea/control normalization, tabs,
  `pre-wrap` justification, and CSS Text 4 balance/clamp selection.
- Broaden script-sensitive justification coverage for remaining complex-script
  expansion cases beyond the current policy-owned cursive/control suppression.
- Audit the full CSS Fonts `unicode-range` descriptor behavior beyond the
  scalar/range/wildcard cases now used by CSS Text shaping and ALReq coverage.
- Broaden WPT coverage for `word-break: keep-all`, CJK unit-boundary
  min-content policy, emergency wrapping, hyphenation contributions, and
  sequence-backed block/flex/table estimate consumers.
- The current local `css/css-text/line-break/` result is **51/51 passing**.
  Selected `pre-wrap` and Unicode-space tails remain represented by graph
  source ranges and retain background paint while their advances are excluded
  from fitting. PDF text extraction still needs a dedicated source-range
  emission path.
- Finish fallback transformed vertical glyph forms beyond font-provided
  `vert`/`vrt2` alternates, text-emphasis collision/line-box expansion,
  full `text-decoration-skip-box`/`text-decoration-skip-self` edge cases,
  ruby, and remaining vertical or deeply fragmented complex-bidi edge placement
  for inline boxes.

## Verification

- Unit tests cover paragraph-scoped whitespace normalization, graph-backed
  intra-run UAX #14, soft hyphen, zero-width-space, `anywhere`, `break-word`,
  transparent inline box edges, selected-line materialization effects,
  partial-run materialization, line sequence pagination metadata, prepared-line
  paint adjustments, logical inline-axis geometry, shared typographic-unit
  policy, Arabic join controls, tatweel boundary grouping, `unicode-range`
  descriptor parsing, intrinsic contribution behavior, page-scope whitespace
  transparency, inside marker boundary roles, marker forced empty records,
  bidi controls around forced breaks, plaintext forced-line alignment, and
  production-path sequence replacement coverage for the retired standalone
  text breaker.
- Local smoke coverage mirrors the nine ALReq CSS Text text-encoding cases for
  `shaping-join-001/002/003`, `shaping-no-join-001/002/003`, and
  `shaping-tatweel-001/002/003` using repo-local WPT font fixtures.
- Smoke tests under `tests/smoke/text.rs` cover many CSS Text rendering paths,
  including white-space modes, `text-align*`, justification, `tab-size`,
  `text-transform`, `word-break`, `line-break`, `hyphens`, `wbr`, bidi, and
  hanging punctuation. Additional smoke coverage exercises generated inline
  content, markers, page-margin text, inline-block text atoms, and
  vertical-writing forced breaks between atomic inline boxes through the same
  graph/sequence path.
- Visual reftest comparisons should use `AGENTS/pdf_comparison.md` when adding
  or refreshing WPT-derived CSS Text cases.
- The measured 355/412 raw result above was produced with the workspace debug
  executable and the local WPT runner's
  `css/css-text/white-space/` directory.

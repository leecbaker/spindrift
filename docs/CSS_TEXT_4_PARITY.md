# CSS Text Level 4 Parity

## `word-break: manual`

Quire parses `manual` as a distinct computed value. The shared break resolver
suppresses only automatic UAX #14 `SA` (complex-context) opportunities, while
retaining authored spaces, U+200B, and forced breaks. The identical predicate
is used during inline measurement and min-content sizing, so intrinsic widths
cannot reintroduce dictionary-derived Thai and Southeast-Asian breaks that
line layout excludes:
<https://drafts.csswg.org/css-text-4/#word-boundary-detection>.

## `text-autospace`

Quire represents autospace as a non-selectable inline edge. Boundary discovery
follows adjacent base characters, so combining marks and default-ignorable
controls remain with their base text. Inline-element edges are transparent only
when their originating edge has no margin, border, or padding; a nonzero
component is a real separator even when a negative margin cancels its net
advance. The edge is transparent to bidi resolution and is
reinserted at its selected visual boundary rather than becoming a UAX #9 object
replacement. The HTML UA stylesheet disables autospace for `pre`, `code`,
`kbd`, `samp`, `tt`, `listing`, `xmp`, and `plaintext`, preserving their
preformatted grids.

Autospace advances use one-eighth of the selected font's used `ic` advance
(U+6C34 with the CSS font-metric fallback), rather than a font-size
approximation. Remaining work includes punctuation and replacement semantics,
vertical edge cases, and complete selected-line line-break ownership for
autospace edges: <https://drafts.csswg.org/css-text-4/#text-autospace-property>.

## `text-spacing-trim` and `text-spacing`

Quire represents `text-spacing-trim` as an inherited computed value and
parses the `text-spacing` shorthand atomically with `text-autospace`. `auto`
uses Quire's deterministic `normal` policy. Candidate lines are materialized
before fitting: eligible CJK opening, closing, middle-dot, colon/dot, and
ideographic-space adjacency effects select `halt` in horizontal text or
`vhal` in vertical text. The same selected materialization supplies the
committed line, paint, and intrinsic measurements, so a narrowed opening
punctuation mark can change where a line wraps.

The selector preserves source text and uses only copied fragments with changed
used glyph advances. It implements start and forced-break-start treatment,
closing-edge trimming, unconditional `trim-both` ends, `trim-all`, and the
adjacent punctuation-pair rules. CJK
colon and dot behavior is selected from the inherited language, including the
Japanese/simplified/traditional Chinese distinctions:
<https://drafts.csswg.org/css-text-4/#text-spacing-trim-property>.

## `text-wrap`

Quire parses inherited `text-wrap`, `text-wrap-mode`, and `text-wrap-style`
values needed by the local white-space tests. `text-wrap-mode: nowrap` feeds
the shared inline opportunity graph, and `text-wrap: wrap` can override the
mode introduced by legacy `white-space: pre` or `nowrap`.

`text-wrap-style: balance` now selects from the existing legal-opportunity
graph. For forced-break groups of two through ten lines, it keeps the normal
line count and searches legal break sequences using the actual remaining
inline space of every line, including per-line float exclusions and indents.
It preserves ordinary selection on a tie and does not change
`text-wrap-style: auto` or `stable` selection.

Tree-abiding block `::before` and `::after` boxes with generated `content`
are recognized as line-box-producing content during block margin-collapse
classification. Their authored margins therefore remain normal-flow spacing
when a balanced anonymous inline block follows.

`line-clamp` and the `-webkit-line-clamp` alias clamp the shared selected line
sequence with one block-global line budget, including text separated by forced
breaks and nested in-flow block contents. Child traversal carries selector
line slots rather than inferring them from margins, used block sizes, or
fragmentainer cursor movement; avoid-break replay restores the same remaining
budget. The typed computed value retains the default, suppressed, or authored
block ellipsis and legacy-WebKit provenance; the final marker is reserved
before selecting the final line and uses the clamp container's root-inline
style. Remaining work is `line-clamp:auto`, positioned/floated and
pseudo-element boundaries, and fragmentation ordering. These are required by
CSS Text 4's
clamp-before-balance ordering:
<https://drafts.csswg.org/css-text-4/#text-wrap-style>.

## `wrap-inside`

Quire supports the non-inherited `wrap-inside: auto | avoid` property on
inline boxes. The shared inline opportunity graph retains lexical inline-box
edges, so a marked box contains text, nested inline descendants, generated
inline content, and atomic inline children without incorrectly inheriting the
property. Line selection retains all otherwise legal candidates and chooses
the latest fitting candidate in the least avoided scope: an external break
first, then a break in an outer avoided box before one in a nested box. If an
avoided unit cannot fit on an otherwise empty line, the normal candidate is
used rather than overflowing it.

`wrap-before` and `wrap-after` remain unimplemented. There is currently no
upstream Web Platform Test coverage for `wrap-inside`; Quire's local parser,
graph, and Ahem-backed layout regressions cover the supported behavior:
<https://www.w3.org/TR/css-text-4/#wrap-inside-property>.

## `word-space-transform`

The inherited typed computed value and `@supports` grammar now cover `none`,
`space`, `ideographic-space`, and `auto-phrase`. During inline collection,
explicit virtual word separators from authored U+200B and the HTML `<wbr>` UA
rule are held through transparent inline edges until their neighboring context
is known, then converted to the selected replacement before graph
construction. Separators adjacent to a forced break or an independent inline
formatting context are discarded rather than expanded; eligible separators
acquire ordinary layout width and legal wrapping behavior.

Remaining work is source-range ownership for non-selectable PDF extraction,
the no-collapse guarantee when a transformed ASCII space abuts authored white
space, and language-sensitive `auto-phrase` segmentation and placement:
<https://drafts.csswg.org/css-text-4/#word-space-transform>.

## `text-transform: math-auto`

`math-auto` is parsed as its own CSS Text transform and maps the Latin,
Greek, and variant characters defined by MathML Core to their Mathematical
Italic Unicode scalars before shaping. This makes the mapping participate in
normal selected-line measurement, fallback-font selection, PDF paint, and text
extraction rather than substituting a post-layout glyph feature:
<https://w3c.github.io/mathml-core/#math-auto-transform>.

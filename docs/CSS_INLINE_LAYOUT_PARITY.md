# CSS Inline Layout Parity

Last updated: 2026-07-28

CSS Inline Layout Level 3 and CSS Pseudo-Elements Level 4 are the conformance
targets for inline line construction, typographic pseudo-elements, and initial
letters. WeasyPrint remains useful for legacy `::first-letter` extraction
comparisons, but it is not a complete `initial-letter` model.

## Current Level

- Parser, cascade, computed-style, `@supports`, CSS-wide keyword, inheritance,
  and `::first-letter` allowed-property filtering paths model
  `initial-letter`, `initial-letter-align`, and `initial-letter-wrap`.
- UA stylesheet defaults map common CJK language selectors to ideographic
  initial-letter alignment and common Indic language selectors to hanging
  alignment, with script-subtag selectors for alphabetic and ideographic
  defaults.
- `::first-letter` text extraction is applied while building the inline
  opportunity graph, so styled first letters participate in shaping, measured
  advances, line-break selection, intrinsic measurements, and collected inline
  line records instead of being only a paint-time rewrite.
- Specified `initial-letter` values compute `drop` to a sink equal to
  `floor(size)`, compute `raise` to sink `1`, preserve explicit sink values,
  and reject invalid zero, negative, fractional sink, or unknown keyword forms.
- Basic horizontal initial-letter layout sizes the selected first letter from
  the containing block line-height and available cap-height metrics, isolates
  the initial letter from ordinary line-height expansion, ignores an authored
  `::first-letter` font-size when deriving that used size.
- Ordinary source beside an oversized initial letter is positioned from the
  originating parent-strut baseline, rather than from the initial's isolated
  text-box metrics. This keeps the initial out of ordinary line-height while
  preserving the same text-group paint path for LTR, RTL, margins, and
  `::first-line` inheritance.
- Basic horizontal left-to-right exclusion for dropped initial letters reserves
  a rectangular margin-box width on subsequent lines through the existing
  page-local exclusion-band infrastructure.
- Leading preserved whitespace that is tokenized separately from the first
  typographic letter is represented as a `::first-letter` pseudo fragment. In
  horizontal LTR and RTL text its resolved tab advance and pseudo background
  block extent participate in the combined initial-letter exclusion without
  increasing the ordinary line box.
- In horizontal writing modes, initial-letter exclusions remain available to
  ordinary short following blocks, while a subsequent initial letter clears
  prior initial-letter exclusions in its page-local formatting context. The
  exclusion retains root-strut leading for line wrapping, while clearance uses
  the initial letter's actual margin-box end. This transition is distinct from
  CSS float placement and `clear`.
- Selected inline line fragments retain the page identity of their resolved
  exclusion band. Vertical replay reuses that frozen selection on the same
  page instead of re-querying an exclusion context mutated by later line
  selection; post-fragmentation replay still needs the complete vertical
  geometry model listed below.
- Block-in-inline splits preserve a relatively positioned inline ancestor's
  visual coordinate space for float-exclusion queries while retaining the
  ancestor's normal-flow geometry and single paint translation.
- Atomic inline blocks with non-visible overflow use their required
  margin-box-edge fallback baseline in both line layout and paint placement.
  That box-edge baseline is kept in CSS line coordinates rather than being
  adjusted through an internal text glyph origin.

## Needed for Parity

- Complete the distinct page-local initial-letter exclusion participant. It is
  ordered with CSS float exclusions for line wrapping but is deliberately
  excluded from CSS float placement, `clear`, and float-containment queries;
  its used geometry and lifecycle still need to cover all initial-letter
  alignment, fragmentation, and continuation rules.
- Implement `initial-letter-wrap: first` and `all` from glyph ink geometry.
  `grid` now rounds the rectangular exclusion to a shaped containing-text
  character-cell increment, but does not yet integrate glyph contours. Explicit
  length/percentage offsets now resolve against the final margin-box logical
  inline width, but do not yet combine with glyph contours.
- Complete leading-whitespace initial-letter placement in vertical writing
  modes. Horizontal LTR and RTL tabs retain their used advance and combined
  pseudo background extent.
- Complete `initial-letter-align` used-size calculations for alphabetic,
  ideographic, hanging, and border-box alignment using the required baseline
  and ink metrics for each writing system.
- Handle vertical writing modes, sideways text orientation, mixed-script
  baseline tables, and complex bidi cases where the first typographic letter
  is not at the inline-start position.
- Extend subsequent-initial clearing to vertical writing modes and relevant
  independent formatting-context roots, and replay exclusions page-locally
  across fragmentation.
- Broaden generated-content coverage for `::before`, markers, quotes,
  counters, and nested pseudo-elements so first-letter selection exactly
  matches CSS Pseudo-Elements Level 4 in every inline tree shape.
- Add WPT-style local fixtures for raised, sunken, fractional, aligned,
  wrapped, paginated, generated-content, punctuation, bidi, vertical-writing,
  and float-interaction cases.

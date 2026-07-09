# CSS Inline Layout Parity

Last updated: 2026-07-09

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
  the initial letter from ordinary line-height expansion, and applies
  block-axis sinking through a baseline shift.
- Basic horizontal left-to-right exclusion for dropped initial letters reserves
  a rectangular margin-box width on subsequent lines through the existing
  page-local exclusion-band infrastructure.

## Needed for Parity

- Replace the temporary rectangular exclusion bridge with a distinct
  initial-letter exclusion participant that is ordered with floats but is not
  represented as a float.
- Implement `initial-letter-wrap: first`, `all`, `grid`, and
  length/percentage offsets from glyph ink and grid geometry rather than the
  current margin-box approximation.
- Complete `initial-letter-align` used-size calculations for alphabetic,
  ideographic, hanging, and border-box alignment using the required baseline
  and ink metrics for each writing system.
- Handle vertical writing modes, sideways text orientation, mixed-script
  baseline tables, and complex bidi cases where the first typographic letter
  is not at the inline-start position.
- Carry initial-letter exclusions across short following blocks, clear before
  subsequent initial letters and relevant block formatting context roots, and
  replay exclusions page-locally across fragmentation.
- Broaden generated-content coverage for `::before`, markers, quotes,
  counters, and nested pseudo-elements so first-letter selection exactly
  matches CSS Pseudo-Elements Level 4 in every inline tree shape.
- Add WPT-style local fixtures for raised, sunken, fractional, aligned,
  wrapped, paginated, generated-content, punctuation, bidi, vertical-writing,
  and float-interaction cases.

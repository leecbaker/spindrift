# CSS Custom Properties Parity

Last updated: 2026-08-04

Quire resolves unregistered CSS custom properties at computed-value time using
CSS Syntax tokens. Custom-property names therefore compare as decoded,
case-sensitive identifiers, including CSS escapes, invalid Unicode escapes
that normalize to U+FFFD, and non-ASCII identifiers.

## Covered behavior

- `var()` recognition and arguments use CSS tokenization, including escaped
  function names, comments, nested component blocks, fallbacks, and EOF block
  recovery.
- A `var()` fallback is parsed and validated with its containing declaration,
  but substituted only when the primary custom property is guaranteed-invalid;
  an unresolved nested fallback therefore cannot invalidate an available
  primary value.
- Shared CSS component-value boundaries now use the same tokenization for
  shorthand whitespace, commas, slashes, conditional-rule keywords, color
  functions, and stylesheet simple blocks. Escaped function names and braces
  in comments or strings therefore do not create parser-specific boundaries.
- Custom-property values are retained as canonical component-value streams.
  CSS Syntax error tokens (bad strings, bad URLs, and unmatched closers) reject
  the declaration at specified-value time; valid CDO/CDC tokens and balanced
  simple blocks remain valid until any consuming property grammar is checked.
- Substitution retains token boundaries before the resulting property value is
  parsed, preventing adjacent substituted components from becoming a new token.
  Canonical CSS comment separators preserve internal boundaries without adding
  whitespace semantics; leading and trailing separators are removed at the
  property-value boundary. Consuming parsers read the remaining boundaries as
  CSS tokens, including quoted generated-content strings and ordered
  `font-family` lists.
- Custom-property dependency cycles are resolved before inheritance and include
  references from fallbacks.
- A shorthand containing `var()` is expanded only after successful
  substitution. An unresolved shorthand still invalidates every longhand it
  addresses at computed-value time.
- Nested style rules retain a known property declaration whose value contains
  a curly component block. An unresolved `var()` in that winning declaration
  therefore falls back according to the consuming property's computed-value
  rules instead of reviving an earlier declaration.

## Remaining work

- `@property` registration is not modeled: descriptor grammar, typed values,
  inheritance controls, and registered initial values remain unsupported.
- Full property grammar coverage after substitution is bounded by Quire's
  ordinary property parsers. Complex `background` and `font` shorthands still
  need broader CSS Variables conformance coverage.
- Source-level discovery of every supported at-rule still retains a small
  amount of raw at-keyword searching before token-aware block extraction;
  malformed stylesheets with at-rule-looking text in arbitrary non-rule
  positions need broader CSS Syntax conformance coverage.

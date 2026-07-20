# CSS Custom Properties Parity

Last updated: 2026-07-25

Quire resolves unregistered CSS custom properties at computed-value time using
CSS Syntax tokens. Custom-property names therefore compare as decoded,
case-sensitive identifiers, including CSS escapes, invalid Unicode escapes
that normalize to U+FFFD, and non-ASCII identifiers.

## Covered behavior

- `var()` recognition and arguments use CSS tokenization, including escaped
  function names, comments, nested component blocks, fallbacks, and EOF block
  recovery.
- Shared CSS component-value boundaries now use the same tokenization for
  shorthand whitespace, commas, slashes, conditional-rule keywords, color
  functions, and stylesheet simple blocks. Escaped function names and braces
  in comments or strings therefore do not create parser-specific boundaries.
- Substitution retains token boundaries before the resulting property value is
  parsed, preventing adjacent substituted components from becoming a new token.
- Custom-property dependency cycles are resolved before inheritance and include
  references from fallbacks.
- A shorthand containing `var()` is expanded only after successful
  substitution. An unresolved shorthand still invalidates every longhand it
  addresses at computed-value time.

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

# CSS Conditional Rules parity

`@supports` parses its boolean grammar before evaluation. Invalid condition
syntax therefore invalidates the rule rather than becoming a false declaration
test that can be negated. Declaration conditions keep CSS Variables'
specified-value-time behavior and use the same canonical declaration operation
that normal element and marker cascade application consumes. CSS-wide keywords,
custom properties, `var()`'s specified-value-time grammar, aliases, and
modeled shorthands therefore have one acceptance boundary.

Stylesheet-scoped `@font-face`, `@counter-style`, `@font-feature-values`, and
`@font-palette-values` resources are emitted by the same conditional-rule
parser as style rules, so only matching `@media` and `@supports` branches
register resources. `@namespace` declarations are accepted only in the
stylesheet prelude.

Remaining work includes layout-time container query matching and the CSS
Conditional `font-format()` and `font-tech()` feature-query functions.

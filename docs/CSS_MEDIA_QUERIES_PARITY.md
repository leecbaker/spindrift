# CSS Media Queries Parity

Spindrift evaluates `@media` and `@import` media lists in an explicit rendering
environment. PDF output defaults to the `print` media type; callers can select
the `screen` media type and an initial viewport through render options without
changing the default PDF semantics. The implementation preserves the Media Queries distinction
between an invalid query and a valid query that does not match, so negation
cannot accidentally activate malformed syntax.

## Implemented

- Comma-separated media query lists.
- `print` and `all` media types, with unknown types evaluating as non-matches.
- A configurable `screen` media type whose viewport derives from the initial
  page box, preserving `print` as the default PDF medium.
- Geometry-dependent queries use the renderer-provided initial page box;
  author `@page` size declarations are applied afterward and cannot feed back
  into the media-query cascade.
- `not` and `only` media type modifiers.
- Media conditions with nested parentheses and `not`, `and`, and `or`.
- Boolean `color`, `monochrome`, `width`, and `height` feature evaluation for
  the print capability model.
- Output features `update: none`, `overflow-block: paged`, and the sRGB color
  gamut, including correct rejection of invalid values for known features.
- The `forced-colors` feature, including boolean form and `none`/`active`
  values, resolved from the caller-selected forced-colors environment.
- General-enclosed fallback for unknown media features.

The WPT `css/mediaqueries/mq-gamut-*` group passes 5/5 tests with the local
WPT runner after this implementation. With the WPT screen environment
(an 800×600 CSS-pixel viewport), the complete
`css/mediaqueries/` run passes 41/43 executable tests (95.35%).

## Remaining Work

Media feature evaluation currently happens while parsing stylesheets against
the immutable rendering environment. Dynamic environments would need a durable
conditional-rule representation and a recascade when those inputs change.
`@custom-media` also needs a stylesheet-scoped definition registry and
substitution during media query evaluation.

This work should retain the current three-state result model: invalid syntax
must remain distinct from a valid non-match throughout condition evaluation.

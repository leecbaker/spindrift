# CSS Selectors WPT Parity

## Relational selector snapshots

The complete `css/selectors/has-style-sharing-*` and
`has-style-sharing-pseudo-*` reftest families pass (15/15, 2026-07-29).
Although the tests are named for browser style-sharing optimizations, Quire
cascades each element independently. Their relevant requirement is that every
reconstructed selector element preserves its source-DOM child snapshot and
that relational traversal extends the matched ancestor's path, rather than the
original descendant's path. This supports ancestor-gated descendant and
generated-pseudo rules such as `:has(> .a) .b` and
`:has(> .a) .b::after` in either source order.

Nested inline DOM collection also retains each inline source element in the
selector ancestry while it styles and lays out descendants. Direct-child rules
therefore remain direct-child rules for inline content, including during
intrinsic measurement, rather than matching a deeper inline descendant.

The `eof-right-after-selector-crash.html` and
`eof-some-after-selector-crash.html` WPTs now complete successfully. Their
linked `data:` stylesheets reach the ordinary CSS Syntax EOF-recovery path;
the default `text/plain` data-URL MIME type is ignored for stylesheet use,
while `data:text/css` is parsed normally.

## Language pseudo-class

`:lang()` preserves an unrecognized HTML language tag as a distinct value for
selector matching. Thus `lang="xyzzy"` matches `:lang(xyzzy)` and descendants
inherit that match, while the tag remains unavailable to locale-dependent text
processing. Registry-backed BCP 47 canonicalization and extlang-form
conversion are tracked in `SPEC_DIVERGENCES.md`.

## Remaining selector limitations

- Shadow/tree-scoped selectors and unmodeled UI/highlight pseudo-elements are
  not implemented.
- XML namespace selector edge cases still need broader conformance auditing.
- Dynamic form and display state remain limited by Quire's static rendering
  model.

## Specifications

- <https://drafts.csswg.org/selectors-4/#relational>
- <https://drafts.csswg.org/selectors-4/#overview>

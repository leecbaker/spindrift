# Quire patches to `usvg` 0.48.1

This directory is a source-pinned copy of `usvg` 0.48.1, licensed upstream
under Apache-2.0 OR MIT. Keep local changes small, documented, and suitable
for upstreaming.

## Retain normalized text before font layout

`src/parser/text.rs` constructs normalized `Text` nodes containing SVG text
chunks, inherited styling, character position lists, paint data, and paths.
Upstream immediately calls `text::convert`; that performs fontdb lookup and
Harfbuzz-compatible layout, then drops the node when lookup/layout fails.

Quire deliberately skips that call. Quire's document `FontSystem` is the only
authority for font discovery, `@font-face`, fallback, shaping, subsetting, and
PDF ToUnicode mappings. Keeping the upstream call would both duplicate that
work and make visible SVG text depend on the host's unrelated system-font
database.

The fork therefore retains the normalized `Text` node with empty
layout-derived fields. Consumers must not read `Text::layouted`, its computed
bounds, or `Text::flattened`; they must shape chunks through Quire instead.

The tree-wide paint-server pass is also guarded for such retained text. It
must not convert object-bounding-box paint servers using usvg's placeholder
zero text bounds, because that would erase the original fill/stroke before
Quire can map it against the shaped glyph bounds.

Desired upstream API: expose this parser/normalizer stage as an explicit,
font-independent text-retention option so Quire can remove the fork.

## Retain `text-shadow` as normalized style data

SVG text shadows are CSS text paint, not a parser-side font-layout concern.
`usvg` 0.48.1 does not recognize `text-shadow`, so this fork adds a narrowly
scoped attribute identity and retains its cascaded raw value on `TextSpan`.
Quire parses that value with its existing CSS grammar after document font
selection and output scaling are known, then realizes shadow replays from the
same shaped glyph stream.

The generated perfect-hash map is intentionally left untouched: `AId` handles
`text-shadow` as a documented explicit fast path. This keeps ordinary parser
attribute lookup byte-for-byte compatible with the pinned upstream source.

## Retain text base direction

The fork also retains inherited SVG/CSS `direction` and `unicode-bidi` values
on `Text`. Quire maps them to its document `ComputedStyle` before shaping, so
bidi run ordering uses the same Unicode text path as HTML. These are
deliberately normalized style values, not a second bidi implementation in
`usvg`.

## Retain an opaque host-typography key

Inline SVG descendants participate in the host document's CSS cascade, but
Quire owns document-font selection and shaping. The serializer marks each
inline-SVG element with a private `data-quire-text-typography-key`; this fork
retains that numeric value on normalized `TextSpan` without interpreting it.
Quire then resolves the key against a private typed style table on the owning
SVG asset.

This is intentionally not a CSS, font, or shaping extension to `usvg`. It
only preserves source identity across its normal text chunk construction, so
the parser remains the authority for SVG geometry and chunk boundaries.

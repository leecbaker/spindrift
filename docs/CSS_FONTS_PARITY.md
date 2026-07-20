# CSS Fonts WPT Parity

The renderable `css/css-fonts/` WPT run executes 312 tests. The current local
result is 248 passing tests (additional palette-definition fixes await the
next full-suite measurement).

Recent shared font-path work makes OpenType CFF custom webfonts embed as raw
CIDFontType0C programs, preserving the shaped glyph IDs in PDF consumers. The
font pipeline also supports CSS Fonts 5 `font-size-adjust` dimensionless
`calc()` values, the `ic-width` one-em fallback, selector values from
`@font-feature-values` swash entries, and run-level `@font-face size-adjust`
metadata. It also paints COLR v0 solid glyph layers into PDF paths and honors
the normal, light, dark, indexed, and named base-palette selections from CPAL.
Repeated `@font-face` sources now share decoded font-program storage while
retaining face-specific selection metadata such as `size-adjust` and
`unicode-range`; PDF output embeds a single subset for shared programs.
`ascent-override`, `descent-override`, and `line-gap-override` now change CSS
inline layout metrics without moving the native OpenType glyph coordinate
system used for PDF painting. PDF text emission converts between those systems
using the resolved CSS layout ascent minus the native program ascent, both at
the selected face's used size, so faces whose ascender is shorter than one em
do not rise into a preceding block while metric overrides still position their
native glyph programs correctly.
That conversion is retained as private rendered-line paint metadata, so
prepared inline groups and later table-baseline recovery use the exact
selected-face adjustment even when fallback runs have different
`font-size-adjust` used sizes.
OpenType per-glyph positioning offsets are retained through PDF serialization:
outline text emits offset glyphs from their shaped origins, matching the
existing bitmap and COLR paint paths without changing CSS inline advances or
line geometry.
Fixed `@font-face` `font-weight` and `font-stretch` descriptors also pin the
matching `wght` and `wdth` OpenType axis defaults before shaping, while ranges
and `auto` descriptors retain the font's intrinsic axis defaults for matching.
The `@font-face font-variation-settings` descriptor is instead retained as
selected-face metadata and applied only during the CSS Fonts variation
resolution stage, where element-level `font-variation-settings` values take
precedence; it does not alter font selection.

System generic families are resolved to one concrete, outline-embeddable face
before shaping. This keeps the face used for CSS metrics and glyph shaping
identical to the program embedded in the PDF, rather than allowing the shaping
engine's platform generic alias to select a different restricted font.

After the authored family list is exhausted, installed-font fallback is selected
through Fontique's platform backend (CoreText on macOS), with the character's
Unicode script supplied to the fallback query; Common-script emoji use the
backend's `emoji` generic. The same selector is used for shaping, direct glyph
and metric lookup, and PDF font registration. This is intentionally
platform-specific, as permitted by CSS Fonts; private-use characters do not
trigger installed-font fallback.

Font-feature precedence now follows CSS Fonts: `@font-face` defaults yield to
`font-variant-ligatures`, nonzero letter-spacing disables optional ligature and
contextual features, and `font-feature-settings` has final override priority.
Kerning selects `kern` for horizontal and sideways typographic ranges and
`vkrn` for upright vertical ranges, explicitly disabling the inactive feature
so the shaping backend's horizontal kerning default cannot leak into vertical
text. Mixed `text-orientation` applies vertical glyph-form features to `U`,
`Tu`, and transformed-rotated (`Tr`) typographic units, while only `U` and
`Tu` use upright placement.
Control-only fallback runs and simple selected-face fragments containing
ZWJ/ZWNJ preserve the shaping control while omitting its fallback PDF payload;
the two CSS Fonts feature-resolution reftests pass pixel-identically.

`font-variant-emoji` uses Unicode 15.1's registered emoji variation-sequence
bases rather than an approximate emoji range. This preserves authored VS15 and
VS16 selectors while applying the requested default presentation to keycap
bases, including `#`, `*`, and the ASCII digits; `font-variant-emoji-005`
passes.

## Largest remaining clusters

- Remaining COLR/CPAL v1 paint graphs, CSS `override-colors` edge cases, and
  per-inline-run palette inheritance.
- Variable-font axis instancing before PDF embedding.
- Complete `@font-face size-adjust` metric and fallback behavior.
- Platform-specific standard and UI generic font-family conformance.
- Remaining `unicode-range` and `size-adjust` metric/fallback interactions.

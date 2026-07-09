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
Control-only fallback runs and simple selected-face fragments containing
ZWJ/ZWNJ preserve the shaping control while omitting its fallback PDF payload;
the two CSS Fonts feature-resolution reftests pass pixel-identically.

## Largest remaining clusters

- Remaining COLR/CPAL v1 paint graphs, CSS `override-colors` edge cases, and
  per-inline-run palette inheritance.
- Variable-font axis instancing before PDF embedding.
- Complete `@font-face size-adjust` metric and fallback behavior.
- Platform-specific standard and UI generic font-family conformance.
- Remaining `unicode-range` and font metric override cases.

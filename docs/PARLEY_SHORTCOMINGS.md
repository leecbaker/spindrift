# Parley Shortcomings

## Raw OpenType language-system overrides

CSS `font-language-override` is a four-character, case-sensitive OpenType
language-system tag. It is distinct from the document's BCP-47 language:
the former is consumed only by OpenType shaping, while the latter continues to
drive CSS language-sensitive behavior, fallback, and accessibility.

Parley's current `StyleProperty::Locale` accepts a BCP-47 language value. The
Fontique/Parley path normalizes that value and discards the raw OpenType tag,
so it cannot express CSS `font-language-override` faithfully. Converting tags
such as `trk` to a locale or to HarfBuzz's private-use language form is not a
valid substitute: the CSS tag's bytes and case are observable by OpenType
feature selection.

Parley should expose a second, shaping-only style property carrying an
unaltered `[u8; 4]` OpenType language-system tag. Its HarfBuzz shaping-plan
construction must consume those bytes directly, while `Locale` remains the
separate BCP-47 input. That preserves both CSS language semantics and raw
OpenType language-system semantics without an application-side workaround.

Quire preserves authored tag case and maps the WPT's lower-case `"trk"`
through Parley's BCP-47 `trk` locale. HarfBuzz maps that locale to no OpenType
language-system tag, so `css/css-fonts/font-language-override-03.html` passes.
That compatibility case is not a general solution: a font-defined arbitrary
lower-case or otherwise non-BCP-47-mappable tag still requires the raw-tag API
above. The relevant CSS requirement is
<https://drafts.csswg.org/css-fonts/#font-language-override-prop>.

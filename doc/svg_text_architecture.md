# SVG text architecture

## V1 behavior

Quire uses upstream `usvg` to normalize and lay out SVG `<text>` elements.
On native targets, `usvg` receives one lazily initialized system-font database
and lowers SVG text into its ordinary vector paint scene. Quire paints that
scene as paths; it does not submit SVG text to the HTML `FontSystem`.

Each non-empty flattened SVG text element is wrapped in one tagged-PDF
`/ActualText` span containing its normalized chunks in document order. This
keeps outlined SVG text searchable and accessible without adding an invisible
native-text duplicate. HTML text continues to use Quire font selection,
embedding, subsetting, ToUnicode mappings, and native PDF text operators.

This is intentionally a visual fallback. SVG text can select a different font
than surrounding HTML/CSS when the requested font is not available in the
native system database. It also has the limitations of upstream `usvg` text
flattening, including its best-effort color and bitmap glyph support.

## Boundaries

```text
HTML/CSS text  --> Quire FontSystem --> embedded native PDF text
SVG <text>     --> upstream usvg     --> SVG paths/images + /ActualText
```

SVG text therefore does not create document-font records or PDF `BT` text
operations. SVG paint, clipping, masking, filtering, opacity, and group paint
order continue through Quire's ordinary SVG scene adapter.

## Target support and omissions

Native targets load system fonts once for SVG text. Wasm builds remain
supported, but have no system-font discovery and therefore omit SVG text until
Quire exposes an explicit SVG font-source API.

Host-CSS SVG presentation serialization remains in place for standard SVG text
properties that `usvg` supports. Host-CSS `text-shadow` on SVG text is not
supported in v1; HTML/CSS text shadows are unaffected.

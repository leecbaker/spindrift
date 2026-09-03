# CSS Color parity

## Implemented

- CSS Color 4 legacy and modern `rgb()`/`hsl()` syntax, `hwb()`, hexadecimal
  notation, named colors, alpha clamping, and comments as CSS whitespace in
  color functions.
- Predefined `color()` spaces: `srgb`, `srgb-linear`, `display-p3`,
  `display-p3-linear`, `a98-rgb`, `prophoto-rgb`, `rec2020`, `xyz`,
  `xyz-d50`, and `xyz-d65`. Spindrift owns CSS coordinates and alpha, using
  tagged RGB and D50-PCS coordinate variants. Each RGB variant carries a
  distinct private space marker, and raw coordinates are private. XYZ,
  Lab/LCH, and Oklab/Oklch normalize to unbounded D50 XYZ coordinates.
- Ordinary PDFs emit vector paints and CSS gradients in generated ICCBased
  color spaces. A direct paint that is representable in sRGB is canonically
  emitted as tagged 8-bit sRGB; every other direct paint is converted to the
  fixed Display-P3 ordinary-PDF output condition. PDF resource planning uses
  that final direct-paint condition, so every named color space selected by a
  content stream is declared as an ICCBased page resource. Each native
  gradient selects one final PDF RGB condition for every stop and its shading
  dictionary: sRGB when all stops fit, otherwise Display-P3. CSS interpolation
  still resolves through its selected CSS space before that PDF boundary.
  PCS-derived direct paint takes sRGB when it fits and otherwise Display-P3,
  without intermediate sRGB clipping. PDF/A converts vector paint and
  generated gradients to tagged sRGB, supplies an sRGB OutputIntent, and
  retains only the required sRGB ICC resource.
- Embedded RGB ICC profiles in PNG and JPEG images are retained as ICCBased
  image spaces in ordinary PDFs. Fully opaque, uniform decoded samples can
  use that same calibrated page color space as a vector fill; PDF/A transforms
  decoded samples to its tagged sRGB output condition before the same
  promotion decision. Missing, invalid, and non-RGB source profiles use the
  explicit sRGB input boundary.
- moxcms is used at ICC boundaries: embedded-profile parsing and validation,
  embedded-raster transforms, and generated ICC bytes. CSS predefined-space
  conversion uses the CSS Color 4 matrices and transfer functions directly,
  preserving extended-range components until the selected PDF output boundary.
  PDF serialization consumes final `PdfPaintColor` samples, and generated
  raster images use encoded RGB samples rather than CSS coordinates.

## Conversion ownership

- Palette owns the standard, typed D50 XYZ ↔ Lab and D65 XYZ ↔ OKLab
  transforms. Palette's D50 white point uses CSS Color's exact
  `0.96422 / 1 / 0.82521` reference values, and its unchecked conversions
  preserve CSS extended-range coordinates. Spindrift adapts only at the explicit
  D50/D65 boundary.
- Spindrift owns CSS-specific conversion behavior: the CSS D50/D65 Bradford
  matrices, LCH/OKLCH polar syntax, predefined RGB matrices and signed
  transfer curves, HSL/HWB grammar, missing-component replacement, polar hue
  interpolation, alpha premultiplication, and output gamut policy. Palette's
  generic spaces do not encode those CSS parsing and output rules. Generated
  gradient raster fallbacks retain their interpolation calculation through
  sampling, then choose one RGB image encoding for the whole tile (sRGB when
  representable, otherwise Display-P3); D50 PCS coordinates are never emitted
  directly as three-component PDF image samples.
- moxcms remains limited to ICC work: parsing/validating profiles,
  embedded-profile raster transforms, and ICC byte generation. It is not used
  for CSS predefined-space conversion, where ICC rendering intents and
  profile precision would change CSS-defined extended-range results.
- CSS Color 5 `color-mix()` across Spindrift's supported gradient interpolation
  spaces and polar hue routes. The computed result retains its selected CSS
  space; percentage normalization and premultiplied alpha do not force an
  sRGB conversion.
- CSS Color 5 single-argument `contrast-color()`, selecting black or white by
  WCAG relative-luminance contrast.
- `currentcolor` values remain deferred through computed border, outline,
  background, gradient, and shadow values, then resolve at the fragment-local
  used-color boundary. This includes `::first-line` inline text, edge atoms,
  and ancestor-decoration snapshots. The covered CSS Color 5 relative-color
  forms with a `currentcolor` origin resolve against each element's own
  computed `color`; relative RGB and HSL preserve extended-range target-space
  components.
- CSS Color 5 `light-dark()` resolves color and image branches from each
  consuming element's used color scheme. Image branches resolve before
  `image-set()` candidate negotiation; `none` becomes a transparent generated
  image with no natural size.
- Deterministic print-palette values for CSS system colors and their deprecated
  aliases, so aliases compare consistently within a generated PDF.
- CSS Color Adjustment forced-colors used values, including configurable light
  and dark palettes, preserved authored system-color references, and
  `forced-color-adjust` inheritance for HTML and inline SVG presentation.

## Remaining work

- Apply CSS Color 4 local-MINDE gamut mapping at physical-output boundaries.
- Extend profile-aware paint to patterns and SVG image decoding; add CMYK
  image conversion, custom output ICC, DeviceN, and spot-color workflows.
- Model relative colors as typed computed values for every origin, component
  expression, missing component, alpha, and color space.
- Extend `color-mix()` to the complete interpolation-space and hue-mode
  grammar; add configurable `contrast-color()` algorithms and argument forms,
  configurable `color-scheme`, ICC `@color-profile` spaces, and platform-aware
  system colors.
- Add CSS Color 4 interpolation spaces and hue interpolation to gradients and
  animation.
- Implement the remaining forced-colors backplate and script-driven behavior,
  plus the unimplemented Color Adjustment properties beyond
  `forced-color-adjust`.

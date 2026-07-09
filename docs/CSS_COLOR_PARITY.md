# CSS Color parity

## Implemented

- CSS Color 4 legacy and modern `rgb()`/`hsl()` syntax, `hwb()`, hexadecimal
  notation, named colors, alpha clamping, and comments as CSS whitespace in
  color functions.
- Predefined `color()` spaces: `srgb`, `srgb-linear`, `display-p3`,
  `display-p3-linear`, `a98-rgb`, `prophoto-rgb`, `rec2020`, `xyz`,
  `xyz-d50`, and `xyz-d65`. Direct vector paints retain their CSS RGB space;
  XYZ, Lab/LCH, and Oklab/Oklch normalize to unbounded D50 XYZ coordinates.
- Ordinary PDFs emit direct vector paints and CSS gradients in generated
  ICCBased color spaces. A same-space gradient stays in its authored CSS
  space; mixed-space gradients resolve through D50 XYZ. PDF/A converts vector
  paint and generated gradients to tagged sRGB, supplies an sRGB OutputIntent,
  and retains only the required sRGB ICC resource.
- Embedded RGB ICC profiles in PNG and JPEG images are retained as ICCBased
  image spaces in ordinary PDFs. PDF/A transforms those decoded samples to its
  tagged sRGB output condition; missing, invalid, and non-RGB source profiles
  use the explicit sRGB input boundary.
- CSS Color 5 two-color `color-mix()` in sRGB and LCH, including percentage
  normalization and premultiplied alpha in sRGB.
- CSS Color 5 single-argument `contrast-color()`, selecting black or white by
  WCAG relative-luminance contrast.
- `currentcolor` background values retain their deferred resolution through
  inheritance. The covered CSS Color 5 relative-color forms with a
  `currentcolor` origin resolve against each element's own computed `color`.
- `light-dark()` selects its light branch in Quire's fixed light print scheme.
- Deterministic print-palette values for CSS system colors and their deprecated
  aliases, so aliases compare consistently within a generated PDF.

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

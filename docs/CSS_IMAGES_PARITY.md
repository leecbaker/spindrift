# CSS Images WPT Parity

The non-script `css/css-images/` WPT selection executes 411 tests in the local
PDF runner, including SVG-bearing cases; 53 script tests are excluded. The
latest verified run (2026-07-13) passes 345/411 (83.9%). SVG behavior is now
part of this parity measure rather than a separate raster-only baseline.

## Implemented in this pass

- Replaced raster images now share concrete-object sizing for `fill`,
  `contain`, `cover`, `none`, and `scale-down`, with a parsed, inherited
  `object-position`. Raster `<img>`, `<embed>`, `<object>`, and video poster
  resources use the same layout path. Concrete objects are cropped through the
  common raster paint-area mapping, preserving source-pixel behavior shared
  with background images. `object-position` also applies to `fill`, including
  edge offsets when the concrete object nominally matches the content box.
- SVG replaced images use that same concrete-object geometry. `fill`,
  `contain`, `cover`, `none`, and `scale-down` therefore share source-
  independent sizing, `object-position`, and content-box clipping across
  raster and vector image sources.
- SVG gradients preserve padded endpoint colors and coincident color stops as
  PDF stitching-function discontinuities. Their paint-server matrix is
  composed with the path viewport transform, keeping gradients aligned with
  transformed SVG geometry.
- SVG `pattern` paint servers containing supported solid vector paths are
  emitted as native PDF Type 1 tiling patterns. Their SVG user-space placement
  is composed with the target path transform, so transformed inline SVG shapes
  retain their pattern alignment without expanding a repeated cell into page
  primitives.
- Inline SVG descendants receive the host document's cascaded, resolvable CSS
  transforms before SVG parsing, so document rules correctly override an SVG
  `transform` presentation attribute without modifying the source HTML DOM.
- `image-rendering: pixelated` / `crisp-edges` is propagated to ordinary
  background-image tiles as well as repeated patterns and replaced raster
  images, so all of those PDF image consumers request non-interpolated
  sampling consistently.
- `image-set()` selects the appropriate supported candidate for Quire's
  1dppx output environment in CSS backgrounds and generated content. Its
  selected resolution scales raster and SVG intrinsic dimensions; it supports
  URL/string sources, the standard resolution units, basic `calc()`
  arithmetic, and supported MIME descriptors.
- The image store keeps raw and EXIF-oriented versions of a raster source as
  distinct resources, preserving both correct intrinsic dimensions and PDF
  image deduplication. `image-orientation: from-image` is the initial,
  inherited behavior; `none` can select the raw source for replaced images
  and ordinary backgrounds.
- CSS Images one-stop linear/radial gradients now use the specified two-end
  color-line endpoints. Spatially uniform one-stop conic gradients share the
  existing generated solid-gradient path.
- General conic-gradient parsing and generated-image rasterization support
  angular color stops, `from` angles, `at` positions, and repeating periods.
- CSS Images Level 4 `image(<color>)` is modeled as a generated color image.
  At paint time it uses the common background geometry (size, position,
  repetition, and clipping) and emits a vector fill; `currentcolor` remains
  symbolic until that used-value stage.
- Spatially uniform linear and radial CSS background gradients are emitted as
  clipped PDF solid paths. Fully repeating uniform layers cover their resolved
  positioning area with one clipped path, so near-zero background sizes do not
  expand into one paint operation per tile. Opaque linear and radial
  backgrounds use native PDF axial/radial shadings at their resolved tile
  geometry, including affine ellipse transforms for radial gradients.
  Repeating linear/radial color lines, transition hints, coincident hard
  stops, and alpha masks use the same vector shading path. Repeating color
  lines are one periodic Type 4 calculator function per color line (plus a
  paired alpha function when needed), so PDF size is independent of the
  number of internal repeats. Zero-period and physically unresolvable repeats
  use the CSS gradient-average color. Repeated layers use a shared PDF tiling
  cell containing that shading.
- Repeated URL SVG backgrounds paint one vector Form XObject inside a native
  PDF tiling pattern, using the same size, position, repeat step, and outer
  clip as raster background tiles. CSS URLs in inline `style` attributes are
  preloaded alongside stylesheet URLs, so file-backed SVG backgrounds are
  available during layout.
- Generated gradient raster fallbacks are created after `background-size`,
  `background-position`, and repeat geometry resolve. Their RGB and alpha PDF
  image streams use Flate compression; unsupported fallback images therefore
  have the used tile dimensions rather than page-sized dimensions.
- Atomic inline boxes use the same background-image primitive pipeline as
  block, flex, grid, table, and replaced boxes, so raster and generated
  background images retain normal CSS painting order in `inline-block` and
  related formatting contexts.

## Highest-impact remaining groups

- Remaining conic-gradient grammar and raster parity, including interpolation
  methods, mixed `calc()` angle/percentage expressions, and advanced position
  units.
- CSS Color 4 gradient interpolation spaces, hue interpolation, `none`, and
  `currentcolor` color stops.
- Native PDF representations for conic gradients. Conic gradients retain
  correctness-preserving tile-sized raster fallbacks; linear and radial CSS
  gradients use PDF shadings, periodic calculator functions, and soft masks.
- CSS Images 4 `object-view-box`, including `rect()` and `xywh()` source
  selection before concrete-object sizing.
- `contain-intrinsic-width` and `contain-intrinsic-height` fallback sizing
  for size-contained replaced elements.
- Complete image-orientation plumbing for border images, list markers, masks,
  generated images, and image documents.
- A nearest-neighbor raster fallback for `image-rendering: pixelated` where a
  PDF reader does not honor `/Interpolate false`; the PDF sampling hint alone
  does not satisfy the mixed-scale WPT.
- Full `image-set()` option grammar, dynamic device resolution, MIME
  negotiation, and expression evaluation.

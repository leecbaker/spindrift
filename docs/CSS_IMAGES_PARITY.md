# CSS Images WPT Parity

The non-script `css/css-images/` WPT selection executes 411 tests in the local
PDF runner, including SVG-bearing cases; 53 script tests are excluded. The
latest verified run (2026-07-13) passes 345/411 (83.9%). SVG behavior is now
part of this parity measure rather than a separate raster-only baseline.

## Implemented in this pass

- Fully opaque decoded raster images with one final RGB sample are emitted as
  calibrated PDF vector fills rather than image XObjects. The promotion occurs
  after cropping, orientation, and output-color conversion, retains the same
  built-in or embedded ICC-based color components and destination geometry,
  and is disabled for JPEG passthrough, transparency, non-uniform samples,
  transformed images, and repeated image patterns. This avoids PDF
  image-sampling edge artifacts without changing CSS image sizing or ordinary
  raster-image semantics.
- Replaced raster images now share concrete-object sizing for `fill`,
  `contain`, `cover`, `none`, and `scale-down`, with a parsed, inherited
  `object-position`. Raster `<img>`, `<embed>`, `<object>`, and video poster
  resources use the same layout path. Raster background and replaced-image
  clipping retain each image's full source-to-destination mapping through
  destination-space clips, including fractional source-pixel edges.
  `object-position` also applies to `fill`, including edge offsets when the
  concrete object nominally matches the content box.
- SVG replaced images use that same concrete-object geometry. `fill`,
  `contain`, `cover`, `none`, and `scale-down` therefore share source-
  independent sizing, `object-position`, and content-box clipping across
  raster and vector image sources. Ratio-only SVGs retain their absent natural
  axes during `none` and `scale-down` sizing, so their concrete object uses
  the content box's default-size contain fit rather than SVG's 300×150 parser
  viewport fallback. Their viewport coverage remains SVG scene geometry until
  painting, rather than being replaced with a CSS solid rectangle; this
  preserves source-coordinate extent, clipping, and edge coverage. The
  clipped concrete-object intersection is converted once from Quire's
  bottom-left paint coordinates to SVG's top-left source viewport, preserving
  the same `object-position` source selection as raster images.
- The UA stylesheet defaults HTML image-like replaced elements to
  `overflow: clip` with a `content-box` clip margin. This preserves CSS
  Images' default object-fit crop while allowing an author to opt into
  concrete-object overflow with `overflow: visible`.
- URL SVG background paths receive the CSS `background-clip` even when their
  own redundant root-viewport clip has been elided. Fully non-repeating tiles
  instead crop the root SVG source viewport to their resolved visible
  destination before PDF path emission, avoiding a separate rectangular PDF
  clip and its device-pixel edge seam. Positioned tiles therefore cannot leak
  outside the selected background painting area and retain the same visible
  edge geometry as equivalent replaced SVGs.
- CSS Images 5 `object-view-box` resolves source selection before
  concrete-object sizing and uses the same mapping for inline, block, raster,
  and SVG replaced content. Empty, negative, and inapplicable source bounds
  leave the natural source unchanged.
- A single CSS `content` image now retains a typed replaced-image payload
  through sizing and paint: raster, SVG, and generated linear/radial
  gradients share the used content and border-box geometry. Gradients have no
  natural dimensions, so automatic sizing negotiates the CSS 300px by 150px
  default object size rather than a font-relative square. Eligible generated
  linear and radial replacements use the same native PDF shading program as
  backgrounds; conic gradients, non-native interpolation methods, and
  object-fit/view-box mappings that cannot yet be represented exactly retain a
  tile-bounded raster fallback.
- A sole image in generated `::before` or `::after` content retains the
  pseudo-element's own box and decorations while its anonymous image payload
  paints at its zoomed intrinsic size. This deliberately follows the
  interoperable legacy pseudo-content behavior; ordinary element-level
  `content: <image>` remains a replaced element and continues to use its
  declared sizing and `object-fit` behavior.
- HTML `width` and `height` presentation attributes for image-like replaced
  elements are emitted as zero-specificity author-origin cascade hints. This
  preserves author-CSS precedence for `img`, `embed`, `iframe`, `object`,
  `video`, and image-button inputs, and paired `img`/`video` dimensions also
  supply HTML's `aspect-ratio: auto w / h` hint.
- SVG gradients preserve padded endpoint colors and coincident color stops as
  PDF stitching-function discontinuities. SVG geometry and paint-server
  transforms are materialized independently into page paint space, preserving
  transformed marker `context-fill`/`context-stroke` continuity with the
  context element's bounding box without relying on a local PDF path CTM.
- SVG `pattern` paint servers containing supported solid vector paths are
  emitted as native PDF Type 1 tiling patterns. Their SVG user-space placement
  is materialized in page paint space independently of the target path, so
  transformed inline SVG shapes retain their pattern alignment without
  expanding a repeated cell into page primitives. Inline SVG serialization
  preserves the SVG 2 null-namespace `href` independently of legacy
  `xlink:href`, so the modern reference wins when both are specified.
- Inline SVG descendants receive the host document's cascaded, resolvable CSS
  `transform`, `fill`, `stroke`, and non-percentage `stroke-width` values
  before SVG parsing, so document rules correctly override SVG presentation
  attributes without modifying the source HTML DOM.
- `image-rendering` retains all CSS Images keywords through cascade and into
  PDF resource preparation for raster backgrounds, border images, replaced
  and generated content, and list markers. `pixelated` uses its required
  nearest-integer-plus-smooth sequence, while `crisp-edges` is nearest
  neighbor; sampling is materialized on the configured static device grid
  rather than delegated to a PDF viewer's optional interpolation hint.
- `image-set()` retains typed candidates through cascade and chooses a
  quality-first option for `RenderOptions`' configured device density (1dppx
  by default). It supports the standard and `-webkit-` spellings, URL/string
  and implemented generated-image sources, unordered descriptors, MIME
  filtering, source-order duplicate-resolution removal, intrinsic-resolution
  scaling, and dimension-aware `calc()` arithmetic. An exhausted valid set is
  retained as an invalid image, allowing property-specific fallback such as
  normal border painting.
- Raster metadata keeps encoded sample dimensions distinct from preferred CSS
  natural dimensions. JPEG EXIF density corrects the latter only when all
  fields pass HTML's complete inch-unit, positive-value, exact-consistency
  predicate; invalid, incomplete, centimetre-unit, and JFIF-only metadata use
  the source-pixel-count fallback. PDF image resources retain their original
  sample dimensions.
- The image store keeps encoded and metadata-oriented versions of a raster
  source as distinct resources, preserving natural dimensions, border-image
  slicing, pixels, and PDF deduplication. `image-orientation: from-image` is
  the initial, inherited behavior. `none` selects encoded pixels only for
  origin-clean sources; opaque no-CORS cross-origin images remain
  metadata-oriented. PNG `eXIf` metadata found after `IDAT` affects neither
  orientation nor preferred natural dimensions, while indeterminate placement
  retains decoder metadata as CSS Images requires. Legacy PNG `zTXt` `Raw
  profile type exif` metadata is ignored.
- GIF raster images are supported by every existing raster-image consumer.
  Animated GIFs render their first frame on the logical screen as a static
  PDF image; animation timing, looping, disposal, and later frames are not
  evaluated.
- WebP raster images are supported by every existing raster-image consumer.
  Animated WebP images render their first composited frame on the logical
  screen as a static PDF image; animation timing, looping, disposal, and later
  frames are not evaluated.
- JPEG XL raster images are supported by every existing raster-image consumer.
  Their intrinsic dimensions, rendered RGB/RGBA samples, ICC profile, and EXIF
  orientation are decoded through `jxl-oxide`. Integer 8-bit sources remain
  8-bpc; integer 9--16-bit PNG/JPEG XL sources retain 16-bpc RGB and alpha
  samples through PDF emission. Floating-point/HDR JPEG XL is not supported.
  The JPEG XL WPT reftest subset currently passes 20 of 30 renderable tests;
  the remaining parity gaps are tracked in `SPEC_DIVERGENCES.md`.
- CSS Images one-stop linear/radial gradients now use the specified two-end
  color-line endpoints. Spatially uniform one-stop conic gradients share the
  existing generated solid-gradient path.
- General conic-gradient parsing and generated-image rasterization support
  angular color stops, `from` angles, `at` positions, and repeating periods.
- CSS Images Level 4 `image(<color>)` is modeled as a generated color image.
  At paint time it uses the common background geometry (size, position,
  repetition, and clipping) and emits a vector fill; `currentcolor` remains
  symbolic until that used-value stage.
- CSS Images Level 5 `image()` accepts one optional string/`url()` source,
  one optional fallback color, and an optional leading `ltr`/`rtl` source
  tag. URL request modifiers and document/page URL rebasing are retained in
  the shared `ImageUrl` value. The source is resolved once at layout time;
  a failed source selects its color fallback, while a source-only failure
  retains the consumer's existing invalid-image behavior. Generated content,
  backgrounds (including page backgrounds), border images, and list markers
  all use that shared source/fallback selection.
- `image()` raster sources support the required integer
  `#xywh=x,y,width,height` Media Fragments form. Partially overlapping source
  rectangles clamp to the raster grid, change natural image dimensions, and
  are retained as PDF source rectangles for both ordinary and repeated
  background paint. Invalid fragments select the `image()` fallback color.
  Existing SVG view fragments continue to use the SVG asset path. The legacy
  WPT fallback-chain cases `003` and `004` are invalid under the current
  CSS Images 5 grammar; `005` also has an incorrect opaque-green reference
  for its alpha-blue-over-green composition.
- CSS Color 5 `light-dark()` image values retain typed light and dark branches
  through parsing, then select the owning element's used-scheme image before
  `image-set()` negotiation, layout, resource loading, and paint. A `none`
  branch is represented as the transparent generated image required by CSS.
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
  clip as raster background tiles. Their root SVG viewport is specialized to
  the resolved background-image size before vector painting, preserving root
  percentage geometry and `preserveAspectRatio` behavior. CSS URLs in inline
  `style` attributes are preloaded alongside stylesheet URLs, so file-backed
  SVG backgrounds are available during layout.
- Eligible unchanged 8-bit RGB JPEG URL images retain their original
  `/DCTDecode` data in the PDF, including inside repeated background patterns.
  PDF/A limits this path to untagged sRGB sources; cropped,
  orientation-adjusted, generated, and tagged/color-converted images use
  decoded sample streams instead.
- Generated gradient raster fallbacks are created after `background-size`,
  `background-position`, and repeat geometry resolve. Their RGB and alpha PDF
  image streams use Flate compression; unsupported fallback images therefore
  have the used tile dimensions rather than page-sized dimensions. CSS
  interpolation coordinates are converted into one RGB storage space per tile
  before 8-bit image encoding: in-gamut tiles use sRGB and other tiles use
  Display-P3, never a D50 XYZ profile paired with RGB image bytes.
- CSS Images 4 gradient preludes accept all rectangular and polar interpolation
  spaces for linear, radial, and conic gradients. Encoded rectangular spaces
  keep their native PDF shadings; Oklab, Lab, linear-light, and polar methods
  use the shared ICC-tagged raster sampler so PDF interpolation cannot change
  the CSS color path. Gradient stops retain `none` components and symbolic
  `currentcolor` through used-value resolution. Modern HSL/HWB components
  accept percentages, 0--100 reference-range numbers, and `none`; missing and
  powerless hue components are resolved immediately before premultiplied
  interpolation.
- Atomic inline boxes use the same background-image primitive pipeline as
  block, flex, grid, table, and replaced boxes, so raster and generated
  background images retain normal CSS painting order in `inline-block` and
  related formatting contexts.

## Highest-impact remaining groups

- Remaining conic-gradient grammar and raster parity, including mixed `calc()`
  angle/percentage expressions and advanced position units.
- Native PDF representations for conic gradients. Conic gradients retain
  correctness-preserving tile-sized raster fallbacks; linear and radial CSS
  gradients use PDF shadings, periodic calculator functions, and soft masks.
- `contain-intrinsic-width` and `contain-intrinsic-height` fallback sizing
  for size-contained replaced elements.
- Complete image-orientation plumbing for masks, SVG, and image documents.
- CSS Values Math functions beyond `calc()`, `min()`, `max()`, `clamp()`,
  and statically computable `sign()` expressions in `image-set()` resolution
  descriptors.

# CSS Shapes WPT Parity

## Baseline

The local non-script `css/css-shapes/` WPT selection contains 226 tests: 222
reftests and four crashtests. The cache generated on 2026-07-19 passes 11/226
(4.9%) and fails 215/226 (95.1%); no test errors or skips were recorded.

The baseline passing cases are not feature coverage: they are one standalone
`polygon()` rounding test, crash tests, and cases whose expected output is
compatible with the ordinary rectangular float area. It predates the
Milestone 1 implementation below, when `shape-outside` was not parsed or
carried by `ComputedStyle` and `FloatShape` stored only a `PageTopRect`.

An isolated Spindrift-only remeasurement on 2026-07-20 passes 85/226 (37.6%) and
fails 141/226. This includes 22 circle tests, 21 ellipse tests, 20 shape-box
tests, and the float-adjacent forced-break cases that retain their empty line
boxes. The 15-test gain comes from `shape-margin` for boxes, circles, and
ellipses. Vertical-writing contour placement, complex inset layout, and image
sources remain the major gaps.

The current audited composite re-runs every family affected since that full
measurement: circles pass 28/38, ellipses 27/35, and image/gradient shapes
19/43. Replacing those baseline family results yields **116/226 (51.3%)**.
This clears the 50% CSS Shapes milestone criterion. The WPT artifacts are in
`/private/tmp/spindrift-shapes-{circle,ellipse,image}-final/`.

## Milestone 1 implementation

Float layout now retains the CSS 2.2 margin rectangle for float placement,
stacking, and clearance while attaching a page-local contour for later content
wrapping. The cascade supports `shape-outside` shape boxes plus `inset()`,
`circle()`, `ellipse()`, and `polygon()`; their lengths and percentages resolve
only after the used reference box is known. Polygons retain both CSS fill rules
and use scanline contour intersections. The implementation also clips contours
to the margin box and preserves a contour on every float fragment.

`shape-margin` is represented as a non-inherited, percentage-bearing computed
value and resolved against the float containing block's inline size. It offsets
circle, ellipse, rounded-rectangle, polygon, and raster-alpha contours before
the mandatory margin-box clip.

`shape-image-threshold` and raster image sources are now retained through
layout. Decoded bitmap alpha and generated linear-gradient alpha use the same
page-local raster contour; their `shape-margin` is clipped to the float margin
box rather than the image content box. SVG alpha, several gradient directions
in vertical writing, and absolute-positioned auto-height replays remain
unresolved. Paths, CSS Shapes 2 `shape()`, and initial-letter boxes are still
unsupported.

The parser now accepts the full reordered, single-value, and edge-offset
`<position>` forms used by `circle()` and `ellipse()`. Fresh isolated runs of
`float-retry-push-circle.html` and `float-retry-push-inset.html` pass within
the WPT tolerance. The remaining failures are tracked by the families below.

| WPT family | Pass / total | Missing capability |
| --- | ---: | --- |
| Circle basic shapes | 28 / 38 | Remaining positioned-radius and vertical-writing cases |
| Ellipse basic shapes | 27 / 35 | Remaining positioned-radius and vertical-writing cases |
| Inset basic shapes | 1 / 20 | Rounded corners and atomic-inline layout cases |
| Polygon basic shapes | 8 / 20 | Concave offsets and vertical-writing cases |
| `path()` and `shape()` | 3 / 9 | Path commands and fill-rule geometry |
| Shape boxes | 20 / 42 | Remaining content/padding/border cases and radii |
| Images and gradients | 19 / 43 | SVG alpha and remaining gradient/writing-mode cases |
| Float retry / examples | 9 / 15 | Remaining rectangular/image retry cases |

## Priority order

1. Complete the focused WPT matrix for the Milestone 1 families, beginning
   with the remaining vertical-writing contour placement cases. The
   zero-`line-height` atomic-inline selector now refines against the used line
   slab in every writing mode, but vertical physical/logical float placement
   still disagrees with the WPT references.

2. Cover the remaining circle, ellipse, inset, and shape-box position/radius
   variants using the same full-slab geometry. Keep parser, used-value, and
   float-band regressions separate so WPT diffs identify the failing layer.

3. Complete image-source parity: SVG alpha rasterization, the remaining
   linear-gradient directions, and positioned auto-height replay isolation.
   Keep raster contours tied to their used content box and clipped to the
   float margin box.

4. Generalize the contour backend to `path()` and CSS Shapes 2 `shape()`.
   Flatten curves/arcs adaptively for line-intersection queries, preserving the
   typed source representation and fill rule for correctness. Treat this as a
   shared layout geometry facility, not a PDF paint-path feature.

5. Complete the remaining inset and concave-polygon contour cases. In
   particular, validate the shaped-band handoff for zero-line-height atomic
   inline blocks before changing the contour math for a single WPT.

## Architecture constraints

- Preserve `FloatShape`'s margin-box rectangle for CSS 2.2 placement,
  clearance, fragmentation, and same-row float stacking. Attach the shape
  area rather than substituting it for the margin box.
- The float-band API currently returns one continuous interval. CSS Shapes
  still permits content to wrap only on the outer side appropriate to a left
  or right float, so the initial implementation can query the relevant
  leftmost or rightmost shape boundary. Keep the internal contour capable of
  reporting multiple intersections for future CSS Exclusions support.
- Resolve basic-shape percentages at the selected reference box, after used
  box metrics and border-radius normalization; resolve `shape-margin`
  percentages against the containing block's inline size. Do not collapse
  box-model units into raw `f32` values at cascade time.
- Store page-local used contours alongside float fragments. A fragmented float
  must use the contour clipped to that fragmentainer's margin box; shape
  geometry cannot be recomputed only from the first fragment.

The normative behavior is CSS Shapes: shapes change the float area used for
wrapping but not float positioning or stacking; a shape is clipped to the
float margin box and a left/right float remains one-sided for wrapping.
<https://drafts.csswg.org/css-shapes-1/#relation-to-box-model-and-float-behavior>

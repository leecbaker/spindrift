# CSS Backgrounds WPT Parity

The latest `css/css-backgrounds/` WPT run (2026-07-13) executes 568 renderable
tests in the local runner. Quire passes 451/568 tests (79.4%).

## Implemented in this pass

Hard-edged inset box shadows now paint the padding box outside the actual
shifted, spread-adjusted shadow perimeter. This preserves negative spread,
keeps semi-transparent corners from compositing twice, and makes
`box-shadow-041.html`, `box-shadow-042.html`, and
`box-shadow-inset-without-border-radius.html` pass.

CSS-wide `inherit` now copies individual background longhand values into the
child's existing background layers instead of replacing the entire layer list.
This preserves a child `background-image` while inheriting `background-origin`,
`background-size`, `background-position`, `background-repeat`, or
`background-clip`, as required by CSS Cascade and CSS Backgrounds.

The cascaded `background-image` value now establishes the definitive layer
count. Longer `background-size`, `background-position`, `background-repeat`,
`background-origin`, and `background-clip` lists are truncated after cascade,
regardless of declaration order. The color layer uses the resulting bottom-most
clip, as required by CSS Backgrounds; `background-color-clip.html` now passes.

CSS-wide `inherit` now also preserves the initial `background-image: none`
layer when it supplies a background longhand to a color-only child. This keeps
the inherited value through post-cascade layer normalization; in particular,
`background-clip-006.html` now passes.

`background-attachment: fixed` now receives its own positioning area: the
page-area viewport in paged media, or a transform/containment-established
fixed containing block. It remains clipped to the element's background clip.
This makes `background-origin-006.html` and both root-canvas
`background-attachment-margin-root-*` cases pass. Repeated spatially uniform
generated images likewise cover the full clip area rather than stopping at a
smaller positioning area.

`border-image-repeat: repeat` now centers the complete sequence of edge tiles
before clipping its equal overhang at each end. This preserves the correct
source pixels when an edge is not a whole number of tiles wide, including an
edge shorter than one tile. It makes `border-image-repeat-repeat-001.html` and
the three `border-image-slice-fill-*` reftests pass.

`border-image-repeat: space` now distributes free space before the first tile,
between tiles, and after the last tile, as the border-image process requires.
This makes `border-image-repeat-005.html` pass.

Raster border-image tiles now use the same non-interpolating sampling policy
as raster background images. This avoids resampling a CSS image differently
solely because it is used as a nine-slice border, and makes the basic
`border-image-repeat: round` and `space` reftests pass.

Simple raster URL background layers now preserve CSS decoration order: the
background color and images paint below the box border. This makes the
transparent-border `background-origin` cases and a multi-layer
`background-clip: border-area` case pass without changing generated-image or
SVG painting, which still requires a shared phase-typed paint representation.

Rounded `padding-box` and `content-box` clips now derive their radii by
subtracting the respective border and padding insets from already-normalized
outer radii. They are not reduced a second time against the smaller inner box,
as required by the background corner-shaping rules; this makes the extreme
single-corner padding-box clip case pass.

For propagated root backgrounds, the canvas is now only the painting and
clipping area: image size, position, and tiling remain relative to the root
element's own used box. Repeated raster, vector, and native-gradient patterns
therefore continue across the canvas without being resized to document height.

Documents whose root element is `display: none` now retain the initial blank
page instead of attempting to serialize a zero-page PDF. This makes the
root/body background-propagation negative cases pass while preserving their
required empty visual output.

The focused WPT cases `background-origin-008.html` and
`background-size-034.html` pass after the change.

`background-size` now also parses its one- and two-axis grammar strictly and
enforces its non-negative definite-value range. Invalid negative values and
third axes no longer partially override an earlier valid declaration. The
focused `background-size-with-negative-value.html` WPT now passes.

The ordinary one- and two-value numeric `background-position` grammar now
resolves each supplied length or percentage against the appropriate axis. This
fixes negative source offsets such as `background-position: -27px 0`, and the
focused negative-percentage comparison WPTs pass.

CSS math `min()` and `max()` comparisons whose percentage coefficients differ
now remain deferred to used-value resolution. This is required because a
background-position percentage is resolved against potentially negative free
space. The `background-position-x` and `background-position-y` longhands now
also update just their intended axis across each background layer. Together,
these changes make both negative-percentage comparison WPTs pass.

No-repeat image layers now remain paintable when their background positioning
area has a zero-sized axis. The used tile is then clipped by the selected
background-clip area, as CSS Backgrounds requires. This makes
`background-size-cover-003.html` pass.

Repeated generated images now share the same PDF tiling-pattern emission as
repeated URL images. This preserves the resolved tile geometry while bounding
the output for sub-pixel tiles; in particular,
`background-size-near-zero-gradient.html` passes without producing a
multi-megabyte PDF.

The `background-size-*` WPT subset currently passes 30 of 36 executable
tests. The remaining cases divide into five legacy repeated-raster-image
references (their used tile geometry matches, but raster output differs from
the equivalent `<img>` reference) and one broader positioned-root sizing case.

## Highest-impact remaining groups

- `background-clip`: the largest cluster is CSS Backgrounds Level 4
  `background-clip:text` and `border-area`, which need text glyph clipping and
  border-area geometry rather than ordinary rectangular background clipping.
- `background-origin`: directory reftests still expose shared reference-layout
  positioning differences in addition to image positioning-area behavior.
- `box-shadow`: remaining cases need broader shadow rasterization and stacking
  behavior.

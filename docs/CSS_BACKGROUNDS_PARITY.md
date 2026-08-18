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
page margin box in paged media, or a transform/containment-established
fixed containing block. It remains clipped to the element's background clip.
This makes `background-origin-006.html` and both root-canvas
`background-attachment-margin-root-*` cases pass. Repeated spatially uniform
generated images likewise cover the full clip area rather than stopping at a
smaller positioning area.

Background paint now distinguishes the document canvas from an ordinary
element paint subtree. A transformed non-root fixed background resolves as
scroll-attached, while propagated canvas paint remains outside an ordinary
root transform. SVG tiling-pattern resources retain the active CSS transform,
so repeated vector backgrounds rotate, scale, and inherit ancestor transforms
with their element rather than staying page-aligned.

`border-image-repeat: repeat` now centers the complete sequence of edge tiles
before clipping its equal overhang at each end. This preserves the correct
source pixels when an edge is not a whole number of tiles wide, including an
edge shorter than one tile. The center slice now rejects zero and infinite
cross-axis edge scales, falls back to the opposite edge, and finally retains
its unscaled source size as required by the border-image process.

`border-image-repeat: space` now distributes free space before the first tile,
between tiles, and after the last tile, as the border-image process requires.

Raster border-image tiles isolate each sliced source region before scaling or
tiling. Integral raster regions use a PDF resource crop; fractional regions
retain their source geometry in an edge-extended local raster, so PDF clipping
and sampling cannot leak neighboring nine-slice pixels across a slice boundary.
They use the same non-interpolating sampling policy as raster background images.

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

Border-image source slices now retain fractional source coordinates through
the shared raster/SVG tiling stage. The source middle region becomes empty
when opposing slices overlap; it is not rescaled. Percentage border-image
widths resolve against the border-image area (including outsets), while
numeric widths and outsets use computed border widths rather than a
style-suppressed used width. Edge and center tile sizes now follow the
cross-axis scaling stage before `repeat`, `round`, or `space` placement;
`space` leaves an undersized region unpainted.

Border-image remains rectangular even when the originating box has a border
radius. A corner with either zero radius component is also emitted as a square
corner, avoiding a degenerate elliptical path at the paint boundary.

Replaced elements now record their principal background and border in the
background/border paint phase. Their concrete object retains its own
content-edge overflow clip, while the principal decoration—including
`border-image-outset` ink—remains outside that descendant-only clip. This
makes positioned replaced border images obey the same overflow ownership as
ordinary CSS boxes.

The focused `css/css-backgrounds/border` sweep on 2026-08-15 executed 85
tests: 65 matched and 20 had remaining reference comparisons. The positioned
inline-block replay now assigns its captured absolute descendants paint order
at the atom's final location, making all `border-left-width-*` and
`border-right-width-*` keyword comparisons pass. The remaining
`border-top-width-*` and `border-bottom-width-*` keyword comparisons use a
separate block margin-collapse/containing-block timing path. The
`border-radius-horizontal-value-is-zero` comparison now uses the same
straight-border primitive as a genuinely zero-radius box after used-value
resolution. Auto-width
`table-layout: fixed` references now follow the automatic track path, and
legacy `<col width>` attributes participate in the author-origin
presentational-hint cascade; this restores the reference geometry for
`border-image-006`. Raster border-image tiles use contained source crops rather
than full-source placement beneath a destination clip, preventing
PDF edge-coverage from sampling an adjacent border-image region. Table height
distribution now distinguishes
rows with an explicit single-row cell height from genuinely auto-height rows;
remaining `round` reference mismatches include inline-table margin/baseline
geometry. The two `border-width-pixel-snapping-001-*` comparisons are
intentionally retained as vector-versus-raster limitations; CSS geometry is
not quantized to satisfy them.

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

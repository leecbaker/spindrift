# Projective CSS transforms parity

`perspective`, `perspective-origin`, and the `perspective()` transform
function retain homogeneous CSS 3D matrices until paint lowering.  A
non-`none` perspective uses CSS's one-pixel rendering clamp, while preserving
the authored/computed zero value. Plane polygons and links are clipped at the
viewer before perspective division; planes wholly behind the viewer are not
painted.

Projective lowering runs before the affine 3D/PDF-CTM fast path. An affine
3D descendant such as `translateZ()` is consequently promoted losslessly when
an ancestor contributes perspective, rather than losing its depth in a 2D
CTM. Generated overflow and fragmentation scopes are transparent layout
bridges: their page-local overflow edge clips the already projected descendant
paint and does not create an additional CSS `transform-style: flat` boundary.
<https://drafts.csswg.org/css-transforms-2/#perspective-property>
<https://drafts.csswg.org/css-transforms-2/#grouping-property-values>
<https://drafts.csswg.org/css-overflow-3/#overflow-clipping>

The PDF backend has no projective CTM. It therefore lowers rectangular paint
to projected polygons and preserves its normal affine CTM fast path whenever
possible. Remaining projective paint kinds are listed centrally in
`SPEC_DIVERGENCES.md`: arbitrary paths/strokes, text outlines and `ActualText`,
images, gradients/patterns, and SVG viewports require dedicated lowerers.

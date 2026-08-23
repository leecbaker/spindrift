use super::*;

/// Computed `clip-path` support relevant to paint isolation and clipping.
///
/// A basic-shape path establishes a stacking context and clips to the
/// associated geometry box. CSS Masking defines the default geometry box as
/// the border box:
/// <https://www.w3.org/TR/css-masking-1/#the-clip-path>.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum ClipPath {
    None,
    Polygon(Vec<ClipPathPolygonPoint>),
    Inset {
        top: ComputedLengthPercentage,
        right: ComputedLengthPercentage,
        bottom: ComputedLengthPercentage,
        left: ComputedLengthPercentage,
    },
    Shape,
    Url,
}

impl ClipPath {
    pub(crate) fn resolve_root_font_metric_lengths(&mut self, basis: RootFontMetricLengthBasis) {
        match self {
            Self::Polygon(points) => {
                for point in points {
                    point.x.resolve_root_font_metric_lengths(basis);
                    point.y.resolve_root_font_metric_lengths(basis);
                }
            }
            Self::Inset {
                top,
                right,
                bottom,
                left,
            } => {
                for value in [top, right, bottom, left] {
                    value.resolve_root_font_metric_lengths(basis);
                }
            }
            Self::None | Self::Shape | Self::Url => {}
        }
    }

    pub(crate) fn requires_root_font_metrics(&self) -> bool {
        match self {
            Self::Polygon(points) => points.iter().any(|point| {
                point.x.requires_root_font_metrics() || point.y.requires_root_font_metrics()
            }),
            Self::Inset {
                top,
                right,
                bottom,
                left,
            } => [top, right, bottom, left]
                .into_iter()
                .any(|value| value.requires_root_font_metrics()),
            Self::None | Self::Shape | Self::Url => false,
        }
    }
}

/// Computed legacy CSS 2 `clip` value for absolutely positioned boxes.
///
/// Unlike `clip-path`, `clip` is a rectangle whose four physical edges are
/// offsets from the box's border edges. Percentages are invalid and `auto`
/// keeps the corresponding generated border-box edge. Keeping the edges
/// typed until paint has the used border box prevents a layout-relative value
/// from escaping into page paint as an untyped scalar.
/// <https://drafts.csswg.org/css2/#propdef-clip>
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum LegacyClip {
    Auto,
    Rect([LegacyClipEdge; 4]),
}

impl LegacyClip {
    pub(crate) const AUTO: Self = Self::Auto;

    pub(crate) fn forces_flattening(&self) -> bool {
        !matches!(self, Self::Auto)
    }

    pub(crate) fn resolve_root_font_metric_lengths(&mut self, basis: RootFontMetricLengthBasis) {
        let Self::Rect(edges) = self else {
            return;
        };
        for edge in edges {
            if let LegacyClipEdge::Length(value) = edge {
                value.resolve_root_font_metric_lengths(basis);
            }
        }
    }

    pub(crate) fn requires_root_font_metrics(&self) -> bool {
        matches!(self, Self::Rect(edges) if edges.iter().any(|edge| {
            matches!(edge, LegacyClipEdge::Length(value) if value.requires_root_font_metrics())
        }))
    }
}

/// One physical edge of a legacy CSS 2 `clip: rect()` value, in top, right,
/// bottom, left order.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum LegacyClipEdge {
    Auto,
    Length(ComputedLengthPercentage),
}

/// One `<length-percentage> <length-percentage>` vertex of `polygon()`.
///
/// Keeping each coordinate as a computed length percentage delays resolution
/// until the relevant `clip-path` geometry box is known.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ClipPathPolygonPoint {
    pub(crate) x: ComputedLengthPercentage,
    pub(crate) y: ComputedLengthPercentage,
}

/// Computed CSS Shapes Level 1 `shape-outside` value.
///
/// Percentages deliberately remain unresolved until float layout has selected
/// the shape reference box. CSS Shapes changes a float's wrapping area, not
/// its margin-box placement or painting:
/// <https://drafts.csswg.org/css-shapes-1/#shape-outside-property>.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum ShapeOutside {
    None,
    Box(ShapeBox),
    /// An image alpha contour. CSS Shapes sizes this image as a replaced
    /// element with the float's used content-box dimensions.
    /// <https://drafts.csswg.org/css-shapes-1/#shapes-from-image>
    Image(BackgroundImage),
    Basic {
        shape: BasicShape,
        reference_box: ShapeBox,
    },
}

impl ShapeOutside {
    pub(crate) const NONE: Self = Self::None;

    pub(crate) fn resolve_root_font_metric_lengths(&mut self, basis: RootFontMetricLengthBasis) {
        match self {
            Self::Image(image) => image.resolve_root_font_metric_lengths(basis),
            Self::Basic { shape, .. } => shape.resolve_root_font_metric_lengths(basis),
            Self::None | Self::Box(_) => {}
        }
    }

    pub(crate) fn requires_root_font_metrics(&self) -> bool {
        match self {
            Self::Image(image) => image.requires_root_font_metrics(),
            Self::Basic { shape, .. } => shape.requires_root_font_metrics(),
            Self::None | Self::Box(_) => false,
        }
    }
}

/// The reference box used by CSS Shapes float-area geometry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ShapeBox {
    Margin,
    Border,
    Padding,
    Content,
}

/// Basic shapes implemented for the first CSS Shapes milestone.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum BasicShape {
    Inset(ShapeInset),
    Circle(ShapeCircle),
    Ellipse(ShapeEllipse),
    Polygon(ShapePolygon),
}

impl BasicShape {
    fn resolve_root_font_metric_lengths(&mut self, basis: RootFontMetricLengthBasis) {
        match self {
            Self::Inset(shape) => {
                for value in [
                    &mut shape.top,
                    &mut shape.right,
                    &mut shape.bottom,
                    &mut shape.left,
                ] {
                    value.resolve_root_font_metric_lengths(basis);
                }
                shape.radii.resolve_root_font_metric_lengths(basis);
            }
            Self::Circle(shape) => {
                shape.radius.resolve_root_font_metric_lengths(basis);
                shape.position.resolve_root_font_metric_lengths(basis);
            }
            Self::Ellipse(shape) => {
                shape
                    .horizontal_radius
                    .resolve_root_font_metric_lengths(basis);
                shape
                    .vertical_radius
                    .resolve_root_font_metric_lengths(basis);
                shape.position.resolve_root_font_metric_lengths(basis);
            }
            Self::Polygon(shape) => {
                for point in &mut shape.vertices {
                    point.x.resolve_root_font_metric_lengths(basis);
                    point.y.resolve_root_font_metric_lengths(basis);
                }
            }
        }
    }

    fn requires_root_font_metrics(&self) -> bool {
        match self {
            Self::Inset(shape) => {
                [&shape.top, &shape.right, &shape.bottom, &shape.left]
                    .into_iter()
                    .any(|value| value.requires_root_font_metrics())
                    || shape.radii.requires_root_font_metrics()
            }
            Self::Circle(shape) => {
                shape.radius.requires_root_font_metrics()
                    || shape.position.requires_root_font_metrics()
            }
            Self::Ellipse(shape) => {
                shape.horizontal_radius.requires_root_font_metrics()
                    || shape.vertical_radius.requires_root_font_metrics()
                    || shape.position.requires_root_font_metrics()
            }
            Self::Polygon(shape) => shape.vertices.iter().any(|point| {
                point.x.requires_root_font_metrics() || point.y.requires_root_font_metrics()
            }),
        }
    }
}

impl ShapeCircleRadius {
    fn resolve_root_font_metric_lengths(&mut self, basis: RootFontMetricLengthBasis) {
        if let Self::LengthPercentage(value) = self {
            value.resolve_root_font_metric_lengths(basis);
        }
    }

    fn requires_root_font_metrics(&self) -> bool {
        matches!(self, Self::LengthPercentage(value) if value.requires_root_font_metrics())
    }
}

impl ShapeEllipseRadius {
    fn resolve_root_font_metric_lengths(&mut self, basis: RootFontMetricLengthBasis) {
        if let Self::LengthPercentage(value) = self {
            value.resolve_root_font_metric_lengths(basis);
        }
    }

    fn requires_root_font_metrics(&self) -> bool {
        matches!(self, Self::LengthPercentage(value) if value.requires_root_font_metrics())
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ShapeInset {
    pub(crate) top: ComputedLengthPercentage,
    pub(crate) right: ComputedLengthPercentage,
    pub(crate) bottom: ComputedLengthPercentage,
    pub(crate) left: ComputedLengthPercentage,
    pub(crate) radii: BorderRadius,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ShapeCircle {
    pub(crate) radius: ShapeCircleRadius,
    pub(crate) position: ShapePosition,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum ShapeCircleRadius {
    LengthPercentage(ComputedLengthPercentage),
    ClosestSide,
    FarthestSide,
    ClosestCorner,
    FarthestCorner,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ShapeEllipse {
    pub(crate) horizontal_radius: ShapeEllipseRadius,
    pub(crate) vertical_radius: ShapeEllipseRadius,
    pub(crate) position: ShapePosition,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum ShapeEllipseRadius {
    LengthPercentage(ComputedLengthPercentage),
    ClosestSide,
    FarthestSide,
}

/// Typed CSS Shapes Level 1 `polygon()` contour.
///
/// Vertices retain their independent percentage bases until float layout has
/// resolved the selected shape reference box:
/// <https://drafts.csswg.org/css-shapes-1/#funcdef-polygon>.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ShapePolygon {
    pub(crate) fill_rule: ShapeFillRule,
    pub(crate) vertices: Vec<ShapePolygonPoint>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ShapeFillRule {
    NonZero,
    EvenOdd,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ShapePolygonPoint {
    pub(crate) x: ComputedLengthPercentage,
    pub(crate) y: ComputedLengthPercentage,
}

/// A basic-shape center expressed as coordinates in its reference box.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ShapePosition {
    pub(crate) x: ComputedLengthPercentage,
    pub(crate) y: ComputedLengthPercentage,
}

impl ShapePosition {
    pub(crate) fn center() -> Self {
        Self {
            x: ComputedLengthPercentage::from_percent(0.5),
            y: ComputedLengthPercentage::from_percent(0.5),
        }
    }

    fn resolve_root_font_metric_lengths(&mut self, basis: RootFontMetricLengthBasis) {
        self.x.resolve_root_font_metric_lengths(basis);
        self.y.resolve_root_font_metric_lengths(basis);
    }

    fn requires_root_font_metrics(&self) -> bool {
        self.x.requires_root_font_metrics() || self.y.requires_root_font_metrics()
    }
}

/// Computed CSS Borders 4 `border-shape` value.
///
/// `border-shape` changes only painting and overflow clipping; it does not
/// alter box layout. Basic-shape coordinates stay as typed computed lengths
/// until their selected geometry box is available during paint:
/// <https://drafts.csswg.org/css-borders-4/#border-shape>.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum BorderShape {
    None,
    Circle(BorderShapeCircle),
    Ellipse(BorderShapeEllipse),
    Path(BorderShapePath),
    Inset(BorderShapeInset),
    Polygon(BorderShapePolygon),
    /// The outer and inner contours of a CSS Borders 4 `border-shape`.
    ///
    /// Keeping the pair heterogeneous avoids encoding an artificial
    /// same-primitive restriction in the computed value: the grammar permits
    /// any two basic shapes, each with its own geometry box.
    Pair {
        outer: Box<BorderShape>,
        inner: Box<BorderShape>,
    },
}

impl BorderShape {
    /// Assign the geometry box parsed immediately after this basic shape.
    /// A pair is not a single basic shape and cannot receive one.
    pub(crate) fn set_geometry_box(&mut self, geometry_box: BorderShapeGeometryBox) -> Option<()> {
        match self {
            Self::Circle(shape) => shape.geometry_box = geometry_box,
            Self::Ellipse(shape) => shape.geometry_box = geometry_box,
            Self::Path(shape) => shape.geometry_box = geometry_box,
            Self::Inset(shape) => shape.geometry_box = geometry_box,
            Self::Polygon(shape) => shape.geometry_box = geometry_box,
            Self::None | Self::Pair { .. } => return None,
        }
        Some(())
    }

    /// Apply the distinct default geometry boxes for the outer and inner
    /// basic shapes of a `border-shape` pair.
    pub(crate) fn replace_half_border_box(&mut self, geometry_box: BorderShapeGeometryBox) {
        let current = match self {
            Self::Circle(shape) => &mut shape.geometry_box,
            Self::Ellipse(shape) => &mut shape.geometry_box,
            Self::Path(shape) => &mut shape.geometry_box,
            Self::Inset(shape) => &mut shape.geometry_box,
            Self::Polygon(shape) => &mut shape.geometry_box,
            Self::None | Self::Pair { .. } => return,
        };
        if *current == BorderShapeGeometryBox::HalfBorder {
            *current = geometry_box;
        }
    }

    pub(crate) fn resolve_root_font_metric_lengths(&mut self, basis: RootFontMetricLengthBasis) {
        match self {
            Self::Circle(shape) => {
                shape.radius.resolve_root_font_metric_lengths(basis);
                shape.position.resolve_root_font_metric_lengths(basis);
            }
            Self::Ellipse(shape) => {
                shape
                    .horizontal_radius
                    .resolve_root_font_metric_lengths(basis);
                shape
                    .vertical_radius
                    .resolve_root_font_metric_lengths(basis);
                shape.position.resolve_root_font_metric_lengths(basis);
            }
            Self::Path(shape) => {
                for vertex in &mut shape.vertices {
                    vertex.resolve_root_font_metric_lengths(basis);
                }
            }
            Self::Inset(shape) => {
                for value in [
                    &mut shape.top,
                    &mut shape.right,
                    &mut shape.bottom,
                    &mut shape.left,
                ] {
                    value.resolve_root_font_metric_lengths(basis);
                }
                if let Some(radius) = &mut shape.corner_radius {
                    radius.resolve_root_font_metric_lengths(basis);
                }
            }
            Self::Polygon(shape) => {
                for vertex in &mut shape.vertices {
                    vertex.resolve_root_font_metric_lengths(basis);
                }
            }
            Self::Pair { outer, inner } => {
                outer.resolve_root_font_metric_lengths(basis);
                inner.resolve_root_font_metric_lengths(basis);
            }
            Self::None => {}
        }
    }

    pub(crate) fn requires_root_font_metrics(&self) -> bool {
        match self {
            Self::Circle(shape) => {
                shape.radius.requires_root_font_metrics()
                    || shape.position.requires_root_font_metrics()
            }
            Self::Ellipse(shape) => {
                shape.horizontal_radius.requires_root_font_metrics()
                    || shape.vertical_radius.requires_root_font_metrics()
                    || shape.position.requires_root_font_metrics()
            }
            Self::Path(shape) => shape
                .vertices
                .iter()
                .any(BorderShapePosition::requires_root_font_metrics),
            Self::Polygon(shape) => shape
                .vertices
                .iter()
                .any(BorderShapePosition::requires_root_font_metrics),
            Self::Inset(shape) => {
                [&shape.top, &shape.right, &shape.bottom, &shape.left]
                    .into_iter()
                    .any(|value| value.requires_root_font_metrics())
                    || shape
                        .corner_radius
                        .as_ref()
                        .is_some_and(ComputedLengthPercentage::requires_root_font_metrics)
            }
            Self::Pair { outer, inner } => {
                outer.requires_root_font_metrics() || inner.requires_root_font_metrics()
            }
            Self::None => false,
        }
    }
}

/// A closed line-only CSS basic shape retained for `border-shape` painting.
///
/// Each vertex remains a typed computed length percentage until its geometry
/// box is known, matching the percentage-resolution boundary for the other
/// border-shape primitives:
/// <https://drafts.csswg.org/css-shapes-2/#shape-function>.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct BorderShapePath {
    pub(crate) vertices: Vec<BorderShapePosition>,
    pub(crate) geometry_box: BorderShapeGeometryBox,
}

/// Typed inset distances for a CSS `inset()` border shape.
///
/// Percentages resolve against their respective geometry-box axes at paint
/// time, so this deliberately stores physical sides rather than an eagerly
/// resolved rectangle:
/// <https://drafts.csswg.org/css-shapes-1/#funcdef-inset>.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct BorderShapeInset {
    pub(crate) top: ComputedLengthPercentage,
    pub(crate) right: ComputedLengthPercentage,
    pub(crate) bottom: ComputedLengthPercentage,
    pub(crate) left: ComputedLengthPercentage,
    /// Uniform `round <length-percentage>` radius, retained for paint-time
    /// resolution against the inset rectangle's axes.
    pub(crate) corner_radius: Option<ComputedLengthPercentage>,
    pub(crate) geometry_box: BorderShapeGeometryBox,
}

/// Typed vertices of the CSS `polygon()` basic shape for `border-shape`.
///
/// The fill rule is intentionally not represented yet: values that request a
/// non-default rule are rejected by the parser until paint can preserve that
/// choice. Coordinates stay typed through percentage resolution.
/// <https://drafts.csswg.org/css-shapes-1/#funcdef-polygon>
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct BorderShapePolygon {
    pub(crate) vertices: Vec<BorderShapePosition>,
    pub(crate) geometry_box: BorderShapeGeometryBox,
}

/// One currently-supported circular basic shape in `border-shape`.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct BorderShapeCircle {
    pub(crate) radius: BorderShapeCircleRadius,
    pub(crate) position: BorderShapePosition,
    pub(crate) geometry_box: BorderShapeGeometryBox,
}

/// Circle radius syntax retained until geometry-box resolution.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum BorderShapeCircleRadius {
    LengthPercentage(ComputedLengthPercentage),
    ClosestSide,
    FarthestSide,
    ClosestCorner,
    FarthestCorner,
}

impl BorderShapeCircleRadius {
    fn resolve_root_font_metric_lengths(&mut self, basis: RootFontMetricLengthBasis) {
        if let Self::LengthPercentage(value) = self {
            value.resolve_root_font_metric_lengths(basis);
        }
    }

    fn requires_root_font_metrics(&self) -> bool {
        matches!(self, Self::LengthPercentage(value) if value.requires_root_font_metrics())
    }
}

/// One currently-supported elliptical basic shape in `border-shape`.
///
/// Ellipse radii are preserved independently because CSS percentage radii
/// resolve against their respective geometry-box axes:
/// <https://drafts.csswg.org/css-shapes-1/#funcdef-ellipse>.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct BorderShapeEllipse {
    pub(crate) horizontal_radius: BorderShapeEllipseRadius,
    pub(crate) vertical_radius: BorderShapeEllipseRadius,
    pub(crate) position: BorderShapePosition,
    pub(crate) geometry_box: BorderShapeGeometryBox,
}

/// One axis radius of a CSS `ellipse()` basic shape.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum BorderShapeEllipseRadius {
    LengthPercentage(ComputedLengthPercentage),
    ClosestSide,
    FarthestSide,
    ClosestCorner,
    FarthestCorner,
}

impl BorderShapeEllipseRadius {
    fn resolve_root_font_metric_lengths(&mut self, basis: RootFontMetricLengthBasis) {
        if let Self::LengthPercentage(value) = self {
            value.resolve_root_font_metric_lengths(basis);
        }
    }

    fn requires_root_font_metrics(&self) -> bool {
        matches!(self, Self::LengthPercentage(value) if value.requires_root_font_metrics())
    }
}

/// Basic-shape center relative to its selected geometry box.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct BorderShapePosition {
    pub(crate) x: ComputedLengthPercentage,
    pub(crate) y: ComputedLengthPercentage,
}

impl BorderShapePosition {
    pub(crate) fn center() -> Self {
        Self {
            x: ComputedLengthPercentage::from_percent(0.5),
            y: ComputedLengthPercentage::from_percent(0.5),
        }
    }

    fn resolve_root_font_metric_lengths(&mut self, basis: RootFontMetricLengthBasis) {
        self.x.resolve_root_font_metric_lengths(basis);
        self.y.resolve_root_font_metric_lengths(basis);
    }

    fn requires_root_font_metrics(&self) -> bool {
        self.x.requires_root_font_metrics() || self.y.requires_root_font_metrics()
    }
}

/// Reference rectangle for a `border-shape` basic shape.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BorderShapeGeometryBox {
    Border,
    Padding,
    Content,
    Margin,
    HalfBorder,
}

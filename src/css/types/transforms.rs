use super::*;

pub(crate) enum CssTransformSpace {}

pub(crate) type CssTransform = euclid::Transform2D<f32, CssTransformSpace, CssTransformSpace>;
/// The typed homogeneous matrix representation used by CSS 3D transforms.
///
/// Keeping the source and destination CSS coordinate spaces equal prevents a
/// 3D transform from being accidentally applied to page paint coordinates
/// before its CSS length units and y-axis convention are resolved.
pub(crate) type CssTransform3D = euclid::Transform3D<f32, CssTransformSpace, CssTransformSpace>;

/// A CSS `matrix(a, b, c, d, e, f)` function before its target coordinate
/// system has been selected.
#[derive(Debug, Clone, Copy, PartialEq)]
#[repr(transparent)]
pub(crate) struct CssAffineMatrix(CssTransform);

impl CssAffineMatrix {
    pub(crate) const fn new(a: f32, b: f32, c: f32, d: f32, e: f32, f: f32) -> Self {
        Self(CssTransform::new(a, b, c, d, e, f))
    }

    /// Project this CSS matrix into another y-down coordinate space using an
    /// explicit CSS-unit basis. SVG source coordinates share CSS's y-down
    /// orientation and therefore do not use the paint-space projection.
    pub(crate) fn into_space<Space>(
        self,
        css_unit_to_target: euclid::Scale<f32, CssTransformSpace, Space>,
    ) -> euclid::Transform2D<f32, Space, Space> {
        euclid::Transform2D::new(
            self.0.m11,
            self.0.m12,
            self.0.m21,
            self.0.m22,
            self.0.m31 * css_unit_to_target.0,
            self.0.m32 * css_unit_to_target.0,
        )
    }

    /// Project this CSS y-down affine matrix into a y-up target space.
    ///
    /// Page paint coordinates use PDF's upward-positive y axis.  The
    /// projection is therefore `S · M · S⁻¹`, with `S =
    /// diag(css_unit_to_target, -css_unit_to_target)`, rather than a unit
    /// conversion of only the translation terms.
    /// <https://drafts.csswg.org/css-transforms-1/#mathematical-description>
    pub(crate) fn into_y_up_space<Space>(
        self,
        css_unit_to_target: euclid::Scale<f32, CssTransformSpace, Space>,
    ) -> euclid::Transform2D<f32, Space, Space> {
        euclid::Transform2D::new(
            self.0.m11,
            -self.0.m12,
            -self.0.m21,
            self.0.m22,
            self.0.m31 * css_unit_to_target.0,
            -self.0.m32 * css_unit_to_target.0,
        )
    }
}

/// A CSS `matrix3d()` value before it is resolved into a paint-space matrix.
///
/// CSS serializes 4×4 matrices in column-major order, which is also the
/// field order used by Euclid's homogeneous transform constructor:
/// <https://drafts.csswg.org/css-transforms-2/#funcdef-transform-matrix3d>.
#[derive(Debug, Clone, Copy, PartialEq)]
#[repr(transparent)]
pub(crate) struct CssMatrix3D(pub(crate) CssTransform3D);

impl CssMatrix3D {
    #[allow(clippy::too_many_arguments)]
    pub(crate) const fn new(
        m11: f32,
        m12: f32,
        m13: f32,
        m14: f32,
        m21: f32,
        m22: f32,
        m23: f32,
        m24: f32,
        m31: f32,
        m32: f32,
        m33: f32,
        m34: f32,
        m41: f32,
        m42: f32,
        m43: f32,
        m44: f32,
    ) -> Self {
        Self(CssTransform3D::new(
            m11, m12, m13, m14, m21, m22, m23, m24, m31, m32, m33, m34, m41, m42, m43, m44,
        ))
    }
}

/// A two-dimensional CSS translation. Its components are lengths or
/// percentages, rather than a paint vector, until the reference box is known.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct CssTransformTranslation {
    pub(crate) x: ComputedLengthPercentage,
    pub(crate) y: ComputedLengthPercentage,
}

/// A three-dimensional translation. CSS forbids percentages in the z
/// component, but retaining the computed length representation lets font and
/// viewport units resolve at the same used-value boundary as x and y.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct CssTransformTranslation3D {
    pub(crate) x: ComputedLengthPercentage,
    pub(crate) y: ComputedLengthPercentage,
    pub(crate) z: ComputedLengthPercentage,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct CssScaleFactors {
    pub(crate) x: f32,
    pub(crate) y: f32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct CssScaleFactors3D {
    pub(crate) x: f32,
    pub(crate) y: f32,
    pub(crate) z: f32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct CssRotate3D {
    pub(crate) axis_x: f32,
    pub(crate) axis_y: f32,
    pub(crate) axis_z: f32,
    pub(crate) angle: euclid::Angle<f32>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct CssSkewAngles {
    pub(crate) x: euclid::Angle<f32>,
    pub(crate) y: euclid::Angle<f32>,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum TransformFunction {
    Matrix(CssAffineMatrix),
    Matrix3D(CssMatrix3D),
    Translate(CssTransformTranslation),
    Translate3D(CssTransformTranslation3D),
    Scale(CssScaleFactors),
    Scale3D(CssScaleFactors3D),
    Rotate(euclid::Angle<f32>),
    Rotate3D(CssRotate3D),
    Skew(CssSkewAngles),
    Perspective(ComputedPerspective),
}

/// A computed CSS perspective distance.
///
/// The grammar accepts `none` and non-negative lengths, including zero. The
/// zero value is preserved through cascade and clamped only for rendering.
/// <https://drafts.csswg.org/css-transforms-2/#perspective-property>
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum ComputedPerspective {
    None,
    Distance(NonNegativeComputedLength),
}

impl ComputedPerspective {
    pub(crate) const NONE: Self = Self::None;

    pub(crate) fn used_for_rendering(&self) -> Option<UsedPerspectiveDistance> {
        let Self::Distance(distance) = self else {
            return None;
        };
        Some(UsedPerspectiveDistance(layout_pt(
            distance.length().max(crate::css::CSS_PX_TO_PT),
        )))
    }

    pub(crate) fn resolve_font_metric_lengths(&mut self, ch_advance: LayoutLength) {
        if let Self::Distance(distance) = self {
            distance.resolve_font_metric_lengths(ch_advance);
        }
    }

    pub(crate) fn resolve_root_font_metric_lengths(&mut self, basis: RootFontMetricLengthBasis) {
        if let Self::Distance(distance) = self {
            distance.resolve_root_font_metric_lengths(basis);
        }
    }

    pub(crate) fn requires_ch_advance(&self) -> bool {
        matches!(self, Self::Distance(distance) if distance.requires_ch_advance())
    }

    pub(crate) fn requires_root_font_metrics(&self) -> bool {
        matches!(self, Self::Distance(distance) if distance.requires_root_font_metrics())
    }
}

/// A non-negative absolute computed length. Percentages cannot cross this
/// boundary, preserving the CSS grammar for perspective distances.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct NonNegativeComputedLength(ComputedLengthPercentage);

impl NonNegativeComputedLength {
    pub(crate) fn new(value: ComputedLengthPercentage) -> Option<Self> {
        value
            .length_if_no_percent()
            .filter(|value| value.is_finite() && *value >= 0.0)
            .map(|_| Self(value))
    }

    pub(crate) fn length(&self) -> f32 {
        self.0
            .length_if_no_percent()
            .expect("perspective distance cannot contain percentages")
    }

    pub(crate) fn resolve_font_metric_lengths(&mut self, ch_advance: LayoutLength) {
        self.0.resolve_font_metric_lengths(ch_advance);
        debug_assert!(self.length() >= 0.0);
    }

    pub(crate) fn resolve_root_font_metric_lengths(&mut self, basis: RootFontMetricLengthBasis) {
        self.0.resolve_root_font_metric_lengths(basis);
        debug_assert!(self.length() >= 0.0);
    }

    pub(crate) fn requires_ch_advance(&self) -> bool {
        self.0.requires_ch_advance()
    }

    pub(crate) fn requires_root_font_metrics(&self) -> bool {
        self.0.requires_root_font_metrics()
    }
}

/// A non-zero paint-space perspective distance after CSS's rendering clamp.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct UsedPerspectiveDistance(LayoutLength);

impl UsedPerspectiveDistance {
    pub(crate) fn points(self) -> f32 {
        self.0.points()
    }
}

impl TransformFunction {
    pub(crate) fn resolve_font_metric_lengths(&mut self, ch_advance: LayoutLength) {
        match self {
            Self::Translate(translation) => {
                translation.x.resolve_font_metric_lengths(ch_advance);
                translation.y.resolve_font_metric_lengths(ch_advance);
            }
            Self::Translate3D(translation) => {
                translation.x.resolve_font_metric_lengths(ch_advance);
                translation.y.resolve_font_metric_lengths(ch_advance);
                translation.z.resolve_font_metric_lengths(ch_advance);
            }
            Self::Perspective(perspective) => perspective.resolve_font_metric_lengths(ch_advance),
            _ => {}
        }
    }

    pub(crate) fn resolve_root_font_metric_lengths(&mut self, basis: RootFontMetricLengthBasis) {
        match self {
            Self::Translate(translation) => {
                translation.x.resolve_root_font_metric_lengths(basis);
                translation.y.resolve_root_font_metric_lengths(basis);
            }
            Self::Translate3D(translation) => {
                translation.x.resolve_root_font_metric_lengths(basis);
                translation.y.resolve_root_font_metric_lengths(basis);
                translation.z.resolve_root_font_metric_lengths(basis);
            }
            Self::Perspective(perspective) => perspective.resolve_root_font_metric_lengths(basis),
            _ => {}
        }
    }

    pub(crate) fn requires_ch_advance(&self) -> bool {
        match self {
            Self::Translate(translation) => {
                translation.x.requires_ch_advance() || translation.y.requires_ch_advance()
            }
            Self::Translate3D(translation) => {
                translation.x.requires_ch_advance()
                    || translation.y.requires_ch_advance()
                    || translation.z.requires_ch_advance()
            }
            Self::Perspective(perspective) => perspective.requires_ch_advance(),
            _ => false,
        }
    }

    pub(crate) fn requires_root_font_metrics(&self) -> bool {
        match self {
            Self::Translate(translation) => {
                translation.x.requires_root_font_metrics()
                    || translation.y.requires_root_font_metrics()
            }
            Self::Translate3D(translation) => {
                translation.x.requires_root_font_metrics()
                    || translation.y.requires_root_font_metrics()
                    || translation.z.requires_root_font_metrics()
            }
            Self::Perspective(perspective) => perspective.requires_root_font_metrics(),
            _ => false,
        }
    }
}

pub(crate) type TransformList = Vec<TransformFunction>;

/// Computed independent 2D transform properties.
///
/// CSS Transforms Level 2 composes these properties before the legacy
/// `transform` list, in translate/rotate/scale order. Keeping the values
/// distinct preserves that order through cascade and used-value resolution:
/// <https://drafts.csswg.org/css-transforms-2/#individual-transforms>.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct IndividualTransforms {
    pub(crate) translate: Option<CssTransformTranslation>,
    pub(crate) rotate: Option<euclid::Angle<f32>>,
    pub(crate) scale: Option<CssScaleFactors>,
}

impl IndividualTransforms {
    pub(crate) const NONE: Self = Self {
        translate: None,
        rotate: None,
        scale: None,
    };

    pub(crate) fn is_none(&self) -> bool {
        self.translate.is_none() && self.rotate.is_none() && self.scale.is_none()
    }

    pub(crate) fn requires_ch_advance(&self) -> bool {
        self.translate.as_ref().is_some_and(|translation| {
            translation.x.requires_ch_advance() || translation.y.requires_ch_advance()
        })
    }

    pub(crate) fn requires_root_font_metrics(&self) -> bool {
        self.translate.as_ref().is_some_and(|translation| {
            translation.x.requires_root_font_metrics() || translation.y.requires_root_font_metrics()
        })
    }

    pub(crate) fn resolve_font_metric_lengths(&mut self, ch_advance: LayoutLength) {
        if let Some(translation) = &mut self.translate {
            translation.x.resolve_font_metric_lengths(ch_advance);
            translation.y.resolve_font_metric_lengths(ch_advance);
        }
    }

    pub(crate) fn resolve_root_font_metric_lengths(&mut self, basis: RootFontMetricLengthBasis) {
        if let Some(translation) = &mut self.translate {
            translation.x.resolve_root_font_metric_lengths(basis);
            translation.y.resolve_root_font_metric_lengths(basis);
        }
    }
}

/// Computed `transform-origin`, including its absolute z component.
///
/// Percentages resolve against the selected two-dimensional reference box;
/// CSS Transforms forbids percentages for the z component:
/// <https://www.w3.org/TR/css-transforms-1/#transform-origin-property>.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct TransformOrigin {
    pub(crate) x: ComputedLengthPercentage,
    pub(crate) y: ComputedLengthPercentage,
    pub(crate) z: ComputedLengthPercentage,
    /// Distinguishes CSS `initial` from an author-specified `50% 50%`.
    /// SVG graphics resolve the former to `0 0`, while HTML boxes resolve it
    /// to the property’s ordinary `50% 50%` initial used value.
    pub(crate) is_initial: bool,
}

/// The two-dimensional reference-box point from which an element projects
/// descendants under the CSS `perspective` property.
/// <https://drafts.csswg.org/css-transforms-2/#perspective-origin-property>
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct PerspectiveOrigin {
    pub(crate) x: ComputedLengthPercentage,
    pub(crate) y: ComputedLengthPercentage,
}

impl PerspectiveOrigin {
    pub(crate) const INITIAL: Self = Self {
        x: ComputedLengthPercentage::from_percent(0.5),
        y: ComputedLengthPercentage::from_percent(0.5),
    };

    pub(crate) const fn new(x: ComputedLengthPercentage, y: ComputedLengthPercentage) -> Self {
        Self { x, y }
    }

    pub(crate) fn resolve_font_metric_lengths(&mut self, ch_advance: LayoutLength) {
        self.x.resolve_font_metric_lengths(ch_advance);
        self.y.resolve_font_metric_lengths(ch_advance);
    }

    pub(crate) fn resolve_root_font_metric_lengths(&mut self, basis: RootFontMetricLengthBasis) {
        self.x.resolve_root_font_metric_lengths(basis);
        self.y.resolve_root_font_metric_lengths(basis);
    }

    pub(crate) fn requires_ch_advance(&self) -> bool {
        self.x.requires_ch_advance() || self.y.requires_ch_advance()
    }

    pub(crate) fn requires_root_font_metrics(&self) -> bool {
        self.x.requires_root_font_metrics() || self.y.requires_root_font_metrics()
    }

    /// Resolve the origin against the perspective element's transform
    /// reference box.  Unlike `transform-origin`, CSS never gives this
    /// property a z component.
    pub(crate) fn resolve_against_paint_rect(
        self,
        border_box: crate::document::paint::geometry::PaintRect,
    ) -> crate::document::paint::geometry::PaintPoint {
        crate::document::paint::geometry::PaintPoint::new(
            border_box.origin.x
                + self
                    .x
                    .used_length_with_percentage_basis(PercentageBasis::definite(layout_pt(
                        border_box.size.width,
                    )))
                    .map(layout_points)
                    .unwrap_or(0.0),
            border_box.origin.y + border_box.size.height
                - self
                    .y
                    .used_length_with_percentage_basis(PercentageBasis::definite(layout_pt(
                        border_box.size.height,
                    )))
                    .map(layout_points)
                    .unwrap_or(0.0),
        )
    }
}

/// Whether the back-facing side of a flattened 3D transform is painted.
/// <https://drafts.csswg.org/css-transforms-2/#backface-visibility-property>
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BackfaceVisibility {
    Visible,
    Hidden,
}

/// Whether a transformable element flattens descendants into its own plane or
/// extends a CSS 3D rendering context.
///
/// The computed value is the specified keyword, while grouping properties can
/// force the used value to [`Self::Flat`].
/// <https://drafts.csswg.org/css-transforms-2/#transform-style-property>
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TransformStyle {
    Flat,
    Preserve3d,
}

/// Reference box selected for CSS transform percentages and `transform-origin`.
///
/// HTML boxes use their border box unless `content-box` is explicitly selected.
/// SVG-specific boxes are retained in the computed value so the SVG scene
/// adapter can resolve them from its own geometry.
/// <https://drafts.csswg.org/css-transforms-1/#transform-box-property>
#[allow(
    clippy::enum_variant_names,
    reason = "CSS syntax defines each transform-box keyword with the `-box` suffix."
)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TransformBox {
    ContentBox,
    BorderBox,
    FillBox,
    StrokeBox,
    ViewBox,
}

impl TransformBox {
    /// CSS Transforms' initial `view-box` behaves as `border-box` for an HTML
    /// layout box, which has no SVG viewport.
    pub(crate) const INITIAL: Self = Self::ViewBox;

    pub(crate) const fn html_reference_is_content_box(self) -> bool {
        matches!(self, Self::ContentBox)
    }
}

impl TransformOrigin {
    pub(crate) const INITIAL: Self = Self {
        x: ComputedLengthPercentage::from_percent(0.5),
        y: ComputedLengthPercentage::from_percent(0.5),
        z: ComputedLengthPercentage::ZERO,
        is_initial: true,
    };

    pub(crate) const fn specified(
        x: ComputedLengthPercentage,
        y: ComputedLengthPercentage,
        z: ComputedLengthPercentage,
    ) -> Self {
        Self {
            x,
            y,
            z,
            is_initial: false,
        }
    }

    pub(crate) fn resolve_font_metric_lengths(&mut self, ch_advance: LayoutLength) {
        self.x.resolve_font_metric_lengths(ch_advance);
        self.y.resolve_font_metric_lengths(ch_advance);
        self.z.resolve_font_metric_lengths(ch_advance);
    }

    pub(crate) fn resolve_root_font_metric_lengths(&mut self, basis: RootFontMetricLengthBasis) {
        self.x.resolve_root_font_metric_lengths(basis);
        self.y.resolve_root_font_metric_lengths(basis);
        self.z.resolve_root_font_metric_lengths(basis);
    }

    pub(crate) fn requires_ch_advance(&self) -> bool {
        self.x.requires_ch_advance() || self.y.requires_ch_advance() || self.z.requires_ch_advance()
    }

    pub(crate) fn requires_root_font_metrics(&self) -> bool {
        self.x.requires_root_font_metrics()
            || self.y.requires_root_font_metrics()
            || self.z.requires_root_font_metrics()
    }

    /// Resolve this CSS transform origin against a page-local transform
    /// reference box.
    ///
    /// CSS physical y coordinates start at the top edge, while `PaintPoint`
    /// uses PDF's bottom-left coordinate system.  The conversion belongs at
    /// this boundary so every transform function receives a paint-space
    /// origin.
    /// <https://www.w3.org/TR/css-transforms-1/#transform-origin-property>
    pub(crate) fn resolve_against_paint_rect(
        self,
        border_box: crate::document::paint::geometry::PaintRect,
    ) -> crate::document::paint::geometry::PaintPoint {
        crate::document::paint::geometry::PaintPoint::new(
            border_box.origin.x
                + self
                    .x
                    .used_length_with_percentage_basis(PercentageBasis::definite(layout_pt(
                        border_box.size.width,
                    )))
                    .map(layout_points)
                    .unwrap_or(0.0),
            border_box.origin.y + border_box.size.height
                - self
                    .y
                    .used_length_with_percentage_basis(PercentageBasis::definite(layout_pt(
                        border_box.size.height,
                    )))
                    .map(layout_points)
                    .unwrap_or(0.0),
        )
    }

    /// Resolve the complete three-dimensional origin in paint coordinates.
    pub(crate) fn resolve_3d_against_paint_rect(
        self,
        border_box: crate::document::paint::geometry::PaintRect,
    ) -> euclid::Point3D<f32, crate::document::paint::geometry::PaintSpace> {
        let z = self
            .z
            .used_length_with_percentage_basis(PercentageBasis::definite(layout_pt(0.0)))
            .map(layout_points)
            .unwrap_or(0.0);
        let xy = self.resolve_against_paint_rect(border_box);
        euclid::Point3D::new(xy.x, xy.y, z)
    }
}

impl ResolveViewportLengths for TransformFunction {
    fn resolve_viewport_lengths(&mut self, basis: ViewportLengthBasis) {
        match self {
            Self::Translate(translation) => {
                translation.x.resolve_viewport_lengths(basis);
                translation.y.resolve_viewport_lengths(basis);
            }
            Self::Translate3D(translation) => {
                translation.x.resolve_viewport_lengths(basis);
                translation.y.resolve_viewport_lengths(basis);
                translation.z.resolve_viewport_lengths(basis);
            }
            Self::Perspective(_) => {}
            _ => {}
        }
    }
}

impl ResolveViewportLengths for IndividualTransforms {
    fn resolve_viewport_lengths(&mut self, basis: ViewportLengthBasis) {
        if let Some(translation) = &mut self.translate {
            translation.x.resolve_viewport_lengths(basis);
            translation.y.resolve_viewport_lengths(basis);
        }
    }
}

impl ResolveViewportLengths for TransformOrigin {
    fn resolve_viewport_lengths(&mut self, basis: ViewportLengthBasis) {
        self.x.resolve_viewport_lengths(basis);
        self.y.resolve_viewport_lengths(basis);
        self.z.resolve_viewport_lengths(basis);
    }
}

impl ResolveViewportLengths for PerspectiveOrigin {
    fn resolve_viewport_lengths(&mut self, basis: ViewportLengthBasis) {
        self.x.resolve_viewport_lengths(basis);
        self.y.resolve_viewport_lengths(basis);
    }
}

/// Whether a used box may establish transform behavior.
///
/// CSS Transforms only applies to transformable elements. Ruby containers and
/// their internal role boxes are layout-internal inline structure rather than
/// independently transformable boxes; carrying this as an enum avoids
/// repeatedly treating `has_transform()` as the applicability decision.
/// <https://drafts.csswg.org/css-transforms-1/#transformable-element>
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TransformApplicability {
    Transformable,
    NonTransformableRubyInternal,
}

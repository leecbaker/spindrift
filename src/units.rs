//! Typed scalar units used across CSS computation, layout, paint, and PDF output.
//!
//! Quire's canonical layout scalar is a CSS computed absolute length measured
//! in PDF points. CSS Values and Units defines `1in = 96px`, while PDF default
//! user space uses 72 points per inch, so `1px = 0.75pt`:
//! <https://www.w3.org/TR/css-values-4/#absolute-lengths> and
//! ISO 32000-2:2020, 8.3 "Coordinate Systems".

/// Marker for Quire's canonical computed/layout length unit.
///
/// Values are stored numerically as PDF points. This names the scalar unit
/// separately from coordinate spaces such as paint space and PDF user space.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LayoutUnit {}

/// Marker for decoded raster image pixels.
///
/// Raster source pixels are image buffer coordinates, not CSS px or PDF
/// points. Keep them typed at the boundary so natural image dimensions must be
/// explicitly converted before entering layout.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RasterPixelUnit {}

/// Marker for an integer CSS-pixel image dimension.
///
/// This is deliberately distinct from [`RasterPixelUnit`]: a raster source's
/// preferred CSS size can come from validated image metadata rather than its
/// encoded sample grid.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CssPixelUnit {}

/// Marker for a CSS content-box length or size.
///
/// This is still stored in Quire's PDF-point layout scalar, but the marker
/// records the CSS box-model semantic space. Keeping content-box values
/// distinct from border-box values makes padding and border expansion explicit:
/// <https://www.w3.org/TR/css-box-3/#content-box> and
/// <https://www.w3.org/TR/css-sizing-3/#box-sizing>.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ContentBoxUnit {}

/// Marker for a CSS border-box length or size.
///
/// This is a semantic coordinate space over Quire's PDF-point layout scalar,
/// not a different physical unit. Conversions to or from content-box values
/// must explicitly add or subtract padding and border widths:
/// <https://www.w3.org/TR/css-box-3/#border-box> and
/// <https://www.w3.org/TR/css-sizing-3/#box-sizing>.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BorderBoxUnit {}

/// Marker for padding plus border extents used in box-model conversions.
///
/// Non-content lengths are CSS layout lengths measured in PDF points. They
/// intentionally do not share a unit marker with content-box or border-box
/// values, so callers must choose an explicit conversion helper:
/// <https://www.w3.org/TR/css-box-3/#box-model>.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum NonContentUnit {}

/// Marker for a CSS margin-box extent.
///
/// Margin boxes include the border box and used margins. They are useful for
/// float placement, where CSS 2.2 positions the margin box but collision and
/// painting still refer to the border box:
/// <https://www.w3.org/TR/CSS22/visuren.html#floats>.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MarginBoxUnit {}

/// A named text baseline measured from a CSS inline content-box block start.
///
/// This must not be used as a line-relative displacement: it has a content-box
/// origin and becomes comparable with another baseline only after rebasing on
/// each box's alphabetic baseline.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ContentBoxBaselineUnit {}

/// A named baseline measured relative to its box's alphabetic baseline.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AlphabeticBaselineRelativeUnit {}

/// The CSS Inline baseline-table displacement between a child and its direct
/// parent, before `baseline-shift` is applied.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BaselineTableAlignmentUnit {}

/// The used displacement authored through CSS `baseline-shift`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AuthorBaselineShiftUnit {}

/// A line-relative displacement that may be applied to a painted glyph
/// baseline origin.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum GlyphBaselineDisplacementUnit {}

/// A baseline exported by an atomic inline's own alignment source box.
///
/// The source can be the principal border box or, for an `inline-table`, the
/// table box. This is deliberately not a line-relative coordinate: callers
/// must rebase it through the atomic inline's margin box before using it for
/// line sizing or baseline alignment.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AtomicInlineBaselineSourceUnit {}

/// An atomic inline baseline measured from its logical margin-box block start.
///
/// CSS Inline uses this coordinate when calculating the atomic inline's line
/// contribution. It includes the logical block-start margin even when the
/// exported source is an inline table's table box.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AtomicInlineMarginBoxBaselineUnit {}

/// An atomic inline baseline coordinate used to place a captured paint box.
///
/// This stays distinct from [`AtomicInlineMarginBoxBaselineUnit`]: a captured
/// inline-table fragment starts at its table box and must not reapply the
/// wrapper's block-start margin during replay.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AtomicInlinePaintPlacementBaselineUnit {}

/// A CSS computed absolute length in Quire's canonical layout unit.
pub(crate) type LayoutLength = euclid::Length<f32, LayoutUnit>;

/// A CSS computed size in Quire's canonical layout unit.
pub(crate) type LayoutSize = euclid::Size2D<f32, LayoutUnit>;

/// A decoded raster image size in source pixels.
pub(crate) type RasterPixelSize = euclid::Size2D<u32, RasterPixelUnit>;

/// A raster image's preferred natural size in CSS pixels.
///
/// HTML image metadata can provide this independently of the raster sample
/// dimensions. It becomes a [`LayoutSize`] only at the CSS layout boundary.
pub(crate) type CssPixelSize = euclid::Size2D<u32, CssPixelUnit>;

/// A CSS content-box length in Quire's PDF-point layout scalar.
pub(crate) type ContentBoxLength = euclid::Length<f32, ContentBoxUnit>;

/// A CSS border-box length in Quire's PDF-point layout scalar.
pub(crate) type BorderBoxLength = euclid::Length<f32, BorderBoxUnit>;

/// Padding plus border extent in Quire's PDF-point layout scalar.
pub(crate) type NonContentLength = euclid::Length<f32, NonContentUnit>;

/// A CSS margin-box extent in Quire's PDF-point layout scalar.
pub(crate) type MarginBoxLength = euclid::Length<f32, MarginBoxUnit>;

pub(crate) type ContentBoxBaselineOffset = euclid::Length<f32, ContentBoxBaselineUnit>;
pub(crate) type AlphabeticBaselineRelativeOffset =
    euclid::Length<f32, AlphabeticBaselineRelativeUnit>;
pub(crate) type BaselineTableAlignmentDelta = euclid::Length<f32, BaselineTableAlignmentUnit>;
pub(crate) type AuthorBaselineShift = euclid::Length<f32, AuthorBaselineShiftUnit>;
pub(crate) type GlyphBaselineDisplacement = euclid::Length<f32, GlyphBaselineDisplacementUnit>;
pub(crate) type AtomicInlineBaselineSourceOffset =
    euclid::Length<f32, AtomicInlineBaselineSourceUnit>;
pub(crate) type AtomicInlineMarginBoxBaselineOffset =
    euclid::Length<f32, AtomicInlineMarginBoxBaselineUnit>;
pub(crate) type AtomicInlinePaintPlacementBaselineOffset =
    euclid::Length<f32, AtomicInlinePaintPlacementBaselineUnit>;

/// A physical CSS margin-box size in Quire's PDF-point layout coordinates.
///
/// Float placement uses both physical dimensions of the margin box, while
/// collision of BFC roots continues to use their border boxes:
/// <https://www.w3.org/TR/CSS22/visuren.html#floats>.
pub(crate) type MarginBoxSize = euclid::Size2D<f32, MarginBoxUnit>;

/// A CSS content-box size in Quire's PDF-point layout scalar.
pub(crate) type ContentBoxSize = euclid::Size2D<f32, ContentBoxUnit>;

/// A CSS border-box size in Quire's PDF-point layout scalar.
pub(crate) type BorderBoxSize = euclid::Size2D<f32, BorderBoxUnit>;

/// Construct a layout length from PDF points.
pub(crate) const fn layout_pt(value: f32) -> LayoutLength {
    LayoutLength::new(value)
}

/// Construct a layout length from CSS pixels.
pub(crate) const fn layout_px(value: f32) -> LayoutLength {
    layout_pt(value * crate::css::CSS_PX_TO_PT)
}

/// Construct a content-box length from PDF points.
pub(crate) const fn content_box_pt(value: f32) -> ContentBoxLength {
    ContentBoxLength::new(value)
}

/// Construct a border-box length from PDF points.
pub(crate) const fn border_box_pt(value: f32) -> BorderBoxLength {
    BorderBoxLength::new(value)
}

/// Construct a padding-plus-border length from PDF points.
pub(crate) const fn non_content_pt(value: f32) -> NonContentLength {
    NonContentLength::new(value)
}

/// Construct a margin-box extent from PDF points.
pub(crate) const fn margin_box_pt(value: f32) -> MarginBoxLength {
    MarginBoxLength::new(value)
}

pub(crate) const fn content_box_baseline_pt(value: f32) -> ContentBoxBaselineOffset {
    ContentBoxBaselineOffset::new(value)
}

pub(crate) const fn alphabetic_baseline_relative_pt(
    value: f32,
) -> AlphabeticBaselineRelativeOffset {
    AlphabeticBaselineRelativeOffset::new(value)
}

pub(crate) const fn baseline_table_alignment_pt(value: f32) -> BaselineTableAlignmentDelta {
    BaselineTableAlignmentDelta::new(value)
}

pub(crate) const fn author_baseline_shift_pt(value: f32) -> AuthorBaselineShift {
    AuthorBaselineShift::new(value)
}

pub(crate) const fn glyph_baseline_displacement_pt(value: f32) -> GlyphBaselineDisplacement {
    GlyphBaselineDisplacement::new(value)
}

pub(crate) const fn atomic_inline_baseline_source_pt(
    value: f32,
) -> AtomicInlineBaselineSourceOffset {
    AtomicInlineBaselineSourceOffset::new(value)
}

pub(crate) const fn atomic_inline_margin_box_baseline_pt(
    value: f32,
) -> AtomicInlineMarginBoxBaselineOffset {
    AtomicInlineMarginBoxBaselineOffset::new(value)
}

pub(crate) const fn atomic_inline_paint_placement_baseline_pt(
    value: f32,
) -> AtomicInlinePaintPlacementBaselineOffset {
    AtomicInlinePaintPlacementBaselineOffset::new(value)
}

/// Construct a physical CSS margin-box size from PDF points.
pub(crate) const fn margin_box_size_pt(width: f32, height: f32) -> MarginBoxSize {
    MarginBoxSize::new(width, height)
}

/// Construct a content-box size from PDF points.
pub(crate) const fn content_box_size_pt(width: f32, height: f32) -> ContentBoxSize {
    ContentBoxSize::new(width, height)
}

/// Construct a border-box size from PDF points.
pub(crate) const fn border_box_size_pt(width: f32, height: f32) -> BorderBoxSize {
    BorderBoxSize::new(width, height)
}

/// Return the numeric PDF-point value of a layout length.
pub(crate) fn layout_points(length: LayoutLength) -> f32 {
    length.get()
}

/// Re-label a generic layout length as a content-box length without changing
/// its PDF-point value.
///
/// The caller establishes that the value denotes a content-box extent; this
/// helper does not remove padding or border.
pub(crate) fn layout_to_content_box_length(length: LayoutLength) -> ContentBoxLength {
    length.cast_unit()
}

/// Re-label a generic layout length as a border-box length without changing
/// its PDF-point value.
///
/// The caller establishes that the value denotes a border-box extent; this
/// helper does not add padding or border.
pub(crate) fn layout_to_border_box_length(length: LayoutLength) -> BorderBoxLength {
    length.cast_unit()
}

/// Extract the numeric PDF-point value from a typed layout length.
pub(crate) trait SemanticLengthExt {
    /// Return this typed length in Quire's canonical PDF-point layout scalar.
    fn points(self) -> f32;
}

impl<Unit> SemanticLengthExt for euclid::Length<f32, Unit> {
    fn points(self) -> f32 {
        self.get()
    }
}

/// Re-label a semantic CSS length as a generic layout length without changing
/// its PDF-point value.
///
/// This is only a unit-marker conversion. It never performs box-model
/// arithmetic; use the named content-box/border-box helpers when padding and
/// border must be added or removed.
pub(crate) trait IntoLayoutLength {
    /// Return this length as Quire's generic layout length.
    fn into_layout_length(self) -> LayoutLength;
}

impl IntoLayoutLength for LayoutLength {
    fn into_layout_length(self) -> LayoutLength {
        self
    }
}

impl IntoLayoutLength for ContentBoxLength {
    fn into_layout_length(self) -> LayoutLength {
        self.cast_unit()
    }
}

impl IntoLayoutLength for BorderBoxLength {
    fn into_layout_length(self) -> LayoutLength {
        self.cast_unit()
    }
}

impl IntoLayoutLength for NonContentLength {
    fn into_layout_length(self) -> LayoutLength {
        self.cast_unit()
    }
}

impl IntoLayoutLength for MarginBoxLength {
    fn into_layout_length(self) -> LayoutLength {
        self.cast_unit()
    }
}

/// A CSS percentage-resolution basis that may or may not be definite.
///
/// CSS Sizing resolves percentage components only when the relevant containing
/// block axis is definite. The source records why a definite basis exists at
/// the layout boundary:
/// <https://www.w3.org/TR/css-sizing-3/#definite>.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum PercentageBasis<T, Source = ()> {
    Definite { value: T, source: Source },
    Indefinite,
}

/// A value whose CSS sizing definiteness has been established at a layout
/// boundary.
///
/// CSS Sizing distinguishes an available numeric size from a definite size:
/// only the latter may resolve percentage-dependent layout or justify a
/// definite-size preflight. Keeping that fact in the type prevents callers
/// from replacing an absent definite size with a numeric fallback.
/// <https://www.w3.org/TR/css-sizing-3/#definite>
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(transparent)]
pub(crate) struct Definite<T>(T);

impl<T> Definite<T> {
    pub(crate) fn new(value: T) -> Self {
        Self(value)
    }

    pub(crate) fn value(self) -> T {
        self.0
    }
}

impl<T> PercentageBasis<T, ()> {
    pub(crate) fn definite(value: T) -> Self {
        Self::Definite { value, source: () }
    }
}

impl<T, Source> PercentageBasis<T, Source> {
    pub(crate) fn definite_from(value: T, source: Source) -> Self {
        Self::Definite { value, source }
    }

    pub(crate) fn indefinite() -> Self {
        Self::Indefinite
    }

    pub(crate) fn is_definite(&self) -> bool {
        matches!(self, Self::Definite { .. })
    }

    pub(crate) fn value(self) -> Option<T> {
        match self {
            Self::Definite { value, .. } => Some(value),
            Self::Indefinite => None,
        }
    }

    pub(crate) fn map_value<U>(self, map: impl FnOnce(T) -> U) -> PercentageBasis<U, Source> {
        match self {
            Self::Definite { value, source } => PercentageBasis::Definite {
                value: map(value),
                source,
            },
            Self::Indefinite => PercentageBasis::Indefinite,
        }
    }

    /// Preserve this basis's value and definiteness while changing the
    /// provenance carried at a formatting-context boundary.
    ///
    /// Percentage-basis provenance is meaningful to layout algorithms, but a
    /// child formatting context may need to record the same definite value in
    /// its own source domain. This avoids extracting and reconstructing the
    /// value merely to change that metadata.
    pub(crate) fn map_source<NewSource>(
        self,
        map: impl FnOnce(Source) -> NewSource,
    ) -> PercentageBasis<T, NewSource> {
        match self {
            Self::Definite { value, source } => PercentageBasis::Definite {
                value,
                source: map(source),
            },
            Self::Indefinite => PercentageBasis::Indefinite,
        }
    }
}

impl<T, Source> PercentageBasis<T, Source>
where
    T: SemanticLengthExt,
{
    pub(crate) fn points(self) -> Option<f32> {
        self.value().map(SemanticLengthExt::points)
    }
}

/// Expand a content-box length by padding and border extents.
pub(crate) fn content_box_to_border_box_length(
    content: ContentBoxLength,
    extras: NonContentLength,
) -> BorderBoxLength {
    border_box_pt((content.points() + extras.points()).max(0.0))
}

/// Expand a content-box length through its border box to its margin box.
///
/// The margin contribution remains a signed generic layout length because CSS
/// margins can be negative. Callers must construct it at the CSS used-value
/// boundary; this helper is the only box-model conversion step that relabels
/// the result as a margin-box extent.
pub(crate) fn content_box_to_margin_box_length(
    content: ContentBoxLength,
    non_content: NonContentLength,
    margins: LayoutLength,
) -> MarginBoxLength {
    margin_box_pt(content.points() + non_content.points() + margins.points())
}

/// Shrink a border-box length by padding and border extents, clamping at zero.
pub(crate) fn border_box_to_content_box_length(
    border: BorderBoxLength,
    extras: NonContentLength,
) -> ContentBoxLength {
    content_box_pt((border.points() - extras.points()).max(0.0))
}

/// Expand a content-box size by horizontal and vertical padding/border extents.
pub(crate) fn content_box_to_border_box_size(
    content: ContentBoxSize,
    horizontal_extras: NonContentLength,
    vertical_extras: NonContentLength,
) -> BorderBoxSize {
    border_box_size_pt(
        (content.width + horizontal_extras.points()).max(0.0),
        (content.height + vertical_extras.points()).max(0.0),
    )
}

/// Shrink a border-box size by horizontal and vertical padding/border extents.
///
/// The result clamps at zero on each axis, matching CSS used-value behavior for
/// over-constrained content boxes.
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn border_box_to_content_box_size(
    border: BorderBoxSize,
    horizontal_extras: NonContentLength,
    vertical_extras: NonContentLength,
) -> ContentBoxSize {
    content_box_size_pt(
        (border.width - horizontal_extras.points()).max(0.0),
        (border.height - vertical_extras.points()).max(0.0),
    )
}

/// Convert an image's preferred natural CSS-pixel dimensions into layout
/// dimensions.
///
/// CSS Values fixes `1px = 1/96in`, while Quire's layout unit is PDF points,
/// so each CSS pixel contributes `0.75pt`:
/// <https://www.w3.org/TR/css-values-4/#absolute-lengths>.
pub(crate) fn css_pixel_natural_layout_size(size: CssPixelSize) -> LayoutSize {
    LayoutSize::new(
        layout_points(layout_px(size.width as f32)),
        layout_points(layout_px(size.height as f32)),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn semantic_relabeling_preserves_points_without_box_model_arithmetic() {
        let layout: LayoutLength = content_box_pt(42.0).into_layout_length();
        let content: ContentBoxLength = layout_to_content_box_length(layout);

        assert_eq!(layout, layout_pt(42.0));
        assert_eq!(content, content_box_pt(42.0));
        assert_eq!(border_box_pt(42.0).into_layout_length(), layout_pt(42.0));
        assert_eq!(non_content_pt(42.0).into_layout_length(), layout_pt(42.0));
    }

    #[test]
    fn content_box_size_expands_to_border_box_size() {
        let border = content_box_to_border_box_size(
            content_box_size_pt(150.0, 150.0),
            non_content_pt(150.0),
            non_content_pt(150.0),
        );
        assert_eq!(border.width, 300.0);
        assert_eq!(border.height, 300.0);
    }

    #[test]
    fn border_box_size_shrinks_to_non_negative_content_box_size() {
        let content = border_box_to_content_box_size(
            border_box_size_pt(100.0, 100.0),
            non_content_pt(150.0),
            non_content_pt(150.0),
        );
        assert_eq!(content.width, 0.0);
        assert_eq!(content.height, 0.0);
    }

    #[test]
    fn content_box_to_margin_box_keeps_signed_used_margins_explicit() {
        assert_eq!(
            content_box_to_margin_box_length(
                content_box_pt(40.0),
                non_content_pt(10.0),
                layout_pt(-15.0),
            ),
            margin_box_pt(35.0)
        );
    }

    #[test]
    fn percentage_basis_map_source_preserves_value_and_indefiniteness() {
        #[derive(Debug, Clone, Copy, PartialEq)]
        enum Source {
            Parent,
            Child,
        }

        let definite = PercentageBasis::definite_from(content_box_pt(42.0), Source::Parent)
            .map_source(|_| Source::Child);
        assert_eq!(
            definite,
            PercentageBasis::definite_from(content_box_pt(42.0), Source::Child)
        );

        let indefinite: PercentageBasis<ContentBoxLength, Source> = PercentageBasis::indefinite();
        assert_eq!(
            indefinite.map_source(|_| Source::Child),
            PercentageBasis::indefinite()
        );
    }
}

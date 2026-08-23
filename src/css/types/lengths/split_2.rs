use std::ops::{Deref, DerefMut};

use super::*;

/// Computed `flex-basis` value.
///
/// CSS Flexbox defines `flex-basis` as `content | <width>`, where `<width>`
/// includes intrinsic sizing keywords, `<length-percentage>`, and `auto`. The
/// `content` keyword is not a generic box-size value: it forces content-based
/// flex base sizing instead of retrieving the main-size property like `auto`:
/// <https://www.w3.org/TR/css-flexbox-1/#flex-basis-property> and
/// <https://www.w3.org/TR/css-sizing-3/#intrinsic-sizes>.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum ComputedFlexBasis {
    Auto,
    Content,
    MinContent,
    MaxContent,
    FitContent(Option<ComputedLengthPercentage>),
    LengthPercentage(ComputedFlexBasisLength),
}

/// Computed `<length-percentage>` used by `flex-basis`.
///
/// CSS Flexbox resolves percentages in `flex-basis` against the flex
/// container's inner main size, and falls back to `content` when that size is
/// indefinite. Percentage presence, including authored `0%`, belongs to the
/// unified `<length-percentage>` representation:
/// <https://www.w3.org/TR/css-flexbox-1/#flex-basis-property>.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ComputedFlexBasisLength {
    pub value: ComputedLengthPercentage,
}

impl ComputedFlexBasisLength {
    pub(crate) fn new(value: ComputedLengthPercentage) -> Self {
        Self { value }
    }

    pub(crate) fn contains_percentage(&self) -> bool {
        self.value.contains_percentage()
    }
}

impl ComputedFlexBasis {
    pub(crate) const AUTO: Self = Self::Auto;

    /// Scale the fixed components of a flex basis at the CSS `zoom`
    /// used-value boundary.
    ///
    /// Percentages intentionally remain unscaled: CSS Flexbox resolves them
    /// against the flex container's already zoomed inner main size.  Intrinsic
    /// keywords and `auto` likewise remain algorithmic values.
    /// <https://drafts.csswg.org/css-viewport/#zoom-property>
    /// <https://drafts.csswg.org/css-flexbox-1/#flex-basis-property>
    pub(crate) fn scale_fixed_length_components(&mut self, factor: f32) {
        match self {
            Self::FitContent(Some(value)) => value.scale_fixed_length_components(factor),
            Self::LengthPercentage(value) => value.value.scale_fixed_length_components(factor),
            Self::Auto
            | Self::Content
            | Self::MinContent
            | Self::MaxContent
            | Self::FitContent(None) => {}
        }
    }

    pub(crate) fn resolve_font_metric_lengths(&mut self, ch_advance: LayoutLength) {
        match self {
            Self::FitContent(Some(value)) => {
                value.resolve_font_metric_lengths(ch_advance);
            }
            Self::LengthPercentage(value) => value.value.resolve_font_metric_lengths(ch_advance),
            Self::Auto
            | Self::Content
            | Self::MinContent
            | Self::MaxContent
            | Self::FitContent(None) => {}
        }
    }

    pub(crate) fn resolve_root_font_metric_lengths(&mut self, basis: RootFontMetricLengthBasis) {
        match self {
            Self::FitContent(Some(value)) => value.resolve_root_font_metric_lengths(basis),
            Self::LengthPercentage(value) => value.value.resolve_root_font_metric_lengths(basis),
            Self::Auto
            | Self::Content
            | Self::MinContent
            | Self::MaxContent
            | Self::FitContent(None) => {}
        }
    }

    pub(crate) fn requires_ch_advance(&self) -> bool {
        match self {
            Self::FitContent(Some(value)) => value.requires_ch_advance(),
            Self::LengthPercentage(value) => value.value.requires_ch_advance(),
            Self::Auto
            | Self::Content
            | Self::MinContent
            | Self::MaxContent
            | Self::FitContent(None) => false,
        }
    }

    pub(crate) fn requires_root_font_metrics(&self) -> bool {
        match self {
            Self::FitContent(Some(value)) => value.requires_root_font_metrics(),
            Self::LengthPercentage(value) => value.value.requires_root_font_metrics(),
            Self::Auto
            | Self::Content
            | Self::MinContent
            | Self::MaxContent
            | Self::FitContent(None) => false,
        }
    }
}

/// Four physical CSS edges in top/right/bottom/left order.
///
/// CSS Box Model Level 3 defines physical margin, padding, and border edge
/// properties in this order:
/// <https://www.w3.org/TR/css-box-3/#the-margin-properties>.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct CssEdges<T> {
    pub top: T,
    pub right: T,
    pub bottom: T,
    pub left: T,
}

impl<T: Clone> CssEdges<T> {
    pub(crate) fn all(value: T) -> Self {
        Self {
            top: value.clone(),
            right: value.clone(),
            bottom: value.clone(),
            left: value,
        }
    }
}

/// Typed computed box-model values retained until layout resolves used values.
///
/// CSS Cascade defines computed values:
/// <https://www.w3.org/TR/css-cascade-5/#computed>.
/// CSS 2.2 defines used widths, margins, padding, and positioned offsets:
/// <https://www.w3.org/TR/CSS22/visudet.html>,
/// <https://www.w3.org/TR/CSS22/box.html>, and
/// <https://www.w3.org/TR/CSS22/visuren.html#relative-positioning>.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ComputedBoxValues {
    pub margin: CssEdges<ComputedLengthPercentageOrAuto>,
    pub padding: CssEdges<ComputedLengthPercentage>,
    pub width: ComputedLengthPercentageOrAuto,
    pub height: PhysicalHeight,
    pub min_width: ComputedLengthPercentageOrAuto,
    pub max_width: ComputedLengthPercentageOrAuto,
    pub min_height: ComputedLengthPercentageOrAuto,
    pub max_height: ComputedLengthPercentageOrAuto,
    pub inset_left: ComputedLengthPercentageOrAuto,
    pub inset_top: ComputedLengthPercentageOrAuto,
    pub inset_right: ComputedLengthPercentageOrAuto,
    pub inset_bottom: ComputedLengthPercentageOrAuto,
}

/// A physical `height` together with the used-value lifecycle of its selected
/// font metric.
///
/// An orthogonal table row must resolve `ch` against its own track context.
/// The enum keeps that fact attached to the exact computed value until layout
/// intentionally substitutes a definite used height or `auto`.
/// <https://www.w3.org/TR/css-values-4/#font-relative-lengths>
/// <https://www.w3.org/TR/css-tables-3/#row-layout>
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum PhysicalHeight {
    Resolved(ComputedLengthPercentageOrAuto),
    DeferredFontMetric(ComputedLengthPercentageOrAuto),
}

impl PhysicalHeight {
    pub(crate) const AUTO: Self = Self::Resolved(ComputedLengthPercentageOrAuto::AUTO);

    pub(crate) fn from_computed(value: ComputedLengthPercentageOrAuto) -> Self {
        if value.requires_ch_advance() {
            Self::DeferredFontMetric(value)
        } else {
            Self::Resolved(value)
        }
    }

    pub(crate) const fn is_deferred_font_metric(&self) -> bool {
        matches!(self, Self::DeferredFontMetric(_))
    }

    pub(crate) fn value(&self) -> &ComputedLengthPercentageOrAuto {
        match self {
            Self::Resolved(value) | Self::DeferredFontMetric(value) => value,
        }
    }

    pub(crate) fn value_mut(&mut self) -> &mut ComputedLengthPercentageOrAuto {
        match self {
            Self::Resolved(value) | Self::DeferredFontMetric(value) => value,
        }
    }

    pub(crate) fn replace_with_used(&mut self, value: ComputedLengthPercentageOrAuto) {
        *self = Self::Resolved(value);
    }
}

impl Deref for PhysicalHeight {
    type Target = ComputedLengthPercentageOrAuto;

    fn deref(&self) -> &Self::Target {
        self.value()
    }
}

impl DerefMut for PhysicalHeight {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.value_mut()
    }
}

impl PartialEq<ComputedLengthPercentageOrAuto> for PhysicalHeight {
    fn eq(&self, other: &ComputedLengthPercentageOrAuto) -> bool {
        self.value() == other
    }
}

impl ComputedBoxValues {
    /// Resolve every element-local font metric using the element's one inline
    /// formatting context.  CSS Values does not select a separate metric for
    /// each physical box edge.
    /// <https://drafts.csswg.org/css-values-4/#font-relative-lengths>
    pub(crate) fn resolve_selected_font_metric_lengths(
        &mut self,
        basis: SelectedFontMetricLengthBasis,
    ) {
        self.resolve_ch_relative_lengths(basis.ch_advance);
        self.resolve_ic_relative_lengths(basis.ic_advance);
        self.resolve_ex_relative_lengths(basis.x_height.points());
        self.resolve_cap_relative_lengths(basis.cap_height.points());
    }

    /// Scale fixed box-model length components for CSS `zoom`.
    pub(crate) fn scale_fixed_length_components(&mut self, factor: f32) {
        for value in [
            &mut self.margin.top,
            &mut self.margin.right,
            &mut self.margin.bottom,
            &mut self.margin.left,
            &mut self.width,
            &mut self.height,
            &mut self.min_width,
            &mut self.max_width,
            &mut self.min_height,
            &mut self.max_height,
            &mut self.inset_left,
            &mut self.inset_top,
            &mut self.inset_right,
            &mut self.inset_bottom,
        ] {
            value.scale_fixed_length_components(factor);
        }
        for value in [
            &mut self.padding.top,
            &mut self.padding.right,
            &mut self.padding.bottom,
            &mut self.padding.left,
        ] {
            value.scale_fixed_length_components(factor);
        }
    }

    /// Resolves `ch` terms by physical box axis.
    ///
    /// In vertical writing, the glyph's horizontal and vertical advances can
    /// differ. Physical width/left/right values therefore cannot share the
    /// basis used by physical height/top/bottom values.
    /// <https://www.w3.org/TR/css-values-4/#font-relative-lengths>
    pub(crate) fn resolve_ch_relative_lengths(&mut self, advance: LayoutLength) {
        let horizontal_advance = advance;
        let vertical_advance = advance;
        self.margin
            .top
            .resolve_font_metric_lengths(vertical_advance);
        self.margin
            .right
            .resolve_font_metric_lengths(horizontal_advance);
        self.margin
            .bottom
            .resolve_font_metric_lengths(vertical_advance);
        self.margin
            .left
            .resolve_font_metric_lengths(horizontal_advance);
        self.padding
            .top
            .resolve_font_metric_lengths(vertical_advance);
        self.padding
            .right
            .resolve_font_metric_lengths(horizontal_advance);
        self.padding
            .bottom
            .resolve_font_metric_lengths(vertical_advance);
        self.padding
            .left
            .resolve_font_metric_lengths(horizontal_advance);
        self.width.resolve_font_metric_lengths(horizontal_advance);
        self.height.resolve_font_metric_lengths(vertical_advance);
        self.min_width
            .resolve_font_metric_lengths(horizontal_advance);
        self.max_width
            .resolve_font_metric_lengths(horizontal_advance);
        self.min_height
            .resolve_font_metric_lengths(vertical_advance);
        self.max_height
            .resolve_font_metric_lengths(vertical_advance);
        self.inset_left
            .resolve_font_metric_lengths(horizontal_advance);
        self.inset_top.resolve_font_metric_lengths(vertical_advance);
        self.inset_right
            .resolve_font_metric_lengths(horizontal_advance);
        self.inset_bottom
            .resolve_font_metric_lengths(vertical_advance);
    }

    pub(crate) fn initial() -> Self {
        Self {
            margin: CssEdges::all(ComputedLengthPercentageOrAuto::ZERO),
            padding: CssEdges::all(ComputedLengthPercentage::ZERO),
            width: ComputedLengthPercentageOrAuto::AUTO,
            height: PhysicalHeight::AUTO,
            min_width: ComputedLengthPercentageOrAuto::AUTO,
            max_width: ComputedLengthPercentageOrAuto::AUTO,
            min_height: ComputedLengthPercentageOrAuto::AUTO,
            max_height: ComputedLengthPercentageOrAuto::AUTO,
            inset_left: ComputedLengthPercentageOrAuto::AUTO,
            inset_top: ComputedLengthPercentageOrAuto::AUTO,
            inset_right: ComputedLengthPercentageOrAuto::AUTO,
            inset_bottom: ComputedLengthPercentageOrAuto::AUTO,
        }
    }

    /// Resolves box-model font-relative components once the element's used
    /// font metrics are available.
    /// <https://www.w3.org/TR/css-values-4/#font-relative-lengths>
    #[allow(dead_code)]
    pub(crate) fn resolve_font_relative_lengths(&mut self, basis: FontRelativeLengthBasis) {
        self.margin.top.resolve_font_relative_lengths(basis);
        self.margin.right.resolve_font_relative_lengths(basis);
        self.margin.bottom.resolve_font_relative_lengths(basis);
        self.margin.left.resolve_font_relative_lengths(basis);
        self.padding.top.resolve_font_relative_lengths(basis);
        self.padding.right.resolve_font_relative_lengths(basis);
        self.padding.bottom.resolve_font_relative_lengths(basis);
        self.padding.left.resolve_font_relative_lengths(basis);
        self.width.resolve_font_relative_lengths(basis);
        self.height.resolve_font_relative_lengths(basis);
        self.min_width.resolve_font_relative_lengths(basis);
        self.max_width.resolve_font_relative_lengths(basis);
        self.min_height.resolve_font_relative_lengths(basis);
        self.max_height.resolve_font_relative_lengths(basis);
        self.inset_left.resolve_font_relative_lengths(basis);
        self.inset_top.resolve_font_relative_lengths(basis);
        self.inset_right.resolve_font_relative_lengths(basis);
        self.inset_bottom.resolve_font_relative_lengths(basis);
    }

    /// Resolves the ordinary font-size-relative portion of box-model values
    /// during computed-value finalization.
    /// <https://www.w3.org/TR/css-values-4/#font-relative-lengths>
    pub(crate) fn resolve_em_relative_lengths(&mut self, font_size: LayoutLength) {
        self.margin.top.resolve_em_relative_lengths(font_size);
        self.margin.right.resolve_em_relative_lengths(font_size);
        self.margin.bottom.resolve_em_relative_lengths(font_size);
        self.margin.left.resolve_em_relative_lengths(font_size);
        self.padding.top.resolve_em_relative_lengths(font_size);
        self.padding.right.resolve_em_relative_lengths(font_size);
        self.padding.bottom.resolve_em_relative_lengths(font_size);
        self.padding.left.resolve_em_relative_lengths(font_size);
        self.width.resolve_em_relative_lengths(font_size);
        self.height.resolve_em_relative_lengths(font_size);
        self.min_width.resolve_em_relative_lengths(font_size);
        self.max_width.resolve_em_relative_lengths(font_size);
        self.min_height.resolve_em_relative_lengths(font_size);
        self.max_height.resolve_em_relative_lengths(font_size);
        self.inset_left.resolve_em_relative_lengths(font_size);
        self.inset_top.resolve_em_relative_lengths(font_size);
        self.inset_right.resolve_em_relative_lengths(font_size);
        self.inset_bottom.resolve_em_relative_lengths(font_size);
    }

    /// Resolves root-font-relative box-model components after the document
    /// root's used font size is known.
    /// <https://www.w3.org/TR/css-values-4/#rem>
    pub(crate) fn resolve_root_font_relative_lengths(&mut self, root_font_size: f32) {
        self.margin
            .top
            .resolve_root_font_relative_lengths(root_font_size);
        self.margin
            .right
            .resolve_root_font_relative_lengths(root_font_size);
        self.margin
            .bottom
            .resolve_root_font_relative_lengths(root_font_size);
        self.margin
            .left
            .resolve_root_font_relative_lengths(root_font_size);
        self.padding
            .top
            .resolve_root_font_relative_lengths(root_font_size);
        self.padding
            .right
            .resolve_root_font_relative_lengths(root_font_size);
        self.padding
            .bottom
            .resolve_root_font_relative_lengths(root_font_size);
        self.padding
            .left
            .resolve_root_font_relative_lengths(root_font_size);
        self.width
            .resolve_root_font_relative_lengths(root_font_size);
        self.height
            .resolve_root_font_relative_lengths(root_font_size);
        self.min_width
            .resolve_root_font_relative_lengths(root_font_size);
        self.max_width
            .resolve_root_font_relative_lengths(root_font_size);
        self.min_height
            .resolve_root_font_relative_lengths(root_font_size);
        self.max_height
            .resolve_root_font_relative_lengths(root_font_size);
        self.inset_left
            .resolve_root_font_relative_lengths(root_font_size);
        self.inset_top
            .resolve_root_font_relative_lengths(root_font_size);
        self.inset_right
            .resolve_root_font_relative_lengths(root_font_size);
        self.inset_bottom
            .resolve_root_font_relative_lengths(root_font_size);
    }

    /// Resolves `ic` terms by physical box axis.
    /// <https://www.w3.org/TR/css-values-4/#font-relative-lengths>
    fn resolve_ic_relative_lengths(&mut self, advance: LayoutLength) {
        let horizontal_advance = advance;
        let vertical_advance = advance;
        self.margin
            .top
            .resolve_ic_relative_lengths(vertical_advance);
        self.margin
            .right
            .resolve_ic_relative_lengths(horizontal_advance);
        self.margin
            .bottom
            .resolve_ic_relative_lengths(vertical_advance);
        self.margin
            .left
            .resolve_ic_relative_lengths(horizontal_advance);
        self.padding
            .top
            .resolve_ic_relative_lengths(vertical_advance);
        self.padding
            .right
            .resolve_ic_relative_lengths(horizontal_advance);
        self.padding
            .bottom
            .resolve_ic_relative_lengths(vertical_advance);
        self.padding
            .left
            .resolve_ic_relative_lengths(horizontal_advance);
        self.width.resolve_ic_relative_lengths(horizontal_advance);
        self.height.resolve_ic_relative_lengths(vertical_advance);
        self.min_width
            .resolve_ic_relative_lengths(horizontal_advance);
        self.max_width
            .resolve_ic_relative_lengths(horizontal_advance);
        self.min_height
            .resolve_ic_relative_lengths(vertical_advance);
        self.max_height
            .resolve_ic_relative_lengths(vertical_advance);
        self.inset_left
            .resolve_ic_relative_lengths(horizontal_advance);
        self.inset_top.resolve_ic_relative_lengths(vertical_advance);
        self.inset_right
            .resolve_ic_relative_lengths(horizontal_advance);
        self.inset_bottom
            .resolve_ic_relative_lengths(vertical_advance);
    }

    /// Resolves `ex` components after selecting the element's font.
    /// <https://www.w3.org/TR/css-values-4/#ex>
    pub(crate) fn resolve_ex_relative_lengths(&mut self, x_height: f32) {
        self.margin.top.resolve_ex_relative_lengths(x_height);
        self.margin.right.resolve_ex_relative_lengths(x_height);
        self.margin.bottom.resolve_ex_relative_lengths(x_height);
        self.margin.left.resolve_ex_relative_lengths(x_height);
        self.padding.top.resolve_ex_relative_lengths(x_height);
        self.padding.right.resolve_ex_relative_lengths(x_height);
        self.padding.bottom.resolve_ex_relative_lengths(x_height);
        self.padding.left.resolve_ex_relative_lengths(x_height);
        self.width.resolve_ex_relative_lengths(x_height);
        self.height.resolve_ex_relative_lengths(x_height);
        self.min_width.resolve_ex_relative_lengths(x_height);
        self.max_width.resolve_ex_relative_lengths(x_height);
        self.min_height.resolve_ex_relative_lengths(x_height);
        self.max_height.resolve_ex_relative_lengths(x_height);
        self.inset_left.resolve_ex_relative_lengths(x_height);
        self.inset_top.resolve_ex_relative_lengths(x_height);
        self.inset_right.resolve_ex_relative_lengths(x_height);
        self.inset_bottom.resolve_ex_relative_lengths(x_height);
    }

    /// Resolves `cap` components after selecting the element's font.
    /// <https://www.w3.org/TR/css-values-4/#cap>
    pub(crate) fn resolve_cap_relative_lengths(&mut self, cap_height: f32) {
        self.margin.top.resolve_cap_relative_lengths(cap_height);
        self.margin.right.resolve_cap_relative_lengths(cap_height);
        self.margin.bottom.resolve_cap_relative_lengths(cap_height);
        self.margin.left.resolve_cap_relative_lengths(cap_height);
        self.padding.top.resolve_cap_relative_lengths(cap_height);
        self.padding.right.resolve_cap_relative_lengths(cap_height);
        self.padding.bottom.resolve_cap_relative_lengths(cap_height);
        self.padding.left.resolve_cap_relative_lengths(cap_height);
        self.width.resolve_cap_relative_lengths(cap_height);
        self.height.resolve_cap_relative_lengths(cap_height);
        self.min_width.resolve_cap_relative_lengths(cap_height);
        self.max_width.resolve_cap_relative_lengths(cap_height);
        self.min_height.resolve_cap_relative_lengths(cap_height);
        self.max_height.resolve_cap_relative_lengths(cap_height);
        self.inset_left.resolve_cap_relative_lengths(cap_height);
        self.inset_top.resolve_cap_relative_lengths(cap_height);
        self.inset_right.resolve_cap_relative_lengths(cap_height);
        self.inset_bottom.resolve_cap_relative_lengths(cap_height);
    }

    /// Resolves ordinary `lh` components against this element's computed line
    /// height. The `line-height` property itself is resolved separately
    /// against its inherited basis.
    /// <https://www.w3.org/TR/css-values-4/#lh>
    pub(crate) fn resolve_line_height_relative_lengths(&mut self, line_height: LayoutLength) {
        self.margin
            .top
            .resolve_line_height_relative_lengths(line_height);
        self.margin
            .right
            .resolve_line_height_relative_lengths(line_height);
        self.margin
            .bottom
            .resolve_line_height_relative_lengths(line_height);
        self.margin
            .left
            .resolve_line_height_relative_lengths(line_height);
        self.padding
            .top
            .resolve_line_height_relative_lengths(line_height);
        self.padding
            .right
            .resolve_line_height_relative_lengths(line_height);
        self.padding
            .bottom
            .resolve_line_height_relative_lengths(line_height);
        self.padding
            .left
            .resolve_line_height_relative_lengths(line_height);
        self.width.resolve_line_height_relative_lengths(line_height);
        self.height
            .resolve_line_height_relative_lengths(line_height);
        self.min_width
            .resolve_line_height_relative_lengths(line_height);
        self.max_width
            .resolve_line_height_relative_lengths(line_height);
        self.min_height
            .resolve_line_height_relative_lengths(line_height);
        self.max_height
            .resolve_line_height_relative_lengths(line_height);
        self.inset_left
            .resolve_line_height_relative_lengths(line_height);
        self.inset_top
            .resolve_line_height_relative_lengths(line_height);
        self.inset_right
            .resolve_line_height_relative_lengths(line_height);
        self.inset_bottom
            .resolve_line_height_relative_lengths(line_height);
    }

    /// Resolves root-font metric components against the document-root metric
    /// snapshot shared by every element.
    /// <https://www.w3.org/TR/css-values-4/#font-relative-lengths>
    pub(crate) fn resolve_root_font_metric_lengths(&mut self, basis: RootFontMetricLengthBasis) {
        self.margin.top.resolve_root_font_metric_lengths(basis);
        self.margin.right.resolve_root_font_metric_lengths(basis);
        self.margin.bottom.resolve_root_font_metric_lengths(basis);
        self.margin.left.resolve_root_font_metric_lengths(basis);
        self.padding.top.resolve_root_font_metric_lengths(basis);
        self.padding.right.resolve_root_font_metric_lengths(basis);
        self.padding.bottom.resolve_root_font_metric_lengths(basis);
        self.padding.left.resolve_root_font_metric_lengths(basis);
        self.width.resolve_root_font_metric_lengths(basis);
        self.height.resolve_root_font_metric_lengths(basis);
        self.min_width.resolve_root_font_metric_lengths(basis);
        self.max_width.resolve_root_font_metric_lengths(basis);
        self.min_height.resolve_root_font_metric_lengths(basis);
        self.max_height.resolve_root_font_metric_lengths(basis);
        self.inset_left.resolve_root_font_metric_lengths(basis);
        self.inset_top.resolve_root_font_metric_lengths(basis);
        self.inset_right.resolve_root_font_metric_lengths(basis);
        self.inset_bottom.resolve_root_font_metric_lengths(basis);
    }

    pub(crate) fn requires_ch_advance(&self) -> bool {
        [
            &self.margin.top,
            &self.margin.right,
            &self.margin.bottom,
            &self.margin.left,
            &self.width,
            &self.height,
            &self.min_width,
            &self.max_width,
            &self.min_height,
            &self.max_height,
            &self.inset_left,
            &self.inset_top,
            &self.inset_right,
            &self.inset_bottom,
        ]
        .into_iter()
        .any(|value| value.requires_ch_advance())
            || [
                &self.padding.top,
                &self.padding.right,
                &self.padding.bottom,
                &self.padding.left,
            ]
            .into_iter()
            .any(|value| value.requires_ch_advance())
    }

    /// Whether any box-model value needs a metric from its selected font.
    /// <https://www.w3.org/TR/css-values-4/#font-relative-lengths>
    pub(crate) fn requires_selected_font_metrics(&self) -> bool {
        [
            &self.margin.top,
            &self.margin.right,
            &self.margin.bottom,
            &self.margin.left,
            &self.width,
            self.height.value(),
            &self.min_width,
            &self.max_width,
            &self.min_height,
            &self.max_height,
            &self.inset_left,
            &self.inset_top,
            &self.inset_right,
            &self.inset_bottom,
        ]
        .into_iter()
        .any(|value| value.requires_selected_font_metrics())
            || [
                &self.padding.top,
                &self.padding.right,
                &self.padding.bottom,
                &self.padding.left,
            ]
            .into_iter()
            .any(ComputedLengthPercentage::requires_selected_font_metrics)
    }

    /// Whether any box-model value needs a metric from the document root's
    /// selected font.
    /// <https://www.w3.org/TR/css-values-4/#font-relative-lengths>
    pub(crate) fn requires_root_font_metrics(&self) -> bool {
        [
            &self.margin.top,
            &self.margin.right,
            &self.margin.bottom,
            &self.margin.left,
            &self.width,
            self.height.value(),
            &self.min_width,
            &self.max_width,
            &self.min_height,
            &self.max_height,
            &self.inset_left,
            &self.inset_top,
            &self.inset_right,
            &self.inset_bottom,
        ]
        .into_iter()
        .any(|value| value.requires_root_font_metrics())
            || [
                &self.padding.top,
                &self.padding.right,
                &self.padding.bottom,
                &self.padding.left,
            ]
            .into_iter()
            .any(ComputedLengthPercentage::requires_root_font_metrics)
    }
}

impl ResolveViewportLengths for ComputedFlexBasis {
    fn resolve_viewport_lengths(&mut self, basis: ViewportLengthBasis) {
        match self {
            Self::FitContent(Some(value)) => {
                value.resolve_viewport_lengths(basis);
            }
            Self::LengthPercentage(value) => value.value.resolve_viewport_lengths(basis),
            Self::Auto
            | Self::Content
            | Self::MinContent
            | Self::MaxContent
            | Self::FitContent(None) => {}
        }
    }
}

impl ResolveViewportLengths for ComputedBoxValues {
    fn resolve_viewport_lengths(&mut self, basis: ViewportLengthBasis) {
        self.margin.top.resolve_viewport_lengths(basis);
        self.margin.right.resolve_viewport_lengths(basis);
        self.margin.bottom.resolve_viewport_lengths(basis);
        self.margin.left.resolve_viewport_lengths(basis);
        self.padding.top.resolve_viewport_lengths(basis);
        self.padding.right.resolve_viewport_lengths(basis);
        self.padding.bottom.resolve_viewport_lengths(basis);
        self.padding.left.resolve_viewport_lengths(basis);
        self.width.resolve_viewport_lengths(basis);
        self.height.resolve_viewport_lengths(basis);
        self.min_width.resolve_viewport_lengths(basis);
        self.max_width.resolve_viewport_lengths(basis);
        self.min_height.resolve_viewport_lengths(basis);
        self.max_height.resolve_viewport_lengths(basis);
        self.inset_left.resolve_viewport_lengths(basis);
        self.inset_top.resolve_viewport_lengths(basis);
        self.inset_right.resolve_viewport_lengths(basis);
        self.inset_bottom.resolve_viewport_lengths(basis);
    }
}

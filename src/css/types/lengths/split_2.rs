use super::*;

/// Computed `flex-basis` value.
///
/// CSS Flexbox defines `flex-basis` as `content | <width>`, where `<width>`
/// includes intrinsic sizing keywords, `<length-percentage>`, and `auto`. The
/// `content` keyword is not a generic box-size value: it forces content-based
/// flex base sizing instead of retrieving the main-size property like `auto`:
/// <https://www.w3.org/TR/css-flexbox-1/#flex-basis-property> and
/// <https://www.w3.org/TR/css-sizing-3/#intrinsic-sizes>.
#[derive(Debug, Clone, Copy, PartialEq)]
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
/// indefinite. A zero percentage computes to the same numeric components as a
/// zero length, so flex-basis keeps this authored percentage bit for used-value
/// resolution:
/// <https://www.w3.org/TR/css-flexbox-1/#flex-basis-property>.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct ComputedFlexBasisLength {
    pub value: ComputedLengthPercentage,
    pub has_percentage: bool,
}

impl ComputedFlexBasisLength {
    pub(crate) fn new(value: ComputedLengthPercentage, has_percentage: bool) -> Self {
        Self {
            value,
            has_percentage,
        }
    }
}

impl ComputedFlexBasis {
    pub(crate) const AUTO: Self = Self::Auto;

    pub(crate) fn resolve_font_metric_lengths(&mut self, ch_advance: f32) {
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

    pub(crate) fn resolve_viewport_lengths(
        &mut self,
        viewport_width: f32,
        viewport_height: f32,
        viewport_inline: f32,
        viewport_block: f32,
    ) {
        match self {
            Self::FitContent(Some(value)) => {
                value.resolve_viewport_lengths(
                    viewport_width,
                    viewport_height,
                    viewport_inline,
                    viewport_block,
                );
            }
            Self::LengthPercentage(value) => value.value.resolve_viewport_lengths(
                viewport_width,
                viewport_height,
                viewport_inline,
                viewport_block,
            ),
            Self::Auto
            | Self::Content
            | Self::MinContent
            | Self::MaxContent
            | Self::FitContent(None) => {}
        }
    }
}

/// Four physical CSS edges in top/right/bottom/left order.
///
/// CSS Box Model Level 3 defines physical margin, padding, and border edge
/// properties in this order:
/// <https://www.w3.org/TR/css-box-3/#the-margin-properties>.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct CssEdges<T> {
    pub top: T,
    pub right: T,
    pub bottom: T,
    pub left: T,
}

impl<T: Copy> CssEdges<T> {
    pub(crate) const fn all(value: T) -> Self {
        Self {
            top: value,
            right: value,
            bottom: value,
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
    pub height: ComputedLengthPercentageOrAuto,
    pub min_width: ComputedLengthPercentageOrAuto,
    pub max_width: ComputedLengthPercentageOrAuto,
    pub min_height: ComputedLengthPercentageOrAuto,
    pub max_height: ComputedLengthPercentageOrAuto,
    pub inset_left: ComputedLengthPercentageOrAuto,
    pub inset_top: ComputedLengthPercentageOrAuto,
    pub inset_right: ComputedLengthPercentageOrAuto,
    pub inset_bottom: ComputedLengthPercentageOrAuto,
}

impl ComputedBoxValues {
    pub(crate) const fn initial() -> Self {
        Self {
            margin: CssEdges::all(ComputedLengthPercentageOrAuto::ZERO),
            padding: CssEdges::all(ComputedLengthPercentage::ZERO),
            width: ComputedLengthPercentageOrAuto::AUTO,
            height: ComputedLengthPercentageOrAuto::AUTO,
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

    pub(crate) fn resolve_font_metric_lengths(&mut self, ch_advance: f32) {
        self.margin.top.resolve_font_metric_lengths(ch_advance);
        self.margin.right.resolve_font_metric_lengths(ch_advance);
        self.margin.bottom.resolve_font_metric_lengths(ch_advance);
        self.margin.left.resolve_font_metric_lengths(ch_advance);
        self.padding.top.resolve_font_metric_lengths(ch_advance);
        self.padding.right.resolve_font_metric_lengths(ch_advance);
        self.padding.bottom.resolve_font_metric_lengths(ch_advance);
        self.padding.left.resolve_font_metric_lengths(ch_advance);
        self.width.resolve_font_metric_lengths(ch_advance);
        self.height.resolve_font_metric_lengths(ch_advance);
        self.min_width.resolve_font_metric_lengths(ch_advance);
        self.max_width.resolve_font_metric_lengths(ch_advance);
        self.min_height.resolve_font_metric_lengths(ch_advance);
        self.max_height.resolve_font_metric_lengths(ch_advance);
        self.inset_left.resolve_font_metric_lengths(ch_advance);
        self.inset_top.resolve_font_metric_lengths(ch_advance);
        self.inset_right.resolve_font_metric_lengths(ch_advance);
        self.inset_bottom.resolve_font_metric_lengths(ch_advance);
    }

    pub(crate) fn resolve_viewport_lengths(
        &mut self,
        viewport_width: f32,
        viewport_height: f32,
        viewport_inline: f32,
        viewport_block: f32,
    ) {
        self.margin.top.resolve_viewport_lengths(
            viewport_width,
            viewport_height,
            viewport_inline,
            viewport_block,
        );
        self.margin.right.resolve_viewport_lengths(
            viewport_width,
            viewport_height,
            viewport_inline,
            viewport_block,
        );
        self.margin.bottom.resolve_viewport_lengths(
            viewport_width,
            viewport_height,
            viewport_inline,
            viewport_block,
        );
        self.margin.left.resolve_viewport_lengths(
            viewport_width,
            viewport_height,
            viewport_inline,
            viewport_block,
        );
        self.padding.top.resolve_viewport_lengths(
            viewport_width,
            viewport_height,
            viewport_inline,
            viewport_block,
        );
        self.padding.right.resolve_viewport_lengths(
            viewport_width,
            viewport_height,
            viewport_inline,
            viewport_block,
        );
        self.padding.bottom.resolve_viewport_lengths(
            viewport_width,
            viewport_height,
            viewport_inline,
            viewport_block,
        );
        self.padding.left.resolve_viewport_lengths(
            viewport_width,
            viewport_height,
            viewport_inline,
            viewport_block,
        );
        self.width.resolve_viewport_lengths(
            viewport_width,
            viewport_height,
            viewport_inline,
            viewport_block,
        );
        self.height.resolve_viewport_lengths(
            viewport_width,
            viewport_height,
            viewport_inline,
            viewport_block,
        );
        self.min_width.resolve_viewport_lengths(
            viewport_width,
            viewport_height,
            viewport_inline,
            viewport_block,
        );
        self.max_width.resolve_viewport_lengths(
            viewport_width,
            viewport_height,
            viewport_inline,
            viewport_block,
        );
        self.min_height.resolve_viewport_lengths(
            viewport_width,
            viewport_height,
            viewport_inline,
            viewport_block,
        );
        self.max_height.resolve_viewport_lengths(
            viewport_width,
            viewport_height,
            viewport_inline,
            viewport_block,
        );
        self.inset_left.resolve_viewport_lengths(
            viewport_width,
            viewport_height,
            viewport_inline,
            viewport_block,
        );
        self.inset_top.resolve_viewport_lengths(
            viewport_width,
            viewport_height,
            viewport_inline,
            viewport_block,
        );
        self.inset_right.resolve_viewport_lengths(
            viewport_width,
            viewport_height,
            viewport_inline,
            viewport_block,
        );
        self.inset_bottom.resolve_viewport_lengths(
            viewport_width,
            viewport_height,
            viewport_inline,
            viewport_block,
        );
    }
}

use super::*;

/// Used physical margin or padding edges for a layout formatting context.
///
/// CSS Box Model defines physical box edges and percentage resolution for
/// margin and padding:
/// <https://www.w3.org/TR/CSS22/box.html#margin-properties> and
/// <https://www.w3.org/TR/CSS22/box.html#padding-properties>.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct UsedEdges {
    pub(super) top: f32,
    pub(super) right: f32,
    pub(super) bottom: f32,
    pub(super) left: f32,
}

impl UsedEdges {
    /// Converts used edge lengths back to the renderer's existing edge shape.
    ///
    /// CSS Box Model defines the physical edge order used here:
    /// <https://www.w3.org/TR/css-box-3/#box-model>.
    pub(super) fn to_css_edges(self) -> css::Edges {
        css::Edges {
            top: self.top,
            right: self.right,
            bottom: self.bottom,
            left: self.left,
        }
    }
}

/// Used margin and padding edges for a box in a specific containing block.
///
/// CSS 2.2 resolves margin and padding percentages against the containing
/// block width:
/// <https://www.w3.org/TR/CSS22/box.html#margin-properties> and
/// <https://www.w3.org/TR/CSS22/box.html#padding-properties>.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct UsedBoxEdges {
    pub(super) margin: UsedEdges,
    pub(super) padding: UsedEdges,
}

/// Resolves a computed `<length-percentage>` against a used percentage basis.
///
/// CSS Values and Units Level 4 defines computed `<length-percentage>` values
/// whose percentage component is resolved later against a property-specific
/// basis:
/// <https://www.w3.org/TR/css-values-4/#mixed-percentages>.
pub(super) fn used_length_percentage(
    value: css::ComputedLengthPercentage,
    percentage_basis: f32,
) -> f32 {
    value.length + value.percent * percentage_basis
}

/// Resolves a computed `<length-percentage> | auto` value, preserving `auto`.
///
/// CSS Cascade defines computed values and CSS 2.2 visual formatting defines
/// the later used-value stage where `auto` may be resolved by the formatting
/// context:
/// <https://www.w3.org/TR/css-cascade-5/#computed> and
/// <https://www.w3.org/TR/CSS22/visudet.html>.
pub(super) fn used_length_percentage_or_auto(
    value: css::ComputedLengthPercentageOrAuto,
    percentage_basis: f32,
) -> Option<f32> {
    match value {
        css::ComputedLengthPercentageOrAuto::Auto => None,
        css::ComputedLengthPercentageOrAuto::LengthPercentage(value) => {
            Some(used_length_percentage(value, percentage_basis))
        }
        css::ComputedLengthPercentageOrAuto::MinContent
        | css::ComputedLengthPercentageOrAuto::MaxContent
        | css::ComputedLengthPercentageOrAuto::FitContent(_) => None,
    }
}

/// Resolves a computed `<length-percentage> | auto` value against an optional basis.
///
/// CSS Sizing defines percentages as definite only when the relevant
/// containing block axis is definite. Intrinsic sizing paths pass `None` so
/// unresolved percentages behave like `auto` rather than accidentally using an
/// available-size constraint as a containing block:
/// <https://www.w3.org/TR/css-sizing-3/#definite> and
/// <https://www.w3.org/TR/css-sizing-3/#intrinsic-sizes>.
pub(super) fn used_length_percentage_or_auto_with_optional_basis(
    value: css::ComputedLengthPercentageOrAuto,
    percentage_basis: Option<f32>,
) -> Option<f32> {
    match value {
        css::ComputedLengthPercentageOrAuto::Auto => None,
        css::ComputedLengthPercentageOrAuto::LengthPercentage(value) => {
            if value.percent == 0.0 {
                Some(value.length)
            } else {
                Some(used_length_percentage(value, percentage_basis?))
            }
        }
        css::ComputedLengthPercentageOrAuto::MinContent
        | css::ComputedLengthPercentageOrAuto::MaxContent
        | css::ComputedLengthPercentageOrAuto::FitContent(_) => None,
    }
}

/// Resolves a computed gap for flex layout.
///
/// CSS Box Alignment defines `normal` gaps as zero for flex containers and
/// resolves percentage gaps against the corresponding content box dimension:
/// <https://www.w3.org/TR/css-align-3/#gaps>.
pub(super) fn used_flex_gap(value: css::ComputedGap, percentage_basis: f32) -> f32 {
    match value {
        css::ComputedGap::Normal => 0.0,
        css::ComputedGap::LengthPercentage(value) => {
            used_length_percentage(value, percentage_basis).max(0.0)
        }
    }
}

/// Resolves a computed column gap for multi-column layout.
///
/// CSS Multi-column Layout defines `column-gap: normal` as `1em`; CSS Box
/// Alignment supplies the shared length-percentage gap syntax:
/// <https://www.w3.org/TR/css-multicol-1/#cgap> and
/// <https://www.w3.org/TR/css-align-3/#gaps>.
pub(super) fn used_multicol_column_gap(
    value: css::ComputedGap,
    percentage_basis: f32,
    font_size: f32,
) -> f32 {
    match value {
        css::ComputedGap::Normal => font_size.max(0.0),
        css::ComputedGap::LengthPercentage(value) => {
            used_length_percentage(value, percentage_basis).max(0.0)
        }
    }
}

/// Resolves the number of columns for the current multi-column formatting context.
///
/// CSS Multi-column Layout derives the used column count from `column-count`,
/// `column-width`, the available inline size, and the used column gap:
/// <https://www.w3.org/TR/css-multicol-1/#pseudo-algorithm>.
pub(super) fn used_multicol_column_count(
    style: &ComputedStyle,
    available_width: f32,
    gap: f32,
) -> Option<usize> {
    if let Some(count) = style.column_count.filter(|count| *count > 0) {
        return Some(count);
    }
    let css::ComputedColumnWidth::Length(width) = style.column_width else {
        return None;
    };
    if width <= 0.0 {
        return None;
    }
    let count = ((available_width + gap) / (width + gap)).floor().max(1.0) as usize;
    Some(count)
}

/// Resolves used padding edges for the current containing block.
///
/// CSS 2.2 says padding percentages on all sides refer to the containing
/// block's width:
/// <https://www.w3.org/TR/CSS22/box.html#padding-properties>.
pub(super) fn used_padding_edges(style: &ComputedStyle, inline_basis: f32) -> UsedEdges {
    let padding = style.box_values.padding;
    UsedEdges {
        top: used_padding_edge(padding.top, style.padding.top, inline_basis),
        right: used_padding_edge(padding.right, style.padding.right, inline_basis),
        bottom: used_padding_edge(padding.bottom, style.padding.bottom, inline_basis),
        left: used_padding_edge(padding.left, style.padding.left, inline_basis),
    }
}

/// Resolves one padding edge, using the typed percentage component when present.
///
/// CSS 2.2 padding percentages resolve against the containing block width:
/// <https://www.w3.org/TR/CSS22/box.html#padding-properties>.
fn used_padding_edge(value: css::ComputedLengthPercentage, legacy_length: f32, basis: f32) -> f32 {
    if value.percent != 0.0 {
        used_length_percentage(value, basis).max(0.0)
    } else {
        legacy_length.max(0.0)
    }
}

/// Resolves used margin edges for the current containing block.
///
/// CSS 2.2 says margin percentages on all sides refer to the containing block's
/// width. Auto margins are resolved by the formatting context; this helper
/// returns zero for auto edges when a caller only needs occupied non-auto
/// margin space:
/// <https://www.w3.org/TR/CSS22/box.html#margin-properties>.
pub(super) fn used_margin_edges(style: &ComputedStyle, inline_basis: f32) -> UsedEdges {
    let margin = style.box_values.margin;
    UsedEdges {
        top: used_margin_edge(margin.top, style.margin.top, inline_basis),
        right: used_margin_edge(margin.right, style.margin.right, inline_basis),
        bottom: used_margin_edge(margin.bottom, style.margin.bottom, inline_basis),
        left: used_margin_edge(margin.left, style.margin.left, inline_basis),
    }
}

/// Resolves one margin edge, preserving formatting-context handling for `auto`.
///
/// CSS 2.2 margin percentages resolve against the containing block width:
/// <https://www.w3.org/TR/CSS22/box.html#margin-properties>.
fn used_margin_edge(
    value: css::ComputedLengthPercentageOrAuto,
    legacy_length: f32,
    basis: f32,
) -> f32 {
    match value {
        css::ComputedLengthPercentageOrAuto::Auto => 0.0,
        css::ComputedLengthPercentageOrAuto::LengthPercentage(value) => {
            if value.percent != 0.0 {
                used_length_percentage(value, basis)
            } else {
                legacy_length
            }
        }
        css::ComputedLengthPercentageOrAuto::MinContent
        | css::ComputedLengthPercentageOrAuto::MaxContent
        | css::ComputedLengthPercentageOrAuto::FitContent(_) => legacy_length,
    }
}

/// Resolves both margin and padding edges for a box.
///
/// CSS 2.2 defines the used-value resolution for margin and padding:
/// <https://www.w3.org/TR/CSS22/box.html#margin-properties> and
/// <https://www.w3.org/TR/CSS22/box.html#padding-properties>.
pub(super) fn used_box_edges(style: &ComputedStyle, inline_basis: f32) -> UsedBoxEdges {
    UsedBoxEdges {
        margin: used_margin_edges(style, inline_basis),
        padding: used_padding_edges(style, inline_basis),
    }
}

/// Return the normal-flow block border-box left edge after margin resolution.
///
/// CSS 2.2 block-width resolution treats a fixed-width block with no `auto`
/// horizontal inputs as over-constrained when the equation does not balance.
/// In that case the ignored side depends on the containing block's
/// `direction`: `margin-right` is ignored for LTR and `margin-left` for RTL.
/// Given an already resolved border box width, this helper positions the box
/// from the side that is not ignored:
/// <https://www.w3.org/TR/CSS22/visudet.html#blockwidth>.
pub(super) fn normal_flow_block_outer_x(
    containing_left: f32,
    containing_right: f32,
    style: &ComputedStyle,
    border_box_width: f32,
    containing_direction: Direction,
) -> f32 {
    if has_auto_width(style) {
        return containing_left + style.margin.left;
    }
    match containing_direction {
        Direction::Ltr => containing_left + style.margin.left,
        Direction::Rtl => containing_right - style.margin.right - border_box_width,
    }
}

/// Returns whether `width` is computed as `auto`.
///
/// CSS 2.2 block width calculations depend on whether `width` is `auto`:
/// <https://www.w3.org/TR/CSS22/visudet.html#blockwidth>.
pub(super) fn has_auto_width(style: &ComputedStyle) -> bool {
    style.box_values.width.is_auto()
}

/// Returns whether `height` is computed as `auto`.
///
/// CSS 2.2 block height calculations depend on whether `height` is `auto`:
/// <https://www.w3.org/TR/CSS22/visudet.html#normal-block>.
pub(super) fn has_auto_height(style: &ComputedStyle) -> bool {
    style.box_values.height.is_auto()
}

/// Resolves used content width, falling back to filling available space for `auto`.
///
/// CSS 2.2 defines block-width used-value resolution and CSS Box Sizing defines
/// how `box-sizing` changes the content-box size:
/// <https://www.w3.org/TR/CSS22/visudet.html#blockwidth> and
/// <https://www.w3.org/TR/css-sizing-3/#box-sizing>.
pub(super) fn used_content_width(
    style: &ComputedStyle,
    available_outer_width: f32,
    horizontal_non_content: f32,
) -> f32 {
    used_content_size(
        style.box_values.width,
        style.box_sizing,
        available_outer_width,
        horizontal_non_content,
    )
    .unwrap_or_else(|| (available_outer_width - horizontal_non_content).max(0.0))
}

/// Resolves used content width, returning `None` when the computed width is `auto`.
///
/// CSS 2.2 defines used width and auto width handling:
/// <https://www.w3.org/TR/CSS22/visudet.html#blockwidth>.
pub(super) fn used_content_width_or_auto(
    style: &ComputedStyle,
    available_outer_width: f32,
    horizontal_non_content: f32,
) -> Option<f32> {
    used_content_size(
        style.box_values.width,
        style.box_sizing,
        available_outer_width,
        horizontal_non_content,
    )
}

/// Resolves used content width, returning `None` for `auto` or unresolved percentages.
///
/// CSS Sizing treats pure lengths as definite without needing a percentage
/// basis, while percentage sizes are definite only when their containing block
/// axis is definite:
/// <https://www.w3.org/TR/css-sizing-3/#definite> and
/// <https://www.w3.org/TR/CSS22/visudet.html#blockwidth>.
pub(super) fn used_content_width_or_auto_with_optional_basis(
    style: &ComputedStyle,
    available_outer_width: Option<f32>,
    horizontal_non_content: f32,
) -> Option<f32> {
    used_content_size_with_optional_basis(
        style.box_values.width,
        style.box_sizing,
        available_outer_width,
        horizontal_non_content,
    )
}

/// Resolves used content height, returning `None` when the computed height is `auto`.
///
/// CSS 2.2 defines block height and auto height handling:
/// <https://www.w3.org/TR/CSS22/visudet.html#normal-block>.
pub(super) fn used_content_height_or_auto(
    style: &ComputedStyle,
    available_outer_height: f32,
    vertical_non_content: f32,
) -> Option<f32> {
    used_content_size(
        style.box_values.height,
        style.box_sizing,
        available_outer_height,
        vertical_non_content,
    )
}

/// Resolves used content height, returning `None` for `auto` or unresolved percentages.
///
/// CSS Sizing treats pure lengths as definite without needing a percentage
/// basis, while percentage sizes are definite only when their containing block
/// axis is definite:
/// <https://www.w3.org/TR/css-sizing-3/#definite> and
/// <https://www.w3.org/TR/CSS22/visudet.html#normal-block>.
pub(super) fn used_content_height_or_auto_with_optional_basis(
    style: &ComputedStyle,
    available_outer_height: Option<f32>,
    vertical_non_content: f32,
) -> Option<f32> {
    used_content_size_with_optional_basis(
        style.box_values.height,
        style.box_sizing,
        available_outer_height,
        vertical_non_content,
    )
}

/// Resolves a width/height value to a content-box used size.
///
/// CSS Box Sizing defines conversion between border-box and content-box sizes:
/// <https://www.w3.org/TR/css-sizing-3/#box-sizing>.
fn used_content_size(
    value: css::ComputedLengthPercentageOrAuto,
    box_sizing: BoxSizing,
    percentage_basis: f32,
    non_content: f32,
) -> Option<f32> {
    let specified = used_length_percentage_or_auto(value, percentage_basis)?;
    Some(match box_sizing {
        BoxSizing::BorderBox => (specified - non_content).max(0.0),
        BoxSizing::ContentBox => specified.max(0.0),
    })
}

fn used_content_size_with_optional_basis(
    value: css::ComputedLengthPercentageOrAuto,
    box_sizing: BoxSizing,
    percentage_basis: Option<f32>,
    non_content: f32,
) -> Option<f32> {
    let specified = match value {
        css::ComputedLengthPercentageOrAuto::Auto => return None,
        css::ComputedLengthPercentageOrAuto::LengthPercentage(value) => {
            if value.percent == 0.0 {
                value.length
            } else {
                used_length_percentage(value, percentage_basis?)
            }
        }
        css::ComputedLengthPercentageOrAuto::MinContent
        | css::ComputedLengthPercentageOrAuto::MaxContent
        | css::ComputedLengthPercentageOrAuto::FitContent(_) => return None,
    };
    Some(match box_sizing {
        BoxSizing::BorderBox => (specified - non_content).max(0.0),
        BoxSizing::ContentBox => specified.max(0.0),
    })
}

/// Resolves used `min-width`.
///
/// CSS 2.2 defines min/max width constraints:
/// <https://www.w3.org/TR/CSS22/visudet.html#min-max-widths>.
pub(super) fn used_min_width(style: &ComputedStyle, percentage_basis: f32) -> Option<f32> {
    used_length_percentage_or_auto(style.box_values.min_width, percentage_basis)
        .map(|value| value.max(0.0))
}

/// Resolves used `max-width`.
///
/// CSS 2.2 defines min/max width constraints:
/// <https://www.w3.org/TR/CSS22/visudet.html#min-max-widths>.
pub(super) fn used_max_width(style: &ComputedStyle, percentage_basis: f32) -> Option<f32> {
    used_length_percentage_or_auto(style.box_values.max_width, percentage_basis)
        .map(|value| value.max(0.0))
}

/// Resolves used `min-height`.
///
/// CSS 2.2 defines min/max height constraints:
/// <https://www.w3.org/TR/CSS22/visudet.html#min-max-heights>.
pub(super) fn used_min_height(style: &ComputedStyle, percentage_basis: f32) -> Option<f32> {
    used_length_percentage_or_auto(style.box_values.min_height, percentage_basis)
        .map(|value| value.max(0.0))
}

/// Resolves used `max-height`.
///
/// CSS 2.2 defines min/max height constraints:
/// <https://www.w3.org/TR/CSS22/visudet.html#min-max-heights>.
pub(super) fn used_max_height(style: &ComputedStyle, percentage_basis: f32) -> Option<f32> {
    used_length_percentage_or_auto(style.box_values.max_height, percentage_basis)
        .map(|value| value.max(0.0))
}

/// Applies used min/max width constraints to a content width.
///
/// CSS 2.2 defines min/max width constraint application:
/// <https://www.w3.org/TR/CSS22/visudet.html#min-max-widths>.
pub(super) fn constrain_width(style: &ComputedStyle, value: f32, percentage_basis: f32) -> f32 {
    constrain(
        value,
        used_min_width(style, percentage_basis),
        used_max_width(style, percentage_basis),
    )
}

/// Applies used min/max height constraints to a content height.
///
/// CSS 2.2 defines min/max height constraint application:
/// <https://www.w3.org/TR/CSS22/visudet.html#min-max-heights>.
pub(super) fn constrain_height(style: &ComputedStyle, value: f32, percentage_basis: f32) -> f32 {
    constrain(
        value,
        used_min_height(style, percentage_basis),
        used_max_height(style, percentage_basis),
    )
}

/// Resolves the physical `left` inset for positioned layout.
///
/// CSS 2.2 positioned layout defines offsets and percentage bases:
/// <https://www.w3.org/TR/CSS22/visuren.html#position-props>.
pub(super) fn used_inset_left(
    style: &ComputedStyle,
    containing_block: ContainingBlock,
) -> Option<f32> {
    used_length_percentage_or_auto(style.box_values.inset_left, containing_block.width)
}

/// Resolves the physical `right` inset for positioned layout.
///
/// CSS 2.2 positioned layout defines offsets and percentage bases:
/// <https://www.w3.org/TR/CSS22/visuren.html#position-props>.
pub(super) fn used_inset_right(
    style: &ComputedStyle,
    containing_block: ContainingBlock,
) -> Option<f32> {
    used_length_percentage_or_auto(style.box_values.inset_right, containing_block.width)
}

/// Resolves the physical `top` inset for positioned layout.
///
/// CSS 2.2 positioned layout defines offsets and percentage bases:
/// <https://www.w3.org/TR/CSS22/visuren.html#position-props>.
pub(super) fn used_inset_top(
    style: &ComputedStyle,
    containing_block: ContainingBlock,
) -> Option<f32> {
    used_length_percentage_or_auto(style.box_values.inset_top, containing_block.height)
}

/// Resolves the physical `bottom` inset for positioned layout.
///
/// CSS 2.2 positioned layout defines offsets and percentage bases:
/// <https://www.w3.org/TR/CSS22/visuren.html#position-props>.
pub(super) fn used_inset_bottom(
    style: &ComputedStyle,
    containing_block: ContainingBlock,
) -> Option<f32> {
    used_length_percentage_or_auto(style.box_values.inset_bottom, containing_block.height)
}

/// Replaces computed width with a definite used width for a temporary layout style.
///
/// CSS Cascade separates computed values from used values:
/// <https://www.w3.org/TR/css-cascade-5/#value-stages>.
pub(super) fn set_style_used_width(style: &mut ComputedStyle, width: f32) {
    let width = width.max(0.0);
    style.box_values.width = css::ComputedLengthPercentageOrAuto::LengthPercentage(
        css::ComputedLengthPercentage::from_length(width),
    );
}

/// Replaces computed height with a definite used height for a temporary layout style.
///
/// CSS Cascade separates computed values from used values:
/// <https://www.w3.org/TR/css-cascade-5/#value-stages>.
pub(super) fn set_style_used_height(style: &mut ComputedStyle, height: f32) {
    let height = height.max(0.0);
    style.box_values.height = css::ComputedLengthPercentageOrAuto::LengthPercentage(
        css::ComputedLengthPercentage::from_length(height),
    );
}

/// Freezes temporary width constraints to an already-resolved border-box width.
///
/// CSS Flexbox resolves flex item used sizes before the item is laid out for
/// painting. Replaying the child layout must not apply the item's authored
/// main-axis min/max constraints a second time. The block constraint helpers
/// operate on content-box sizes, so this converts the resolved border-box
/// flex item size back to a content-box constraint:
/// <https://www.w3.org/TR/css-flexbox-1/#layout-algorithm> and
/// <https://www.w3.org/TR/css-sizing-3/#box-sizing>.
pub(super) fn set_style_used_width_bounds(style: &mut ComputedStyle, width: f32) {
    let borders = used_border_widths(style);
    let non_content = borders.left + borders.right + style.padding.left + style.padding.right;
    let content_width = (width - non_content).max(0.0);
    let width = css::ComputedLengthPercentageOrAuto::LengthPercentage(
        css::ComputedLengthPercentage::from_length(content_width),
    );
    style.box_values.min_width = width;
    style.box_values.max_width = width;
}

/// Freezes temporary height constraints to an already-resolved border-box height.
///
/// CSS Flexbox resolves flex item used sizes before the item is laid out for
/// painting. Replaying the child layout must not apply the item's authored
/// main-axis min/max constraints a second time. The block constraint helpers
/// operate on content-box sizes, so this converts the resolved border-box
/// flex item size back to a content-box constraint:
/// <https://www.w3.org/TR/css-flexbox-1/#layout-algorithm> and
/// <https://www.w3.org/TR/css-sizing-3/#box-sizing>.
pub(super) fn set_style_used_height_bounds(style: &mut ComputedStyle, height: f32) {
    let borders = used_border_widths(style);
    let non_content = borders.top + borders.bottom + style.padding.top + style.padding.bottom;
    let content_height = (height - non_content).max(0.0);
    let height = css::ComputedLengthPercentageOrAuto::LengthPercentage(
        css::ComputedLengthPercentage::from_length(content_height),
    );
    style.box_values.min_height = height;
    style.box_values.max_height = height;
}

/// Restores `width: auto` on a temporary layout style.
///
/// CSS 2.2 uses `auto` as the initial width value in normal flow:
/// <https://www.w3.org/TR/CSS22/visudet.html#the-width-property>.
pub(super) fn set_style_auto_width(style: &mut ComputedStyle) {
    style.box_values.width = css::ComputedLengthPercentageOrAuto::Auto;
}

/// Restores `height: auto` on a temporary layout style.
///
/// CSS 2.2 uses `auto` as the initial height value in normal flow:
/// <https://www.w3.org/TR/CSS22/visudet.html#the-height-property>.
pub(super) fn set_style_auto_height(style: &mut ComputedStyle) {
    style.box_values.height = css::ComputedLengthPercentageOrAuto::Auto;
}

/// Clears physical positioned offsets on a temporary layout style.
///
/// CSS 2.2 defines physical inset properties for positioned boxes:
/// <https://www.w3.org/TR/CSS22/visuren.html#position-props>.
pub(super) fn clear_style_insets(style: &mut ComputedStyle) {
    style.box_values.inset_left = css::ComputedLengthPercentageOrAuto::Auto;
    style.box_values.inset_top = css::ComputedLengthPercentageOrAuto::Auto;
    style.box_values.inset_right = css::ComputedLengthPercentageOrAuto::Auto;
    style.box_values.inset_bottom = css::ComputedLengthPercentageOrAuto::Auto;
}

/// Converts a computed CSS `<length-percentage>` into Taffy's equivalent type.
///
/// CSS Flexbox delegates flex item sizing to width/height/flex-basis used
/// values, and Taffy represents the same length/percentage distinction:
/// <https://www.w3.org/TR/css-flexbox-1/#layout-algorithm>.
pub(super) fn taffy_length_percentage(
    value: css::ComputedLengthPercentage,
) -> taffy_layout::LengthPercentage {
    if value.percent != 0.0 && value.length == 0.0 {
        taffy_layout::LengthPercentage::percent(value.percent)
    } else {
        taffy_layout::LengthPercentage::length(value.length.max(0.0))
    }
}

/// Converts a computed CSS `<length-percentage> | auto` into Taffy's type.
///
/// CSS Flexbox uses `auto` for flex basis and margins, and CSS Values defines
/// the length-percentage value shape:
/// <https://www.w3.org/TR/css-flexbox-1/#flex-property> and
/// <https://www.w3.org/TR/css-values-4/#mixed-percentages>.
pub(super) fn taffy_length_percentage_auto(
    value: css::ComputedLengthPercentageOrAuto,
) -> taffy_layout::LengthPercentageAuto {
    match value {
        css::ComputedLengthPercentageOrAuto::Auto => taffy_layout::LengthPercentageAuto::auto(),
        css::ComputedLengthPercentageOrAuto::LengthPercentage(value) => {
            if value.percent != 0.0 && value.length == 0.0 {
                taffy_layout::LengthPercentageAuto::percent(value.percent)
            } else {
                taffy_layout::LengthPercentageAuto::length(value.length.max(0.0))
            }
        }
        css::ComputedLengthPercentageOrAuto::MinContent
        | css::ComputedLengthPercentageOrAuto::MaxContent
        | css::ComputedLengthPercentageOrAuto::FitContent(_) => {
            taffy_layout::LengthPercentageAuto::auto()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn used_lengths_resolve_percentage_against_basis() {
        let value = css::ComputedLengthPercentage {
            length: 12.0,
            percent: 0.25,
            ch: 0.0,
            ..css::ComputedLengthPercentage::ZERO
        };
        assert_eq!(used_length_percentage(value, 200.0), 62.0);
    }

    #[tokio::test]
    async fn mutating_used_width_replaces_typed_percentage_with_used_length() {
        let mut style = ComputedStyle::initial();
        style.box_values.width = css::ComputedLengthPercentageOrAuto::LengthPercentage(
            css::ComputedLengthPercentage::from_percent(0.5),
        );

        set_style_used_width(&mut style, 42.0);

        assert_eq!(
            style.box_values.width,
            css::ComputedLengthPercentageOrAuto::LengthPercentage(
                css::ComputedLengthPercentage::from_length(42.0)
            )
        );
    }

    #[tokio::test]
    async fn multicol_count_can_derive_from_computed_column_width() {
        let mut style = ComputedStyle::initial();
        style.column_width = css::ComputedColumnWidth::Length(40.0);

        assert_eq!(used_multicol_column_count(&style, 150.0, 10.0), Some(3));

        style.column_count = Some(2);
        assert_eq!(used_multicol_column_count(&style, 150.0, 10.0), Some(2));
    }
}

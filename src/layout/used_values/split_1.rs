use super::*;

/// Used physical margin or padding edges for a layout formatting context.
///
/// CSS Box Model defines physical box edges and percentage resolution for
/// margin and padding:
/// <https://www.w3.org/TR/CSS22/box.html#margin-properties> and
/// <https://www.w3.org/TR/CSS22/box.html#padding-properties>.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(in crate::layout) struct UsedEdges {
    pub(in crate::layout) top: LayoutLength,
    pub(in crate::layout) right: LayoutLength,
    pub(in crate::layout) bottom: LayoutLength,
    pub(in crate::layout) left: LayoutLength,
}

impl UsedEdges {
    pub(in crate::layout) fn top(self) -> f32 {
        layout_points(self.top)
    }

    pub(in crate::layout) fn right(self) -> f32 {
        layout_points(self.right)
    }

    pub(in crate::layout) fn bottom(self) -> f32 {
        layout_points(self.bottom)
    }

    pub(in crate::layout) fn left(self) -> f32 {
        layout_points(self.left)
    }

    /// Converts used edge lengths back to the renderer's existing edge shape.
    ///
    /// CSS Box Model defines the physical edge order used here:
    /// <https://www.w3.org/TR/css-box-3/#box-model>.
    pub(in crate::layout) fn to_css_edges(self) -> css::Edges {
        css::Edges {
            top: self.top(),
            right: self.right(),
            bottom: self.bottom(),
            left: self.left(),
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
pub(in crate::layout) struct UsedBoxEdges {
    pub(in crate::layout) margin: UsedEdges,
    pub(in crate::layout) padding: UsedEdges,
}

/// Used physical box metrics after margin and padding percentages are resolved.
///
/// CSS Box Model lays out content, padding, border, and margin as nested
/// physical edges; CSS 2.2 resolves margin and padding percentages against the
/// containing block width before used geometry is computed:
/// <https://www.w3.org/TR/css-box-3/#box-model> and
/// <https://www.w3.org/TR/CSS22/box.html#box-dimensions>.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(in crate::layout) struct UsedBoxMetrics {
    pub(in crate::layout) margin: css::Edges,
    pub(in crate::layout) padding: css::Edges,
    pub(in crate::layout) border: css::Edges,
}

impl UsedBoxMetrics {
    pub(in crate::layout) fn horizontal_non_content(self) -> f32 {
        self.border.left + self.border.right + self.padding.left + self.padding.right
    }

    pub(in crate::layout) fn vertical_non_content(self) -> f32 {
        self.border.top + self.border.bottom + self.padding.top + self.padding.bottom
    }
}

/// Resolves a computed `<length-percentage>` against a used percentage basis.
///
/// CSS Values and Units Level 4 defines computed `<length-percentage>` values
/// whose percentage component is resolved later against a property-specific
/// basis:
/// <https://www.w3.org/TR/css-values-4/#mixed-percentages>.
pub(in crate::layout) fn used_length_percentage(
    value: css::ComputedLengthPercentage,
    percentage_basis: f32,
) -> f32 {
    value
        .used_length_with_percentage_basis(percentage_basis)
        .unwrap_or(value.length_with_percentage_basis(percentage_basis))
}

/// Resolves a computed `<length-percentage> | auto` value, preserving `auto`.
///
/// CSS Cascade defines computed values and CSS 2.2 visual formatting defines
/// the later used-value stage where `auto` may be resolved by the formatting
/// context:
/// <https://www.w3.org/TR/css-cascade-5/#computed> and
/// <https://www.w3.org/TR/CSS22/visudet.html>.
pub(in crate::layout) fn used_length_percentage_or_auto(
    value: css::ComputedLengthPercentageOrAuto,
    percentage_basis: f32,
) -> Option<f32> {
    match value {
        css::ComputedLengthPercentageOrAuto::Auto
        | css::ComputedLengthPercentageOrAuto::Stretch => None,
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
pub(in crate::layout) fn used_length_percentage_or_auto_with_optional_basis(
    value: css::ComputedLengthPercentageOrAuto,
    percentage_basis: Option<f32>,
) -> Option<f32> {
    match value {
        css::ComputedLengthPercentageOrAuto::Auto
        | css::ComputedLengthPercentageOrAuto::Stretch => None,
        css::ComputedLengthPercentageOrAuto::LengthPercentage(value) => {
            if value.percent == 0.0 && !value.has_percentage && value.math.is_none() {
                Some(value.length_points())
            } else {
                Some(used_length_percentage(value, percentage_basis?))
            }
        }
        css::ComputedLengthPercentageOrAuto::MinContent
        | css::ComputedLengthPercentageOrAuto::MaxContent
        | css::ComputedLengthPercentageOrAuto::FitContent(_) => None,
    }
}

/// Resolves CSS Sizing Level 4 stretch-fit sizing to a content-box size.
///
/// `stretch` attempts to make the margin box fill the available space. Layout
/// callers pass the already-resolved available margin-box size, used margins,
/// and padding plus border for the relevant axis, and the content box is
/// floored at zero:
/// <https://drafts.csswg.org/css-sizing-4/#stretch-fit-sizing>.
/// Resolves CSS Sizing Level 4 stretch-fit sizing to a content-box length.
///
/// `stretch` attempts to make the margin box fill the available space. The
/// result is a CSS content-box length in Quire's PDF-point layout scalar, with
/// padding and border represented as an explicit semantic non-content length:
/// <https://drafts.csswg.org/css-sizing-4/#stretch-fit-sizing> and
/// <https://www.w3.org/TR/css-sizing-3/#box-sizing>.
pub(in crate::layout) fn stretch_fit_content_box_size(
    available_margin_box_size: f32,
    margin_size: f32,
    non_content_size: NonContentLength,
) -> ContentBoxLength {
    content_box_pt((available_margin_box_size - margin_size - non_content_size.points()).max(0.0))
}

/// Resolves a computed gap for flex layout.
///
/// CSS Box Alignment defines `normal` gaps as zero for flex containers and
/// resolves percentage gaps against the corresponding content box dimension:
/// <https://www.w3.org/TR/css-align-3/#gaps>.
pub(in crate::layout) fn used_flex_gap(value: css::ComputedGap, percentage_basis: f32) -> f32 {
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
pub(in crate::layout) fn used_multicol_column_gap(
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
pub(in crate::layout) fn used_multicol_column_count(
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
    let width = width.length_if_no_percent()?;
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
pub(in crate::layout) fn used_padding_edges(style: &ComputedStyle, inline_basis: f32) -> UsedEdges {
    let padding = style.box_values.padding;
    UsedEdges {
        top: layout_pt(used_padding_edge(
            padding.top,
            style.padding.top,
            inline_basis,
        )),
        right: layout_pt(used_padding_edge(
            padding.right,
            style.padding.right,
            inline_basis,
        )),
        bottom: layout_pt(used_padding_edge(
            padding.bottom,
            style.padding.bottom,
            inline_basis,
        )),
        left: layout_pt(used_padding_edge(
            padding.left,
            style.padding.left,
            inline_basis,
        )),
    }
}

/// Resolves one padding edge, using the typed percentage component when present.
///
/// CSS 2.2 padding percentages resolve against the containing block width:
/// <https://www.w3.org/TR/CSS22/box.html#padding-properties>.
pub(in crate::layout) fn used_padding_edge(
    value: css::ComputedLengthPercentage,
    legacy_length: f32,
    basis: f32,
) -> f32 {
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
pub(in crate::layout) fn used_margin_edges(style: &ComputedStyle, inline_basis: f32) -> UsedEdges {
    let margin = style.box_values.margin;
    UsedEdges {
        top: layout_pt(used_margin_edge(margin.top, style.margin.top, inline_basis)),
        right: layout_pt(used_margin_edge(
            margin.right,
            style.margin.right,
            inline_basis,
        )),
        bottom: layout_pt(used_margin_edge(
            margin.bottom,
            style.margin.bottom,
            inline_basis,
        )),
        left: layout_pt(used_margin_edge(
            margin.left,
            style.margin.left,
            inline_basis,
        )),
    }
}

/// Resolves one margin edge, preserving formatting-context handling for `auto`.
///
/// CSS 2.2 margin percentages resolve against the containing block width:
/// <https://www.w3.org/TR/CSS22/box.html#margin-properties>.
pub(in crate::layout) fn used_margin_edge(
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
        | css::ComputedLengthPercentageOrAuto::FitContent(_)
        | css::ComputedLengthPercentageOrAuto::Stretch => legacy_length,
    }
}

/// Resolves both margin and padding edges for a box.
///
/// CSS 2.2 defines the used-value resolution for margin and padding:
/// <https://www.w3.org/TR/CSS22/box.html#margin-properties> and
/// <https://www.w3.org/TR/CSS22/box.html#padding-properties>.
pub(in crate::layout) fn used_box_edges(style: &ComputedStyle, inline_basis: f32) -> UsedBoxEdges {
    UsedBoxEdges {
        margin: used_margin_edges(style, inline_basis),
        padding: used_padding_edges(style, inline_basis),
    }
}

/// Return used box metrics without mutating the caller's style.
///
/// This is useful for intrinsic sizing paths that need resolved non-content
/// edges but must not overwrite the computed style carried by a formatting box.
/// CSS separates computed and used values:
/// <https://www.w3.org/TR/css-cascade-5/#value-stages>.
pub(in crate::layout) fn used_box_metrics(
    style: &ComputedStyle,
    inline_basis: f32,
) -> UsedBoxMetrics {
    let used_edges = used_box_edges(style, inline_basis);
    UsedBoxMetrics {
        margin: used_edges.margin.to_css_edges(),
        padding: used_edges.padding.to_css_edges(),
        border: used_border_widths(style),
    }
}

/// Resolve a temporary layout style's margin and padding and return box metrics.
///
/// Layout code often needs both the mutated style, so later code can consume
/// used edge values, and the derived non-content sizes. Keeping those steps
/// together avoids call sites accidentally mixing computed and used edges:
/// <https://www.w3.org/TR/css-cascade-5/#used>.
pub(in crate::layout) fn apply_used_box_metrics(
    style: &mut ComputedStyle,
    inline_basis: f32,
) -> UsedBoxMetrics {
    let metrics = used_box_metrics(style, inline_basis);
    style.margin = metrics.margin;
    style.padding = metrics.padding;
    metrics
}

/// Resolved horizontal geometry for a normal-flow block-level box.
///
/// CSS 2.2 defines one equation for the used inline margins, borders, padding,
/// and content width of block-level non-replaced boxes in normal flow. Keeping
/// the result together avoids callers mixing a content width resolved against
/// one basis with a border box positioned from another:
/// <https://www.w3.org/TR/CSS22/visudet.html#blockwidth>.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(in crate::layout) struct NormalFlowBlockWidth {
    pub(in crate::layout) content_width: ContentBoxLength,
    pub(in crate::layout) border_box_width: BorderBoxLength,
    pub(in crate::layout) border_box_x: f32,
}

/// Width inputs for resolving a block container's requested content size.
///
/// `available_outer_width` is the margin-adjusted stretch-fit size used by
/// `auto` and intrinsic sizing keywords. `percentage_basis` is the containing
/// block inline size used by length-percentage properties:
/// <https://www.w3.org/TR/CSS22/visudet.html#blockwidth>.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(in crate::layout) struct BlockContentWidthInputs {
    pub(in crate::layout) available_outer_width: f32,
    pub(in crate::layout) percentage_basis: f32,
    pub(in crate::layout) horizontal_non_content: NonContentLength,
}

/// Return the outer available inline size used by `width:auto` block boxes.
///
/// CSS 2.2 resolves margin percentages against the containing block width, but
/// `width:auto` itself follows the block-width equation after non-auto margins
/// have their used values. Negative margins therefore increase this available
/// space rather than being clamped away:
/// <https://www.w3.org/TR/CSS22/visudet.html#blockwidth> and
/// <https://www.w3.org/TR/CSS22/box.html#margin-properties>.
pub(in crate::layout) fn normal_flow_block_available_outer_width(
    style: &ComputedStyle,
    containing_inline_size: f32,
) -> f32 {
    containing_inline_size - style.margin.left - style.margin.right
}

/// Resolve content width, border-box width, and border-box x for a normal block.
///
/// Percentages in `width`, `min-width`, and `max-width` use the containing
/// block width as their percentage basis. Only `width:auto` consumes the
/// margin-adjusted space from the CSS 2.2 block-width equation:
/// <https://www.w3.org/TR/CSS22/visudet.html#blockwidth>.
pub(in crate::layout) fn resolve_normal_flow_block_width(
    style: &mut ComputedStyle,
    containing_left: f32,
    containing_right: f32,
    requested_content_width: ContentBoxLength,
    horizontal_non_content: NonContentLength,
    containing_direction: Direction,
    resolve_auto_margins: bool,
) -> NormalFlowBlockWidth {
    let containing_inline_size = (containing_right - containing_left).max(0.0);
    let content_width = content_box_pt(constrain_width(
        style,
        requested_content_width.points(),
        containing_inline_size,
    ));
    let border_box_width = content_box_to_border_box_length(content_width, horizontal_non_content);
    if resolve_auto_margins {
        resolve_normal_flow_block_auto_margins(
            style,
            containing_inline_size,
            border_box_width.points(),
            containing_direction,
        );
    }
    let border_box_x = normal_flow_block_outer_x(
        containing_left,
        containing_right,
        style,
        border_box_width.points(),
        containing_direction,
    );

    NormalFlowBlockWidth {
        content_width,
        border_box_width,
        border_box_x,
    }
}

/// Resolve the requested content width for a normal-flow block-level box.
///
/// Lengths and percentages use the containing block as their percentage basis,
/// while `auto` fills the margin-adjusted available space. This preserves
/// negative margins as required by the CSS 2.2 block-width equation:
/// <https://www.w3.org/TR/CSS22/visudet.html#blockwidth>.
pub(in crate::layout) fn used_normal_flow_block_content_box_width(
    style: &ComputedStyle,
    containing_inline_size: f32,
    horizontal_non_content: NonContentLength,
) -> ContentBoxLength {
    used_content_box_size(
        style.box_values.width,
        style.box_sizing,
        containing_inline_size,
        horizontal_non_content,
    )
    .unwrap_or_else(|| {
        content_box_pt(
            (normal_flow_block_available_outer_width(style, containing_inline_size)
                - horizontal_non_content.points())
            .max(0.0),
        )
    })
}

/// Resolve horizontal `auto` margins for a normal-flow block with a used width.
///
/// CSS 2.2 defines the block width equation over horizontal margins, borders,
/// padding, and width. Once the used border-box width is known, auto horizontal
/// margins absorb remaining inline space; when no horizontal margin is auto,
/// the over-constrained side is handled during positioning:
/// <https://www.w3.org/TR/CSS22/visudet.html#blockwidth>.
pub(in crate::layout) fn resolve_normal_flow_block_auto_margins(
    style: &mut ComputedStyle,
    containing_inline_size: f32,
    border_box_width: f32,
    containing_direction: Direction,
) {
    let left_auto = style.box_values.margin.left.is_auto();
    let right_auto = style.box_values.margin.right.is_auto();
    if has_auto_width(style) || (!left_auto && !right_auto) {
        return;
    }

    resolve_normal_flow_auto_margins_for_known_width(
        style,
        containing_inline_size,
        border_box_width,
        containing_direction,
    );
}

/// Resolve horizontal `auto` margins when the formatting context has already
/// resolved a concrete border-box width.
///
/// CSS table wrappers with `width:auto` can shrink-wrap to their final grid
/// width, so they need the same CSS 2.2 block-width auto-margin equation after
/// table width resolution rather than normal block auto-width fill. When the
/// equation is over-constrained, CSS first treats any auto horizontal margins
/// as zero and then ignores the containing block's end-side margin:
/// <https://www.w3.org/TR/CSS22/visudet.html#blockwidth> and
/// <https://drafts.csswg.org/css-tables-3/#computing-the-table-width>.
pub(in crate::layout) fn resolve_normal_flow_auto_margins_for_known_width(
    style: &mut ComputedStyle,
    containing_inline_size: f32,
    border_box_width: f32,
    containing_direction: Direction,
) {
    let left_auto = style.box_values.margin.left.is_auto();
    let right_auto = style.box_values.margin.right.is_auto();
    if !left_auto && !right_auto {
        return;
    }

    let free_space =
        containing_inline_size - style.margin.left - border_box_width - style.margin.right;
    if free_space < 0.0 {
        if left_auto {
            style.margin.left = 0.0;
        }
        if right_auto {
            style.margin.right = 0.0;
        }
        match containing_direction {
            Direction::Ltr => {
                style.margin.right = containing_inline_size - style.margin.left - border_box_width;
            }
            Direction::Rtl => {
                style.margin.left = containing_inline_size - border_box_width - style.margin.right;
            }
        }
    } else if left_auto && right_auto {
        style.margin.left = free_space / 2.0;
        style.margin.right = free_space / 2.0;
    } else if left_auto {
        style.margin.left = free_space;
    } else if right_auto {
        style.margin.right = free_space;
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
pub(in crate::layout) fn normal_flow_block_outer_x(
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
pub(in crate::layout) fn has_auto_width(style: &ComputedStyle) -> bool {
    style.box_values.width.is_auto()
}

/// Returns whether `height` is computed as `auto`.
///
/// CSS 2.2 block height calculations depend on whether `height` is `auto`:
/// <https://www.w3.org/TR/CSS22/visudet.html#normal-block>.
pub(in crate::layout) fn has_auto_height(style: &ComputedStyle) -> bool {
    style.box_values.height.is_auto()
}

/// Resolves used content width, falling back to filling available space for `auto`.
///
/// CSS 2.2 defines block-width used-value resolution and CSS Box Sizing defines
/// how `box-sizing` changes the content-box size:
/// <https://www.w3.org/TR/CSS22/visudet.html#blockwidth> and
/// <https://www.w3.org/TR/css-sizing-3/#box-sizing>.
pub(in crate::layout) fn used_content_width(
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
///
/// Transitional raw compatibility wrapper. Prefer `used_content_box_size` when
/// the caller can keep content-box sizes typed until a layout/paint boundary.
pub(in crate::layout) fn used_content_width_or_auto(
    style: &ComputedStyle,
    available_outer_width: f32,
    horizontal_non_content: f32,
) -> Option<f32> {
    used_content_box_size(
        style.box_values.width,
        style.box_sizing,
        available_outer_width,
        non_content_pt(horizontal_non_content),
    )
    .map(SemanticLengthExt::points)
}

/// Resolves used content width, returning `None` for `auto` or unresolved percentages.
///
/// CSS Sizing treats pure lengths as definite without needing a percentage
/// basis, while percentage sizes are definite only when their containing block
/// axis is definite:
/// <https://www.w3.org/TR/css-sizing-3/#definite> and
/// <https://www.w3.org/TR/CSS22/visudet.html#blockwidth>.
///
/// Transitional raw compatibility wrapper. Prefer
/// `used_content_box_size_with_optional_basis` when the caller can keep
/// content-box sizes typed until a layout/paint boundary.
pub(in crate::layout) fn used_content_width_or_auto_with_optional_basis(
    style: &ComputedStyle,
    available_outer_width: Option<f32>,
    horizontal_non_content: f32,
) -> Option<f32> {
    used_content_box_size_with_optional_basis(
        style.box_values.width,
        style.box_sizing,
        available_outer_width,
        non_content_pt(horizontal_non_content),
    )
    .map(SemanticLengthExt::points)
}

/// Resolves used content height, returning `None` when the computed height is `auto`.
///
/// CSS 2.2 defines block height and auto height handling:
/// <https://www.w3.org/TR/CSS22/visudet.html#normal-block>.
///
/// Transitional raw compatibility wrapper. Prefer `used_content_box_size` when
/// the caller can keep content-box sizes typed until a layout/paint boundary.
pub(in crate::layout) fn used_content_height_or_auto(
    style: &ComputedStyle,
    available_outer_height: f32,
    vertical_non_content: f32,
) -> Option<f32> {
    used_content_box_size(
        style.box_values.height,
        style.box_sizing,
        available_outer_height,
        non_content_pt(vertical_non_content),
    )
    .map(SemanticLengthExt::points)
}

/// Resolves used content height, returning `None` for `auto` or unresolved percentages.
///
/// CSS Sizing treats pure lengths as definite without needing a percentage
/// basis, while percentage sizes are definite only when their containing block
/// axis is definite:
/// <https://www.w3.org/TR/css-sizing-3/#definite> and
/// <https://www.w3.org/TR/CSS22/visudet.html#normal-block>.
///
/// Transitional raw compatibility wrapper. Prefer
/// `used_content_box_size_with_optional_basis` when the caller can keep
/// content-box sizes typed until a layout/paint boundary.
pub(in crate::layout) fn used_content_height_or_auto_with_optional_basis(
    style: &ComputedStyle,
    available_outer_height: Option<f32>,
    vertical_non_content: f32,
) -> Option<f32> {
    used_content_box_size_with_optional_basis(
        style.box_values.height,
        style.box_sizing,
        available_outer_height,
        non_content_pt(vertical_non_content),
    )
    .map(SemanticLengthExt::points)
}

/// Resolves a width/height value to a typed content-box used size.
///
/// CSS Box Sizing defines conversion between border-box and content-box sizes:
/// <https://www.w3.org/TR/css-sizing-3/#box-sizing>.
///
/// The returned value is a CSS content-box length in Quire's PDF-point layout
/// scalar. Callers should keep this typed until they cross a layout/paint or
/// external adapter boundary.
pub(in crate::layout) fn used_content_box_size(
    value: css::ComputedLengthPercentageOrAuto,
    box_sizing: BoxSizing,
    percentage_basis: f32,
    non_content: NonContentLength,
) -> Option<ContentBoxLength> {
    if value == css::ComputedLengthPercentageOrAuto::Stretch {
        return Some(stretch_fit_content_box_size(
            percentage_basis,
            0.0,
            non_content,
        ));
    }
    let specified = used_length_percentage_or_auto(value, percentage_basis)?;
    Some(used_content_box_size_from_specified(
        specified,
        box_sizing,
        non_content,
    ))
}

/// Resolves a width/height value to a typed content-box used size.
///
/// CSS Sizing treats pure lengths as definite without a percentage basis, while
/// percentage sizes are definite only when the containing block axis is
/// definite. CSS Box Sizing then maps the specified content-box or border-box
/// value into the content-box coordinate space:
/// <https://www.w3.org/TR/css-sizing-3/#definite> and
/// <https://www.w3.org/TR/css-sizing-3/#box-sizing>.
pub(in crate::layout) fn used_content_box_size_with_optional_basis(
    value: css::ComputedLengthPercentageOrAuto,
    box_sizing: BoxSizing,
    percentage_basis: Option<f32>,
    non_content: NonContentLength,
) -> Option<ContentBoxLength> {
    let specified = match value {
        css::ComputedLengthPercentageOrAuto::Auto => return None,
        css::ComputedLengthPercentageOrAuto::Stretch => {
            return percentage_basis
                .map(|basis| stretch_fit_content_box_size(basis, 0.0, non_content));
        }
        css::ComputedLengthPercentageOrAuto::LengthPercentage(value) => {
            if value.percent == 0.0 && !value.has_percentage {
                value.length_points()
            } else {
                used_length_percentage(value, percentage_basis?)
            }
        }
        css::ComputedLengthPercentageOrAuto::MinContent
        | css::ComputedLengthPercentageOrAuto::MaxContent
        | css::ComputedLengthPercentageOrAuto::FitContent(_) => return None,
    };
    Some(used_content_box_size_from_specified(
        specified,
        box_sizing,
        non_content,
    ))
}

fn used_content_box_size_from_specified(
    specified: f32,
    box_sizing: BoxSizing,
    non_content: NonContentLength,
) -> ContentBoxLength {
    match box_sizing {
        BoxSizing::BorderBox => {
            border_box_to_content_box_length(border_box_pt(specified), non_content)
        }
        BoxSizing::ContentBox => content_box_pt(specified.max(0.0)),
    }
}

/// Resolves a width/height value to a content-box used size.
///
/// CSS Box Sizing defines conversion between border-box and content-box sizes:
/// <https://www.w3.org/TR/css-sizing-3/#box-sizing>.
///
/// Transitional raw compatibility wrapper. Prefer `used_content_box_size` when
/// the caller can keep content-box sizes typed until a layout/paint boundary.
pub(in crate::layout) fn used_content_size(
    value: css::ComputedLengthPercentageOrAuto,
    box_sizing: BoxSizing,
    percentage_basis: f32,
    non_content: f32,
) -> Option<f32> {
    used_content_box_size(
        value,
        box_sizing,
        percentage_basis,
        non_content_pt(non_content),
    )
    .map(SemanticLengthExt::points)
}

/// Resolves used `min-width`.
///
/// CSS 2.2 defines min/max width constraints:
/// <https://www.w3.org/TR/CSS22/visudet.html#min-max-widths>.
pub(in crate::layout) fn used_min_width(
    style: &ComputedStyle,
    percentage_basis: f32,
) -> Option<f32> {
    used_length_percentage_or_auto(style.box_values.min_width, percentage_basis)
        .map(|value| value.max(0.0))
}

/// Resolves used `max-width`.
///
/// CSS 2.2 defines min/max width constraints:
/// <https://www.w3.org/TR/CSS22/visudet.html#min-max-widths>.
pub(in crate::layout) fn used_max_width(
    style: &ComputedStyle,
    percentage_basis: f32,
) -> Option<f32> {
    used_length_percentage_or_auto(style.box_values.max_width, percentage_basis)
        .map(|value| value.max(0.0))
}

/// Resolves used `min-height`.
///
/// CSS 2.2 defines min/max height constraints:
/// <https://www.w3.org/TR/CSS22/visudet.html#min-max-heights>.
pub(in crate::layout) fn used_min_height(
    style: &ComputedStyle,
    percentage_basis: f32,
) -> Option<f32> {
    used_length_percentage_or_auto(style.box_values.min_height, percentage_basis)
        .map(|value| value.max(0.0))
}

/// Resolves used `max-height`.
///
/// CSS 2.2 defines min/max height constraints:
/// <https://www.w3.org/TR/CSS22/visudet.html#min-max-heights>.
pub(in crate::layout) fn used_max_height(
    style: &ComputedStyle,
    percentage_basis: f32,
) -> Option<f32> {
    used_length_percentage_or_auto(style.box_values.max_height, percentage_basis)
        .map(|value| value.max(0.0))
}

/// Applies used min/max width constraints to a content width.
///
/// CSS 2.2 defines min/max width constraint application:
/// <https://www.w3.org/TR/CSS22/visudet.html#min-max-widths>.
pub(in crate::layout) fn constrain_width(
    style: &ComputedStyle,
    value: f32,
    percentage_basis: f32,
) -> f32 {
    constrain(
        value,
        used_min_width(style, percentage_basis),
        used_max_width(style, percentage_basis),
    )
}

/// Apply non-replaced intrinsic inline-size constraints to content widths.
///
/// CSS Sizing defines intrinsic contributions for non-replaced boxes by
/// treating cyclic percentage `width` and `max-width` values as their initial
/// values while resolving `min-width` percentages against zero:
/// <https://www.w3.org/TR/css-sizing-3/#intrinsic-contribution>.
pub(in crate::layout) fn constrain_non_replaced_intrinsic_widths(
    style: &ComputedStyle,
    min_content: f32,
    max_content: f32,
    horizontal_non_content: f32,
) -> (f32, f32) {
    let horizontal_non_content = non_content_pt(horizontal_non_content);
    let specified_width = used_content_box_size_with_optional_basis(
        style.box_values.width,
        style.box_sizing,
        None,
        horizontal_non_content,
    )
    .map(SemanticLengthExt::points);
    let min_constraint = intrinsic_min_width_constraint(style);
    let max_constraint = intrinsic_max_width_constraint(style);
    let min_content = specified_width.unwrap_or(min_content.max(0.0));
    let max_content = specified_width.unwrap_or(max_content.max(min_content).max(0.0));
    (
        constrain(min_content, min_constraint, max_constraint).max(0.0),
        constrain(max_content, min_constraint, max_constraint).max(0.0),
    )
}

fn intrinsic_min_width_constraint(style: &ComputedStyle) -> Option<f32> {
    match style.box_values.min_width {
        css::ComputedLengthPercentageOrAuto::LengthPercentage(value) => {
            Some(used_length_percentage(value, 0.0).max(0.0))
        }
        css::ComputedLengthPercentageOrAuto::Auto
        | css::ComputedLengthPercentageOrAuto::MinContent
        | css::ComputedLengthPercentageOrAuto::MaxContent
        | css::ComputedLengthPercentageOrAuto::FitContent(_)
        | css::ComputedLengthPercentageOrAuto::Stretch => None,
    }
}

fn intrinsic_max_width_constraint(style: &ComputedStyle) -> Option<f32> {
    match style.box_values.max_width {
        css::ComputedLengthPercentageOrAuto::LengthPercentage(value) if !value.has_percentage => {
            Some(used_length_percentage(value, 0.0).max(0.0))
        }
        css::ComputedLengthPercentageOrAuto::LengthPercentage(_)
        | css::ComputedLengthPercentageOrAuto::Auto
        | css::ComputedLengthPercentageOrAuto::MinContent
        | css::ComputedLengthPercentageOrAuto::MaxContent
        | css::ComputedLengthPercentageOrAuto::FitContent(_)
        | css::ComputedLengthPercentageOrAuto::Stretch => None,
    }
}

/// Applies used min/max height constraints to a content height.
///
/// CSS 2.2 defines min/max height constraint application:
/// <https://www.w3.org/TR/CSS22/visudet.html#min-max-heights>.
pub(in crate::layout) fn constrain_height(
    style: &ComputedStyle,
    value: f32,
    percentage_basis: f32,
) -> f32 {
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
pub(in crate::layout) fn used_inset_left(
    style: &ComputedStyle,
    containing_block: ContainingBlock,
) -> Option<f32> {
    used_length_percentage_or_auto(style.box_values.inset_left, containing_block.width())
}

/// Resolves the physical `right` inset for positioned layout.
///
/// CSS 2.2 positioned layout defines offsets and percentage bases:
/// <https://www.w3.org/TR/CSS22/visuren.html#position-props>.
pub(in crate::layout) fn used_inset_right(
    style: &ComputedStyle,
    containing_block: ContainingBlock,
) -> Option<f32> {
    used_length_percentage_or_auto(style.box_values.inset_right, containing_block.width())
}

/// Resolves the physical `top` inset for positioned layout.
///
/// CSS 2.2 positioned layout defines offsets and percentage bases:
/// <https://www.w3.org/TR/CSS22/visuren.html#position-props>.
pub(in crate::layout) fn used_inset_top(
    style: &ComputedStyle,
    containing_block: ContainingBlock,
) -> Option<f32> {
    used_length_percentage_or_auto(style.box_values.inset_top, containing_block.height())
}

/// Resolves the physical `bottom` inset for positioned layout.
///
/// CSS 2.2 positioned layout defines offsets and percentage bases:
/// <https://www.w3.org/TR/CSS22/visuren.html#position-props>.
pub(in crate::layout) fn used_inset_bottom(
    style: &ComputedStyle,
    containing_block: ContainingBlock,
) -> Option<f32> {
    used_length_percentage_or_auto(style.box_values.inset_bottom, containing_block.height())
}

/// Replaces computed width with a definite used width for a temporary layout style.
///
/// CSS Cascade separates computed values from used values:
/// <https://www.w3.org/TR/css-cascade-5/#value-stages>.
pub(in crate::layout) fn set_style_used_width(style: &mut ComputedStyle, width: f32) {
    let width = width.max(0.0);
    style.box_values.width = css::ComputedLengthPercentageOrAuto::LengthPercentage(
        css::ComputedLengthPercentage::from_points(width),
    );
}

/// Replaces computed height with a definite used height for a temporary layout style.
///
/// CSS Cascade separates computed values from used values:
/// <https://www.w3.org/TR/css-cascade-5/#value-stages>.
pub(in crate::layout) fn set_style_used_height(style: &mut ComputedStyle, height: f32) {
    let height = height.max(0.0);
    style.box_values.height = css::ComputedLengthPercentageOrAuto::LengthPercentage(
        css::ComputedLengthPercentage::from_points(height),
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
pub(in crate::layout) fn set_style_used_width_bounds(style: &mut ComputedStyle, width: f32) {
    let borders = used_border_widths(style);
    let non_content = borders.left + borders.right + style.padding.left + style.padding.right;
    let content_width = (width - non_content).max(0.0);
    let width = css::ComputedLengthPercentageOrAuto::LengthPercentage(
        css::ComputedLengthPercentage::from_points(content_width),
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
pub(in crate::layout) fn set_style_used_height_bounds(style: &mut ComputedStyle, height: f32) {
    let borders = used_border_widths(style);
    let non_content = borders.top + borders.bottom + style.padding.top + style.padding.bottom;
    let content_height = (height - non_content).max(0.0);
    let height = css::ComputedLengthPercentageOrAuto::LengthPercentage(
        css::ComputedLengthPercentage::from_points(content_height),
    );
    style.box_values.min_height = height;
    style.box_values.max_height = height;
}

/// Restores `width: auto` on a temporary layout style.
///
/// CSS 2.2 uses `auto` as the initial width value in normal flow:
/// <https://www.w3.org/TR/CSS22/visudet.html#the-width-property>.
pub(in crate::layout) fn set_style_auto_width(style: &mut ComputedStyle) {
    style.box_values.width = css::ComputedLengthPercentageOrAuto::Auto;
}

/// Restores `height: auto` on a temporary layout style.
///
/// CSS 2.2 uses `auto` as the initial height value in normal flow:
/// <https://www.w3.org/TR/CSS22/visudet.html#the-height-property>.
pub(in crate::layout) fn set_style_auto_height(style: &mut ComputedStyle) {
    style.box_values.height = css::ComputedLengthPercentageOrAuto::Auto;
}

/// Clears physical positioned offsets on a temporary layout style.
///
/// CSS 2.2 defines physical inset properties for positioned boxes:
/// <https://www.w3.org/TR/CSS22/visuren.html#position-props>.
pub(in crate::layout) fn clear_style_insets(style: &mut ComputedStyle) {
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
pub(in crate::layout) fn taffy_length_percentage(
    value: css::ComputedLengthPercentage,
) -> taffy_layout::LengthPercentage {
    if value.percent != 0.0 && value.length_is_zero() {
        taffy_layout::LengthPercentage::percent(value.percent)
    } else {
        taffy_layout::LengthPercentage::length(value.length_points_max_zero())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn length(points: f32) -> css::ComputedLengthPercentageOrAuto {
        css::ComputedLengthPercentageOrAuto::LengthPercentage(
            css::ComputedLengthPercentage::from_points(points),
        )
    }

    fn percent(percent: f32) -> css::ComputedLengthPercentageOrAuto {
        css::ComputedLengthPercentageOrAuto::LengthPercentage(
            css::ComputedLengthPercentage::from_percent(percent),
        )
    }

    #[test]
    fn stretch_fit_content_size_is_non_negative_after_borders() {
        let non_content = 200.0;
        let content = stretch_fit_content_box_size(0.0, 0.0, non_content_pt(non_content)).points();

        assert_eq!(content, 0.0);
        assert_eq!(content + non_content, 200.0);
    }

    #[test]
    fn used_content_box_size_keeps_content_box_specified_size() {
        let content = used_content_box_size(
            length(120.0),
            BoxSizing::ContentBox,
            300.0,
            non_content_pt(40.0),
        )
        .expect("content-box length should resolve");

        assert_eq!(content.points(), 120.0);
    }

    #[test]
    fn used_content_box_size_subtracts_border_box_non_content_and_clamps() {
        let content = used_content_box_size(
            length(100.0),
            BoxSizing::BorderBox,
            300.0,
            non_content_pt(150.0),
        )
        .expect("border-box length should resolve");

        assert_eq!(content.points(), 0.0);
    }

    #[test]
    fn stretch_fit_content_box_size_returns_typed_non_negative_content() {
        let content = stretch_fit_content_box_size(100.0, 30.0, non_content_pt(120.0));

        assert_eq!(content.points(), 0.0);
    }

    #[test]
    fn used_content_box_size_optional_basis_resolves_lengths_not_percentages() {
        let length_content = used_content_box_size_with_optional_basis(
            length(42.0),
            BoxSizing::ContentBox,
            None,
            non_content_pt(10.0),
        )
        .expect("length-only value should resolve without a basis");
        let percentage_content = used_content_box_size_with_optional_basis(
            percent(0.5),
            BoxSizing::ContentBox,
            None,
            non_content_pt(10.0),
        );

        assert_eq!(length_content.points(), 42.0);
        assert!(percentage_content.is_none());
    }
}

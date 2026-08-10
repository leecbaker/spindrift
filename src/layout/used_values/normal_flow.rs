use super::*;
/// Resolved horizontal geometry for a normal-flow block-level box.
///
/// CSS 2.2 defines one equation for the used inline margins, borders, padding,
/// and content width of block-level non-replaced boxes in normal flow. Keeping
/// the result together avoids callers mixing a content width resolved against
/// one basis with a border box positioned from another:
/// <https://www.w3.org/TR/CSS22/visudet.html#blockwidth>.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(in crate::layout) struct NormalFlowBlockInlineGeometry {
    pub(in crate::layout) content_width: ContentBoxLength,
    pub(in crate::layout) border_box_inline_span: PageInlineSpan,
}

impl NormalFlowBlockInlineGeometry {
    pub(in crate::layout) fn border_box_width(self) -> BorderBoxLength {
        border_box_pt(self.border_box_inline_span.width())
    }
}

/// Width inputs for resolving a block container's requested content size.
///
/// `available_outer_width` is the margin-adjusted stretch-fit size used by
/// `auto` and intrinsic sizing keywords. `percentage_basis` is the containing
/// block inline size used by length-percentage properties:
/// <https://www.w3.org/TR/CSS22/visudet.html#blockwidth>.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(in crate::layout) struct BlockContentWidthInputs {
    pub(in crate::layout) available_outer_width: LayoutLength,
    pub(in crate::layout) percentage_basis: PercentageBasis<LayoutLength>,
    pub(in crate::layout) horizontal_non_content: NonContentLength,
    /// A resolved physical height, which is the logical inline-size of a
    /// vertical block while its auto physical width is being measured.
    pub(in crate::layout) definite_content_height: Option<PhysicalContentHeight>,
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
    containing_inline_size: LayoutLength,
) -> LayoutLength {
    layout_pt(containing_inline_size.points() - style.margin.left - style.margin.right)
}

/// Resolve a normal block's content width and page-space border-box span.
///
/// Percentages in `width`, `min-width`, and `max-width` use the containing
/// block width as their percentage basis. Only `width:auto` consumes the
/// margin-adjusted space from the CSS 2.2 block-width equation:
/// <https://www.w3.org/TR/CSS22/visudet.html#blockwidth>.
pub(in crate::layout) fn resolve_normal_flow_block_inline_geometry(
    style: &mut ComputedStyle,
    containing_inline_span: PageInlineSpan,
    requested_content_width: PhysicalContentWidth,
    horizontal_non_content: NonContentLength,
    containing_direction: Direction,
    resolve_auto_margins: bool,
) -> NormalFlowBlockInlineGeometry {
    let containing_inline_size = containing_inline_span.width();
    let content_width = constrain_width_with_stretch_fit(
        style,
        requested_content_width.content_box_length(),
        layout_pt(containing_inline_size),
        layout_pt(style.margin.left + style.margin.right),
        horizontal_non_content,
    );
    let border_box_width = content_box_to_border_box_length(content_width, horizontal_non_content);
    if resolve_auto_margins {
        resolve_normal_flow_block_auto_margins(
            style,
            containing_inline_span,
            border_box_width,
            containing_direction,
        );
    }
    let border_box_inline_span = normal_flow_block_border_box_span(
        containing_inline_span,
        style,
        border_box_width,
        containing_direction,
    );

    NormalFlowBlockInlineGeometry {
        content_width,
        border_box_inline_span,
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
    containing_inline_size: LayoutLength,
    horizontal_non_content: NonContentLength,
) -> ContentBoxLength {
    used_content_box_size(
        style.box_values.width.clone(),
        style.box_sizing,
        PercentageBasis::definite(content_box_pt(containing_inline_size.points())),
        horizontal_non_content,
    )
    .unwrap_or_else(|| {
        content_box_pt(
            (normal_flow_block_available_outer_width(style, containing_inline_size).points()
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
    containing_inline_span: PageInlineSpan,
    border_box_width: BorderBoxLength,
    containing_direction: Direction,
) {
    let left_auto = style.box_values.margin.clone().left.is_auto();
    let right_auto = style.box_values.margin.clone().right.is_auto();
    if has_auto_width(style) || (!left_auto && !right_auto) {
        return;
    }

    resolve_normal_flow_auto_margins_for_known_width(
        style,
        containing_inline_span,
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
    containing_inline_span: PageInlineSpan,
    border_box_width: BorderBoxLength,
    containing_direction: Direction,
) {
    let left_auto = style.box_values.margin.clone().left.is_auto();
    let right_auto = style.box_values.margin.clone().right.is_auto();
    if !left_auto && !right_auto {
        return;
    }

    let containing_inline_size = containing_inline_span.width();
    let border_box_width = border_box_width.points();
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

/// Return the normal-flow block border-box span after margin resolution.
///
/// CSS 2.2 block-width resolution treats a fixed-width block with no `auto`
/// horizontal inputs as over-constrained when the equation does not balance.
/// In that case the ignored side depends on the containing block's
/// `direction`: `margin-right` is ignored for LTR and `margin-left` for RTL.
/// Given an already resolved border box width, this helper positions the box
/// from the side that is not ignored:
/// <https://www.w3.org/TR/CSS22/visudet.html#blockwidth>.
pub(in crate::layout) fn normal_flow_block_border_box_span(
    containing_inline_span: PageInlineSpan,
    style: &ComputedStyle,
    border_box_width: BorderBoxLength,
    containing_direction: Direction,
) -> PageInlineSpan {
    let border_box_width = border_box_width.points();
    let start = match containing_direction {
        Direction::Ltr => containing_inline_span.left_x() + style.margin.left,
        Direction::Rtl => containing_inline_span.right_x() - style.margin.right - border_box_width,
    };
    PageInlineSpan::new(start, border_box_width)
}

/// Returns whether `width` is computed as `auto`.
///
/// CSS 2.2 block width calculations depend on whether `width` is `auto`:
/// <https://www.w3.org/TR/CSS22/visudet.html#blockwidth>.
pub(in crate::layout) fn has_auto_width(style: &ComputedStyle) -> bool {
    style.box_values.width.clone().is_auto()
}

/// Returns whether `height` is computed as `auto`.
///
/// CSS 2.2 block height calculations depend on whether `height` is `auto`:
/// <https://www.w3.org/TR/CSS22/visudet.html#normal-block>.
pub(in crate::layout) fn has_auto_height(style: &ComputedStyle) -> bool {
    style.box_values.height.is_auto()
}

#[cfg(test)]
mod tests {
    use super::super::*;

    fn style_with_horizontal_margins(
        left: css::ComputedLengthPercentageOrAuto,
        right: css::ComputedLengthPercentageOrAuto,
        used_left: f32,
        used_right: f32,
    ) -> ComputedStyle {
        let mut style = ComputedStyle::initial();
        style.box_values.margin.left = left;
        style.box_values.margin.right = right;
        style.margin.left = used_left;
        style.margin.right = used_right;
        style
    }

    fn length_auto(value: f32) -> css::ComputedLengthPercentageOrAuto {
        css::ComputedLengthPercentageOrAuto::LengthPercentage(
            css::ComputedLengthPercentage::from_points(value),
        )
    }

    fn percent_auto(value: f32) -> css::ComputedLengthPercentageOrAuto {
        css::ComputedLengthPercentageOrAuto::LengthPercentage(
            css::ComputedLengthPercentage::from_percent(value),
        )
    }
    #[test]
    fn normal_flow_auto_width_expands_through_negative_margins() {
        let mut style =
            style_with_horizontal_margins(length_auto(-20.0), length_auto(-50.0), -20.0, -50.0);

        let horizontal_non_content = non_content_pt(20.0);
        let requested = used_normal_flow_block_content_box_width(
            &style,
            layout_pt(100.0),
            horizontal_non_content,
        );
        let width = resolve_normal_flow_block_inline_geometry(
            &mut style,
            PageInlineSpan::from_edges(0.0, 100.0),
            PhysicalContentWidth::new(requested),
            horizontal_non_content,
            Direction::Ltr,
            true,
        );

        assert_eq!(requested.points(), 150.0);
        assert_eq!(width.content_width.points(), 150.0);
        assert_eq!(width.border_box_width().points(), 170.0);
        assert_eq!(width.border_box_inline_span.left_x(), -20.0);
    }

    #[test]
    fn normal_flow_percentage_width_uses_containing_block_despite_negative_margins() {
        let mut style =
            style_with_horizontal_margins(length_auto(-20.0), length_auto(-50.0), -20.0, -50.0);
        style.box_values.width = percent_auto(0.5);

        let horizontal_non_content = non_content_pt(0.0);
        let requested = used_normal_flow_block_content_box_width(
            &style,
            layout_pt(100.0),
            horizontal_non_content,
        );
        let width = resolve_normal_flow_block_inline_geometry(
            &mut style,
            PageInlineSpan::from_edges(0.0, 100.0),
            PhysicalContentWidth::new(requested),
            horizontal_non_content,
            Direction::Ltr,
            true,
        );

        assert_eq!(requested.points(), 50.0);
        assert_eq!(width.content_width.points(), 50.0);
        assert_eq!(width.border_box_width().points(), 50.0);
        assert_eq!(width.border_box_inline_span.left_x(), -20.0);
    }

    #[test]
    fn normal_flow_rtl_fixed_width_anchors_from_right_side() {
        let mut style =
            style_with_horizontal_margins(length_auto(15.0), length_auto(20.0), 15.0, 20.0);
        style.box_values.width = length_auto(80.0);

        let horizontal_non_content = non_content_pt(0.0);
        let requested = used_normal_flow_block_content_box_width(
            &style,
            layout_pt(100.0),
            horizontal_non_content,
        );
        let width = resolve_normal_flow_block_inline_geometry(
            &mut style,
            PageInlineSpan::from_edges(0.0, 100.0),
            PhysicalContentWidth::new(requested),
            horizontal_non_content,
            Direction::Rtl,
            true,
        );

        assert_eq!(requested.points(), 80.0);
        assert_eq!(width.content_width.points(), 80.0);
        assert_eq!(width.border_box_width().points(), 80.0);
        assert_eq!(width.border_box_inline_span.left_x(), 0.0);
    }

    #[test]
    fn normal_flow_block_width_keeps_content_and_border_box_types_distinct() {
        let mut style = style_with_horizontal_margins(length_auto(0.0), length_auto(0.0), 0.0, 0.0);
        style.box_values.width = length_auto(150.0);
        let horizontal_non_content = non_content_pt(20.0);
        let requested = used_normal_flow_block_content_box_width(
            &style,
            layout_pt(300.0),
            horizontal_non_content,
        );
        let width = resolve_normal_flow_block_inline_geometry(
            &mut style,
            PageInlineSpan::from_edges(0.0, 300.0),
            PhysicalContentWidth::new(requested),
            horizontal_non_content,
            Direction::Ltr,
            true,
        );

        let _content: crate::units::ContentBoxLength = width.content_width;
        let _border: crate::units::BorderBoxLength = width.border_box_width();
        assert_eq!(width.content_width.points(), 150.0);
        assert_eq!(width.border_box_width().points(), 170.0);
    }

    #[test]
    fn normal_flow_block_width_uses_border_box_points_for_auto_margins() {
        let mut style = style_with_horizontal_margins(
            css::ComputedLengthPercentageOrAuto::Auto,
            css::ComputedLengthPercentageOrAuto::Auto,
            0.0,
            0.0,
        );
        style.box_values.width = length_auto(150.0);
        let horizontal_non_content = non_content_pt(20.0);
        let requested = used_normal_flow_block_content_box_width(
            &style,
            layout_pt(200.0),
            horizontal_non_content,
        );

        let width = resolve_normal_flow_block_inline_geometry(
            &mut style,
            PageInlineSpan::from_edges(0.0, 200.0),
            PhysicalContentWidth::new(requested),
            horizontal_non_content,
            Direction::Ltr,
            true,
        );

        assert_eq!(width.border_box_width().points(), 170.0);
        assert_eq!(style.margin.left, 15.0);
        assert_eq!(style.margin.right, 15.0);
    }

    #[test]
    fn both_auto_margins_keep_start_side_zero_when_ltr_block_overflows() {
        let mut style = style_with_horizontal_margins(
            css::ComputedLengthPercentageOrAuto::Auto,
            css::ComputedLengthPercentageOrAuto::Auto,
            0.0,
            0.0,
        );

        resolve_normal_flow_auto_margins_for_known_width(
            &mut style,
            PageInlineSpan::new(0.0, 100.0),
            border_box_pt(200.0),
            Direction::Ltr,
        );

        assert_eq!(style.margin.left, 0.0);
        assert_eq!(style.margin.right, -100.0);
    }

    #[test]
    fn right_auto_margin_can_be_negative_when_ltr_block_overflows() {
        let mut style = style_with_horizontal_margins(
            css::ComputedLengthPercentageOrAuto::LengthPercentage(
                css::ComputedLengthPercentage::from_points(25.0),
            ),
            css::ComputedLengthPercentageOrAuto::Auto,
            25.0,
            0.0,
        );

        resolve_normal_flow_auto_margins_for_known_width(
            &mut style,
            PageInlineSpan::new(0.0, 100.0),
            border_box_pt(200.0),
            Direction::Ltr,
        );

        assert_eq!(style.margin.left, 25.0);
        assert_eq!(style.margin.right, -125.0);
    }

    #[test]
    fn left_auto_margin_stays_zero_when_ltr_block_overflows() {
        let mut style = style_with_horizontal_margins(
            css::ComputedLengthPercentageOrAuto::Auto,
            css::ComputedLengthPercentageOrAuto::LengthPercentage(
                css::ComputedLengthPercentage::from_points(25.0),
            ),
            0.0,
            25.0,
        );

        resolve_normal_flow_auto_margins_for_known_width(
            &mut style,
            PageInlineSpan::new(0.0, 100.0),
            border_box_pt(200.0),
            Direction::Ltr,
        );

        assert_eq!(style.margin.left, 0.0);
        assert_eq!(style.margin.right, -100.0);
    }

    #[test]
    fn both_auto_margins_keep_end_side_zero_when_rtl_block_overflows() {
        let mut style = style_with_horizontal_margins(
            css::ComputedLengthPercentageOrAuto::Auto,
            css::ComputedLengthPercentageOrAuto::Auto,
            0.0,
            0.0,
        );

        resolve_normal_flow_auto_margins_for_known_width(
            &mut style,
            PageInlineSpan::new(0.0, 100.0),
            border_box_pt(200.0),
            Direction::Rtl,
        );

        assert_eq!(style.margin.left, -100.0);
        assert_eq!(style.margin.right, 0.0);
    }
}

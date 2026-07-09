use super::super::super::*;

#[derive(Debug, Clone, Copy)]
pub(in crate::layout) struct ResolvedFloatInlineSize {
    pub(in crate::layout) content_width: ContentBoxLength,
    pub(in crate::layout) border_box_width: BorderBoxLength,
    pub(in crate::layout) margin_box_width: f32,
}

/// Freeze a float's temporary replay style to the used inline size.
///
/// CSS 2.2 resolves a float's used width from its original containing block,
/// then lays out the float's contents in that used box. Quire replays the
/// floated element in an isolated temporary containing block, so percentage
/// widths and constraints must not resolve a second time against the replay
/// block:
/// <https://www.w3.org/TR/CSS22/visudet.html#float-width> and
/// <https://www.w3.org/TR/css-cascade-5/#used>.
pub(in crate::layout) fn freeze_float_replay_width(
    style: &mut ComputedStyle,
    inline_size: ResolvedFloatInlineSize,
) {
    let replay_width = match style.box_sizing {
        BoxSizing::ContentBox => inline_size.content_width.points(),
        BoxSizing::BorderBox => inline_size.border_box_width.points(),
    };
    style.box_values.width = css::ComputedLengthPercentageOrAuto::LengthPercentage(
        css::ComputedLengthPercentage::from_points(replay_width.max(0.0)),
    );
    // The used width already incorporates the original min/max constraints.
    // Replaying those authored constraints would resolve them again in the
    // temporary containing block (and, for `border-box`, in a different box
    // coordinate from the frozen content width).
    // <https://www.w3.org/TR/css-sizing-3/#min-size-auto>
    style.box_values.min_width = css::ComputedLengthPercentageOrAuto::Auto;
    style.box_values.max_width = css::ComputedLengthPercentageOrAuto::Auto;
}

/// Prepare the principal box style used by both float measurement and final
/// float replay.
///
/// Float placement consumes the outer margins in margin-box coordinates. The
/// independent formatting context therefore starts at the border box with
/// those margins removed. Keeping this transformation shared prevents the
/// speculative used block size from resolving a different formatting context
/// than the final float replay.
/// <https://www.w3.org/TR/CSS22/visuren.html#floats>.
pub(in crate::layout) fn float_replay_style(placed_style: &ComputedStyle) -> ComputedStyle {
    let mut replay_style = placed_style.clone();
    suppress_replayed_item_margins(&mut replay_style);
    replay_style
}

impl<'a> LayoutBuilder<'a> {
    pub(in crate::layout) fn float_margin_box_width(
        &mut self,
        element: &Element,
        style: &ComputedStyle,
        stylesheets: &[Stylesheet],
        containing_width: f32,
        child_boxes: Option<&[box_tree::FormattingBox<'_>]>,
        table_fragment: Option<&box_tree::TableFragment<'_>>,
    ) -> f32 {
        self.resolved_float_inline_size(
            element,
            style,
            stylesheets,
            containing_width,
            child_boxes,
            table_fragment,
        )
        .margin_box_width
    }

    pub(in crate::layout) fn resolved_float_inline_size(
        &mut self,
        element: &Element,
        style: &ComputedStyle,
        stylesheets: &[Stylesheet],
        containing_width: f32,
        child_boxes: Option<&[box_tree::FormattingBox<'_>]>,
        table_fragment: Option<&box_tree::TableFragment<'_>>,
    ) -> ResolvedFloatInlineSize {
        let collapsed_table =
            style.display.is_table() && style.border_collapse == css::BorderCollapse::Collapse;
        let border_widths = if collapsed_table {
            css::Edges::ZERO
        } else {
            used_border_widths(style)
        };
        let horizontal_padding = if collapsed_table {
            0.0
        } else {
            style.padding.left + style.padding.right
        };
        let horizontal_extras =
            non_content_pt(border_widths.left + border_widths.right + horizontal_padding);
        let built_child_boxes;
        let built_table_fragment;
        let resolved_table_fragment = if style.display.is_table() {
            if let Some(fragment) = table_fragment {
                Some(fragment)
            } else {
                let table_children = if let Some(children) = child_boxes {
                    children
                } else {
                    built_child_boxes = self.build_frozen_child_boxes_with_current_ancestors(
                        element,
                        stylesheets,
                        style,
                    );
                    &built_child_boxes
                };
                let signature = self
                    .ancestors
                    .last()
                    .cloned()
                    .unwrap_or_else(|| element_signature(element));
                built_table_fragment =
                    box_tree::build_frozen_table_fragment(element, &signature, table_children);
                Some(&built_table_fragment)
            }
        } else {
            table_fragment
        };
        let available_outer_width =
            (containing_width - style.margin.left - style.margin.right).max(0.0);
        let vertical_non_content = non_content_pt(
            style.padding.top + style.padding.bottom + border_widths.top + border_widths.bottom,
        );
        // A float with an explicitly definite height establishes the
        // percentage basis for descendants while its shrink-to-fit inline
        // size is measured. In particular, a replaced descendant's
        // `height: 100%` can transfer through its intrinsic ratio to produce
        // the float's intrinsic width.
        // <https://www.w3.org/TR/css-sizing-3/#intrinsic-sizes>
        // <https://www.w3.org/TR/css-sizing-3/#percentage-sizing>
        let intrinsic_height_basis = used_content_box_size_with_basis(
            style.box_values.height.clone(),
            style.box_sizing,
            self.definite_block_size_stack
                .last()
                .cloned()
                .unwrap_or_else(PercentageBasis::indefinite),
            vertical_non_content,
        )
        .map(|height| {
            PercentageBasis::definite_from(height, BlockSizeBasisSource::ContainingBlock)
        });
        let measure_intrinsic_widths = |layout: &mut Self, available_width: f32| {
            if let Some(basis) = intrinsic_height_basis {
                layout.definite_block_size_stack.push(basis);
            }
            let sizes = layout.formatting_context_intrinsic_widths(
                element,
                style,
                stylesheets,
                available_width,
                child_boxes,
                resolved_table_fragment,
            );
            if intrinsic_height_basis.is_some() {
                layout.definite_block_size_stack.pop();
            }
            sizes
        };
        let specified_content_width = used_content_box_size(
            style.box_values.width.clone(),
            style.box_sizing,
            PercentageBasis::definite(content_box_pt(available_outer_width)),
            horizontal_extras,
        );
        let intrinsic_widths = ((specified_content_width.is_none()
            && needs_intrinsic_width_contribution(style.box_values.width.clone()))
            || needs_intrinsic_width_contribution(style.box_values.min_width.clone())
            || needs_intrinsic_width_contribution(style.box_values.max_width.clone()))
        .then(|| {
            let content_available_width =
                (available_outer_width - horizontal_extras.points()).max(0.0);
            let (preferred_min, preferred) =
                measure_intrinsic_widths(self, content_available_width);
            let preferred = if style.display.is_flex()
                && style.box_values.width.is_auto()
                && style.flex_wrap != FlexWrap::NoWrap
                && style.flex_direction.is_column_axis()
                && content_available_width > preferred + 0.01
            {
                // CSS 2.2 floats apply shrink-to-fit after the flex container
                // intrinsic pass. WPT's column-wrap float case treats an
                // unconstrained available width as min-content, while still
                // letting narrower containing blocks clamp up to wrapped
                // max-content:
                // https://www.w3.org/TR/CSS22/visudet.html#float-width
                // https://www.w3.org/TR/css-flexbox-1/#intrinsic-cross-sizes
                preferred_min
            } else {
                preferred
            };
            (preferred_min, preferred)
        });
        let content_width = specified_content_width.unwrap_or_else(|| {
            if let Some((preferred_min, preferred)) = intrinsic_widths {
                intrinsic::content_box_width_from_intrinsic(
                    style,
                    layout_pt(available_outer_width),
                    horizontal_extras,
                    content_box_pt(preferred_min),
                    content_box_pt(preferred),
                    intrinsic::IntrinsicAutoWidth::ShrinkToFit,
                )
            } else {
                let content_available_width =
                    (available_outer_width - horizontal_extras.points()).max(0.0);
                let (preferred_min, preferred) =
                    measure_intrinsic_widths(self, content_available_width);
                let preferred = if style.display.is_flex()
                    && style.box_values.width.is_auto()
                    && style.flex_wrap != FlexWrap::NoWrap
                    && style.flex_direction.is_column_axis()
                    && content_available_width > preferred + 0.01
                {
                    preferred_min
                } else {
                    preferred
                };
                intrinsic::content_box_width_from_intrinsic(
                    style,
                    layout_pt(available_outer_width),
                    horizontal_extras,
                    content_box_pt(preferred_min),
                    content_box_pt(preferred),
                    intrinsic::IntrinsicAutoWidth::ShrinkToFit,
                )
            }
        });
        let content_width = if let Some((preferred_min, preferred)) = intrinsic_widths {
            constrain_width_with_intrinsic(
                style,
                content_width,
                content_box_pt(preferred_min),
                content_box_pt(preferred),
                PercentageBasis::definite(content_box_pt(available_outer_width)),
                horizontal_extras,
            )
        } else {
            constrain_content_width(
                style,
                content_width,
                PercentageBasis::definite(layout_pt(available_outer_width)),
            )
        };
        let visual_horizontal_extras = if collapsed_table {
            self.collapsed_table_outer_horizontal_insets(
                style,
                stylesheets,
                resolved_table_fragment,
            )
            .unwrap_or(0.0)
        } else {
            0.0
        };
        resolved_float_inline_size_from_content_box(
            style,
            content_width,
            horizontal_extras,
            visual_horizontal_extras,
        )
    }

    /// Return the floated margin-box block size used for placement.
    ///
    /// CSS 2.2 makes auto-height floating non-replaced elements use the same
    /// descendant-based height calculation as BFC roots. Empty floats therefore
    /// have zero content height instead of reserving a synthetic line box:
    /// <https://www.w3.org/TR/CSS22/visudet.html#root-height>.
    pub(in crate::layout) fn float_margin_box_height(
        &mut self,
        element: &Element,
        style: &ComputedStyle,
        stylesheets: &[Stylesheet],
        inline_size: ResolvedFloatInlineSize,
        child_boxes: Option<&[box_tree::FormattingBox<'_>]>,
        table_fragment: Option<&box_tree::TableFragment<'_>>,
    ) -> f32 {
        let built_child_boxes;
        let child_boxes = if child_boxes.is_some() || is_replaced_element(element) {
            child_boxes
        } else {
            built_child_boxes =
                self.build_frozen_child_boxes_with_current_ancestors(element, stylesheets, style);
            Some(built_child_boxes.as_slice())
        };
        // A definite used height needs no speculative formatting pass. Keep
        // the established estimator for that path, which is also used by
        // intrinsic float queries.
        if !has_auto_height(style) {
            if style.display.is_flex() {
                return self.estimate_floated_flex_margin_box_height(
                    element,
                    style,
                    stylesheets,
                    inline_size.margin_box_width,
                    child_boxes,
                );
            }
            return self
                .estimate_element_height(
                    element,
                    style,
                    stylesheets,
                    inline_size.margin_box_width,
                    child_boxes,
                )
                .unwrap_or(0.0);
        }

        self.measure_auto_float_margin_box_height(
            element,
            style,
            stylesheets,
            inline_size,
            child_boxes,
            table_fragment,
        )
    }

    /// Measure an auto-height float in the same isolated flow-root replay
    /// used to paint it later.
    ///
    /// A generic descendant height estimate cannot model every inline and
    /// generated-content interaction. In particular, it can retain earlier
    /// float exclusions while recursively estimating a later floated list.
    /// Replay instead uses the frozen used width and a fresh float context,
    /// then rolls back pages, counters, generated content, positioned layers,
    /// and paint through `LayoutSnapshot`.
    /// <https://www.w3.org/TR/CSS22/visudet.html#root-height> and
    /// <https://www.w3.org/TR/CSS22/visuren.html#floats>.
    fn measure_auto_float_margin_box_height(
        &mut self,
        element: &Element,
        placed_style: &ComputedStyle,
        stylesheets: &[Stylesheet],
        inline_size: ResolvedFloatInlineSize,
        child_boxes: Option<&[box_tree::FormattingBox<'_>]>,
        table_fragment: Option<&box_tree::TableFragment<'_>>,
    ) -> f32 {
        let snapshot = self.snapshot();
        let probe_top = 10_000.0;
        let replay_style = float_replay_style(placed_style);

        // The probe is a page-shaped, non-persistent formatting context. Its
        // fragmentainer is made effectively unbounded so it measures the
        // float's used block size rather than a page fragment.
        self.current_page = Page::new(inline_size.border_box_width.points().max(1.0), probe_top);
        self.content_left = placed_style.margin.left;
        self.content_right = self.content_left + inline_size.border_box_width.points().max(1.0);
        self.cursor_y = probe_top - placed_style.margin.top;
        self.containing_block_direction = placed_style.direction;
        self.containing_block_writing_mode = placed_style.writing_mode;
        self.fragmentation_suppression_depth += 1;
        self.push_page_name_scope_suppression();
        self.push_float_context();
        self.preserve_scoped_paint_public_order = true;
        self.layout_element_with_child_boxes_and_table_fragment(
            element,
            &replay_style,
            stylesheets,
            child_boxes,
            table_fragment,
        );
        let border_box_height = (probe_top - placed_style.margin.top - self.cursor_y).max(0.0);
        self.restore(snapshot);

        placed_style.margin.top + border_box_height + placed_style.margin.bottom
    }
}

fn resolved_float_inline_size_from_content_box(
    style: &ComputedStyle,
    content_width: ContentBoxLength,
    horizontal_extras: NonContentLength,
    visual_horizontal_extras: f32,
) -> ResolvedFloatInlineSize {
    let border_box_width = content_box_to_border_box_length(content_width, horizontal_extras);
    ResolvedFloatInlineSize {
        content_width,
        border_box_width,
        margin_box_width: style.margin.left
            + border_box_width.points()
            + visual_horizontal_extras
            + style.margin.right,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn length(value: f32) -> css::ComputedLengthPercentageOrAuto {
        css::ComputedLengthPercentageOrAuto::LengthPercentage(
            css::ComputedLengthPercentage::from_points(value),
        )
    }

    fn length_points(value: css::ComputedLengthPercentageOrAuto) -> f32 {
        match value {
            css::ComputedLengthPercentageOrAuto::LengthPercentage(value) => value.length_points(),
            _ => panic!("expected resolved length"),
        }
    }

    #[test]
    fn float_inline_size_expands_content_box_to_border_and_margin_boxes() {
        let mut style = ComputedStyle::initial();
        style.box_values.width = length(150.0);
        style.margin.left = 5.0;
        style.margin.right = 7.0;

        let inline_size = resolved_float_inline_size_from_content_box(
            &style,
            content_box_pt(150.0),
            non_content_pt(20.0),
            0.0,
        );

        assert_eq!(inline_size.content_width.points(), 150.0);
        assert_eq!(inline_size.border_box_width.points(), 170.0);
        assert_eq!(inline_size.margin_box_width, 182.0);
    }

    #[test]
    fn border_box_float_width_clamps_content_and_keeps_extras_once() {
        let mut style = ComputedStyle::initial();
        style.box_sizing = BoxSizing::BorderBox;
        style.box_values.width = length(100.0);
        let extras = non_content_pt(150.0);
        let content_width = used_content_box_size(
            style.box_values.width.clone(),
            style.box_sizing,
            PercentageBasis::definite(content_box_pt(300.0)),
            extras,
        )
        .unwrap();

        let inline_size =
            resolved_float_inline_size_from_content_box(&style, content_width, extras, 0.0);

        assert_eq!(inline_size.content_width.points(), 0.0);
        assert_eq!(inline_size.border_box_width.points(), 150.0);
        assert_eq!(inline_size.margin_box_width, 150.0);
    }

    #[test]
    fn auto_float_shrink_to_fit_returns_content_box_length() {
        let style = ComputedStyle::initial();
        let width = intrinsic::content_box_width_from_intrinsic(
            &style,
            layout_pt(150.0),
            non_content_pt(20.0),
            content_box_pt(80.0),
            content_box_pt(200.0),
            intrinsic::IntrinsicAutoWidth::ShrinkToFit,
        );

        let _typed: ContentBoxLength = width;
        assert_eq!(width.points(), 130.0);
    }

    #[test]
    fn freeze_float_replay_width_writes_box_sizing_specific_used_width() {
        let inline_size = ResolvedFloatInlineSize {
            content_width: content_box_pt(80.0),
            border_box_width: border_box_pt(120.0),
            margin_box_width: 120.0,
        };
        let mut content_box_style = ComputedStyle::initial();
        content_box_style.box_sizing = BoxSizing::ContentBox;
        freeze_float_replay_width(&mut content_box_style, inline_size);

        let mut border_box_style = ComputedStyle::initial();
        border_box_style.box_sizing = BoxSizing::BorderBox;
        freeze_float_replay_width(&mut border_box_style, inline_size);

        assert_eq!(length_points(content_box_style.box_values.width), 80.0);
        assert!(content_box_style.box_values.min_width.is_auto());
        assert!(content_box_style.box_values.max_width.is_auto());
        assert_eq!(length_points(border_box_style.box_values.width), 120.0);
        assert!(border_box_style.box_values.min_width.is_auto());
        assert!(border_box_style.box_values.max_width.is_auto());
    }
}

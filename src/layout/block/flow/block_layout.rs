use super::*;
use crate::document::paint::geometry::AxisSelectivePaintClip;
use crate::layout::assets::{DocumentPageIndex, FragmentainerOrdinal};
use crate::layout::block::float::FLOAT_EPSILON;
use crate::layout::block::formatting_boxes_have_eligible_multicol_spanner;
use crate::layout::paint_ops::FragmentedDecorationSlice;

/// A definite block-size constraint prepared for a normal-flow replay.
///
/// CSS 2.2 requires the second pass after a winning `min-height` or
/// `max-height` constraint to use that constraint as the computed `height`.
/// CSS Box Sizing makes a fixed sizing value refer to either the content or
/// border box, so retaining that coordinate system prevents replay from
/// applying padding and borders a second time.
/// <https://www.w3.org/TR/CSS22/visudet.html#min-max-heights>
/// <https://www.w3.org/TR/css-sizing-3/#box-model>
#[derive(Debug, Clone, Copy, PartialEq)]
enum PostLayoutHeightReplay {
    ContentBox(ContentBoxLength),
    BorderBox(BorderBoxLength),
}

impl PostLayoutHeightReplay {
    fn from_specified_length(value: LayoutLength, box_sizing: BoxSizing) -> Self {
        match box_sizing {
            BoxSizing::ContentBox => Self::ContentBox(content_box_pt(value.points().max(0.0))),
            BoxSizing::BorderBox => Self::BorderBox(border_box_pt(value.points().max(0.0))),
        }
    }

    fn content_box_length(self, vertical_non_content: NonContentLength) -> ContentBoxLength {
        match self {
            Self::ContentBox(value) => value,
            Self::BorderBox(value) => border_box_to_content_box_length(value, vertical_non_content),
        }
    }

    fn as_used_height(self) -> css::ComputedLengthPercentageOrAuto {
        let value = match self {
            Self::ContentBox(value) => value.points(),
            Self::BorderBox(value) => value.points(),
        };
        css::ComputedLengthPercentageOrAuto::LengthPercentage(
            css::ComputedLengthPercentage::from_points(value),
        )
    }
}

/// Return the min/max constraint that CSS 2.2 substitutes for `height`.
///
/// Maximum constraints apply first, then minimum constraints. The returned
/// value remains in the sizing property's specified box-model space, while
/// comparison happens in content-box space.
/// <https://www.w3.org/TR/CSS22/visudet.html#min-max-heights>
fn post_layout_height_replay_constraint(
    style: &ComputedStyle,
    percentage_basis: BlockSizePercentageBasis,
    vertical_non_content: NonContentLength,
    tentative_content_height: ContentBoxLength,
) -> Option<PostLayoutHeightReplay> {
    let min = match style.box_values.min_height.clone() {
        css::ComputedLengthPercentageOrAuto::LengthPercentage(value) => {
            Some(PostLayoutHeightReplay::from_specified_length(
                used_length_percentage(value, percentage_basis),
                style.box_sizing,
            ))
        }
        _ => None,
    };
    let max = used_length_percentage_or_auto(style.box_values.max_height.clone(), percentage_basis)
        .map(|value| PostLayoutHeightReplay::from_specified_length(value, style.box_sizing));

    let after_max = max.map_or(tentative_content_height, |constraint| {
        tentative_content_height.min(constraint.content_box_length(vertical_non_content))
    });
    if let Some(min) = min
        && after_max < min.content_box_length(vertical_non_content)
    {
        return Some(min);
    }
    max.filter(|constraint| {
        tentative_content_height > constraint.content_box_length(vertical_non_content)
    })
}

/// Permission to measure descendants for the deferred-overflow path.
///
/// CSS overflow can move descendant paint independently only after the
/// principal box has a definite content height. Keeping that height in this
/// capability prevents an auto-height box from being measured as if its
/// absent size were zero.
/// <https://www.w3.org/TR/css-sizing-3/#definite>
/// <https://www.w3.org/TR/css-overflow-3/#overflow>
#[derive(Debug, Clone, Copy)]
struct DeferredDescendantOverflowProbe {
    content_height: DefinitePhysicalContentHeight,
    vertical_root_block_size: Option<LayoutLength>,
}

impl DeferredDescendantOverflowProbe {
    fn new(
        content_height: Option<DefinitePhysicalContentHeight>,
        has_deferred_overflow_candidate: bool,
        multicol_spanner_speculation_depth: usize,
        overflow_contribution: DescendantOverflowContribution,
        vertical_root_block_size: Option<LayoutLength>,
    ) -> Option<Self> {
        (has_deferred_overflow_candidate
            && multicol_spanner_speculation_depth == 0
            && overflow_contribution == DescendantOverflowContribution::Scrollable)
            .then_some(content_height)
            .flatten()
            .map(|content_height| Self {
                content_height,
                vertical_root_block_size,
            })
    }

    fn content_height(self) -> PhysicalContentHeight {
        self.content_height.value()
    }

    fn is_vertical_root(self) -> bool {
        self.vertical_root_block_size.is_some()
    }
}

impl<'a> LayoutBuilder<'a> {
    /// Measure a block formatting context root in one candidate float band.
    ///
    /// The percentage basis remains the containing block's full content
    /// width: float avoidance changes a BFC root's available inline space,
    /// not the percentage basis inherited by its descendants.
    /// <https://www.w3.org/TR/CSS22/visuren.html#floats>
    #[allow(clippy::too_many_arguments)]
    fn measure_float_avoiding_bfc(
        &mut self,
        element: &Element,
        style: &ComputedStyle,
        stylesheets: &Stylesheets<'_>,
        child_boxes: Option<&[box_tree::FormattingBox<'_>]>,
        containing_inline_size: f32,
        containing_inline_span: PageInlineSpan,
        band: FloatBand,
    ) -> FloatAvoidanceCandidate {
        let band_left = band.left();
        let band_width = band.width();
        let auto_border_box_width = (band_width < containing_inline_size - FLOAT_EPSILON)
            .then_some(float_avoiding_auto_border_box_width(
                PageInlineSpan::new(band_left, band_width),
                containing_inline_span,
                style.margin.left,
                style.margin.right,
            ));
        let constrained_auto_width = auto_border_box_width.is_some();
        let mut candidate_geometry = self.block_layout_geometry_in_inline_span(
            element,
            style,
            stylesheets,
            child_boxes,
            BlockLayoutInlineConstraint {
                // Width and margin resolution still use the CSS containing
                // span. Only an auto-width root gets the residual band as a
                // selected border-box placement below.
                containing_inline_span,
                percentage_basis: PercentageBasis::definite(LogicalInlineContentSize::new(
                    content_box_pt(containing_inline_size),
                )),
                physical_width_percentage_basis: PhysicalContentWidth::new(content_box_pt(
                    containing_inline_size,
                )),
                auto_border_box_width,
            },
        );
        // A BFC root's border box may meet the float at the band edge while
        // its margin overlaps the exclusion. The ordinary block-width
        // resolver intentionally places a margin box from the containing
        // span, so normalize the candidate back to the edge selected by float
        // avoidance before testing collision:
        // <https://www.w3.org/TR/CSS22/visuren.html#floats>.
        if constrained_auto_width {
            let border_box_width = candidate_geometry.outer_inline().width().points();
            let border_box_left = if style.direction == Direction::Rtl {
                band_left + (band_width - border_box_width).max(0.0)
            } else {
                band_left
            };
            candidate_geometry.reanchor_float_avoiding_border_box(PageInlineSpan::new(
                border_box_left,
                border_box_width,
            ));
        }
        let candidate_style = &candidate_geometry.style;
        let estimated_outer_height = self
            .estimate_element_height(
                element,
                candidate_style,
                stylesheets,
                candidate_geometry.outer_inline().width().points(),
                child_boxes,
            )
            .unwrap_or(
                candidate_style.margin.top
                    + candidate_style.line_height
                    + candidate_style.margin.bottom,
            );
        let border_box_height =
            (estimated_outer_height - candidate_style.margin.top - candidate_style.margin.bottom)
                .max(0.0);
        candidate_geometry.float_avoidance_candidate(border_box_pt(border_box_height))
    }

    pub(in crate::layout) fn layout_block(
        &mut self,
        element: &Element,
        style: &ComputedStyle,
        stylesheets: &Stylesheets<'_>,
        run_in_children: &[box_tree::FormattingBox<'_>],
        child_boxes: Option<&[box_tree::FormattingBox<'_>]>,
    ) {
        self.layout_block_with_descendant_percentage_height_basis(
            element,
            style,
            stylesheets,
            run_in_children,
            child_boxes,
            None,
            PrincipalBoxPaintMode::RootPaints,
        );
    }

    #[allow(clippy::too_many_arguments)]
    pub(in crate::layout) fn layout_block_with_descendant_percentage_height_basis(
        &mut self,
        element: &Element,
        style: &ComputedStyle,
        stylesheets: &Stylesheets<'_>,
        run_in_children: &[box_tree::FormattingBox<'_>],
        child_boxes: Option<&[box_tree::FormattingBox<'_>]>,
        descendant_percentage_height_basis: Option<BlockSizePercentageBasis>,
        principal_box_paint_mode: PrincipalBoxPaintMode,
    ) {
        // Multicolumn layout and CSS block-in-inline splitting both require
        // the normalized formatting tree. Build that structure from the
        // unscaled cascade parent before this block crosses its normal-flow
        // used-value boundary.
        let requires_block_in_inline_normalization =
            has_block_in_inline_split_boundary_with_font_metrics(
                element,
                style,
                stylesheets,
                &self.ancestors,
                &mut self.font_system,
            );
        let requires_table_internal_fixup =
            has_unwrapped_table_internal_descendant_with_font_metrics(
                element,
                style,
                stylesheets,
                &self.ancestors,
                &mut self.font_system,
            );
        let requires_run_in_normalization = has_direct_run_in_child_with_font_metrics(
            element,
            style,
            stylesheets,
            &self.ancestors,
            &mut self.font_system,
        );
        let requires_ruby_structure = has_ruby_formatting_descendant_with_font_metrics(
            element,
            style,
            stylesheets,
            &self.ancestors,
            &mut self.font_system,
            &mut self.ruby_formatting_descendants,
        );
        // Tree-abiding generated pseudos participate in the originating
        // element's child list, including as flex and grid items. The DOM
        // traversal intentionally visits only principal element children,
        // while the frozen formatting tree retains `::before`/`::after`
        // boxes and their display blockification.
        // <https://www.w3.org/TR/css-pseudo-4/#generated-content>
        // <https://www.w3.org/TR/css-position-3/#position-property>
        let requires_generated_child_tree = style
            .before_style
            .iter()
            .chain(style.after_style.iter())
            .any(|pseudo| pseudo.content.is_generated());
        // The HTML rendered-legend selection is defined over generated child
        // boxes, not just DOM elements. Build the frozen structural tree even
        // for an otherwise ordinary fieldset so direct DOM traversal and
        // replay traverse the same candidate set.
        // <https://html.spec.whatwg.org/multipage/rendering.html#the-fieldset-and-legend-elements>
        let requires_fieldset_structure = element.tag.eq_ignore_ascii_case("fieldset");
        let built_structural_child_boxes;
        let child_boxes = if child_boxes.is_none()
            && (matches!(style.column_count, css::ColumnCount::Count(_))
                || matches!(style.column_width, css::ComputedColumnWidth::Length(_))
                || matches!(style.column_height, css::ComputedColumnHeight::Length(_))
                // Freeze a zoomed subtree from its computed parent. The DOM
                // inline fast path otherwise cascades descendants from this
                // block's used values and multiplies their effective zoom.
                // <https://drafts.csswg.org/css-viewport/#zoom-property>
                || style.effective_zoom.factor() != 1.0
                || requires_block_in_inline_normalization
                || requires_table_internal_fixup
                || requires_run_in_normalization
                || requires_ruby_structure
                || requires_generated_child_tree
                || requires_fieldset_structure)
        {
            built_structural_child_boxes =
                self.build_frozen_child_boxes_with_current_ancestors(element, stylesheets, style);
            Some(built_structural_child_boxes.as_slice())
        } else {
            child_boxes
        };
        // A rendered legend is laid out before the anonymous fieldset content
        // box even when generated content or source text precedes it. Keep
        // the original boxes themselves intact—only their rendering order is
        // changed—so counters, selectors, and source metadata remain owned by
        // their original principal or pseudo boxes.
        // <https://html.spec.whatwg.org/multipage/rendering.html#the-fieldset-and-legend-elements>
        let reordered_fieldset_child_boxes;
        let child_boxes = if requires_fieldset_structure {
            let Some(children) = child_boxes else {
                unreachable!("fieldset structure always builds child boxes")
            };
            if let Some(legend_index) =
                box_tree::FieldsetFormattingBox::from_children(children).rendered_legend_index
            {
                let mut reordered = Vec::with_capacity(children.len());
                reordered.push(children[legend_index].clone());
                reordered.extend_from_slice(&children[..legend_index]);
                reordered.extend_from_slice(&children[legend_index + 1..]);
                reordered_fieldset_child_boxes = reordered;
                Some(reordered_fieldset_child_boxes.as_slice())
            } else {
                child_boxes
            }
        } else {
            child_boxes
        };
        // Block line layout consumes its own style directly for line-height
        // and baseline geometry, unlike box sizing which clones a used style
        // internally. Normalize this boundary before collecting inline items.
        // <https://drafts.csswg.org/css-viewport/#zoom-property>
        let source_style = style;
        let mut used_style = self.style_with_current_used_lengths(style);
        if element.tag.eq_ignore_ascii_case("fieldset") {
            // HTML gives a fieldset a used flow-root (or inline-block) role
            // without changing its computed `display` value.
            // <https://html.spec.whatwg.org/multipage/rendering.html#the-fieldset-and-legend-elements>
            used_style.display = used_style.display.with_inner(DisplayInner::FlowRoot);
        }
        let style = &used_style;
        let containment = used_property_containment(element, style);
        if matches!(style.position, Position::Absolute | Position::Fixed) {
            self.layout_positioned_block_with_static_source(
                element,
                // `style` is this block's already-normalized used view.
                // Positioned layout owns a separate used-value boundary, so
                // it must start from the frozen cascade source rather than
                // normalizing the used view a second time.
                source_style,
                stylesheets,
                child_boxes,
                None,
            );
            return;
        }

        let fragmentainer_kind = self.active_fragmentainer_kind();
        let fragments_as_promoted_spanner = self.multicol_spanner_fragmentation_depth > 0
            && self.fragmentation_suppression_depth == 0
            && style.column_span == css::ColumnSpan::All;
        let mut geometry =
            self.block_layout_geometry(element, source_style, stylesheets, child_boxes);
        let definite_principal_fits_current_column = fragmentainer_kind
            == FragmentainerKind::Column
            && !fragments_as_promoted_spanner
            && style.writing_mode == WritingMode::HorizontalTb
            && !containment.size
            && !fragmentainer_kind.is_forced_break(style.break_before)
            && !formatting_boxes_have_forced_break_in(child_boxes, fragmentainer_kind)
            && geometry.definite_content_height.is_some_and(|height| {
                let available =
                    (self.cursor_y + geometry.relative_offset.y() - self.page_bottom()).max(0.0);
                let outer_height = style.margin.top
                    + geometry.vertical_non_content.points()
                    + height.value().points().max(0.0)
                    + style.margin.bottom;
                outer_height <= available + 0.01
            });
        let definite_vertical_root_block_size = (fragmentainer_kind == FragmentainerKind::Page
            && style.writing_mode == self.principal_flow.writing_mode
            && self.containing_block_writing_mode == self.principal_flow.writing_mode
            && WritingModeAxes::new(
                self.principal_flow.writing_mode,
                self.principal_flow.used_direction(),
            )
            .swaps_physical_axes()
            && matches!(
                style.box_values.width.clone(),
                css::ComputedLengthPercentageOrAuto::LengthPercentage(_)
            )
            && geometry.outer_inline().width().points()
                > self
                    .current_page_context
                    .logical_block_size(self.principal_flow.writing_mode)
                    + 0.01)
            .then(|| layout_pt(geometry.outer_inline().width().points()));
        let deferred_overflow_probe = DeferredDescendantOverflowProbe::new(
            geometry.definite_content_height,
            fragments_as_promoted_spanner
                || definite_principal_fits_current_column
                || definite_vertical_root_block_size.is_some(),
            self.multicol_spanner_speculation_depth,
            descendant_overflow_contribution_for_element(element, style),
            definite_vertical_root_block_size,
        );
        if let Some(probe) = deferred_overflow_probe {
            let descendants_overflow = if probe.is_vertical_root() {
                // The speculative pass is authoritative: logical block-size
                // estimation cannot see every normal-flow formatting-box shape.
                // Enter it for a fixed vertical root block with in-flow children
                // and retain only the captured range that actually paints.
                child_boxes.map_or_else(
                    || {
                        has_direct_flow_child_with_font_metrics(
                            element,
                            style,
                            stylesheets,
                            &mut self.font_system,
                        )
                    },
                    |children| {
                        children.iter().any(|child| match child {
                            box_tree::FormattingBox::AnonymousBlock(_) => true,
                            _ => child.element_parts().is_some_and(|(_, _, child_style, _)| {
                                style_is_in_normal_flow(child_style)
                                    && child_style.display.is_block_level()
                            }),
                        })
                    },
                )
            } else {
                self.definite_block_descendants_overflow(
                    child_boxes,
                    stylesheets,
                    geometry.content_inline().width().points(),
                    probe,
                )
            };
            if descendants_overflow {
                self.layout_definite_block_with_deferred_descendant_overflow(
                    element,
                    style,
                    stylesheets,
                    run_in_children,
                    child_boxes,
                    descendant_percentage_height_basis,
                    probe,
                    geometry.vertical_non_content,
                    principal_box_paint_mode,
                );
                return;
            }
        }
        // CSS 2 resolves a winning min/max constraint by redoing layout with
        // the constrained value as the used height. Keep a rollback point so
        // a post-layout constraint can replay the complete fragmentation
        // decision, rather than retagging fragments that were assigned while
        // the principal box was still auto-sized.
        // <https://www.w3.org/TR/CSS2/visudet.html#min-max-heights>
        let may_need_post_layout_constraint_replay = geometry.definite_content_height.is_none()
            && (used_min_height(style, geometry.containing_block_content_height).is_some()
                || used_max_height(style, geometry.containing_block_content_height).is_some());
        let post_layout_constraint_replay_snapshot =
            may_need_post_layout_constraint_replay.then(|| self.snapshot());
        self.begin_clamp_line_slot_capture();
        self.apply_forced_break_before_box_in(fragmentainer_kind, style);
        // A class-A prebreak repositions this source box before it has made
        // layout progress. Recompute geometry once in the destination because
        // that fragmentainer can have different available space, but never
        // retry the same source box again: a repeated prebreak can otherwise
        // create an unbounded sequence of empty fragmentainers.
        // <https://www.w3.org/TR/css-break-3/#possible-breaks>
        let mut prebroken_before_layout = false;
        loop {
            let prebreak_content_height = geometry
                .definite_content_height
                .map(|height| height.value().points())
                .or_else(|| {
                    // A clipped overflow box is a non-fragmenting formatting
                    // context. When `break-inside` avoids this fragmentainer,
                    // measure its auto content so the class-A break before the
                    // box can keep that otherwise monolithic box intact.
                    // <https://www.w3.org/TR/css-break-3/#break-inside>
                    (fragmentainer_kind.avoids_break_inside(&geometry.style)
                        && self.element_used_overflow_clips(element, &geometry.style))
                    .then(|| {
                        self.estimate_block_like_height(
                            element,
                            &geometry.style,
                            stylesheets,
                            geometry.content_inline().width().points(),
                            child_boxes,
                        ) - geometry.style.margin.top
                            - geometry.vertical_non_content.points()
                            - geometry.style.margin.bottom
                    })
                })
                .or_else(|| {
                    let establishes_multicol =
                        matches!(geometry.style.column_count, css::ColumnCount::Count(_))
                            || matches!(
                                geometry.style.column_width,
                                css::ComputedColumnWidth::Length(_)
                            )
                            || matches!(
                                geometry.style.column_height,
                                css::ComputedColumnHeight::Length(_)
                            );
                    (fragmentainer_kind == FragmentainerKind::Column && establishes_multicol)
                        .then(|| {
                            child_boxes.and_then(|children| {
                                self.estimate_multicol_auto_block_size(
                                    &geometry.style,
                                    stylesheets,
                                    children,
                                    geometry.content_inline().width().points(),
                                )
                            })
                        })
                        .flatten()
                });
            let current_fragmentainer = self.fragmentainer_from_page_cursor(
                PageTopBlockPosition::new(self.cursor_y + geometry.relative_offset.y()),
            );
            let empty_destination_fragmentainer = match fragmentainer_kind {
                FragmentainerKind::Page => {
                    let next_context = self
                        .fragmentainer_override
                        .filter(|override_| override_.kind == FragmentainerKind::Page)
                        .map(|override_| override_.context_for_fragmentainer(self.pages.len() + 1))
                        .unwrap_or_else(|| {
                            self.resolved_page_context(
                                self.destination_document_page_number(self.pages.len() + 2),
                                false,
                            )
                        });
                    Fragmentainer::new(
                        layout_pt(next_context.area_height()),
                        layout_pt(next_context.area_height()),
                    )
                }
                FragmentainerKind::Column => {
                    let next_capacity = self
                        .fragmentainer_override
                        .map(|override_| {
                            override_
                                .context_for_fragmentainer(self.pages.len() + 1)
                                .area_height()
                        })
                        .unwrap_or_else(|| self.page_area_height());
                    Fragmentainer::new(layout_pt(next_capacity), layout_pt(next_capacity))
                }
            };
            let should_prebreak = !prebroken_before_layout
                && self.out_of_flow_prebreak_suppression_depth == 0
                && !fragments_as_promoted_spanner
                && should_prebreak_definite_block(DefiniteBlockBreakContext {
                    // A multicol formatting context has a measurable row-grid
                    // block size even when its principal `height` is auto. If
                    // its first anonymous column cannot make progress in the
                    // remaining outer column but the complete grid fits in an
                    // empty one, CSS fragmentation places it at that earlier
                    // class-A opportunity instead of creating a subpixel first
                    // column.
                    // <https://www.w3.org/TR/css-break-3/#unforced-breaks>
                    definite_content_height: prebreak_content_height,
                    vertical_non_content: geometry.vertical_non_content,
                    style: &geometry.style,
                    current_fragmentainer,
                    empty_destination_fragmentainer,
                    fragmentainer_has_occupied_flow: self.current_page_has_content()
                        || self.cursor_y < self.page_top() - 0.01,
                    at_page_top: self.cursor_is_at_page_top(),
                    suppress_for_avoid_retry: self.avoid_inside_retry_depth > 0,
                });
            if !should_prebreak {
                break;
            }
            self.push_page();
            geometry = self.block_layout_geometry(element, source_style, stylesheets, child_boxes);
            prebroken_before_layout = true;
        }
        // Inline atomic descendants can be measured while preparing the
        // block's child-flow strategy, before the child phase itself begins.
        // A supplied flex replay basis is authoritative, including an
        // explicit indefinite value; otherwise use the principal block's
        // definite content height.
        // <https://drafts.csswg.org/css-flexbox/#definite-sizes> and
        // <https://www.w3.org/TR/css-sizing-3/#percentage-sizing>
        let preparatory_descendant_percentage_basis = descendant_percentage_height_basis
            .unwrap_or_else(|| {
                block_size_percentage_basis_from_points(
                    geometry
                        .definite_content_height
                        .map(|height| height.value().points()),
                    BlockSizeBasisSource::ContainingBlock,
                )
            });
        let preparatory_descendant_percentage_basis = if preparatory_descendant_percentage_basis
            .is_definite()
            || element.document_compatibility_mode != dom::DocumentCompatibilityMode::Quirks
        {
            preparatory_descendant_percentage_basis
        } else {
            // HTML's documented parsing modes, rather than source-text
            // inference, select this browser-compatible quirks behavior.
            // Preserve a definite ancestor block basis through auto-height
            // wrappers only in a quirks document.
            self.definite_block_size_stack
                .last()
                .cloned()
                .unwrap_or(preparatory_descendant_percentage_basis)
        };
        self.definite_block_size_stack
            .push(preparatory_descendant_percentage_basis);
        let defer_own_decoration_promotion = self.defer_next_block_decoration_promotion;
        self.defer_next_block_decoration_promotion = false;
        let mut suppress_own_principal_box_decoration = !principal_box_paint_mode.root_paints();
        let containing_left = self.content_left;
        let containing_right = self.content_right;
        let containing_inline_size = (containing_right - containing_left).max(0.0);
        // Relative positioning normally enters before descendant layout so
        // the descendants paint in the shifted coordinate space. A cleared
        // relative box is the exception: `clear` must first resolve at its
        // unshifted normal-flow border edge (CSS 2.2, 9.5.2).
        if matches!(
            geometry.style.position,
            Position::Relative | Position::Sticky
        ) && geometry.style.clear == Clear::None
        {
            self.cursor_y += geometry.relative_offset.y();
        }
        let mut block_align_content_offset_y = 0.0;
        let starts_at_page_top = self.cursor_is_at_page_top() && self.truncate_page_start_margins;
        // CSS 2.2 defines clearance from the hypothetical border edge after
        // adjoining parent/first-child margins have collapsed. Resolve that
        // complete start-margin set before moving the border edge for `clear`;
        // the child traversal receives an explicit marker so it does not
        // consume that same descendant contribution a second time.
        // <https://www.w3.org/TR/CSS22/visuren.html#flow-control>
        // <https://www.w3.org/TR/CSS22/box.html#collapsing-margins>
        // The root margin is a canvas inset and cannot collapse through its
        // first child. The selected `body` remains a normal block parent for
        // its in-flow children, so its adjoining first-child margin still
        // collapses under CSS 2.2's ordinary block formatting rules.
        //
        // Treating the body itself as a collapse boundary adds its canvas
        // margin to the child's own start margin, unlike browser layout.
        // <https://www.w3.org/TR/CSS22/box.html#collapsing-margins>
        let is_document_canvas = self.element_uses_document_canvas_flow(element);
        let can_adjoin_first_child_margin =
            !is_document_canvas || !element.tag.eq_ignore_ascii_case("html");
        let (hypothetical_start_margin, _clearance_hypothetical_uses_adjoining_start_margin) =
            if geometry.style.clear != Clear::None {
                if let Some(children) = child_boxes {
                    let can_adjoin = can_adjoin_first_child_margin
                        && can_collapse_block_start_margin(
                            element,
                            &geometry.style,
                            geometry.border_edges,
                            has_direct_inline_content_box(children),
                            self.used_overflow_for_element(element, &geometry.style),
                        )
                        && collapsible_first_child_start_margin_from_boxes(
                            children,
                            element,
                            &geometry.style,
                            self.document_canvas_overflow,
                        )
                        .is_some();
                    (
                        if can_adjoin_first_child_margin {
                            collapsible_start_margin_for_box(
                                element,
                                &geometry.style,
                                children,
                                self.document_canvas_overflow,
                            )
                        } else {
                            geometry.style.margin.top
                        },
                        can_adjoin,
                    )
                } else {
                    let can_adjoin = can_adjoin_first_child_margin
                        && can_collapse_block_start_margin(
                            element,
                            &geometry.style,
                            geometry.border_edges,
                            has_direct_inline_content_before_first_flow_child_dom_with_font_metrics(
                                element,
                                &geometry.style,
                                stylesheets,
                                &self.ancestors,
                                &mut self.font_system,
                            ),
                            self.used_overflow_for_element(element, &geometry.style),
                        )
                        && collapsible_first_child_start_margin_dom_with_font_metrics(
                            element,
                            &geometry.style,
                            stylesheets,
                            &self.ancestors,
                            &mut self.font_system,
                            self.document_canvas_overflow,
                        )
                        .is_some();
                    let mut resolver = DomStyleResolver::with_font_system(&mut self.font_system);
                    (
                        if can_adjoin_first_child_margin {
                            collapsible_start_margin_dom_with_resolver(
                                element,
                                &geometry.style,
                                stylesheets,
                                &self.ancestors,
                                &mut resolver,
                                self.document_canvas_overflow,
                            )
                        } else {
                            geometry.style.margin.top
                        },
                        can_adjoin,
                    )
                }
            } else {
                (geometry.style.margin.top, false)
            };
        let applied_start_margin =
            page_start_margin(layout_pt(hypothetical_start_margin), starts_at_page_top);
        // A transparent parent may have received only its local placement
        // delta after an ancestor consumed part of a larger adjoining start
        // margin set. Its own first child must compare against that complete
        // set, or the descendant contribution is applied a second time.
        let inherited_adjoining_start_margin =
            self.inherited_adjoining_start_margins.last().copied();
        let descendant_applied_start_margin = inherited_adjoining_start_margin
            .map(InheritedAdjoiningStartMargin::complete_margin)
            .unwrap_or(applied_start_margin);
        let margin_edge_top = self.cursor_y;
        self.cursor_y -= applied_start_margin.points();
        let establishes_independent_bfc = geometry
            .style
            .display
            .establishes_block_formatting_context()
            || style_establishes_line_clamp_formatting_context(&geometry.style)
            || layout_containment_applies_to_element(element, &geometry.style)
            || paint_containment_applies_to_element(element, &geometry.style)
            || self.element_used_overflow_clips(element, &geometry.style)
            || block_align_content_establishes_independent_formatting_context(
                geometry.style.align_content,
            );
        let start_margin_arrangement = if !establishes_independent_bfc {
            let clearance = self.resolve_block_clearance(BlockClearanceRequest::block_flow(
                geometry.style.clear,
                geometry.style.writing_mode,
                geometry.style.used_direction(),
                PageTopBlockPosition::new(self.cursor_y),
                PageTopBlockPosition::new(margin_edge_top),
                applied_start_margin,
                (geometry.style.clear != Clear::None)
                    .then(|| {
                        inherited_adjoining_start_margin
                            .map(InheritedAdjoiningStartMargin::parent_start_clearance_hypothesis)
                    })
                    .flatten(),
            ));
            let start_margin_arrangement = match clearance.clearance {
                BlockClearance::NotIntroduced => BlockStartMarginArrangement::Adjoining {
                    applied_start_margin: descendant_applied_start_margin,
                },
                introduced => {
                    BlockStartMarginArrangement::from_clearance(introduced, applied_start_margin)
                }
            };
            self.cursor_y = match start_margin_arrangement {
                BlockStartMarginArrangement::Adjoining { .. } => {
                    clearance.used_border_edge.points()
                }
                BlockStartMarginArrangement::SeparatedByClearance {
                    adjusted_top_margin,
                } => adjusted_top_margin
                    .border_edge_from(PageTopBlockPosition::new(margin_edge_top))
                    .points(),
            };
            // A zero-height cleared block can still be the first normal-flow
            // content in its destination column. Keep that column before
            // temporary multicolumn pages are projected; float continuation
            // itself remains a parallel flow and never sets this ownership.
            // <https://drafts.csswg.org/css-break/#parallel-flows>
            if clearance.fragmentainer_progress.advanced() {
                self.mark_current_page_flow_content();
            }
            start_margin_arrangement
        } else {
            BlockStartMarginArrangement::Adjoining {
                applied_start_margin: descendant_applied_start_margin,
            }
        };
        if establishes_independent_bfc && geometry.style.float == Float::None {
            // A BFC root's `clear` is resolved from its hypothetical margin
            // edge.  This permits negative clearance when its start margin
            // would otherwise put the border edge below an adjoining float.
            let bfc_clearance = self.resolve_block_clearance(BlockClearanceRequest::bfc_root(
                geometry.style.clear,
                geometry.style.writing_mode,
                geometry.style.used_direction(),
                PageTopBlockPosition::new(margin_edge_top),
                PageTopBlockPosition::new(self.cursor_y),
                applied_start_margin,
            ));
            let bfc_clearance_applied = bfc_clearance.clearance.is_introduced();
            if bfc_clearance_applied {
                self.cursor_y = bfc_clearance.used_border_edge.points();
            }
            if bfc_clearance.fragmentainer_progress.advanced() {
                self.mark_current_page_flow_content();
            }
            if self.containing_block_writing_mode == WritingMode::HorizontalTb
                && geometry.style.writing_mode == WritingMode::HorizontalTb
            {
                let context = self
                    .float_contexts
                    .last()
                    .expect("root float context exists")
                    .clone();
                let page_index = self.current_float_page_index();
                if context.has_css_float_on_page(page_index) {
                    let clear = if bfc_clearance_applied {
                        Clear::None
                    } else {
                        geometry.style.clear
                    };
                    let writing_mode = geometry.style.writing_mode;
                    let direction = geometry.style.used_direction();
                    let containing_inline_span =
                        PageInlineSpan::from_edges(containing_left, containing_right);
                    let normal_border_top = self.cursor_y;
                    let mut solve_placement = |top| {
                        context.avoiding_bfc_root_position(
                            page_index,
                            top,
                            clear,
                            writing_mode,
                            direction,
                            containing_left,
                            containing_right,
                            |band, _candidate_top| {
                                self.measure_float_avoiding_bfc(
                                    element,
                                    style,
                                    stylesheets,
                                    child_boxes,
                                    containing_inline_size,
                                    containing_inline_span,
                                    band,
                                )
                            },
                        )
                    };
                    // A positive adjoining start margin can cross an active float
                    // before it puts the BFC root's border box below that float.
                    // Test the margin edge first.  When the root cannot occupy
                    // that band, retain only the portion of the margin needed to
                    // reach the float's block-end edge, just as clearance does.
                    // If it fits beside the float, the normal margin placement is
                    // retained and solved at the border edge below.
                    // <https://www.w3.org/TR/CSS22/visuren.html#floats>
                    let margin_edge_placement = (applied_start_margin.points() > FLOAT_EPSILON
                        && !bfc_clearance_applied)
                        .then(|| solve_placement(PageTopBlockPosition::new(margin_edge_top)));
                    let placement = match margin_edge_placement {
                        Some(placement)
                            if placement.placement.origin.top_y()
                                < margin_edge_top - FLOAT_EPSILON
                                && placement.placement.origin.top_y()
                                    > normal_border_top + FLOAT_EPSILON =>
                        {
                            placement
                        }
                        _ => solve_placement(PageTopBlockPosition::new(normal_border_top)),
                    };
                    self.cursor_y = placement.placement.origin.top_y();
                    let available_span = placement.placement.available_span;
                    let containing_inline_span =
                        PageInlineSpan::from_edges(containing_left, containing_right);
                    let auto_border_box_width = (available_span.width()
                        < containing_inline_size - FLOAT_EPSILON)
                        .then_some(float_avoiding_auto_border_box_width(
                            available_span,
                            containing_inline_span,
                            style.margin.left,
                            style.margin.right,
                        ));
                    let constrained_auto_width = auto_border_box_width.is_some();
                    geometry = self.block_layout_geometry_in_inline_span(
                        element,
                        style,
                        stylesheets,
                        child_boxes,
                        BlockLayoutInlineConstraint {
                            // The residual band controls auto-width sizing, but it
                            // is not the block's CSS containing span. Resolve the
                            // final width and percentage-dependent geometry from
                            // the original containing block, then apply the
                            // measured candidate's border-box origin below.
                            containing_inline_span,
                            percentage_basis: PercentageBasis::definite(
                                LogicalInlineContentSize::new(content_box_pt(
                                    containing_inline_size,
                                )),
                            ),
                            physical_width_percentage_basis: PhysicalContentWidth::new(
                                content_box_pt(containing_inline_size),
                            ),
                            auto_border_box_width,
                        },
                    );
                    if constrained_auto_width {
                        geometry.reanchor_float_avoiding_border_box(
                            placement.candidate.normal_flow_border_box_inline_span,
                        );
                    }
                }
            } else {
                // In a vertical principal flow, float avoidance advances the
                // physical block-axis slab. Reuse the logical exclusion query
                // used by table, grid, flex, and replaced BFC roots, then
                // project its selected physical origin into this block's
                // pre-layout border-box geometry.
                // <https://www.w3.org/TR/CSS22/visuren.html#floats>
                // <https://www.w3.org/TR/css-writing-modes-4/#logical-to-physical>
                let border_box_width = geometry.outer_inline().width().points();
                let estimated_outer_height = self
                    .estimate_element_height(
                        element,
                        &geometry.style,
                        stylesheets,
                        geometry.outer_inline().width().points(),
                        child_boxes,
                    )
                    .unwrap_or(
                        geometry.style.margin.top
                            + geometry.style.line_height
                            + geometry.style.margin.bottom,
                    );
                let placement = self.place_float_avoiding_margin_box(
                    PageTopBlockPosition::new(self.cursor_y),
                    margin_box_size_pt(
                        geometry.style.margin.left + border_box_width + geometry.style.margin.right,
                        estimated_outer_height,
                    ),
                    geometry.style.clear,
                    geometry.style.writing_mode,
                    geometry.style.used_direction(),
                    self.containing_block_direction,
                );
                // A `vertical-rl` root with a short enough physical inline
                // extent fits in the inline track below an inline-start
                // (physical-top) float. It should move down that inline axis,
                // rather than needlessly creating the next block-axis slab.
                // An over-tall root still uses the slab selected above.
                // <https://www.w3.org/TR/css-writing-modes-4/#block-flow>
                let inline_fit_below_top_float = (self.containing_block_writing_mode
                    == WritingMode::VerticalRl)
                    .then(|| {
                        self.float_contexts
                            .last()
                            .expect("root float context exists")
                            .shapes
                            .iter()
                            .filter(|shape| {
                                shape.is_css_float()
                                    && shape.page_index == self.current_float_page_index()
                                    && shape.side == UsedFloatSide::Top
                            })
                            .map(|shape| shape.physical_margin_box().bottom_y())
                            .filter(|bottom| {
                                estimated_outer_height
                                    <= *bottom - self.page_bottom() + FLOAT_EPSILON
                            })
                            .min_by(f32::total_cmp)
                    })
                    .flatten();
                self.cursor_y = inline_fit_below_top_float.unwrap_or(placement.origin.top_y());
                geometry.reanchor_float_avoiding_border_box(PageInlineSpan::new(
                    placement.origin.x() + geometry.style.margin.left,
                    border_box_width,
                ));
            }
        }
        let selected_orthogonal_inline_layout = geometry.selected_orthogonal_inline_layout.take();
        let style = &geometry.style;
        let height_depends_on_intrinsic_content =
            needs_intrinsic_height_contribution(style.box_values.height.value().clone())
                || needs_intrinsic_height_contribution(style.box_values.min_height.clone())
                || needs_intrinsic_height_contribution(style.box_values.max_height.clone());
        let block_line_trim = self.effective_text_box_line_trim_for_style(style);
        let relative_offset = geometry.relative_offset;
        // Relative positioning shifts the box's painted position and its
        // descendants, but it does not change the box's normal-flow position.
        // Resolve margins, `clear`, and BFC float avoidance first; applying a
        // relative block offset before clearance would let clearance cancel a
        // negative `top` offset and incorrectly change following flow.
        // <https://www.w3.org/TR/CSS22/visuren.html#relative-positioning>
        if matches!(style.position, Position::Relative | Position::Sticky)
            && style.clear != Clear::None
        {
            self.cursor_y += relative_offset.y();
        }
        let border_edges = geometry.border_edges;
        let border_widths = border_edges.to_css_edges();
        let vertical_non_content = geometry.vertical_non_content;
        let principal_fragment_decoration =
            FragmentDecoration::for_box_decoration_break(style.box_decoration_break, false, false);
        let principal_fragment_decoration_reservation = FragmentDecorationReservation::new(
            principal_fragment_decoration,
            non_content_pt(border_widths.top + style.padding.top),
            non_content_pt(style.padding.bottom + border_widths.bottom),
        );
        let vertical_extras = vertical_non_content.points();
        let containing_block_content_height = geometry.containing_block_content_height;
        let containing_block_height_basis = containing_block_content_height;
        let definite_content_height_for_children = geometry.definite_content_height;
        let definite_content_height =
            definite_content_height_for_children.map(|height| height.value().points());
        // Multicolumn sizing consumes the used block constraint before it
        // chooses a local column set. Resolve `lh` here, at the block's used
        // line-height boundary, so a finite `height: 2lh` does not degrade
        // into an auto-height balanced row.
        let mut multicol_used_height = style.box_values.height.clone();
        multicol_used_height.resolve_line_height_relative_lengths(layout_pt(style.line_height));
        let multicol_content_height =
            definite_content_height.or_else(|| multicol_used_height.length_if_no_percent());
        let outer_inline = geometry.outer_inline();
        let content_inline = geometry.content_inline();
        let content_width = content_inline.width().points();
        let mut content_logical_inline_size = geometry.content_logical_inline_size().points();
        if element.tag.eq_ignore_ascii_case("body")
            && self.principal_flow.is_source_body(element)
            && style.writing_mode == WritingMode::VerticalRl
        {
            // A propagated body is the initial canvas. Its visual inline-end
            // margin offsets painting, but does not shorten the inline span
            // available to its canvas descendants.
            // <https://www.w3.org/TR/css-writing-modes-4/#principal-flow>
            content_logical_inline_size +=
                match inline_start_side(style.writing_mode, style.used_direction()) {
                    PhysicalSide::Top => style.margin.bottom,
                    PhysicalSide::Bottom => style.margin.top,
                    PhysicalSide::Left | PhysicalSide::Right => {
                        unreachable!("a vertical writing mode must have a vertical inline axis")
                    }
                };
        }
        let built_multicol_child_boxes;
        let child_boxes = if child_boxes.is_none()
            && (matches!(style.column_count, css::ColumnCount::Count(_))
                || matches!(style.column_width, css::ComputedColumnWidth::Length(_))
                || matches!(style.column_height, css::ComputedColumnHeight::Length(_)))
        {
            built_multicol_child_boxes = self.build_frozen_child_boxes_with_current_ancestors(
                element,
                stylesheets,
                source_style,
            );
            Some(built_multicol_child_boxes.as_slice())
        } else {
            child_boxes
        };
        let outer_x = outer_inline.span().left_x();
        let inner_x = content_inline.span().left_x();
        let inner_width = content_inline.width().points();
        if self.principal_flow.is_source_body(element)
            && inline_start_side(style.writing_mode, style.used_direction()) == PhysicalSide::Bottom
        {
            self.principal_inline_end_inset = style.margin.bottom;
        }
        if self.principal_flow.is_source_body(element)
            && self.principal_flow.writing_mode == WritingMode::HorizontalTb
        {
            self.principal_body_block_end_inset = layout_pt(style.margin.right);
        }
        // The propagated body is the first box entered from the document's
        // used principal flow. A bottom-origin vertical principal flow cannot
        // inherit the legacy page-top cursor at that boundary: its first
        // child must start at physical page bottom before ordinary decreasing
        // Y block layout converts that inline progress into page coordinates.
        // Later descendants retain their own containing-block cursor.
        // <https://drafts.csswg.org/css-writing-modes-4/#principal-flow>
        // <https://drafts.csswg.org/css-writing-modes-4/#logical-to-physical>
        if self.principal_flow.is_source_body(element)
            && inline_start_side(style.writing_mode, style.used_direction()) == PhysicalSide::Bottom
        {
            self.cursor_y = self.page_bottom();
        }
        // <https://www.w3.org/TR/css-writing-modes-4/#logical-to-physical>
        let block_top = self.cursor_y;
        let propagated_vertical_body_canvas = self.principal_flow.is_source_body(element)
            && WritingModeAxes::new(style.writing_mode, style.used_direction())
                .swaps_physical_axes();
        if propagated_vertical_body_canvas
            && self.root_principal_flow_context.active_canvas.is_none()
        {
            self.root_principal_flow_context.active_canvas = Some(ActiveDocumentCanvas {
                body: Some(element.id),
                inline_end_inset: layout_pt(
                    match inline_start_side(style.writing_mode, style.used_direction()) {
                        PhysicalSide::Top => style.margin.bottom,
                        PhysicalSide::Bottom => style.margin.top,
                        PhysicalSide::Left | PhysicalSide::Right => {
                            unreachable!("a vertical writing mode must have a vertical inline axis")
                        }
                    },
                ),
                inline_origin: PageTopBlockPosition::new(block_top),
                block_track_occupancy: layout_pt(
                    (self.current_page_context.right() - self.page_left()).max(0.0),
                ),
                trailing_child_block_margin: layout_pt(0.0),
            });
        }
        let mut fragmented_definite_block = false;
        let mut vertical_root_fragmented_block = false;
        let mut vertical_root_page_fragmentation = None;
        let principal_block_size_disposition = definite_content_height.map_or(
            PrincipalBlockSizeDisposition::ContentSized,
            |content_height| {
                PrincipalBlockSizeDisposition::Fixed(
                    FixedPrincipalBlockSize::from_resolved_content_height(
                        PhysicalContentHeight::new(content_box_pt(content_height)),
                        style.box_sizing,
                        vertical_non_content,
                    ),
                )
            },
        );
        debug_assert!(match principal_block_size_disposition {
            PrincipalBlockSizeDisposition::ContentSized => true,
            PrincipalBlockSizeDisposition::Fixed(size) => match size.specified_box() {
                FixedPrincipalBlockSpecifiedBox::ContentBox => true,
                FixedPrincipalBlockSpecifiedBox::BorderBox(border_box_height) => {
                    border_box_height.points() + 0.01 >= size.content_height().points()
                }
            },
        });
        // In a vertical writing mode, physical `height` is the logical inline
        // size. Keep the already-resolved principal-box disposition distinct
        // from selected text occupancy: a line sequence can size an automatic
        // box but cannot replace this definite used inline size.
        // <https://www.w3.org/TR/css-writing-modes-4/#dimension-mapping>
        let vertical_inline_size_is_definite = style.writing_mode.has_vertical_lines()
            && principal_block_size_disposition
                .fixed_content_height()
                .is_some();
        let positioning_containing_block_mode =
            PositionedContainingBlockMode::for_element(element, style);
        let paint_checkpoint = self.current_page.paint_checkpoint();
        let paint_page_index = self.pages.len();
        let positioned_layer_start = self.positioned_layers.len();
        let pending_paint_fragment_start = self.pending_paint_fragments.len();
        let pending_positioned_page_span_target_at_start = self
            .pending_positioned_fragmentation
            .materialized_destination_end();
        let static_scroll_snap_scope =
            self.begin_static_scroll_snap_scope(style, element.tag.eq_ignore_ascii_case("html"));
        let block_start_page_context = self.current_page_context;
        let block_start_page_index = self.pages.len();
        self.cursor_y -= border_widths.top + style.padding.top;
        let content_top = self.cursor_y;
        let first_fragment_start = self.current_page_context.top() - content_top;
        let decoration_reservation = FragmentDecorationReservation::new(
            FragmentDecoration::for_box_decoration_break(style.box_decoration_break, false, false),
            non_content_pt(border_widths.top + style.padding.top),
            non_content_pt(style.padding.bottom + border_widths.bottom),
        );
        // The generic page-flow cursor is physical top-to-bottom. Vertical
        // root fragmentation has a separate logical projection and must not
        // inherit these physical continuation insets.
        let fragment_top_offset = if style.writing_mode == WritingMode::HorizontalTb
            && style.box_decoration_break == css::BoxDecorationBreak::Clone
        {
            FragmentTopOffset::cloned_block_decoration(
                first_fragment_start,
                decoration_reservation.block_start().points(),
                decoration_reservation.block_end().points(),
            )
        } else {
            FragmentTopOffset::unreserved(first_fragment_start)
        };
        self.fragment_top_offsets.push(fragment_top_offset);
        self.add_bookmark(element, style, paint_space_point(inner_x, block_top));
        self.add_page_anchor(element, style);
        let descendant_bookmark_start = self.bookmarks.len();

        let previous_left = self.content_left;
        let previous_right = self.content_right;
        let previous_containing_block_direction = self.containing_block_direction;
        let previous_containing_block_writing_mode = self.containing_block_writing_mode;
        let is_vertical_orthogonal_flow = style.writing_mode.has_vertical_lines()
            && writing_modes_are_orthogonal(
                previous_containing_block_writing_mode,
                style.writing_mode,
            );
        let is_vertical_parallel_flow_auto_inline_size = style.writing_mode.has_vertical_lines()
            && !writing_modes_are_orthogonal(
                previous_containing_block_writing_mode,
                style.writing_mode,
            )
            && style.box_values.height.is_auto()
            && !needs_intrinsic_height_contribution(style.box_values.min_height.clone())
            && !needs_intrinsic_height_contribution(style.box_values.max_height.clone());
        self.content_left = inner_x;
        self.content_right = inner_x + inner_width;
        // A contained root or body keeps its ordinary principal box, but it
        // does not supply the document canvas.  Canvas-only geometry (the
        // fragment insets consumed by descendants and viewport overflow) must
        // follow the used propagation result rather than the element name.
        // <https://drafts.csswg.org/css-contain-1/#containment-layout>
        if is_document_canvas {
            self.document_canvas_fragment_insets.push(FragmentOffsets {
                left: inner_x - self.current_page_context.left(),
                right: self.current_page_context.right() - (inner_x + inner_width),
                top: self.current_page_context.top() - content_top,
            });
        }
        // A containing block exports its used inline base direction. In
        // vertical typographic mode, `text-orientation: upright` forces this
        // value to LTR without changing the computed value inherited by an
        // orthogonal descendant.
        // <https://drafts.csswg.org/css-writing-modes-4/#text-orientation>
        self.containing_block_direction = style.used_direction();
        self.containing_block_writing_mode = style.writing_mode;
        self.content_logical_inline_size_stack
            .push(content_logical_inline_size);
        let parent_child_available_space = self.current_child_available_space();
        let inherited_orthogonal_available_height =
            parent_child_available_space.orthogonal_available_height;
        let mut child_available_space = child_available_space_for_block(
            style,
            PhysicalContentWidth::new(content_inline.width()),
            definite_content_height_for_children,
            inherited_orthogonal_available_height,
            self.initial_containing_block_physical_height(),
        );
        if style.writing_mode.has_vertical_lines() && style.box_values.width.is_auto() {
            // The used physical width of an auto-sized vertical block may be
            // content-derived, but that does not make it a percentage basis
            // for an orthogonal child: resolving the child from that value
            // would create the sizing cycle described by CSS Writing Modes.
            // Export the containing block's available physical-width basis
            // instead. This is normal orthogonal-flow behavior, not a
            // document-canvas exception; an unpropagated body still obtains
            // its basis from its actual containing block through this path.
            // <https://www.w3.org/TR/css-writing-modes-4/#orthogonal-flows>
            child_available_space = child_available_space
                .with_orthogonal_physical_width_percentage_basis(
                    parent_child_available_space.orthogonal_physical_width_percentage_basis,
                );
        }
        self.child_available_space_stack.push(child_available_space);
        let container_unit_scope = self.push_container_unit_context(
            style,
            PhysicalContentWidth::new(content_inline.width()),
            PhysicalContentHeight::new(content_box_pt(definite_content_height.unwrap_or(0.0))),
        );
        if establishes_independent_bfc {
            self.push_float_context();
        }
        let positioned_containing_block_scope =
            if let Some(mode) = positioning_containing_block_mode {
                // Absolute descendants resolve percentage block sizes against the
                // positioned ancestor's final padding box. An auto-height block
                // gets that height from its in-flow children, which have not yet
                // been laid out at this point. Estimate the same normal-flow
                // contribution before entering the positioned-descendant scope so
                // those descendants do not observe the line-height placeholder.
                // The ordinary child pass remains authoritative for final flow
                // geometry and fragmentation.
                // <https://www.w3.org/TR/css-position-3/#def-cb>
                let positioning_content_height = definite_content_height.unwrap_or_else(|| {
                    (self.estimate_block_like_height(
                        element,
                        &geometry.style,
                        stylesheets,
                        content_width,
                        None,
                    ) - geometry.style.margin.top
                        - geometry.style.margin.bottom
                        - vertical_extras)
                        .max(0.0)
                });
                let containing_block = ContainingBlock::from_page_top_rect(
                    geometry.padding_box_top_rect(block_top, positioning_content_height),
                );
                Some(self.push_positioned_containing_block(mode, containing_block))
            } else {
                None
            };
        let overflow_clip_content_height = (!height_depends_on_intrinsic_content)
            .then(|| {
                used_content_box_height_or_auto(
                    style,
                    layout_pt(self.page_area_height()),
                    vertical_non_content,
                )
                .map(SemanticLengthExt::points)
            })
            .flatten()
            .map(|height| {
                constrain_content_height(
                    style,
                    content_box_pt(height),
                    PercentageBasis::definite(layout_pt(content_width)),
                )
                .points()
            });
        let used_overflow_clips = self.element_used_overflow_clips(element, style);
        // A deferred effect is required when the used height is intrinsic,
        // when paint containment supplies the clip, or when a longhand axis
        // creates the scroll container independently of the legacy shorthand
        // field. The retained effect owns all visual-overflow clipping: a
        // primitive clip would mutate descendant source geometry before a
        // later CSS transform is applied.
        // <https://www.w3.org/TR/css-overflow-3/#overflow-clip-edge>
        // <https://www.w3.org/TR/css-contain-1/#containment-paint>
        let paint_containment_applies = paint_containment_applies_to_element(element, style);
        // A rectangular primitive clip would discard the corners before the
        // final paint effect can apply their border-radius and `corner-shape`
        // contour. Keep those descendants intact until that typed contour is
        // emitted as one effect scope.
        // <https://drafts.csswg.org/css-backgrounds-3/#corner-clipping> and
        // <https://drafts.csswg.org/css-borders-4/#corner-shaping>.
        let has_single_border_shape = match &style.border_shape {
            css::BorderShape::None | css::BorderShape::Pair { .. } => false,
            css::BorderShape::Circle(_)
            | css::BorderShape::Ellipse(_)
            | css::BorderShape::Path(_)
            | css::BorderShape::Inset(_)
            | css::BorderShape::Polygon(_) => true,
        };
        let needs_contoured_overflow_clip = used_overflow_clips
            && (!style.border_radius.clone().is_zero() || has_single_border_shape);
        let needs_deferred_overflow_clip = used_overflow_clips
            && (self.active_fragmentainer_kind() != FragmentainerKind::Column
                || definite_content_height.is_some()
                || paint_containment_applies
                || needs_contoured_overflow_clip);
        // The normal deferred path preserves an element-level PDF scope until
        // its used block size is known. When one padding-box dimension is
        // already known to be empty, though, no descendant can survive that
        // scope. Keep the active layout clip as well so the public paint
        // projection culls descendants immediately (and so no inherited
        // overflow clip is accidentally popped).
        // <https://www.w3.org/TR/css-overflow-3/#overflow-clipping>
        let has_empty_known_overflow_clip =
            content_width + style.padding.left + style.padding.right <= 0.0
                || overflow_clip_content_height
                    .is_some_and(|height| height + style.padding.top + style.padding.bottom <= 0.0);
        let overflow_clip_active = if used_overflow_clips
            && style_clips_overflow(style)
            && !paint_containment_applies
            && (!needs_deferred_overflow_clip || has_empty_known_overflow_clip)
        // The deferred effect scopes one element-level clip around the
        // completed paint fragment. Keeping an eager clip as well turns
        // it into a synthetic clip for each text line, which can cut ink
        // that lies inside the element's real overflow clip edge.
        {
            let clip_content_height = overflow_clip_content_height.unwrap_or_else(|| {
                (block_top
                    - border_widths.top
                    - style.padding.top
                    - style.padding.bottom
                    - self.page_bottom())
                .max(0.0)
            });
            let used_overflow = UsedOverflowAxes::from_style(style);
            let (clip_edge_x, clip_edge_y) = overflow_clip_edge_axes(style);
            let (clip_x, clip_y) = overflow_clipping_axes(style);
            let margin = if clip_edge_x || clip_edge_y {
                style.overflow_clip_margin.length
            } else {
                0.0
            };
            let clip_height = clip_content_height + style.padding.top + style.padding.bottom;
            self.push_overflow_clip(OverflowClip::from_paint_rect_with_axes_and_non_scrollable(
                PageTopRect::new(
                    outer_x + border_widths.left - margin,
                    block_top - border_widths.top + margin,
                    content_width + style.padding.left + style.padding.right + margin * 2.0,
                    clip_height + margin * 2.0,
                )
                .paint_rect(),
                clip_x,
                clip_y,
                used_overflow.non_scrollable_clip_x(),
                used_overflow.non_scrollable_clip_y(),
            ));
            true
        } else {
            false
        };
        let has_single_unbreakable_inline_line = fragmentainer_kind == FragmentainerKind::Column
            && content_top - self.page_bottom() <= css::CSS_PX_TO_PT + 0.01
            && child_boxes.is_some_and(|children| {
                self.block_has_single_unbreakable_inline_line(
                    element,
                    style,
                    children,
                    content_width,
                )
            });
        let propagated_viewport_clip = self
            .document_canvas_overflow
            .is_viewport_overflow_source(element)
            && self
                .document_canvas_overflow
                .viewport_clips_block_fragmentation();
        let suppresses_descendant_fragmentation = (used_overflow_clips
            && definite_content_height.is_some())
            || has_single_unbreakable_inline_line
            || (containment.size && fragmentainer_kind == FragmentainerKind::Column)
            || propagated_viewport_clip;
        if suppresses_descendant_fragmentation {
            // Scroll containers with a definite size, an unbreakable line in
            // a zero-height column, and size-contained column subjects
            // establish monolithic outer-flow boxes. An auto-height overflow
            // box instead fragments with its contents, so the used block size
            // can be established from all of its fragments.
            // <https://www.w3.org/TR/css-contain-1/#containment-size>
            // <https://www.w3.org/TR/css-break-3/#monolithic>
            self.fragmentation_suppression_depth += 1;
        }

        // Capture the static-position containing block before any child work.
        // Initial inline layout can encounter an out-of-flow descendant before
        // the later block-child traversal begins. Its `justify-self:auto`
        // must resolve from this block's non-inherited `justify-items`, not
        // from an outer block whose mutable inline span happens to be active.
        // `align-items` does not apply to hypothetical block-level children.
        // <https://www.w3.org/TR/css-position-3/#static-position>
        // <https://drafts.csswg.org/css-align-3/#justify-items-property>
        let static_content_height = if style.writing_mode.has_vertical_lines() {
            // `height` is the logical inline size of a vertical block. The
            // static-position rectangle for a block child must span that
            // physical vertical inline axis, even though the legacy
            // `definite_content_height` path is a block-axis sizing input.
            // An auto-sized vertical parent can defer its own used inline
            // extent until after child layout; its static rectangle still
            // uses the definite available inline span of its containing
            // block. Without this fallback an RTL vertical source becomes a
            // zero-height alignment container and places an abspos child's
            // physical top at the wrong inline edge.
            // <https://drafts.csswg.org/css-position-3/#staticpos-rect>
            // <https://www.w3.org/TR/css-writing-modes-4/#abstract-box>
            let own_inline_span = if is_vertical_parallel_flow_auto_inline_size {
                (content_logical_inline_size - vertical_extras).max(0.0)
            } else {
                content_logical_inline_size
            };
            own_inline_span.max(
                containing_block_content_height
                    .points()
                    .map(|height| (height - vertical_extras).max(0.0))
                    .unwrap_or(0.0)
                    .max(
                        self.content_logical_inline_size_stack
                            .last()
                            .copied()
                            .map(|height| (height - vertical_extras).max(0.0))
                            .unwrap_or(0.0),
                    ),
            )
        } else {
            definite_content_height.unwrap_or_else(|| {
                containing_block_content_height
                    .points()
                    .map(|height| (height - vertical_extras).max(0.0))
                    .unwrap_or(0.0)
            })
        };
        self.static_position_containing_blocks
            .push(StaticPositionContainingBlock::new(
                WritingModeAxes::new(style.writing_mode, style.used_direction()),
                PageTopRect::new(
                    self.content_left,
                    content_top,
                    (self.content_right - self.content_left).max(0.0),
                    static_content_height,
                ),
                style.justify_items,
            ));

        let list_marker =
            self.marker_for_list_item(element, style, previous_containing_block_direction);
        let has_pending_outside_marker_anchor = self.begin_outside_marker_anchor(
            list_marker.as_ref(),
            style,
            PageInlineSpan::from_edges(self.content_left, self.content_right),
        );

        // Generated pseudo-elements are tree-abiding boxes whose `content`
        // is evaluated by the pseudo-content path. They must never fall back
        // to the originating element's DOM children merely because an atomic
        // inline/float replay did not carry a frozen child list.
        // <https://www.w3.org/TR/css-pseudo-4/#generated-content>
        let has_generated_content = style.content.is_generated();
        // A normalized child stream already owns its inline breaks. Inspecting
        // the raw DOM as well would resurrect a `<br>` whose computed display
        // was suppressed during box-tree construction (notably Appendix B's
        // `display: contents` → `none` rule for HTML controls).
        // <https://drafts.csswg.org/css-display-3/#unbox-html>
        let has_explicit_line_break = !has_generated_content
            && child_boxes.is_none()
            && element_has_direct_line_break(element);
        let root_has_generated_content = element.tag.eq_ignore_ascii_case("html")
            && style
                .before_style
                .iter()
                .chain(style.after_style.iter())
                .any(|pseudo| pseudo.content.is_generated());
        let use_ordered_mixed_flow = !has_generated_content
            // Root pseudos are tree-abiding children. The ordered DOM path
            // has no pseudo source entries, so it would drop a block-level
            // `html::before` before the propagated principal-flow child
            // traversal can lay it out.
            // <https://www.w3.org/TR/css-pseudo-4/#generated-content>
            && !root_has_generated_content
            && !requires_block_in_inline_normalization
            && !requires_run_in_normalization
            && (((child_boxes.is_none() || child_boxes.is_some_and(has_non_inline_formatting_box))
                && has_ordered_mixed_flow_content_with_font_metrics(
                    element,
                    style,
                    stylesheets,
                    &self.ancestors,
                    &mut self.font_system,
                ))
                // Formatting-tree normalization stores direct `<br>` nodes
                // outside the block-child sequence. When a block also has
                // normal-flow children, replaying all direct inline content
                // before that sequence changes DOM order; retain the ordered
                // mixed-flow path instead.
                // <https://html.spec.whatwg.org/multipage/text-level-semantics.html#the-br-element>
                || (has_explicit_line_break
                    && child_boxes.is_some_and(has_non_inline_formatting_box)));
        let has_normalized_flow_children = !has_generated_content
            && child_boxes
                .map(has_non_inline_formatting_box)
                .unwrap_or(false);
        let has_direct_dom_flow_child = !has_generated_content
            && child_boxes.is_none()
            && has_direct_flow_child_with_font_metrics(
                element,
                style,
                stylesheets,
                &mut self.font_system,
            );
        let use_box_inline_items = !use_ordered_mixed_flow
            && !has_generated_content
            && !has_normalized_flow_children
            && child_boxes
                .map(|boxes| {
                    formatting_box_has_inline_content(boxes)
                        && boxes
                            .iter()
                            .any(|box_| !formatting_box_can_only_create_phantom_line_boxes(box_))
                })
                .unwrap_or(false);
        let has_run_in_inline_content = !run_in_children.is_empty();

        // If normalization consumed a run-in source's children, do not replay
        // its original DOM text here. Inline pseudo content that survives in
        // normalized inline boxes is handled through `use_box_inline_items`.
        let normalized_children_empty = child_boxes.is_some_and(|boxes| boxes.is_empty());
        let detached_normalized_text = normalized_children_empty
            && !has_generated_content
            && !inline_text_for_style(element, style).is_empty();
        let text = if normalized_children_empty
            || has_generated_content
            || use_ordered_mixed_flow
            || has_normalized_flow_children
            || use_box_inline_items
        {
            String::new()
        } else if is_document_canvas {
            own_inline_text_for_style(element, style)
        } else if has_direct_dom_flow_child {
            // The DOM fallback lays direct block-flow children in its later
            // child traversal. Their descendant text must not be flattened
            // into a synthetic leading anonymous line in this parent.
            // <https://www.w3.org/TR/CSS22/visuren.html#block-formatting>
            own_inline_text_for_style(element, style)
        } else {
            inline_text_for_style(element, style)
        };
        // Frozen root children already contain tree-abiding `::after` boxes.
        // Do not also collect the pseudo from the root style in the leading
        // inline run; that run precedes the propagated body canvas and would
        // both duplicate the pseudo and reverse its DOM order.
        // <https://www.w3.org/TR/css-pseudo-4/#generated-content>
        let defer_root_after_pseudo = element.tag.eq_ignore_ascii_case("html")
            && child_boxes.is_some()
            && style
                .after_style
                .as_deref()
                .is_some_and(|after| after.content.is_generated());
        let mut leading_inline_style;
        let inline_style = if defer_root_after_pseudo {
            leading_inline_style = style.clone();
            leading_inline_style.after_style = None;
            &leading_inline_style
        } else {
            style
        };
        let has_generated_inline_content = !detached_normalized_text
            && ((has_generated_content && generated_content_has_non_phantom_inline_content(style))
                || generated_content_has_non_phantom_inline_content(inline_style)
                || (child_boxes.is_none()
                    && (inline_style.before_style.is_some()
                        || inline_style.after_style.is_some())));
        // Once normalization has exposed block-flow children, its anonymous
        // inline runs own the descendant inline source. Recollecting the raw
        // DOM solely because it has a styled inline descendant would replay
        // text on the parent before the normalized sequence.
        let has_styled_inline_descendant = !has_normalized_flow_children
            && has_styled_inline_descendant_with_font_metrics(
                element,
                style,
                stylesheets,
                &self.ancestors,
                &mut self.font_system,
            );
        // The canvas box has no own text run.  When its direct children are
        // exclusively ordinary inline boxes, collect that one source stream
        // instead of dropping it while preserving all mixed/block fallback
        // paths for more complex canvas children.
        let has_plain_document_canvas_inline_stream = is_document_canvas
            && child_boxes.is_none()
            && has_only_direct_in_flow_inline_dom_content_with_font_metrics(
                element,
                style,
                stylesheets,
                &self.ancestors,
                &mut self.font_system,
            );
        // A `<br>` creates a line box even when the surrounding text has
        // collapsed away. Treat it as collectable inline content so its
        // forced boundaries are laid out—and fragmented—through the shared
        // line-record path rather than being discarded as empty text.
        // <https://html.spec.whatwg.org/multipage/text-level-semantics.html#the-br-element>
        let has_collectable_inline_content = inline_text_has_non_phantom_content(&text, style)
            || has_generated_inline_content
            || has_explicit_line_break
            || has_styled_inline_descendant
            || has_plain_document_canvas_inline_stream;
        let use_inline_items = has_collectable_inline_content
            && (has_plain_document_canvas_inline_stream
                || has_styled_inline_descendant
                || has_generated_inline_content
                || plain_inline_content_needs_inline_items(&text, style)
                || has_explicit_line_break
                || style.text_align.justifies()
                || self.active_float_exclusions_at(PageBlockSpan::new(
                    self.cursor_y,
                    style.line_height,
                )));
        let run_in_inline_items_laid_out =
            has_run_in_inline_content && !has_normalized_flow_children && child_boxes.is_some();
        // An inline-only multicolumn pass owns both its anonymous column
        // fragments and their gap decorations. Do not subsequently route the
        // same source through the generic block-column path, which would
        // infer a second, incompatible set of occupied columns from the
        // post-layout cursor.
        // <https://www.w3.org/TR/css-multicol-1/#the-multi-column-model>
        let mut laid_out_inline_multicol = false;
        let mut preceding_inline_local_cutoff = false;
        let mut preceding_inline_clamp_block_advance = crate::units::content_box_pt(0.0);
        // Retain a committed simple vertical inline sequence when available.
        // Vertical auto-size geometry consumes that exact layout rather than
        // reconstructing the same text after selecting its inline measure.
        let mut committed_vertical_inline_sequence = None;
        if run_in_inline_items_laid_out {
            let child_boxes = child_boxes.expect("run-in layout requires frozen target children");
            self.layout_run_in_inline_items_block(
                element,
                style,
                stylesheets,
                run_in_children,
                child_boxes,
                element.attrs.get("href").map(String::as_str),
                list_marker.as_ref(),
            );
        // Ordered mixed-flow traversal owns every inline run, including a
        // standalone `<br>` between floated or block-level siblings. Laying
        // the parent inline items here as well would collect the complete DOM
        // subtree and place those floats a second time before the ordered
        // traversal reaches their source positions.
        // <https://www.w3.org/TR/CSS22/visuren.html#anonymous-block-level>
        } else if has_collectable_inline_content && !use_ordered_mixed_flow {
            let pushed_text_box_trim = self.push_text_box_line_trim_scope(block_line_trim);
            if use_inline_items {
                let laid_out_multicol_inline_items = self.layout_multicol_inline_items_block(
                    element,
                    style,
                    stylesheets,
                    None,
                    (0.0, 0.0),
                    element.attrs.get("href").map(String::as_str),
                    list_marker.as_ref(),
                    multicol_content_height,
                );
                laid_out_inline_multicol = laid_out_multicol_inline_items;
                if !laid_out_multicol_inline_items {
                    committed_vertical_inline_sequence =
                        if let Some(selected) = selected_orthogonal_inline_layout.as_ref() {
                            debug_assert_eq!(
                                content_logical_inline_size,
                                selected.logical_inline_measure.points(),
                                "orthogonal auto geometry must paint at its selected inline measure"
                            );
                            self.paint_inline_line_sequence(&selected.line_sequence, inline_style);
                            self.layout_frozen_inline_replay_positioned_descendants(
                                &selected.frozen_replay_input,
                                stylesheets,
                            );
                            Some(selected.line_sequence.clone())
                        } else {
                            self.layout_inline_items_block(
                                element,
                                inline_style,
                                stylesheets,
                                // The frozen inline stream is also the source used
                                // for this block's intrinsic geometry. Recollecting
                                // the DOM here can flatten an inline-block into
                                // styled text edges, losing its line-box block-size
                                // contribution after the orthogonal auto-size pass
                                // selected a width from that contribution.
                                // <https://www.w3.org/TR/css-inline-3/#line-boxes>
                                // <https://www.w3.org/TR/css-writing-modes-4/#orthogonal-flows>
                                child_boxes,
                                (0.0, 0.0),
                                element.attrs.get("href").map(String::as_str),
                                list_marker.as_ref(),
                            )
                        };
                    preceding_inline_local_cutoff = committed_vertical_inline_sequence
                        .as_ref()
                        .is_some_and(|sequence| sequence.has_local_continuation_cutoff);
                    preceding_inline_clamp_block_advance = committed_vertical_inline_sequence
                        .as_ref()
                        .map(|sequence| sequence.layout_outcome().clamp_block_advance)
                        .unwrap_or_else(|| crate::units::content_box_pt(0.0));
                }
            } else if style.display.is_list_item() {
                self.layout_list_text_block(
                    &text,
                    inline_style,
                    0.0,
                    0.0,
                    element.attrs.get("href").map(String::as_str),
                    list_marker.as_ref(),
                );
            } else {
                let laid_out_multicol_text = self.layout_multicol_text_block(
                    &text,
                    inline_style,
                    0.0,
                    0.0,
                    element.attrs.get("href").map(String::as_str),
                    multicol_content_height,
                );
                laid_out_inline_multicol = laid_out_multicol_text;
                if !laid_out_multicol_text {
                    let outcome = self.layout_text_block(
                        &text,
                        style,
                        0.0,
                        0.0,
                        element.attrs.get("href").map(String::as_str),
                    );
                    preceding_inline_local_cutoff = outcome.has_local_continuation_cutoff;
                    preceding_inline_clamp_block_advance = outcome.clamp_block_advance;
                }
            }
            self.pop_text_box_line_trim_scope(pushed_text_box_trim);
        }
        let mut box_inline_has_flow_effects = false;
        let mut laid_out_box_inline_multicol = false;
        if !has_run_in_inline_content
            && use_box_inline_items
            && !(has_collectable_inline_content && use_inline_items)
            && let Some(child_boxes) = child_boxes
        {
            let pushed_text_box_trim = self.push_text_box_line_trim_scope(block_line_trim);
            // Collected text, forced-break boxes, and atomic inline fragments
            // go directly through the shared multicol line sequence. Atomic
            // fragments retain their owned paint/positioning state in the
            // sequence, so an ordinary anonymous-block pass here would paint
            // them once before the column planner paints them again.
            // <https://www.w3.org/TR/css-multicol-1/#the-multi-column-model>
            laid_out_box_inline_multicol = list_marker.is_none()
                && self.layout_multicol_inline_items_block(
                    element,
                    style,
                    stylesheets,
                    has_atomic_inline_formatting_box(child_boxes).then_some(child_boxes),
                    (0.0, 0.0),
                    element.attrs.get("href").map(String::as_str),
                    None,
                    multicol_content_height,
                );
            if !laid_out_box_inline_multicol {
                box_inline_has_flow_effects = self.layout_anonymous_block(
                    style,
                    child_boxes,
                    stylesheets,
                    list_marker.as_ref(),
                );
            }
            self.pop_text_box_line_trim_scope(pushed_text_box_trim);
        }
        // A list item's marker belongs to its principal flow, not to a
        // particular anonymous column set.  For an inside marker followed by
        // a spanner, the frozen child sequence starts with an anonymous
        // block (or directly with the spanner), so the column planner has no
        // root-level inline formatter in which to create that first marker
        // line. Commit it once before the segmented flow; outside markers
        // retain their deferred first-line anchor below.
        // <https://drafts.csswg.org/css-lists-3/#list-style-position-property>
        let laid_out_inside_multicol_marker = !laid_out_inline_multicol
            && !laid_out_box_inline_multicol
            && text.is_empty()
            && style_establishes_multicol_formatting_context(style)
            && list_marker
                .as_ref()
                .is_some_and(ListMarker::participates_in_first_line)
            && child_boxes.is_some_and(formatting_boxes_have_eligible_multicol_spanner);
        if laid_out_inside_multicol_marker {
            let pushed_text_box_trim = self.push_text_box_line_trim_scope(block_line_trim);
            self.layout_list_text_block(
                "",
                style,
                0.0,
                0.0,
                element.attrs.get("href").map(String::as_str),
                list_marker.as_ref(),
            );
            self.pop_text_box_line_trim_scope(pushed_text_box_trim);
        }
        let laid_out_column_children = laid_out_inline_multicol
            || laid_out_box_inline_multicol
            || (!laid_out_inline_multicol
                && text.is_empty()
                && (self.layout_definition_list_columns(element, style, stylesheets, child_boxes)
                    || self
                        .layout_simple_block_child_columns(
                            element,
                            // Column planning creates its own used style;
                            // begin from the frozen cascade source rather
                            // than the block's already-zoomed geometry.
                            source_style,
                            stylesheets,
                            child_boxes,
                            multicol_content_height,
                        )
                        .is_multicol_layout()));
        if laid_out_column_children
            && let Some(marker) = list_marker.as_ref()
            && marker.paints_outside()
            && !self.outside_marker_anchor_is_pending(marker)
        {
            // A multicol list item still owns one marker at the principal
            // box's block start. The column planner consumes the block
            // children, so paint that marker here rather than falling through
            // to the non-column empty-content fallback below.
            // <https://www.w3.org/TR/css-lists-3/#marker-position>
            let fallback_baseline_offset = self.inline_box_text_line_layout_baseline_offset(style);
            self.paint_outside_marker(
                marker,
                style,
                OutsideMarkerAnchor {
                    principal_line_inline_span: PageInlineSpan::from_edges(
                        self.content_left,
                        self.content_right,
                    ),
                    formatted_line_block_start: PageTopBlockPosition::new(content_top),
                    alphabetic_baseline: PageTopBlockPosition::new(content_top)
                        .toward_block_end(layout_pt(fallback_baseline_offset)),
                },
            );
        }
        let has_inside_marker_line_content = list_marker.as_ref().is_some_and(|marker| {
            marker.participates_in_first_line() && marker.has_in_flow_content()
        });
        let has_direct_inline_content = has_run_in_inline_content
            || box_inline_has_flow_effects
            || child_boxes.is_some_and(has_direct_inline_content_box)
            || has_collectable_inline_content
            || laid_out_column_children
            // The marker is generated inline-level content of the principal
            // list-item box. It prevents the principal box from being
            // self-collapsing even when the DOM contributes only collapsible
            // whitespace.
            // <https://drafts.csswg.org/css-lists-3/#marker-layout>
            || has_inside_marker_line_content;
        if style.writing_mode != WritingMode::HorizontalTb
            && has_direct_inline_content
            && !vertical_inline_size_is_definite
        {
            let vertical_inline_height = if use_box_inline_items
                && let Some(marker) = list_marker.as_ref()
                && marker.participates_in_first_line()
            {
                child_boxes
                    .map(|child_boxes| {
                        self.intrinsic_inline_measurement_for_boxes_with_marker(
                            child_boxes,
                            style,
                            marker,
                            stylesheets,
                            content_logical_inline_size,
                        )
                        .physical_height(style)
                    })
                    .unwrap_or(0.0)
            } else if let Some(sequence) = committed_vertical_inline_sequence.as_ref() {
                sequence.occupied_physical_inline_extent(style).points()
            } else if let Some(marker) = list_marker.as_ref()
                && marker.participates_in_first_line()
                && !text.is_empty()
            {
                // In vertical writing, the physical height consumed by an
                // inside marker is part of the line's inline-axis advance.
                // Measuring only the principal text makes successive list
                // items overlap and ignores marker font-size changes.
                // <https://drafts.csswg.org/css-writing-modes-4/#vertical-layout>
                // <https://drafts.csswg.org/css-lists-3/#marker-position>
                let mut items = Vec::new();
                self.push_inside_marker_items(marker, style, None, &mut items);
                self.push_inline_words(
                    &text,
                    style,
                    None,
                    0.0,
                    InlineVisualOffset::zero(),
                    &mut items,
                );
                self.intrinsic_inline_measurement_for_items(
                    items,
                    style,
                    content_logical_inline_size,
                )
                .physical_height(style)
            } else if use_box_inline_items {
                child_boxes
                    .map(|child_boxes| {
                        self.intrinsic_inline_measurement_for_boxes(
                            child_boxes,
                            style,
                            stylesheets,
                            content_logical_inline_size,
                        )
                        .physical_height(style)
                    })
                    .unwrap_or(0.0)
            } else if !text.is_empty() {
                // This fallback still has styled inline descendants in the
                // DOM-backed collector.  Measuring its flattened text under
                // the block style would discard an inner atomic inline's
                // effective layout footprint: in particular a horizontal
                // text-combine-upright composition must contribute its one-em
                // square, not its uncompressed horizontal advance.
                //
                // Use the same styled item collection and intrinsic graph as
                // line layout, which forms TCY before measuring its atomic
                // parent participant.
                // <https://drafts.csswg.org/css-writing-modes-4/#text-combine-layout>
                // <https://drafts.csswg.org/css-sizing-3/#intrinsic>
                self.intrinsic_inline_measurement_for_element(
                    element,
                    style,
                    stylesheets,
                    child_boxes,
                    content_logical_inline_size,
                )
                .physical_height(style)
            } else {
                0.0
            };
            if vertical_inline_height > 0.0 {
                self.cursor_y = self.cursor_y.min(content_top - vertical_inline_height);
            }
        }
        if let Some(marker) = list_marker.as_ref()
            && !inline_text_has_non_phantom_content(&text, inline_style)
            && !has_collectable_inline_content
            && !use_box_inline_items
            && !laid_out_column_children
        {
            if marker.paints_outside() {
                if self.cursor_y - style.font_size < self.page_bottom() {
                    self.push_page();
                }
                if !has_pending_outside_marker_anchor {
                    let anchor = self.outside_marker_fallback_anchor(
                        style,
                        PageInlineSpan::from_edges(self.content_left, self.content_right),
                    );
                    self.paint_outside_marker(marker, style, anchor);
                }
            } else if marker.has_in_flow_content() {
                // An inside marker is inline-level content. It therefore
                // establishes the principal line box even when the only DOM
                // text was collapsible whitespace (as in an empty HTML
                // `<li>` formatted across source lines).
                // <https://drafts.csswg.org/css-lists-3/#list-style-position-property>
                // <https://drafts.csswg.org/css-inline-3/#line-box>
                let pushed_text_box_trim = self.push_text_box_line_trim_scope(block_line_trim);
                self.layout_list_text_block("", style, 0.0, 0.0, None, Some(marker));
                self.pop_text_box_line_trim_scope(pushed_text_box_trim);
            }
        }
        let can_collapse_start_margin = can_adjoin_first_child_margin
            && can_collapse_block_start_margin(
                element,
                style,
                border_edges,
                child_boxes.map_or_else(
                    || {
                        has_direct_inline_content_before_first_flow_child_dom_with_font_metrics(
                            element,
                            style,
                            stylesheets,
                            &self.ancestors,
                            &mut self.font_system,
                        )
                    },
                    has_direct_inline_content_box,
                ),
                self.used_overflow_for_element(element, style),
            );
        let can_collapse_end_margin = can_collapse_block_end_margin(
            element,
            style,
            geometry.containing_block_content_height,
            border_edges,
            has_direct_inline_content,
            self.used_overflow_for_element(element, style),
        );
        let mut margin_collapse_style = None;
        if height_behaves_as_auto_for_margin_collapse(
            style,
            geometry.containing_block_content_height,
        ) {
            let mut used_style = style.clone();
            used_style
                .box_values
                .height
                .replace_with_used(css::ComputedLengthPercentageOrAuto::Auto);
            margin_collapse_style = Some(used_style);
        }
        let margin_collapse_style = margin_collapse_style.as_ref().unwrap_or(style);
        let self_collapsing_block = !has_direct_inline_content
            && if let Some(child_boxes) = child_boxes {
                is_self_collapsing_block_box(
                    element,
                    margin_collapse_style,
                    child_boxes,
                    self.document_canvas_overflow,
                )
            } else {
                is_self_collapsing_block_dom_with_font_metrics(
                    element,
                    margin_collapse_style,
                    stylesheets,
                    &self.ancestors,
                    &mut self.font_system,
                    self.document_canvas_overflow,
                )
            };
        // Relative positioning percentages resolve against the normal-flow
        // containing block even when that block does not establish an
        // absolute-positioning containing block. Keep this scope separate
        // from `containing_blocks`, which intentionally tracks only the
        // positioned-containing-block chain.
        // <https://www.w3.org/TR/css-position-3/#relative-positioning>.
        self.normal_flow_relative_containing_blocks
            .push(NormalFlowRelativeContainingBlock {
                physical_content_width: PhysicalContentWidth::new(content_box_pt(content_width)),
                physical_content_height: definite_content_height
                    .map(|height| PhysicalContentHeight::new(content_box_pt(height))),
            });
        let children_outcome =
            self.layout_block_flow_children_phase(Box::new(BlockFlowChildrenPhaseInput {
                fragmentainer_kind,
                element,
                style,
                stylesheets,
                child_boxes,
                can_collapse_start_margin,
                can_collapse_end_margin,
                start_margin_arrangement,
                starts_at_page_top,
                laid_out_column_children,
                use_box_inline_items,
                run_in_inline_items_laid_out,
                use_ordered_mixed_flow,
                has_preceding_inline_flow_content: has_collectable_inline_content
                    && !use_ordered_mixed_flow,
                preceding_inline_local_cutoff,
                preceding_inline_clamp_block_advance,
                discard_region_limit: None,
                direct_automatic_block_size_constraint:
                    super::children::state::direct_automatic_block_size_constraint(style),
                definite_content_height: definite_content_height_for_children,
                descendant_percentage_height_basis,
            }));
        self.normal_flow_relative_containing_blocks.pop();
        self.static_position_containing_blocks.pop();
        self.definite_block_size_stack.pop();
        if has_pending_outside_marker_anchor {
            self.finish_outside_marker_anchor();
        }
        let pending_end_margin_collapse = children_outcome.pending_end_margin_collapse;
        let collapsed_start_margin_offset = children_outcome.collapsed_start_margin_offset;
        let margin_collapse_boundary = match start_margin_arrangement.margin_collapse_boundary() {
            BlockMarginCollapseBoundary::Adjoining => {
                children_outcome.adjoining_margin_set_boundary
            }
            BlockMarginCollapseBoundary::SeparatedByClearance => {
                BlockMarginCollapseBoundary::SeparatedByClearance
            }
        };
        // Keep the rendered legend's source-fragment geometry at the parent
        // decoration boundary.  The next decoration pass consumes it to
        // exclude only the fieldset border, never descendant backgrounds.
        // <https://html.spec.whatwg.org/multipage/rendering.html#the-fieldset-and-legend-elements>
        let rendered_legend = children_outcome.rendered_legend;
        let inline_capture = self.finish_clamp_line_slot_capture();
        let clamp_line_slots =
            inline_capture.line_slots + children_outcome.descendant_clamp_line_slots;
        let has_local_continuation_cutoff = inline_capture.has_local_continuation_cutoff
            || children_outcome.has_local_continuation_cutoff;
        let in_flow_child_fragment_end =
            (self.pages.len() > paint_page_index).then_some(InFlowFragmentEnd {
                page_index: self.pages.len(),
                cursor: PageTopBlockPosition::new(self.cursor_y),
            });
        if suppresses_descendant_fragmentation {
            self.fragmentation_suppression_depth -= 1;
        }

        let mut independent_bfc_had_float_content = false;
        if establishes_independent_bfc
            && has_auto_height(style)
            && let Some((float_page_index, float_bottom)) =
                self.current_float_context_last_fragment_end()
        {
            independent_bfc_had_float_content = true;
            while self.pages.len() < float_page_index {
                // Floats are out of flow, so their paint alone does not make
                // this anonymous column non-empty. The auto-height BFC itself
                // nevertheless has normal-flow geometry through every column
                // required to contain those floats; preserve that structural
                // occupancy before advancing, or `push_page` will replace the
                // empty column instead of materializing the continuation.
                // <https://www.w3.org/TR/CSS22/visudet.html#root-height>
                // <https://www.w3.org/TR/css-break-3/#fragmentation-model>
                if !self.current_page_has_content() {
                    self.mark_current_page_flow_content();
                }
                self.push_page();
            }
            self.cursor_y = self.cursor_y.min(float_bottom.points());
        }
        if establishes_independent_bfc {
            self.pop_float_context();
        }
        if let Some(scope) = positioned_containing_block_scope {
            self.pop_positioned_containing_block(scope);
        }
        self.pop_overflow_clip(overflow_clip_active);
        self.pop_container_unit_context(container_unit_scope);
        self.child_available_space_stack.pop();
        self.content_logical_inline_size_stack.pop();
        if propagated_vertical_body_canvas {
            let block_end_inset = match block_start_side(style.writing_mode) {
                PhysicalSide::Left => style.margin.right,
                PhysicalSide::Right => style.margin.left,
                PhysicalSide::Top | PhysicalSide::Bottom => {
                    unreachable!("a vertical document canvas has a horizontal block axis")
                }
            };
            if let Some(active_canvas) = self.root_principal_flow_context.active_canvas.take() {
                debug_assert_eq!(active_canvas.body, Some(element.id));
                self.root_principal_flow_context.completed_canvas = Some(CompletedDocumentCanvas {
                    body: active_canvas.body,
                    // A fragmented body completes on its final fragmentainer,
                    // not necessarily the page on which its canvas began.
                    // Root content following the canvas must therefore make
                    // its placement decision against this committed source
                    // fragment.
                    source_page: DocumentPageIndex::new(self.pages.len()),
                    source_block_track: PageInlineSpan::from_edges(
                        self.content_left,
                        self.content_right,
                    ),
                    inline_origin: active_canvas.inline_origin,
                    inline_end_inset: active_canvas.inline_end_inset,
                    block_end_inset: layout_pt(block_end_inset),
                    block_track_occupancy: active_canvas.block_track_occupancy,
                    trailing_child_block_margin: active_canvas.trailing_child_block_margin,
                });
            }
        }
        self.restore_page_area_parent_context_after_page_transition(
            previous_left,
            previous_right,
            block_start_page_context,
            block_start_page_index,
        );
        if is_document_canvas {
            self.document_canvas_fragment_insets.pop();
        }
        self.containing_block_direction = previous_containing_block_direction;
        self.containing_block_writing_mode = previous_containing_block_writing_mode;

        if self_collapsing_block
            && !independent_bfc_had_float_content
            && self.pages.len() == paint_page_index
            && margin_collapse_boundary == BlockMarginCollapseBoundary::Adjoining
            // Adjoining block margins require no block-axis border or
            // padding.  Retaining a nonzero vertical edge is essential even
            // for an otherwise empty positioned body: that edge is the
            // principal decoration of its stacking context.
            // <https://www.w3.org/TR/CSS22/box.html#collapsing-margins>
            && vertical_extras == 0.0
        {
            self.cursor_y = content_top;
        }

        let mut block_end_margin_to_consume = style.margin.bottom;
        if let Some(pending) = pending_end_margin_collapse {
            let content_height_with_child_margin = content_top - self.cursor_y;
            let content_height_without_child_margin =
                content_height_with_child_margin - pending.child_consumed_margin.points();
            self.cursor_y += pending.child_consumed_margin.points();
            if block_end_margin_collapse_survives_height_constraints(
                style,
                PhysicalContentWidth::new(content_inline.width()),
                vertical_non_content,
                PhysicalContentHeight::new(content_box_pt(content_height_without_child_margin)),
            ) {
                block_end_margin_to_consume = pending.collapsed_margin.points();
            }
        }

        if is_vertical_orthogonal_flow && !vertical_inline_size_is_definite {
            // Orthogonal auto sizing has selected the vertical box's used
            // logical inline content size through fit-content negotiation.
            // That used size controls its own box geometry, even though the
            // fallback that selected it remains an indefinite percentage
            // basis for descendants.
            // <https://www.w3.org/TR/css-writing-modes-4/#orthogonal-auto>
            self.cursor_y = content_top - content_logical_inline_size;
        } else if is_vertical_parallel_flow_auto_inline_size && self.cursor_y >= content_top - 0.01
        {
            // In vertical writing, physical height is logical inline size. A
            // normal-flow block with an automatic inline size fills its
            // containing block's available inline size, just as an automatic
            // physical width does in horizontal block flow. The inline layout
            // pass selected that content-box size, so retain it rather than
            // collapsing to the height contributed by inline contents.
            // <https://www.w3.org/TR/css-writing-modes-4/#dimension-mapping>
            self.cursor_y = content_top - content_logical_inline_size;
        } else if definite_content_height.is_some()
            || used_min_height(style, containing_block_height_basis).is_some()
            || used_max_height(style, containing_block_height_basis).is_some()
            || style.box_values.min_height == css::ComputedLengthPercentageOrAuto::Stretch
            || style.box_values.max_height == css::ComputedLengthPercentageOrAuto::Stretch
            || height_depends_on_intrinsic_content
            || intrinsic_physical_height_is_contained(style)
        {
            // Size containment fixes the principal box's used size as if it
            // had no content. Descendants are still laid out and painted in
            // place, but their overflow does not move following siblings.
            // <https://www.w3.org/TR/css-contain-1/#containment-size>
            let current_content_height = if intrinsic_physical_height_is_contained(style) {
                style
                    .contain_intrinsic_size
                    .height
                    .clone()
                    .map(|height| {
                        used_length_percentage(
                            height,
                            PercentageBasis::definite(layout_pt(content_width.max(0.0))),
                        )
                        .points()
                    })
                    .unwrap_or(0.0)
            } else {
                content_top - self.cursor_y
            };
            // Once child layout has crossed an outer fragmentainer, the
            // current cursor reports only the final fragment's local extent.
            // CSS sizing instead constrains the block's continuous automatic
            // height, before that source flow is split into fragmentainers.
            // <https://www.w3.org/TR/CSS2/visudet.html#min-max-heights>
            // <https://www.w3.org/TR/css-break-3/#parallel-flows>
            let measured_content_height = if self.pages.len() > paint_page_index {
                promoted_spanner_paint_slices(
                    paint_page_index,
                    self.pages.len(),
                    content_top,
                    self.cursor_y,
                    block_start_page_context,
                    self.current_page_context,
                    self.fragmentainer_override,
                )
                .iter()
                .map(|slice| (slice.top - slice.bottom).max(0.0))
                .sum()
            } else {
                current_content_height
            };
            // An intrinsic min/max constraint is resolved only after in-flow
            // content has been measured, but it must constrain the automatic
            // preferred height supplied by aspect-ratio rather than replace
            // it with that content height. This lets calc-size(auto, …) use
            // the content-derived automatic minimum independently from the
            // ratio-derived preferred size.
            // <https://www.w3.org/TR/css-sizing-4/#aspect-ratio> and
            // <https://drafts.csswg.org/css-values-5/#calc-size>.
            let aspect_ratio_preferred_height = style
                .box_values
                .height
                .is_auto()
                .then(|| {
                    non_replaced_aspect_ratio_content_height(
                        style,
                        content_width,
                        border_widths.left
                            + border_widths.right
                            + style.padding.left
                            + style.padding.right,
                        vertical_extras,
                    )
                })
                .flatten();
            let mut requested_content_height = definite_content_height
                .or(aspect_ratio_preferred_height)
                .unwrap_or_else(|| {
                    used_content_box_height_or_auto_with_basis(
                        style,
                        containing_block_content_height,
                        vertical_non_content,
                    )
                    .map(SemanticLengthExt::points)
                    .unwrap_or(measured_content_height)
                });
            // A preferred `calc-size()` block size substitutes its automatic
            // basis only after in-flow content has been measured. At this
            // point normal-flow layout has that content contribution, so
            // retain CSS Math bounds such as `min(size, 100px)` instead of
            // treating the preferred size as ordinary `auto`.
            // <https://drafts.csswg.org/css-values-5/#calc-size>.
            if definite_content_height.is_none()
                && let css::ComputedLengthPercentageOrAuto::CalcSize(value) =
                    &*style.box_values.height
            {
                requested_content_height = calc_size_intrinsic_constraint(
                    value.clone(),
                    style.box_sizing,
                    PercentageBasis::definite(content_box_pt(content_width)),
                    vertical_non_content,
                    content_box_pt(measured_content_height),
                    content_box_pt(measured_content_height),
                )
                .map(SemanticLengthExt::points)
                .unwrap_or(requested_content_height);
            }
            // A preferred aspect ratio supplies the automatic preferred block
            // size, but it does not discard the content-based automatic
            // minimum of an ordinary flow box. The final used height must
            // therefore accommodate in-flow content when `min-height:auto`.
            // <https://drafts.csswg.org/css-sizing-4/#aspect-ratio>
            if style.box_values.height.is_auto()
                && style
                    .aspect_ratio
                    .preferred_ratio_for_non_replaced(false)
                    .is_some()
                && style.box_values.min_height.is_auto()
                && !style.overflow_y.is_scrollable()
                && !intrinsic_physical_height_is_contained(style)
            {
                requested_content_height = requested_content_height.max(measured_content_height);
            }
            let height = if height_depends_on_intrinsic_content {
                constrain_height_with_intrinsic(
                    style,
                    content_box_pt(requested_content_height),
                    content_box_pt(current_content_height),
                    content_box_pt(current_content_height),
                    containing_block_height_basis,
                    non_content_pt(vertical_extras),
                )
                .points()
            } else {
                containing_block_content_height.points().map_or_else(
                    || {
                        constrain_content_height(
                            style,
                            content_box_pt(requested_content_height),
                            containing_block_height_basis,
                        )
                        .points()
                    },
                    |basis| {
                        constrain_height_with_stretch_fit(
                            style,
                            content_box_pt(requested_content_height),
                            layout_pt(basis),
                            layout_pt(style.margin.top + style.margin.bottom),
                            vertical_non_content,
                        )
                        .points()
                    },
                )
            };
            // A winning post-layout min/max constraint reruns CSS sizing with
            // a fixed used height. It changes the principal box's fragment
            // ownership but cannot become a percentage basis for descendants:
            // they were laid out against the original auto-height containing
            // block before this constraint was known.
            // <https://www.w3.org/TR/CSS2/visudet.html#min-max-heights>
            // <https://www.w3.org/TR/css-break-3/#parallel-flows>
            let post_layout_height_replay = definite_content_height
                .is_none()
                .then(|| {
                    post_layout_height_replay_constraint(
                        style,
                        containing_block_height_basis,
                        vertical_non_content,
                        content_box_pt(requested_content_height),
                    )
                })
                .flatten()
                .filter(|constraint| {
                    (requested_content_height
                        - constraint.content_box_length(vertical_non_content).points())
                    .abs()
                        > 0.01
                });
            if let Some(replay_height) = post_layout_height_replay {
                // The constrained used height must drive the complete second
                // layout pass. In particular, fragmentation has to assign
                // the principal box before its visible overflow reaches later
                // columns. Preserve the percentage basis selected before the
                // clamp: a post-layout constraint never makes descendants'
                // percentage heights definite retroactively.
                // <https://www.w3.org/TR/CSS2/visudet.html#min-max-heights>
                // <https://www.w3.org/TR/css-break-3/#parallel-flows>
                let mut constrained_style = style.clone();
                constrained_style
                    .box_values
                    .height
                    .replace_with_used(replay_height.as_used_height());
                self.restore(
                    post_layout_constraint_replay_snapshot
                        .expect("post-layout min/max constraint replay has a snapshot"),
                );
                self.layout_block_with_descendant_percentage_height_basis(
                    element,
                    &constrained_style,
                    stylesheets,
                    run_in_children,
                    child_boxes,
                    Some(preparatory_descendant_percentage_basis),
                    principal_box_paint_mode,
                );
                return;
            }
            if self.pages.len() == paint_page_index
                && style.writing_mode == WritingMode::HorizontalTb
            {
                let free_space = height - current_content_height;
                block_align_content_offset_y = if laid_out_column_children {
                    multicol_align_content_y_offset(style.align_content, free_space)
                } else {
                    block_align_content_y_offset_for_style(style, free_space)
                };
            }
            let definite_block_overflows_fragmentainer = self.fragmentation_suppression_depth == 0
                && style.writing_mode == WritingMode::HorizontalTb
                && !(containment.size && fragmentainer_kind == FragmentainerKind::Column)
                && !has_single_unbreakable_inline_line
                // Root/body overflow propagated to the viewport is clipped
                // by the viewport rather than fragmented into additional
                // document pages.
                // <https://drafts.csswg.org/css-overflow-3/#overflow-propagation>
                && !propagated_viewport_clip
                && height > content_top - self.page_bottom() + 0.01;
            if self.pages.len() > paint_page_index && definite_block_overflows_fragmentainer {
                // Descendant layout may already have crossed one or more
                // fragmentainers before the definite principal size is
                // applied (for example, a line starting at an exhausted page
                // edge or nested multicol rows). Consume only the unoccupied
                // remainder of the authored block size; reapplying the full
                // size double-counts those fragments, while using a page-local
                // subtraction drops the remaining continuous extent.
                // <https://www.w3.org/TR/css-break-3/#fragmentation-model>
                // <https://www.w3.org/TR/css-multicol-1/#pagination-and-overflow-outside-multicol>
                let consumed_content_height = promoted_spanner_paint_slices(
                    paint_page_index,
                    self.pages.len(),
                    content_top,
                    self.cursor_y,
                    block_start_page_context,
                    self.current_page_context,
                    self.fragmentainer_override,
                )
                .iter()
                .map(|slice| (slice.top - slice.bottom).max(0.0))
                .sum::<f32>();
                let remaining_content_height = (height - consumed_content_height).max(0.0);
                if remaining_content_height > 0.01 {
                    self.consume_definite_block_size_through_fragmentainers(
                        self.cursor_y,
                        remaining_content_height,
                    );
                }
                fragmented_definite_block = true;
            } else if self.pages.len() == paint_page_index
                && (fragments_as_promoted_spanner || definite_block_overflows_fragmentainer)
            {
                let vertical_fragmentation = if self.active_fragmentainer_kind()
                    == FragmentainerKind::Page
                    && style.writing_mode == self.principal_flow.writing_mode
                    && self.containing_block_writing_mode == self.principal_flow.writing_mode
                {
                    self.consume_vertical_root_page_block_size(
                        layout_pt(outer_inline.width().points()),
                        PageTopBlockPosition::new(block_top),
                    )
                } else {
                    None
                };
                if let Some(vertical_fragmentation) = vertical_fragmentation {
                    fragmented_definite_block = vertical_fragmentation.fragments.len() > 1;
                    vertical_root_fragmented_block = true;
                    vertical_root_page_fragmentation = Some(vertical_fragmentation);
                    suppress_own_principal_box_decoration = true;
                } else {
                    let PrincipalBlockSizeDisposition::Fixed(fixed_size) =
                        principal_block_size_disposition
                    else {
                        unreachable!("a definite block height owns a fixed principal size")
                    };
                    self.consume_fixed_principal_block_size_through_fragmentainers(
                        content_top,
                        fixed_size,
                        principal_fragment_decoration,
                        principal_fragment_decoration_reservation,
                    );
                    fragmented_definite_block = self.pages.len() > paint_page_index;
                }
            } else {
                self.cursor_y = content_top - height;
            }
        }
        self.fragment_top_offsets.pop();
        self.cursor_y -= style.padding.bottom + border_widths.bottom;
        let block_bottom = self.cursor_y;
        let fragmented_spanner_slices = if !vertical_root_fragmented_block
            && (fragments_as_promoted_spanner || fragmented_definite_block)
            && self.pages.len() > paint_page_index
        {
            promoted_spanner_paint_slices(
                paint_page_index,
                self.pages.len(),
                block_top,
                block_bottom,
                block_start_page_context,
                self.current_page_context,
                self.fragmentainer_override,
            )
        } else {
            Vec::new()
        };
        // An ordinary auto-sized block that crosses fragmentainers owns one
        // fragment box in every fragmentainer it reaches. Its decoration is
        // therefore painted per fragment, just like a definite promoted
        // spanner, rather than being attached only to the final page on which
        // used-size resolution happens. In particular, a forced column break
        // extends every continued fragment through the remaining column and
        // its background must cover that extent.
        // <https://www.w3.org/TR/css-break-3/#break-decoration>
        // <https://www.w3.org/TR/css-backgrounds-3/#box-decoration-break>
        // A fragmented float is a parallel fragmentation flow. Its
        // continuation must not manufacture principal-box fragments for an
        // ordinary ancestor: only normal-flow and independently positioned
        // descendant paint can extend this decoration span. BFC float
        // containment is handled separately when resolving the BFC's used
        // auto block size.
        // <https://drafts.csswg.org/css-break/#parallel-flows>
        // <https://www.w3.org/TR/css-backgrounds-3/#box-decoration-break>
        let out_of_flow_fragmentainer_end = self
            .pending_paint_fragments
            .get(pending_paint_fragment_start..)
            .unwrap_or_default()
            .iter()
            .filter(|fragment| fragment.kind != PendingPaintFragmentKind::FragmentedFloat)
            .map(|fragment| fragment.page_index)
            .chain(
                self.positioned_layers
                    .get(positioned_layer_start..)
                    .unwrap_or_default()
                    .iter()
                    .map(|layer| layer.page_index),
            )
            .chain(
                self.pending_positioned_fragmentation
                    .materialized_destination_end()
                    .filter(|target| {
                        pending_positioned_page_span_target_at_start
                            .is_none_or(|initial| *target > initial)
                    }),
            )
            .max();
        // A deferred out-of-flow placement can discover a static position
        // beyond the current column set while its in-flow containing block is
        // still committing the next fragment. That placement must not make
        // the auto-height ancestor manufacture further box fragments; its
        // immediate continuation is the committed decoration boundary.
        let block_fragmentainer_end = out_of_flow_fragmentainer_end
            .map_or(self.pages.len(), |end| {
                end.max(self.pages.len()).min(self.pages.len() + 1)
            });
        let out_of_flow_continues_block = block_fragmentainer_end > self.pages.len();
        let block_end_page_context = if out_of_flow_continues_block {
            self.fragmentainer_override
                .map(|override_| override_.context_for_fragmentainer(block_fragmentainer_end))
                .unwrap_or_else(|| self.resolved_page_context(block_fragmentainer_end + 1, false))
        } else {
            self.current_page_context
        };
        // A parallel descendant flow may continue after this block's own
        // principal box has ended.  That continuation must not extend the
        // block's decoration range: it is an occupied destination containing
        // descendant overflow, not another fragment of this box.  Retain the
        // actual principal block end here and let the pending descendant
        // paint carry its own later-fragmentainer assignment.
        //
        // CSS Fragmentation defines parallel flows independently, and
        // `box-decoration-break: clone` applies only to box fragments—not to
        // every fragmentainer that happens to contain descendant ink.
        // <https://www.w3.org/TR/css-break-3/#parallel-flows>
        // <https://www.w3.org/TR/css-break-3/#break-decoration>
        let decoration_block_bottom = block_bottom;
        let fragmented_block_slices = if !vertical_root_fragmented_block
            && fragmented_spanner_slices.is_empty()
            && block_fragmentainer_end > paint_page_index
            && (principal_block_size_disposition
                .fixed_content_height()
                .is_none()
                || fragmented_definite_block)
        {
            promoted_spanner_paint_slices(
                paint_page_index,
                block_fragmentainer_end,
                block_top,
                decoration_block_bottom,
                block_start_page_context,
                block_end_page_context,
                self.fragmentainer_override,
            )
        } else {
            Vec::new()
        };
        let block_height = if fragmented_spanner_slices.is_empty() {
            (block_top - block_bottom).max(0.0)
        } else {
            fragmented_spanner_slices
                .iter()
                .map(|slice| (slice.top - slice.bottom).max(0.0))
                .sum()
        };
        let paint_block_top = block_top - collapsed_start_margin_offset.points();
        let paint_block_height = (block_height - collapsed_start_margin_offset.points()).max(0.0);
        let border_box = geometry.border_box_top_rect(paint_block_top, paint_block_height);
        let border_paint_rect = border_box.page_top_rect().paint_rect();
        let vertical_root_border_paint_rect = vertical_root_page_fragmentation.as_ref().map(|_| {
            // In vertical writing, the legacy block-layout rectangle's
            // physical height follows its physical `height` property. The
            // principal box's logical inline span is instead the used
            // vertical extent selected before its horizontal block-axis
            // fragments are assigned. Reconstruct that one physical source
            // rectangle at the Writing Modes boundary before projection.
            // <https://www.w3.org/TR/css-writing-modes-4/#dimension-mapping>
            paint_space_rect(
                outer_x,
                self.current_page_context.top() - (content_logical_inline_size + vertical_extras),
                outer_inline.width().points(),
                content_logical_inline_size + vertical_extras,
            )
        });
        let vertical_root_fragmentation_handled = vertical_root_page_fragmentation.is_some();
        let rendered_legend_border_exclusion = element
            .tag
            .eq_ignore_ascii_case("fieldset")
            .then_some(rendered_legend)
            .flatten()
            .and_then(|legend| legend.border_exclusion(border_paint_rect, paint_page_index));
        self.record_static_scroll_snap_area(element, style, border_paint_rect);
        self.record_static_scroll_target_area(element.is_target, border_paint_rect, style);
        // CSS Break permits an oversized monolithic box to be sliced when it
        // cannot fit a fragmentainer. Keep decoration-only size-contained
        // boxes intact (matching replaced elements), while allowing boxes
        // with fragmentable contents to use Quire's contiguous slice path.
        // <https://www.w3.org/TR/css-break-3/#breaking-rules>
        let retain_size_contained_monolithic_paint = containment.size
            && (border_paint_rect.size.height <= self.page_area_height() + 0.01
                || crate::text::trim_css_collapsible_whitespace(&inline_text_for_style(
                    element, style,
                ))
                .is_empty());
        if element.tag.eq_ignore_ascii_case("html") {
            self.record_document_canvas_root_positioning_area(
                PaintBackgroundArea::from_paint_rect(border_paint_rect),
            );
            self.document_canvas_overflow.record_auto_overflow(
                border_paint_rect.size.width,
                border_paint_rect.size.height,
                self.current_page_context.area_width(),
                self.current_page_context.area_height(),
            );
        }
        // Auto-height overflow clips know their inline and block-start edges
        // before child layout, but the block-end edge is only available after
        // resolving the used height. CSS Overflow clips to the used padding box:
        // <https://www.w3.org/TR/css-overflow-3/#overflow-clip-edge>.
        let deferred_overflow_clip = needs_deferred_overflow_clip.then(|| {
            let clip_content_height = (block_height - vertical_extras).max(0.0);
            let (x, y, width, height) = match style.overflow_clip_margin.reference_box {
                css::OverflowClipMarginBox::Border => (
                    outer_x,
                    block_top,
                    content_width
                        + style.padding.left
                        + style.padding.right
                        + border_widths.left
                        + border_widths.right,
                    clip_content_height
                        + style.padding.top
                        + style.padding.bottom
                        + border_widths.top
                        + border_widths.bottom,
                ),
                css::OverflowClipMarginBox::Content => (
                    outer_x + border_widths.left + style.padding.left,
                    block_top - border_widths.top - style.padding.top,
                    content_width,
                    clip_content_height,
                ),
                css::OverflowClipMarginBox::Padding => (
                    outer_x + border_widths.left,
                    block_top - border_widths.top,
                    content_width + style.padding.left + style.padding.right,
                    clip_content_height + style.padding.top + style.padding.bottom,
                ),
            };
            let margin = if style.overflow_x == css::Overflow::Clip
                || style.overflow_y == css::Overflow::Clip
            {
                style.overflow_clip_margin.length
            } else {
                0.0
            };
            PageTopRect::new(
                x - margin,
                y + margin,
                width + margin * 2.0,
                height + margin * 2.0,
            )
            .paint_clip()
        });
        let used_overflow_axes = self.used_overflow_axes_for_element(element, style);
        let deferred_overflow_is_fully_bounded = paint_containment_applies
            || (used_overflow_axes.clips_x() && used_overflow_axes.clips_y());
        let deferred_axis_selective_overflow_clip = deferred_overflow_clip.and_then(|clip| {
            if paint_containment_applies {
                return None;
            }
            // A fully bounded clip retains the existing rounded/path effect
            // route. A single-axis CSS clip must keep its companion axis
            // unbounded through deferred paint.
            (!deferred_overflow_is_fully_bounded).then_some(AxisSelectivePaintClip::new(
                clip,
                used_overflow_axes.clips_x(),
                used_overflow_axes.clips_y(),
            ))
        });
        let deferred_box_content_contour = deferred_overflow_clip
            .filter(|_| deferred_overflow_is_fully_bounded)
            .and_then(|overflow_bounds| {
                let reference_box = match style.overflow_clip_margin.reference_box {
                    css::OverflowClipMarginBox::Border => css::BackgroundBox::Border,
                    css::OverflowClipMarginBox::Padding => css::BackgroundBox::Padding,
                    css::OverflowClipMarginBox::Content => css::BackgroundBox::Content,
                };
                let outset = if style.overflow_x == css::Overflow::Clip
                    || style.overflow_y == css::Overflow::Clip
                {
                    style.overflow_clip_margin.length
                } else {
                    0.0
                };
                resolve_box_content_contour(
                    border_paint_rect,
                    style,
                    border_widths,
                    BoxContentContourRequest::Overflow {
                        reference_box,
                        outset,
                    },
                )
                .map(|mut contour| {
                    // Fragmentation owns the used overflow extent. The
                    // contour resolver owns only its exact border edge;
                    // replacing this bound with an independently-derived
                    // box would lose a definite/fragment-local clip size.
                    contour.bounds = overflow_bounds;
                    contour
                })
            });
        let deferred_box_content_contour = deferred_box_content_contour
            .filter(|clip| !matches!(&clip.contour, BoxContentContour::Rect));
        if block_height > 0.0 {
            self.mark_current_page_flow_content();
        }
        // A definite principal box that fits its originating fragmentainer
        // keeps its decoration there even when visible descendant overflow
        // materializes later fragmentainers. Those later pages belong to the
        // descendant paint, not to fragments of the principal box.
        // <https://www.w3.org/TR/css-break-3/#fragmentation-model>
        let background_page_index = if principal_block_size_disposition
            .fixed_content_height()
            .is_some()
            && !fragmented_definite_block
            && !fragments_as_promoted_spanner
        {
            paint_page_index
        } else {
            self.pages.len()
        };
        let propagates_document_canvas_properties =
            self.element_propagates_document_canvas_properties(element, style);
        // The root element's border is part of the root stacking context's
        // background/border phase.  It must not be promoted with ordinary
        // block descendants, because negative positioned descendants paint
        // only after that root phase.
        // <https://www.w3.org/TR/CSS22/zindex.html>
        let is_root_element = element.tag.eq_ignore_ascii_case("html");
        let mut own_background_primitives = Vec::new();
        let mut own_outline_primitives = Vec::new();
        if propagates_document_canvas_properties && style.visibility == Visibility::Visible {
            self.capture_document_canvas_background(element, style);
        }
        // Only the source selected by CSS Backgrounds paints the document
        // canvas. A body whose background does not propagate remains ordinary
        // descendant paint and must be captured by an ancestor root transform.
        // <https://drafts.csswg.org/css-backgrounds-3/#special-backgrounds>
        // <https://drafts.csswg.org/css-transforms-1/#transform-rendering>
        let propagates_document_canvas_background = propagates_document_canvas_properties
            && self.element_paints_document_canvas_background(element);
        if propagates_document_canvas_background {
            // Capturing the root background creates the canvas-background
            // record. Preserve the used root box after that creation: a
            // position-fixed root can have a much smaller positioning area
            // than the canvas it paints through, and percentages in
            // `background-size` resolve against this box rather than the
            // page canvas.
            // <https://www.w3.org/TR/css-backgrounds-3/#special-backgrounds>
            // <https://www.w3.org/TR/css-backgrounds-3/#the-background-size>
            if element.tag.eq_ignore_ascii_case("html") {
                self.record_document_canvas_root_positioning_area(
                    PaintBackgroundArea::from_paint_rect(border_paint_rect),
                );
            }
            if !suppress_own_principal_box_decoration
                && (used_border_width(style) > layout_pt(0.0)
                    || style.border_image.source.is_image())
                && style.visibility == Visibility::Visible
            {
                // CSS Backgrounds propagates the root/body background to the
                // canvas, but borders are not canvas backgrounds; they remain
                // ordinary element border painting behind descendants:
                // <https://www.w3.org/TR/css-backgrounds-3/#special-backgrounds>.
                let mut border_style = style.clone();
                border_style.background.background_color = css::BackgroundColor::TRANSPARENT;
                border_style.background.background_image = css::ComputedImage::None;
                border_style.background.background_layers.clear();
                own_background_primitives =
                    self.box_background_primitives(border_paint_rect, &border_style);
            }
        } else if !suppress_own_principal_box_decoration
            && !vertical_root_fragmented_block
            && fragmented_spanner_slices.is_empty()
            && fragmented_block_slices.is_empty()
            && ((border_paint_rect.size.width > 0.0 && border_paint_rect.size.height > 0.0)
                || used_border_width(style) > layout_pt(0.0)
                || style.border_image.source.is_image())
            && (style.background.background_color.is_potentially_visible()
                || style.background.background_image.is_image()
                || style.border_image.source.is_image()
                || used_border_width(style) > layout_pt(0.0))
            && style.visibility == Visibility::Visible
        {
            // Once body-to-canvas propagation is disabled, the body is an
            // ordinary contained principal block. Its background therefore
            // uses its own border box; only the resolver-selected root/body
            // source can paint the page canvas.
            // <https://drafts.csswg.org/css-backgrounds-3/#special-backgrounds>
            if let Some(exclusion) = rendered_legend_border_exclusion.as_ref()
                && let Some((backgrounds, borders, deferred_images)) =
                    self.split_background_and_normal_border_primitives(border_paint_rect, style)
            {
                own_background_primitives = backgrounds;
                own_background_primitives.extend(
                    self.clip_rectangular_border_primitives(borders, &exclusion.visible_regions),
                );
                own_background_primitives.extend(deferred_images);
            } else {
                own_background_primitives =
                    self.box_background_primitives(border_paint_rect, style);
            }
        }
        if !suppress_own_principal_box_decoration
            && !vertical_root_fragmented_block
            && fragmented_spanner_slices.is_empty()
            && fragmented_block_slices.is_empty()
            && border_paint_rect.size.width > 0.0
            && border_paint_rect.size.height > 0.0
            && style.visibility == Visibility::Visible
        {
            own_outline_primitives = self.box_outline_primitives(border_paint_rect, style);
        }
        let has_own_background_primitives = !own_background_primitives.is_empty();
        let has_own_outline_primitives = !own_outline_primitives.is_empty();
        let scroll_content_bounds = self
            .current_page
            .paint_tree_fragment_since(&paint_checkpoint)
            .bounds()
            .map(PaintClip::paint_rect)
            .unwrap_or(border_paint_rect);
        let scroll_padding_box = deferred_overflow_clip
            .map(PaintClip::paint_rect)
            .unwrap_or(border_paint_rect);
        let static_scroll_offset = self.finish_static_scroll_snap_scope(
            static_scroll_snap_scope,
            scroll_padding_box,
            scroll_content_bounds,
        );
        let static_scroll_translation =
            crate::layout::scroll_snap::static_scroll_translation(static_scroll_offset, style);
        // Positioned descendants escape normal-flow paint capture, but remain
        // contents of this scroll container. Apply the same static scroll
        // translation and overflow clip before they are replayed into the
        // ancestor stacking context.
        // <https://www.w3.org/TR/css-overflow-3/#scrollable>
        if static_scroll_translation.x != 0.0 || static_scroll_translation.y != 0.0 {
            for layer in self
                .positioned_layers
                .get_mut(positioned_layer_start..)
                .unwrap_or_default()
            {
                *layer = layer.clone().translated(static_scroll_translation);
            }
        }
        if let Some(overflow_clip) = deferred_overflow_clip {
            let content_overflow_clip = match deferred_box_content_contour.as_ref() {
                Some(ResolvedBoxContentClip {
                    contour: BoxContentContour::Empty,
                    ..
                }) => PaintClip::new(overflow_clip.x(), overflow_clip.y(), 0.0, 0.0),
                _ => overflow_clip,
            };
            for layer in self
                .positioned_layers
                .get_mut(positioned_layer_start..)
                .unwrap_or_default()
            {
                // An escaped positioned layer still belongs to this scroll
                // container, but an overflow scope that cannot exclude any
                // of its recorded ink is not a paint-order boundary. In
                // particular, retaining it changes PDF edge coverage where
                // a later opaque in-flow background fully covers the layer.
                // <https://www.w3.org/TR/css-overflow-3/#overflow-clip-edge>
                // A rectangular-bounds containment check cannot prove that a
                // layer fits a curved `border-shape` contour.  Keep the
                // scope so positioned descendants are clipped at the shape,
                // even when they lie wholly inside its rectangular bounds.
                let layer_is_wholly_inside_clip = deferred_box_content_contour.is_none()
                    && layer
                        .context
                        .bounds
                        .is_some_and(|bounds| content_overflow_clip.contains(bounds));
                if layer_is_wholly_inside_clip {
                    continue;
                }
                let effective_bounds = layer
                    .context
                    .effects
                    .overflow_clip_bounds()
                    .and_then(|existing| existing.intersect(content_overflow_clip))
                    .unwrap_or(content_overflow_clip);
                layer
                    .context
                    .effects
                    .intersect_overflow_clip_bounds(content_overflow_clip);
                // Out-of-flow positioned descendants escape the ordinary
                // fragment capture, so carry the enclosing contour onto the
                // layer that will later be replayed into the ancestor stack.
                // The rectangular clip above remains the conservative bounds
                // for links and raster culling; this typed contour supplies
                // the visible inner edge.
                if let Some(mut contour) = deferred_box_content_contour.clone() {
                    contour.bounds = effective_bounds;
                    layer.context.effects.overflow_clip_effect = Some(
                        crate::document::paint::contours::OverflowClipEffect::Contoured(contour),
                    );
                }
            }
        }
        self.translate_aligned_block_descendant_bookmarks(
            descendant_bookmark_start,
            paint_page_index,
            0.0,
            block_align_content_offset_y,
        );
        if self.preserve_scoped_paint_public_order
            && !vertical_root_fragmentation_handled
            && self.pages.len() == paint_page_index
            && block_align_content_offset_y.abs() <= 0.01
            && !vertical_block_align_content_needs_fragment_bounds(style)
        {
            let mut fragment = self
                .current_page
                .paint_tree_fragment_since(&paint_checkpoint);
            // Descendant block backgrounds are captured in BackgroundBorder,
            // but that band is reserved for this box's own decoration by the
            // overflow helper. Promote descendants before creating the clip
            // scope so their paint remains part of the scrolling contents.
            fragment.promote_background_border_to_in_flow_block();
            fragment.promote_outline_to_in_flow_outline();
            if static_scroll_translation.x != 0.0 || static_scroll_translation.y != 0.0 {
                fragment = fragment.translated(static_scroll_translation);
            }
            if let Some(overflow_clip) = deferred_overflow_clip {
                fragment = if let Some(contour) = deferred_box_content_contour {
                    fragment.with_contents_effect_scoped_to_box_content_contour(contour)
                } else if paint_containment_applies {
                    fragment.with_paint_containment_contents_effect_scoped_to_rect(overflow_clip)
                } else if let Some(axis_selective_clip) = deferred_axis_selective_overflow_clip {
                    fragment.with_contents_effect_scoped_to_axis_selective_rect(axis_selective_clip)
                } else {
                    // Determine whether a descendant needs the effect from
                    // its untrimmed ink bounds.  Clipping primitives first
                    // makes overflowing paint appear contained and erases
                    // the PDF clip scope that must remain authoritative.
                    fragment.with_contents_effect_scoped_to_rect_preserving_contained_ink(
                        &self.current_page,
                        overflow_clip,
                    )
                };
            }
            if is_root_element {
                fragment.promote_background_border_to_in_flow_block();
                fragment.promote_outline_to_in_flow_outline();
            }
            if background_page_index == paint_page_index {
                self.current_page.prepend_recorded_primitives_to_fragment(
                    &mut fragment,
                    PaintBand::BackgroundBorder,
                    own_background_primitives,
                );
                self.current_page.append_recorded_primitives_to_fragment(
                    &mut fragment,
                    PaintBand::Outline,
                    own_outline_primitives,
                );
            }
            // A synthesized continuation has no descendant paint tree that
            // could promote this principal-box decoration later. Its
            // background therefore needs its resolved parent-level phase
            // immediately.
            if !is_root_element {
                fragment.promote_background_border_to_in_flow_block();
                fragment.promote_outline_to_in_flow_outline();
            }
            if retain_size_contained_monolithic_paint {
                fragment = fragment.with_monolithic_fragmentation_scope(
                    PaintClip::from_paint_rect(border_paint_rect),
                );
            }
            if ((has_own_background_primitives || has_own_outline_primitives)
                || deferred_overflow_clip.is_some())
                && !fragment.is_empty()
            {
                self.current_page
                    .replace_paint_tree_since_with_fragment(&paint_checkpoint, fragment);
            }
            self.cursor_y -= block_end_margin_to_consume;
            self.last_block_layout_outcome = BlockLayoutOutcome {
                consumed_bottom_margin: layout_pt(block_end_margin_to_consume),
                margin_collapse_boundary,
                physical_border_box_inline_span: outer_inline.width(),
                static_border_box: Some(border_paint_rect),
                clamp_line_slots,
                has_local_continuation_cutoff,
                in_flow_child_fragment_end,
            };
            if matches!(style.position, Position::Relative | Position::Sticky) {
                self.cursor_y -= relative_offset.y();
            }
            self.apply_forced_break_after_box_in(fragmentainer_kind, style);
            return;
        }
        let fragments =
            self.take_positioned_fragments_since(paint_page_index, paint_checkpoint.clone());
        let vertical_root_projected_fragments = if let Some(fragmentation) =
            vertical_root_page_fragmentation.as_ref()
        {
            let vertical_border_rect = vertical_root_border_paint_rect
                .expect("vertical root fragmentation supplies a logical border rect");
            let axes = FlowAxes::new(style.writing_mode, style.used_direction());
            let first_fragment_inline_offset = (fragmentation.first_inline_origin.points()
                - self.current_page_context.top())
            .abs();
            let source_extent = LogicalSize {
                inline: vertical_border_rect.size.height,
                block: vertical_border_rect.size.width,
            };
            let source_origin = PageTopPoint::new(
                vertical_border_rect.origin.x,
                vertical_border_rect.origin.y + vertical_border_rect.size.height,
            );
            // Capture descendants before appending the projected principal-box
            // decoration; otherwise the newly materialized decorations would
            // be replayed a second time as source content.
            let captured_source = self
                .current_page
                .take_paint_fragment_since(paint_checkpoint);
            self.vertical_root_block_fragment_paint(
                &fragmentation.fragments,
                style,
                vertical_border_rect,
            );
            let projected = self.project_vertical_root_fragment_paint(
                captured_source,
                &fragmentation.fragments,
                axes,
                source_origin,
                source_extent,
            );
            let projected = if first_fragment_inline_offset > 0.01 {
                let first_fragment_translation = match axes.inline_start_side() {
                    PhysicalSide::Top => PaintTranslation::new(0.0, -first_fragment_inline_offset),
                    PhysicalSide::Bottom => {
                        PaintTranslation::new(0.0, first_fragment_inline_offset)
                    }
                    PhysicalSide::Left | PhysicalSide::Right => {
                        unreachable!("vertical root page flow has a vertical logical inline axis")
                    }
                };
                projected
                    .into_iter()
                    .map(|(page_index, fragment)| {
                        if page_index == fragmentation.fragments[0].page_index {
                            (page_index, fragment.translated(first_fragment_translation))
                        } else {
                            (page_index, fragment)
                        }
                    })
                    .collect()
            } else {
                projected
            };
            Some(projected)
        } else {
            None
        };
        if let Some(projected) = vertical_root_projected_fragments {
            for (page_index, fragment) in projected {
                if page_index < self.pages.len() {
                    self.pages[page_index]
                        .append_paint_fragment_owned(fragment, PaintTranslation::identity());
                } else {
                    self.current_page
                        .append_paint_fragment_owned(fragment, PaintTranslation::identity());
                }
            }
            self.cursor_y -= block_end_margin_to_consume;
            self.last_block_layout_outcome = BlockLayoutOutcome {
                consumed_bottom_margin: layout_pt(block_end_margin_to_consume),
                margin_collapse_boundary,
                physical_border_box_inline_span: outer_inline.width(),
                static_border_box: Some(border_paint_rect),
                clamp_line_slots,
                has_local_continuation_cutoff,
                in_flow_child_fragment_end,
            };
            if matches!(style.position, Position::Relative | Position::Sticky) {
                self.cursor_y -= relative_offset.y();
            }
            self.apply_forced_break_after_box_in(fragmentainer_kind, style);
            return;
        }
        let first_descendant_paint_page = fragments
            .iter()
            .filter(|(_, fragment)| !fragment.is_empty())
            .map(|(page_index, _)| *page_index)
            .min();
        // If an auto-height block has no start-edge material or direct inline
        // content, and its first child prebreaks, the block's first fragment
        // begins with that child. Painting a decoration-only fragment in the
        // preceding remainder would manufacture background before the box has
        // generated any fragment content.
        // <https://www.w3.org/TR/css-break-3/#box-splitting>
        let suppress_leading_empty_block_fragment = out_of_flow_fragmentainer_end.is_none()
            && fragmentainer_kind == FragmentainerKind::Page
            && has_auto_height(style)
            && !has_direct_inline_content
            && border_widths.top <= 0.01
            && style.padding.top <= 0.01
            && first_descendant_paint_page.is_some_and(|page| page > paint_page_index);
        let mut decorated_block_pages = Vec::new();
        let mut translated_vertical_bookmarks = false;
        for (page_index, mut fragment) in fragments {
            // A non-fragmented overflow-clipping box owns only its originating
            // fragmentainer. Descendant overflow must not manufacture later
            // paged-media fragments: those pages are outside the scrollport
            // and are discarded before the container's clip is applied.
            // <https://www.w3.org/TR/css-overflow-3/#scrollable-overflow>
            if deferred_overflow_clip.is_some()
                && !has_auto_height(style)
                && !fragmented_definite_block
                && fragmented_block_slices.is_empty()
                && fragmented_spanner_slices.is_empty()
                && page_index != paint_page_index
            {
                continue;
            }
            if suppress_leading_empty_block_fragment
                && first_descendant_paint_page.is_some_and(|first_page| page_index < first_page)
            {
                continue;
            }
            // This captured fragment contains descendant block decorations in
            // the BackgroundBorder band. Move them into the in-flow phase
            // before applying the container's overflow scope; otherwise the
            // scope deliberately preserves BackgroundBorder for the container
            // itself and descendant backgrounds can escape the clip.
            // <https://www.w3.org/TR/css-overflow-3/#overflow-clipping>
            fragment.promote_background_border_to_in_flow_block();
            fragment.promote_outline_to_in_flow_outline();
            let mut block_align_content_offset_x = 0.0;
            if page_index == paint_page_index {
                block_align_content_offset_x = vertical_block_align_content_x_offset(
                    style,
                    content_inline.span(),
                    fragment.bounds(),
                );
                if block_align_content_offset_x.abs() > 0.01 && !translated_vertical_bookmarks {
                    self.translate_aligned_block_descendant_bookmarks(
                        descendant_bookmark_start,
                        paint_page_index,
                        block_align_content_offset_x,
                        0.0,
                    );
                    translated_vertical_bookmarks = true;
                }
            }
            if page_index == paint_page_index
                && (block_align_content_offset_x.abs() > 0.01
                    || block_align_content_offset_y.abs() > 0.01)
            {
                fragment = fragment.translated(PaintTranslation::new(
                    block_align_content_offset_x,
                    block_align_content_offset_y,
                ));
            }
            if page_index == paint_page_index
                && (static_scroll_translation.x != 0.0 || static_scroll_translation.y != 0.0)
            {
                fragment = fragment.translated(static_scroll_translation);
            }
            let fragment_overflow_clip = deferred_overflow_clip.map(|overflow_clip| {
                if has_auto_height(style) && page_index != paint_page_index {
                    // An auto-height overflow BFC fragments with its
                    // contents. Its destination fragment owns a fresh
                    // overflow clip at that fragment's painted inline origin,
                    // rather than retaining the source page's clip edge.
                    // <https://www.w3.org/TR/css-break-3/#box-splitting>
                    // <https://www.w3.org/TR/css-overflow-3/#overflow-clipping>
                    fragment.bounds().map_or(overflow_clip, |bounds| {
                        let destination_height =
                            overflow_clip.height() + (overflow_clip.y() - bounds.y()).max(0.0);
                        let canvas_block_start = self
                            .document_canvas_fragment_insets
                            .iter()
                            .map(|inset| inset.top)
                            .sum::<f32>();
                        PaintClip::new(
                            bounds.x(),
                            bounds.y(),
                            overflow_clip.width(),
                            destination_height + canvas_block_start,
                        )
                    })
                } else {
                    overflow_clip
                }
            });
            if let Some(overflow_clip) = fragment_overflow_clip {
                fragment = if let Some(contour) = deferred_box_content_contour.clone() {
                    fragment.with_contents_effect_scoped_to_box_content_contour(contour)
                } else if paint_containment_applies {
                    fragment.with_paint_containment_contents_effect_scoped_to_rect(overflow_clip)
                } else if let Some(axis_selective_clip) = deferred_axis_selective_overflow_clip {
                    fragment.with_contents_effect_scoped_to_axis_selective_rect(axis_selective_clip)
                } else {
                    // See the non-fragmented path above: scope based on the
                    // original descendant ink before updating public
                    // primitive geometry to the visible clip rectangle.
                    fragment.with_contents_effect_scoped_to_rect_preserving_contained_ink(
                        &self.current_page,
                        overflow_clip,
                    )
                };
            }
            if let Some(slice) = fragmented_block_slices
                .iter()
                .find(|slice| slice.page_index == page_index)
            {
                let mut fragment_style = style.clone();
                let decoration = slice.decoration(style.box_decoration_break);
                if propagates_document_canvas_background {
                    suppress_document_canvas_background(&mut fragment_style);
                }
                suppress_fragmented_box_edges(
                    &mut fragment_style,
                    decoration.owns_block_start(),
                    decoration.owns_block_end(),
                );
                if style.visibility == Visibility::Visible {
                    let slice_height = (slice.top - slice.bottom).max(0.0);
                    let decoration_height = if style.box_decoration_break
                        == css::BoxDecorationBreak::Clone
                    {
                        let capacity = if slice.page_index == paint_page_index {
                            block_start_page_context.area_height()
                        } else {
                            self.fragmentainer_override
                                .map(|override_| override_.context.area_height())
                                .unwrap_or_else(|| self.current_page_context.area_height())
                        };
                        // Child layout enters a continuation below its
                        // cloned block-start edge, so this source slice
                        // already contains that edge. Its block-end edge
                        // remains reserved below the child cursor and
                        // must be restored to the destination border box.
                        // Adding both edges would double-count the start;
                        // adding neither leaves the final clone fragment
                        // short by its border-plus-padding reservation.
                        (slice_height + style.padding.bottom + border_widths.bottom).min(capacity)
                    } else {
                        slice_height
                    };
                    let decoration_bottom = slice.top - decoration_height;
                    let decoration_bounds = PaintClip::new(
                        outer_x,
                        decoration_bottom,
                        outer_inline.width().points(),
                        decoration_height,
                    );
                    let committed_fragment =
                        slice.principal_box_fragment(decoration_bounds, decoration);
                    let fragment_border_rect = paint_space_rect(
                        outer_x,
                        decoration_bottom,
                        outer_inline.width().points(),
                        decoration_height,
                    );
                    let legend_exclusion = rendered_legend.and_then(|legend| {
                        legend.border_exclusion(fragment_border_rect, slice.page_index)
                    });
                    let backgrounds = self.box_background_primitives_with_legend_border_exclusion(
                        fragment_border_rect,
                        fragmented_slice_background_positioning_border_rect(
                            slice,
                            &fragmented_block_slices,
                            border_paint_rect,
                            fragment_border_rect,
                            style.box_decoration_break,
                        ),
                        &fragment_style,
                        legend_exclusion.as_ref(),
                    );
                    let outlines = self.box_outline_primitives(
                        paint_space_rect(
                            outer_x,
                            decoration_bottom,
                            outer_inline.width().points(),
                            decoration_height,
                        ),
                        &fragment_style,
                    );
                    if committed_fragment
                        .kind()
                        .principal_box()
                        .expect("principal block slice retains decoration geometry")
                        .decoration()
                        .is_clone()
                    {
                        fragment.prepend_monolithic_primitives_in_band(
                            PaintBand::BackgroundBorder,
                            decoration_bounds,
                            backgrounds,
                        );
                        fragment.append_monolithic_primitives_in_band(
                            PaintBand::Outline,
                            decoration_bounds,
                            outlines,
                        );
                    } else {
                        fragment
                            .prepend_primitives_in_band(PaintBand::BackgroundBorder, backgrounds);
                        fragment.append_primitives_in_band(PaintBand::Outline, outlines);
                    }
                }
                decorated_block_pages.push(page_index);
            } else if page_index == background_page_index {
                if is_root_element {
                    // Descendant block backgrounds recorded by the root
                    // still belong to normal flow, while the root's own
                    // border remains in Appendix E's earlier root phase.
                    fragment.promote_background_border_to_in_flow_block();
                    fragment.promote_outline_to_in_flow_outline();
                }
                fragment.prepend_primitives_in_band(
                    PaintBand::BackgroundBorder,
                    own_background_primitives.clone(),
                );
                fragment
                    .append_primitives_in_band(PaintBand::Outline, own_outline_primitives.clone());
            }
            if !defer_own_decoration_promotion && !is_root_element {
                fragment.promote_background_border_to_in_flow_block();
                fragment.promote_outline_to_in_flow_outline();
            }
            if retain_size_contained_monolithic_paint {
                fragment = fragment.with_monolithic_fragmentation_scope(
                    PaintClip::from_paint_rect(border_paint_rect),
                );
            }
            if fragment.is_empty() {
                continue;
            }
            if page_index < self.pages.len() {
                self.pages[page_index]
                    .append_paint_fragment_owned(fragment, PaintTranslation::identity());
            } else {
                self.current_page
                    .append_paint_fragment_owned(fragment, PaintTranslation::identity());
            }
        }
        // A fragment can contain only the principal box's decoration (for
        // example when a forced break occurs beside an empty child). Preserve
        // that fragment even when there was no descendant paint tree to which
        // the decoration could be prepended above.
        for slice in fragmented_block_slices
            .iter()
            .filter(|slice| !decorated_block_pages.contains(&slice.page_index))
            .filter(|slice| {
                !suppress_leading_empty_block_fragment
                    || first_descendant_paint_page
                        .is_none_or(|first_page| slice.page_index >= first_page)
            })
        {
            if style.visibility != Visibility::Visible {
                continue;
            }
            let mut fragment_style = style.clone();
            let decoration = slice.decoration(style.box_decoration_break);
            if propagates_document_canvas_background {
                suppress_document_canvas_background(&mut fragment_style);
            }
            suppress_fragmented_box_edges(
                &mut fragment_style,
                decoration.owns_block_start(),
                decoration.owns_block_end(),
            );
            let mut fragment = PaintFragment::from_primitives(Vec::new(), Vec::new());
            let slice_height = (slice.top - slice.bottom).max(0.0);
            let decoration_height = if style.box_decoration_break == css::BoxDecorationBreak::Clone
            {
                let capacity = if slice.page_index == paint_page_index {
                    block_start_page_context.area_height()
                } else {
                    self.fragmentainer_override
                        .map(|override_| override_.context.area_height())
                        .unwrap_or_else(|| self.current_page_context.area_height())
                };
                (slice_height + style.padding.bottom + border_widths.bottom).min(capacity)
            } else {
                slice_height
            };
            let decoration_bottom = slice.top - decoration_height;
            let decoration_bounds = PaintClip::new(
                outer_x,
                decoration_bottom,
                outer_inline.width().points(),
                decoration_height,
            );
            let committed_fragment = slice.principal_box_fragment(decoration_bounds, decoration);
            let fragment_border_rect = paint_space_rect(
                outer_x,
                decoration_bottom,
                outer_inline.width().points(),
                decoration_height,
            );
            let legend_exclusion = rendered_legend
                .and_then(|legend| legend.border_exclusion(fragment_border_rect, slice.page_index));
            let backgrounds = self.box_background_primitives_with_legend_border_exclusion(
                fragment_border_rect,
                fragmented_slice_background_positioning_border_rect(
                    slice,
                    &fragmented_block_slices,
                    border_paint_rect,
                    fragment_border_rect,
                    style.box_decoration_break,
                ),
                &fragment_style,
                legend_exclusion.as_ref(),
            );
            let outlines = self.box_outline_primitives(
                paint_space_rect(
                    outer_x,
                    decoration_bottom,
                    outer_inline.width().points(),
                    decoration_height,
                ),
                &fragment_style,
            );
            if committed_fragment
                .kind()
                .principal_box()
                .expect("principal block slice retains decoration geometry")
                .decoration()
                .is_clone()
            {
                fragment.prepend_monolithic_primitives_in_band(
                    PaintBand::BackgroundBorder,
                    decoration_bounds,
                    backgrounds,
                );
                fragment.append_monolithic_primitives_in_band(
                    PaintBand::Outline,
                    decoration_bounds,
                    outlines,
                );
            } else {
                fragment.prepend_primitives_in_band(PaintBand::BackgroundBorder, backgrounds);
                fragment.append_primitives_in_band(PaintBand::Outline, outlines);
            }
            if !defer_own_decoration_promotion {
                fragment.promote_background_border_to_in_flow_block();
                fragment.promote_outline_to_in_flow_outline();
            }
            if fragment.is_empty() {
                continue;
            }
            if slice.page_index < self.pages.len() {
                self.pages[slice.page_index]
                    .append_paint_fragment_owned(fragment, PaintTranslation::identity());
            } else if slice.page_index == self.pages.len() {
                self.current_page
                    .append_paint_fragment_owned(fragment, PaintTranslation::identity());
            } else {
                self.pending_paint_fragments.push(PendingPaintFragment {
                    page_index: slice.page_index,
                    fragment,
                    kind: PendingPaintFragmentKind::InFlowOverflow,
                });
            }
        }
        if style.visibility == Visibility::Visible {
            for slice in &fragmented_spanner_slices {
                let mut fragment_style = style.clone();
                let decoration = slice.decoration(style.box_decoration_break);
                if propagates_document_canvas_background {
                    suppress_document_canvas_background(&mut fragment_style);
                }
                suppress_fragmented_box_edges(
                    &mut fragment_style,
                    decoration.owns_block_start(),
                    decoration.owns_block_end(),
                );
                let mut fragment = PaintFragment::from_primitives(Vec::new(), Vec::new());
                let slice_height = (slice.top - slice.bottom).max(0.0);
                let decoration_height =
                    if style.box_decoration_break == css::BoxDecorationBreak::Clone {
                        let capacity = if slice.page_index == paint_page_index {
                            block_start_page_context.area_height()
                        } else {
                            self.fragmentainer_override
                                .map(|override_| override_.context.area_height())
                                .unwrap_or_else(|| self.current_page_context.area_height())
                        };
                        (slice_height + style.padding.bottom + border_widths.bottom).min(capacity)
                    } else {
                        slice_height
                    };
                let decoration_bottom = slice.top - decoration_height;
                let decoration_bounds = PaintClip::new(
                    outer_x,
                    decoration_bottom,
                    outer_inline.width().points(),
                    decoration_height,
                );
                let committed_fragment =
                    slice.principal_box_fragment(decoration_bounds, decoration);
                let fragment_border_rect = paint_space_rect(
                    outer_x,
                    decoration_bottom,
                    outer_inline.width().points(),
                    decoration_height,
                );
                let legend_exclusion = rendered_legend.and_then(|legend| {
                    legend.border_exclusion(fragment_border_rect, slice.page_index)
                });
                let backgrounds = self.box_background_primitives_with_legend_border_exclusion(
                    fragment_border_rect,
                    fragmented_slice_background_positioning_border_rect(
                        slice,
                        &fragmented_spanner_slices,
                        border_paint_rect,
                        fragment_border_rect,
                        style.box_decoration_break,
                    ),
                    &fragment_style,
                    legend_exclusion.as_ref(),
                );
                let outlines = self.box_outline_primitives(
                    paint_space_rect(
                        outer_x,
                        decoration_bottom,
                        outer_inline.width().points(),
                        decoration_height,
                    ),
                    &fragment_style,
                );
                if committed_fragment
                    .kind()
                    .principal_box()
                    .expect("principal block slice retains decoration geometry")
                    .decoration()
                    .is_clone()
                {
                    fragment.prepend_monolithic_primitives_in_band(
                        PaintBand::BackgroundBorder,
                        decoration_bounds,
                        backgrounds,
                    );
                    fragment.append_monolithic_primitives_in_band(
                        PaintBand::Outline,
                        decoration_bounds,
                        outlines,
                    );
                } else {
                    fragment.prepend_primitives_in_band(PaintBand::BackgroundBorder, backgrounds);
                    fragment.append_primitives_in_band(PaintBand::Outline, outlines);
                }
                // See the equivalent ordinary-fragment loop above: this is
                // the sole paint owner for a synthesized definite span.
                fragment.promote_background_border_to_in_flow_block();
                fragment.promote_outline_to_in_flow_outline();
                if slice.page_index < self.pages.len() {
                    self.pages[slice.page_index]
                        .append_paint_fragment_owned(fragment, PaintTranslation::identity());
                } else {
                    self.current_page
                        .append_paint_fragment_owned(fragment, PaintTranslation::identity());
                }
            }
        }
        self.cursor_y -= block_end_margin_to_consume;
        self.last_block_layout_outcome = BlockLayoutOutcome {
            consumed_bottom_margin: layout_pt(block_end_margin_to_consume),
            margin_collapse_boundary,
            physical_border_box_inline_span: outer_inline.width(),
            static_border_box: Some(border_paint_rect),
            clamp_line_slots,
            has_local_continuation_cutoff,
            in_flow_child_fragment_end,
        };
        if matches!(style.position, Position::Relative | Position::Sticky) {
            self.cursor_y -= relative_offset.y();
        }
        self.apply_forced_break_after_box_in(fragmentainer_kind, style);
    }

    /// Position the content end of a definite promoted spanner through the
    /// current outer fragmentainer sequence.
    ///
    /// Unlike an ordinary class-A sibling, a promoted spanner may itself be
    /// fragmented by an enclosing page or multicol context. Consuming the
    /// definite size continuously avoids leaving unused remainder space before
    /// moving to the next outer fragmentainer.
    /// <https://www.w3.org/TR/css-multicol-1/#spanning-columns>
    /// <https://www.w3.org/TR/css-break-3/#breaking-rules>
    /// Consume a fixed principal block through page or outer-column
    /// fragmentainers.
    ///
    /// CSS sizing preserves a definite descendant percentage basis as a
    /// content-box length, but a cloned fixed `border-box` consumes its
    /// destination extent a fragment at a time. Keep that projection at the
    /// fragmentation cursor rather than converting the size to an untyped
    /// scalar at layout entry.
    /// <https://www.w3.org/TR/css-sizing-3/#box-model>
    /// <https://www.w3.org/TR/css-break-3/#box-model-for-breaking>
    fn consume_fixed_principal_block_size_through_fragmentainers(
        &mut self,
        content_top: f32,
        size: FixedPrincipalBlockSize,
        decoration: FragmentDecoration,
        reservation: FragmentDecorationReservation,
    ) {
        if let Some(mut progress) = size.cloned_border_box_progress(decoration, reservation) {
            self.consume_cloned_border_box_through_fragmentainers(content_top, &mut progress);
        } else {
            self.consume_definite_block_size_through_fragmentainers(
                content_top,
                size.content_height().points(),
            );
        }
    }

    /// Consume a cloned fixed border-box's destination budget through the
    /// active fragmentainer sequence.
    ///
    /// The fragmentainer cursor advances only by the content portion returned
    /// by [`ClonedBorderBoxProgress`]; each fragment's repeated decoration is
    /// already represented by its destination page coordinates.
    fn consume_cloned_border_box_through_fragmentainers(
        &mut self,
        content_top: f32,
        progress: &mut ClonedBorderBoxProgress,
    ) {
        self.cursor_y = content_top;
        let consume_current = |progress: &mut ClonedBorderBoxProgress, content_capacity: f32| {
            progress.consume_content_capacity(layout_pt(content_capacity))
        };
        if self.active_fragmentainer_kind() == FragmentainerKind::Column {
            let available = (self.cursor_y - self.page_bottom()).max(0.0);
            self.cursor_y -= consume_current(progress, available).points();
            if progress.is_complete() {
                return;
            }
            self.mark_current_page_flow_content();
            let materialization_limit = self
                .positioned_scratch_page_limit()
                .unwrap_or(MAX_MATERIALIZED_COLUMN_FRAGMENTAINERS);
            while !progress.is_complete() && self.pages.len() + 1 < materialization_limit {
                self.push_page();
                let destination_content_capacity =
                    (self.cursor_y - self.page_bottom()).max(css::CSS_PX_TO_PT);
                self.cursor_y -= consume_current(progress, destination_content_capacity).points();
                if progress.is_complete() {
                    return;
                }
                self.mark_current_page_flow_content();
            }
            if !progress.is_complete() {
                self.cursor_y = self.page_top() - progress.remaining_border_box().points();
            }
            return;
        }
        let available = (self.cursor_y - self.page_bottom()).max(0.0);
        self.cursor_y -= consume_current(progress, available).points();
        if progress.is_complete() {
            return;
        }
        self.mark_current_page_flow_content();
        let materialization_limit = self
            .positioned_scratch_page_limit()
            .unwrap_or(MAX_MATERIALIZED_PAGE_FRAGMENTAINERS);
        while !progress.is_complete() && self.pages.len() + 1 < materialization_limit {
            self.push_page();
            let destination_content_capacity =
                (self.cursor_y - self.page_bottom()).max(css::CSS_PX_TO_PT);
            self.cursor_y -= consume_current(progress, destination_content_capacity).points();
            if progress.is_complete() {
                return;
            }
            self.mark_current_page_flow_content();
        }
        if !progress.is_complete() {
            self.cursor_y = self.page_top() - progress.remaining_border_box().points();
        }
    }

    /// Consume a definite physical block size continuously through page or
    /// outer-column fragmentainers.
    ///
    /// Oversized monolithic boxes remain one layout object, but CSS
    /// Fragmentation still places their graphical slices in every crossed
    /// fragmentainer and resumes following flow at the continuous block-end.
    /// This primitive is shared by definite blocks, promoted spanners, and
    /// oversized atomic line boxes.
    /// <https://www.w3.org/TR/css-break-3/#monolithic>
    pub(in crate::layout) fn consume_definite_block_size_through_fragmentainers(
        &mut self,
        content_top: f32,
        height: f32,
    ) {
        self.cursor_y = content_top;
        let mut remaining = height.max(0.0);
        if self.active_fragmentainer_kind() == FragmentainerKind::Column {
            let available = (self.cursor_y - self.page_bottom()).max(0.0);
            if remaining <= available + 0.01 {
                self.cursor_y -= remaining;
                return;
            }
            if available > 0.01 {
                self.cursor_y -= available;
                remaining -= available;
                self.mark_current_page_flow_content();
            }
            let materialization_limit = self
                .positioned_scratch_page_limit()
                .unwrap_or(MAX_MATERIALIZED_COLUMN_FRAGMENTAINERS);
            while remaining > 0.01 && self.pages.len() + 1 < materialization_limit {
                self.push_page();
                // `push_page` enters below every active cloned block-start
                // edge, while `page_bottom` retains their block-end edges.
                // Compute the usable content capacity from those destination
                // coordinates instead of from raw column height.  A raw
                // chunk would let a definite box consume its cloned border
                // and padding, shortening the final principal fragment.
                // <https://www.w3.org/TR/css-break-3/#box-model-for-breaking>
                let destination_capacity =
                    (self.cursor_y - self.page_bottom()).max(css::CSS_PX_TO_PT);
                let consumed = remaining.min(destination_capacity);
                self.cursor_y -= consumed;
                remaining -= consumed;
                if remaining > 0.01 {
                    self.mark_current_page_flow_content();
                } else {
                    return;
                }
            }
            if remaining > 0.01 {
                // The retained current page stands in for the last conceptual
                // off-canvas column so following invisible flow preserves its
                // block offset without allocating the skipped prefix.
                self.cursor_y = self.page_top() - remaining;
            }
            return;
        }
        let available = (self.cursor_y - self.page_bottom()).max(0.0);
        if remaining <= available + 0.01 {
            self.cursor_y -= remaining;
            return;
        }
        if available > 0.01 {
            self.cursor_y -= available;
            remaining -= available;
            // The box occupies this fragment even when its background and
            // descendants are appended after used-size resolution. Mark that
            // occupancy so the empty-page guard does not collapse a real
            // definite block fragment into its continuation.
            self.mark_current_page_flow_content();
        }
        let materialization_limit = self
            .positioned_scratch_page_limit()
            .unwrap_or(MAX_MATERIALIZED_PAGE_FRAGMENTAINERS);
        while remaining > 0.01 && self.pages.len() + 1 < materialization_limit {
            self.push_page();
            // Every page fragmentainer may establish a distinct page area.
            // Preserve the source box's continuous block progress in
            // `remaining`, but resolve the destination capacity only after
            // advancing into that destination context.
            // <https://drafts.csswg.org/css-break-4/#varying-size-fragmentainers>
            let destination_capacity = (self.cursor_y - self.page_bottom()).max(css::CSS_PX_TO_PT);
            let consumed = remaining.min(destination_capacity);
            self.cursor_y -= consumed;
            remaining -= consumed;
            if remaining > 0.01 {
                self.mark_current_page_flow_content();
            } else {
                return;
            }
        }
        if remaining > 0.01 {
            // Retain the bounded materialized prefix for pathological
            // lengths. The current destination keeps the source's remaining
            // logical progress without allocating an unbounded page run.
            self.cursor_y = self.page_top() - remaining;
        }
    }

    fn definite_block_descendants_overflow(
        &mut self,
        child_boxes: Option<&[box_tree::FormattingBox<'_>]>,
        stylesheets: &Stylesheets<'_>,
        available_width: f32,
        probe: DeferredDescendantOverflowProbe,
    ) -> bool {
        let Some(child_boxes) = child_boxes else {
            return false;
        };
        let estimated = child_boxes
            .iter()
            .filter_map(|child| match child {
                box_tree::FormattingBox::AnonymousBlock(box_) => {
                    formatting_box_has_inline_content(&box_.children)
                        .then_some(box_.style.line_height)
                }
                _ => child
                    .element_parts()
                    .and_then(|(element, _, style, children)| {
                        style_is_in_normal_flow(style).then(|| {
                            self.estimate_element_height(
                                element,
                                style,
                                stylesheets,
                                available_width,
                                Some(children),
                            )
                        })
                    })
                    .flatten(),
            })
            .sum::<f32>();
        estimated > probe.content_height().points() + 0.01
    }

    /// Lay out overflowing descendants independently from a definite
    /// principal box's normal-flow end position.
    ///
    /// CSS overflow does not enlarge a definite-height box. Descendant paint
    /// can therefore reach later outer fragmentainers while the following
    /// sibling starts at the principal box's authored block-end. This applies
    /// both to a promoted spanner and to an ordinary definite box that fits in
    /// its current column. Quire already uses this speculative/deferred-paint
    /// model for fragmented floats; the same mechanism preserves counters,
    /// bookmarks, links, named strings, and running elements while restoring
    /// normal-flow geometry.
    /// <https://www.w3.org/TR/css-multicol-1/#spanning-columns>
    /// <https://www.w3.org/TR/css-break-3/#fragmentation-model>
    #[allow(clippy::too_many_arguments)]
    fn layout_definite_block_with_deferred_descendant_overflow(
        &mut self,
        element: &Element,
        style: &ComputedStyle,
        stylesheets: &Stylesheets<'_>,
        run_in_children: &[box_tree::FormattingBox<'_>],
        child_boxes: Option<&[box_tree::FormattingBox<'_>]>,
        descendant_percentage_height_basis: Option<BlockSizePercentageBasis>,
        probe: DeferredDescendantOverflowProbe,
        vertical_non_content: NonContentLength,
        principal_box_paint_mode: PrincipalBoxPaintMode,
    ) {
        let fragmentainer_kind = self.active_fragmentainer_kind();
        let snapshot = self.snapshot();
        let paint_page_index = self.pages.len();
        let paint_checkpoint = self.current_page.paint_checkpoint();

        self.multicol_spanner_speculation_depth += 1;
        self.fragmentation_suppression_depth += 1;
        self.layout_block_with_descendant_percentage_height_basis(
            element,
            style,
            stylesheets,
            run_in_children,
            child_boxes,
            descendant_percentage_height_basis,
            principal_box_paint_mode,
        );
        self.fragmentation_suppression_depth -= 1;
        self.multicol_spanner_speculation_depth -= 1;

        let clearance_crossed_fragmentainer = self.pages.len() > snapshot.pages.len()
            && self.last_block_layout_outcome.margin_collapse_boundary
                == BlockMarginCollapseBoundary::SeparatedByClearance;
        let captured_fragments =
            self.take_positioned_fragments_since(paint_page_index, paint_checkpoint);
        let fragments = captured_fragments
            .into_iter()
            .flat_map(|(page_index, fragment)| {
                if clearance_crossed_fragmentainer {
                    // This definite box has no block-size contribution, but
                    // its normal-flow source crossed float continuation
                    // fragmentainers before its overflowing descendants
                    // painted. Retain that source assignment: projecting
                    // from the pre-clear snapshot would replay the descendant
                    // in the original column.
                    // <https://www.w3.org/TR/CSS22/visuren.html#flow-control>
                    // <https://drafts.csswg.org/css-break/#parallel-flows>
                    vec![(page_index, fragment)]
                } else if probe.is_vertical_root() {
                    vertical_root_continuous_fragmentainer_paint_slices(
                        &snapshot,
                        fragment,
                        self.principal_flow,
                    )
                } else {
                    continuous_fragmentainer_paint_slices(&snapshot, fragment)
                }
            })
            .collect::<Vec<_>>();
        let side_effects = self.deferred_layout_side_effects_since(&snapshot);
        let counter_set = self.counter_set.clone();
        let quote_depth = self.quote_depth;
        let next_assignment_id = self.next_assignment_id;
        let next_paint_source_order = self.next_paint_source_order;
        self.restore(snapshot);
        self.counter_set = counter_set;
        self.quote_depth = quote_depth;
        self.next_assignment_id = next_assignment_id;
        self.next_paint_source_order = next_paint_source_order;
        self.apply_deferred_layout_side_effects(side_effects);

        for (page_index, fragment) in fragments {
            if page_index == self.pages.len() {
                self.current_page
                    .append_paint_fragment_owned(fragment, PaintTranslation::identity());
                self.mark_current_page_flow_content();
            } else {
                self.pending_paint_fragments.push(PendingPaintFragment {
                    page_index,
                    fragment,
                    kind: PendingPaintFragmentKind::InFlowOverflow,
                });
            }
        }

        self.apply_forced_break_before_box_in(fragmentainer_kind, style);
        let starts_at_page_top = self.cursor_is_at_page_top() && self.truncate_page_start_margins;
        self.cursor_y -=
            page_start_margin(layout_pt(style.margin.top), starts_at_page_top).points();
        let definite_content_height = probe.content_height().points();
        let border_box_height = definite_content_height + vertical_non_content.points();
        let block_top = self.cursor_y;
        if let Some(block_size) = probe.vertical_root_block_size {
            let fragmentation = self
                .consume_vertical_root_page_block_size(
                    block_size,
                    PageTopBlockPosition::new(block_top),
                )
                .expect("a qualifying vertical root block must have a page continuation");
            debug_assert!(fragmentation.fragments.len() > 1);
        } else {
            self.consume_definite_block_size_through_fragmentainers(block_top, border_box_height);
        }
        self.cursor_y -= style.margin.bottom;
        self.last_block_layout_outcome = BlockLayoutOutcome {
            consumed_bottom_margin: layout_pt(style.margin.bottom),
            // This deferred replay has already consumed the principal box's
            // normal-flow placement above. Its synthetic continuation does
            // not resolve `clear`, so it cannot introduce a new clearance
            // margin-collapse boundary.
            margin_collapse_boundary: BlockMarginCollapseBoundary::Adjoining,
            physical_border_box_inline_span: border_box_pt(0.0),
            static_border_box: None,
            clamp_line_slots: 0,
            has_local_continuation_cutoff: false,
            in_flow_child_fragment_end: None,
        };
        self.apply_forced_break_after_box_in(fragmentainer_kind, style);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn definite_content_height(value: f32) -> DefinitePhysicalContentHeight {
        DefinitePhysicalContentHeight::new(PhysicalContentHeight::new(content_box_pt(value)))
    }

    fn length(value: f32) -> css::ComputedLengthPercentageOrAuto {
        css::ComputedLengthPercentageOrAuto::LengthPercentage(
            css::ComputedLengthPercentage::from_points(value),
        )
    }

    fn replay_length(value: PostLayoutHeightReplay) -> f32 {
        match value.as_used_height() {
            css::ComputedLengthPercentageOrAuto::LengthPercentage(value) => value.length_points(),
            _ => unreachable!("replay height is always a fixed length"),
        }
    }

    #[test]
    fn deferred_descendant_overflow_probe_requires_definite_scrollable_candidate() {
        let height = definite_content_height(24.0);

        assert!(
            DeferredDescendantOverflowProbe::new(
                None,
                true,
                0,
                DescendantOverflowContribution::Scrollable,
                None,
            )
            .is_none()
        );
        assert!(
            DeferredDescendantOverflowProbe::new(
                Some(height),
                false,
                0,
                DescendantOverflowContribution::Scrollable,
                None,
            )
            .is_none()
        );
        assert!(
            DeferredDescendantOverflowProbe::new(
                Some(height),
                true,
                1,
                DescendantOverflowContribution::Scrollable,
                None,
            )
            .is_none()
        );
        assert!(
            DeferredDescendantOverflowProbe::new(
                Some(height),
                true,
                0,
                DescendantOverflowContribution::InkOnly,
                None,
            )
            .is_none()
        );

        let probe = DeferredDescendantOverflowProbe::new(
            Some(height),
            true,
            0,
            DescendantOverflowContribution::Scrollable,
            Some(layout_pt(40.0)),
        )
        .expect("eligible definite-height block retains its overflow probe");

        assert_eq!(
            probe.content_height(),
            PhysicalContentHeight::new(content_box_pt(24.0))
        );
        assert!(probe.is_vertical_root());
    }

    #[test]
    fn post_layout_height_replay_preserves_border_box_min_height() {
        let mut style = ComputedStyle::initial();
        style.box_sizing = BoxSizing::BorderBox;
        *style.box_values.height = length(10.0);
        style.box_values.min_height = length(25.0);
        let vertical_non_content = non_content_pt(15.0);

        let replay = post_layout_height_replay_constraint(
            &style,
            PercentageBasis::indefinite(),
            vertical_non_content,
            content_box_pt(0.0),
        )
        .expect("the minimum height wins");

        assert_eq!(replay_length(replay), 25.0);
        assert_eq!(
            replay.content_box_length(vertical_non_content),
            content_box_pt(10.0)
        );
    }

    #[test]
    fn post_layout_height_replay_preserves_border_box_max_height() {
        let mut style = ComputedStyle::initial();
        style.box_sizing = BoxSizing::BorderBox;
        *style.box_values.height = length(100.0);
        style.box_values.max_height = length(25.0);
        let vertical_non_content = non_content_pt(15.0);

        let replay = post_layout_height_replay_constraint(
            &style,
            PercentageBasis::indefinite(),
            vertical_non_content,
            content_box_pt(85.0),
        )
        .expect("the maximum height wins");

        assert_eq!(replay_length(replay), 25.0);
        assert_eq!(
            replay.content_box_length(vertical_non_content),
            content_box_pt(10.0)
        );
    }

    #[test]
    fn post_layout_height_replay_keeps_content_box_constraints_unmodified() {
        let mut style = ComputedStyle::initial();
        style.box_values.max_height = length(25.0);

        let replay = post_layout_height_replay_constraint(
            &style,
            PercentageBasis::indefinite(),
            non_content_pt(15.0),
            content_box_pt(100.0),
        )
        .expect("the maximum height wins");

        assert_eq!(
            replay,
            PostLayoutHeightReplay::ContentBox(content_box_pt(25.0))
        );
        assert_eq!(replay_length(replay), 25.0);
    }

    #[test]
    fn post_layout_height_replay_applies_maximum_before_minimum() {
        let mut style = ComputedStyle::initial();
        style.box_values.min_height = length(50.0);
        style.box_values.max_height = length(25.0);

        let replay = post_layout_height_replay_constraint(
            &style,
            PercentageBasis::indefinite(),
            non_content_pt(0.0),
            content_box_pt(100.0),
        )
        .expect("the minimum wins after the maximum cap");

        assert_eq!(
            replay,
            PostLayoutHeightReplay::ContentBox(content_box_pt(50.0))
        );
    }
}

/// Whether a descendant contributes a forced boundary to this fragmented
/// flow.
///
/// A definite principal box can defer ordinary visible overflow without
/// moving its normal-flow end, but a forced descendant break establishes a
/// real boundary in the descendant's parallel fragmented flow and must remain
/// materialized by the regular fragmentation algorithm.
/// <https://www.w3.org/TR/css-break-3/#forced-breaks>
fn formatting_boxes_have_forced_break_in(
    boxes: Option<&[box_tree::FormattingBox<'_>]>,
    fragmentainer_kind: FragmentainerKind,
) -> bool {
    boxes.is_some_and(|boxes| {
        boxes.iter().any(|box_| match box_ {
            box_tree::FormattingBox::AnonymousBlock(anonymous) => {
                fragmentainer_kind.is_forced_break(anonymous.style.break_before)
                    || fragmentainer_kind.is_forced_break(anonymous.style.break_after)
                    || formatting_boxes_have_forced_break_in(
                        Some(&anonymous.children),
                        fragmentainer_kind,
                    )
            }
            box_tree::FormattingBox::InlineSplitBlockContext(context) => {
                formatting_boxes_have_forced_break_in(
                    Some(&context.core.children),
                    fragmentainer_kind,
                )
            }
            _ => box_.element_parts().is_some_and(|(_, _, style, children)| {
                style_is_in_normal_flow(style)
                    && (fragmentainer_kind.is_forced_break(style.break_before)
                        || fragmentainer_kind.is_forced_break(style.break_after)
                        || formatting_boxes_have_forced_break_in(
                            Some(children),
                            fragmentainer_kind,
                        ))
            }),
        })
    })
}

#[derive(Debug, Clone, Copy)]
struct PromotedSpannerPaintSlice {
    page_index: usize,
    top: f32,
    bottom: f32,
    owns_block_start: bool,
    owns_block_end: bool,
}

impl PromotedSpannerPaintSlice {
    fn decoration(self, decoration_break: css::BoxDecorationBreak) -> FragmentDecoration {
        FragmentDecoration::for_box_decoration_break(
            decoration_break,
            self.owns_block_start,
            self.owns_block_end,
        )
    }

    fn principal_box_fragment(
        self,
        border_box: PaintClip,
        decoration: FragmentDecoration,
    ) -> CommittedContainerFragment<()> {
        CommittedContainerFragment::principal(
            FragmentainerOrdinal::new(self.page_index),
            (),
            border_box,
            decoration,
        )
    }
}

/// Map the continuous source background-positioning area into one physical
/// page/column fragmentainer.
///
/// With `box-decoration-break: slice`, CSS Backgrounds positions a background
/// once for the unfragmented box, then CSS Fragmentation clips that result in
/// each fragment.  The layout engine stores page paint in page-local physical
/// coordinates, so every continuation needs the accumulated earlier
/// block-span translation before resolving `background-position` and
/// `background-size`.  The fragment border rectangle remains the clip area.
/// Logical block progression has already been projected to this physical
/// fragmentainer boundary; keeping that projection here prevents a vertical
/// writing-mode caller from independently reinterpreting the offset.
///
/// <https://www.w3.org/TR/css-backgrounds-3/#background-position>
/// <https://www.w3.org/TR/css-backgrounds-3/#box-decoration-break>
/// <https://www.w3.org/TR/css-break-3/#fragmentation-model>
/// <https://www.w3.org/TR/css-writing-modes-4/#logical-to-physical>
fn fragmented_slice_background_positioning_border_rect(
    slice: &PromotedSpannerPaintSlice,
    slices: &[PromotedSpannerPaintSlice],
    source_border_rect: PaintRect,
    fragment_border_rect: PaintRect,
    decoration_break: css::BoxDecorationBreak,
) -> PaintRect {
    let preceding_block_span = slices
        .iter()
        .take_while(|candidate| candidate.page_index < slice.page_index)
        .map(|candidate| (candidate.top - candidate.bottom).max(0.0))
        .sum::<f32>();
    FragmentedDecorationSlice::new(
        source_border_rect,
        fragment_border_rect,
        PaintTranslation::new(0.0, preceding_block_span),
        slice.owns_block_start,
        slice.owns_block_end,
    )
    .positioning_border_rect(decoration_break)
}

/// Slice continuous paint from one source coordinate space into its outer
/// fragmentainers.
///
/// Overflow is laid out in one unbounded source coordinate space, then clipped
/// and translated into the current remainder and full continuation
/// fragmentainers. This is the paint counterpart of the independently tracked
/// normal-flow block size. Definite spanners and oversized atomic line boxes
/// use this same projection so paint and flow consume identical fragmentainer
/// distances.
/// <https://www.w3.org/TR/css-break-3/#fragmentation-model>
pub(in crate::layout) fn continuous_fragmentainer_paint_slices(
    snapshot: &LayoutSnapshot,
    fragment: PaintFragment,
) -> Vec<(usize, PaintFragment)> {
    let Some(bounds) = fragment.bounds() else {
        return Vec::new();
    };
    let first_context = snapshot.current_page_context;
    let continuation_context = snapshot
        .fragmentainer_override
        .map(|override_| override_.context)
        .unwrap_or(first_context);
    let source_bottom = bounds.y();
    let mut source_top = snapshot.cursor_y.max(source_bottom);
    let mut page_index = snapshot.pages.len();
    let mut first_slice = true;
    let mut slices = Vec::new();
    while source_top > source_bottom + 0.01 {
        let context = if first_slice {
            first_context
        } else {
            continuation_context
        };
        let target_top = if first_slice {
            snapshot.cursor_y
        } else {
            context.top()
        };
        let capacity = if first_slice {
            (snapshot.cursor_y - context.bottom()).max(0.0)
        } else {
            context.area_height().max(0.0)
        };
        if capacity <= 0.01 {
            page_index += 1;
            first_slice = false;
            continue;
        }
        let slice_height = (source_top - source_bottom).min(capacity);
        let source_clip = PageTopRect::new(
            context.left(),
            source_top,
            context.area_width(),
            slice_height,
        )
        .paint_clip();
        let slice = fragment.clone().clipped_to_rect(source_clip);
        if !slice.is_empty() {
            slices.push((
                page_index,
                slice.translated(PaintTranslation::new(0.0, target_top - source_top)),
            ));
        }
        source_top -= slice_height;
        page_index += 1;
        first_slice = false;
    }
    slices
}

/// Slice deferred fixed-box overflow through the root's vertical page flow.
///
/// The captured source remains a continuous physical paint canvas, but CSS
/// Fragmentation connects page content areas in the root element's block-flow
/// direction.  A vertical root therefore slices physical X while preserving
/// the source's physical Y (logical inline) overflow.  Later slices restart
/// at a fresh page area, which is the zero-decoration overflow origin defined
/// for paginated fixed-box overflow.
/// <https://www.w3.org/TR/css-break-3/#parallel-flows>
/// <https://www.w3.org/TR/css-break-3/#transforms>
/// <https://www.w3.org/TR/css-writing-modes-4/#block-flow>
fn vertical_root_continuous_fragmentainer_paint_slices(
    snapshot: &LayoutSnapshot,
    fragment: PaintFragment,
    principal_flow: DocumentPrincipalFlow,
) -> Vec<(usize, PaintFragment)> {
    let Some(bounds) = fragment.bounds() else {
        return Vec::new();
    };
    let axes = FlowAxes::new(principal_flow.writing_mode, principal_flow.used_direction());
    debug_assert!(
        WritingModeAxes::new(principal_flow.writing_mode, principal_flow.used_direction(),)
            .swaps_physical_axes()
    );

    let source_origin = PageTopPoint::new(bounds.x(), bounds.y() + bounds.height());
    let source_extent = LogicalSize {
        inline: bounds.height(),
        block: bounds.width(),
    };
    let first_context = snapshot.current_page_context;
    let continuation_context = snapshot
        .fragmentainer_override
        .map(|override_| override_.context)
        .unwrap_or(first_context);
    let capacity = first_context.logical_block_size(principal_flow.writing_mode);
    let ranges = root_page_block_slices(layout_pt(source_extent.block), layout_pt(capacity));
    let mut slices = Vec::new();
    for (slice_index, range) in ranges.into_iter().enumerate() {
        let first_slice = slice_index == 0;
        let context = if first_slice {
            first_context
        } else {
            continuation_context
        };
        let destination_origin = if first_slice {
            // The source box retains its actual first-fragment inline origin.
            // Continuations instead begin at the empty overflow box at the
            // destination fragmentainer's inline start.
            PageTopPoint::new(context.left(), source_origin.top_y())
        } else {
            PageTopPoint::new(context.left(), context.top())
        };
        let projection = FragmentainerProjection::new(FragmentainerProjectionInput {
            source_axes: axes,
            source_origin,
            source_extent,
            source_slice: LogicalRect {
                origin: LogicalPoint {
                    inline: 0.0,
                    block: range.source_block_start.points(),
                },
                size: LogicalSize {
                    inline: source_extent.inline,
                    block: range.block_size.points(),
                },
            },
            destination_axes: axes,
            destination_origin,
            destination_extent: LogicalSize {
                inline: source_extent.inline,
                block: capacity,
            },
            destination_slice: LogicalRect {
                origin: LogicalPoint {
                    inline: 0.0,
                    block: 0.0,
                },
                size: LogicalSize {
                    inline: source_extent.inline,
                    block: range.block_size.points(),
                },
            },
            destination_page_area: PageTopRect::new(
                context.left(),
                context.top(),
                context.area_width(),
                context.area_height(),
            ),
        });
        let slice = fragment
            .clone()
            .with_primitives_clipped_to_physical_axis_range_preserving_cross_axis_overflow(
                css::PhysicalAxis::Horizontal,
                projection.source_clip(),
                true,
            )
            .translated(projection.destination_translation());
        if !slice.is_empty() {
            slices.push((snapshot.pages.len() + slice_index, slice));
        }
    }
    slices
}

fn promoted_spanner_paint_slices(
    first_page_index: usize,
    last_page_index: usize,
    block_top: f32,
    block_bottom: f32,
    first_context: PageContext,
    last_context: PageContext,
    fragmentainer_override: Option<FragmentainerOverride>,
) -> Vec<PromotedSpannerPaintSlice> {
    let continuation_context = fragmentainer_override
        .map(|override_| override_.context)
        .unwrap_or(last_context);
    (first_page_index..=last_page_index)
        .filter_map(|page_index| {
            let context = if page_index == first_page_index {
                first_context
            } else if page_index == last_page_index {
                last_context
            } else {
                continuation_context
            };
            let top = if page_index == first_page_index {
                block_top
            } else {
                context.top()
            };
            let bottom = if page_index == last_page_index {
                block_bottom
            } else {
                context.bottom()
            };
            (top > bottom + 0.01).then_some(PromotedSpannerPaintSlice {
                page_index,
                top,
                bottom,
                owns_block_start: page_index == first_page_index,
                owns_block_end: page_index == last_page_index,
            })
        })
        .collect()
}

pub(in crate::layout) fn suppress_fragmented_box_edges(
    style: &mut ComputedStyle,
    owns_block_start: bool,
    owns_block_end: bool,
) {
    if style.box_decoration_break == css::BoxDecorationBreak::Clone {
        return;
    }
    if !owns_block_start {
        suppress_promoted_spanner_physical_edge(style, block_start_side(style.writing_mode));
    }
    if !owns_block_end {
        suppress_promoted_spanner_physical_edge(style, block_end_side(style.writing_mode));
    }
}

/// Remove the box background that CSS Backgrounds propagates to the document
/// canvas while retaining borders and other fragment-local decoration.
///
/// A propagated root/body background is painted once in the canvas coordinate
/// system; cloning it into every page fragment would re-anchor image layers:
/// <https://www.w3.org/TR/css-backgrounds-3/#special-backgrounds>.
fn suppress_document_canvas_background(style: &mut ComputedStyle) {
    style.background.background_color = css::BackgroundColor::TRANSPARENT;
    style.background.background_image = css::ComputedImage::None;
    style.background.background_layers.clear();
}

fn suppress_promoted_spanner_physical_edge(style: &mut ComputedStyle, side: PhysicalSide) {
    let zero = css::ComputedLengthPercentage::ZERO;
    match side {
        PhysicalSide::Top => {
            style.padding.top = 0.0;
            style.border_widths.top = 0.0;
            style.box_values.padding.top = zero.clone();
            style.border_width_values.top = zero;
            style.border_styles.top = css::BorderStyle::None;
        }
        PhysicalSide::Right => {
            style.padding.right = 0.0;
            style.border_widths.right = 0.0;
            style.box_values.padding.right = zero.clone();
            style.border_width_values.right = zero;
            style.border_styles.right = css::BorderStyle::None;
        }
        PhysicalSide::Bottom => {
            style.padding.bottom = 0.0;
            style.border_widths.bottom = 0.0;
            style.box_values.padding.bottom = zero.clone();
            style.border_width_values.bottom = zero;
            style.border_styles.bottom = css::BorderStyle::None;
        }
        PhysicalSide::Left => {
            style.padding.left = 0.0;
            style.border_widths.left = 0.0;
            style.box_values.padding.left = zero.clone();
            style.border_width_values.left = zero;
            style.border_styles.left = css::BorderStyle::None;
        }
    }
}

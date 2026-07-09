use super::*;
use crate::css::Edges;
use crate::layout::block::float::FLOAT_EPSILON;
use crate::layout::inline_collect::IntrinsicInlineCollectionContext;

mod dom;
mod formatting_boxes;
pub(in crate::layout) mod shared;
pub(in crate::layout) mod state;

use state::{BlockFlowChildTraversalState, ChildFlowTraversalOutcome};

#[cfg(test)]
use shared::apply_pending_normal_flow_margin_before_float;

impl<'a> LayoutBuilder<'a> {
    #[expect(
        clippy::boxed_local,
        reason = "This large debug-build layout frame stays under the normal worker stack limit."
    )]
    pub(in crate::layout) fn layout_block_flow_children_phase(
        &mut self,
        input: Box<BlockFlowChildrenPhaseInput<'_, '_>>,
    ) -> BlockFlowChildrenPhaseOutcome {
        let BlockFlowChildrenPhaseInput {
            fragmentainer_kind,
            element,
            style,
            stylesheets,
            child_boxes,
            can_collapse_start_margin,
            can_collapse_end_margin,
            applied_start_margin,
            starts_at_page_top,
            laid_out_column_children,
            use_box_inline_items,
            use_ordered_mixed_flow,
            definite_content_height,
            descendant_percentage_height_basis,
        } = *input;
        let mut traversal_state = BlockFlowChildTraversalState::new(style);
        let descendant_percentage_height_basis =
            descendant_percentage_height_basis.unwrap_or_else(|| {
                block_size_percentage_basis_from_points(
                    definite_content_height,
                    BlockSizeBasisSource::ContainingBlock,
                )
            });
        self.definite_block_size_stack
            .push(descendant_percentage_height_basis);

        let traversal_outcome = if laid_out_column_children || use_box_inline_items {
            ChildFlowTraversalOutcome::default()
        } else if use_ordered_mixed_flow {
            ChildFlowTraversalOutcome {
                pending_end_margin_collapse: self.layout_ordered_mixed_flow_children(
                    element,
                    style,
                    stylesheets,
                    can_collapse_start_margin,
                    can_collapse_end_margin,
                    &mut traversal_state,
                ),
                collapsed_start_margin_offset: layout_pt(0.0),
            }
        } else if let Some(child_boxes) = child_boxes {
            self.layout_formatting_box_flow_children(
                fragmentainer_kind,
                element,
                style,
                stylesheets,
                child_boxes,
                can_collapse_start_margin,
                can_collapse_end_margin,
                applied_start_margin,
                starts_at_page_top,
                &mut traversal_state,
            )
        } else {
            self.layout_dom_flow_children(
                fragmentainer_kind,
                element,
                style,
                stylesheets,
                can_collapse_start_margin,
                can_collapse_end_margin,
                applied_start_margin,
                starts_at_page_top,
                &mut traversal_state,
            )
        };
        self.definite_block_size_stack.pop();

        BlockFlowChildrenPhaseOutcome {
            pending_end_margin_collapse: traversal_outcome.pending_end_margin_collapse,
            collapsed_start_margin_offset: traversal_outcome.collapsed_start_margin_offset,
            descendant_clamp_line_slots: traversal_state.descendant_clamp_line_slots(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::css::LineClamp;
    use crate::{Html, RenderOptions};

    async fn rendered_text(source: &str) -> String {
        let document = Html::from_string(source)
            .render(&RenderOptions::default())
            .await
            .unwrap();
        document
            .pages
            .iter()
            .flat_map(|page| page.lines())
            .map(|line| line.text.as_str())
            .collect()
    }

    #[test]
    fn float_hypothetical_position_includes_pending_block_margin() {
        let mut style = ComputedStyle::initial();
        style.float = Float::Left;
        style.margin.top = 12.0;

        apply_pending_normal_flow_margin_before_float(&mut style, Some(18.0));
        assert_eq!(style.margin.top, 30.0);

        apply_pending_normal_flow_margin_before_float(&mut style, Some(-8.0));
        assert_eq!(style.margin.top, 22.0);
    }

    #[test]
    fn non_float_does_not_consume_pending_block_margin() {
        let mut style = ComputedStyle::initial();
        style.margin.top = 12.0;

        apply_pending_normal_flow_margin_before_float(&mut style, Some(18.0));
        assert_eq!(style.margin.top, 12.0);
    }

    #[tokio::test]
    async fn line_clamp_budget_continues_from_plain_text_into_a_block_child() {
        let text = rendered_text(
            "<style>@page { size: 160pt 120pt; margin: 10pt } \
             .clamp { line-clamp: 1; width: 100pt; font: 10pt/10pt monospace } \
             p { margin: 0 }</style>\
             <div class=\"clamp\">first text<p>second text</p></div>",
        )
        .await;

        assert!(text.contains("first"));
        assert!(!text.contains("second"), "clamped text={text:?}");
    }

    #[tokio::test]
    async fn fixed_height_child_does_not_consume_line_clamp_slots() {
        let text = rendered_text(
            "<style>@page { size: 160pt 160pt; margin: 10pt } \
             .clamp { line-clamp: 1; width: 100pt; font: 10pt/10pt monospace } \
             .spacer { height: 50pt } p { margin: 0 }</style>\
             <div class=\"clamp\"><div class=\"spacer\"></div><p>visible text</p></div>",
        )
        .await;

        assert!(text.contains("visible"), "clamped text={text:?}");
    }

    #[tokio::test]
    async fn nested_line_height_does_not_overdebit_parent_line_clamp() {
        let text = rendered_text(
            "<style>@page { size: 160pt 180pt; margin: 10pt } \
             .clamp { line-clamp: 2; width: 100pt; font: 10pt/10pt monospace } \
             .tall { font: 10pt/30pt monospace; margin: 0 } p { margin: 0 }</style>\
             <div class=\"clamp\"><div class=\"tall\">tall line</div><p>later line</p></div>",
        )
        .await;

        assert!(text.contains("tall"), "clamped text={text:?}");
        assert!(text.contains("later"), "clamped text={text:?}");
    }

    #[tokio::test]
    async fn ordered_mixed_inline_run_debits_parent_line_clamp() {
        let text = rendered_text(
            "<style>@page { size: 160pt 120pt; margin: 10pt } \
             .clamp { line-clamp: 1; width: 100pt; font: 10pt/10pt monospace } \
             span { font-weight: bold } p { margin: 0 }</style>\
             <div class=\"clamp\"><span>first inline</span><p>later block</p></div>",
        )
        .await;

        assert!(text.contains("first"), "clamped text={text:?}");
        assert!(!text.contains("later"), "clamped text={text:?}");
    }

    #[test]
    fn avoid_replay_restores_the_saved_remaining_line_clamp_budget() {
        let mut style = ComputedStyle::initial();
        style.line_clamp = Some(LineClamp::new(3, false));
        let mut traversal_state = BlockFlowChildTraversalState::new(&style);

        traversal_state.debit(1);
        let saved_remaining = traversal_state.capture_avoid_replay();
        traversal_state.debit(2);
        traversal_state.restore_avoid_replay(saved_remaining);

        let mut replayed_style = ComputedStyle::initial();
        traversal_state.apply_to(&mut replayed_style);
        assert_eq!(
            replayed_style.line_clamp.map(|clamp| clamp.max_lines),
            Some(2)
        );
    }
}

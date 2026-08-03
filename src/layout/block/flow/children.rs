use super::*;
use crate::css::Edges;
use crate::layout::block::float::FLOAT_EPSILON;
use crate::layout::inline_collect::IntrinsicInlineCollectionContext;

mod dom;
mod formatting_boxes;
pub(in crate::layout) mod shared;
pub(in crate::layout) mod state;

use state::{BlockFlowChildTraversalState, ChildFlowTraversalOutcome};

/// Parent margin-collapse state supplied to a normal-flow child traversal.
///
/// Keeping the applied start margin and fragmentainer-top rule together avoids
/// passing a partially related set of scalar layout flags to specialized
/// traversal paths. CSS 2.2 defines the associated start/end collapsing rules
/// as one margin-collapsing operation.
/// <https://www.w3.org/TR/CSS22/box.html#collapsing-margins>
#[derive(Debug, Clone, Copy)]
pub(in crate::layout) struct BlockFlowMarginCollapseContext {
    pub(in crate::layout) can_collapse_start_margin: bool,
    pub(in crate::layout) can_collapse_end_margin: bool,
    pub(in crate::layout) applied_start_margin: LayoutLength,
    pub(in crate::layout) starts_at_page_top: bool,
}

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
            clearance_consumed_adjoining_start_margin,
            starts_at_page_top,
            laid_out_column_children,
            use_box_inline_items,
            run_in_inline_items_laid_out,
            use_ordered_mixed_flow,
            has_preceding_inline_flow_content,
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
                    BlockFlowMarginCollapseContext {
                        can_collapse_start_margin,
                        can_collapse_end_margin,
                        applied_start_margin,
                        starts_at_page_top,
                    },
                    &mut traversal_state,
                ),
                collapsed_start_margin_offset: layout_pt(0.0),
                rendered_legend: None,
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
                clearance_consumed_adjoining_start_margin,
                starts_at_page_top,
                has_preceding_inline_flow_content,
                run_in_inline_items_laid_out,
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
                clearance_consumed_adjoining_start_margin,
                starts_at_page_top,
                &mut traversal_state,
            )
        };
        self.definite_block_size_stack.pop();

        BlockFlowChildrenPhaseOutcome {
            pending_end_margin_collapse: traversal_outcome.pending_end_margin_collapse,
            collapsed_start_margin_offset: traversal_outcome.collapsed_start_margin_offset,
            rendered_legend: traversal_outcome.rendered_legend,
            descendant_clamp_line_slots: traversal_state.descendant_clamp_line_slots(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::css::{ComputedClampContinuation, ComputedLineClamp};
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
    async fn line_clamp_marks_a_terminal_child_line_when_a_later_block_is_discarded() {
        let text = rendered_text(
            "<style>@page { size: 160pt 120pt; margin: 10pt } \
             .clamp { line-clamp: 1; width: 100pt; font: 10pt/10pt monospace } \
             p { margin: 0 }</style>\
             <div class=\"clamp\"><p>visible</p><p>discarded</p></div>",
        )
        .await;

        assert!(text.contains("visible…"), "clamped text={text:?}");
        assert!(!text.contains("discarded"), "clamped text={text:?}");
    }

    #[tokio::test]
    async fn line_clamp_marks_a_preserved_break_child_before_later_float_and_block_source() {
        let text = rendered_text(
            "<style>@page { size: 200pt 180pt; margin: 10pt } \
             .clamp { line-clamp: 4; width: 120pt; font: 10pt/10pt monospace } \
             .pre { white-space: pre } .float { float: left; width: 20pt; height: 20pt } \
             div { margin: 0 }</style>\
             <div class=\"clamp\"><div class=\"pre\">one\ntwo\nthree\nfour</div>\
             <div class=\"float\"></div><div>discarded</div></div>",
        )
        .await;

        assert!(text.contains("four…"), "clamped text={text:?}");
        assert!(!text.contains("discarded"), "clamped text={text:?}");
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

    #[tokio::test]
    async fn display_contents_generated_inline_content_keeps_its_source_order() {
        let text = rendered_text(
            r#"<style>
                @page { size: 160pt 120pt; margin: 10pt }
                p, div { margin: 0 }
                #create-counter { counter-reset: counter-of-span 9 }
                #test { contain: style; display: contents }
                #test span { counter-increment: counter-of-span 5 }
                #test span::after { content: counter(counter-of-span) }
               </style>
               <p>preceding block</p>
               <div id="create-counter"></div>
               <div id="test"><span></span></div>"#,
        )
        .await;

        let preceding_block = text.find("preceding block").unwrap();
        let counter = text.find("14").unwrap();
        assert!(
            preceding_block < counter,
            "flattened generated content was replayed before its source boundary: {text:?}"
        );
    }

    #[tokio::test]
    async fn terminal_collapsible_whitespace_does_not_create_a_clamp_point() {
        let text = rendered_text(
            r#"<style>
                @page { size: 320pt 120pt; margin: 10pt }
                .clamp { line-clamp: 2; width: 31.1ch; font-family: monospace }
                p { margin: 0 }
               </style>
               <div class="clamp"><p>
               There should not be an ellipsis
               at the end of this line of text
               </p></div>"#,
        )
        .await;

        assert!(!text.contains('…'), "clamped text={text:?}");
    }

    #[test]
    fn avoid_replay_restores_the_saved_remaining_line_clamp_budget() {
        let mut style = ComputedStyle::initial();
        style.line_clamp = Some(ComputedLineClamp::new(
            std::num::NonZeroUsize::new(3).unwrap(),
            ComputedClampContinuation::Collapse,
        ));
        let mut traversal_state = BlockFlowChildTraversalState::new(&style);

        traversal_state.debit(1);
        let saved_remaining = traversal_state.capture_avoid_replay();
        traversal_state.debit(2);
        traversal_state.restore_avoid_replay(saved_remaining);

        let mut replayed_style = ComputedStyle::initial();
        traversal_state.apply_to(&mut replayed_style);
        assert_eq!(
            replayed_style.used_line_clamp.map(|clamp| clamp.max_lines),
            Some(2),
        );
        assert_eq!(style.line_clamp.unwrap().max_lines.get(), 3);
    }

    #[test]
    fn multicol_container_does_not_create_a_used_line_clamp_budget() {
        let mut style = ComputedStyle::initial();
        style.line_clamp = Some(ComputedLineClamp::new(
            std::num::NonZeroUsize::new(2).unwrap(),
            ComputedClampContinuation::Collapse,
        ));
        style.column_count = css::ColumnCount::Count(std::num::NonZeroUsize::new(3).unwrap());

        let traversal_state = BlockFlowChildTraversalState::new(&style);
        assert!(!traversal_state.has_active_clamp());
    }
}

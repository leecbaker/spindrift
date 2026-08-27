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
        #[cfg(all(feature = "stack-profile", target_os = "macos"))]
        let _stack_profile_scope = stack_profile::enter("layout_block_flow_children_phase");
        let BlockFlowChildrenPhaseInput {
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
            traversal_mode,
            run_in_inline_items_laid_out,
            has_preceding_inline_flow_content,
            preceding_inline_local_cutoff,
            preceding_inline_clamp_block_advance,
            discard_region_limit,
            direct_automatic_block_size_constraint,
            definite_content_height,
            descendant_percentage_height_basis,
        } = *input;
        let can_collapse_start_margin =
            can_collapse_start_margin && start_margin_arrangement.permits_parent_start_collapse();
        let applied_start_margin = start_margin_arrangement
            .applied_start_margin()
            .unwrap_or_else(|| layout_pt(0.0));
        let mut traversal_state =
            BlockFlowChildTraversalState::new(style, direct_automatic_block_size_constraint);
        traversal_state.debit_automatic_block_contribution(preceding_inline_clamp_block_advance);
        if preceding_inline_local_cutoff {
            traversal_state.mark_local_continuation_cutoff();
        }
        traversal_state.set_discard_region_limit(discard_region_limit);
        let descendant_percentage_height_basis =
            descendant_percentage_height_basis.unwrap_or_else(|| {
                block_size_percentage_basis_from_points(
                    definite_content_height.map(|height| height.value().points()),
                    BlockSizeBasisSource::ContainingBlock,
                )
            });
        self.definite_block_size_stack
            .push(descendant_percentage_height_basis);

        let traversal_outcome = if laid_out_column_children
            || matches!(traversal_mode, ChildTraversalMode::InlineSequence)
        {
            ChildFlowTraversalOutcome::default()
        } else if matches!(traversal_mode, ChildTraversalMode::OrderedMixed) {
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
                adjoining_margin_set_boundary: BlockMarginCollapseBoundary::Adjoining,
                rendered_legend: None,
            }
        } else if matches!(traversal_mode, ChildTraversalMode::DirectFloatChildren) {
            // Formatting-tree anonymous inline wrappers must not turn source
            // whitespace next to a direct float into a parent line box. This
            // mode deliberately replays the direct children, where collapsed
            // whitespace is discarded before the float is placed.
            self.layout_dom_flow_children(
                fragmentainer_kind,
                element,
                style,
                stylesheets,
                can_collapse_start_margin,
                can_collapse_end_margin,
                applied_start_margin,
                start_margin_arrangement,
                starts_at_page_top,
                has_preceding_inline_flow_content,
                preceding_inline_clamp_block_advance,
                &mut traversal_state,
            )
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
                start_margin_arrangement,
                starts_at_page_top,
                has_preceding_inline_flow_content,
                preceding_inline_clamp_block_advance,
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
                start_margin_arrangement,
                starts_at_page_top,
                has_preceding_inline_flow_content,
                preceding_inline_clamp_block_advance,
                &mut traversal_state,
            )
        };
        self.definite_block_size_stack.pop();

        BlockFlowChildrenPhaseOutcome {
            pending_end_margin_collapse: traversal_outcome.pending_end_margin_collapse,
            collapsed_start_margin_offset: traversal_outcome.collapsed_start_margin_offset,
            adjoining_margin_set_boundary: traversal_outcome.adjoining_margin_set_boundary,
            rendered_legend: traversal_outcome.rendered_legend,
            descendant_clamp_line_slots: traversal_state.descendant_clamp_line_slots(),
            has_local_continuation_cutoff: traversal_state.has_local_continuation_cutoff(),
            discard_source_prefix: traversal_state.discard_source_prefix(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::css::{BlockEllipsis, Continue, MaxLines, PositiveLineCount, RemainingLineSlots};
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
    async fn generated_and_direct_float_children_keep_tree_source_order() {
        let document = Html::from_string(
            "<style>@page { size: 120pt 120pt; margin: 10pt } body { margin: 0 } \
             .parent { width: 60pt; background: yellow } \
             .parent::before { content: ''; display: block; float: left; width: 20pt; height: 20pt; background: blue } \
             .direct { float: left; clear: left; width: 20pt; height: 20pt; background: green } \
             .parent::after { content: ''; display: block; clear: both; width: 60pt; height: 10pt; background: red }</style>\
             <div class=\"parent\"><div class=\"direct\"></div></div>",
        )
        .render(&RenderOptions::default())
        .await
        .unwrap();

        let page = &document.pages[0];
        let blue = page
            .rects()
            .iter()
            .find(|rect| rect.fill == Some(CssColor::new(0, 0, 255)))
            .expect("generated ::before float should paint");
        let green = page
            .rects()
            .iter()
            .find(|rect| rect.fill == Some(CssColor::new(0, 128, 0)))
            .expect("direct float should paint");
        let red = page
            .rects()
            .iter()
            .find(|rect| rect.fill == Some(CssColor::new(255, 0, 0)))
            .expect("generated ::after clear box should paint");

        assert!(
            green.y() + green.height() <= blue.y() + 0.01,
            "the direct float must clear the preceding generated float: blue={blue:?}, green={green:?}"
        );
        assert!(
            red.y() + red.height() <= green.y() + 0.01,
            "the generated clear box must follow both floats: green={green:?}, red={red:?}"
        );
    }

    #[tokio::test]
    async fn direct_float_only_children_do_not_create_a_parent_inline_strut() {
        let document = Html::from_string(
            "<style>@page { size: 120pt 120pt; margin: 10pt } \
             body { margin: 0 } .parent { width: 80pt; font: 10pt/20pt sans-serif } \
             .float { display: inline-block; float: left; width: 20pt; height: 10pt; margin-top: 5pt; background: green } \
             .text { clear: both; margin-top: 20pt } .text .float { background: blue }</style>\
             <div class=\"parent\">\n  <span class=\"float\">float</span>\n</div>\
             <div class=\"parent text\">prefix<span class=\"float\">float</span></div>",
        )
        .render(&RenderOptions::default())
        .await
        .unwrap();

        let page = &document.pages[0];
        let green = page
            .rects()
            .iter()
            .find(|rect| rect.fill == Some(CssColor::new(0, 128, 0)))
            .expect("direct float should paint");
        assert!(
            (green.y() + green.height() - 105.0).abs() < 0.01,
            "a whitespace-only parent must place its float at content-start plus its own 5pt margin, not after a 20pt inline strut: green={green:?}"
        );
        assert!(
            page.lines().iter().any(|line| line.text.contains("prefix")),
            "real sibling inline content must still select the inline float-marker path"
        );
    }

    #[tokio::test]
    async fn automatic_line_clamp_selects_a_measured_terminal_line() {
        let text = rendered_text(
            "<style>@page { size: 160pt 120pt; margin: 10pt } \
             .clamp { line-clamp: auto; max-height: 40pt; width: 100pt; \
             font: 10pt/10pt monospace; white-space: pre } </style> \
             <div class=clamp>one\ntwo\nthree\nfour\nfive</div>",
        )
        .await;
        assert_eq!(text, "onetwothreefour…");
    }

    #[tokio::test]
    async fn automatic_line_clamp_resolves_lh_against_the_used_line_height() {
        let text = rendered_text(
            "<style>@page { size: 160pt 120pt; margin: 10pt } \
             .clamp { line-clamp: auto; max-height: 4lh; width: 100pt; \
             font: 10pt/10pt monospace; white-space: pre } </style> \
             <div class=clamp>one\ntwo\nthree\nfour\nfive</div>",
        )
        .await;
        assert_eq!(text, "onetwothreefour…");
    }

    #[tokio::test]
    async fn automatic_line_clamp_uses_min_height_when_it_exceeds_max_height() {
        let text = rendered_text(
            "<style>@page { size: 160pt 160pt; margin: 10pt } \
             .clamp { line-clamp: auto; min-height: 4lh; max-height: 3lh; \
             width: 100pt; font: 10pt/10pt monospace; white-space: pre } </style> \
             <div class=clamp>one\ntwo\nthree\nfour\nfive\nsix</div>",
        )
        .await;

        assert!(text.contains("four…"), "clamped text={text:?}");
        assert!(!text.contains("five"), "clamped text={text:?}");
    }

    #[tokio::test]
    async fn automatic_line_clamp_propagates_a_typed_block_constraint_to_a_descendant() {
        let text = rendered_text(
            "<style>@page { size: 160pt 160pt; margin: 10pt } \
             .clamp { line-clamp: auto; max-height: 40pt; width: 100pt; \
             font: 10pt/10pt monospace; white-space: pre } </style> \
             <div class=clamp><div>one\ntwo\nthree\nfour\nfive</div></div>",
        )
        .await;

        assert!(text.contains("four…"), "clamped text={text:?}");
        assert!(!text.contains("five"), "clamped text={text:?}");
    }

    #[tokio::test]
    async fn automatic_line_clamp_uses_an_independent_child_as_a_block_boundary() {
        let text = rendered_text(
            "<style>@page { size: 160pt 220pt; margin: 10pt } \
             .clamp { line-clamp: auto; max-height: 6lh; width: 100pt; \
             font: 10pt/10pt monospace; white-space: pre } \
             .isolated { display: flow-root; margin: 0 }</style> \
             <div class=clamp>one\ntwo\nthree\nfour\n\
             <div class=isolated>five\nsix</div><div class=isolated>seven\neight</div>nine</div>",
        )
        .await;

        assert!(text.contains("six"), "clamped text={text:?}");
        assert!(!text.contains("seven"), "clamped text={text:?}");
        assert!(!text.contains("nine"), "clamped text={text:?}");
    }

    #[tokio::test]
    async fn automatic_line_clamp_replays_the_preceding_inline_endpoint_before_a_fixed_child() {
        let text = rendered_text(
            "<style>@page { size: 160pt 220pt; margin: 10pt } \
             .clamp { line-clamp: auto; max-height: 6lh; width: 100pt; \
             font: 10pt/10pt monospace; white-space: pre } \
             .fixed { height: 3lh }</style> \
             <div class=clamp>one\ntwo\nthree\nfour\n<div class=fixed>five\nsix</div>seven</div>",
        )
        .await;

        assert!(text.contains("four…"), "clamped text={text:?}");
        assert!(!text.contains("five"), "clamped text={text:?}");
        assert!(!text.contains("seven"), "clamped text={text:?}");
    }

    #[tokio::test]
    async fn automatic_line_clamp_replays_before_a_nested_minimum_height() {
        let text = rendered_text(
            "<style>@page { size: 160pt 220pt; margin: 10pt } \
             .clamp { line-clamp: auto; max-height: 6lh; width: 100pt; \
             font: 10pt/10pt monospace; white-space: pre } \
             .minimum { min-height: 3lh }</style> \
             <div class=clamp>one\ntwo\nthree\nfour\n<div class=minimum>five\nsix</div>seven</div>",
        )
        .await;

        assert!(text.contains("four…"), "clamped text={text:?}");
        assert!(!text.contains("five"), "clamped text={text:?}");
        assert!(!text.contains("seven"), "clamped text={text:?}");
    }

    #[tokio::test]
    async fn discard_captures_an_unforced_local_region_break_without_pagination() {
        let text = rendered_text(
            "<style>@page { size: 160pt 120pt; margin: 10pt } \
             .discard { continue: discard; block-ellipsis: auto; max-height: 40pt; \
             width: 100pt; font: 10pt/10pt monospace; white-space: pre } </style> \
             <div class=discard>one\ntwo\nthree\nfour\nfive</div>",
        )
        .await;
        assert_eq!(text, "onetwothreefour…");
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
    async fn independently_cascaded_discard_longhands_use_the_line_limit_cutoff() {
        let text = rendered_text(
            "<style>@page { size: 160pt 120pt; margin: 10pt } \
             .clamp { max-lines: 1; block-ellipsis: \" [more]\"; continue: discard; \
                       width: 100pt; font: 10pt/10pt monospace } p { margin: 0 }</style>\
             <div class=\"clamp\"><p>visible</p><p>discarded</p></div>",
        )
        .await;

        assert!(text.contains("visible [more]"), "clamped text={text:?}");
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
    async fn automatic_inline_cutoff_discards_following_block_source_locally() {
        let text = rendered_text(
            r#"<style>@page { size: 160pt 120pt; margin: 10pt }
             .clamp { line-clamp: auto; max-height: 20pt; width: 100pt;
                      font: 10pt/10pt monospace }
             p { margin: 0 }</style>
             <div class=clamp>one<br>two<br>three<p>discarded block</p></div>"#,
        )
        .await;

        assert!(text.contains("two…"), "clamped text={text:?}");
        assert!(!text.contains("three"), "clamped text={text:?}");
        assert!(!text.contains("discarded"), "clamped text={text:?}");
    }

    #[tokio::test]
    async fn automatic_line_clamp_resolves_a_definite_percentage_block_constraint() {
        let text = rendered_text(
            r#"<style>@page { size: 160pt 160pt; margin: 10pt }
             .outer { height: 60pt }
             .clamp { line-clamp: auto; max-height: 50%; width: 100pt;
                      font: 10pt/10pt monospace; white-space: pre }</style>
             <div class=outer><div class=clamp>one
two
three
four</div></div>"#,
        )
        .await;

        assert!(text.contains("three…"), "clamped text={text:?}");
        assert!(!text.contains("four"), "clamped text={text:?}");
    }

    #[tokio::test]
    async fn unforced_discard_break_does_not_advance_to_a_page_or_block_sibling() {
        let text = rendered_text(
            r#"<style>@page { size: 160pt 120pt; margin: 10pt }
             .discard { continue: discard; block-ellipsis: auto; max-height: 20pt; width: 100pt;
                        font: 10pt/10pt monospace }
             p { margin: 0 }</style>
             <div class=discard>one<br>two<br>three<p>discarded block</p></div>"#,
        )
        .await;

        assert!(text.contains("two…"), "discarded text={text:?}");
        assert!(!text.contains("three"), "discarded text={text:?}");
        assert!(!text.contains("discarded"), "discarded text={text:?}");
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
        style.max_lines = MaxLines::Lines(std::num::NonZeroUsize::new(3).unwrap());
        style.block_ellipsis = BlockEllipsis::Auto;
        style.continue_ = Continue::Collapse;
        let mut traversal_state = BlockFlowChildTraversalState::new(&style, None);

        traversal_state.debit(PositiveLineCount::from_rendered_slots(1).unwrap());
        let saved_remaining = traversal_state.capture_avoid_replay();
        traversal_state.debit(PositiveLineCount::from_rendered_slots(2).unwrap());
        traversal_state.restore_avoid_replay(saved_remaining);

        let mut replayed_style = ComputedStyle::initial();
        traversal_state.apply_to(&mut replayed_style);
        assert_eq!(
            replayed_style
                .line_limit_traversal
                .map(|clamp| clamp.remaining),
            Some(RemainingLineSlots::Available(
                PositiveLineCount::from_rendered_slots(2).unwrap()
            )),
        );
        assert_eq!(
            style.max_lines,
            MaxLines::Lines(std::num::NonZeroUsize::new(3).unwrap())
        );
    }

    #[test]
    fn nested_shared_block_flow_preserves_the_remaining_line_clamp_budget() {
        let mut root_style = ComputedStyle::initial();
        root_style.max_lines = MaxLines::Lines(std::num::NonZeroUsize::new(3).unwrap());
        root_style.block_ellipsis = BlockEllipsis::Auto;
        root_style.continue_ = Continue::Collapse;
        let mut root_traversal = BlockFlowChildTraversalState::new(&root_style, None);
        root_traversal.debit(PositiveLineCount::from_rendered_slots(1).unwrap());

        let mut child_style = ComputedStyle::initial();
        root_traversal.apply_to(&mut child_style);
        let nested_traversal = BlockFlowChildTraversalState::new(&child_style, None);

        assert_eq!(
            nested_traversal.remaining_line_slots(),
            Some(RemainingLineSlots::Available(
                PositiveLineCount::from_rendered_slots(2).unwrap()
            )),
        );
    }

    #[test]
    fn multicol_container_does_not_create_a_used_line_clamp_budget() {
        let mut style = ComputedStyle::initial();
        style.max_lines = MaxLines::Lines(std::num::NonZeroUsize::new(2).unwrap());
        style.block_ellipsis = BlockEllipsis::Auto;
        style.continue_ = Continue::Collapse;
        style.column_count = css::ColumnCount::Count(std::num::NonZeroUsize::new(3).unwrap());

        let traversal_state = BlockFlowChildTraversalState::new(&style, None);
        assert!(!traversal_state.has_active_clamp());
    }
}

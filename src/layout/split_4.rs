#[cfg(test)]
mod tests {
    use super::super::*;
    use crate::css::{
        BoxDecorationBreak, ComputedLengthPercentage, Hyphens, TextAlignLast, TextBoxEdge,
        TextBoxTrim, TextEdgeMetric, TextEdgePair, TextOrientation,
    };

    fn test_layout_builder<'a>(
        options: &'a RenderOptions,
        stylesheets: &'a [Stylesheet],
        resource_cache: &'a ResourceCache,
    ) -> LayoutBuilder<'a> {
        LayoutBuilder::new(LayoutBuilderConfig {
            options,
            stylesheets,
            base_url: None,
            root_url: None,
            resource_cache,
            page_progression_direction: Direction::Ltr,
            page_counter_initial_values: HashMap::new(),
            font_system: FontSystem::new(),
        })
    }

    fn inline_fragment(text: &str, style: ComputedStyle) -> InlineFragment {
        InlineFragment::new(
            text,
            style,
            0.0,
            None,
            true,
            InlineTextSource::Normal,
            false,
            InlineHangingEdges::default(),
            Vec::new(),
        )
    }

    fn inline_word(text: &str, style: &ComputedStyle) -> InlineItem {
        InlineItem::Word(Box::new(InlineWord {
            text: text.to_string(),
            style: inline_style(style),
            baseline_shift: 0.0,
            visual_offset: InlineVisualOffset::zero(),
            link_target: None,
            mergeable: true,
            source: InlineTextSource::Normal,
            hanging_edges: InlineHangingEdges::default(),
            ancestor_inline_decorations: Vec::new(),
        }))
    }

    fn inline_box_edge(width: f32, style: &ComputedStyle) -> InlineItem {
        InlineItem::Atom(Box::new(InlineAtom::new(
            InlineAtomContent::InlineEdge(InlineEdgeRole::BoxEdge(InlineBoxEdgeFragment {
                logical_edge: InlineLogicalEdge::End,
                physical_side: inline_end_side(style.writing_mode, style.direction),
                advance: width,
                paint_extent: width.max(0.0),
            })),
            style.clone(),
            None,
            width,
            style.line_height,
            style.font_size,
            0.0,
            None,
            None,
        )))
    }

    fn measured_inline_box_edge(
        boundary: usize,
        logical_edge: InlineLogicalEdge,
        physical_side: PhysicalSide,
        style: &ComputedStyle,
    ) -> inline_layout::RangedMeasuredMixedInlineLineItem {
        inline_layout::RangedMeasuredMixedInlineLineItem {
            item: inline_layout::MeasuredInlineItem {
                item: InlineLineItem::Atom(InlineAtom::new(
                    InlineAtomContent::InlineEdge(InlineEdgeRole::BoxEdge(InlineBoxEdgeFragment {
                        logical_edge,
                        physical_side,
                        advance: 5.0,
                        paint_extent: 5.0,
                    })),
                    style.clone(),
                    None,
                    5.0,
                    style.line_height,
                    style.font_size,
                    0.0,
                    None,
                    None,
                )),
                width: 5.0,
                shaped: None,
            },
            range: boundary..boundary,
        }
    }

    fn ranged_fragment(
        text: &str,
        range: std::ops::Range<usize>,
        style: &ComputedStyle,
    ) -> inline_layout::RangedMeasuredMixedInlineLineItem {
        inline_layout::RangedMeasuredMixedInlineLineItem {
            item: inline_layout::MeasuredInlineItem {
                item: InlineLineItem::Fragment(inline_fragment(text, style.clone())),
                width: text.len() as f32,
                shaped: None,
            },
            range,
        }
    }

    #[test]
    fn mixed_inline_visual_ranges_split_at_sibling_box_edge_boundary() {
        let mut style = ComputedStyle::initial();
        style.direction = Direction::Rtl;
        let ranged_items = vec![
            measured_inline_box_edge(0, InlineLogicalEdge::Start, PhysicalSide::Right, &style),
            ranged_fragment("inspect", 0..7, &style),
            measured_inline_box_edge(7, InlineLogicalEdge::End, PhysicalSide::Left, &style),
            ranged_fragment("pause", 7..12, &style),
        ];

        let split = inline_layout::split_mixed_inline_visual_ranges_at_box_edges(
            std::iter::once(0..12).collect(),
            &ranged_items,
            "inspectpause",
        );

        assert_eq!(split, vec![0..7, 7..12]);
    }

    #[test]
    fn mixed_inline_visual_ranges_split_before_neutral_space_between_siblings() {
        let mut style = ComputedStyle::initial();
        style.direction = Direction::Rtl;
        let ranged_items = vec![
            measured_inline_box_edge(0, InlineLogicalEdge::Start, PhysicalSide::Right, &style),
            ranged_fragment("inspect", 0..7, &style),
            measured_inline_box_edge(7, InlineLogicalEdge::End, PhysicalSide::Left, &style),
            ranged_fragment(" ", 7..8, &style),
            ranged_fragment("pause", 8..13, &style),
        ];

        let split = inline_layout::split_mixed_inline_visual_ranges_at_box_edges(
            std::iter::once(0..13).collect(),
            &ranged_items,
            "inspect pause",
        );

        assert_eq!(split, vec![0..7, 7..13]);
    }

    #[test]
    fn mixed_inline_visual_ranges_do_not_split_nested_child_without_box_edge() {
        let mut style = ComputedStyle::initial();
        style.direction = Direction::Rtl;
        let ranged_items = vec![
            measured_inline_box_edge(0, InlineLogicalEdge::Start, PhysicalSide::Right, &style),
            ranged_fragment("inspect", 0..7, &style),
            ranged_fragment("pause", 7..12, &style),
            measured_inline_box_edge(12, InlineLogicalEdge::End, PhysicalSide::Left, &style),
        ];

        let split = inline_layout::split_mixed_inline_visual_ranges_at_box_edges(
            std::iter::once(0..12).collect(),
            &ranged_items,
            "inspectpause",
        );

        assert_eq!(split, vec![0..12]);
    }

    #[test]
    fn inline_text_metrics_separate_content_area_from_line_height() {
        let options = RenderOptions::default();
        let stylesheets = Vec::new();
        let resource_cache = ResourceCache::default();
        let mut builder = test_layout_builder(&options, &stylesheets, &resource_cache);
        let mut tall = ComputedStyle::initial();
        tall.font_family = css::FontFamily::SansSerif;
        tall.font_size = 50.0;
        tall.line_height = 200.0;
        tall.line_height_is_normal = false;

        let mut short = tall.clone();
        short.line_height = 30.0;

        let tall_metrics = builder.inline_text_box_metrics(&tall, None, 0.0);
        let short_metrics = builder.inline_text_box_metrics(&short, None, 0.0);

        assert!((tall_metrics.content_block_size - 50.0).abs() < 0.01);
        assert!((short_metrics.content_block_size - 50.0).abs() < 0.01);
        assert!((tall_metrics.half_leading - 75.0).abs() < 0.01);
        assert!((short_metrics.half_leading + 10.0).abs() < 0.01);
        assert!(
            (tall_metrics.content_baseline_offset - short_metrics.content_baseline_offset).abs()
                < 0.01
        );
        assert!(
            (tall_metrics.line_baseline_offset - short_metrics.line_baseline_offset - 85.0).abs()
                < 0.01
        );
    }

    fn inline_test_atom(width: f32, style: &ComputedStyle) -> InlineItem {
        InlineItem::Atom(Box::new(InlineAtom::new(
            InlineAtomContent::InlineBox {
                sequence: empty_inline_sequence(),
            },
            style.clone(),
            None,
            width,
            0.0,
            0.0,
            0.0,
            None,
            None,
        )))
    }

    fn inline_test_float(style: &ComputedStyle) -> InlineItem {
        let mut style = style.clone();
        style.float = Float::Left;
        let NodeKind::Element(element) = Node::element("span").kind else {
            unreachable!("element constructor should produce an element")
        };
        let signature = ElementSignature::new(element.tag.clone(), element.attrs.clone());
        InlineItem::Float(Box::new(InlineFloat::new(element, signature, style, None)))
    }

    fn inline_leader(pattern: &str, style: &ComputedStyle) -> InlineItem {
        InlineItem::Atom(Box::new(InlineAtom::new(
            InlineAtomContent::Leader(pattern.to_string()),
            style.clone(),
            None,
            0.0,
            style.line_height,
            style.font_size,
            0.0,
            Some("https://example.test/".to_string()),
            None,
        )))
    }

    fn list_marker_text(text: &str, style: &ComputedStyle, suffix_space: bool) -> ListMarker {
        ListMarker {
            text: text.to_string(),
            image: None,
            style: style.clone(),
            position: ListStylePosition::Inside,
            positioning_direction: style.direction,
            suffix_space,
        }
    }

    fn list_marker_image(width: f32, height: f32, style: &ComputedStyle) -> ListMarker {
        ListMarker {
            text: String::new(),
            image: Some(MarkerImage {
                decoded: DecodedPngImage {
                    pixel_width: 1,
                    pixel_height: 1,
                    rgb: vec![0, 0, 0],
                    alpha: None,
                },
                width,
                height,
            }),
            style: style.clone(),
            position: ListStylePosition::Inside,
            positioning_direction: style.direction,
            suffix_space: true,
        }
    }

    fn empty_inline_sequence() -> inline_layout::InlineLineSequence {
        inline_layout::InlineLineSequence {
            records: Vec::new(),
            available_width: 0.0,
            padding_left: 0.0,
            hanging_indent: 0.0,
            hanging_punctuation_reserve: 0.0,
            fragment_text_box_trim: TextBoxLineTrim::default(),
        }
    }

    fn inline_item_boundary_roles(items: &[InlineItem]) -> Vec<InlineBoundaryRole> {
        items.iter().map(inline_item_boundary_role).collect()
    }

    fn normalized_inline_item_text(items: &mut Vec<InlineItem>) -> String {
        inline_collect::normalize_inline_whitespace_items(items);
        items
            .iter()
            .map(|item| match item {
                InlineItem::Word(word) => word.text.clone(),
                InlineItem::Break(_) => "|".to_string(),
                InlineItem::Atom(_) => "\u{fffc}".to_string(),
                InlineItem::Float(_) | InlineItem::PageScopeStart(_) | InlineItem::PageScopeEnd => {
                    String::new()
                }
            })
            .collect()
    }

    fn normalized_inline_word_text(items: &mut Vec<InlineItem>) -> String {
        inline_collect::normalize_inline_whitespace_items(items);
        items
            .iter()
            .filter_map(|item| match item {
                InlineItem::Word(word) => Some(word.text.as_str()),
                _ => None,
            })
            .collect()
    }

    fn raw_text_sequence(
        builder: &mut LayoutBuilder<'_>,
        text: &str,
        style: &ComputedStyle,
        available_width: f32,
    ) -> inline_layout::InlineLineSequence {
        builder.inline_line_sequence_for_raw_inline_text(text, style, available_width, 0.0, None)
    }

    #[test]
    fn cloned_text_box_trim_applies_to_slice_local_line_edges() {
        let mut style = ComputedStyle::initial();
        style.line_height = 100.0;
        let records = ["A", "B", "C", "D"]
            .into_iter()
            .map(|text| inline_line_record_for_items(Vec::new(), text, 10.0, 100.0, &style))
            .collect::<Vec<_>>();
        let sequence = inline_layout::InlineLineSequence {
            records,
            available_width: 100.0,
            padding_left: 0.0,
            hanging_indent: 0.0,
            hanging_punctuation_reserve: 0.0,
            fragment_text_box_trim: TextBoxLineTrim {
                trims_block_start: true,
                trims_block_end: true,
                block_start: 10.0,
                block_end: 20.0,
            },
        };

        let (_, records) = sequence.fragment_records_for_slice_paint(0.0, -100.01, -300.0);
        assert_eq!(
            records
                .iter()
                .map(|record| record.fragment.as_ref().unwrap().text())
                .collect::<Vec<_>>(),
            ["B", "C"]
        );
        assert_eq!(records[0].block_start_trim, 10.0);
        assert_eq!(records[0].block_end_trim, 0.0);
        assert_eq!(records[1].block_start_trim, 0.0);
        assert_eq!(records[1].block_end_trim, 20.0);
    }

    #[tokio::test]
    async fn direct_text_sequence_applies_own_text_box_trim() {
        let options = RenderOptions::default();
        let stylesheets = Vec::new();
        let resource_cache = ResourceCache::default();
        let mut builder = test_layout_builder(&options, &stylesheets, &resource_cache);
        let mut style = ComputedStyle::initial();
        style.font_family = css::FontFamily::SansSerif;
        style.font_size = 50.0;
        style.line_height = 100.0;
        style.text_box_trim = TextBoxTrim::TrimEnd;
        style.text_box_edge = TextBoxEdge::Text(TextEdgePair::new(
            TextEdgeMetric::Text,
            TextEdgeMetric::Alphabetic,
        ));

        let sequence =
            builder.inline_line_sequence_for_raw_inline_text("A", &style, 200.0, 0.0, None);

        assert_eq!(sequence.records.len(), 1);
        assert!(
            sequence.records[0].block_end_trim > 0.0,
            "direct text sequence should apply its own text-box-trim"
        );
        assert!(sequence.total_height() < style.line_height);
    }

    #[tokio::test]
    async fn collected_inline_sequence_preserves_forced_break_clearance() {
        let options = RenderOptions::default();
        let stylesheets = Vec::new();
        let resource_cache = ResourceCache::default();
        let mut builder = test_layout_builder(&options, &stylesheets, &resource_cache);
        let mut style = ComputedStyle::initial();
        style.font_family = css::FontFamily::SansSerif;
        style.font_size = 12.0;
        style.line_height = 14.0;

        let sequence = builder.collect_inline_line_sequence(
            vec![
                inline_word("A", &style),
                InlineItem::Break(InlineBreak { clear: Clear::Both }),
            ],
            &style,
            100.0,
            0.0,
            0.0,
        );

        assert_eq!(sequence.records.len(), 1);
        assert_eq!(sequence.records[0].clear_after, Clear::Both);
        assert!(!sequence.records[0].is_forced_empty);
        assert!(sequence.records[0].fragment.is_some());
    }

    #[tokio::test]
    async fn intrinsic_inline_measurement_applies_own_text_box_trim() {
        let options = RenderOptions::default();
        let stylesheets = Vec::new();
        let resource_cache = ResourceCache::default();
        let mut builder = test_layout_builder(&options, &stylesheets, &resource_cache);
        let mut style = ComputedStyle::initial();
        style.font_family = css::FontFamily::SansSerif;
        style.font_size = 50.0;
        style.line_height = 100.0;
        style.text_box_trim = TextBoxTrim::TrimStart;
        style.text_box_edge = TextBoxEdge::Text(TextEdgePair::new(
            TextEdgeMetric::Cap,
            TextEdgeMetric::Alphabetic,
        ));

        let measurement = builder.intrinsic_inline_measurement_for_text("A B", &style, 30.0);

        assert_eq!(measurement.sequence.records.len(), 2);
        assert!(
            measurement.sequence.records[0].block_start_trim > 0.0,
            "intrinsic sequence should apply text-box-trim to the first formatted line"
        );
        assert_eq!(measurement.sequence.records[1].block_start_trim, 0.0);
        assert!(measurement.height() < style.line_height * 2.0);
    }

    #[tokio::test]
    async fn text_box_trim_skips_forced_empty_lines_when_selecting_formatted_edges() {
        let options = RenderOptions::default();
        let stylesheets = Vec::new();
        let resource_cache = ResourceCache::default();
        let mut builder = test_layout_builder(&options, &stylesheets, &resource_cache);
        let mut style = ComputedStyle::initial();
        style.font_family = css::FontFamily::SansSerif;
        style.font_size = 50.0;
        style.line_height = 100.0;
        style.text_box_trim = TextBoxTrim::TrimBoth;
        style.text_box_edge = TextBoxEdge::Text(TextEdgePair::new(
            TextEdgeMetric::Cap,
            TextEdgeMetric::Alphabetic,
        ));
        style.box_decoration_break = BoxDecorationBreak::Slice;

        let sequence = builder.collect_inline_line_sequence_with_text_box_trim(
            vec![
                InlineItem::Break(InlineBreak::default()),
                inline_word("A", &style),
                InlineItem::Break(InlineBreak::default()),
                InlineItem::Break(InlineBreak::default()),
            ],
            &style,
            200.0,
            0.0,
            0.0,
        );

        assert_eq!(sequence.records.len(), 3);
        assert!(sequence.records[0].fragment.is_none());
        assert!(sequence.records[0].is_forced_empty);
        assert!(sequence.records[2].fragment.is_none());
        assert!(sequence.records[2].is_forced_empty);
        assert!(sequence.records[1].fragment.is_some());
        assert!(
            sequence.records[1].block_start_trim > 0.0,
            "trim-start should apply to the first real formatted line"
        );
        assert!(
            sequence.records[1].block_end_trim > 0.0,
            "trim-end should apply to the last real formatted line"
        );
        assert_eq!(sequence.records[0].block_start_trim, 0.0);
        assert_eq!(sequence.records[0].block_end_trim, 0.0);
        assert_eq!(sequence.records[2].block_start_trim, 0.0);
        assert_eq!(sequence.records[2].block_end_trim, 0.0);
    }

    #[tokio::test]
    async fn vertical_text_box_trim_reduces_logical_block_line_size() {
        let options = RenderOptions::default();
        let stylesheets = Vec::new();
        let resource_cache = ResourceCache::default();
        let mut builder = test_layout_builder(&options, &stylesheets, &resource_cache);
        let mut style = ComputedStyle::initial();
        style.writing_mode = WritingMode::VerticalRl;
        style.font_family = css::FontFamily::SansSerif;
        style.font_size = 50.0;
        style.line_height = 100.0;
        style.text_box_trim = TextBoxTrim::TrimEnd;
        style.text_box_edge = TextBoxEdge::Text(TextEdgePair::new(
            TextEdgeMetric::Text,
            TextEdgeMetric::Alphabetic,
        ));

        let sequence =
            builder.inline_line_sequence_for_raw_inline_text("A", &style, 200.0, 0.0, None);

        assert_eq!(sequence.records.len(), 1);
        assert!(
            sequence.records[0].block_end_trim > 0.0,
            "vertical trim-end should still resolve on the logical block-end side"
        );
        assert!(
            sequence.total_height() < style.line_height,
            "vertical logical block line size should use the trimmed line height"
        );
    }

    #[tokio::test]
    async fn vertical_lr_text_box_trim_reduces_logical_block_line_size() {
        let options = RenderOptions::default();
        let stylesheets = Vec::new();
        let resource_cache = ResourceCache::default();
        let mut builder = test_layout_builder(&options, &stylesheets, &resource_cache);
        let mut style = ComputedStyle::initial();
        style.writing_mode = WritingMode::VerticalLr;
        style.font_family = css::FontFamily::SansSerif;
        style.font_size = 50.0;
        style.line_height = 100.0;
        style.text_box_trim = TextBoxTrim::TrimStart;
        style.text_box_edge =
            TextBoxEdge::Text(TextEdgePair::new(TextEdgeMetric::Cap, TextEdgeMetric::Text));

        let sequence =
            builder.inline_line_sequence_for_raw_inline_text("A", &style, 200.0, 0.0, None);

        assert_eq!(sequence.records.len(), 1);
        assert!(
            sequence.records[0].block_start_trim > 0.0,
            "vertical-lr trim-start should resolve on the logical block-start side"
        );
        assert!(
            sequence.total_height() < style.line_height,
            "vertical-lr logical block line size should use the trimmed line height"
        );
    }

    #[test]
    fn inline_block_sequence_baseline_offset_uses_trimmed_preceding_lines() {
        let options = RenderOptions::default();
        let stylesheets = Vec::new();
        let resource_cache = ResourceCache::default();
        let mut builder = test_layout_builder(&options, &stylesheets, &resource_cache);
        let mut style = ComputedStyle::initial();
        style.font_family = css::FontFamily::SansSerif;
        style.font_size = 50.0;
        style.line_height = 100.0;

        let first = inline_line_record_for_items(Vec::new(), "A", 10.0, 100.0, &style);
        let second = inline_line_record_for_items(Vec::new(), "B", 10.0, 100.0, &style);
        let untrimmed = inline_layout::InlineLineSequence {
            records: vec![first.clone(), second.clone()],
            available_width: 100.0,
            padding_left: 0.0,
            hanging_indent: 0.0,
            hanging_punctuation_reserve: 0.0,
            fragment_text_box_trim: TextBoxLineTrim::default(),
        };
        let mut trimmed_first = first;
        trimmed_first.block_start_trim = 10.0;
        let trimmed = inline_layout::InlineLineSequence {
            records: vec![trimmed_first, second],
            available_width: 100.0,
            padding_left: 0.0,
            hanging_indent: 0.0,
            hanging_punctuation_reserve: 0.0,
            fragment_text_box_trim: TextBoxLineTrim::default(),
        };

        let borders = css::Edges::ZERO;
        let untrimmed_offset = builder
            .inline_box_sequence_baseline_offset(&untrimmed, &style, borders)
            .expect("untrimmed inline-block sequence should expose a baseline");
        let trimmed_offset = builder
            .inline_box_sequence_baseline_offset(&trimmed, &style, borders)
            .expect("trimmed inline-block sequence should expose a baseline");

        assert!(
            (untrimmed_offset - trimmed_offset - 10.0).abs() < 0.01,
            "inline-block baseline offset should use trimmed preceding line height: untrimmed={untrimmed_offset}, trimmed={trimmed_offset}"
        );
    }

    fn sequence_fragment_texts(sequence: &inline_layout::InlineLineSequence) -> Vec<String> {
        sequence
            .records
            .iter()
            .map(|record| {
                record
                    .fragment
                    .as_ref()
                    .map(|fragment| fragment.text().to_string())
                    .unwrap_or_default()
            })
            .collect()
    }

    fn first_sequence_line_width(sequence: &inline_layout::InlineLineSequence) -> f32 {
        sequence.records[0]
            .fragment
            .as_ref()
            .expect("first selected line should carry a fragment")
            .metrics
            .width
    }

    #[test]
    fn inline_boundary_policy_classifies_text_transparent_and_opaque_boundaries() {
        let style = ComputedStyle::initial();
        let bidi_control = InlineItem::Word(Box::new(InlineWord {
            text: "\u{2067}".to_string(),
            style: inline_style(&style),
            baseline_shift: 0.0,
            visual_offset: InlineVisualOffset::zero(),
            link_target: None,
            mergeable: true,
            source: InlineTextSource::Normal,
            hanging_edges: InlineHangingEdges::default(),
            ancestor_inline_decorations: Vec::new(),
        }));

        assert_eq!(
            inline_item_boundary_role(&inline_word("text", &style)),
            InlineBoundaryRole::Text
        );
        assert_eq!(
            inline_item_boundary_role(&bidi_control),
            InlineBoundaryRole::TransparentTextBoundary
        );
        assert_eq!(
            inline_item_boundary_role(&inline_box_edge(0.0, &style)),
            InlineBoundaryRole::TransparentTextBoundary
        );
        assert_eq!(
            inline_item_boundary_role(&InlineItem::PageScopeStart(Some("chapter".to_string()))),
            InlineBoundaryRole::PageScopeStart
        );
        assert_eq!(
            inline_item_boundary_role(&inline_test_atom(5.0, &style)),
            InlineBoundaryRole::IndependentFormattingContext
        );
        assert_eq!(
            inline_atom_boundary_role(&InlineAtomContent::Leader(".".to_string())),
            InlineBoundaryRole::OpaqueAtomic
        );
        assert!(InlineBoundaryRole::PageScopeStart.is_transparent_to_whitespace());
        assert!(InlineBoundaryRole::OpaqueAtomic.resets_text_context());
    }

    #[test]
    fn whitespace_normalization_uses_boundary_policy_for_transparent_boundaries() {
        let style = ComputedStyle::initial();
        let mut latin_items = vec![
            inline_word("A\n", &style),
            InlineItem::PageScopeStart(Some("chapter".to_string())),
            inline_box_edge(0.0, &style),
            InlineItem::PageScopeEnd,
            inline_word("B", &style),
        ];
        let mut cjk_items = vec![
            inline_word("中\n", &style),
            InlineItem::PageScopeStart(Some("chapter".to_string())),
            inline_box_edge(0.0, &style),
            InlineItem::PageScopeEnd,
            inline_word("文", &style),
        ];

        assert_eq!(normalized_inline_word_text(&mut latin_items), "A B");
        assert_eq!(normalized_inline_word_text(&mut cjk_items), "中文");
    }

    #[test]
    fn whitespace_normalization_resets_context_at_opaque_atomic_boundaries() {
        let style = ComputedStyle::initial();
        let mut items = vec![
            inline_word("A\n", &style),
            inline_test_atom(5.0, &style),
            inline_word("  B", &style),
        ];

        assert_eq!(
            normalized_inline_item_text(&mut items),
            format!("A \u{fffc} B")
        );
    }

    #[test]
    fn whitespace_normalization_preserves_segment_break_between_atoms() {
        let style = ComputedStyle::initial();
        let mut items = vec![
            inline_test_atom(5.0, &style),
            inline_word("\n  ", &style),
            inline_test_atom(5.0, &style),
        ];

        assert_eq!(
            normalized_inline_item_text(&mut items),
            format!("\u{fffc} \u{fffc}")
        );
    }

    #[tokio::test]
    async fn inline_line_sequence_splits_at_page_scope_boundaries_without_graphing_controls() {
        let options = RenderOptions::default();
        let stylesheets = Vec::new();
        let resource_cache = ResourceCache::default();
        let mut builder = test_layout_builder(&options, &stylesheets, &resource_cache);
        let mut style = ComputedStyle::initial();
        style.font_family = css::FontFamily::SansSerif;
        style.font_size = 12.0;
        style.line_height = 14.0;
        let items = vec![
            inline_word("alpha", &style),
            InlineItem::PageScopeStart(Some("chapter".to_string())),
            inline_word("beta", &style),
            InlineItem::PageScopeEnd,
            inline_word("gamma", &style),
        ];

        let sequence = builder.collect_inline_line_sequence(items, &style, 400.0, 0.0, 0.0);

        assert_eq!(
            sequence_fragment_texts(&sequence),
            vec!["alpha", "beta", "gamma"]
        );
        assert_eq!(sequence.records[0].paragraph_index, 0);
        assert_eq!(sequence.records[1].paragraph_index, 1);
        assert_eq!(sequence.records[2].paragraph_index, 2);
    }

    fn prepared_visual_texts_for_sequence(
        builder: &mut LayoutBuilder<'_>,
        sequence: &inline_layout::InlineLineSequence,
        style: &ComputedStyle,
    ) -> Vec<String> {
        let context = inline_paragraph_context(style, sequence.available_width);
        let mut plaintext_state = None;
        sequence
            .records
            .iter()
            .filter_map(|record| {
                builder.prepare_inline_line_record(record, context, &mut plaintext_state)
            })
            .map(|prepared| {
                prepared_text_groups(&prepared)
                    .into_iter()
                    .map(|group| group.shaped.text.clone())
                    .collect::<String>()
            })
            .collect()
    }

    #[tokio::test]
    async fn wraps_at_word_boundaries() {
        assert_eq!(
            wrap_text("one two three", 7),
            vec!["one two".to_string(), "three".to_string()]
        );
        assert_eq!(wrap_text("Parent Child", 4), vec!["Parent", "Child"]);
    }

    #[tokio::test]
    async fn production_sequence_wraps_text_with_shaped_widths() {
        let options = RenderOptions::default();
        let stylesheets = Vec::new();
        let resource_cache = ResourceCache::default();
        let mut builder = test_layout_builder(&options, &stylesheets, &resource_cache);
        let mut style = ComputedStyle::initial();
        style.font_family = css::FontFamily::SansSerif;
        style.font_size = 12.0;
        style.line_height = 14.4;
        let available_width = builder.font_system.measure_text("one two", &style) + 0.1;
        assert!(builder.font_system.measure_text("one two three", &style) > available_width);

        let sequence = raw_text_sequence(&mut builder, "one two three", &style, available_width);

        assert_eq!(sequence_fragment_texts(&sequence), vec!["one two", "three"]);
        let prepared = prepared_visual_texts_for_sequence(&mut builder, &sequence, &style);
        assert_eq!(prepared, vec!["one two", "three"]);
    }

    #[tokio::test]
    async fn production_sequence_wraps_break_spaces_and_preserves_trailing_advance() {
        let options = RenderOptions::default();
        let stylesheets = Vec::new();
        let resource_cache = ResourceCache::default();
        let mut builder = test_layout_builder(&options, &stylesheets, &resource_cache);
        let mut style = ComputedStyle::initial();
        style.font_family = css::FontFamily::SansSerif;
        style.font_size = 12.0;
        style.line_height = 14.4;
        style.white_space = WhiteSpace::BreakSpaces;
        let available_width = builder.font_system.measure_text("A  ", &style) + 0.1;
        assert!(builder.font_system.measure_text("A   ", &style) > available_width);

        let wrapped = raw_text_sequence(&mut builder, "A   B", &style, available_width);
        let unwrapped = raw_text_sequence(&mut builder, "A  ", &style, 100.0);

        assert_eq!(sequence_fragment_texts(&wrapped).concat(), "A   B");
        assert!(wrapped.records.len() > 1);
        assert_eq!(sequence_fragment_texts(&unwrapped), vec!["A  "]);
        assert!(
            first_sequence_line_width(&unwrapped) > builder.font_system.measure_text("A", &style)
        );
    }

    #[tokio::test]
    async fn production_sequence_keeps_css_text_hanging_width_effects() {
        let options = RenderOptions::default();
        let stylesheets = Vec::new();
        let resource_cache = ResourceCache::default();
        let mut builder = test_layout_builder(&options, &stylesheets, &resource_cache);
        let mut normal = ComputedStyle::initial();
        normal.font_family = css::FontFamily::SansSerif;
        normal.font_size = 12.0;
        normal.line_height = 14.4;
        let available_width = builder.font_system.measure_text("X", &normal) + 0.1;

        let normal_sequence =
            raw_text_sequence(&mut builder, "X\u{3000}", &normal, available_width);
        assert_eq!(sequence_fragment_texts(&normal_sequence), vec!["X\u{3000}"]);
        assert!(
            (first_sequence_line_width(&normal_sequence)
                - builder.font_system.measure_text("X", &normal))
            .abs()
                < 0.01
        );

        let mut break_spaces = normal.clone();
        break_spaces.white_space = WhiteSpace::BreakSpaces;
        let break_spaces_sequence =
            raw_text_sequence(&mut builder, "X\u{3000}", &break_spaces, 500.0);
        assert_eq!(
            sequence_fragment_texts(&break_spaces_sequence),
            vec!["X\u{3000}"]
        );
        assert!(
            (first_sequence_line_width(&break_spaces_sequence)
                - builder.font_system.measure_text("X\u{3000}", &break_spaces))
            .abs()
                < 0.01
        );
    }

    #[tokio::test]
    async fn production_prepared_lines_apply_bidi_visual_order() {
        let options = RenderOptions::default();
        let stylesheets = Vec::new();
        let resource_cache = ResourceCache::default();
        let mut builder = test_layout_builder(&options, &stylesheets, &resource_cache);
        builder.cursor_y = 100.0;
        let mut style = ComputedStyle::initial();
        style.font_family = css::FontFamily::SansSerif;
        style.font_size = 12.0;
        style.line_height = 14.4;

        let ltr_sequence = raw_text_sequence(&mut builder, "abc אבג def", &style, 500.0);
        assert_eq!(
            prepared_visual_texts_for_sequence(&mut builder, &ltr_sequence, &style),
            vec!["abc גבא def"]
        );

        style.direction = Direction::Rtl;
        style.unicode_bidi = UnicodeBidi::BidiOverride;
        let override_sequence = raw_text_sequence(&mut builder, "abc def", &style, 500.0);
        let visual = prepared_visual_texts_for_sequence(&mut builder, &override_sequence, &style);
        assert_eq!(visual, vec!["fed cba"]);
        assert!(
            visual
                .iter()
                .all(|text| !text.chars().any(character_is_bidi_format_control))
        );
    }

    #[tokio::test]
    async fn production_sequence_uses_css_text_emergency_break_controls() {
        let options = RenderOptions::default();
        let stylesheets = Vec::new();
        let resource_cache = ResourceCache::default();
        let mut builder = test_layout_builder(&options, &stylesheets, &resource_cache);
        let mut normal = ComputedStyle::initial();
        normal.font_family = css::FontFamily::SansSerif;
        normal.font_size = 12.0;
        normal.line_height = 14.4;
        let available_width = builder.font_system.measure_text("abc", &normal) + 0.1;

        let normal_sequence = raw_text_sequence(&mut builder, "abcdefgh", &normal, available_width);
        assert_eq!(normal_sequence.records.len(), 1);

        let mut anywhere = normal.clone();
        anywhere.overflow_wrap = css::OverflowWrap::Anywhere;
        let anywhere_sequence =
            raw_text_sequence(&mut builder, "abcdefgh", &anywhere, available_width);
        assert!(anywhere_sequence.records.len() > 1);
        assert_eq!(
            sequence_fragment_texts(&anywhere_sequence).concat(),
            "abcdefgh"
        );

        let mut break_all = normal.clone();
        break_all.word_break = css::WordBreak::BreakAll;
        let break_all_sequence =
            raw_text_sequence(&mut builder, "abcdefgh", &break_all, available_width);
        assert!(break_all_sequence.records.len() > 1);
        assert_eq!(
            sequence_fragment_texts(&break_all_sequence).concat(),
            "abcdefgh"
        );

        let mut line_break_anywhere = normal.clone();
        line_break_anywhere.line_break = css::LineBreak::Anywhere;
        let anywhere_line_break_sequence = raw_text_sequence(
            &mut builder,
            "abcdefgh",
            &line_break_anywhere,
            available_width,
        );
        assert!(anywhere_line_break_sequence.records.len() > 1);
        assert_eq!(
            sequence_fragment_texts(&anywhere_line_break_sequence).concat(),
            "abcdefgh"
        );

        line_break_anywhere.white_space = WhiteSpace::Pre;
        assert_eq!(
            raw_text_sequence(&mut builder, " XXX", &line_break_anywhere, available_width)
                .records
                .len(),
            1
        );
        line_break_anywhere.white_space = WhiteSpace::NoWrap;
        assert_eq!(
            raw_text_sequence(
                &mut builder,
                "XXXX XX",
                &line_break_anywhere,
                available_width
            )
            .records
            .len(),
            1
        );
    }

    #[tokio::test]
    async fn production_sequence_handles_soft_hyphen_and_auto_hyphenation() {
        let options = RenderOptions::default();
        let stylesheets = Vec::new();
        let resource_cache = ResourceCache::default();
        let mut builder = test_layout_builder(&options, &stylesheets, &resource_cache);
        let mut style = ComputedStyle::initial();
        style.font_family = css::FontFamily::SansSerif;
        style.font_size = 12.0;
        style.line_height = 14.4;

        let unbroken = raw_text_sequence(&mut builder, "hyphen\u{00ad}ation", &style, 500.0);
        assert_eq!(sequence_fragment_texts(&unbroken), vec!["hyphenation"]);

        let available_width = builder.font_system.measure_text("hyphen", &style) + 0.1;
        let broken =
            raw_text_sequence(&mut builder, "hyphen\u{00ad}ation", &style, available_width);
        assert_eq!(sequence_fragment_texts(&broken).concat(), "hyphen-ation");

        style.hyphens = Hyphens::None;
        let suppressed =
            raw_text_sequence(&mut builder, "hyphen\u{00ad}ation", &style, available_width);
        assert_eq!(sequence_fragment_texts(&suppressed), vec!["hyphenation"]);

        let mut auto = style.clone();
        auto.hyphens = Hyphens::Auto;
        auto.language = Some("en".to_string());
        let auto_available_width = builder.font_system.measure_text("ribo", &auto) + 0.1;
        let auto_sequence =
            raw_text_sequence(&mut builder, "ribonuclease", &auto, auto_available_width);
        assert!(auto_sequence.records.len() > 1);
        assert!(
            sequence_fragment_texts(&auto_sequence)
                .iter()
                .any(|text| text.ends_with('-'))
        );
        assert_eq!(
            sequence_fragment_texts(&auto_sequence)
                .iter()
                .map(|text| text.replace('-', ""))
                .collect::<String>(),
            "ribonuclease"
        );

        auto.language = None;
        let unknown_language =
            raw_text_sequence(&mut builder, "ribonuclease", &auto, auto_available_width);
        assert_eq!(
            sequence_fragment_texts(&unknown_language),
            vec!["ribonuclease"]
        );
    }

    #[tokio::test]
    async fn production_sequence_handles_break_spaces_with_break_all() {
        let options = RenderOptions::default();
        let stylesheets = Vec::new();
        let resource_cache = ResourceCache::default();
        let mut builder = test_layout_builder(&options, &stylesheets, &resource_cache);
        let mut style = ComputedStyle::initial();
        style.white_space = WhiteSpace::BreakSpaces;
        style.word_break = css::WordBreak::BreakAll;
        style.font_family = css::FontFamily::SansSerif;
        style.font_size = 12.0;

        let available_width = builder.font_system.measure_text("  A", &style) + 0.1;
        let sequence = raw_text_sequence(&mut builder, "  AB", &style, available_width);
        assert_eq!(sequence_fragment_texts(&sequence).concat(), "  AB");
        assert!(sequence.records.len() > 1);

        let available_width = builder.font_system.measure_text("X XX", &style) + 0.1;
        let sequence = raw_text_sequence(&mut builder, "X XX X", &style, available_width);
        assert_eq!(sequence_fragment_texts(&sequence).concat(), "X XX X");
        assert!(sequence.records.len() > 1);

        style.line_break = css::LineBreak::Anywhere;
        let sequence = raw_text_sequence(&mut builder, "X XX X", &style, available_width);
        assert_eq!(sequence_fragment_texts(&sequence).concat(), "X XX X");
        assert!(sequence.records.len() > 1);
    }

    #[tokio::test]
    async fn css_whitespace_normalization_preserves_ideographic_spaces() {
        assert_eq!(
            collapse_whitespace("\u{3000}\u{3000}XX"),
            "\u{3000}\u{3000}XX"
        );
        assert_eq!(
            normalize_inline_text("\u{3000}\u{3000}XX"),
            "\u{3000}\u{3000}XX"
        );
        assert_eq!(normalize_inline_text("  XX  "), "XX");
    }

    #[tokio::test]
    async fn inline_whitespace_processor_collapses_across_transparent_edges() {
        let mut style = ComputedStyle::initial();
        style.font_family = css::FontFamily::SansSerif;
        let mut items = vec![
            inline_word("A ", &style),
            inline_box_edge(1.0, &style),
            inline_word("  B", &style),
        ];

        assert_eq!(normalized_inline_item_text(&mut items), "A \u{fffc}B");
    }

    #[tokio::test]
    async fn inline_whitespace_processor_treats_page_scopes_as_text_transparent() {
        let mut style = ComputedStyle::initial();
        style.font_family = css::FontFamily::SansSerif;
        let mut spaced = vec![
            inline_word("A ", &style),
            InlineItem::PageScopeStart(Some("named".to_string())),
            InlineItem::PageScopeEnd,
            inline_word("  B", &style),
        ];
        assert_eq!(normalized_inline_item_text(&mut spaced), "A B");

        let mut cjk = vec![
            inline_word("中\n", &style),
            InlineItem::PageScopeStart(Some("named".to_string())),
            InlineItem::PageScopeEnd,
            inline_word("文", &style),
        ];
        assert_eq!(normalized_inline_item_text(&mut cjk), "中文");
    }

    #[tokio::test]
    async fn inline_whitespace_processor_resets_across_real_atoms() {
        let mut style = ComputedStyle::initial();
        style.font_family = css::FontFamily::SansSerif;
        let mut items = vec![
            inline_word("A ", &style),
            inline_test_atom(4.0, &style),
            inline_word(" B", &style),
        ];

        assert_eq!(normalized_inline_item_text(&mut items), "A \u{fffc} B");
    }

    #[tokio::test]
    async fn inline_whitespace_processor_handles_pre_line_and_preserved_modes() {
        let mut pre_line = ComputedStyle::initial();
        pre_line.font_family = css::FontFamily::SansSerif;
        pre_line.white_space = WhiteSpace::PreLine;
        let mut pre_line_items = vec![inline_word("A   B\nC", &pre_line)];
        assert_eq!(normalized_inline_item_text(&mut pre_line_items), "A B|C");
        let mut consecutive_pre_line_items = vec![inline_word("A\n\nB", &pre_line)];
        assert_eq!(
            normalized_inline_item_text(&mut consecutive_pre_line_items),
            "A||B"
        );

        let mut pre_wrap = pre_line.clone();
        pre_wrap.white_space = WhiteSpace::PreWrap;
        let mut pre_wrap_items = vec![inline_word("A\t B\n", &pre_wrap)];
        assert_eq!(normalized_inline_item_text(&mut pre_wrap_items), "A\t B");

        let mut pre = pre_line.clone();
        pre.white_space = WhiteSpace::Pre;
        let mut pre_items = vec![inline_word("\n", &pre)];
        assert_eq!(normalized_inline_item_text(&mut pre_items), "|");
        assert_eq!(
            inline_collect::trim_terminal_preserved_segment_breaks("\n", &pre),
            "\n"
        );
        assert_eq!(
            inline_collect::trim_terminal_preserved_segment_breaks("A\n", &pre),
            "A"
        );

        let mut break_spaces = pre_line;
        break_spaces.white_space = WhiteSpace::BreakSpaces;
        let mut break_spaces_items = vec![inline_word("A  ", &break_spaces)];
        inline_collect::normalize_inline_whitespace_items(&mut break_spaces_items);
        assert_eq!(
            break_spaces_items
                .iter()
                .filter(|item| matches!(item, InlineItem::Word(_)))
                .count(),
            3
        );
    }

    #[tokio::test]
    async fn inline_whitespace_processor_replaces_visible_controls() {
        let mut style = ComputedStyle::initial();
        style.font_family = css::FontFamily::SansSerif;
        let mut items = vec![inline_word("A\u{0099}B", &style)];

        assert_eq!(normalized_inline_item_text(&mut items), "A\u{fffd}B");
    }

    #[tokio::test]
    async fn inline_whitespace_processor_transforms_segment_breaks_by_context() {
        let mut style = ComputedStyle::initial();
        style.font_family = css::FontFamily::SansSerif;
        let mut cjk = vec![inline_word("中文\n中文", &style)];
        let mut mixed = vec![inline_word("中文\nenglish", &style)];
        let mut latin = vec![inline_word("word\nword", &style)];

        assert_eq!(normalized_inline_item_text(&mut cjk), "中文中文");
        assert_eq!(normalized_inline_item_text(&mut mixed), "中文 english");
        assert_eq!(normalized_inline_item_text(&mut latin), "word word");
    }

    #[tokio::test]
    async fn inline_whitespace_processor_keeps_bidi_controls_transparent() {
        let mut style = ComputedStyle::initial();
        style.font_family = css::FontFamily::SansSerif;
        let mut items = vec![
            inline_word("中\n", &style),
            inline_word("\u{2066}", &style),
            inline_word("文", &style),
        ];

        assert_eq!(normalized_inline_item_text(&mut items), "中\u{2066}文");
    }

    #[tokio::test]
    async fn inside_marker_text_participates_in_shared_whitespace_context() {
        let options = RenderOptions::default();
        let stylesheets = Vec::new();
        let resource_cache = ResourceCache::default();
        let mut builder = test_layout_builder(&options, &stylesheets, &resource_cache);
        let mut style = ComputedStyle::initial();
        style.font_family = css::FontFamily::SansSerif;
        let marker = list_marker_text("中\n", &style, false);
        let mut items = Vec::new();

        builder.push_inside_marker_items(&marker, &style, None, &mut items);
        items.push(inline_word("文", &style));

        assert_eq!(
            inline_item_boundary_roles(&items),
            vec![InlineBoundaryRole::Text, InlineBoundaryRole::Text]
        );
        assert_eq!(normalized_inline_item_text(&mut items), "中文");
    }

    #[tokio::test]
    async fn inside_marker_image_resets_whitespace_context_as_atomic_boundary() {
        let options = RenderOptions::default();
        let stylesheets = Vec::new();
        let resource_cache = ResourceCache::default();
        let mut builder = test_layout_builder(&options, &stylesheets, &resource_cache);
        let mut style = ComputedStyle::initial();
        style.font_family = css::FontFamily::SansSerif;
        let marker = list_marker_image(6.0, 6.0, &style);
        let mut items = Vec::new();

        builder.push_inside_marker_items(&marker, &style, None, &mut items);
        items.push(inline_word(" B", &style));

        assert_eq!(
            inline_item_boundary_roles(&items),
            vec![
                InlineBoundaryRole::OpaqueAtomic,
                InlineBoundaryRole::Text,
                InlineBoundaryRole::Text
            ]
        );
        assert_eq!(normalized_inline_item_text(&mut items), "\u{fffc} B");
    }

    #[tokio::test]
    async fn generated_marker_forced_breaks_survive_sequence_records() {
        let options = RenderOptions::default();
        let stylesheets = Vec::new();
        let resource_cache = ResourceCache::default();
        let mut builder = test_layout_builder(&options, &stylesheets, &resource_cache);
        let mut style = ComputedStyle::initial();
        style.font_family = css::FontFamily::SansSerif;
        style.font_size = 12.0;
        style.line_height = 10.0;
        style.white_space = WhiteSpace::PreLine;
        let marker = list_marker_text("M\n\n", &style, false);
        let mut items = Vec::new();

        builder.push_inside_marker_items(&marker, &style, None, &mut items);
        items.push(inline_word("B", &style));
        let sequence = builder.collect_inline_line_sequence(items, &style, 100.0, 0.0, 0.0);

        assert_eq!(sequence_fragment_texts(&sequence), vec!["M", "", "B"]);
        assert!(sequence.records[1].is_forced_empty);
        assert!(sequence.records[1].fragment.is_none());
    }

    #[tokio::test]
    async fn bidi_controls_around_forced_breaks_stay_invisible_in_prepared_lines() {
        let options = RenderOptions::default();
        let stylesheets = Vec::new();
        let resource_cache = ResourceCache::default();
        let mut builder = test_layout_builder(&options, &stylesheets, &resource_cache);
        builder.cursor_y = 100.0;
        let mut style = ComputedStyle::initial();
        style.font_family = css::FontFamily::SansSerif;
        style.font_size = 12.0;
        style.line_height = 14.0;
        style.direction = Direction::Rtl;
        style.unicode_bidi = UnicodeBidi::BidiOverride;
        let mut items = Vec::new();

        builder.push_bidi_scope_start(&style, None, 0.0, InlineVisualOffset::zero(), &mut items);
        items.push(inline_word("abc", &style));
        items.push(InlineItem::Break(InlineBreak::default()));
        items.push(inline_word("def", &style));
        builder.push_bidi_scope_end(&style, None, 0.0, InlineVisualOffset::zero(), &mut items);

        let sequence = builder.collect_inline_line_sequence(items, &style, 100.0, 0.0, 0.0);
        let visual = prepared_visual_texts_for_sequence(&mut builder, &sequence, &style);

        assert_eq!(
            sequence_fragment_texts(&sequence),
            vec!["\u{202e}abc", "def\u{202c}"]
        );
        assert_eq!(visual, vec!["cba", "fed"]);
        assert!(
            visual
                .iter()
                .all(|text| !text.chars().any(character_is_bidi_format_control))
        );
    }

    #[tokio::test]
    async fn plaintext_alignment_resolves_per_forced_sequence_line() {
        let options = RenderOptions::default();
        let stylesheets = Vec::new();
        let resource_cache = ResourceCache::default();
        let mut builder = test_layout_builder(&options, &stylesheets, &resource_cache);
        builder.cursor_y = 100.0;
        let mut style = ComputedStyle::initial();
        style.font_family = css::FontFamily::SansSerif;
        style.font_size = 16.0;
        style.line_height = 20.0;
        style.text_align = TextAlign::Start;
        style.unicode_bidi = UnicodeBidi::Plaintext;
        style.direction = Direction::Ltr;
        let items = vec![
            inline_word("אב", &style),
            InlineItem::Break(InlineBreak::default()),
            inline_word("abc", &style),
        ];
        let sequence = builder.collect_inline_line_sequence(items, &style, 120.0, 0.0, 0.0);
        let context = inline_paragraph_context(&style, 120.0);
        let mut plaintext_state = None;

        let rtl_prepared = builder
            .prepare_inline_line_record(&sequence.records[0], context, &mut plaintext_state)
            .expect("rtl plaintext line should prepare");
        let ltr_prepared = builder
            .prepare_inline_line_record(&sequence.records[1], context, &mut plaintext_state)
            .expect("ltr plaintext line should prepare");
        let rtl_group = prepared_text_groups(&rtl_prepared)[0];
        let ltr_group = prepared_text_groups(&ltr_prepared)[0];

        assert_eq!(sequence_fragment_texts(&sequence), vec!["אב", "abc"]);
        assert_eq!(plaintext_state, Some(Direction::Ltr));
        assert!(rtl_group.x() > ltr_group.x() + 60.0);
        assert_eq!(sequence.records[0].fragment.as_ref().unwrap().text(), "אב");
        assert_eq!(sequence.records[1].fragment.as_ref().unwrap().text(), "abc");
    }

    #[tokio::test]
    async fn inline_line_sequence_keeps_forced_empty_lines_as_records() {
        let options = RenderOptions::default();
        let stylesheets = Vec::new();
        let resource_cache = ResourceCache::default();
        let mut builder = test_layout_builder(&options, &stylesheets, &resource_cache);
        let mut style = ComputedStyle::initial();
        style.font_family = css::FontFamily::SansSerif;
        style.font_size = 12.0;
        style.line_height = 10.0;
        style.line_height_value = css::ComputedLineHeight::from_points(10.0);
        style.line_height_multiplier = None;
        style.line_height_is_normal = false;
        let items = vec![
            inline_word("A", &style),
            InlineItem::Break(InlineBreak::default()),
            InlineItem::Break(InlineBreak::default()),
            inline_word("B", &style),
        ];

        let sequence = builder.collect_inline_line_sequence(items, &style, 100.0, 0.0, 0.0);

        assert_eq!(sequence.records.len(), 3);
        assert!(sequence.records[1].fragment.is_none());
        assert!(sequence.records[1].is_forced_empty);
        assert_eq!(sequence.records[1].paragraph_index, 1);
        assert_eq!(sequence.records[1].block_line_index, 1);
        assert_eq!(sequence.total_height(), 30.0);
    }

    #[tokio::test]
    async fn forced_empty_line_updates_last_in_flow_line_baseline() {
        let options = RenderOptions::default();
        let stylesheets = Vec::new();
        let resource_cache = ResourceCache::default();
        let mut builder = test_layout_builder(&options, &stylesheets, &resource_cache);
        builder.cursor_y = 100.0;
        let mut style = ComputedStyle::initial();
        style.font_family = css::FontFamily::SansSerif;
        style.font_size = 12.0;
        style.line_height = 10.0;
        style.line_height_value = css::ComputedLineHeight::from_points(10.0);
        style.line_height_multiplier = None;
        style.line_height_is_normal = false;
        let sequence = builder.collect_inline_line_sequence(
            vec![InlineItem::Break(InlineBreak::default())],
            &style,
            100.0,
            0.0,
            0.0,
        );
        let expected_baseline_y =
            100.0 - builder.inline_box_text_line_layout_baseline_offset(&style);

        builder.paint_inline_line_sequence(&sequence, &style);

        let baseline_y = builder
            .last_in_flow_line_baseline_y
            .expect("forced empty line should export an in-flow line baseline");
        assert!(
            (baseline_y - expected_baseline_y).abs() < 0.01,
            "expected forced empty baseline {expected_baseline_y}, got {baseline_y}"
        );
    }

    #[tokio::test]
    async fn generated_terminal_preserved_break_creates_forced_empty_line() {
        let options = RenderOptions::default();
        let stylesheets = Vec::new();
        let resource_cache = ResourceCache::default();
        let mut builder = test_layout_builder(&options, &stylesheets, &resource_cache);
        let mut style = ComputedStyle::initial();
        style.font_family = css::FontFamily::SansSerif;
        style.font_size = 12.0;
        style.line_height = 10.0;
        style.line_height_value = css::ComputedLineHeight::from_points(10.0);
        style.line_height_multiplier = None;
        style.line_height_is_normal = false;
        style.white_space = WhiteSpace::PreLine;
        let mut generated_items = vec![InlineItem::Word(Box::new(InlineWord {
            text: "\n".to_string(),
            style: inline_style(&style),
            baseline_shift: 0.0,
            visual_offset: InlineVisualOffset::zero(),
            link_target: None,
            mergeable: true,
            source: InlineTextSource::Generated,
            hanging_edges: InlineHangingEdges::default(),
            ancestor_inline_decorations: Vec::new(),
        }))];
        let mut normal_items = vec![inline_word("\n", &style)];

        inline_collect::normalize_inline_whitespace_items(&mut generated_items);
        inline_collect::normalize_inline_whitespace_items(&mut normal_items);
        let sequence =
            builder.collect_inline_line_sequence(generated_items, &style, 100.0, 0.0, 0.0);

        assert!(matches!(normal_items.as_slice(), []));
        assert_eq!(sequence.records.len(), 1);
        assert!(sequence.records[0].fragment.is_none());
        assert!(sequence.records[0].is_forced_empty);
        assert_eq!(sequence.total_height(), 10.0);
    }

    #[tokio::test]
    async fn terminal_pre_newline_creates_forced_empty_line() {
        let options = RenderOptions::default();
        let stylesheets = Vec::new();
        let resource_cache = ResourceCache::default();
        let mut builder = test_layout_builder(&options, &stylesheets, &resource_cache);
        let mut style = ComputedStyle::initial();
        style.font_family = css::FontFamily::SansSerif;
        style.font_size = 0.0;
        style.line_height = 0.0;
        style.line_height_value = css::ComputedLineHeight::from_points(0.0);
        style.line_height_multiplier = None;
        style.line_height_is_normal = false;
        style.white_space = WhiteSpace::Pre;

        let sequence = builder.collect_inline_line_sequence(
            vec![inline_word("\n", &style)],
            &style,
            100.0,
            0.0,
            0.0,
        );

        assert_eq!(sequence.records.len(), 1);
        assert!(sequence.records[0].fragment.is_none());
        assert!(sequence.records[0].is_forced_empty);
        assert_eq!(sequence.total_height(), 0.0);
        assert_eq!(
            builder.inline_box_sequence_baseline_offset(&sequence, &style, css::Edges::ZERO),
            Some(0.0)
        );
    }

    #[tokio::test]
    async fn zero_advance_inline_box_edge_creates_line_with_owner_line_height() {
        let options = RenderOptions::default();
        let stylesheets = Vec::new();
        let resource_cache = ResourceCache::default();
        let mut builder = test_layout_builder(&options, &stylesheets, &resource_cache);
        let mut style = ComputedStyle::initial();
        style.font_family = css::FontFamily::SansSerif;
        style.font_size = 12.0;
        style.line_height = 100.0;
        let items = vec![inline_box_edge(0.0, &style)];

        let sequence = builder.collect_inline_line_sequence(items, &style, 100.0, 0.0, 0.0);

        assert_eq!(sequence.records.len(), 1);
        assert!((sequence.total_height() - 100.0).abs() < 0.01);
    }

    #[tokio::test]
    async fn inline_line_sequence_fitting_applies_orphans_and_widows() {
        let options = RenderOptions::default();
        let stylesheets = Vec::new();
        let resource_cache = ResourceCache::default();
        let mut builder = test_layout_builder(&options, &stylesheets, &resource_cache);
        let mut style = ComputedStyle::initial();
        style.font_family = css::FontFamily::SansSerif;
        style.font_size = 12.0;
        style.line_height = 10.0;
        let items = vec![
            inline_word("A", &style),
            InlineItem::Break(InlineBreak::default()),
            inline_word("B", &style),
            InlineItem::Break(InlineBreak::default()),
            inline_word("C", &style),
            InlineItem::Break(InlineBreak::default()),
            inline_word("D", &style),
            InlineItem::Break(InlineBreak::default()),
            inline_word("E", &style),
        ];

        let sequence = builder.collect_inline_line_sequence(items, &style, 100.0, 0.0, 0.0);

        assert_eq!(sequence.records.len(), 5);
        assert_eq!(sequence.fitting_line_count(0, 25.0, false, 2, 2), 2);
        assert_eq!(sequence.fitting_line_count(0, 35.0, false, 2, 3), 2);
        assert_eq!(sequence.fitting_line_count(0, 5.0, false, 2, 2), 0);
        assert_eq!(sequence.fitting_line_count(0, 5.0, true, 2, 2), 1);
    }

    #[tokio::test]
    async fn inline_line_sequence_flags_are_paragraph_local() {
        let options = RenderOptions::default();
        let stylesheets = Vec::new();
        let resource_cache = ResourceCache::default();
        let mut builder = test_layout_builder(&options, &stylesheets, &resource_cache);
        let mut style = ComputedStyle::initial();
        style.font_family = css::FontFamily::SansSerif;
        style.font_size = 12.0;
        style.line_height = 10.0;
        let items = vec![
            inline_word("A", &style),
            InlineItem::Break(InlineBreak::default()),
            inline_word("B", &style),
        ];

        let sequence = builder.collect_inline_line_sequence(items, &style, 100.0, 0.0, 0.0);

        assert_eq!(sequence.records.len(), 2);
        assert_eq!(sequence.records[0].paragraph_index, 0);
        assert_eq!(sequence.records[0].paragraph_line_index, 0);
        assert!(sequence.records[0].is_first_formatted_line);
        assert!(sequence.records[0].is_last_line_in_paragraph);
        assert_eq!(sequence.records[1].paragraph_index, 1);
        assert_eq!(sequence.records[1].paragraph_line_index, 0);
        assert!(!sequence.records[1].is_first_formatted_line);
        assert!(sequence.records[1].is_last_line_in_paragraph);
    }

    #[tokio::test]
    async fn inline_line_sequence_shares_paragraph_last_hanging_width() {
        let options = RenderOptions::default();
        let stylesheets = Vec::new();
        let resource_cache = ResourceCache::default();
        let mut builder = test_layout_builder(&options, &stylesheets, &resource_cache);
        let mut style = ComputedStyle::initial();
        style.font_family = css::FontFamily::SansSerif;
        style.font_size = 12.0;
        style.line_height = 10.0;
        style.hanging_punctuation.force_end = true;
        let items = vec![inline_word("Alpha beta gamma.", &style)];

        let sequence = builder.collect_inline_line_sequence(items, &style, 34.0, 0.0, 0.0);

        assert!(sequence.records.len() > 1);
        let paragraph_width = sequence.records[0].paragraph_last_hanging_width;
        assert!(paragraph_width > 0.0);
        assert!(
            sequence
                .records
                .iter()
                .all(|record| (record.paragraph_last_hanging_width - paragraph_width).abs() < 0.01)
        );
    }

    #[tokio::test]
    async fn inline_line_sequence_keeps_plaintext_bidi_logical_text() {
        let options = RenderOptions::default();
        let stylesheets = Vec::new();
        let resource_cache = ResourceCache::default();
        let mut builder = test_layout_builder(&options, &stylesheets, &resource_cache);
        let mut style = ComputedStyle::initial();
        style.font_family = css::FontFamily::SansSerif;
        style.font_size = 12.0;
        style.line_height = 10.0;
        style.unicode_bidi = css::UnicodeBidi::Plaintext;
        let items = vec![inline_word("אבג", &style)];

        let sequence = builder.collect_inline_line_sequence(items, &style, 100.0, 0.0, 0.0);

        let fragment = sequence.records[0].fragment.as_ref().unwrap();
        assert_eq!(fragment.text(), "אבג");
    }

    #[tokio::test]
    async fn inline_box_padding_breaks_boundary_shaping() {
        let mut left_style = ComputedStyle::initial();
        left_style.display = Display::BLOCK;
        left_style.padding.left = 10.0;
        let left = inline_fragment("ع", left_style);
        let mut right = left.clone();
        *right.style_mut() = ComputedStyle::initial();

        assert!(can_shape_inline_fragments_together(&left, &right));

        right.style_mut().padding.left = 1.0;
        assert!(!can_shape_inline_fragments_together(&left, &right));
    }

    #[tokio::test]
    async fn inline_box_margin_breaks_boundary_shaping() {
        let left = inline_fragment("ع", ComputedStyle::initial());
        let mut right = left.clone();

        assert!(can_shape_inline_fragments_together(&left, &right));

        right.style_mut().margin.left = 1.0;
        assert!(!can_shape_inline_fragments_together(&left, &right));
    }

    #[tokio::test]
    async fn inline_box_used_border_breaks_boundary_shaping() {
        let left = inline_fragment("ع", ComputedStyle::initial());
        let mut right = left.clone();
        right.style_mut().border_widths.left = 1.0;
        right.style_mut().border_styles.left = BorderStyle::None;

        assert!(can_shape_inline_fragments_together(&left, &right));

        right.style_mut().border_styles.left = BorderStyle::Solid;
        assert!(!can_shape_inline_fragments_together(&left, &right));
    }

    #[tokio::test]
    async fn font_style_change_does_not_break_boundary_shaping() {
        let left = inline_fragment("ع", ComputedStyle::initial());
        let mut right = left.clone();
        right.style_mut().font_style = css::FontStyle::Italic;

        assert!(can_shape_inline_fragments_together(&left, &right));
    }

    #[tokio::test]
    async fn tatweel_only_inline_fragments_preserve_shaping_group() {
        let left = inline_fragment("\u{0640}", ComputedStyle::initial());
        let mut right = left.clone();
        right.set_text("ب");
        right.style_mut().font_family = css::FontFamily::Serif;

        assert!(can_shape_inline_fragments_together(&left, &right));
        assert!(inline_fragment_is_arabic_tatweel_only(&left));
        assert!(can_queue_inline_fragments_for_shaping(&left, &right));
    }

    #[tokio::test]
    async fn direction_change_alone_does_not_break_boundary_shaping() {
        let mut left_style = ComputedStyle::initial();
        left_style.direction = Direction::Rtl;
        let left = inline_fragment("ع", left_style);
        let mut right = left.clone();
        right.style_mut().direction = Direction::Ltr;

        assert!(can_shape_inline_fragments_together(&left, &right));

        right.style_mut().unicode_bidi = css::UnicodeBidi::Isolate;
        assert!(!can_shape_inline_fragments_together(&left, &right));
    }

    #[tokio::test]
    async fn table_cell_anonymous_inline_text_uses_baseline_vertical_align() {
        let mut cell_style = ComputedStyle::initial();
        cell_style.display = Display::TABLE_CELL;
        cell_style.vertical_align =
            VerticalAlign::BASELINE.with_table_cell_align(TableCellVerticalAlign::Middle);
        cell_style.unicode_bidi = css::UnicodeBidi::Isolate;

        let normalized = normalized_anonymous_inline_content_style(&cell_style);

        assert_eq!(normalized.vertical_align, VerticalAlign::BASELINE);
        assert_eq!(normalized.unicode_bidi, css::UnicodeBidi::Normal);
    }

    #[tokio::test]
    async fn join_control_inline_fragments_do_not_break_boundary_shaping() {
        let mut left_style = ComputedStyle::initial();
        left_style.font_family = css::FontFamily::SansSerif;
        let left = inline_fragment("ع", left_style);
        let mut joiner = left.clone();
        joiner.set_text("\u{200c}");
        joiner.style_mut().font_family = css::FontFamily::Serif;

        assert!(inline_fragment_is_join_control_only(&joiner));
        assert!(can_shape_inline_fragments_together(&left, &joiner));

        joiner.style_mut().padding.left = 1.0;
        assert!(can_shape_inline_fragments_together(&left, &joiner));
        let mut visible_right = joiner.clone();
        visible_right.set_text("ب");
        assert!(!can_shape_inline_fragments_together(&left, &visible_right));
    }

    #[tokio::test]
    async fn prepared_inline_text_group_preserves_shaped_runs() {
        let options = RenderOptions::default();
        let stylesheets = Vec::new();
        let resource_cache = ResourceCache::default();
        let mut builder = test_layout_builder(&options, &stylesheets, &resource_cache);
        let mut style = ComputedStyle::initial();
        style.font_family = css::FontFamily::SansSerif;
        style.font_size = 16.0;
        style.line_height = 20.0;
        builder.cursor_y = 100.0;

        let fragments = vec![
            inline_fragment("A", style.clone()),
            inline_fragment("B", style),
        ];
        let group = builder
            .prepare_inline_text_group(&fragments, 12.0)
            .expect("text group should shape");

        assert_eq!(group.x(), 12.0);
        assert_eq!(group.shaped.text, "AB");
        assert!(group.shaped.first_font_id().is_some());
        assert!((group.width() - group.shaped.advance_width()).abs() < 0.01);
        assert!(
            group
                .shaped
                .runs
                .iter()
                .flat_map(|run| &run.glyphs)
                .any(|glyph| glyph.paints && !glyph.source_text().is_empty())
        );
    }

    #[tokio::test]
    async fn prepared_inline_text_group_keeps_join_controls_out_of_paint() {
        let options = RenderOptions::default();
        let stylesheets = Vec::new();
        let resource_cache = ResourceCache::default();
        let mut builder = test_layout_builder(&options, &stylesheets, &resource_cache);
        let mut style = ComputedStyle::initial();
        style.font_family = css::FontFamily::SansSerif;
        style.font_size = 16.0;
        style.line_height = 20.0;
        builder.cursor_y = 100.0;

        let fragments = vec![
            inline_fragment("ع", style.clone()),
            inline_fragment("\u{200d}", style.clone()),
            inline_fragment("ب", style),
        ];
        let group = builder
            .prepare_inline_text_group(&fragments, 0.0)
            .expect("join-control group should shape");

        assert!(group.shaped.text.contains('\u{200d}'));
        assert!(
            group
                .shaped
                .rendered_runs()
                .iter()
                .flat_map(|run| run.glyphs.iter().flatten())
                .all(|glyph| !glyph.unicode.chars().any(character_is_join_control)),
            "{:?}",
            group.shaped
        );
    }

    #[tokio::test]
    async fn prepared_inline_line_shapes_across_styled_tatweel_fragment() {
        let stylesheet = css::parse_stylesheet(
            &crate::css::Css::from_string(
                r#"@font-face {
                    font-family: AlreqNaskh;
                    src: url("tests/resources/fonts/NotoNaskhArabic-regular.woff2");
                }
                @font-face {
                    font-family: AlreqTatweel;
                    src: url("tests/resources/fonts/Scheherazade-Regular.woff");
                }"#,
            )
            .with_base_url(Some(std::path::PathBuf::from("."))),
        );
        let font_system = FontSystem::start_loading()
            .load_stylesheet_fonts(&[stylesheet])
            .finish()
            .await;
        let options = RenderOptions::default();
        let stylesheets = Vec::new();
        let resource_cache = ResourceCache::default();
        let mut builder = LayoutBuilder::new(LayoutBuilderConfig {
            options: &options,
            stylesheets: &stylesheets,
            base_url: None,
            root_url: None,
            resource_cache: &resource_cache,
            page_progression_direction: Direction::Ltr,
            page_counter_initial_values: HashMap::new(),
            font_system,
        });
        builder.cursor_y = 100.0;

        let mut arabic = ComputedStyle::initial();
        arabic.font_family = css::FontFamily::Names(vec!["AlreqNaskh".to_string()]);
        arabic.font_size = 20.0;
        arabic.line_height = 24.0;
        arabic.direction = Direction::Rtl;
        let mut tatweel = arabic.clone();
        tatweel.font_family = css::FontFamily::Names(vec!["AlreqTatweel".to_string()]);

        let isolated_beh = builder
            .font_system
            .shape_unwrapped_line("\u{0628}", &arabic, arabic.line_height)
            .expect("isolated beh should shape")
            .runs
            .into_iter()
            .flat_map(|run| run.glyphs)
            .find(|glyph| glyph.source_text() == "\u{0628}")
            .expect("isolated beh glyph")
            .rendered
            .id;
        let beh_fragment = inline_fragment("\u{0628}", arabic.clone());
        let tatweel_fragment = inline_fragment("\u{0640}", tatweel.clone());
        let beh_width = builder.font_system.measure_text("\u{0628}", &arabic);
        let tatweel_width = builder.font_system.measure_text("\u{0640}", &tatweel);
        let line_fragment = inline_layout::InlineLineFragment::new(
            vec![
                inline_layout::MeasuredInlineItem {
                    item: InlineLineItem::Fragment(beh_fragment),
                    width: beh_width,
                    shaped: None,
                },
                inline_layout::MeasuredInlineItem {
                    item: InlineLineItem::Fragment(tatweel_fragment),
                    width: tatweel_width,
                    shaped: None,
                },
            ],
            InlineLineMetrics {
                width: beh_width + tatweel_width,
                height: arabic.line_height,
                baseline_offset: arabic.font_size,
            },
            HangingPunctuationWidths::default(),
            0.0,
            200.0,
            false,
            "\u{0628}\u{0640}",
        );

        let prepared = builder
            .prepare_inline_line_fragment(
                &line_fragment,
                InlinePaintContext {
                    block_style: &arabic,
                    direction: Direction::Rtl,
                    available_width: 200.0,
                    padding_left: 0.0,
                    line_indent: 0.0,
                    text_align: TextAlign::Left,
                    is_first_line: true,
                },
            )
            .expect("prepared line");
        let groups = prepared_text_groups(&prepared);

        assert_eq!(groups.len(), 1, "{prepared:?}");
        let group = groups[0];
        let beh_glyph = group
            .shaped
            .runs
            .iter()
            .flat_map(|run| &run.glyphs)
            .find(|glyph| glyph.source_text() == "\u{0628}")
            .expect("joined beh glyph");
        assert_ne!(beh_glyph.rendered.id, isolated_beh, "{prepared:?}");
        assert!(
            group
                .shaped
                .runs
                .iter()
                .filter_map(|run| run.font_id)
                .count()
                >= 2,
            "{prepared:?}"
        );
    }

    #[tokio::test]
    async fn prepared_inline_line_splits_text_groups_at_atoms() {
        let options = RenderOptions::default();
        let stylesheets = Vec::new();
        let resource_cache = ResourceCache::default();
        let mut builder = test_layout_builder(&options, &stylesheets, &resource_cache);
        let mut style = ComputedStyle::initial();
        style.font_family = css::FontFamily::SansSerif;
        style.font_size = 16.0;
        style.line_height = 20.0;
        builder.cursor_y = 100.0;
        let left = InlineLineItem::Fragment(inline_fragment("A", style.clone()));
        let atom = InlineLineItem::Atom(InlineAtom::new(
            InlineAtomContent::InlineEdge(InlineEdgeRole::BoxEdge(InlineBoxEdgeFragment {
                logical_edge: InlineLogicalEdge::End,
                physical_side: PhysicalSide::Right,
                advance: 10.0,
                paint_extent: 10.0,
            })),
            style.clone(),
            None,
            10.0,
            0.0,
            0.0,
            0.0,
            None,
            None,
        ));
        let right = InlineLineItem::Fragment(inline_fragment("B", style.clone()));
        let left_width = builder.font_system.measure_text("A", &style);
        let right_width = builder.font_system.measure_text("B", &style);
        let carried_left_width = left_width + 5.0;
        let line_left = builder.content_left;
        let line_fragment = inline_layout::InlineLineFragment::new(
            vec![
                inline_layout::MeasuredInlineItem {
                    item: left,
                    width: carried_left_width,
                    shaped: None,
                },
                inline_layout::MeasuredInlineItem {
                    item: atom,
                    width: 10.0,
                    shaped: None,
                },
                inline_layout::MeasuredInlineItem {
                    item: right,
                    width: right_width,
                    shaped: None,
                },
            ],
            InlineLineMetrics {
                width: carried_left_width + 10.0 + right_width,
                height: 20.0,
                baseline_offset: 16.0,
            },
            HangingPunctuationWidths::default(),
            0.0,
            200.0,
            false,
            "AB",
        );
        let prepared = builder
            .prepare_inline_line_fragment(
                &line_fragment,
                InlinePaintContext {
                    block_style: &style,
                    direction: style.direction,
                    available_width: 200.0,
                    padding_left: 0.0,
                    line_indent: 0.0,
                    text_align: TextAlign::Left,
                    is_first_line: true,
                },
            )
            .expect("mixed line should prepare");

        let text_groups = prepared
            .paint_items
            .iter()
            .filter(|item| matches!(item, PreparedInlinePaintItem::TextGroup(_)))
            .count();
        let atoms = prepared
            .paint_items
            .iter()
            .filter(|item| matches!(item, PreparedInlinePaintItem::Atom(_)))
            .count();
        assert_eq!(text_groups, 2);
        assert_eq!(atoms, 1);
        let atom_x = prepared
            .paint_items
            .iter()
            .find_map(|item| match item {
                PreparedInlinePaintItem::Atom(atom) => Some(atom.content_rect.x()),
                _ => None,
            })
            .expect("atom should be prepared");
        assert!(
            (atom_x - (line_left + carried_left_width)).abs() < 0.01,
            "mixed inline painting should advance with the carried graph width"
        );
    }

    #[tokio::test]
    async fn prepared_split_inline_end_edge_keeps_border_paint_with_negative_margin() {
        let options = RenderOptions::default();
        let stylesheets = Vec::new();
        let resource_cache = ResourceCache::default();
        let mut builder = test_layout_builder(&options, &stylesheets, &resource_cache);
        let mut style = ComputedStyle::initial();
        style.font_size = 150.0;
        style.line_height = 150.0;
        style.border_widths.right = 150.0;
        style.border_styles.right = BorderStyle::Solid;
        style.border_colors.right = Color::new(0, 128, 0);
        style.margin.right = -150.0;
        builder.cursor_y = 180.0;

        let edge = InlineBoxEdgeFragment {
            logical_edge: InlineLogicalEdge::End,
            physical_side: PhysicalSide::Right,
            advance: 0.0,
            paint_extent: 150.0,
        };
        let line_fragment = inline_layout::InlineLineFragment::new(
            vec![inline_layout::MeasuredInlineItem {
                item: InlineLineItem::Atom(InlineAtom::new(
                    InlineAtomContent::InlineEdge(InlineEdgeRole::BoxEdge(edge)),
                    style.clone(),
                    None,
                    edge.advance,
                    style.line_height,
                    style.font_size,
                    0.0,
                    None,
                    None,
                )),
                width: edge.advance,
                shaped: None,
            }],
            InlineLineMetrics {
                width: edge.advance,
                height: style.line_height,
                baseline_offset: style.font_size,
            },
            HangingPunctuationWidths::default(),
            0.0,
            300.0,
            false,
            String::new(),
        );

        let prepared = builder
            .prepare_inline_line_fragment(
                &line_fragment,
                InlinePaintContext {
                    block_style: &style,
                    direction: style.direction,
                    available_width: 300.0,
                    padding_left: 0.0,
                    line_indent: 0.0,
                    text_align: TextAlign::Left,
                    is_first_line: true,
                },
            )
            .expect("edge-only split inline line should prepare");
        let edge_rect = prepared
            .paint_items
            .iter()
            .find_map(|item| match item {
                PreparedInlinePaintItem::Atom(atom) => Some(atom.content_rect),
                _ => None,
            })
            .expect("edge-only split inline should prepare an edge paint atom");

        assert_eq!(line_fragment.metrics.width, 0.0);
        assert_eq!(edge_rect.width(), 150.0);
        assert_eq!(edge_rect.height(), 150.0);
    }

    #[tokio::test]
    async fn sequence_materialization_preserves_internal_spaces_around_opaque_atoms() {
        let options = RenderOptions::default();
        let stylesheets = Vec::new();
        let resource_cache = ResourceCache::default();
        let mut builder = test_layout_builder(&options, &stylesheets, &resource_cache);
        let mut style = ComputedStyle::initial();
        style.font_family = css::FontFamily::SansSerif;
        style.font_size = 16.0;
        style.line_height = 20.0;

        let sequence = builder.collect_inline_line_sequence(
            vec![
                inline_word("A\n", &style),
                inline_test_atom(5.0, &style),
                inline_word(" B", &style),
            ],
            &style,
            200.0,
            0.0,
            0.0,
        );
        let fragment = sequence.records[0].fragment.as_ref().unwrap();
        let fragment_texts = fragment
            .items
            .iter()
            .filter_map(|item| match &item.item {
                InlineLineItem::Fragment(fragment) => Some(fragment.text().to_string()),
                InlineLineItem::Atom(_) | InlineLineItem::Float(_) => None,
            })
            .collect::<Vec<_>>();

        assert_eq!(fragment.text(), "A  B");
        assert_eq!(fragment_texts, vec!["A", " ", " ", "B"]);
    }

    #[tokio::test]
    async fn prepared_inline_line_emits_space_text_groups_around_opaque_atoms() {
        let options = RenderOptions::default();
        let stylesheets = Vec::new();
        let resource_cache = ResourceCache::default();
        let mut builder = test_layout_builder(&options, &stylesheets, &resource_cache);
        let mut style = ComputedStyle::initial();
        style.font_family = css::FontFamily::SansSerif;
        style.font_size = 16.0;
        style.line_height = 20.0;
        builder.cursor_y = 100.0;

        let sequence = builder.collect_inline_line_sequence(
            vec![
                inline_word("A\n", &style),
                inline_test_atom(5.0, &style),
                inline_word(" B", &style),
            ],
            &style,
            200.0,
            0.0,
            0.0,
        );
        let mut plaintext_state = None;
        let prepared = builder
            .prepare_inline_line_record(
                &sequence.records[0],
                inline_paragraph_context(&style, 200.0),
                &mut plaintext_state,
            )
            .expect("atom-adjacent line should prepare");
        let groups = prepared_text_groups(&prepared);

        assert_eq!(groups.len(), 2, "{prepared:?}");
        assert_eq!(groups[0].shaped.text, "A ");
        assert_eq!(groups[1].shaped.text, " B");
        assert!(groups[0].width() > builder.font_system.measure_text("A", &style));
        assert!(groups[1].width() > builder.font_system.measure_text("B", &style));
    }

    #[tokio::test]
    async fn prepared_inline_line_keeps_transparent_edge_spaces_in_text_context() {
        let options = RenderOptions::default();
        let stylesheets = Vec::new();
        let resource_cache = ResourceCache::default();
        let mut builder = test_layout_builder(&options, &stylesheets, &resource_cache);
        let mut style = ComputedStyle::initial();
        style.font_family = css::FontFamily::SansSerif;
        style.font_size = 16.0;
        style.line_height = 20.0;
        builder.cursor_y = 100.0;

        let sequence = builder.collect_inline_line_sequence(
            vec![
                inline_word("A\n", &style),
                inline_box_edge(0.0, &style),
                inline_word("B", &style),
            ],
            &style,
            200.0,
            0.0,
            0.0,
        );
        let mut plaintext_state = None;
        let prepared = builder
            .prepare_inline_line_record(
                &sequence.records[0],
                inline_paragraph_context(&style, 200.0),
                &mut plaintext_state,
            )
            .expect("transparent-edge line should prepare");
        let group_text = prepared_text_groups(&prepared)
            .into_iter()
            .map(|group| group.shaped.text.as_str())
            .collect::<String>();

        assert_eq!(sequence.records[0].fragment.as_ref().unwrap().text(), "A B");
        assert_eq!(group_text, "A B");
    }

    fn prepared_text_groups(prepared: &PreparedInlineLine) -> Vec<&PreparedInlineTextGroup> {
        prepared
            .paint_items
            .iter()
            .filter_map(|item| match item {
                PreparedInlinePaintItem::TextGroup(group) => Some(group),
                _ => None,
            })
            .collect()
    }

    fn formatting_box_contains_text(box_: &box_tree::FormattingBox<'_>, needle: &str) -> bool {
        match box_ {
            box_tree::FormattingBox::Text(text) => text.text.contains(needle),
            _ => box_
                .children()
                .iter()
                .any(|child| formatting_box_contains_text(child, needle)),
        }
    }

    fn prepared_fragment_backgrounds(
        prepared: &PreparedInlineLine,
    ) -> Vec<&PreparedInlineFragment> {
        prepared
            .paint_items
            .iter()
            .filter_map(|item| match item {
                PreparedInlinePaintItem::FragmentBackground(fragment) => Some(fragment),
                _ => None,
            })
            .collect()
    }

    #[tokio::test]
    async fn split_inline_after_block_paints_only_inline_end_edge() {
        let root = dom::parse(
            r#"<!DOCTYPE html>
            <html><body><span><div>One</div>Two</span></body></html>"#,
        );
        let author = css::parse_stylesheet(&crate::css::Css::from_string(
            "body > span { border: 3px solid blue }",
        ));
        let stylesheets = vec![css::html5_user_agent_stylesheet(), author];
        let parent_style = ComputedStyle {
            font_size: 12.0,
            line_height: 14.4,
            color: Color::BLACK,
            ..ComputedStyle::initial()
        };
        let page =
            box_tree::freeze_page_box(box_tree::build_page_box(&root, &stylesheets, &parent_style));
        let body = &page.children[0].children()[0];
        let anonymous = body
            .children()
            .iter()
            .find_map(|child| match child {
                box_tree::FormattingBox::AnonymousBlock(anonymous)
                    if formatting_box_contains_text(child, "Two") =>
                {
                    Some(anonymous)
                }
                _ => None,
            })
            .expect("span text after the block should be wrapped in an anonymous block");
        let options = RenderOptions::default();
        let resource_cache = ResourceCache::default();
        let mut builder = test_layout_builder(&options, &stylesheets, &resource_cache);
        builder.cursor_y = 100.0;
        let mut items = Vec::new();
        builder.collect_inline_box_items(
            &anonymous.children,
            &stylesheets,
            None,
            0.0,
            InlineVisualOffset::zero(),
            &anonymous.style,
            anonymous.style.text_decoration,
            &mut items,
        );

        assert_eq!(
            items
                .iter()
                .filter(|item| {
                    matches!(
                        item,
                        InlineItem::Atom(atom)
                            if matches!(
                                atom.content(),
                                InlineAtomContent::InlineEdge(InlineEdgeRole::BoxEdge(_))
                            )
                    )
                })
                .count(),
            1,
            "{items:?}"
        );
        assert!(matches!(
            items.last(),
            Some(InlineItem::Atom(atom))
                if matches!(
                    atom.content(),
                    InlineAtomContent::InlineEdge(InlineEdgeRole::BoxEdge(_))
                )
        ));

        let sequence =
            builder.collect_inline_line_sequence(items, &anonymous.style, 200.0, 0.0, 0.0);
        let mut plaintext_state = None;
        let prepared = builder
            .prepare_inline_line_record(
                &sequence.records[0],
                inline_paragraph_context(&anonymous.style, 200.0),
                &mut plaintext_state,
            )
            .expect("split inline line should prepare");
        let backgrounds = prepared_fragment_backgrounds(&prepared);
        assert_eq!(backgrounds.len(), 1, "{prepared:?}");
        assert!(!backgrounds[0].fragment.hanging_edges().blocks_start);
        assert!(backgrounds[0].fragment.hanging_edges().blocks_end);
    }

    fn inline_line_record_for_items(
        items: Vec<inline_layout::MeasuredInlineItem>,
        text: &str,
        width: f32,
        available_width: f32,
        style: &ComputedStyle,
    ) -> inline_layout::InlineLineRecord {
        inline_layout::InlineLineRecord {
            paragraph_index: 0,
            block_line_index: 0,
            paragraph_line_index: 0,
            fragment: Some(inline_layout::InlineLineFragment::new(
                items,
                InlineLineMetrics {
                    width,
                    height: style.line_height,
                    baseline_offset: style.font_size,
                },
                HangingPunctuationWidths::default(),
                0.0,
                available_width,
                false,
                text,
            )),
            is_first_formatted_line: true,
            is_last_line_in_paragraph: true,
            is_forced_empty: false,
            clear_after: Clear::None,
            block_start_trim: 0.0,
            block_end_trim: 0.0,
            paragraph_last_hanging_width: 0.0,
            used_indent: 0.0,
            available_width,
            line_height: style.line_height,
        }
    }

    fn inline_paragraph_context<'a>(
        style: &'a ComputedStyle,
        available_width: f32,
    ) -> InlineParagraphContext<'a> {
        InlineParagraphContext {
            block_style: style,
            stylesheets: &[],
            available_width,
            padding_left: 0.0,
            hanging_indent: 0.0,
            hanging_punctuation_reserve: 0.0,
        }
    }

    #[test]
    fn inline_line_geometry_maps_horizontal_ltr_and_rtl_indents() {
        let mut style = ComputedStyle::initial();
        style.direction = Direction::Ltr;
        let ltr = InlineLineGeometry::new(
            20.0,
            100.0,
            InlinePaintContext {
                block_style: &style,
                direction: style.direction,
                available_width: 120.0,
                padding_left: 2.0,
                line_indent: 10.0,
                text_align: TextAlign::Left,
                is_first_line: true,
            },
        );
        let ltr_origin = ltr.visual_line_origin(0.0, 12.0);
        let ltr_rect = ltr.visual_line_item_rect(0.0, ltr_origin, 0.0, 12.0, 80.0, 20.0);
        assert!((ltr_rect.x() - 32.0).abs() < 0.01);
        assert!((ltr_rect.y() - 80.0).abs() < 0.01);
        assert!((ltr_rect.width() - 12.0).abs() < 0.01);
        assert!((ltr_rect.height() - 20.0).abs() < 0.01);

        style.direction = Direction::Rtl;
        let rtl = InlineLineGeometry::new(
            20.0,
            100.0,
            InlinePaintContext {
                block_style: &style,
                direction: style.direction,
                available_width: 120.0,
                padding_left: 2.0,
                line_indent: 10.0,
                text_align: TextAlign::Right,
                is_first_line: true,
            },
        );
        let rtl_origin = rtl.visual_line_origin(0.0, 12.0);
        let rtl_rect = rtl.visual_line_item_rect(0.0, rtl_origin, 0.0, 12.0, 80.0, 20.0);
        assert!((rtl_rect.x() - 120.0).abs() < 0.01);
        assert!((rtl_rect.y() - 80.0).abs() < 0.01);
        let rtl_next = rtl.visual_line_item_rect(0.0, rtl_origin, 12.0, 8.0, 80.0, 20.0);
        assert!((rtl_next.x() - 132.0).abs() < 0.01);
    }

    #[test]
    fn inline_line_geometry_maps_vertical_inline_axis() {
        let mut style = ComputedStyle::initial();
        style.writing_mode = WritingMode::VerticalRl;
        style.direction = Direction::Ltr;
        let geometry = InlineLineGeometry::new(
            20.0,
            100.0,
            InlinePaintContext {
                block_style: &style,
                direction: style.direction,
                available_width: 120.0,
                padding_left: 2.0,
                line_indent: 10.0,
                text_align: TextAlign::Left,
                is_first_line: true,
            },
        );
        let origin = geometry.visual_line_origin(5.0, 12.0);
        let rect = geometry.visual_line_item_rect(5.0, origin, 0.0, 12.0, 80.0, 20.0);
        assert!((rect.x() - 22.0).abs() < 0.01);
        assert!((rect.y() - 73.0).abs() < 0.01);
        assert!((rect.width() - 20.0).abs() < 0.01);
        assert!((rect.height() - 12.0).abs() < 0.01);
    }

    #[test]
    fn inline_line_geometry_resolves_alignment_and_hanging_logically() {
        let mut style = ComputedStyle::initial();
        let ltr = InlineLineGeometry::new(
            0.0,
            100.0,
            InlinePaintContext {
                block_style: &style,
                direction: style.direction,
                available_width: 100.0,
                padding_left: 0.0,
                line_indent: 0.0,
                text_align: TextAlign::Left,
                is_first_line: true,
            },
        );
        assert!((ltr.alignment_offset(30.0, TextAlign::Left) - 0.0).abs() < 0.01);
        assert!((ltr.alignment_offset(30.0, TextAlign::Center) - 35.0).abs() < 0.01);
        assert!((ltr.alignment_offset(30.0, TextAlign::Right) - 70.0).abs() < 0.01);
        assert!(
            (ltr.hanging_punctuation_offset(HangingPunctuationWidths {
                start: 5.0,
                end: 7.0
            }) + 5.0)
                .abs()
                < 0.01
        );

        style.direction = Direction::Rtl;
        let rtl = InlineLineGeometry::new(
            0.0,
            100.0,
            InlinePaintContext {
                block_style: &style,
                direction: style.direction,
                available_width: 100.0,
                padding_left: 0.0,
                line_indent: 0.0,
                text_align: TextAlign::Right,
                is_first_line: true,
            },
        );
        assert!(
            (rtl.hanging_punctuation_offset(HangingPunctuationWidths {
                start: 5.0,
                end: 7.0
            }) - 7.0)
                .abs()
                < 0.01
        );
    }

    #[tokio::test]
    async fn prepared_inline_line_record_unifies_split_and_unsplit_text() {
        let options = RenderOptions::default();
        let stylesheets = Vec::new();
        let resource_cache = ResourceCache::default();
        let mut builder = test_layout_builder(&options, &stylesheets, &resource_cache);
        let mut style = ComputedStyle::initial();
        style.font_family = css::FontFamily::SansSerif;
        style.font_size = 16.0;
        style.line_height = 20.0;
        style.text_align = TextAlign::Justify;
        style.text_align_last = TextAlignLast::Align(TextAlign::Justify);
        builder.cursor_y = 100.0;

        let whole_width = builder.font_system.measure_text("A B", &style);
        let whole = inline_layout::MeasuredInlineItem {
            item: InlineLineItem::Fragment(inline_fragment("A B", style.clone())),
            width: whole_width,
            shaped: None,
        };
        let split_left_width = builder.font_system.measure_text("A", &style);
        let split_right_width = builder.font_system.measure_text(" B", &style);
        let split = vec![
            inline_layout::MeasuredInlineItem {
                item: InlineLineItem::Fragment(inline_fragment("A", style.clone())),
                width: split_left_width,
                shaped: None,
            },
            inline_layout::MeasuredInlineItem {
                item: InlineLineItem::Fragment(inline_fragment(" B", style.clone())),
                width: split_right_width,
                shaped: None,
            },
        ];
        let available_width = 120.0;
        let whole_record =
            inline_line_record_for_items(vec![whole], "A B", whole_width, available_width, &style);
        let split_record =
            inline_line_record_for_items(split, "A B", whole_width, available_width, &style);
        let context = inline_paragraph_context(&style, available_width);
        let mut whole_plaintext_state = None;
        let mut split_plaintext_state = None;

        let whole_prepared = builder
            .prepare_inline_line_record(&whole_record, context, &mut whole_plaintext_state)
            .expect("whole line should prepare");
        let split_prepared = builder
            .prepare_inline_line_record(&split_record, context, &mut split_plaintext_state)
            .expect("split line should prepare");

        let whole_group = prepared_text_groups(&whole_prepared)[0];
        let split_group = prepared_text_groups(&split_prepared)[0];
        assert_eq!(whole_group.shaped.text, "A B");
        assert_eq!(split_group.shaped.text, "A B");
        assert!((whole_group.x() - split_group.x()).abs() < 0.01);
        assert!((whole_group.width() - split_group.width()).abs() < 0.01);
    }

    #[tokio::test]
    async fn prepared_justified_line_uses_boundary_shaped_width_for_expansion() {
        let options = RenderOptions::default();
        let stylesheets = Vec::new();
        let resource_cache = ResourceCache::default();
        let mut builder = test_layout_builder(&options, &stylesheets, &resource_cache);
        let mut style = ComputedStyle::initial();
        style.font_family = css::FontFamily::SansSerif;
        style.font_size = 16.0;
        style.line_height = 20.0;
        style.text_align = TextAlign::Justify;
        style.text_align_last = TextAlignLast::Align(TextAlign::Justify);
        builder.cursor_y = 100.0;

        let text = "A B";
        let measured_width = builder.font_system.measure_text(text, &style);
        let left_width = builder.font_system.measure_text("A", &style);
        let space_width = builder.font_system.measure_text(" ", &style);
        let right_width = builder.font_system.measure_text("B", &style);
        let record = inline_line_record_for_items(
            vec![
                inline_layout::MeasuredInlineItem {
                    item: InlineLineItem::Fragment(inline_fragment("A", style.clone())),
                    width: left_width,
                    shaped: None,
                },
                inline_layout::MeasuredInlineItem {
                    item: InlineLineItem::Fragment(inline_fragment(" ", style.clone())),
                    width: space_width,
                    shaped: None,
                },
                inline_layout::MeasuredInlineItem {
                    item: InlineLineItem::Fragment(inline_fragment("B", style.clone())),
                    width: right_width,
                    shaped: None,
                },
            ],
            text,
            measured_width + 5.0,
            120.0,
            &style,
        );
        let mut plaintext_state = None;
        let prepared = builder
            .prepare_inline_line_record(
                &record,
                inline_paragraph_context(&style, 120.0),
                &mut plaintext_state,
            )
            .expect("justified line should prepare");

        let group = prepared_text_groups(&prepared)[0];
        assert_eq!(group.shaped.text, text);
        assert!(
            (group.width() - 120.0).abs() < 0.01,
            "justification should fill from painted text width, got {}",
            group.width()
        );
    }

    #[tokio::test]
    async fn prepared_inline_line_record_excludes_trailing_tracking_once() {
        let options = RenderOptions::default();
        let stylesheets = Vec::new();
        let resource_cache = ResourceCache::default();
        let mut builder = test_layout_builder(&options, &stylesheets, &resource_cache);
        let mut style = ComputedStyle::initial();
        style.font_family = css::FontFamily::SansSerif;
        style.font_size = 16.0;
        style.line_height = 20.0;
        style.letter_spacing = ComputedLengthPercentage::from_points(5.0);
        builder.cursor_y = 100.0;

        let measured_width = builder.font_system.measure_text("AB", &style);
        let record = inline_line_record_for_items(
            vec![inline_layout::MeasuredInlineItem {
                item: InlineLineItem::Fragment(inline_fragment("AB", style.clone())),
                width: measured_width,
                shaped: None,
            }],
            "AB",
            measured_width - style.used_letter_spacing(),
            100.0,
            &style,
        );
        let mut plaintext_state = None;
        let prepared = builder
            .prepare_inline_line_record(
                &record,
                inline_paragraph_context(&style, 100.0),
                &mut plaintext_state,
            )
            .expect("tracked line should prepare");

        let background = prepared_fragment_backgrounds(&prepared)[0];
        assert!(
            (background.rect.width() - (measured_width - style.used_letter_spacing())).abs() < 0.01
        );
    }

    #[tokio::test]
    async fn prepared_inline_fragment_background_uses_font_content_area() {
        let options = RenderOptions::default();
        let stylesheets = Vec::new();
        let resource_cache = ResourceCache::default();
        let mut builder = test_layout_builder(&options, &stylesheets, &resource_cache);
        let mut style = ComputedStyle::initial();
        style.font_family = css::FontFamily::SansSerif;
        style.font_size = 16.0;
        style.line_height = 24.0;
        style.border_widths.top = 5.0;
        style.border_widths.right = 5.0;
        style.border_widths.bottom = 5.0;
        style.border_widths.left = 5.0;
        style.border_styles.top = BorderStyle::Solid;
        style.border_styles.right = BorderStyle::Solid;
        style.border_styles.bottom = BorderStyle::Solid;
        style.border_styles.left = BorderStyle::Solid;
        builder.cursor_y = 100.0;

        let measured_width = builder.font_system.measure_text("inspect", &style);
        let record = inline_line_record_for_items(
            vec![inline_layout::MeasuredInlineItem {
                item: InlineLineItem::Fragment(
                    inline_fragment("inspect", style.clone()).with_hanging_edges(
                        InlineHangingEdges {
                            blocks_start: true,
                            blocks_end: true,
                        },
                    ),
                ),
                width: measured_width,
                shaped: None,
            }],
            "inspect",
            measured_width,
            200.0,
            &style,
        );
        let mut plaintext_state = None;
        let prepared = builder
            .prepare_inline_line_record(
                &record,
                inline_paragraph_context(&style, 200.0),
                &mut plaintext_state,
            )
            .expect("inline line should prepare");

        let background = prepared_fragment_backgrounds(&prepared)[0];
        assert!(
            (background.rect.height() - style.font_size).abs() < 0.01,
            "prepared fragment geometry should be the CSS inline content area: {background:?}"
        );
        builder.paint_prepared_inline_line(&prepared);
        let purple_rects = builder
            .current_page
            .rects
            .iter()
            .filter(|rect| rect.fill == Some(Color::BLACK))
            .collect::<Vec<_>>();
        assert_eq!(purple_rects.len(), 4, "{purple_rects:?}");
        let top = purple_rects
            .iter()
            .find(|rect| (rect.height() - 5.0).abs() < 0.01 && rect.y() > background.rect.y())
            .expect("top border should paint above the content area");
        let bottom = purple_rects
            .iter()
            .find(|rect| (rect.height() - 5.0).abs() < 0.01 && rect.y() < background.rect.y())
            .expect("bottom border should paint below the content area");
        assert!(
            (top.y() - (background.rect.y() + style.font_size)).abs() < 0.01,
            "top border should begin at content top: top={top:?} background={background:?}"
        );
        assert!(
            ((bottom.y() + bottom.height()) - background.rect.y()).abs() < 0.01,
            "bottom border should end at content bottom: bottom={bottom:?} background={background:?}"
        );
    }

    #[tokio::test]
    async fn rtl_inline_border_backgrounds_expand_away_from_text_content() {
        let options = RenderOptions::default();
        let stylesheets = Vec::new();
        let resource_cache = ResourceCache::default();
        let mut builder = test_layout_builder(&options, &stylesheets, &resource_cache);
        let mut style = ComputedStyle::initial();
        style.direction = Direction::Rtl;
        style.font_family = css::FontFamily::SansSerif;
        style.font_size = 16.0;
        style.line_height = 24.0;
        style.border_widths.top = 5.0;
        style.border_widths.right = 5.0;
        style.border_widths.bottom = 5.0;
        style.border_widths.left = 5.0;
        style.border_styles.top = BorderStyle::Solid;
        style.border_styles.right = BorderStyle::Solid;
        style.border_styles.bottom = BorderStyle::Solid;
        style.border_styles.left = BorderStyle::Solid;
        builder.cursor_y = 100.0;

        for (items, expected_backgrounds) in [
            (
                vec![
                    inline_layout::MeasuredInlineItem {
                        item: InlineLineItem::Fragment(
                            inline_fragment("inspect", style.clone()).with_hanging_edges(
                                InlineHangingEdges {
                                    blocks_start: true,
                                    blocks_end: true,
                                },
                            ),
                        ),
                        width: builder.font_system.measure_text("inspect", &style),
                        shaped: None,
                    },
                    inline_layout::MeasuredInlineItem {
                        item: InlineLineItem::Fragment(inline_fragment("pause", style.clone())),
                        width: builder.font_system.measure_text("pause", &style),
                        shaped: None,
                    },
                ],
                2usize,
            ),
            (
                vec![
                    inline_layout::MeasuredInlineItem {
                        item: InlineLineItem::Fragment(
                            inline_fragment("inspect", style.clone()).with_hanging_edges(
                                InlineHangingEdges {
                                    blocks_start: true,
                                    blocks_end: true,
                                },
                            ),
                        ),
                        width: builder.font_system.measure_text("inspect", &style),
                        shaped: None,
                    },
                    inline_layout::MeasuredInlineItem {
                        item: InlineLineItem::Fragment(inline_fragment(" ", style.clone())),
                        width: builder.font_system.measure_text(" ", &style),
                        shaped: None,
                    },
                    inline_layout::MeasuredInlineItem {
                        item: InlineLineItem::Fragment(inline_fragment("pause", style.clone())),
                        width: builder.font_system.measure_text("pause", &style),
                        shaped: None,
                    },
                ],
                3usize,
            ),
            (
                vec![inline_layout::MeasuredInlineItem {
                    item: InlineLineItem::Fragment(
                        inline_fragment("inspectpause", style.clone()).with_hanging_edges(
                            InlineHangingEdges {
                                blocks_start: true,
                                blocks_end: true,
                            },
                        ),
                    ),
                    width: builder.font_system.measure_text("inspectpause", &style),
                    shaped: None,
                }],
                1usize,
            ),
        ] {
            let width = items.iter().map(|item| item.width).sum::<f32>();
            let record = inline_line_record_for_items(items, "inspectpause", width, 300.0, &style);
            let mut plaintext_state = None;
            let prepared = builder
                .prepare_inline_line_record(
                    &record,
                    inline_paragraph_context(&style, 300.0),
                    &mut plaintext_state,
                )
                .expect("RTL inline line should prepare");
            let backgrounds = prepared_fragment_backgrounds(&prepared);
            assert_eq!(backgrounds.len(), expected_backgrounds, "{prepared:?}");
            let bordered = backgrounds[0];
            assert!(
                (bordered.rect.height() - style.font_size).abs() < 0.01,
                "RTL bordered inline content area should remain font-sized: {bordered:?}"
            );
        }
    }

    #[tokio::test]
    async fn rtl_bidi_nested_inline_preserves_parent_border_across_child() {
        let document = crate::Html::from_string(
            r#"<!DOCTYPE html>
            <style>
              @page { size: 420pt 180pt; margin: 20pt }
              body { margin: 0; font: 16pt/20pt sans-serif }
              .container { width: 300pt; background: pink }
              div { margin-bottom: 10pt }
              .purple { border: purple solid 5pt }
              .orange { background: orange }
            </style>
            <body>
              <div class="container">
                <div dir="rtl">
                  <span class="purple">inspect<span class="orange">pause</span></span>
                </div>
              </div>
            </body>"#,
        )
        .render_async(&RenderOptions::default())
        .await
        .expect("RTL nested inline document should render");

        let page = &document.pages[0];
        let purple = Color::new(128, 0, 128);
        let orange = Color::new(255, 165, 0);
        let orange_background = page
            .rects()
            .iter()
            .find(|rect| rect.fill == Some(orange))
            .unwrap_or_else(|| panic!("orange child background should paint: {:?}", page.rects()));
        let purple_rects = page
            .rects()
            .iter()
            .filter(|rect| rect.fill == Some(purple))
            .collect::<Vec<_>>();
        let vertical_edges = purple_rects
            .iter()
            .filter(|rect| (rect.width() - 5.0).abs() < 0.01 && rect.height() > 15.0);
        assert_eq!(
            vertical_edges.count(),
            2,
            "parent inline should paint both vertical purple borders: {purple_rects:?}"
        );

        let horizontal_edges = purple_rects
            .iter()
            .filter(|rect| (rect.height() - 5.0).abs() < 0.01 && rect.width() > 5.0)
            .collect::<Vec<_>>();
        assert!(
            horizontal_edges.len() >= 2,
            "parent inline should paint purple top/bottom borders: {purple_rects:?}"
        );
        let covers_orange_on_both_block_edges = horizontal_edges.iter().filter(|edge| {
            edge.x() <= orange_background.x() + 0.01
                && edge.x() + edge.width()
                    >= orange_background.x() + orange_background.width() - 0.01
        });
        assert!(
            covers_orange_on_both_block_edges.count() >= 2,
            "purple top/bottom borders should cover the orange child span: purple={purple_rects:?}, orange={orange_background:?}"
        );
    }

    #[tokio::test]
    async fn prepared_inline_line_record_vertical_indent_moves_logical_inline_start() {
        let options = RenderOptions::default();
        let stylesheets = Vec::new();
        let resource_cache = ResourceCache::default();
        let mut builder = test_layout_builder(&options, &stylesheets, &resource_cache);
        let mut style = ComputedStyle::initial();
        style.font_family = css::FontFamily::SansSerif;
        style.font_size = 16.0;
        style.line_height = 20.0;
        style.writing_mode = WritingMode::VerticalRl;
        builder.cursor_y = 100.0;

        let measured_width = builder.font_system.measure_text("A", &style);
        let record = inline_line_record_for_items(
            vec![inline_layout::MeasuredInlineItem {
                item: InlineLineItem::Fragment(inline_fragment("A", style.clone())),
                width: measured_width,
                shaped: None,
            }],
            "A",
            measured_width,
            100.0,
            &style,
        );
        let mut indented_record = record.clone();
        indented_record.used_indent = 10.0;
        let context = inline_paragraph_context(&style, 100.0);

        let mut plaintext_state = None;
        let unindented = builder
            .prepare_inline_line_record(&record, context, &mut plaintext_state)
            .expect("vertical line should prepare");
        let unindented_y = prepared_text_groups(&unindented)[0].y();

        builder.cursor_y = 100.0;
        let mut plaintext_state = None;
        let indented = builder
            .prepare_inline_line_record(&indented_record, context, &mut plaintext_state)
            .expect("indented vertical line should prepare");
        let indented_y = prepared_text_groups(&indented)[0].y();

        assert!(
            indented_y < unindented_y - 9.0,
            "vertical text-indent should move along the inline axis: {indented_y} vs {unindented_y}"
        );
    }

    #[tokio::test]
    async fn vertical_writing_positions_cjk_upright_and_latin_sideways() {
        let options = RenderOptions::default();
        let stylesheets = Vec::new();
        let resource_cache = ResourceCache::default();
        let mut builder = test_layout_builder(&options, &stylesheets, &resource_cache);
        let mut style = ComputedStyle::initial();
        style.font_family = css::FontFamily::SansSerif;
        style.font_size = 16.0;
        style.line_height = 20.0;
        style.writing_mode = WritingMode::VerticalRl;

        let shaped = builder
            .font_system
            .shape_unwrapped_line("中文AB", &style, style.line_height)
            .expect("vertical text should shape");
        let runs = text_paint::positioned_rendered_runs_for_writing_mode(&shaped, &style);

        assert!(runs.iter().any(|run| {
            run.text.contains('中') && run.text_matrix == RenderedTextMatrix::IDENTITY
        }));
        assert!(runs.iter().any(|run| {
            run.text.contains("AB") && run.text_matrix == RenderedTextMatrix::ROTATE_CW
        }));
        let cjk_offsets = runs
            .iter()
            .filter(|run| run.text.contains('中') || run.text.contains('文'))
            .map(|run| run.y_offset)
            .collect::<Vec<_>>();
        assert!(
            cjk_offsets.windows(2).all(|window| window[1] < window[0]),
            "vertical LTR CJK offsets should advance downward: {cjk_offsets:?}"
        );
    }

    #[tokio::test]
    async fn vertical_mixed_orientation_uses_unicode_vertical_orientation() {
        let options = RenderOptions::default();
        let stylesheets = Vec::new();
        let resource_cache = ResourceCache::default();
        let mut builder = test_layout_builder(&options, &stylesheets, &resource_cache);
        let mut style = ComputedStyle::initial();
        style.font_family = css::FontFamily::SansSerif;
        style.font_size = 16.0;
        style.line_height = 20.0;
        style.writing_mode = WritingMode::VerticalRl;

        let shaped = builder
            .font_system
            .shape_unwrapped_line("a§、\u{2329}", &style, style.line_height)
            .expect("mixed vertical text should shape");
        let runs = text_paint::positioned_rendered_runs_for_writing_mode(&shaped, &style);

        assert!(runs.iter().any(|run| {
            run.text.contains('a') && run.text_matrix == RenderedTextMatrix::ROTATE_CW
        }));
        assert!(runs.iter().any(|run| {
            run.text.contains('§') && run.text_matrix == RenderedTextMatrix::IDENTITY
        }));
        assert!(runs.iter().any(|run| {
            run.text.contains('、') && run.text_matrix == RenderedTextMatrix::IDENTITY
        }));
        assert!(runs.iter().any(|run| {
            run.text.contains('\u{2329}') && run.text_matrix == RenderedTextMatrix::ROTATE_CW
        }));
    }

    #[tokio::test]
    async fn horizontal_writing_ignores_text_orientation_for_run_matrices() {
        let options = RenderOptions::default();
        let stylesheets = Vec::new();
        let resource_cache = ResourceCache::default();
        let mut builder = test_layout_builder(&options, &stylesheets, &resource_cache);
        let mut style = ComputedStyle::initial();
        style.font_family = css::FontFamily::SansSerif;
        style.font_size = 16.0;
        style.line_height = 20.0;
        style.text_orientation = TextOrientation::Sideways;

        let shaped = builder
            .font_system
            .shape_unwrapped_line("AB中文", &style, style.line_height)
            .expect("horizontal text should shape");
        let runs = text_paint::positioned_rendered_runs_for_writing_mode(&shaped, &style);

        assert!(
            runs.iter()
                .all(|run| run.text_matrix == RenderedTextMatrix::IDENTITY && run.y_offset == 0.0)
        );
    }

    #[tokio::test]
    async fn vertical_text_orientation_upright_keeps_text_units_upright() {
        let options = RenderOptions::default();
        let stylesheets = Vec::new();
        let resource_cache = ResourceCache::default();
        let mut builder = test_layout_builder(&options, &stylesheets, &resource_cache);
        let mut style = ComputedStyle::initial();
        style.font_family = css::FontFamily::SansSerif;
        style.font_size = 16.0;
        style.line_height = 20.0;
        style.writing_mode = WritingMode::VerticalRl;
        style.text_orientation = TextOrientation::Upright;

        let shaped = builder
            .font_system
            .shape_unwrapped_line("AB中", &style, style.line_height)
            .expect("upright vertical text should shape");
        let runs = text_paint::positioned_rendered_runs_for_writing_mode(&shaped, &style);

        assert!(
            runs.iter()
                .filter(|run| !run.text.is_empty())
                .all(|run| run.text_matrix == RenderedTextMatrix::IDENTITY)
        );
        assert!(runs.iter().any(|run| run.text.contains("A")));
        assert!(runs.iter().any(|run| run.text.contains('中')));
    }

    #[tokio::test]
    async fn vertical_text_orientation_sideways_rotates_all_text_units() {
        let options = RenderOptions::default();
        let stylesheets = Vec::new();
        let resource_cache = ResourceCache::default();
        let mut builder = test_layout_builder(&options, &stylesheets, &resource_cache);
        let mut style = ComputedStyle::initial();
        style.font_family = css::FontFamily::SansSerif;
        style.font_size = 16.0;
        style.line_height = 20.0;
        style.writing_mode = WritingMode::VerticalRl;
        style.text_orientation = TextOrientation::Sideways;

        let shaped = builder
            .font_system
            .shape_unwrapped_line("中文AB", &style, style.line_height)
            .expect("sideways vertical text should shape");
        let runs = text_paint::positioned_rendered_runs_for_writing_mode(&shaped, &style);

        assert!(runs.iter().any(|run| run.text.contains("中文")));
        assert!(runs.iter().any(|run| run.text.contains("AB")));
        assert!(
            runs.iter()
                .filter(|run| !run.text.is_empty())
                .all(|run| run.text_matrix == RenderedTextMatrix::ROTATE_CW)
        );
    }

    #[tokio::test]
    async fn prepared_inline_line_record_inter_character_preserves_fragment_metadata() {
        let options = RenderOptions::default();
        let stylesheets = Vec::new();
        let resource_cache = ResourceCache::default();
        let mut builder = test_layout_builder(&options, &stylesheets, &resource_cache);
        let mut style = ComputedStyle::initial();
        style.font_family = css::FontFamily::SansSerif;
        style.font_size = 16.0;
        style.line_height = 20.0;
        style.text_align = TextAlign::Justify;
        style.text_align_last = TextAlignLast::Align(TextAlign::Justify);
        style.text_justify = TextJustify::InterCharacter;
        builder.cursor_y = 100.0;

        let mut fragment = inline_fragment("AB", style.clone());
        fragment.set_link_target(Some("#target".to_string()));
        fragment.baseline_shift = 2.0;
        let measured_width = builder.font_system.measure_text("AB", &style);
        let record = inline_line_record_for_items(
            vec![inline_layout::MeasuredInlineItem {
                item: InlineLineItem::Fragment(fragment),
                width: measured_width,
                shaped: None,
            }],
            "AB",
            measured_width,
            120.0,
            &style,
        );
        let mut plaintext_state = None;
        let prepared = builder
            .prepare_inline_line_record(
                &record,
                inline_paragraph_context(&style, 120.0),
                &mut plaintext_state,
            )
            .expect("inter-character line should prepare");

        let groups = prepared_text_groups(&prepared);
        assert_eq!(groups.len(), 2);
        assert!(
            groups
                .iter()
                .all(|group| group.link_target.as_deref() == Some("#target"))
        );
        assert!(
            groups
                .iter()
                .all(|group| (group.y() - (100.0 - 16.0 + 2.0)).abs() < 5.0)
        );
    }

    #[tokio::test]
    async fn prepared_inline_line_record_inter_character_avoids_joining_sequences() {
        let options = RenderOptions::default();
        let stylesheets = Vec::new();
        let resource_cache = ResourceCache::default();
        let mut builder = test_layout_builder(&options, &stylesheets, &resource_cache);
        let mut style = ComputedStyle::initial();
        style.font_family = css::FontFamily::SansSerif;
        style.font_size = 16.0;
        style.line_height = 20.0;
        style.text_align = TextAlign::Justify;
        style.text_align_last = TextAlignLast::Align(TextAlign::Justify);
        style.text_justify = TextJustify::InterCharacter;
        builder.cursor_y = 100.0;

        let text = "سلام";
        let measured_width = builder.font_system.measure_text(text, &style);
        let record = inline_line_record_for_items(
            vec![inline_layout::MeasuredInlineItem {
                item: InlineLineItem::Fragment(inline_fragment(text, style.clone())),
                width: measured_width,
                shaped: None,
            }],
            text,
            measured_width,
            160.0,
            &style,
        );
        let mut plaintext_state = None;
        let prepared = builder
            .prepare_inline_line_record(
                &record,
                inline_paragraph_context(&style, 160.0),
                &mut plaintext_state,
            )
            .expect("inter-character line should prepare");

        let groups = prepared_text_groups(&prepared);
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].shaped.text.chars().count(), text.chars().count());
        assert!(groups[0].width() < 80.0);
    }

    #[tokio::test]
    async fn prepared_inline_line_record_inter_character_blocks_atom_boundaries() {
        let options = RenderOptions::default();
        let stylesheets = Vec::new();
        let resource_cache = ResourceCache::default();
        let mut builder = test_layout_builder(&options, &stylesheets, &resource_cache);
        let mut style = ComputedStyle::initial();
        style.font_family = css::FontFamily::SansSerif;
        style.font_size = 16.0;
        style.line_height = 20.0;
        style.text_align = TextAlign::Justify;
        style.text_align_last = TextAlignLast::Align(TextAlign::Justify);
        style.text_justify = TextJustify::InterCharacter;
        builder.cursor_y = 100.0;

        let left_width = builder.font_system.measure_text("A", &style);
        let right_width = builder.font_system.measure_text("B", &style);
        let atom_width = 10.0;
        let line_left = builder.content_left;
        let record = inline_line_record_for_items(
            vec![
                inline_layout::MeasuredInlineItem {
                    item: InlineLineItem::Fragment(inline_fragment("A", style.clone())),
                    width: left_width,
                    shaped: None,
                },
                inline_layout::MeasuredInlineItem {
                    item: InlineLineItem::Atom(InlineAtom::new(
                        InlineAtomContent::Canvas,
                        style.clone(),
                        None,
                        atom_width,
                        10.0,
                        8.0,
                        0.0,
                        None,
                        None,
                    )),
                    width: atom_width,
                    shaped: None,
                },
                inline_layout::MeasuredInlineItem {
                    item: InlineLineItem::Fragment(inline_fragment("B", style.clone())),
                    width: right_width,
                    shaped: None,
                },
            ],
            "AB",
            left_width + atom_width + right_width,
            200.0,
            &style,
        );
        let mut plaintext_state = None;
        let prepared = builder
            .prepare_inline_line_record(
                &record,
                inline_paragraph_context(&style, 200.0),
                &mut plaintext_state,
            )
            .expect("mixed inter-character line should prepare");

        let atom_x = prepared
            .paint_items
            .iter()
            .find_map(|item| match item {
                PreparedInlinePaintItem::Atom(atom) => Some(atom.content_rect.x()),
                _ => None,
            })
            .expect("atom should be prepared");
        assert!(
            (atom_x - (line_left + left_width)).abs() < 0.01,
            "inter-character justification must not expand across opaque atom boundaries"
        );
    }

    #[tokio::test]
    async fn prepared_inline_line_record_keeps_plaintext_alignment_paint_local() {
        let options = RenderOptions::default();
        let stylesheets = Vec::new();
        let resource_cache = ResourceCache::default();
        let mut builder = test_layout_builder(&options, &stylesheets, &resource_cache);
        let mut style = ComputedStyle::initial();
        style.font_family = css::FontFamily::SansSerif;
        style.font_size = 16.0;
        style.line_height = 20.0;
        style.text_align = TextAlign::Start;
        style.unicode_bidi = UnicodeBidi::Plaintext;
        style.direction = Direction::Ltr;
        builder.cursor_y = 100.0;

        let text = "אב";
        let measured_width = builder.font_system.measure_text(text, &style);
        let record = inline_line_record_for_items(
            vec![inline_layout::MeasuredInlineItem {
                item: InlineLineItem::Fragment(inline_fragment(text, style.clone())),
                width: measured_width,
                shaped: None,
            }],
            text,
            measured_width,
            120.0,
            &style,
        );
        let original_text = record.fragment.as_ref().unwrap().text.clone();
        let mut plaintext_state = None;
        let prepared = builder
            .prepare_inline_line_record(
                &record,
                inline_paragraph_context(&style, 120.0),
                &mut plaintext_state,
            )
            .expect("plaintext line should prepare");

        let group = prepared_text_groups(&prepared)[0];
        assert_eq!(plaintext_state, Some(Direction::Rtl));
        assert_eq!(record.fragment.as_ref().unwrap().text, original_text);
        assert!(
            group.x() > builder.content_left + 80.0,
            "RTL plaintext start should align right"
        );
    }

    #[tokio::test]
    async fn inline_text_measurement_splits_pre_line_paragraphs() {
        let options = RenderOptions::default();
        let stylesheets = Vec::new();
        let resource_cache = ResourceCache::default();
        let mut builder = test_layout_builder(&options, &stylesheets, &resource_cache);
        let mut style = ComputedStyle::initial();
        style.font_family = css::FontFamily::SansSerif;
        style.font_size = 16.0;
        style.line_height = 20.0;
        style.white_space = WhiteSpace::PreLine;
        let text = "alpha beta\ngamma";

        let alpha = builder.font_system.measure_line_text("alpha", &style);
        let beta = builder.font_system.measure_line_text("beta", &style);
        let gamma = builder.font_system.measure_line_text("gamma", &style);
        let first_line = builder.font_system.measure_line_text("alpha beta", &style);
        let measurement = builder.intrinsic_inline_measurement_for_text(text, &style, f32::MAX);

        assert_eq!(measurement.paragraphs.len(), 2);
        assert_eq!(measurement.line_count(), 2);
        assert_eq!(measurement.sequence.records.len(), 2);
        assert_eq!(
            measurement.sequence.records[0]
                .fragment
                .as_ref()
                .unwrap()
                .text(),
            "alpha beta"
        );
        assert_eq!(
            measurement.sequence.records[1]
                .fragment
                .as_ref()
                .unwrap()
                .text(),
            "gamma"
        );
        assert!((measurement.height() - 40.0).abs() < 0.01);
        assert!((measurement.contribution.min_content - alpha.max(beta).max(gamma)).abs() < 0.01);
        assert!((measurement.contribution.max_content - first_line.max(gamma)).abs() < 0.01);
    }

    #[tokio::test]
    async fn intrinsic_inline_measurement_uses_sequence_for_forced_empty_lines() {
        let options = RenderOptions::default();
        let stylesheets = Vec::new();
        let resource_cache = ResourceCache::default();
        let mut builder = test_layout_builder(&options, &stylesheets, &resource_cache);
        let mut style = ComputedStyle::initial();
        style.font_family = css::FontFamily::SansSerif;
        style.font_size = 12.0;
        style.line_height = 14.0;
        style.white_space = WhiteSpace::PreLine;

        let measurement =
            builder.intrinsic_inline_measurement_for_text("alpha\n\nbeta", &style, 200.0);

        assert_eq!(measurement.line_count(), 3);
        assert_eq!(measurement.sequence.records.len(), 3);
        assert_eq!(measurement.forced_empty_line_count(), 1);
        assert_eq!(
            measurement.sequence.records[0]
                .fragment
                .as_ref()
                .unwrap()
                .text(),
            "alpha"
        );
        assert!(measurement.sequence.records[1].is_forced_empty);
        assert_eq!(
            measurement.sequence.records[2]
                .fragment
                .as_ref()
                .unwrap()
                .text(),
            "beta"
        );
        assert!((measurement.height() - 42.0).abs() < 0.01);
    }

    #[tokio::test]
    async fn raw_text_sequence_preserves_forced_empty_lines() {
        let options = RenderOptions::default();
        let stylesheets = Vec::new();
        let resource_cache = ResourceCache::default();
        let mut builder = test_layout_builder(&options, &stylesheets, &resource_cache);
        let mut style = ComputedStyle::initial();
        style.font_family = css::FontFamily::SansSerif;
        style.font_size = 12.0;
        style.line_height = 14.0;
        style.white_space = WhiteSpace::PreLine;

        let sequence = builder.inline_line_sequence_for_raw_inline_text(
            "alpha\n\nbeta",
            &style,
            200.0,
            0.0,
            None,
        );

        assert_eq!(sequence.records.len(), 3);
        assert_eq!(
            sequence.records[0].fragment.as_ref().unwrap().text(),
            "alpha"
        );
        assert!(sequence.records[1].is_forced_empty);
        assert_eq!(
            sequence.records[2].fragment.as_ref().unwrap().text(),
            "beta"
        );
        assert!((sequence.total_height() - 42.0).abs() < 0.01);
    }

    #[tokio::test]
    async fn inline_line_sequence_keeps_generated_like_forced_break_records() {
        let options = RenderOptions::default();
        let stylesheets = Vec::new();
        let resource_cache = ResourceCache::default();
        let mut builder = test_layout_builder(&options, &stylesheets, &resource_cache);
        let mut style = ComputedStyle::initial();
        style.font_family = css::FontFamily::SansSerif;
        style.font_size = 12.0;
        style.line_height = 14.0;
        let items = vec![
            inline_word("prefix", &style),
            InlineItem::Break(InlineBreak::default()),
            InlineItem::Break(InlineBreak::default()),
            inline_word("suffix", &style),
        ];

        let sequence = builder.collect_inline_line_sequence(items, &style, 200.0, 0.0, 0.0);

        assert_eq!(sequence.records.len(), 3);
        assert_eq!(
            sequence_fragment_texts(&sequence),
            vec!["prefix", "", "suffix"]
        );
        assert!(sequence.records[1].is_forced_empty);
        assert!((sequence.total_height() - 42.0).abs() < 0.01);
    }

    #[tokio::test]
    async fn inline_line_sequence_resolves_generated_leaders_before_painting() {
        let options = RenderOptions::default();
        let stylesheets = Vec::new();
        let resource_cache = ResourceCache::default();
        let mut builder = test_layout_builder(&options, &stylesheets, &resource_cache);
        let mut style = ComputedStyle::initial();
        style.font_family = css::FontFamily::Monospace;
        style.font_size = 10.0;
        style.line_height = 12.0;
        let items = vec![
            inline_word("Chapter", &style),
            inline_leader(".", &style),
            inline_word("2", &style),
        ];

        let sequence = builder.collect_inline_line_sequence(items, &style, 120.0, 0.0, 0.0);
        let fragment = sequence.records[0].fragment.as_ref().unwrap();
        let leader_fragments = fragment
            .items
            .iter()
            .filter_map(|item| match &item.item {
                InlineLineItem::Fragment(fragment) if fragment.generated_leader() => Some(fragment),
                _ => None,
            })
            .collect::<Vec<_>>();

        assert_eq!(leader_fragments.len(), 1);
        assert!(
            leader_fragments[0]
                .text()
                .chars()
                .all(|character| character == '.')
        );
        assert!(leader_fragments[0].text().len() > 1);
        assert_eq!(
            leader_fragments[0].link_target(),
            Some("https://example.test/")
        );
        assert!(
            fragment
                .items()
                .iter()
                .all(|item| !matches!(&item.item, InlineLineItem::Atom(atom) if matches!(atom.content(), InlineAtomContent::Leader(_))))
        );
        assert_eq!(
            fragment.text(),
            format!("Chapter{}2", leader_fragments[0].text())
        );
    }

    #[tokio::test]
    async fn inline_line_sequence_divides_multiple_generated_leaders() {
        let options = RenderOptions::default();
        let stylesheets = Vec::new();
        let resource_cache = ResourceCache::default();
        let mut builder = test_layout_builder(&options, &stylesheets, &resource_cache);
        let mut style = ComputedStyle::initial();
        style.font_family = css::FontFamily::Monospace;
        style.font_size = 10.0;
        style.line_height = 12.0;
        let items = vec![
            inline_word("A", &style),
            inline_leader(".", &style),
            inline_word("B", &style),
            inline_leader("_", &style),
            inline_word("C", &style),
        ];

        let sequence = builder.collect_inline_line_sequence(items, &style, 120.0, 0.0, 0.0);
        let fragment = sequence.records[0].fragment.as_ref().unwrap();
        let leader_texts = fragment
            .items()
            .iter()
            .filter_map(|item| match &item.item {
                InlineLineItem::Fragment(fragment) if fragment.generated_leader() => {
                    Some(fragment.text().to_string())
                }
                _ => None,
            })
            .collect::<Vec<_>>();

        assert_eq!(leader_texts.len(), 2);
        assert!(leader_texts[0].chars().all(|character| character == '.'));
        assert!(leader_texts[1].chars().all(|character| character == '_'));
        assert!(leader_texts[0].len().abs_diff(leader_texts[1].len()) <= 1);
        assert_eq!(
            fragment.text(),
            format!("A{}B{}C", leader_texts[0], leader_texts[1])
        );
    }

    #[tokio::test]
    async fn inline_line_sequence_drops_empty_generated_leaders() {
        let options = RenderOptions::default();
        let stylesheets = Vec::new();
        let resource_cache = ResourceCache::default();
        let mut builder = test_layout_builder(&options, &stylesheets, &resource_cache);
        let mut style = ComputedStyle::initial();
        style.font_family = css::FontFamily::Monospace;
        style.font_size = 10.0;
        style.line_height = 12.0;
        let items = vec![
            inline_word("A", &style),
            inline_leader("", &style),
            inline_word("C", &style),
        ];

        let sequence = builder.collect_inline_line_sequence(items, &style, 120.0, 0.0, 0.0);
        let fragment = sequence.records[0].fragment.as_ref().unwrap();

        assert_eq!(fragment.text(), "AC");
        assert!(
            fragment
                .items()
                .iter()
                .all(|item| !matches!(&item.item, InlineLineItem::Fragment(fragment) if fragment.generated_leader()))
        );
    }

    #[tokio::test]
    async fn generated_leader_fragments_are_not_justification_opportunities() {
        let mut style = ComputedStyle::initial();
        style.font_family = css::FontFamily::Monospace;
        let normal_space = inline_fragment("   ", style);
        let mut leader_space = normal_space.clone();
        leader_space.set_mergeable(false);
        leader_space.set_generated_leader(true);

        assert!(inline_fragment_is_inter_word_justification_space(
            &normal_space
        ));
        assert!(!inline_fragment_is_inter_word_justification_space(
            &leader_space
        ));
        let plan = InlineJustificationPlan::for_line(
            &[InlineLineItem::Fragment(leader_space)],
            TextJustify::InterCharacter,
            true,
        );
        assert_eq!(plan.expansion_opportunity_count(), 0);
    }

    #[tokio::test]
    async fn inline_opportunity_graph_records_break_spaces_before_atoms() {
        let options = RenderOptions::default();
        let stylesheets = Vec::new();
        let resource_cache = ResourceCache::default();
        let mut builder = test_layout_builder(&options, &stylesheets, &resource_cache);
        let mut style = ComputedStyle::initial();
        style.white_space = WhiteSpace::BreakSpaces;
        style.font_family = css::FontFamily::SansSerif;
        let items = vec![
            InlineItem::Word(Box::new(InlineWord {
                text: "A".to_string(),
                style: inline_style(&style),
                baseline_shift: 0.0,
                visual_offset: InlineVisualOffset::zero(),
                link_target: None,
                mergeable: true,
                source: InlineTextSource::Normal,
                hanging_edges: InlineHangingEdges::default(),
                ancestor_inline_decorations: Vec::new(),
            })),
            InlineItem::Word(Box::new(InlineWord {
                text: " ".to_string(),
                style: inline_style(&style),
                baseline_shift: 0.0,
                visual_offset: InlineVisualOffset::zero(),
                link_target: None,
                mergeable: true,
                source: InlineTextSource::Normal,
                hanging_edges: InlineHangingEdges::default(),
                ancestor_inline_decorations: Vec::new(),
            })),
            InlineItem::Atom(Box::new(InlineAtom::new(
                InlineAtomContent::InlineBox {
                    sequence: empty_inline_sequence(),
                },
                style.clone(),
                None,
                5.0,
                0.0,
                0.0,
                0.0,
                None,
                None,
            ))),
            InlineItem::Word(Box::new(InlineWord {
                text: "B".to_string(),
                style: inline_style(&style),
                baseline_shift: 0.0,
                visual_offset: InlineVisualOffset::zero(),
                link_target: None,
                mergeable: true,
                source: InlineTextSource::Normal,
                hanging_edges: InlineHangingEdges::default(),
                ancestor_inline_decorations: Vec::new(),
            })),
        ];

        let graph = builder.build_inline_opportunity_graph(&items, &style);

        assert!(graph.opportunities.iter().any(|opportunity| {
            opportunity.kind == inline_layout::InlineBreakKind::BreakSpaces
        }));
        assert!(graph.opportunities.iter().any(|opportunity| {
            opportunity.kind == inline_layout::InlineBreakKind::AtomicBoundary
        }));
    }

    #[tokio::test]
    async fn inline_opportunity_graph_preserves_float_marker_source_order_without_width() {
        let options = RenderOptions::default();
        let stylesheets = Vec::new();
        let resource_cache = ResourceCache::default();
        let mut builder = test_layout_builder(&options, &stylesheets, &resource_cache);
        let mut style = ComputedStyle::initial();
        style.font_family = css::FontFamily::SansSerif;
        style.font_size = 12.0;
        style.line_height = 14.0;
        let items = vec![
            inline_word("A", &style),
            inline_test_float(&style),
            inline_word("B", &style),
        ];

        let graph = builder.build_inline_opportunity_graph(&items, &style);

        assert_eq!(graph.runs.len(), 3);
        assert!(matches!(graph.runs[1].item, InlineLineItem::Float(_)));
        assert_eq!(graph.runs[1].width, 0.0);
        assert_eq!(
            graph.first_float_position_in_range(inline_layout::InlineGraphRange {
                start: graph.start_position(),
                end: graph.end_position(),
            }),
            Some(inline_layout::InlineGraphPosition::at_run_start(1))
        );
    }

    #[tokio::test]
    async fn inline_opportunity_graph_intrinsic_contribution_uses_segments_and_atoms() {
        let options = RenderOptions::default();
        let stylesheets = Vec::new();
        let resource_cache = ResourceCache::default();
        let mut builder = test_layout_builder(&options, &stylesheets, &resource_cache);
        let mut style = ComputedStyle::initial();
        style.font_family = css::FontFamily::SansSerif;
        style.font_size = 12.0;
        style.line_height = 14.0;
        let items = vec![
            InlineItem::Word(Box::new(InlineWord {
                text: "alpha beta".to_string(),
                style: inline_style(&style),
                baseline_shift: 0.0,
                visual_offset: InlineVisualOffset::zero(),
                link_target: None,
                mergeable: true,
                source: InlineTextSource::Normal,
                hanging_edges: InlineHangingEdges::default(),
                ancestor_inline_decorations: Vec::new(),
            })),
            InlineItem::Atom(Box::new(InlineAtom::new(
                InlineAtomContent::InlineBox {
                    sequence: empty_inline_sequence(),
                },
                style.clone(),
                None,
                28.0,
                0.0,
                0.0,
                0.0,
                None,
                None,
            ))),
            InlineItem::Word(Box::new(InlineWord {
                text: "gamma".to_string(),
                style: inline_style(&style),
                baseline_shift: 0.0,
                visual_offset: InlineVisualOffset::zero(),
                link_target: None,
                mergeable: true,
                source: InlineTextSource::Normal,
                hanging_edges: InlineHangingEdges::default(),
                ancestor_inline_decorations: Vec::new(),
            })),
        ];

        let graph = builder.build_inline_opportunity_graph(&items, &style);
        let contribution = graph.intrinsic_contribution(&mut builder.font_system, &style);

        assert!(contribution.max_content > contribution.min_content);
        assert!(contribution.min_content >= 28.0);
        assert!(
            contribution.max_content > 28.0 + builder.font_system.measure_text("gamma", &style)
        );
    }

    #[tokio::test]
    async fn inline_opportunity_graph_records_soft_hyphen_inside_text_run() {
        let options = RenderOptions::default();
        let stylesheets = Vec::new();
        let resource_cache = ResourceCache::default();
        let mut builder = test_layout_builder(&options, &stylesheets, &resource_cache);
        let mut style = ComputedStyle::initial();
        style.font_family = css::FontFamily::SansSerif;
        style.font_size = 12.0;
        style.line_height = 14.0;
        let items = vec![InlineItem::Word(Box::new(InlineWord {
            text: "hyphen\u{00ad}ation".to_string(),
            style: inline_style(&style),
            baseline_shift: 0.0,
            visual_offset: InlineVisualOffset::zero(),
            link_target: None,
            mergeable: true,
            source: InlineTextSource::Normal,
            hanging_edges: InlineHangingEdges::default(),
            ancestor_inline_decorations: Vec::new(),
        }))];

        let graph = builder.build_inline_opportunity_graph(&items, &style);

        assert_eq!(graph.runs.len(), 1);
        assert!(graph.opportunities.iter().any(|opportunity| {
            opportunity.kind == inline_layout::InlineBreakKind::Hyphenation
                && opportunity.soft_hyphen
                && opportunity.position.run_index == 0
                && opportunity.position.byte_offset > 0
        }));
    }

    #[tokio::test]
    async fn inline_opportunity_graph_records_zero_width_space_inside_text_run() {
        let options = RenderOptions::default();
        let stylesheets = Vec::new();
        let resource_cache = ResourceCache::default();
        let mut builder = test_layout_builder(&options, &stylesheets, &resource_cache);
        let mut style = ComputedStyle::initial();
        style.font_family = css::FontFamily::SansSerif;
        style.font_size = 12.0;
        style.line_height = 14.0;
        let items = vec![InlineItem::Word(Box::new(InlineWord {
            text: "abc\u{200b}def".to_string(),
            style: inline_style(&style),
            baseline_shift: 0.0,
            visual_offset: InlineVisualOffset::zero(),
            link_target: None,
            mergeable: true,
            source: InlineTextSource::Normal,
            hanging_edges: InlineHangingEdges::default(),
            ancestor_inline_decorations: Vec::new(),
        }))];

        let graph = builder.build_inline_opportunity_graph(&items, &style);
        let contribution = graph.intrinsic_contribution(&mut builder.font_system, &style);

        assert_eq!(graph.runs.len(), 1);
        assert!(graph.opportunities.iter().any(|opportunity| {
            opportunity.kind == inline_layout::InlineBreakKind::SoftWrap
                && opportunity.position.run_index == 0
                && opportunity.position.byte_offset > 0
        }));
        assert!(contribution.max_content > contribution.min_content);
    }

    #[tokio::test]
    async fn inline_opportunity_graph_materializes_soft_hyphen_visibility() {
        let options = RenderOptions::default();
        let stylesheets = Vec::new();
        let resource_cache = ResourceCache::default();
        let mut builder = test_layout_builder(&options, &stylesheets, &resource_cache);
        let mut style = ComputedStyle::initial();
        style.font_family = css::FontFamily::SansSerif;
        style.font_size = 12.0;
        style.line_height = 14.0;
        let items = vec![inline_word("hyphen\u{00ad}ation", &style)];
        let graph = builder.build_inline_opportunity_graph(&items, &style);
        let hyphen_break = graph
            .opportunities
            .iter()
            .copied()
            .find(|opportunity| opportunity.soft_hyphen)
            .expect("soft hyphen should create a graph opportunity");

        let unbroken = graph.materialize_line(
            inline_layout::InlineGraphRange {
                start: graph.start_position(),
                end: graph.end_position(),
            },
            None,
            &mut builder.font_system,
            &style,
        );
        let broken = graph.materialize_line(
            inline_layout::InlineGraphRange {
                start: graph.start_position(),
                end: hyphen_break.position,
            },
            Some(hyphen_break),
            &mut builder.font_system,
            &style,
        );

        assert_eq!(unbroken.text, "hyphenation");
        assert_eq!(broken.text, "hyphen-");
    }

    #[tokio::test]
    async fn inline_opportunity_graph_materialization_strips_zero_width_space() {
        let options = RenderOptions::default();
        let stylesheets = Vec::new();
        let resource_cache = ResourceCache::default();
        let mut builder = test_layout_builder(&options, &stylesheets, &resource_cache);
        let mut style = ComputedStyle::initial();
        style.font_family = css::FontFamily::SansSerif;
        style.font_size = 12.0;
        style.line_height = 14.0;
        let items = vec![inline_word("abc\u{200b}def", &style)];
        let graph = builder.build_inline_opportunity_graph(&items, &style);

        let materialized = graph.materialize_line(
            inline_layout::InlineGraphRange {
                start: graph.start_position(),
                end: graph.end_position(),
            },
            None,
            &mut builder.font_system,
            &style,
        );

        assert_eq!(materialized.text, "abcdef");
        assert!(!materialized.text.contains('\u{200b}'));
        assert!(materialized.content_width > 0.0);
    }

    #[tokio::test]
    async fn inline_opportunity_graph_materialization_trims_collapsed_trailing_space() {
        let options = RenderOptions::default();
        let stylesheets = Vec::new();
        let resource_cache = ResourceCache::default();
        let mut builder = test_layout_builder(&options, &stylesheets, &resource_cache);
        let mut style = ComputedStyle::initial();
        style.font_family = css::FontFamily::SansSerif;
        style.font_size = 12.0;
        style.line_height = 14.0;
        let items = vec![inline_word("A", &style), inline_word(" ", &style)];
        let graph = builder.build_inline_opportunity_graph(&items, &style);

        let materialized = graph.materialize_line(
            inline_layout::InlineGraphRange {
                start: graph.start_position(),
                end: graph.end_position(),
            },
            None,
            &mut builder.font_system,
            &style,
        );

        assert_eq!(materialized.text, "A");
        assert!(materialized.trimmed_width > 0.0);
        assert_eq!(materialized.items.len(), 1);
    }

    #[tokio::test]
    async fn inline_opportunity_graph_materialization_hangs_pre_wrap_spaces_only_at_break() {
        let options = RenderOptions::default();
        let stylesheets = Vec::new();
        let resource_cache = ResourceCache::default();
        let mut builder = test_layout_builder(&options, &stylesheets, &resource_cache);
        let mut style = ComputedStyle::initial();
        style.font_family = css::FontFamily::SansSerif;
        style.font_size = 12.0;
        style.line_height = 14.0;
        style.white_space = WhiteSpace::PreWrap;
        let items = vec![
            inline_word("A", &style),
            inline_word("   ", &style),
            inline_word("B", &style),
        ];
        let graph = builder.build_inline_opportunity_graph(&items, &style);
        let space_break = graph
            .opportunities
            .iter()
            .copied()
            .find(|opportunity| opportunity.hangs && opportunity.position.run_index == 2)
            .expect("pre-wrap trailing spaces should create a hanging break");

        let broken = graph.materialize_line(
            inline_layout::InlineGraphRange {
                start: graph.start_position(),
                end: space_break.position,
            },
            Some(space_break),
            &mut builder.font_system,
            &style,
        );
        let unbroken = graph.materialize_line(
            inline_layout::InlineGraphRange {
                start: graph.start_position(),
                end: graph.end_position(),
            },
            None,
            &mut builder.font_system,
            &style,
        );

        assert_eq!(broken.text, "A");
        assert!(broken.hanging_space_width > 0.0);
        assert_eq!(unbroken.text, "A   B");
    }

    #[tokio::test]
    async fn inline_opportunity_graph_materialization_preserves_break_spaces() {
        let options = RenderOptions::default();
        let stylesheets = Vec::new();
        let resource_cache = ResourceCache::default();
        let mut builder = test_layout_builder(&options, &stylesheets, &resource_cache);
        let mut style = ComputedStyle::initial();
        style.font_family = css::FontFamily::SansSerif;
        style.font_size = 12.0;
        style.line_height = 14.0;
        style.white_space = WhiteSpace::BreakSpaces;
        let items = vec![inline_word("A", &style), inline_word(" ", &style)];
        let graph = builder.build_inline_opportunity_graph(&items, &style);

        let materialized = graph.materialize_line(
            inline_layout::InlineGraphRange {
                start: graph.start_position(),
                end: graph.end_position(),
            },
            None,
            &mut builder.font_system,
            &style,
        );

        assert_eq!(materialized.text, "A ");
        assert_eq!(materialized.items.len(), 2);
        assert_eq!(materialized.trimmed_width, 0.0);
    }

    #[tokio::test]
    async fn inline_opportunity_graph_materialization_preserves_metadata_after_control_stripping() {
        let options = RenderOptions::default();
        let stylesheets = Vec::new();
        let resource_cache = ResourceCache::default();
        let mut builder = test_layout_builder(&options, &stylesheets, &resource_cache);
        let mut style = ComputedStyle::initial();
        style.font_family = css::FontFamily::SansSerif;
        style.font_size = 12.0;
        style.line_height = 14.0;
        let items = vec![InlineItem::Word(Box::new(InlineWord {
            text: "a\u{200b}bc".to_string(),
            style: inline_style(&style),
            baseline_shift: 2.0,
            visual_offset: InlineVisualOffset::zero(),
            link_target: Some("https://example.test/".to_string()),
            mergeable: false,
            source: InlineTextSource::Normal,
            hanging_edges: InlineHangingEdges {
                blocks_start: true,
                blocks_end: true,
            },
            ancestor_inline_decorations: Vec::new(),
        }))];
        let graph = builder.build_inline_opportunity_graph(&items, &style);

        let materialized = graph.materialize_line(
            inline_layout::InlineGraphRange {
                start: graph.start_position(),
                end: graph.end_position(),
            },
            None,
            &mut builder.font_system,
            &style,
        );

        let [item] = materialized.items.as_slice() else {
            panic!("control stripping should leave one text fragment");
        };
        let InlineLineItem::Fragment(fragment) = &item.item else {
            panic!("materialized item should remain a text fragment");
        };
        assert_eq!(fragment.text(), "abc");
        assert_eq!(fragment.baseline_shift, 2.0);
        assert_eq!(fragment.link_target(), Some("https://example.test/"));
        assert!(!fragment.mergeable());
        assert!(fragment.hanging_edges().blocks_start);
        assert!(fragment.hanging_edges().blocks_end);
    }

    #[tokio::test]
    async fn inline_opportunity_graph_records_uax14_breaks_without_splitting_text_run() {
        let options = RenderOptions::default();
        let stylesheets = Vec::new();
        let resource_cache = ResourceCache::default();
        let mut builder = test_layout_builder(&options, &stylesheets, &resource_cache);
        let mut style = ComputedStyle::initial();
        style.font_family = css::FontFamily::SansSerif;
        style.font_size = 12.0;
        style.line_height = 14.0;
        let items = vec![InlineItem::Word(Box::new(InlineWord {
            text: "中文english中文".to_string(),
            style: inline_style(&style),
            baseline_shift: 0.0,
            visual_offset: InlineVisualOffset::zero(),
            link_target: None,
            mergeable: true,
            source: InlineTextSource::Normal,
            hanging_edges: InlineHangingEdges::default(),
            ancestor_inline_decorations: Vec::new(),
        }))];

        let graph = builder.build_inline_opportunity_graph(&items, &style);

        assert_eq!(graph.runs.len(), 1);
        assert!(graph.opportunities.iter().any(|opportunity| {
            opportunity.kind == inline_layout::InlineBreakKind::SoftWrap
                && opportunity.position.run_index == 0
                && opportunity.position.byte_offset > 0
                && opportunity.position.byte_offset < "中文english中文".len()
        }));
    }

    #[tokio::test]
    async fn inline_opportunity_graph_distinguishes_anywhere_from_break_word_min_content() {
        let options = RenderOptions::default();
        let stylesheets = Vec::new();
        let resource_cache = ResourceCache::default();
        let mut builder = test_layout_builder(&options, &stylesheets, &resource_cache);
        let mut anywhere = ComputedStyle::initial();
        anywhere.font_family = css::FontFamily::SansSerif;
        anywhere.font_size = 12.0;
        anywhere.line_height = 14.0;
        anywhere.overflow_wrap = css::OverflowWrap::Anywhere;
        let anywhere_items = vec![InlineItem::Word(Box::new(InlineWord {
            text: "abcdefgh".to_string(),
            style: inline_style(&anywhere),
            baseline_shift: 0.0,
            visual_offset: InlineVisualOffset::zero(),
            link_target: None,
            mergeable: true,
            source: InlineTextSource::Normal,
            hanging_edges: InlineHangingEdges::default(),
            ancestor_inline_decorations: Vec::new(),
        }))];

        let anywhere_graph = builder.build_inline_opportunity_graph(&anywhere_items, &anywhere);
        let anywhere_contribution =
            anywhere_graph.intrinsic_contribution(&mut builder.font_system, &anywhere);

        assert!(anywhere_graph.opportunities.iter().any(|opportunity| {
            opportunity.position.run_index == 0
                && opportunity.position.byte_offset > 0
                && opportunity.min_content
        }));
        assert!(anywhere_contribution.max_content > anywhere_contribution.min_content);

        let mut break_word = anywhere.clone();
        break_word.overflow_wrap = css::OverflowWrap::BreakWord;
        let break_word_items = vec![InlineItem::Word(Box::new(InlineWord {
            text: "abcdefgh".to_string(),
            style: inline_style(&break_word),
            baseline_shift: 0.0,
            visual_offset: InlineVisualOffset::zero(),
            link_target: None,
            mergeable: true,
            source: InlineTextSource::Normal,
            hanging_edges: InlineHangingEdges::default(),
            ancestor_inline_decorations: Vec::new(),
        }))];

        let break_word_graph =
            builder.build_inline_opportunity_graph(&break_word_items, &break_word);
        let break_word_contribution =
            break_word_graph.intrinsic_contribution(&mut builder.font_system, &break_word);

        assert!(break_word_graph.opportunities.iter().any(|opportunity| {
            opportunity.kind == inline_layout::InlineBreakKind::Emergency
                && opportunity.position.run_index == 0
                && opportunity.position.byte_offset > 0
                && !opportunity.min_content
        }));
        assert!(
            (break_word_contribution.max_content - break_word_contribution.min_content).abs()
                < 0.01
        );
    }

    #[tokio::test]
    async fn inline_opportunity_graph_partial_run_materialization_preserves_fragment_metadata() {
        let options = RenderOptions::default();
        let stylesheets = Vec::new();
        let resource_cache = ResourceCache::default();
        let mut builder = test_layout_builder(&options, &stylesheets, &resource_cache);
        let mut style = ComputedStyle::initial();
        style.font_family = css::FontFamily::SansSerif;
        style.font_size = 12.0;
        style.line_height = 14.0;
        style.overflow_wrap = css::OverflowWrap::Anywhere;
        let items = vec![InlineItem::Word(Box::new(InlineWord {
            text: "abcdef".to_string(),
            style: inline_style(&style),
            baseline_shift: 3.0,
            visual_offset: InlineVisualOffset::zero(),
            link_target: Some("https://example.test/".to_string()),
            mergeable: false,
            source: InlineTextSource::Normal,
            hanging_edges: InlineHangingEdges {
                blocks_start: true,
                blocks_end: true,
            },
            ancestor_inline_decorations: Vec::new(),
        }))];
        let graph = builder.build_inline_opportunity_graph(&items, &style);
        let opportunity = graph
            .opportunities
            .iter()
            .copied()
            .find(|opportunity| {
                opportunity.position.run_index == 0 && opportunity.position.byte_offset > 0
            })
            .expect("anywhere should expose an internal graph opportunity");

        let measured = graph.line_measured_items_for_graph_range(
            inline_layout::InlineGraphRange {
                start: graph.start_position(),
                end: opportunity.position,
            },
            &mut builder.font_system,
        );

        let [item] = measured.as_slice() else {
            panic!("partial graph range should materialize one fragment");
        };
        let InlineLineItem::Fragment(fragment) = &item.item else {
            panic!("partial text range should remain a text fragment");
        };
        assert!(fragment.text().len() < "abcdef".len());
        assert_eq!(fragment.style().font_size, style.font_size);
        assert_eq!(fragment.baseline_shift, 3.0);
        assert_eq!(fragment.link_target(), Some("https://example.test/"));
        assert!(!fragment.mergeable());
        assert!(fragment.hanging_edges().blocks_start);
        assert!(!fragment.hanging_edges().blocks_end);
    }

    #[tokio::test]
    async fn inline_opportunity_graph_breaks_across_transparent_box_edges() {
        let options = RenderOptions::default();
        let stylesheets = Vec::new();
        let resource_cache = ResourceCache::default();
        let mut builder = test_layout_builder(&options, &stylesheets, &resource_cache);
        let mut style = ComputedStyle::initial();
        style.font_family = css::FontFamily::SansSerif;
        style.font_size = 12.0;
        style.line_height = 14.0;
        let items = vec![
            inline_word("中文", &style),
            inline_box_edge(3.0, &style),
            inline_word("english", &style),
        ];

        let graph = builder.build_inline_opportunity_graph(&items, &style);

        assert_eq!(graph.runs.len(), 3);
        assert!(graph.opportunities.iter().any(|opportunity| {
            opportunity.kind == inline_layout::InlineBreakKind::SoftWrap
                && opportunity.position.run_index == 1
                && opportunity.position.byte_offset == 0
        }));
        assert!(!graph.opportunities.iter().any(|opportunity| {
            opportunity.kind == inline_layout::InlineBreakKind::AtomicBoundary
                && opportunity.position.run_index == 1
        }));
    }

    #[tokio::test]
    async fn inline_opportunity_graph_preserves_space_breaks_after_transparent_box_edges() {
        let options = RenderOptions::default();
        let stylesheets = Vec::new();
        let resource_cache = ResourceCache::default();
        let mut builder = test_layout_builder(&options, &stylesheets, &resource_cache);
        let mut style = ComputedStyle::initial();
        style.font_family = css::FontFamily::SansSerif;
        style.font_size = 12.0;
        style.line_height = 14.0;
        let items = vec![
            inline_word("ab", &style),
            inline_box_edge(2.0, &style),
            inline_word(" cd", &style),
        ];

        let graph = builder.build_inline_opportunity_graph(&items, &style);

        assert_eq!(graph.runs.len(), 3);
        assert!(graph.opportunities.iter().any(|opportunity| {
            opportunity.position.run_index == 2 && opportunity.position.byte_offset > 0
        }));
    }

    #[tokio::test]
    async fn inline_opportunity_graph_keeps_real_atoms_atomic() {
        let options = RenderOptions::default();
        let stylesheets = Vec::new();
        let resource_cache = ResourceCache::default();
        let mut builder = test_layout_builder(&options, &stylesheets, &resource_cache);
        let mut style = ComputedStyle::initial();
        style.font_family = css::FontFamily::SansSerif;
        style.font_size = 12.0;
        style.line_height = 14.0;
        let items = vec![
            inline_word("A", &style),
            inline_test_atom(8.0, &style),
            inline_word("B", &style),
        ];

        let graph = builder.build_inline_opportunity_graph(&items, &style);

        assert!(graph.opportunities.iter().any(|opportunity| {
            opportunity.kind == inline_layout::InlineBreakKind::AtomicBoundary
                && opportunity.position.run_index == 1
        }));
        assert!(graph.opportunities.iter().any(|opportunity| {
            opportunity.kind == inline_layout::InlineBreakKind::AtomicBoundary
                && opportunity.position.run_index == 2
        }));
    }

    #[tokio::test]
    async fn inline_opportunity_graph_tracks_across_transparent_box_edges() {
        let options = RenderOptions::default();
        let stylesheets = Vec::new();
        let resource_cache = ResourceCache::default();
        let mut builder = test_layout_builder(&options, &stylesheets, &resource_cache);
        let mut style = ComputedStyle::initial();
        style.font_family = css::FontFamily::SansSerif;
        style.font_size = 12.0;
        style.line_height = 14.0;
        style.letter_spacing = css::ComputedLengthPercentage::from_points(1.0);
        let items = vec![
            inline_word("A", &style),
            inline_box_edge(2.0, &style),
            inline_word("B", &style),
        ];

        let graph = builder.build_inline_opportunity_graph(&items, &style);

        assert!(graph.opportunities.iter().any(|opportunity| {
            opportunity.kind == inline_layout::InlineBreakKind::SoftWrap
                && opportunity.position.run_index == 1
        }));
    }

    #[tokio::test]
    async fn inline_opportunity_graph_materializes_ranges_with_transparent_box_edges() {
        let options = RenderOptions::default();
        let stylesheets = Vec::new();
        let resource_cache = ResourceCache::default();
        let mut builder = test_layout_builder(&options, &stylesheets, &resource_cache);
        let mut style = ComputedStyle::initial();
        style.font_family = css::FontFamily::SansSerif;
        style.font_size = 12.0;
        style.line_height = 14.0;
        let items = vec![
            InlineItem::Word(Box::new(InlineWord {
                text: "ab".to_string(),
                style: inline_style(&style),
                baseline_shift: 2.0,
                visual_offset: InlineVisualOffset::zero(),
                link_target: Some("https://example.test/".to_string()),
                mergeable: false,
                source: InlineTextSource::Normal,
                hanging_edges: InlineHangingEdges::default(),
                ancestor_inline_decorations: Vec::new(),
            })),
            inline_box_edge(2.0, &style),
            inline_word(" cd", &style),
        ];
        let graph = builder.build_inline_opportunity_graph(&items, &style);
        let opportunity = graph
            .opportunities
            .iter()
            .copied()
            .find(|opportunity| {
                opportunity.position.run_index == 2 && opportunity.position.byte_offset > 0
            })
            .expect("space break after a transparent edge should be graph-backed");

        let measured = graph.line_measured_items_for_graph_range(
            inline_layout::InlineGraphRange {
                start: graph.start_position(),
                end: opportunity.position,
            },
            &mut builder.font_system,
        );

        assert_eq!(measured.len(), 3);
        assert!(matches!(
            &measured[1].item,
            InlineLineItem::Atom(atom)
                if matches!(atom.content(), InlineAtomContent::InlineEdge(InlineEdgeRole::BoxEdge(_)))
        ));
        let InlineLineItem::Fragment(fragment) = &measured[0].item else {
            panic!("first item should remain the original text fragment");
        };
        assert_eq!(fragment.baseline_shift, 2.0);
        assert_eq!(fragment.link_target(), Some("https://example.test/"));
        assert!(!fragment.mergeable());
    }

    #[tokio::test]
    async fn inline_line_fragment_preserves_graph_text_summary() {
        let options = RenderOptions::default();
        let stylesheets = Vec::new();
        let resource_cache = ResourceCache::default();
        let mut builder = test_layout_builder(&options, &stylesheets, &resource_cache);
        let mut style = ComputedStyle::initial();
        style.font_family = css::FontFamily::SansSerif;
        style.font_size = 16.0;
        style.line_height = 20.0;
        builder.cursor_y = 100.0;
        let items = vec![InlineItem::Word(Box::new(InlineWord {
            text: "Hello".to_string(),
            style: inline_style(&style),
            baseline_shift: 0.0,
            visual_offset: InlineVisualOffset::zero(),
            link_target: None,
            mergeable: true,
            source: InlineTextSource::Normal,
            hanging_edges: InlineHangingEdges::default(),
            ancestor_inline_decorations: Vec::new(),
        }))];
        let graph = builder.build_inline_opportunity_graph(&items, &style);
        let context = InlineParagraphContext {
            block_style: &style,
            stylesheets: &[],
            available_width: 200.0,
            padding_left: 0.0,
            hanging_indent: 0.0,
            hanging_punctuation_reserve: 0.0,
        };

        let (lines, _) = builder.select_inline_lines_from_graph(&graph, context, 0, false);

        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].text(), "Hello");
        assert_eq!(lines[0].items().len(), 1);
    }

    #[tokio::test]
    async fn used_border_preserves_layout_width_but_hides_non_painting_sides() {
        let mut style = ComputedStyle::initial();
        style.border_widths.top = 4.0;
        style.border_widths.right = 3.0;
        style.border_widths.bottom = 5.0;
        style.border_styles.top = BorderStyle::Hidden;
        style.border_styles.right = BorderStyle::Solid;
        style.border_styles.bottom = BorderStyle::Solid;
        style.border_colors.top = Color::new(255, 0, 0);
        style.border_colors.bottom = Color::TRANSPARENT;

        let border = used_border(&style);

        assert_eq!(border.top.specified_width, 4.0);
        assert_eq!(border.top.used_width, 0.0);
        assert!(!border.top.is_visible());
        assert_eq!(border.right.used_width, 3.0);
        assert!(border.right.is_visible());
        assert_eq!(border.bottom.used_width, 5.0);
        assert!(!border.bottom.is_visible());
    }

    #[tokio::test]
    async fn gap_decoration_helper_paints_flex_row_and_column_rules() {
        let mut style = ComputedStyle::initial();
        style.visibility = Visibility::Visible;
        style.column_rule.widths =
            css::GapRuleList::single(ComputedLengthPercentage::from_points(10.0));
        style.column_rule.styles = css::GapRuleList::single(BorderStyle::Solid);
        style.column_rule.colors = css::GapRuleList::single(Color::new(255, 0, 0));
        style.row_rule.widths =
            css::GapRuleList::single(ComputedLengthPercentage::from_points(30.0));
        style.row_rule.styles = css::GapRuleList::single(BorderStyle::Solid);
        style.row_rule.colors = css::GapRuleList::single(Color::new(0, 0, 255));
        let items = [
            GapDecorationItem::new(0.0, 0.0, 70.0, 50.0),
            GapDecorationItem::new(80.0, 0.0, 70.0, 50.0),
            GapDecorationItem::new(160.0, 0.0, 70.0, 50.0),
            GapDecorationItem::new(0.0, 80.0, 70.0, 50.0),
            GapDecorationItem::new(80.0, 80.0, 70.0, 50.0),
            GapDecorationItem::new(160.0, 80.0, 70.0, 50.0),
        ];

        let gutters = GapDecorationGutters {
            columns: vec![
                GapDecorationGutter::new(70.0, 80.0),
                GapDecorationGutter::new(150.0, 160.0),
            ],
            rows: vec![GapDecorationGutter::new(50.0, 80.0)],
        };
        let primitives = flex_gap_decoration_primitives_with_gutters(
            &style, 0.0, 130.0, 230.0, 130.0, &items, &gutters,
        );
        let strokes = primitives
            .iter()
            .filter_map(|primitive| match primitive {
                PaintPrimitive::Stroke(stroke) => Some(*stroke),
                _ => None,
            })
            .collect::<Vec<_>>();

        assert_eq!(strokes.len(), 3);
        assert!(
            strokes.iter().any(|stroke| {
                stroke.x1() == 75.0 && stroke.x2() == 75.0 && stroke.width == 10.0
            })
        );
        assert!(strokes.iter().any(|stroke| {
            stroke.x1() == 155.0 && stroke.x2() == 155.0 && stroke.width == 10.0
        }));
        assert!(
            strokes.iter().any(|stroke| {
                stroke.y1() == 65.0 && stroke.y2() == 65.0 && stroke.width == 30.0
            })
        );
    }

    #[tokio::test]
    async fn gap_decoration_helper_splits_intersection_segments_with_overlap_join() {
        let mut style = ComputedStyle::initial();
        style.visibility = Visibility::Visible;
        style.column_rule.widths =
            css::GapRuleList::single(ComputedLengthPercentage::from_points(10.0));
        style.column_rule.styles = css::GapRuleList::single(BorderStyle::Solid);
        style.column_rule.colors = css::GapRuleList::single(Color::new(255, 0, 0));
        style.column_rule.rule_break = css::GapRuleBreak::Intersection;
        style.column_rule.inset_junction_start = css::GapRuleInsetValue::OverlapJoin;
        style.column_rule.inset_junction_end = css::GapRuleInsetValue::OverlapJoin;
        style.row_rule.widths =
            css::GapRuleList::single(ComputedLengthPercentage::from_points(6.0));
        style.row_rule.styles = css::GapRuleList::single(BorderStyle::None);
        let items = [
            GapDecorationItem::new(0.0, 0.0, 70.0, 50.0),
            GapDecorationItem::new(80.0, 0.0, 70.0, 50.0),
            GapDecorationItem::new(0.0, 80.0, 70.0, 50.0),
            GapDecorationItem::new(80.0, 80.0, 70.0, 50.0),
        ];

        let gutters = GapDecorationGutters {
            columns: vec![GapDecorationGutter::new(70.0, 80.0)],
            rows: vec![GapDecorationGutter::new(50.0, 80.0)],
        };
        let primitives = flex_gap_decoration_primitives_with_gutters(
            &style, 0.0, 130.0, 150.0, 130.0, &items, &gutters,
        );
        let strokes = primitives
            .iter()
            .filter_map(|primitive| match primitive {
                PaintPrimitive::Stroke(stroke) => Some(*stroke),
                _ => None,
            })
            .collect::<Vec<_>>();

        assert_eq!(strokes.len(), 2);
        assert!(
            strokes.iter().any(|stroke| {
                stroke.x1() == 75.0 && stroke.y1() == 130.0 && stroke.y2() == 62.0
            })
        );
        assert!(
            strokes
                .iter()
                .any(|stroke| { stroke.x1() == 75.0 && stroke.y1() == 68.0 && stroke.y2() == 0.0 })
        );
    }

    #[tokio::test]
    async fn gap_decoration_helper_paints_grid_empty_track_gutters() {
        let mut style = ComputedStyle::initial();
        style.visibility = Visibility::Visible;
        style.column_rule.widths =
            css::GapRuleList::single(ComputedLengthPercentage::from_points(4.0));
        style.column_rule.styles = css::GapRuleList::single(BorderStyle::Solid);
        style.column_rule.colors = css::GapRuleList::single(Color::new(255, 0, 0));
        let gutters = grid_gap_decoration_gutters_from_tracks(
            &[50.0, 50.0, 50.0],
            &[0.0, 10.0, 10.0, 0.0],
            &[50.0],
            &[0.0, 0.0],
            &style,
            170.0,
            50.0,
        );

        let primitives =
            grid_gap_decoration_primitives(&style, 0.0, 50.0, 170.0, 50.0, &[], &gutters);
        let strokes = primitives
            .iter()
            .filter_map(|primitive| match primitive {
                PaintPrimitive::Stroke(stroke) => Some(*stroke),
                _ => None,
            })
            .collect::<Vec<_>>();

        assert_eq!(strokes.len(), 2);
        assert!(
            strokes.iter().any(|stroke| {
                stroke.x1() == 55.0 && stroke.x2() == 55.0 && stroke.width == 4.0
            })
        );
        assert!(
            strokes.iter().any(|stroke| {
                stroke.x1() == 115.0 && stroke.x2() == 115.0 && stroke.width == 4.0
            })
        );
    }

    #[tokio::test]
    async fn gap_decoration_helper_uses_grid_area_spans_for_intersections() {
        let mut style = ComputedStyle::initial();
        style.visibility = Visibility::Visible;
        style.column_rule.widths =
            css::GapRuleList::single(ComputedLengthPercentage::from_points(10.0));
        style.column_rule.styles = css::GapRuleList::single(BorderStyle::Solid);
        style.column_rule.colors = css::GapRuleList::single(Color::new(255, 0, 0));
        style.column_rule.rule_break = css::GapRuleBreak::Intersection;
        style.row_rule.widths =
            css::GapRuleList::single(ComputedLengthPercentage::from_points(6.0));
        style.row_rule.styles = css::GapRuleList::single(BorderStyle::None);
        let gutters = GapDecorationGutters {
            columns: vec![GapDecorationGutter::with_grid_line(70.0, 80.0, Some(2))],
            rows: vec![GapDecorationGutter::with_grid_line(50.0, 80.0, Some(2))],
        };
        let items = [
            GapDecorationItem::with_grid_area(
                0.0,
                0.0,
                0.0,
                0.0,
                GapDecorationGridArea {
                    row_start: 1,
                    row_end: 3,
                    column_start: 1,
                    column_end: 2,
                },
            ),
            GapDecorationItem::with_grid_area(
                80.0,
                0.0,
                0.0,
                0.0,
                GapDecorationGridArea {
                    row_start: 1,
                    row_end: 3,
                    column_start: 2,
                    column_end: 3,
                },
            ),
        ];

        let primitives =
            grid_gap_decoration_primitives(&style, 0.0, 130.0, 150.0, 130.0, &items, &gutters);
        let strokes = primitives
            .iter()
            .filter_map(|primitive| match primitive {
                PaintPrimitive::Stroke(stroke) => Some(*stroke),
                _ => None,
            })
            .collect::<Vec<_>>();

        assert_eq!(strokes.len(), 2);
        assert!(
            strokes.iter().any(|stroke| {
                stroke.x1() == 75.0 && stroke.y1() == 130.0 && stroke.y2() == 80.0
            })
        );
        assert!(
            strokes
                .iter()
                .any(|stroke| { stroke.x1() == 75.0 && stroke.y1() == 80.0 && stroke.y2() == 0.0 })
        );
    }

    #[tokio::test]
    async fn gap_decoration_helper_does_not_join_single_grid_item_span_as_flanking_items() {
        let mut style = ComputedStyle::initial();
        style.visibility = Visibility::Visible;
        style.column_rule.widths =
            css::GapRuleList::single(ComputedLengthPercentage::from_points(10.0));
        style.column_rule.styles = css::GapRuleList::single(BorderStyle::Solid);
        style.column_rule.colors = css::GapRuleList::single(Color::new(255, 0, 0));
        style.column_rule.rule_break = css::GapRuleBreak::Intersection;
        style.row_rule.widths =
            css::GapRuleList::single(ComputedLengthPercentage::from_points(6.0));
        style.row_rule.styles = css::GapRuleList::single(BorderStyle::None);
        let gutters = GapDecorationGutters {
            columns: vec![GapDecorationGutter::with_grid_line(70.0, 80.0, Some(2))],
            rows: vec![GapDecorationGutter::with_grid_line(50.0, 80.0, Some(2))],
        };
        let items = [GapDecorationItem::with_grid_area(
            0.0,
            0.0,
            20.0,
            20.0,
            GapDecorationGridArea {
                row_start: 1,
                row_end: 3,
                column_start: 1,
                column_end: 3,
            },
        )];

        let primitives =
            grid_gap_decoration_primitives(&style, 0.0, 130.0, 150.0, 130.0, &items, &gutters);
        let strokes = primitives
            .iter()
            .filter_map(|primitive| match primitive {
                PaintPrimitive::Stroke(stroke) => Some(*stroke),
                _ => None,
            })
            .collect::<Vec<_>>();

        assert_eq!(strokes.len(), 2);
        assert!(
            strokes.iter().any(|stroke| {
                stroke.x1() == 75.0 && stroke.y1() == 130.0 && stroke.y2() == 80.0
            }),
            "first segment should stop before the unflanked row gap: {strokes:?}"
        );
        assert!(
            strokes
                .iter()
                .any(|stroke| { stroke.x1() == 75.0 && stroke.y1() == 50.0 && stroke.y2() == 0.0 }),
            "second segment should resume after the unflanked row gap: {strokes:?}"
        );
    }

    #[tokio::test]
    async fn gap_decoration_helper_uses_cap_endpoint_for_empty_grid_junction() {
        let mut style = ComputedStyle::initial();
        style.visibility = Visibility::Visible;
        style.column_rule.widths =
            css::GapRuleList::single(ComputedLengthPercentage::from_points(4.0));
        style.column_rule.styles = css::GapRuleList::single(BorderStyle::Solid);
        style.column_rule.colors = css::GapRuleList::single(Color::new(255, 0, 0));
        style.column_rule.rule_break = css::GapRuleBreak::Intersection;
        style.column_rule.inset_junction_end =
            css::GapRuleInsetValue::LengthPercentage(ComputedLengthPercentage::from_points(10.0));
        style.row_rule.widths =
            css::GapRuleList::single(ComputedLengthPercentage::from_points(4.0));
        style.row_rule.styles = css::GapRuleList::single(BorderStyle::Solid);
        style.row_rule.visibility_items = css::GapRuleVisibilityItems::Between;
        let gutters = GapDecorationGutters {
            columns: vec![GapDecorationGutter::with_grid_line(50.0, 60.0, Some(2))],
            rows: vec![GapDecorationGutter::with_grid_line(50.0, 60.0, Some(2))],
        };
        let items = [
            GapDecorationItem::with_grid_area(
                0.0,
                0.0,
                50.0,
                50.0,
                GapDecorationGridArea {
                    row_start: 1,
                    row_end: 2,
                    column_start: 1,
                    column_end: 2,
                },
            ),
            GapDecorationItem::with_grid_area(
                60.0,
                0.0,
                50.0,
                50.0,
                GapDecorationGridArea {
                    row_start: 1,
                    row_end: 2,
                    column_start: 2,
                    column_end: 3,
                },
            ),
        ];

        let primitives =
            grid_gap_decoration_primitives(&style, 0.0, 110.0, 110.0, 110.0, &items, &gutters);
        let strokes = primitives
            .iter()
            .filter_map(|primitive| match primitive {
                PaintPrimitive::Stroke(stroke) => Some(*stroke),
                _ => None,
            })
            .collect::<Vec<_>>();

        assert!(
            strokes.iter().any(|stroke| {
                stroke.x1() == 55.0 && stroke.y1() == 110.0 && stroke.y2() == 60.0
            }),
            "empty crossing segment should make the endpoint a cap, so junction inset must not shorten it: {strokes:?}"
        );
    }

    #[tokio::test]
    async fn gap_decoration_helper_uses_cap_endpoint_for_non_painting_crossing_rule() {
        let mut style = ComputedStyle::initial();
        style.visibility = Visibility::Visible;
        style.column_rule.widths =
            css::GapRuleList::single(ComputedLengthPercentage::from_points(4.0));
        style.column_rule.styles = css::GapRuleList::single(BorderStyle::Solid);
        style.column_rule.colors = css::GapRuleList::single(Color::new(255, 0, 0));
        style.column_rule.rule_break = css::GapRuleBreak::Intersection;
        style.column_rule.inset_junction_end =
            css::GapRuleInsetValue::LengthPercentage(ComputedLengthPercentage::from_points(10.0));
        style.row_rule.widths =
            css::GapRuleList::single(ComputedLengthPercentage::from_points(4.0));
        style.row_rule.styles = css::GapRuleList::single(BorderStyle::None);
        let gutters = GapDecorationGutters {
            columns: vec![GapDecorationGutter::with_grid_line(50.0, 60.0, Some(2))],
            rows: vec![GapDecorationGutter::with_grid_line(50.0, 60.0, Some(2))],
        };
        let items = [
            GapDecorationItem::with_grid_area(
                0.0,
                0.0,
                50.0,
                50.0,
                GapDecorationGridArea {
                    row_start: 1,
                    row_end: 2,
                    column_start: 1,
                    column_end: 2,
                },
            ),
            GapDecorationItem::with_grid_area(
                60.0,
                0.0,
                50.0,
                50.0,
                GapDecorationGridArea {
                    row_start: 1,
                    row_end: 2,
                    column_start: 2,
                    column_end: 3,
                },
            ),
            GapDecorationItem::with_grid_area(
                0.0,
                60.0,
                50.0,
                50.0,
                GapDecorationGridArea {
                    row_start: 2,
                    row_end: 3,
                    column_start: 1,
                    column_end: 2,
                },
            ),
            GapDecorationItem::with_grid_area(
                60.0,
                60.0,
                50.0,
                50.0,
                GapDecorationGridArea {
                    row_start: 2,
                    row_end: 3,
                    column_start: 2,
                    column_end: 3,
                },
            ),
        ];

        let primitives =
            grid_gap_decoration_primitives(&style, 0.0, 110.0, 110.0, 110.0, &items, &gutters);
        let strokes = primitives
            .iter()
            .filter_map(|primitive| match primitive {
                PaintPrimitive::Stroke(stroke) => Some(*stroke),
                _ => None,
            })
            .collect::<Vec<_>>();

        assert!(
            strokes.iter().any(|stroke| {
                stroke.x1() == 55.0 && stroke.y1() == 110.0 && stroke.y2() == 60.0
            }),
            "non-painting crossing rule should make the endpoint a cap, so junction inset must not shorten it: {strokes:?}"
        );
    }

    #[tokio::test]
    async fn gap_decoration_helper_uses_grid_area_spans_for_visibility_items() {
        let mut style = ComputedStyle::initial();
        style.visibility = Visibility::Visible;
        style.column_rule.widths =
            css::GapRuleList::single(ComputedLengthPercentage::from_points(4.0));
        style.column_rule.styles = css::GapRuleList::single(BorderStyle::Solid);
        style.column_rule.colors = css::GapRuleList::single(Color::new(255, 0, 0));
        style.column_rule.visibility_items = css::GapRuleVisibilityItems::Between;
        let gutters = GapDecorationGutters {
            columns: vec![GapDecorationGutter::with_grid_line(50.0, 60.0, Some(2))],
            rows: Vec::new(),
        };
        let items = [
            GapDecorationItem::with_grid_area(
                0.0,
                0.0,
                0.0,
                0.0,
                GapDecorationGridArea {
                    row_start: 1,
                    row_end: 2,
                    column_start: 1,
                    column_end: 2,
                },
            ),
            GapDecorationItem::with_grid_area(
                60.0,
                0.0,
                0.0,
                0.0,
                GapDecorationGridArea {
                    row_start: 1,
                    row_end: 2,
                    column_start: 2,
                    column_end: 3,
                },
            ),
        ];

        let primitives =
            grid_gap_decoration_primitives(&style, 0.0, 50.0, 110.0, 50.0, &items, &gutters);
        let strokes = primitives
            .iter()
            .filter_map(|primitive| match primitive {
                PaintPrimitive::Stroke(stroke) => Some(*stroke),
                _ => None,
            })
            .collect::<Vec<_>>();

        assert_eq!(strokes.len(), 1);
        assert!(
            strokes.iter().any(|stroke| {
                stroke.x1() == 55.0
                    && stroke.y1() == 50.0
                    && stroke.y2() == 0.0
                    && stroke.width == 4.0
            }),
            "grid area adjacency should make the between segment visible: {strokes:?}"
        );
    }

    #[tokio::test]
    async fn gap_decoration_helper_grid_normal_joins_cross_intersections() {
        let mut style = ComputedStyle::initial();
        style.visibility = Visibility::Visible;
        style.column_rule.widths =
            css::GapRuleList::single(ComputedLengthPercentage::from_points(4.0));
        style.column_rule.styles = css::GapRuleList::single(BorderStyle::Solid);
        style.column_rule.colors = css::GapRuleList::single(Color::new(255, 0, 0));
        style.row_rule.widths =
            css::GapRuleList::single(ComputedLengthPercentage::from_points(4.0));
        style.row_rule.styles = css::GapRuleList::single(BorderStyle::Solid);
        style.row_rule.colors = css::GapRuleList::single(Color::new(0, 0, 255));
        let gutters = GapDecorationGutters {
            columns: vec![GapDecorationGutter::with_grid_line(50.0, 60.0, Some(2))],
            rows: vec![GapDecorationGutter::with_grid_line(50.0, 60.0, Some(2))],
        };
        let items = [
            GapDecorationItem::with_grid_area(
                0.0,
                0.0,
                50.0,
                50.0,
                GapDecorationGridArea {
                    row_start: 1,
                    row_end: 2,
                    column_start: 1,
                    column_end: 2,
                },
            ),
            GapDecorationItem::with_grid_area(
                60.0,
                0.0,
                50.0,
                50.0,
                GapDecorationGridArea {
                    row_start: 1,
                    row_end: 2,
                    column_start: 2,
                    column_end: 3,
                },
            ),
            GapDecorationItem::with_grid_area(
                0.0,
                60.0,
                50.0,
                50.0,
                GapDecorationGridArea {
                    row_start: 2,
                    row_end: 3,
                    column_start: 1,
                    column_end: 2,
                },
            ),
            GapDecorationItem::with_grid_area(
                60.0,
                60.0,
                50.0,
                50.0,
                GapDecorationGridArea {
                    row_start: 2,
                    row_end: 3,
                    column_start: 2,
                    column_end: 3,
                },
            ),
        ];

        let primitives =
            grid_gap_decoration_primitives(&style, 0.0, 110.0, 110.0, 110.0, &items, &gutters);
        let strokes = primitives
            .iter()
            .filter_map(|primitive| match primitive {
                PaintPrimitive::Stroke(stroke) => Some(*stroke),
                _ => None,
            })
            .collect::<Vec<_>>();
        let column_strokes = strokes
            .iter()
            .copied()
            .filter(|stroke| stroke.color == Color::new(255, 0, 0))
            .collect::<Vec<_>>();

        assert_eq!(column_strokes.len(), 2);
        assert!(
            column_strokes.iter().any(|stroke| {
                stroke.x1() == 55.0 && stroke.y1() == 110.0 && stroke.y2() == 60.0
            }),
            "first normal grid segment should end at the cross-intersection start: {column_strokes:?}"
        );
        assert!(
            column_strokes
                .iter()
                .any(|stroke| { stroke.x1() == 55.0 && stroke.y1() == 60.0 && stroke.y2() == 0.0 }),
            "second normal grid segment should resume at the same cross-intersection point: {column_strokes:?}"
        );
    }

    #[tokio::test]
    async fn gap_decoration_helper_grid_normal_ignores_non_painting_crossing_rule() {
        let mut style = ComputedStyle::initial();
        style.visibility = Visibility::Visible;
        style.column_rule.widths =
            css::GapRuleList::single(ComputedLengthPercentage::from_points(4.0));
        style.column_rule.styles = css::GapRuleList::single(BorderStyle::Solid);
        style.column_rule.colors = css::GapRuleList::single(Color::new(255, 0, 0));
        style.row_rule.widths =
            css::GapRuleList::single(ComputedLengthPercentage::from_points(4.0));
        style.row_rule.styles = css::GapRuleList::single(BorderStyle::None);
        let gutters = GapDecorationGutters {
            columns: vec![GapDecorationGutter::with_grid_line(50.0, 60.0, Some(2))],
            rows: vec![GapDecorationGutter::with_grid_line(50.0, 60.0, Some(2))],
        };
        let items = [
            GapDecorationItem::with_grid_area(
                0.0,
                0.0,
                50.0,
                50.0,
                GapDecorationGridArea {
                    row_start: 1,
                    row_end: 2,
                    column_start: 1,
                    column_end: 2,
                },
            ),
            GapDecorationItem::with_grid_area(
                60.0,
                0.0,
                50.0,
                50.0,
                GapDecorationGridArea {
                    row_start: 1,
                    row_end: 2,
                    column_start: 2,
                    column_end: 3,
                },
            ),
        ];

        let primitives =
            grid_gap_decoration_primitives(&style, 0.0, 110.0, 110.0, 110.0, &items, &gutters);
        let strokes = primitives
            .iter()
            .filter_map(|primitive| match primitive {
                PaintPrimitive::Stroke(stroke) => Some(*stroke),
                _ => None,
            })
            .collect::<Vec<_>>();

        assert_eq!(strokes.len(), 1);
        assert!(
            strokes.iter().any(|stroke| {
                stroke.x1() == 55.0 && stroke.y1() == 110.0 && stroke.y2() == 0.0
            }),
            "grid normal should not split for a crossing rule that cannot paint: {strokes:?}"
        );
    }
}

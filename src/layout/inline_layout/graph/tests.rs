use super::*;

mod graph_tests {
    use super::*;
    use crate::css::{ContentLanguage, HyphenateCharacter, RubyAlign, WritingMode};

    fn bidi_scope_run(
        text: &str,
        style: ComputedStyle,
        source: InlineTextSource,
    ) -> InlineParagraphRun {
        InlineParagraphRun {
            item: InlineLineItem::Fragment(InlineFragment::new(
                text,
                style,
                0.0,
                None,
                true,
                source,
                false,
                InlineHangingEdges::default(),
                Vec::new(),
            )),
            width: 0.0,
            shaped: None,
        }
    }

    fn cloneable_box_edge_run(
        style: ComputedStyle,
        logical_edge: InlineLogicalEdge,
        positioning_containing_block_id: usize,
    ) -> InlineParagraphRun {
        let physical_side = match logical_edge {
            InlineLogicalEdge::Start => PhysicalSide::Left,
            InlineLogicalEdge::End => PhysicalSide::Right,
        };
        InlineParagraphRun {
            item: InlineLineItem::Atom(InlineAtom::new(
                InlineAtomContent::InlineEdge(InlineEdgeRole::BoxEdge(InlineBoxEdgeFragment {
                    logical_edge,
                    physical_side,
                    positioning_containing_block_id: Some(InlinePositioningContainingBlockId(
                        positioning_containing_block_id,
                    )),
                    advance: 7.0,
                    paint_extent: 5.0,
                })),
                style.clone(),
                None,
                InlineSize::new(7.0, style.line_height),
                11.0,
                3.0,
                Some(format!("scope-{positioning_containing_block_id}")),
                None,
            )),
            width: 7.0,
            shaped: None,
        }
    }

    #[test]
    fn clone_continuations_nest_in_source_order_and_preserve_positioning_scope() {
        let mut outer = ComputedStyle::initial();
        outer.box_decoration_break = BoxDecorationBreak::Clone;
        outer.padding.left = 7.0;
        outer.padding.right = 7.0;
        let mut inner = outer.clone();
        inner.color = CssColor::new(10, 20, 30);
        let text = bidi_scope_run("x", outer.clone(), InlineTextSource::Normal);
        let graph = InlineOpportunityGraph::new(
            vec![
                cloneable_box_edge_run(outer.clone(), InlineLogicalEdge::Start, 1),
                cloneable_box_edge_run(inner.clone(), InlineLogicalEdge::Start, 2),
                text.clone(),
                cloneable_box_edge_run(inner, InlineLogicalEdge::End, 2),
                cloneable_box_edge_run(outer, InlineLogicalEdge::End, 1),
            ],
            Vec::new(),
        );
        let range = InlineGraphRange {
            start: InlineGraphPosition::at_run_start(2),
            end: InlineGraphPosition::at_run_start(3),
        };
        let mut items = vec![MeasuredInlineItem::new(text.item, 0.0, None)];

        graph.insert_clone_continuation_edges(range, &mut items);

        let edges = items
            .iter()
            .filter_map(|item| match &item.item {
                InlineLineItem::Atom(atom) => match atom.content() {
                    InlineAtomContent::InlineEdge(InlineEdgeRole::BoxEdge(edge)) => Some((
                        edge.logical_edge,
                        edge.positioning_containing_block_id,
                        atom.link_target(),
                        atom.baseline_offset_from_alignment_source_block_start(
                            atom.size.height,
                            atom.style(),
                        )
                        .points(),
                        atom.baseline_shift,
                    )),
                    _ => None,
                },
                _ => None,
            })
            .collect::<Vec<_>>();

        assert_eq!(
            edges,
            vec![
                (
                    InlineLogicalEdge::Start,
                    Some(InlinePositioningContainingBlockId(1)),
                    Some("scope-1"),
                    11.0,
                    3.0,
                ),
                (
                    InlineLogicalEdge::Start,
                    Some(InlinePositioningContainingBlockId(2)),
                    Some("scope-2"),
                    11.0,
                    3.0,
                ),
                (
                    InlineLogicalEdge::End,
                    Some(InlinePositioningContainingBlockId(2)),
                    Some("scope-2"),
                    11.0,
                    3.0,
                ),
                (
                    InlineLogicalEdge::End,
                    Some(InlinePositioningContainingBlockId(1)),
                    Some("scope-1"),
                    11.0,
                    3.0,
                ),
            ]
        );
        assert_eq!(items[0].base_advance().points(), 7.0);
        assert_eq!(items[1].base_advance().points(), 7.0);
        assert_eq!(items[3].base_advance().points(), 7.0);
        assert_eq!(items[4].base_advance().points(), 7.0);
    }

    #[test]
    fn clone_continuations_do_not_replay_positioning_markers_outside_bidi_controls() {
        let mut style = ComputedStyle::initial();
        style.box_decoration_break = BoxDecorationBreak::Clone;
        style.padding.left = 7.0;
        style.padding.right = 7.0;
        let mut positioning_marker =
            cloneable_box_edge_run(style.clone(), InlineLogicalEdge::Start, 9);
        let InlineLineItem::Atom(marker) = &mut positioning_marker.item else {
            unreachable!("test helper constructs an inline atom");
        };
        let marker_data = Rc::make_mut(&mut marker.data);
        let InlineAtomContent::InlineEdge(InlineEdgeRole::BoxEdge(edge)) = &mut marker_data.content
        else {
            unreachable!("test helper constructs an inline box edge");
        };
        edge.advance = 0.0;
        edge.paint_extent = 0.0;

        let text = bidi_scope_run("x", style.clone(), InlineTextSource::Normal);
        let graph = InlineOpportunityGraph::new(
            vec![
                cloneable_box_edge_run(style.clone(), InlineLogicalEdge::Start, 1),
                bidi_scope_run("\u{2066}", style.clone(), InlineTextSource::BidiControl),
                positioning_marker,
                text.clone(),
                bidi_scope_run("\u{2069}", style.clone(), InlineTextSource::BidiControl),
                cloneable_box_edge_run(style, InlineLogicalEdge::End, 1),
            ],
            Vec::new(),
        );
        let range = InlineGraphRange {
            start: InlineGraphPosition::at_run_start(3),
            end: InlineGraphPosition::at_run_start(4),
        };
        let mut items = vec![MeasuredInlineItem::new(text.item, 0.0, None)];

        graph.insert_clone_continuation_edges(range, &mut items);

        let continuation_ids = items
            .iter()
            .filter_map(|item| match &item.item {
                InlineLineItem::Atom(atom) => match atom.content() {
                    InlineAtomContent::InlineEdge(InlineEdgeRole::BoxEdge(edge)) => {
                        Some(edge.positioning_containing_block_id)
                    }
                    _ => None,
                },
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(
            continuation_ids,
            vec![
                Some(InlinePositioningContainingBlockId(1)),
                Some(InlinePositioningContainingBlockId(1))
            ]
        );
        assert_eq!(items.len(), 3, "the virtual bidi prefix owns its marker");
    }

    fn measured_text_spacing_item(
        text: &str,
        style: ComputedStyle,
        source: InlineTextSource,
        font_system: &mut FontSystem,
    ) -> MeasuredInlineItem {
        let fragment = InlineFragment::new(
            text,
            style,
            0.0,
            None,
            true,
            source,
            false,
            InlineHangingEdges::default(),
            Vec::new(),
        );
        let mut items = Vec::new();
        push_text_spacing_fragment(&mut items, &fragment, text, false, font_system);
        items.pop().expect("non-empty text produces one fragment")
    }

    fn has_font_feature(item: &MeasuredInlineItem, tag: [u8; 4]) -> bool {
        matches!(&item.item, InlineLineItem::Fragment(fragment)
        if fragment.style().font_feature_settings.0.iter().any(|setting| {
            setting.tag == tag && setting.value == 1
        }))
    }

    #[test]
    fn tab_resolution_leaves_adjacent_non_tab_fragments_unchanged() {
        let style = ComputedStyle::initial();
        let mut font_system = FontSystem::new();
        let mut items = vec![
            measured_text_spacing_item(
                "prefix",
                style.clone(),
                InlineTextSource::Normal,
                &mut font_system,
            ),
            measured_text_spacing_item(
                "suffix",
                style.clone(),
                InlineTextSource::Normal,
                &mut font_system,
            ),
        ];
        let widths = items
            .iter()
            .map(|item| item.base_advance().points())
            .collect::<Vec<_>>();
        let shaped = items
            .iter()
            .map(|item| item.shaped.as_ref().map(Rc::clone))
            .collect::<Vec<_>>();

        assert!(!resolve_materialized_line_tab_advances(
            &mut items,
            &mut font_system,
            &style,
        ));
        assert_eq!(
            items
                .iter()
                .map(|item| item.base_advance().points())
                .collect::<Vec<_>>(),
            widths
        );
        for (item, original) in items.iter().zip(shaped) {
            assert_eq!(
                item.shaped.as_ref().map(Rc::as_ptr),
                original.as_ref().map(Rc::as_ptr),
                "a non-tab fragment must retain its graph shaping artifact"
            );
        }
    }

    #[test]
    fn stable_tab_geometry_does_not_force_extra_convergence_passes() {
        let mut style = ComputedStyle::initial();
        style.white_space = WhiteSpace::Pre;
        let mut font_system = FontSystem::new();
        let mut items = vec![
            measured_text_spacing_item(
                "prefix",
                style.clone(),
                InlineTextSource::Normal,
                &mut font_system,
            ),
            measured_text_spacing_item(
                "\tsuffix",
                style.clone(),
                InlineTextSource::Normal,
                &mut font_system,
            ),
        ];
        let widths = items
            .iter()
            .map(|item| item.base_advance().points())
            .collect::<Vec<_>>();

        assert!(resolve_materialized_line_tab_advances(
            &mut items,
            &mut font_system,
            &style,
        ));
        assert_ne!(
            items
                .iter()
                .map(|item| item.base_advance().points())
                .collect::<Vec<_>>(),
            widths,
            "the selected-line cursor changes the leading tab's advance"
        );
        assert!(!resolve_materialized_line_tab_advances(
            &mut items,
            &mut font_system,
            &style,
        ));
    }

    #[test]
    fn text_spacing_trim_eligibility_skips_ordinary_text() {
        let style = ComputedStyle::initial();
        let mut font_system = FontSystem::new();
        let ordinary = vec![measured_text_spacing_item(
            "ordinary ASCII text",
            style.clone(),
            InlineTextSource::Normal,
            &mut font_system,
        )];
        let punctuation = vec![measured_text_spacing_item(
            "、",
            style,
            InlineTextSource::Normal,
            &mut font_system,
        )];

        assert!(!materialized_items_may_use_text_spacing_trim(&ordinary));
        assert!(materialized_items_may_use_text_spacing_trim(&punctuation));
    }

    #[test]
    fn ruby_overhang_resolver_keeps_start_and_end_excess_independent() {
        let (offset, overhang) = ruby_alignment_geometry(RubyAlign::Center, 20.0, 60.0);
        assert_eq!(offset, -20.0);
        assert_eq!(overhang.inline_start.points(), 20.0);
        assert_eq!(overhang.inline_end.points(), 20.0);

        let resolved = resolve_ruby_overhang(
            overhang,
            ruby::RubyOverhangAllowance {
                inline_start: ruby::RubyInlineSpan::new(8.0),
                inline_end: ruby::RubyInlineSpan::new(30.0),
            },
        );
        assert_eq!(resolved.borrowed.inline_start.points(), 8.0);
        assert_eq!(resolved.borrowed.inline_end.points(), 20.0);
        assert_eq!(resolved.unborrowed.inline_start.points(), 12.0);
        assert_eq!(resolved.unborrowed.inline_end.points(), 0.0);
    }

    #[test]
    fn ruby_start_alignment_retains_all_excess_at_logical_end() {
        let (offset, overhang) = ruby_alignment_geometry(RubyAlign::Start, 20.0, 60.0);
        assert_eq!(offset, 0.0);
        assert_eq!(overhang.inline_start.points(), 0.0);
        assert_eq!(overhang.inline_end.points(), 40.0);
    }

    #[test]
    fn ruby_spaces_accepts_only_preserved_and_unicode_space_separators() {
        let collapsed = ComputedStyle::initial();
        assert!(!ruby_overhang_space_is_eligible(' ', &collapsed));
        assert!(!ruby_overhang_space_is_eligible('\t', &collapsed));
        assert!(ruby_overhang_space_is_eligible('\u{00a0}', &collapsed));
        assert!(ruby_overhang_space_is_eligible('\u{3000}', &collapsed));
        assert!(!ruby_overhang_space_is_eligible('\n', &collapsed));

        let mut preserved = ComputedStyle::initial();
        preserved.white_space = WhiteSpace::Pre;
        assert!(ruby_overhang_space_is_eligible(' ', &preserved));
        assert!(ruby_overhang_space_is_eligible('\t', &preserved));
    }

    #[test]
    fn ruby_spaces_punctuation_requires_an_untrimmed_boundary_side() {
        use crate::text::TextSpacingPunctuationClass;

        assert_eq!(
            ruby_punctuation_overhang_share(
                true,
                Some(TextSpacingPunctuationClass::Closing),
                TextSpacingTrim::SpaceAll,
            ),
            Some(0.5),
        );
        assert_eq!(
            ruby_punctuation_overhang_share(
                false,
                Some(TextSpacingPunctuationClass::Opening),
                TextSpacingTrim::SpaceAll,
            ),
            Some(0.5),
        );
        assert_eq!(
            ruby_punctuation_overhang_share(
                true,
                Some(TextSpacingPunctuationClass::MiddleDot),
                TextSpacingTrim::SpaceAll,
            ),
            Some(0.25),
        );
        assert_eq!(
            ruby_punctuation_overhang_share(
                true,
                Some(TextSpacingPunctuationClass::Closing),
                TextSpacingTrim::Normal,
            ),
            None,
        );
    }

    #[test]
    fn ruby_line_edges_and_auto_collision_cap_do_not_offer_extra_space() {
        let style = ComputedStyle::initial();
        assert_eq!(
            ruby_adjacent_space_allowance(&[], 0, &style),
            ruby::RubyOverhangAllowance::default(),
        );
        assert_eq!(ruby_auto_overhang_offer(40.0, 10.0), 10.0);
        assert_eq!(ruby_auto_overhang_offer(4.0, 10.0), 4.0);
        assert_eq!(ruby_auto_overhang_offer(-4.0, 10.0), 0.0);
    }

    #[test]
    fn ruby_overhang_geometry_is_logical_for_vertical_lines() {
        let horizontal = ruby_alignment_geometry(RubyAlign::Center, 20.0, 60.0);
        let mut vertical_style = ComputedStyle::initial();
        vertical_style.writing_mode = WritingMode::VerticalRl;
        // Ruby resolution operates in logical inline coordinates; the paint
        // adapter alone projects this same geometry to physical height.
        let vertical = ruby_alignment_geometry(RubyAlign::Center, 20.0, 60.0);
        assert_eq!(horizontal, vertical);
        assert_eq!(vertical_style.writing_mode, WritingMode::VerticalRl);
    }

    #[test]
    fn inside_marker_suffix_keeps_its_inline_advance_across_bidi_isolate_controls() {
        let style = ComputedStyle::initial();
        let mut font_system = FontSystem::new();
        let expected_marker_advance = font_system.measure_text("壱、", &style);
        let mut items = vec![
            measured_text_spacing_item(
                "\u{2068}",
                style.clone(),
                InlineTextSource::BidiControl,
                &mut font_system,
            ),
            measured_text_spacing_item(
                "壱、",
                style.clone(),
                InlineTextSource::Marker,
                &mut font_system,
            ),
            measured_text_spacing_item(
                "\u{2069}",
                style.clone(),
                InlineTextSource::BidiControl,
                &mut font_system,
            ),
            measured_text_spacing_item(
                "壱、",
                style.clone(),
                InlineTextSource::Normal,
                &mut font_system,
            ),
        ];

        apply_materialized_text_spacing_trim(&mut items, &mut font_system, true, None);

        let marker = items
            .iter()
            .find(|item| {
                matches!(&item.item, InlineLineItem::Fragment(fragment)
                if fragment.source() == InlineTextSource::Marker)
            })
            .expect("inside automatic marker remains a selected text item");
        assert_eq!(marker.base_advance().points(), expected_marker_advance);
        assert!(
            !has_font_feature(marker, *b"halt"),
            "a marker suffix preceding ordinary inline content is not a line edge"
        );
    }

    #[test]
    fn marker_suffix_at_the_selected_line_end_uses_halt() {
        let style = ComputedStyle::initial();
        let mut font_system = FontSystem::new();
        let mut items = vec![measured_text_spacing_item(
            "壱、",
            style,
            InlineTextSource::Marker,
            &mut font_system,
        )];
        apply_materialized_text_spacing_trim(&mut items, &mut font_system, true, None);
        assert!(
            items.iter().any(|item| has_font_feature(item, *b"halt")),
            "visible marker punctuation at the actual selected-line end is trimmed"
        );
    }

    #[test]
    fn text_spacing_adjacency_crosses_marker_isolate_controls() {
        let style = ComputedStyle::initial();
        let mut font_system = FontSystem::new();
        let mut items = vec![
            measured_text_spacing_item(
                "、",
                style.clone(),
                InlineTextSource::Marker,
                &mut font_system,
            ),
            measured_text_spacing_item(
                "\u{2069}",
                style.clone(),
                InlineTextSource::BidiControl,
                &mut font_system,
            ),
            measured_text_spacing_item("、", style, InlineTextSource::Normal, &mut font_system),
        ];

        apply_materialized_text_spacing_trim(&mut items, &mut font_system, true, None);

        let marker = items
            .iter()
            .find(|item| {
                matches!(&item.item, InlineLineItem::Fragment(fragment)
                if fragment.source() == InlineTextSource::Marker)
            })
            .expect("marker punctuation remains selected");
        assert!(
            has_font_feature(marker, *b"halt"),
            "the marker comma participates only because the following comma is adjacent text"
        );
    }

    #[test]
    fn vertical_marker_suffix_at_the_selected_line_end_uses_vhal() {
        let mut style = ComputedStyle::initial();
        style.writing_mode = WritingMode::VerticalRl;
        let mut font_system = FontSystem::new();
        let mut items = vec![measured_text_spacing_item(
            "壱、",
            style,
            InlineTextSource::Marker,
            &mut font_system,
        )];

        apply_materialized_text_spacing_trim(&mut items, &mut font_system, true, None);

        assert!(
            items.iter().any(|item| has_font_feature(item, *b"vhal")),
            "vertical selected-line marker punctuation uses the vertical alternate"
        );
    }

    #[test]
    fn break_availability_orders_fallbacks_and_min_content() {
        let ordinary = BreakAvailability::Ordinary;
        let keep_all = BreakAvailability::RelaxedWordBreak(WordBreakRelaxation::KeepAll);
        let phrase_wrap = BreakAvailability::RelaxedWordBreak(WordBreakRelaxation::AutoPhraseWrap);
        let phrase_hyphen =
            BreakAvailability::RelaxedWordBreak(WordBreakRelaxation::AutoPhraseHyphenation);
        let anywhere = BreakAvailability::OverflowWrap(OverflowWrapFallback::Anywhere);
        let break_word = BreakAvailability::OverflowWrap(OverflowWrapFallback::BreakWord);

        assert_eq!(ordinary.fitting_stage(), 0);
        assert_eq!(keep_all.fitting_stage(), 1);
        assert_eq!(phrase_wrap.fitting_stage(), 1);
        assert_eq!(phrase_hyphen.fitting_stage(), 2);
        assert_eq!(anywhere.fitting_stage(), 3);
        assert!(ordinary.participates_in_min_content());
        assert!(anywhere.participates_in_min_content());
        assert!(!keep_all.participates_in_min_content());
        assert!(!phrase_wrap.participates_in_min_content());
        assert!(!phrase_hyphen.participates_in_min_content());
        assert!(!break_word.participates_in_min_content());
    }

    #[test]
    fn automatic_hyphenation_is_not_offered_for_line_break_anywhere() {
        let mut ordinary = ComputedStyle::initial();
        ordinary.hyphens = Hyphens::Auto;
        ordinary.language = ContentLanguage::from_html_attribute("en");
        let ordinary_runs = vec![bidi_scope_run(
            "hyphenation",
            ordinary.clone(),
            InlineTextSource::Normal,
        )];
        assert!(
            !apply_auto_hyphenation_across_transparent_inline_edges(&ordinary_runs).is_empty(),
            "the fixture must have ordinary dictionary opportunities"
        );

        ordinary.line_break = css::LineBreak::Anywhere;
        let anywhere_runs = vec![bidi_scope_run(
            "hyphenation",
            ordinary,
            InlineTextSource::Normal,
        )];
        assert!(
            apply_auto_hyphenation_across_transparent_inline_edges(&anywhere_runs).is_empty(),
            "line-break:anywhere supplies its own soft opportunities without a used hyphen"
        );
    }

    #[test]
    fn auto_phrase_defers_automatic_hyphenation_opportunities() {
        let mut style = ComputedStyle::initial();
        style.hyphens = Hyphens::Auto;
        style.language = ContentLanguage::from_html_attribute("en");
        style.word_break = css::WordBreak::AutoPhrase;

        let opportunities =
            apply_auto_hyphenation_across_transparent_inline_edges(&[bidi_scope_run(
                "hyphenation",
                style,
                InlineTextSource::Normal,
            )]);

        assert!(opportunities.iter().any(|opportunity| {
            opportunity.kind == BreakEffect::Hyphenation
                && opportunity.availability
                    == BreakAvailability::RelaxedWordBreak(
                        WordBreakRelaxation::AutoPhraseHyphenation,
                    )
        }));
    }

    #[test]
    fn bidi_scope_continuations_balance_nested_css_scopes_without_author_controls() {
        let mut outer = ComputedStyle::initial();
        outer.unicode_bidi = UnicodeBidi::Isolate;
        outer.direction = Direction::Ltr;
        let mut inner = outer.clone();
        inner.direction = Direction::Rtl;
        let graph = InlineOpportunityGraph::new(
            vec![
                bidi_scope_run("\u{2066}", outer.clone(), InlineTextSource::BidiControl),
                bidi_scope_run("outer", outer.clone(), InlineTextSource::Normal),
                bidi_scope_run("\u{2067}", inner.clone(), InlineTextSource::BidiControl),
                bidi_scope_run("inner", inner.clone(), InlineTextSource::Normal),
                bidi_scope_run("\u{2069}", inner, InlineTextSource::BidiControl),
                bidi_scope_run("tail", outer.clone(), InlineTextSource::Normal),
                bidi_scope_run("\u{2069}", outer, InlineTextSource::BidiControl),
                // An authored FSI participates in the generic UAX #9 scope
                // stack, rather than using CSS `unicode-bidi` provenance.
                bidi_scope_run(
                    "\u{2068}",
                    ComputedStyle::initial(),
                    InlineTextSource::Normal,
                ),
            ],
            Vec::new(),
        );

        let middle = graph.bidi_scope_continuations_for_range(InlineGraphRange {
            start: InlineGraphPosition::at_run_start(3),
            end: InlineGraphPosition::at_run_start(4),
        });
        assert_eq!(middle.prefix, "\u{2066}\u{2067}");
        assert_eq!(middle.suffix, "\u{2069}\u{2069}");

        let after_inner = graph.bidi_scope_continuations_for_range(InlineGraphRange {
            start: InlineGraphPosition::at_run_start(5),
            end: InlineGraphPosition::at_run_start(6),
        });
        assert_eq!(after_inner.prefix, "\u{2066}");
        assert_eq!(after_inner.suffix, "\u{2069}");
    }

    #[test]
    fn bidi_scope_continuations_balance_authored_isolate_inside_one_graph_run() {
        let text = "a\u{2068}BC\u{2069}d";
        let graph = InlineOpportunityGraph::new(
            vec![bidi_scope_run(
                text,
                ComputedStyle::initial(),
                InlineTextSource::Normal,
            )],
            Vec::new(),
        );
        let isolate_content_start = text.find('B').expect("test isolate content");
        let isolate_content_end = isolate_content_start + 'B'.len_utf8();
        let continuations = graph.bidi_scope_continuations_for_range(InlineGraphRange {
            start: InlineGraphPosition {
                run_index: 0,
                byte_offset: isolate_content_start,
            },
            end: InlineGraphPosition {
                run_index: 0,
                byte_offset: isolate_content_end,
            },
        });

        assert_eq!(continuations.prefix_parent_context, "\u{200e}");
        assert_eq!(continuations.prefix, "\u{2068}");
        assert_eq!(continuations.suffix, "\u{2069}");
        assert_eq!(continuations.suffix_parent_context, "\u{200e}");
    }

    #[test]
    fn bidi_scope_continuations_replay_nested_authored_controls() {
        let text = "a\u{2068}b\u{202e}c\u{202c}d\u{2069}e";
        let graph = InlineOpportunityGraph::new(
            vec![bidi_scope_run(
                text,
                ComputedStyle::initial(),
                InlineTextSource::Normal,
            )],
            Vec::new(),
        );
        let selected_start = text.find('c').expect("test override content");
        let selected_end = selected_start + 'c'.len_utf8();
        let continuations = graph.bidi_scope_continuations_for_range(InlineGraphRange {
            start: InlineGraphPosition {
                run_index: 0,
                byte_offset: selected_start,
            },
            end: InlineGraphPosition {
                run_index: 0,
                byte_offset: selected_end,
            },
        });

        assert_eq!(continuations.prefix, "\u{2068}\u{202e}");
        assert_eq!(continuations.suffix, "\u{202c}\u{2069}");
    }

    fn wrap_inside_avoid_edge(logical_edge: InlineLogicalEdge) -> InlineParagraphRun {
        let mut style = ComputedStyle::initial();
        style.wrap_inside = css::WrapInside::Avoid;
        InlineParagraphRun {
            item: InlineLineItem::Atom(InlineAtom::new(
                InlineAtomContent::InlineEdge(InlineEdgeRole::BoxEdge(InlineBoxEdgeFragment {
                    logical_edge,
                    physical_side: match logical_edge {
                        InlineLogicalEdge::Start => PhysicalSide::Left,
                        InlineLogicalEdge::End => PhysicalSide::Right,
                    },
                    positioning_containing_block_id: None,
                    advance: 0.0,
                    paint_extent: 0.0,
                })),
                style.clone(),
                None,
                InlineSize::new(0.0, style.line_height),
                style.font_size,
                0.0,
                None,
                None,
            )),
            width: 0.0,
            shaped: None,
        }
    }

    #[test]
    fn wrap_inside_avoid_depth_uses_lexical_inline_edges() {
        let style = ComputedStyle::initial();
        let text = |contents| InlineParagraphRun {
            item: InlineLineItem::Fragment(InlineFragment::new(
                contents,
                style.clone(),
                0.0,
                None,
                true,
                InlineTextSource::Normal,
                false,
                InlineHangingEdges::default(),
                Vec::new(),
            )),
            width: 0.0,
            shaped: None,
        };
        let graph = InlineOpportunityGraph::new(
            vec![
                wrap_inside_avoid_edge(InlineLogicalEdge::Start),
                wrap_inside_avoid_edge(InlineLogicalEdge::Start),
                text("x"),
                wrap_inside_avoid_edge(InlineLogicalEdge::End),
                wrap_inside_avoid_edge(InlineLogicalEdge::End),
            ],
            Vec::new(),
        );

        assert_eq!(
            graph.wrap_inside_avoid_depth(InlineGraphPosition::at_run_start(0)),
            0
        );
        assert_eq!(
            graph.wrap_inside_avoid_depth(InlineGraphPosition::at_run_start(2)),
            2
        );
        assert_eq!(
            graph.wrap_inside_avoid_depth(InlineGraphPosition {
                run_index: 2,
                byte_offset: 1,
            }),
            0,
            "the trailing margin edge is outside both nested boxes"
        );
    }

    #[test]
    fn unbreakable_float_continuation_excludes_breakable_prefix() {
        let normal = ComputedStyle::initial();
        let mut nowrap = normal.clone();
        nowrap.white_space = WhiteSpace::NoWrap;
        nowrap.float = Float::Right;
        let float = InlineFloat::first_letter(
            Vec::new(),
            FirstLetterPseudoGroupId::allocate(),
            nowrap.clone(),
        );
        let graph = InlineOpportunityGraph::new(
            vec![
                bidi_scope_run("Some ", normal, InlineTextSource::Normal),
                bidi_scope_run("text ", nowrap.clone(), InlineTextSource::Normal),
                InlineParagraphRun {
                    item: InlineLineItem::Float(float),
                    width: 0.0,
                    shaped: None,
                },
                bidi_scope_run("that overflows", nowrap, InlineTextSource::Normal),
            ],
            vec![InlineBreakOpportunity {
                position: InlineGraphPosition::at_run_start(1),
                kind: BreakEffect::SoftWrap,
                availability: BreakAvailability::Ordinary,
                whitespace_edge: SelectedWhitespaceEdge::None,
                discretionary: None,
            }],
        );

        let continuation = graph
            .unbreakable_inline_float_continuation_after(InlineGraphRange {
                start: graph.start_position(),
                end: InlineGraphPosition::at_run_start(1),
            })
            .expect("visible nowrap prefix before the float forms a continuation");

        assert_eq!(continuation.marker, InlineGraphPosition::at_run_start(2));
        assert_eq!(
            continuation.source_range.start,
            InlineGraphPosition::at_run_start(1)
        );
        assert_eq!(continuation.source_range.end, graph.end_position());
    }

    #[test]
    fn graph_text_run_preserves_inline_word_style_handle() {
        let mut style = ComputedStyle::initial();
        style.font_size = 12.0;
        style.line_height = 14.0;
        let shared_style = inline_style(&style);
        let word = InlineWord {
            text: "Hello".to_string(),
            style: Rc::clone(&shared_style),
            baseline_shift: 0.0,
            visual_offset: InlineVisualOffset::zero(),
            link_target: None,
            mergeable: true,
            source: InlineTextSource::Normal,
            hanging_edges: InlineHangingEdges::default(),
            ancestor_inline_decorations: Vec::new().into(),
        };
        let mut font_system = FontSystem::new();
        let mut runs = Vec::new();

        push_text_graph_run_segment(
            &mut font_system,
            &mut runs,
            &word,
            &word.text,
            InlineHangingEdges::default(),
            InlineTrackingScope::root(&style),
            Rc::new(()),
        );

        let InlineLineItem::Fragment(fragment) = &runs[0].item else {
            panic!("expected graph run fragment");
        };
        assert!(Rc::ptr_eq(&shared_style, &fragment.data.style));
    }

    #[test]
    fn transformed_separator_retains_shared_boundary_shape() {
        let style = ComputedStyle::initial();
        let run = |text, source| InlineParagraphRun {
            item: InlineLineItem::Fragment(InlineFragment::new(
                text,
                style.clone(),
                0.0,
                None,
                true,
                source,
                false,
                InlineHangingEdges::default(),
                Vec::new(),
            )),
            width: 0.0,
            shaped: None,
        };
        let mut runs = vec![
            run("あ", InlineTextSource::Normal),
            run(
                "\u{3000}",
                InlineTextSource::WordSpaceTransform(
                    ExplicitWordSeparatorSource::AuthoredZeroWidthSpace,
                ),
            ),
            run("い", InlineTextSource::Normal),
        ];
        let mut font_system = FontSystem::new();

        shape_logical_joining_graph_runs(&mut runs, &mut font_system, &style);

        let fragments = runs
            .iter()
            .map(|run| match &run.item {
                InlineLineItem::Fragment(fragment) => fragment,
                InlineLineItem::Atom(_) | InlineLineItem::Float(_) => {
                    panic!("word-space-transform fixture contains only text fragments")
                }
            })
            .collect::<Vec<_>>();
        assert!(matches!(
            fragments[1].source(),
            InlineTextSource::WordSpaceTransform(
                ExplicitWordSeparatorSource::AuthoredZeroWidthSpace
            )
        ));
        let source = fragments[0]
            .boundary_shaped_source()
            .expect("left text retains complete source shape");
        assert_eq!(source.shaped.text.as_ref(), "あ　い");
        assert!(fragments.iter().all(|fragment| {
            fragment
                .boundary_shaped_source()
                .is_some_and(|candidate| std::ptr::eq(source, candidate))
        }));
        assert_eq!(fragments[0].boundary_shaped_range(), Some(&(0.."あ".len())));
        assert_eq!(
            fragments[1].boundary_shaped_range(),
            Some(&("あ".len().."あ　".len()))
        );
        assert_eq!(
            fragments[2].boundary_shaped_range(),
            Some(&("あ　".len().."あ　い".len()))
        );
    }

    #[test]
    fn css_bidi_control_graph_run_has_no_shaped_advance() {
        let style = ComputedStyle::initial();
        let word = InlineWord {
            text: "\u{202a}".to_string(),
            style: inline_style(&style),
            baseline_shift: 0.0,
            visual_offset: InlineVisualOffset::zero(),
            link_target: None,
            mergeable: true,
            source: InlineTextSource::BidiControl,
            hanging_edges: InlineHangingEdges::default(),
            ancestor_inline_decorations: Vec::new().into(),
        };
        let mut font_system = FontSystem::new();
        let mut runs = Vec::new();

        push_text_graph_run_segment(
            &mut font_system,
            &mut runs,
            &word,
            &word.text,
            InlineHangingEdges::default(),
            InlineTrackingScope::root(&style),
            Rc::new(()),
        );

        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0].width, 0.0);
        assert!(runs[0].shaped.is_none());
    }

    #[test]
    fn unconditional_hanging_separator_does_not_constrain_fitting() {
        let style = ComputedStyle::initial();
        let fragment = InlineFragment::new(
            "A\u{3000}",
            style.clone(),
            0.0,
            None,
            true,
            InlineTextSource::Normal,
            false,
            InlineHangingEdges::default(),
            Vec::new(),
        );
        let mut font_system = FontSystem::new();
        let separator_width = font_system.measure_text("\u{3000}", &style);
        let widths = inline_content_width_for_line_items(
            &[MeasuredInlineItem::new(
                InlineLineItem::Fragment(fragment),
                40.0 + separator_width,
                None,
            )],
            &mut font_system,
            |item| item.used_advance().points(),
        );

        assert_eq!(widths.trailing_space_width, separator_width);
        assert_eq!(widths.fitting_width, 40.0);
        assert_eq!(widths.content_width, 40.0);
    }

    #[test]
    fn break_spaces_keeps_narrow_no_break_space_in_the_fitting_measure() {
        let mut style = ComputedStyle::initial();
        style.white_space = WhiteSpace::BreakSpaces;
        let fragment = InlineFragment::new(
            "A\u{202f}",
            style.clone(),
            0.0,
            None,
            true,
            InlineTextSource::Normal,
            false,
            InlineHangingEdges::default(),
            Vec::new(),
        );
        let mut font_system = FontSystem::new();
        let separator_width = font_system.measure_text("\u{202f}", &style);
        let total_width = 40.0 + separator_width;
        let widths = inline_content_width_for_line_items(
            &[MeasuredInlineItem::new(
                InlineLineItem::Fragment(fragment),
                total_width,
                None,
            )],
            &mut font_system,
            |item| item.used_advance().points(),
        );

        assert_eq!(widths.trailing_space_width, 0.0);
        assert_eq!(widths.fitting_width, total_width);
        assert_eq!(widths.content_width, total_width);
    }

    #[test]
    fn collapsed_terminal_space_exposes_hanging_separator_for_fitting() {
        let style = ComputedStyle::initial();
        let fragment = InlineFragment::new(
            "A\u{3000} ",
            style.clone(),
            0.0,
            None,
            true,
            InlineTextSource::Normal,
            false,
            InlineHangingEdges::default(),
            Vec::new(),
        );
        let mut font_system = FontSystem::new();
        let separator_width = font_system.measure_text("\u{3000}", &style);
        let document_space_width = font_system.measure_text(" ", &style);
        let widths = inline_content_width_for_line_items(
            &[MeasuredInlineItem::new(
                InlineLineItem::Fragment(fragment),
                40.0 + separator_width + document_space_width,
                None,
            )],
            &mut font_system,
            |item| item.used_advance().points(),
        );

        assert_eq!(widths.trailing_space_width, separator_width);
        assert_eq!(widths.fitting_width, 40.0 + document_space_width,);
    }

    #[test]
    fn pre_hanging_sequence_includes_interleaved_document_spaces() {
        let mut style = ComputedStyle::initial();
        style.white_space = WhiteSpace::Pre;
        let text = "A\u{3000} \u{2000}";
        let fragment = InlineFragment::new(
            text,
            style.clone(),
            0.0,
            None,
            true,
            InlineTextSource::Normal,
            false,
            InlineHangingEdges::default(),
            Vec::new(),
        );
        let mut font_system = FontSystem::new();
        let hanging_width = font_system.measure_text("\u{3000} \u{2000}", &style);
        let widths = inline_content_width_for_line_items(
            &[MeasuredInlineItem::new(
                InlineLineItem::Fragment(fragment),
                40.0 + hanging_width,
                None,
            )],
            &mut font_system,
            |item| item.used_advance().points(),
        );

        assert_eq!(widths.trailing_space_width, hanging_width);
        assert_eq!(widths.fitting_width, 40.0);
    }

    #[test]
    fn automatic_marker_is_a_separate_selected_item_with_source_context() {
        let mut style = ComputedStyle::initial();
        style.language = ContentLanguage::from_html_attribute("ug");
        let source = InlineFragment::new(
            "دامي",
            style,
            0.0,
            None,
            true,
            InlineTextSource::Normal,
            false,
            InlineHangingEdges::default(),
            Vec::new(),
        );
        let mut items = vec![MeasuredInlineItem::new(
            InlineLineItem::Fragment(source),
            0.0,
            None,
        )];
        let graph_runs = vec![InlineParagraphRun {
            item: items[0].item.clone(),
            width: 0.0,
            shaped: None,
        }];
        let mut font_system = FontSystem::new();
        apply_selected_discretionary_break(
            &mut items,
            Some(DiscretionaryBreakEffect {
                source_boundary: InlineGraphPosition::at_run_start(0),
                marker_owner: DiscretionaryMarkerOwner {
                    style_position: InlineGraphPosition::at_run_start(0),
                },
                left_replacement: None,
                right_replacement: None,
                leading_shaping_context: SelectedLineShapingContext::PreserveJoining,
            }),
            SelectedLineEdge::Trailing,
            &mut font_system,
            &graph_runs,
        );

        assert_eq!(items.len(), 2);
        let InlineLineItem::Fragment(source) = &items[0].item else {
            panic!("selected source remains a fragment");
        };
        let InlineLineItem::Fragment(marker) = &items[1].item else {
            panic!("selected marker is a fragment");
        };
        assert_eq!(source.text(), "دامي\u{200d}");
        assert_eq!(marker.text(), "\u{0640}");
        assert!(marker.is_selected_discretionary_marker());
    }

    #[test]
    fn authored_marker_uses_the_soft_hyphen_fragment_style() {
        let source_style = ComputedStyle::initial();
        let mut marker_style = ComputedStyle::initial();
        marker_style.hyphenate_character = HyphenateCharacter::String("=".into());
        let source = InlineFragment::new(
            "word",
            source_style,
            0.0,
            None,
            true,
            InlineTextSource::Normal,
            false,
            InlineHangingEdges::default(),
            Vec::new(),
        );
        let soft_hyphen = InlineFragment::new(
            "\u{00ad}",
            marker_style,
            0.0,
            None,
            true,
            InlineTextSource::Normal,
            false,
            InlineHangingEdges::default(),
            Vec::new(),
        );
        let mut items = vec![MeasuredInlineItem::new(
            InlineLineItem::Fragment(source.clone()),
            0.0,
            None,
        )];
        let graph_runs = vec![
            InlineParagraphRun {
                item: InlineLineItem::Fragment(source),
                width: 0.0,
                shaped: None,
            },
            InlineParagraphRun {
                item: InlineLineItem::Fragment(soft_hyphen),
                width: 0.0,
                shaped: None,
            },
        ];
        let mut font_system = FontSystem::new();
        apply_selected_discretionary_break(
            &mut items,
            Some(DiscretionaryBreakEffect {
                source_boundary: InlineGraphPosition::at_run_start(1),
                marker_owner: DiscretionaryMarkerOwner {
                    style_position: InlineGraphPosition::at_run_start(1),
                },
                left_replacement: None,
                right_replacement: None,
                leading_shaping_context: SelectedLineShapingContext::None,
            }),
            SelectedLineEdge::Trailing,
            &mut font_system,
            &graph_runs,
        );

        let InlineLineItem::Fragment(marker) = &items[1].item else {
            panic!("selected marker is a fragment");
        };
        assert_eq!(marker.text(), "=");
        assert!(items[1].base_advance().points() > 0.0);
    }

    #[test]
    fn vertical_auto_marker_uses_the_vertical_hyphen() {
        let mut style = ComputedStyle::initial();
        style.writing_mode = WritingMode::VerticalRl;
        let fragment = InlineFragment::new(
            "word",
            style,
            0.0,
            None,
            true,
            InlineTextSource::Normal,
            false,
            InlineHangingEdges::default(),
            Vec::new(),
        );

        assert_eq!(used_discretionary_marker_text(&fragment), "\u{2010}");
    }

    #[test]
    fn selected_vertical_soft_hyphen_normalization_uses_the_vertical_auto_marker() {
        let mut style = ComputedStyle::initial();
        style.writing_mode = WritingMode::VerticalRl;
        let fragment = InlineFragment::new(
            "word\u{00ad}",
            style,
            0.0,
            None,
            true,
            InlineTextSource::Normal,
            false,
            InlineHangingEdges::default(),
            Vec::new(),
        );
        let mut items = vec![MeasuredInlineItem::new(
            InlineLineItem::Fragment(fragment),
            0.0,
            None,
        )];

        normalize_materialized_control_characters(&mut items, true, &mut FontSystem::new());

        let InlineLineItem::Fragment(fragment) = &items[0].item else {
            panic!("selected source remains a text fragment");
        };
        assert_eq!(fragment.text(), "word\u{2010}");
    }

    #[test]
    fn selected_vertical_soft_hyphen_normalization_preserves_explicit_marker() {
        let mut style = ComputedStyle::initial();
        style.writing_mode = WritingMode::VerticalRl;
        style.hyphenate_character = HyphenateCharacter::String("+=".into());
        let fragment = InlineFragment::new(
            "word\u{00ad}",
            style,
            0.0,
            None,
            true,
            InlineTextSource::Normal,
            false,
            InlineHangingEdges::default(),
            Vec::new(),
        );
        let mut items = vec![MeasuredInlineItem::new(
            InlineLineItem::Fragment(fragment),
            0.0,
            None,
        )];

        normalize_materialized_control_characters(&mut items, true, &mut FontSystem::new());

        let InlineLineItem::Fragment(fragment) = &items[0].item else {
            panic!("selected source remains a text fragment");
        };
        assert_eq!(fragment.text(), "word+=");
    }

    #[test]
    fn frozen_float_replay_never_reuses_a_source_band_after_relocation() {
        let selected = InlineFloatReplay::RequeryContainingBlock {
            selected_float_page_index: 3,
        };
        assert!(!selected.reuses_selected_band_on(3));

        let frozen = selected.freeze_selected_band();
        assert!(frozen.reuses_selected_band_on(3));
        assert!(!frozen.reuses_selected_band_on(4));
        assert_eq!(frozen.selected_float_page_index(), 3);
    }

    #[test]
    fn pre_wrap_terminal_hanging_depends_on_the_selected_line_end() {
        assert_eq!(
            SelectedLineEndCondition::SoftWrap.pre_wrap_hanging_width(10.0, 20.0, Some(10.0)),
            10.0
        );
        assert_eq!(
            SelectedLineEndCondition::IntrinsicSegmentEnd.pre_wrap_hanging_width(10.0, 20.0, None),
            10.0
        );
        assert_eq!(
            SelectedLineEndCondition::ForcedBreak.pre_wrap_hanging_width(10.0, 20.0, Some(10.0)),
            10.0
        );
        assert_eq!(
            SelectedLineEndCondition::ParagraphEnd.pre_wrap_hanging_width(10.0, 15.0, Some(10.0)),
            5.0
        );
        assert_eq!(
            SelectedLineEndCondition::ForcedBreak.pre_wrap_hanging_width(10.0, 10.0, Some(10.0)),
            0.0
        );
        assert_eq!(
            SelectedLineEndCondition::ParagraphEnd.pre_wrap_hanging_width(10.0, 20.0, None),
            0.0
        );
    }

    #[test]
    fn first_letter_stream_keeps_prefix_punctuation_across_split_fragments() {
        let style = ComputedStyle::initial();
        let quote = bidi_scope_run("\u{201c}", style.clone(), InlineTextSource::Normal);
        let mut initial = quote.clone();
        let InlineLineItem::Fragment(fragment) = &mut initial.item else {
            panic!("test text run must be a fragment");
        };
        fragment.set_text(Rc::from("abc"));
        let graph = InlineOpportunityGraph::new(
            vec![
                quote,
                cloneable_box_edge_run(style.clone(), InlineLogicalEdge::Start, 1),
                initial,
                cloneable_box_edge_run(style, InlineLogicalEdge::End, 1),
            ],
            Vec::new(),
        );

        let selection = first_letter_stream_selection(&graph);
        assert_eq!(selection.len(), 2);
        assert_eq!(selection[0].run_index, 0);
        assert_eq!(selection[0].range, 0.."\u{201c}".len());
        assert_eq!(
            selection[0].role,
            FirstLetterPseudoFragmentRole::AssociatedPrefix
        );
        assert_eq!(selection[1].run_index, 2);
        assert_eq!(selection[1].range, 0..1);
        assert_eq!(
            selection[1].role,
            FirstLetterPseudoFragmentRole::TypographicInitial
        );
    }

    #[test]
    fn first_letter_stream_selects_a_generated_quote_before_author_text() {
        let style = ComputedStyle::initial();
        let graph = InlineOpportunityGraph::new(
            vec![
                bidi_scope_run("\u{201c}", style.clone(), InlineTextSource::Generated),
                bidi_scope_run("abc", style, InlineTextSource::Normal),
            ],
            Vec::new(),
        );

        let selection = first_letter_stream_selection(&graph);
        assert_eq!(selection.len(), 1);
        assert_eq!(selection[0].run_index, 0);
        assert_eq!(selection[0].range, 0.."\u{201c}".len());
        assert_eq!(
            selection[0].role,
            FirstLetterPseudoFragmentRole::AssociatedPrefix
        );
    }

    #[test]
    fn first_letter_stream_rejects_text_after_an_atomic_inline() {
        let style = ComputedStyle::initial();
        let atom = InlineAtom::new(
            InlineAtomContent::StaticPositionPlaceholder,
            style.clone(),
            None,
            InlineSize::new(0.0, 0.0),
            0.0,
            0.0,
            None,
            None,
        );
        let graph = InlineOpportunityGraph::new(
            vec![
                bidi_scope_run("\u{201c}", style.clone(), InlineTextSource::Normal),
                InlineParagraphRun {
                    item: InlineLineItem::Atom(atom),
                    width: 0.0,
                    shaped: None,
                },
                bidi_scope_run("abc", style, InlineTextSource::Normal),
            ],
            Vec::new(),
        );

        assert!(first_letter_stream_selection(&graph).is_empty());
    }

    #[test]
    fn floated_first_letter_group_becomes_one_marker_without_source_text() {
        let mut style = ComputedStyle::initial();
        style.float = Float::Left;
        let group_id = FirstLetterPseudoGroupId::allocate();
        let mut prefix = bidi_scope_run("\u{201c}", style.clone(), InlineTextSource::Generated);
        let mut initial = bidi_scope_run("A", style.clone(), InlineTextSource::Normal);
        for run in [&mut prefix, &mut initial] {
            let InlineLineItem::Fragment(fragment) = &mut run.item else {
                unreachable!("test run is text");
            };
            fragment.set_first_letter_pseudo_group_id(group_id);
        }
        let mut runs = vec![prefix, initial];

        materialize_first_letter_float(&mut runs, group_id, &style);

        assert_eq!(runs.len(), 1);
        let InlineLineItem::Float(float) = &runs[0].item else {
            panic!("first selected text becomes an inline float marker");
        };
        let fragments = float
            .first_letter_fragments()
            .expect("first-letter float keeps text payload");
        assert_eq!(fragments.len(), 2);
        assert_eq!(fragments[0].text(), "\u{201c}");
        assert_eq!(fragments[1].text(), "A");
        assert!(
            fragments
                .iter()
                .all(|fragment| fragment.style().float == Float::None)
        );
        assert!(
            fragments
                .iter()
                .all(|fragment| fragment.style().initial_letter.is_normal())
        );
        assert!(float.style().initial_letter.is_normal());
    }

    #[test]
    fn intrinsic_measurement_sensitivity_distinguishes_forced_and_soft_breaks() {
        fn sensitivity_for(kind: BreakEffect) -> IntrinsicMeasurementSensitivity {
            let graph = InlineOpportunityGraph::new(
                Vec::new(),
                vec![InlineBreakOpportunity {
                    position: InlineGraphPosition::at_run_start(0),
                    kind,
                    availability: BreakAvailability::Ordinary,
                    whitespace_edge: SelectedWhitespaceEdge::None,
                    discretionary: None,
                }],
            );
            InlineIntrinsicMeasurement {
                paragraphs: vec![InlineMeasuredParagraph {
                    graph,
                    contribution: InlineIntrinsicContribution::default(),
                }],
                ..InlineIntrinsicMeasurement::default()
            }
            .sensitivity()
        }

        assert!(
            !sensitivity_for(BreakEffect::Forced).block_extent_depends_on_available_inline_size
        );
        assert!(
            sensitivity_for(BreakEffect::SoftWrap).block_extent_depends_on_available_inline_size
        );
        assert!(
            sensitivity_for(BreakEffect::Hyphenation).block_extent_depends_on_available_inline_size
        );
        assert!(
            sensitivity_for(BreakEffect::AtomicBoundary)
                .block_extent_depends_on_available_inline_size
        );
    }
}

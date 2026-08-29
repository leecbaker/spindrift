use super::*;

#[test]
fn list_style_none_suppresses_only_the_automatic_marker() {
    let counter_styles = HashMap::new();
    assert_eq!(
        automatic_marker_text(ListStyleType::None, 2, &counter_styles),
        None
    );

    let mut marker_style = ComputedStyle::initial();
    marker_style.marker_content = MarkerContent::Parts(vec![
        MarkerContentPart::Counter {
            name: LIST_ITEM_COUNTER_NAME.to_string(),
            style: Some(ListStyleType::Decimal),
        },
        MarkerContentPart::Text(". ".to_string()),
    ]);
    let stacks = HashMap::from([(LIST_ITEM_COUNTER_NAME.to_string(), vec![2])]);
    let mut quote_depth = 0;
    assert_eq!(
        marker_text(
            &marker_style,
            2,
            &counter_styles,
            &stacks,
            &mut quote_depth,
            CounterStyleRenderContext::for_style(&marker_style),
        ),
        Some(("2. ".to_string(), false))
    );
}

#[test]
fn outside_anchor_preserves_line_start_and_baseline_as_distinct_positions() {
    let line_start = PageTopBlockPosition::new(100.0);
    let anchor = OutsideMarkerAnchor {
        principal_line_inline_span: PageInlineSpan::from_edges(20.0, 80.0),
        formatted_line_block_start: line_start,
        alphabetic_baseline: line_start.toward_block_end(layout_pt(12.0)),
    };

    assert_eq!(anchor.principal_line_inline_span.left_x(), 20.0);
    assert_eq!(anchor.principal_line_inline_span.right_x(), 80.0);
    assert_eq!(anchor.formatted_line_block_start.points(), 100.0);
    assert_eq!(anchor.alphabetic_baseline.points(), 88.0);
}

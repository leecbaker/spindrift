use super::*;
use crate::css::Css;

fn parent_style() -> ComputedStyle {
    ComputedStyle {
        font_size: 12.0,
        line_height: 14.4,
        color: Color::BLACK,
        ..ComputedStyle::initial()
    }
}

fn build_test_page<'a>(root: &'a Node, author_stylesheets: &[Stylesheet]) -> PageBox<'a> {
    let mut stylesheets = vec![css::html5_user_agent_stylesheet()];
    stylesheets.extend_from_slice(author_stylesheets);
    build_page_box(root, &stylesheets, &parent_style())
}

#[tokio::test]
async fn builds_styled_formatting_box_tree() {
    let root = dom::parse(
        "<html><body><div><p>Hello <em>world</em></p><table><tr><td>A</td></tr></table><img src=\"x\"></div></body></html>",
    );
    let stylesheet = css::parse_stylesheet(&Css::from_string(
        "p { font-size: 10pt } em { font-style: italic }",
    ));
    let page = build_test_page(&root, &[stylesheet]);

    let html = &page.children[0];
    assert_eq!(html.kind(), FormattingBoxKind::Block);
    let body = &html.children()[0];
    assert_eq!(body.kind(), FormattingBoxKind::Block);
    let div = &body.children()[0];
    assert_eq!(div.kind(), FormattingBoxKind::Block);

    let kinds = div
        .children()
        .iter()
        .map(FormattingBox::kind)
        .collect::<Vec<_>>();
    assert_eq!(
        kinds,
        vec![
            FormattingBoxKind::Block,
            FormattingBoxKind::Table,
            FormattingBoxKind::AnonymousBlock
        ]
    );
    assert_eq!(
        div.children()[2]
            .children()
            .iter()
            .map(FormattingBox::kind)
            .collect::<Vec<_>>(),
        vec![FormattingBoxKind::AtomicInline]
    );

    let paragraph = &div.children()[0];
    assert_eq!(paragraph.style().font_size, 10.0);
    assert_eq!(
        paragraph
            .children()
            .iter()
            .map(FormattingBox::kind)
            .collect::<Vec<_>>(),
        vec![FormattingBoxKind::Text, FormattingBoxKind::Inline]
    );
}

#[tokio::test]
async fn pure_block_children_remain_block_children() {
    let root = dom::parse("<html><body><div><p>A</p><section>B</section></div></body></html>");
    let page = build_test_page(&root, &[]);
    let div = &page.children[0].children()[0].children()[0];

    assert_eq!(
        div.children()
            .iter()
            .map(FormattingBox::kind)
            .collect::<Vec<_>>(),
        vec![FormattingBoxKind::Block, FormattingBoxKind::Block]
    );
}

#[tokio::test]
async fn formatting_whitespace_between_block_children_is_ignored() {
    let root =
        dom::parse("<html><body><div>\n<p>A</p>\n<section>B</section>\n</div></body></html>");
    let page = build_test_page(&root, &[]);
    let div = &page.children[0].children()[0].children()[0];

    assert_eq!(
        div.children()
            .iter()
            .map(FormattingBox::kind)
            .collect::<Vec<_>>(),
        vec![FormattingBoxKind::Block, FormattingBoxKind::Block]
    );
}

#[tokio::test]
async fn pure_inline_content_keeps_inline_and_text_boxes() {
    let root = dom::parse("<html><body><p>Hello <em>world</em></p></body></html>");
    let page = build_test_page(&root, &[]);
    let paragraph = &page.children[0].children()[0].children()[0];

    assert_eq!(
        paragraph
            .children()
            .iter()
            .map(FormattingBox::kind)
            .collect::<Vec<_>>(),
        vec![FormattingBoxKind::Text, FormattingBoxKind::Inline]
    );
}

#[tokio::test]
async fn box_tree_preserves_font_shorthand_unit_line_height() {
    let root = dom::parse(
        "<html><body><div class=\"ref\">XX<br>XX</div><div class=\"test\">&#x3000;&#x3000;XX</div></body></html>",
    );
    let stylesheet = css::parse_stylesheet(&Css::from_string(
        "div { font: 50px/1 Ahem; } .test { width: 2em; color: green; background: green; }",
    ));
    let page = build_test_page(&root, &[stylesheet]);
    let body = &page.children[0].children()[0];
    let test_div = &body.children()[1];

    assert_eq!(
        test_div.style().line_height_value,
        css::ComputedLineHeight::Number(1.0)
    );
    assert!((test_div.style().font_size - 37.5).abs() < 0.001);
    assert!((test_div.style().line_height - 37.5).abs() < 0.001);
    assert!(!test_div.style().line_height_is_normal);
}

#[tokio::test]
async fn mixed_inline_and_block_children_create_anonymous_blocks_in_order() {
    let root = dom::parse("<html><body><div>Before<p>Block</p>After</div></body></html>");
    let page = build_test_page(&root, &[]);
    let div = &page.children[0].children()[0].children()[0];

    assert_eq!(
        div.children()
            .iter()
            .map(FormattingBox::kind)
            .collect::<Vec<_>>(),
        vec![
            FormattingBoxKind::AnonymousBlock,
            FormattingBoxKind::Block,
            FormattingBoxKind::AnonymousBlock,
        ]
    );
    assert_eq!(
        div.children()[0]
            .children()
            .iter()
            .map(FormattingBox::kind)
            .collect::<Vec<_>>(),
        vec![FormattingBoxKind::Text]
    );
}

#[tokio::test]
async fn block_inside_inline_is_split_into_parent_block_flow() {
    let root = dom::parse("<html><body><div><span><p>Block</p></span></div></body></html>");
    let page = build_test_page(&root, &[]);
    let div = &page.children[0].children()[0].children()[0];

    assert_eq!(
        div.children()
            .iter()
            .map(FormattingBox::kind)
            .collect::<Vec<_>>(),
        vec![FormattingBoxKind::Block]
    );
}

#[tokio::test]
async fn block_inside_inline_preserves_empty_fragment_with_owned_inline_start_edge() {
    let root = dom::parse("<html><body><span><div>Block</div>X</span></body></html>");
    let stylesheet = css::parse_stylesheet(&Css::from_string(
        "body > span { margin-left: -100px; border-left: 100px solid transparent }",
    ));
    let page = build_test_page(&root, &[stylesheet]);
    let body = &page.children[0].children()[0];

    assert_eq!(
        body.children()
            .iter()
            .map(FormattingBox::kind)
            .collect::<Vec<_>>(),
        vec![
            FormattingBoxKind::AnonymousBlock,
            FormattingBoxKind::Block,
            FormattingBoxKind::AnonymousBlock,
        ]
    );
    let FormattingBox::Inline(before_span) = &body.children()[0].children()[0] else {
        panic!("pre-block anonymous block should contain the edge-only span fragment");
    };
    assert!(before_span.children.is_empty());
    assert_eq!(
        before_span.fragment_edges,
        InlineBoxFragmentEdges {
            owns_start: true,
            owns_end: false,
        }
    );
}

#[tokio::test]
async fn block_inside_inline_after_fragment_owns_only_inline_end_edge() {
    let root = dom::parse("<html><body><span><div>One</div>Two</span></body></html>");
    let stylesheet =
        css::parse_stylesheet(&Css::from_string("body > span { border: 3px solid blue }"));
    let page = build_test_page(&root, &[stylesheet]);
    let body = &page.children[0].children()[0];

    assert_eq!(
        body.children()
            .iter()
            .map(FormattingBox::kind)
            .collect::<Vec<_>>(),
        vec![FormattingBoxKind::Block, FormattingBoxKind::AnonymousBlock]
    );
    assert_eq!(
        body.children()[1]
            .children()
            .iter()
            .map(FormattingBox::kind)
            .collect::<Vec<_>>(),
        vec![FormattingBoxKind::Inline]
    );
    let FormattingBox::Inline(span_fragment) = &body.children()[1].children()[0] else {
        panic!("post-block anonymous block should contain the span inline fragment");
    };
    assert_eq!(
        span_fragment.fragment_edges,
        InlineBoxFragmentEdges {
            owns_start: false,
            owns_end: true,
        }
    );
}

#[tokio::test]
async fn block_inside_inline_splits_surrounding_inline_runs() {
    let root = dom::parse("<html><body><div>A<span>B<p>Block</p>C</span>D</div></body></html>");
    let page = build_test_page(&root, &[]);
    let div = &page.children[0].children()[0].children()[0];

    assert_eq!(
        div.children()
            .iter()
            .map(FormattingBox::kind)
            .collect::<Vec<_>>(),
        vec![
            FormattingBoxKind::AnonymousBlock,
            FormattingBoxKind::Block,
            FormattingBoxKind::AnonymousBlock,
        ]
    );
    assert_eq!(
        div.children()[0]
            .children()
            .iter()
            .map(FormattingBox::kind)
            .collect::<Vec<_>>(),
        vec![FormattingBoxKind::Text, FormattingBoxKind::Inline]
    );
    assert_eq!(
        div.children()[2]
            .children()
            .iter()
            .map(FormattingBox::kind)
            .collect::<Vec<_>>(),
        vec![FormattingBoxKind::Inline, FormattingBoxKind::Text]
    );
    let FormattingBox::Inline(before_span) = &div.children()[0].children()[1] else {
        panic!("before anonymous block should contain a span inline fragment");
    };
    assert_eq!(
        before_span.fragment_edges,
        InlineBoxFragmentEdges {
            owns_start: true,
            owns_end: false,
        }
    );
    let FormattingBox::Inline(after_span) = &div.children()[2].children()[0] else {
        panic!("after anonymous block should contain a span inline fragment");
    };
    assert_eq!(
        after_span.fragment_edges,
        InlineBoxFragmentEdges {
            owns_start: false,
            owns_end: true,
        }
    );
}

#[tokio::test]
async fn nested_block_inside_inline_preserves_each_inline_fragment_edges() {
    let root =
        dom::parse("<html><body><div><span>A<em>B<p>Block</p>C</em>D</span></div></body></html>");
    let page = build_test_page(&root, &[]);
    let div = &page.children[0].children()[0].children()[0];

    assert_eq!(
        div.children()
            .iter()
            .map(FormattingBox::kind)
            .collect::<Vec<_>>(),
        vec![
            FormattingBoxKind::AnonymousBlock,
            FormattingBoxKind::Block,
            FormattingBoxKind::AnonymousBlock,
        ]
    );
    let FormattingBox::Inline(before_span) = &div.children()[0].children()[0] else {
        panic!("before block should start with the span fragment");
    };
    assert_eq!(
        before_span.fragment_edges,
        InlineBoxFragmentEdges {
            owns_start: true,
            owns_end: false,
        }
    );
    let FormattingBox::Inline(before_em) = &before_span.children[1] else {
        panic!("before span fragment should contain the em fragment");
    };
    assert_eq!(
        before_em.fragment_edges,
        InlineBoxFragmentEdges {
            owns_start: true,
            owns_end: false,
        }
    );
    let FormattingBox::Inline(after_span) = &div.children()[2].children()[0] else {
        panic!("after block should contain the span fragment");
    };
    assert_eq!(
        after_span.fragment_edges,
        InlineBoxFragmentEdges {
            owns_start: false,
            owns_end: true,
        }
    );
    let FormattingBox::Inline(after_em) = &after_span.children[0] else {
        panic!("after span fragment should contain the em fragment");
    };
    assert_eq!(
        after_em.fragment_edges,
        InlineBoxFragmentEdges {
            owns_start: false,
            owns_end: true,
        }
    );
}

#[tokio::test]
async fn inline_block_creates_atomic_inline_box_with_children() {
    let root = dom::parse(
        "<html><body><p>A<span style=\"display:inline-block\"><strong>B</strong></span>C</p></body></html>",
    );
    let page = build_test_page(&root, &[]);
    let paragraph = &page.children[0].children()[0].children()[0];

    assert_eq!(
        paragraph
            .children()
            .iter()
            .map(FormattingBox::kind)
            .collect::<Vec<_>>(),
        vec![
            FormattingBoxKind::Text,
            FormattingBoxKind::AtomicInline,
            FormattingBoxKind::Text,
        ]
    );
    assert_eq!(
        paragraph.children()[1]
            .children()
            .iter()
            .map(FormattingBox::kind)
            .collect::<Vec<_>>(),
        vec![FormattingBoxKind::Inline]
    );
}

#[tokio::test]
async fn orphan_table_cells_create_anonymous_table_wrapper() {
    let root = dom::parse(
        "<html><body><div><span style=\"display:table-cell\">A</span><span style=\"display:table-cell\">B</span></div></body></html>",
    );
    let page = build_test_page(&root, &[]);
    let div = &page.children[0].children()[0].children()[0];

    assert_eq!(
        div.children()
            .iter()
            .map(FormattingBox::kind)
            .collect::<Vec<_>>(),
        vec![FormattingBoxKind::Table]
    );
    assert_eq!(
        div.children()[0]
            .children()
            .iter()
            .map(FormattingBox::kind)
            .collect::<Vec<_>>(),
        vec![FormattingBoxKind::Block, FormattingBoxKind::Block]
    );
    let FormattingBox::Table(table) = &div.children()[0] else {
        panic!("expected anonymous table wrapper");
    };
    assert_eq!(table.fragment.rows.len(), 1);
    assert_eq!(table.fragment.rows[0].cells.len(), 2);
}

#[tokio::test]
async fn orphan_table_cell_with_formatting_whitespace_keeps_fragment_rows() {
    let root = dom::parse(
        "<html><body><div>\n  <div style=\"display:table-cell\"><div>Cell</div></div>\n</div></body></html>",
    );
    let page = build_test_page(&root, &[]);
    let div = &page.children[0].children()[0].children()[0];

    assert_eq!(
        div.children()
            .iter()
            .map(FormattingBox::kind)
            .collect::<Vec<_>>(),
        vec![FormattingBoxKind::Table]
    );
    let FormattingBox::Table(table) = &div.children()[0] else {
        panic!("expected anonymous table wrapper");
    };
    assert_eq!(table.fragment.rows.len(), 1);
    assert_eq!(table.fragment.rows[0].cells.len(), 1);
    assert_eq!(table.fragment.rows[0].cells[0].children.len(), 1);
}

#[tokio::test]
async fn orphan_table_cells_inside_inline_create_anonymous_inline_table_wrapper() {
    let root = dom::parse(
        "<html><body><p>Before <span><span style=\"display:table-cell\">Cell</span></span> After</p></body></html>",
    );
    let page = build_test_page(&root, &[]);
    let paragraph = &page.children[0].children()[0].children()[0];

    assert_eq!(
        paragraph
            .children()
            .iter()
            .map(FormattingBox::kind)
            .collect::<Vec<_>>(),
        vec![
            FormattingBoxKind::Text,
            FormattingBoxKind::Inline,
            FormattingBoxKind::Text
        ]
    );
    let FormattingBox::Inline(span) = &paragraph.children()[1] else {
        panic!("expected inline parent to remain inline");
    };
    assert_eq!(
        span.children
            .iter()
            .map(FormattingBox::kind)
            .collect::<Vec<_>>(),
        vec![FormattingBoxKind::AtomicInline]
    );
    let FormattingBox::AtomicInline(table) = &span.children[0] else {
        panic!("expected anonymous inline-table wrapper");
    };
    assert_eq!(table.style.display, Display::INLINE_TABLE);
    let fragment = table
        .table_fragment
        .as_ref()
        .expect("anonymous inline-table should retain a durable table fragment");
    assert_eq!(fragment.rows.len(), 1);
    assert_eq!(fragment.rows[0].cells.len(), 1);
}

#[tokio::test]
async fn table_box_contains_durable_fragment_rows_columns_and_captions() {
    let root = dom::parse(
        "<html><body><table><caption>Cap</caption><colgroup span=\"2\"></colgroup><tbody><tr><td>A</td><td>B</td></tr></tbody></table></body></html>",
    );
    let page = build_test_page(&root, &[]);
    let table = &page.children[0].children()[0].children()[0];

    let FormattingBox::Table(table) = table else {
        panic!("expected table formatting box");
    };
    assert_eq!(table.fragment.captions.len(), 1);
    assert_eq!(table.fragment.columns.len(), 1);
    assert_eq!(table.fragment.columns[0].span, 2);
    assert_eq!(table.fragment.rows.len(), 1);
    assert_eq!(table.fragment.rows[0].cells.len(), 2);
    assert_eq!(table.fragment.grid.column_count, 2);
}

#[tokio::test]
async fn table_fragment_clamps_html_span_attributes() {
    let root = dom::parse(
        "<html><body><table><colgroup span=\"1001px\"></colgroup><tr><td colspan=\"1001px\">A</td></tr><tr><td rowspan=\"999999999999999999999999px\">B</td><td>C</td></tr></table></body></html>",
    );
    let page = build_test_page(&root, &[]);
    let table = &page.children[0].children()[0].children()[0];

    let FormattingBox::Table(table) = table else {
        panic!("expected table formatting box");
    };
    assert_eq!(table.fragment.columns.len(), 1);
    assert_eq!(table.fragment.columns[0].span, 1000);
    assert_eq!(table.fragment.grid.rows[0][0].colspan, 1000);
    assert_eq!(table.fragment.grid.rows[1][0].rowspan, 1);
}

#[tokio::test]
async fn table_fragment_span_attributes_use_ascii_digits_only() {
    let root = dom::parse(
        "<html><body><table><col span=\"２\"></col><tr><td colspan=\"２\">A</td><td>B</td></tr></table></body></html>",
    );
    let page = build_test_page(&root, &[]);
    let table = &page.children[0].children()[0].children()[0];

    let FormattingBox::Table(table) = table else {
        panic!("expected table formatting box");
    };
    assert_eq!(table.fragment.columns[0].span, 1);
    assert_eq!(table.fragment.grid.rows[0][0].colspan, 1);
    assert_eq!(table.fragment.grid.rows[0][1].column, 1);
}

#[tokio::test]
async fn table_fragment_wraps_text_children_in_anonymous_cells() {
    let root = dom::parse(
        "<html><body><div style=\"display:table\">Lead<span style=\"display:table-cell\">Cell</span></div></body></html>",
    );
    let page = build_test_page(&root, &[]);
    let table = &page.children[0].children()[0].children()[0];

    let FormattingBox::Table(table) = table else {
        panic!("expected table formatting box");
    };
    assert_eq!(table.fragment.rows.len(), 1);
    assert_eq!(table.fragment.rows[0].cells.len(), 2);
    assert!(table.fragment.rows[0].cells[0].anonymous);
    assert!(table.fragment.rows[0].cells[0].element.is_none());
    assert_eq!(
        table.fragment.rows[0].cells[0]
            .children
            .iter()
            .map(FormattingBox::kind)
            .collect::<Vec<_>>(),
        vec![FormattingBoxKind::Text]
    );
}

#[tokio::test]
async fn table_fragment_groups_consecutive_non_cell_children_with_whitespace() {
    let root = dom::parse(
        "<html><body><div style=\"display:table\"><span style=\"display:inline-block\">A</span> <span style=\"display:inline-block\">B</span></div></body></html>",
    );
    let page = build_test_page(&root, &[]);
    let table = &page.children[0].children()[0].children()[0];

    let FormattingBox::Table(table) = table else {
        panic!("expected table formatting box");
    };
    assert_eq!(table.fragment.rows.len(), 1);
    assert_eq!(table.fragment.rows[0].cells.len(), 1);
    assert!(table.fragment.rows[0].cells[0].anonymous);
    assert_eq!(
        table.fragment.rows[0].cells[0]
            .children
            .iter()
            .map(FormattingBox::kind)
            .collect::<Vec<_>>(),
        vec![
            FormattingBoxKind::AtomicInline,
            FormattingBoxKind::Text,
            FormattingBoxKind::AtomicInline,
        ]
    );
}

#[tokio::test]
async fn table_fragment_ignores_whitespace_between_internal_cells() {
    let root = dom::parse(
        "<html><body><div style=\"display:table\"><span style=\"display:table-cell\">A</span> <span style=\"display:table-cell\">B</span></div></body></html>",
    );
    let page = build_test_page(&root, &[]);
    let table = &page.children[0].children()[0].children()[0];

    let FormattingBox::Table(table) = table else {
        panic!("expected table formatting box");
    };
    assert_eq!(table.fragment.rows.len(), 1);
    assert_eq!(table.fragment.rows[0].cells.len(), 2);
    assert!(!table.fragment.rows[0].cells[0].anonymous);
    assert!(!table.fragment.rows[0].cells[1].anonymous);
}

#[tokio::test]
async fn display_contents_inside_table_flattens_children_with_inherited_style() {
    let root = dom::parse(
        "<html><body><div style=\"display:table;color:red\"><div style=\"display:contents;color:green\">X<div style=\"display:table-cell\">X</div>X<div style=\"display:table-row\">X</div>X</div></div></body></html>",
    );
    let page = build_test_page(&root, &[]);
    let table = &page.children[0].children()[0].children()[0];

    let FormattingBox::Table(table) = table else {
        panic!("expected table formatting box");
    };
    assert_eq!(table.style.border_spacing.horizontal, 0.0);
    assert_eq!(table.style.border_spacing.vertical, 0.0);
    assert!(!table.style.border_spacing_explicit);
    assert_eq!(table.fragment.rows.len(), 3);
    assert_eq!(table.fragment.rows[0].cells.len(), 3);
    assert_eq!(table.fragment.rows[1].cells.len(), 1);
    assert_eq!(table.fragment.rows[2].cells.len(), 1);

    for row in &table.fragment.rows {
        for cell in &row.cells {
            assert_eq!(cell.children[0].style().color, Color::new(0, 128, 0));
        }
    }
}

#[tokio::test]
async fn table_fragment_grid_tracks_rowspan_and_colspan_occupancy() {
    let root = dom::parse(
        "<html><body><table><tbody><tr><td rowspan=\"2\">Span</td><td colspan=\"2\">Wide</td></tr><tr><td>A</td><td>B</td></tr></tbody></table></body></html>",
    );
    let page = build_test_page(&root, &[]);
    let table = &page.children[0].children()[0].children()[0];

    let FormattingBox::Table(table) = table else {
        panic!("expected table formatting box");
    };
    assert_eq!(table.fragment.grid.column_count, 3);
    assert_eq!(table.fragment.grid.rows[0].len(), 2);
    assert_eq!(table.fragment.grid.rows[0][0].column, 0);
    assert_eq!(table.fragment.grid.rows[0][0].rowspan, 2);
    assert_eq!(table.fragment.grid.rows[0][1].column, 1);
    assert_eq!(table.fragment.grid.rows[0][1].colspan, 2);
    assert_eq!(table.fragment.grid.rows[1][0].column, 1);
    assert_eq!(table.fragment.grid.rows[1][1].column, 2);
}

#[tokio::test]
async fn table_fragment_preserves_authored_empty_rows() {
    let root = dom::parse(
        "<html><body><table><tbody><tr><td>A</td></tr><tr style=\"height:40px\"></tr><tr><td>B</td></tr></tbody></table></body></html>",
    );
    let page = build_test_page(&root, &[]);
    let table = &page.children[0].children()[0].children()[0];

    let FormattingBox::Table(table) = table else {
        panic!("expected table formatting box");
    };
    assert_eq!(table.fragment.rows.len(), 3);
    assert_eq!(table.fragment.rows[1].cells.len(), 0);
    assert_eq!(table.fragment.grid.rows.len(), 3);
    assert_eq!(table.fragment.grid.rows[1].len(), 0);
}

#[tokio::test]
async fn inline_table_creates_atomic_inline_box_with_table_children() {
    let root = dom::parse(
        "<html><body><p>A<table style=\"display:inline-table\"><tr><td>B</td></tr></table>C</p></body></html>",
    );
    let page = build_test_page(&root, &[]);
    let paragraph = &page.children[0].children()[0].children()[0];

    assert_eq!(
        paragraph
            .children()
            .iter()
            .map(FormattingBox::kind)
            .collect::<Vec<_>>(),
        vec![
            FormattingBoxKind::Text,
            FormattingBoxKind::AtomicInline,
            FormattingBoxKind::Text,
        ]
    );
    assert_eq!(
        paragraph.children()[1]
            .children()
            .iter()
            .map(FormattingBox::kind)
            .collect::<Vec<_>>(),
        vec![FormattingBoxKind::Block]
    );
}

#[tokio::test]
async fn inline_text_boxes_preserve_collapsed_edge_spaces() {
    let root = dom::parse(
        "<html><body><p>A <span style=\"display:inline-block\">B</span> C</p></body></html>",
    );
    let page = build_test_page(&root, &[]);
    let paragraph = &page.children[0].children()[0].children()[0];

    match (&paragraph.children()[0], &paragraph.children()[2]) {
        (FormattingBox::Text(before), FormattingBox::Text(after)) => {
            assert_eq!(before.text, "A ");
            assert_eq!(after.text, " C");
        }
        _ => panic!("expected text boxes around the atomic inline box"),
    }
}

#[tokio::test]
async fn list_items_have_marker_boxes_in_formatting_tree() {
    let root = dom::parse(
        "<html><body><section style=\"display:list-item; list-style-position: inside\">Item</section></body></html>",
    );
    let page = build_test_page(&root, &[]);
    let section = &page.children[0].children()[0].children()[0];

    match section {
        FormattingBox::Block(box_) => {
            let marker = box_.marker.as_ref().expect("list-item marker box");
            assert_eq!(marker.style.list_style_position, ListStylePosition::Inside);
        }
        _ => panic!("expected display:list-item to create a block principal box"),
    }
}

#[tokio::test]
async fn run_in_merges_into_following_block_prelude() {
    let root = dom::parse(
        "<html><body><div><h3 style=\"display:run-in\">Term</h3><p>Definition</p></div></body></html>",
    );
    let page = build_test_page(&root, &[]);
    let div = &page.children[0].children()[0].children()[0];

    assert_eq!(
        div.children()
            .iter()
            .map(FormattingBox::kind)
            .collect::<Vec<_>>(),
        vec![FormattingBoxKind::Block]
    );
    let FormattingBox::Block(paragraph) = &div.children()[0] else {
        panic!("expected following paragraph block");
    };
    assert_eq!(paragraph.run_in_children.len(), 1);
    assert_eq!(
        paragraph.run_in_children[0].kind(),
        FormattingBoxKind::Inline
    );
    assert_eq!(
        paragraph.run_in_children[0].style().display,
        Display::INLINE
    );
}

#[tokio::test]
async fn run_in_sequence_keeps_whitespace_and_out_of_flow_boxes() {
    let root = dom::parse(
        "<html><body><div><h3 style=\"display:run-in\">One</h3> <span style=\"position:absolute\">Abs</span><h4 style=\"display:run-in\">Two</h4><p>Definition</p></div></body></html>",
    );
    let page = build_test_page(&root, &[]);
    let div = &page.children[0].children()[0].children()[0];
    let FormattingBox::Block(paragraph) = &div.children()[0] else {
        panic!("expected paragraph block");
    };

    assert_eq!(
        paragraph
            .run_in_children
            .iter()
            .map(FormattingBox::kind)
            .collect::<Vec<_>>(),
        vec![
            FormattingBoxKind::Inline,
            FormattingBoxKind::Text,
            FormattingBoxKind::Block,
            FormattingBoxKind::Inline,
        ]
    );
    assert!(is_out_of_flow_box(&paragraph.run_in_children[2]));
}

#[tokio::test]
async fn run_in_falls_back_before_bfc_block() {
    let root = dom::parse(
        "<html><body><div><h3 style=\"display:run-in\">Term</h3><section style=\"display:flow-root\">Block</section></div></body></html>",
    );
    let page = build_test_page(&root, &[]);
    let div = &page.children[0].children()[0].children()[0];

    assert_eq!(
        div.children()
            .iter()
            .map(FormattingBox::kind)
            .collect::<Vec<_>>(),
        vec![FormattingBoxKind::AnonymousBlock, FormattingBoxKind::Block]
    );
    assert_eq!(
        div.children()[0]
            .children()
            .iter()
            .map(FormattingBox::kind)
            .collect::<Vec<_>>(),
        vec![FormattingBoxKind::Inline]
    );
}

#[tokio::test]
async fn run_in_recurses_into_deepest_following_block() {
    let root = dom::parse(
        "<html><body><div><h3 style=\"display:run-in\">Term</h3><section><p>Definition</p></section></div></body></html>",
    );
    let page = build_test_page(&root, &[]);
    let div = &page.children[0].children()[0].children()[0];
    let section = &div.children()[0];
    let paragraph = &section.children()[0];
    let FormattingBox::Block(paragraph) = paragraph else {
        panic!("expected nested paragraph block");
    };

    assert_eq!(paragraph.run_in_children.len(), 1);
    assert_eq!(
        paragraph.run_in_children[0].style().display,
        Display::INLINE
    );
}

#[tokio::test]
async fn run_in_prelude_sits_after_marker_and_before_generated_before() {
    let root = dom::parse(
        "<html><body><div><h3 style=\"display:run-in\">Term</h3><p style=\"display:list-item\">Definition</p></div></body></html>",
    );
    let stylesheet = css::parse_stylesheet(&Css::from_string("p::before { content: \"Before\" }"));
    let page = build_test_page(&root, &[stylesheet]);
    let div = &page.children[0].children()[0].children()[0];
    let FormattingBox::Block(paragraph) = &div.children()[0] else {
        panic!("expected list-item paragraph block");
    };

    assert!(paragraph.marker.is_some());
    assert_eq!(paragraph.run_in_children.len(), 1);
    assert!(paragraph.style.before_style.is_some());
    assert_eq!(
        paragraph
            .children
            .iter()
            .map(FormattingBox::kind)
            .collect::<Vec<_>>(),
        vec![FormattingBoxKind::Text]
    );
}

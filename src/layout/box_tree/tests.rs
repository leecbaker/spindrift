use super::*;
use crate::css::{
    ComputedLengthPercentage, ComputedLengthPercentageOrAuto, ComputedLineHeight, Css, Edges,
    FontFamily, TextOrientation,
};
use std::collections::HashMap;
use std::rc::Rc;

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
    freeze_page_box(build_page_box(root, &stylesheets, &parent_style()))
}

async fn build_test_page_with_font_metrics<'a>(
    root: &'a Node,
    author_stylesheets: &[Stylesheet],
) -> PageBox<'a> {
    let mut stylesheets = vec![css::html5_user_agent_stylesheet()];
    stylesheets.extend_from_slice(author_stylesheets);
    let mut font_system = FontSystem::start_loading()
        .load_stylesheet_fonts(&stylesheets)
        .finish()
        .await;
    freeze_page_box(build_page_box_with_font_metrics(
        root,
        &stylesheets,
        &parent_style(),
        &mut font_system,
    ))
}

fn test_signature(tag: &str) -> ElementSignature {
    ElementSignature::new(tag, HashMap::new())
}

fn styled_text_box(text: &str, style: &ComputedStyle) -> MutableFormattingBox<'static> {
    MutableFormattingBox::Text(MutableTextBox {
        text: text.to_string(),
        style: Box::new(style.clone()),
    })
}

fn formatting_box_contains_text(box_: &FormattingBox<'_>, text: &str) -> bool {
    match box_ {
        FormattingBox::Text(text_box) => text_box.text.contains(text),
        _ => box_
            .children()
            .iter()
            .any(|child| formatting_box_contains_text(child, text)),
    }
}

#[test]
fn freeze_child_boxes_shares_equal_style_handles() {
    let mut style = ComputedStyle::initial();
    style.font_size = 19.0;

    let frozen = freeze_child_boxes(vec![
        styled_text_box("A", &style),
        styled_text_box("B", &style),
    ]);

    let [FormattingBox::Text(first), FormattingBox::Text(second)] = frozen.as_slice() else {
        panic!("expected two frozen text boxes");
    };
    assert_eq!(first.style.font_size, 19.0);
    assert!(Rc::ptr_eq(&first.style, &second.style));
}

#[test]
fn owned_style_mutation_does_not_mutate_frozen_style() {
    let mut style = ComputedStyle::initial();
    style.font_size = 19.0;

    let frozen = freeze_child_boxes(vec![styled_text_box("A", &style)]);
    let FormattingBox::Text(text) = &frozen[0] else {
        panic!("expected frozen text box");
    };

    let mut derived = owned_style(&text.style);
    derived.font_size = 23.0;

    assert_eq!(text.style.font_size, 19.0);
    assert_eq!(derived.font_size, 23.0);
}

#[test]
fn hidden_optgroup_suppresses_option_boxes() {
    let root = dom::parse(
        r#"<body><select size="4" class="red"><optgroup class="none green"><option>option</option></optgroup></select></body>"#,
    );
    let stylesheet = css::parse_stylesheet(&Css::from_string(
        ".none { display: none } .red { color: red } .green { color: green }",
    ));
    let page = build_test_page(&root, &[stylesheet]);
    let body = &page.children[0].children()[0];
    let select = &body.children()[0];

    assert!(
        !formatting_box_contains_text(select, "option"),
        "hidden optgroup should suppress descendant option boxes: {select:#?}"
    );
}

#[test]
fn hidden_option_inside_visible_optgroup_is_not_boxed() {
    let root = dom::parse(
        r#"<body><select size="4" class="red"><optgroup><option class="none">option</select></body>"#,
    );
    let stylesheet = css::parse_stylesheet(&Css::from_string(
        ".none { display: none } .red { color: red }",
    ));
    let page = build_test_page(&root, &[stylesheet]);
    let body = &page.children[0].children()[0];
    let select = &body.children()[0];

    assert!(
        !formatting_box_contains_text(select, "option"),
        "hidden option should not create descendant text boxes: {select:#?}"
    );
}

#[test]
fn select_display_none_wpt_fixture_has_no_option_text_boxes() {
    let root = dom::parse(
        r#"<body>
<option class="none red">text</option>
<optgroup class="none red">text</optgroup>
<optgroup class="none red"><option>option</option></optgroup>
<optgroup><option class="none red">option</option></optgroup>
<optgroup class="contents red"><option class="none">option</option></optgroup>
<optgroup class="contents green" label="optgroup"><option class="none red">option</option></optgroup>
<optgroup class="none red" label="optgroup"><option class="red">option</option></optgroup>
<br>
<select class="red" size="4">select</select>
<select size="4" class="red"><optgroup class="none" label="optgroup"></select>
<select size="4" class="red"><option class="none">option</select>
<select size="4" class="red"><optgroup><option class="none">option</select>
<select size="4"><optgroup class="none"><option class="green">option</select>
<select size="4" class="red"><optgroup class="none green" label="optgroup"><option>option</select>
<select size="4" class="red"><optgroup class="none"><option class="none">option</select>
<select size="4" class="red"><optgroup class="none green" label="optgroup"><option class="none">option</select>
</body>"#,
    );
    let stylesheet = css::parse_stylesheet(&Css::from_string(
        ".none { display: none } .contents { display: contents } .red { color: red } .green { color: green }",
    ));
    let page = build_test_page(&root, &[stylesheet]);

    assert!(
        !page
            .children
            .iter()
            .any(|child| formatting_box_contains_text(child, "option")),
        "fixture should not create option text boxes: {:#?}",
        page.children
    );
}

#[test]
fn freeze_table_fragment_preserves_and_shares_fragment_styles() {
    let table_node = Node::element("table");
    let NodeKind::Element(table_element) = &table_node.kind else {
        panic!("expected element node");
    };
    let mut style = ComputedStyle::initial();
    style.font_size = 21.0;
    let table_ancestor = test_signature("table");

    let fragment = MutableTableFragment {
        rows: vec![MutableTableFragmentRow {
            element: Some(table_element),
            signature: test_signature("tr"),
            ancestors: vec![table_ancestor.clone()],
            row_groups: vec![MutableTableFragmentRowGroup {
                element: table_element,
                signature: test_signature("tbody"),
                style: Some(Box::new(style.clone())),
            }],
            style: Some(Box::new(style.clone())),
            cells: vec![MutableTableFragmentCell {
                element: Some(table_element),
                signature: test_signature("td"),
                style: Some(Box::new(style.clone())),
                children: vec![styled_text_box("Cell", &style)],
                anonymous: false,
            }],
        }],
        captions: vec![MutableTableFragmentCaption {
            element: table_element,
            signature: test_signature("caption"),
            style: Some(Box::new(style.clone())),
            children: Vec::new(),
        }],
        columns: vec![MutableTableFragmentColumn {
            element: table_element,
            signature: test_signature("col"),
            style: Some(Box::new(style.clone())),
            group: Some(MutableTableFragmentColumnGroup {
                element: table_element,
                signature: test_signature("colgroup"),
                style: Some(Box::new(style.clone())),
                span: 1,
            }),
            span: 1,
        }],
        grid: TableFragmentGrid {
            rows: vec![vec![TableFragmentCellPlacement {
                cell: 0,
                column: 0,
                colspan: 1,
                rowspan: 1,
            }]],
            column_count: 1,
        },
    };

    let frozen = freeze_table_fragment(fragment);
    let row_style = frozen.rows[0].style.as_ref().unwrap();
    let group_style = frozen.rows[0].row_groups[0].style.as_ref().unwrap();
    let cell_style = frozen.rows[0].cells[0].style.as_ref().unwrap();
    let caption_style = frozen.captions[0].style.as_ref().unwrap();
    let column_style = frozen.columns[0].style.as_ref().unwrap();
    let column_group_style = frozen.columns[0]
        .group
        .as_ref()
        .unwrap()
        .style
        .as_ref()
        .unwrap();
    let FormattingBox::Text(text) = &frozen.rows[0].cells[0].children[0] else {
        panic!("expected frozen table cell text");
    };

    assert_eq!(row_style.font_size, 21.0);
    assert_eq!(frozen.rows[0].ancestors, vec![table_ancestor]);
    assert_eq!(frozen.grid.column_count, 1);
    assert!(Rc::ptr_eq(row_style, group_style));
    assert!(Rc::ptr_eq(row_style, cell_style));
    assert!(Rc::ptr_eq(row_style, caption_style));
    assert!(Rc::ptr_eq(row_style, column_style));
    assert!(Rc::ptr_eq(row_style, column_group_style));
    assert!(Rc::ptr_eq(row_style, &text.style));
}

#[test]
fn builds_styled_formatting_box_tree() {
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

#[test]
fn pure_block_children_remain_block_children() {
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

#[test]
fn formatting_whitespace_between_block_children_is_ignored() {
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

#[test]
fn pure_inline_content_keeps_inline_and_text_boxes() {
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

#[test]
fn generated_fixed_pseudos_are_out_of_flow_tree_abiding_boxes() {
    let root = dom::parse("<html><body><div id=\"test\"></div></body></html>");
    let stylesheet = css::parse_stylesheet(&Css::from_string(
        r#"#test::before,
           #test::after {
             content: "";
             position: fixed;
             width: 50pt;
             height: 100pt;
           }"#,
    ));
    let page = build_test_page(&root, &[stylesheet]);
    let div = &page.children[0].children()[0].children()[0];
    let children = div.children();

    assert_eq!(
        children.iter().map(FormattingBox::kind).collect::<Vec<_>>(),
        vec![FormattingBoxKind::Block, FormattingBoxKind::Block]
    );
    assert!(is_out_of_flow_box(&children[0]));
    assert!(is_out_of_flow_box(&children[1]));

    let FormattingBox::Block(before) = &children[0] else {
        panic!("expected generated ::before block");
    };
    let FormattingBox::Block(after) = &children[1] else {
        panic!("expected generated ::after block");
    };
    assert!(matches!(
        &before.source,
        BoxSource::GeneratedPseudo(pseudo) if pseudo.kind == GeneratedPseudoKind::Before
    ));
    assert!(matches!(
        &after.source,
        BoxSource::GeneratedPseudo(pseudo) if pseudo.kind == GeneratedPseudoKind::After
    ));
}

#[test]
fn floated_generated_pseudos_do_not_own_originating_element_children() {
    let root = dom::parse(
        "<html><body><section id=\"target\"><h4>Heading</h4><p>Body</p></section></body></html>",
    );
    let stylesheet = css::parse_stylesheet(&Css::from_string(
        "#target::before { content: ''; display: inline-block; float: left; height: 20pt; width: 20pt }",
    ));
    let page = build_test_page(&root, &[stylesheet]);
    let section = &page.children[0].children()[0].children()[0];
    assert_eq!(
        section
            .children()
            .iter()
            .map(FormattingBox::kind)
            .collect::<Vec<_>>(),
        vec![
            FormattingBoxKind::AnonymousBlock,
            FormattingBoxKind::Block,
            FormattingBoxKind::Block,
        ]
    );
    let FormattingBox::AnonymousBlock(anonymous) = &section.children()[0] else {
        panic!("expected an anonymous wrapper for the inline floated pseudo");
    };
    let before = &anonymous.children[0];

    assert!(matches!(
        before,
        FormattingBox::AtomicInline(box_)
            if matches!(&box_.source, BoxSource::GeneratedPseudo(pseudo)
                if pseudo.kind == GeneratedPseudoKind::Before)
    ));
    assert!(before.children().is_empty());
}

#[test]
fn box_tree_preserves_font_shorthand_unit_line_height() {
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
async fn font_size_ch_uses_measured_parent_zero_advance_during_box_tree_build() {
    let root = dom::parse(
        "<html><body><div><span style=\"font-size: 2ch\">probe</span></div></body></html>",
    );
    let stylesheet = css::parse_stylesheet(
        &Css::from_string(
            r#"@font-face {
                font-family: MetricProbe;
                src: url("tests/resources/fonts/noto-sans-v8-latin-regular.woff");
            }
            div {
                font-family: MetricProbe;
                font-size: 40pt;
            }"#,
        )
        .with_base_path(".")
        .expect("current directory should be a valid file URL"),
    );
    let mut metric_style = ComputedStyle {
        font_family: FontFamily::Names(vec!["MetricProbe".to_string()]),
        font_size: 40.0,
        ..ComputedStyle::initial()
    };
    metric_style.line_height = metric_style.font_size;
    let mut font_system = FontSystem::start_loading()
        .load_stylesheet_fonts(std::slice::from_ref(&stylesheet))
        .finish()
        .await;
    let parent_ch_advance = font_system.ch_advance(&metric_style);
    assert!(
        (parent_ch_advance.points() - metric_style.font_size * 0.5).abs() > 0.01,
        "fixture must differ from the generic 0.5em ch fallback"
    );

    let page = build_test_page_with_font_metrics(&root, &[stylesheet]).await;
    let span = &page.children[0].children()[0].children()[0].children()[0];

    assert!(
        (span.style().font_size - parent_ch_advance.points() * 2.0).abs() < 0.01,
        "font-size: 2ch should resolve against the measured parent zero advance"
    );
}

#[test]
fn mutable_tree_defers_parent_ch_font_size_without_a_font_system() {
    let root = dom::parse(
        "<html><body><div><span style=\"font-size: 2ch\">probe</span></div></body></html>",
    );
    let page = build_page_box(
        &root,
        &[css::html5_user_agent_stylesheet()],
        &parent_style(),
    );
    let span = &page.children[0].children()[0].children()[0].children()[0];

    assert!(matches!(
        &span.style().deferred_font_size,
        css::DeferredFontSize::RelativeToParent(value) if *value == css::ComputedLengthPercentage::from_ch(2.0)
    ));
}

#[tokio::test]
async fn pseudo_font_size_ch_uses_measured_originating_zero_advance_during_box_tree_build() {
    let root = dom::parse("<html><body><p>Probe</p></body></html>");
    let stylesheet = css::parse_stylesheet(
        &Css::from_string(
            r#"@font-face {
                font-family: MetricProbe;
                src: url("tests/resources/fonts/noto-sans-v8-latin-regular.woff");
            }
            p {
                font-family: MetricProbe;
                font-size: 40pt;
            }
            p::before {
                content: "x";
                font-size: 2ch;
            }
            p::first-line {
                font-size: 3ch;
            }
            p::first-letter {
                font-size: 4ch;
            }"#,
        )
        .with_base_path(".")
        .expect("current directory should be a valid file URL"),
    );
    let mut metric_style = ComputedStyle {
        font_family: FontFamily::Names(vec!["MetricProbe".to_string()]),
        font_size: 40.0,
        ..ComputedStyle::initial()
    };
    metric_style.line_height = metric_style.font_size;
    let mut font_system = FontSystem::start_loading()
        .load_stylesheet_fonts(std::slice::from_ref(&stylesheet))
        .finish()
        .await;
    let originating_ch_advance = font_system.ch_advance(&metric_style);
    assert!(
        (originating_ch_advance.points() - metric_style.font_size * 0.5).abs() > 0.01,
        "fixture must differ from the generic 0.5em ch fallback"
    );

    let page = build_test_page_with_font_metrics(&root, &[stylesheet]).await;
    let paragraph = &page.children[0].children()[0].children()[0];
    let style = paragraph.style();

    assert!(
        (style.before_style.as_ref().unwrap().font_size - originating_ch_advance.points() * 2.0)
            .abs()
            < 0.01,
        "::before font-size: 2ch should use the measured originating zero advance"
    );
    assert!(
        (style.first_line_style.as_ref().unwrap().font_size
            - originating_ch_advance.points() * 3.0)
            .abs()
            < 0.01,
        "::first-line font-size: 3ch should use the measured originating zero advance"
    );
    assert!(
        (style.first_letter_style.as_ref().unwrap().font_size
            - originating_ch_advance.points() * 4.0)
            .abs()
            < 0.01,
        "::first-letter font-size: 4ch should use the measured originating zero advance"
    );
}

#[test]
fn mixed_inline_and_block_children_create_anonymous_blocks_in_order() {
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

#[test]
fn anonymous_blocks_reset_non_inherited_parent_properties() {
    let stylesheet = css::parse_stylesheet(&Css::from_string(
        ".parent { color: red; writing-mode: vertical-rl; margin: 20pt; padding: 10pt; border: 3pt solid green; background: blue; width: 40pt; position: relative; float: left }",
    ));
    let root =
        dom::parse("<html><body><div class=\"parent\">Before<p>Block</p>After</div></body></html>");
    let page = build_test_page(&root, &[stylesheet]);
    let div = &page.children[0].children()[0].children()[0];

    for anonymous in [&div.children()[0], &div.children()[2]] {
        let FormattingBox::AnonymousBlock(anonymous) = anonymous else {
            panic!("expected anonymous block");
        };
        let style = &anonymous.style;
        assert_eq!(style.display, Display::BLOCK);
        assert_eq!(style.color, Color::new(255, 0, 0));
        assert_eq!(style.writing_mode, WritingMode::VerticalRl);
        assert_eq!(style.margin, Edges::ZERO);
        assert_eq!(style.padding, Edges::ZERO);
        assert_eq!(style.border_widths, Edges::ZERO);
        assert_eq!(style.background_color, None);
        assert!(style.box_values.width.is_auto());
        assert_eq!(style.position, Position::Static);
        assert_eq!(style.float, Float::None);
    }
}

#[test]
fn block_inside_inline_is_split_into_parent_block_flow() {
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

#[test]
fn positioned_inline_block_split_preserves_inline_context_for_block_segment() {
    let root = dom::parse(
        "<html><body><span style=\"position:relative; z-index:2; top:-100px\"><div>Block</div></span></body></html>",
    );
    let page = build_test_page(&root, &[]);
    let body = &page.children[0].children()[0];

    assert_eq!(
        body.children()
            .iter()
            .map(FormattingBox::kind)
            .collect::<Vec<_>>(),
        vec![FormattingBoxKind::InlineSplitBlockContext]
    );
    let FormattingBox::InlineSplitBlockContext(context) = &body.children()[0] else {
        panic!("positioned inline split should preserve a transparent block context");
    };
    assert_eq!(context.style.position, Position::Relative);
    assert_eq!(context.style.z_index, Some(2));
    assert_eq!(
        context
            .children
            .iter()
            .map(FormattingBox::kind)
            .collect::<Vec<_>>(),
        vec![FormattingBoxKind::Block]
    );
}

#[test]
fn floated_block_inside_positioned_inline_stays_in_inline_run() {
    let root = dom::parse(
        "<html><body><span style=\"position:relative\"><div style=\"float:left\">Float</div></span></body></html>",
    );
    let page = build_test_page(&root, &[]);
    let body = &page.children[0].children()[0];

    assert_eq!(
        body.children()
            .iter()
            .map(FormattingBox::kind)
            .collect::<Vec<_>>(),
        vec![FormattingBoxKind::Inline]
    );
    let FormattingBox::Inline(inline) = &body.children()[0] else {
        panic!("floated block should remain inside the inline formatting run");
    };
    assert_eq!(inline.style.position, Position::Relative);
    assert_eq!(
        inline
            .children
            .iter()
            .map(FormattingBox::kind)
            .collect::<Vec<_>>(),
        vec![FormattingBoxKind::Block]
    );
    assert!(is_floated_box(&inline.children[0]));
}

#[test]
fn block_inside_inline_preserves_empty_fragment_with_owned_inline_start_edge() {
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

#[test]
fn block_abspos_inside_inline_splits_static_position_fragments() {
    let root = dom::parse(
        "<html><body><span><div style=\"position:absolute\"></div>X</span></body></html>",
    );
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
            FormattingBoxKind::Inline,
            FormattingBoxKind::Block,
            FormattingBoxKind::Inline,
        ]
    );
    let FormattingBox::Inline(before_span) = &body.children()[0] else {
        panic!("pre-abspos inline fragment should be preserved");
    };
    assert!(before_span.children.is_empty());
    assert_eq!(
        before_span.fragment_edges,
        InlineBoxFragmentEdges {
            owns_start: true,
            owns_end: false,
        }
    );
    assert!(is_out_of_flow_box(&body.children()[1]));
    let FormattingBox::Inline(after_span) = &body.children()[2] else {
        panic!("post-abspos inline fragment should be preserved");
    };
    assert_eq!(
        after_span.fragment_edges,
        InlineBoxFragmentEdges {
            owns_start: false,
            owns_end: true,
        }
    );
}

#[test]
fn block_inside_inline_after_fragment_owns_only_inline_end_edge() {
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
        vec![
            FormattingBoxKind::AnonymousBlock,
            FormattingBoxKind::Block,
            FormattingBoxKind::AnonymousBlock
        ]
    );
    let FormattingBox::Inline(before_span) = &body.children()[0].children()[0] else {
        panic!("pre-block anonymous block should contain the span inline fragment");
    };
    assert_eq!(
        before_span.fragment_edges,
        InlineBoxFragmentEdges {
            owns_start: true,
            owns_end: false,
        }
    );
    assert_eq!(
        body.children()[2]
            .children()
            .iter()
            .map(FormattingBox::kind)
            .collect::<Vec<_>>(),
        vec![FormattingBoxKind::Inline]
    );
    let FormattingBox::Inline(span_fragment) = &body.children()[2].children()[0] else {
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

#[test]
fn block_inside_inline_splits_surrounding_inline_runs() {
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

#[test]
fn nested_block_inside_inline_preserves_each_inline_fragment_edges() {
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

#[test]
fn inline_block_creates_atomic_inline_box_with_children() {
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

#[test]
fn orphan_table_cells_create_anonymous_table_wrapper() {
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

#[test]
fn orphan_table_cell_with_formatting_whitespace_keeps_fragment_rows() {
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

#[test]
fn orphan_table_cells_inside_inline_create_anonymous_inline_table_wrapper() {
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

#[test]
fn nested_table_row_group_anonymous_fixup_keeps_sibling_cells_in_one_row() {
    let root = dom::parse(
        "<html><body>\
         <div style=\"display:table-row-group\">\
         <div style=\"display:table-row-group\">\
         <div style=\"display:table-cell\">a</div>\
         <div style=\"display:table-cell\">b</div>\
         </div>\
         <div style=\"display:table-cell\">cccc</div>\
         <div style=\"display:table-cell\">dddd</div>\
         </div>\
         </body></html>",
    );
    let page = build_test_page(&root, &[]);
    let table = &page.children[0].children()[0].children()[0];

    let FormattingBox::Table(table) = table else {
        panic!("expected anonymous table wrapper");
    };
    assert_eq!(table.fragment.rows.len(), 1);
    let row = &table.fragment.rows[0];
    assert_eq!(row.cells.len(), 3);
    assert_eq!(table.fragment.grid.column_count, 3);

    let first = &row.cells[0];
    assert!(first.anonymous);
    assert!(first.element.is_none());
    assert_eq!(first.children.len(), 1);
    let FormattingBox::Table(nested_table) = &first.children[0] else {
        panic!("expected nested anonymous table in anonymous cell");
    };
    assert_eq!(nested_table.style.display, Display::TABLE);
    assert_eq!(nested_table.fragment.rows.len(), 1);
    assert_eq!(nested_table.fragment.rows[0].cells.len(), 2);
    assert_eq!(
        table_cell_text(&nested_table.fragment.rows[0].cells[0]),
        "a"
    );
    assert_eq!(
        table_cell_text(&nested_table.fragment.rows[0].cells[1]),
        "b"
    );

    assert_eq!(
        row.cells[1].element.map(|element| element.tag.as_str()),
        Some("div")
    );
    assert_eq!(
        row.cells[2].element.map(|element| element.tag.as_str()),
        Some("div")
    );
    assert_eq!(table_cell_text(&row.cells[1]), "cccc");
    assert_eq!(table_cell_text(&row.cells[2]), "dddd");
}

fn table_cell_text<'a>(cell: &'a TableFragmentCell<'_>) -> &'a str {
    let [FormattingBox::Text(text)] = cell.children.as_slice() else {
        panic!("expected one text child");
    };
    &text.text
}

#[test]
fn table_box_contains_durable_fragment_rows_columns_and_captions() {
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

#[test]
fn table_fragment_clamps_html_span_attributes() {
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

#[test]
fn table_fragment_span_attributes_use_ascii_digits_only() {
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

#[test]
fn table_fragment_wraps_text_children_in_anonymous_cells() {
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

#[test]
fn table_fragment_groups_consecutive_non_cell_children_with_whitespace() {
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

#[test]
fn table_fragment_ignores_whitespace_between_internal_cells() {
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

#[test]
fn table_fragment_ignores_preserved_whitespace_adjacent_to_internal_children() {
    let root = dom::parse(
        "<html><body><div style=\"display:table; white-space:break-spaces\">\n  <span style=\"display:table-row\">\n    <span style=\"display:table-cell\">A</span>\n    <!-- split indentation -->\n  </span>\n</div></body></html>",
    );
    let page = build_test_page(&root, &[]);
    let table = &page.children[0].children()[0].children()[0];

    let FormattingBox::Table(table) = table else {
        panic!("expected table formatting box");
    };
    assert_eq!(table.fragment.rows.len(), 1);
    assert_eq!(table.fragment.rows[0].cells.len(), 1);
    assert!(!table.fragment.rows[0].cells[0].anonymous);
}

#[test]
fn body_display_table_preserves_positioned_child_in_table_fragment() {
    let root = dom::parse(
        "<html><body style=\"display:table\"><div style=\"display:table-cell\">A</div><p style=\"position:absolute;top:0;left:0\">Out of flow</p></body></html>",
    );
    let page = build_test_page(&root, &[]);
    let body = &page.children[0].children()[0];

    let FormattingBox::Table(table) = body else {
        panic!("expected body display:table to create a table formatting box");
    };
    assert_eq!(table.fragment.rows.len(), 1);
    assert_eq!(table.fragment.rows[0].cells.len(), 2);
    let positioned_cell = &table.fragment.rows[0].cells[1];
    assert!(positioned_cell.anonymous);
    assert_eq!(positioned_cell.children.len(), 1);
    let FormattingBox::Block(paragraph) = &positioned_cell.children[0] else {
        panic!("expected positioned paragraph to remain reachable");
    };
    assert_eq!(paragraph.element.tag, "p");
    assert_eq!(paragraph.style.position, Position::Absolute);
}

#[test]
fn table_fragment_preserves_metric_dependent_row_and_cell_styles() {
    let root = dom::parse(
        r#"<html><body><table><col style="writing-mode:vertical-rl;text-orientation:sideways;width:5ch"><tbody><tr style="writing-mode:vertical-rl;text-orientation:upright;line-height:5ch"><td style="height:5ch">A</td></tr></tbody></table></body></html>"#,
    );
    let page = build_test_page(&root, &[]);
    let table = &page.children[0].children()[0].children()[0];

    let FormattingBox::Table(table) = table else {
        panic!("expected table formatting box");
    };
    let row_style = table.fragment.rows[0]
        .style
        .as_ref()
        .expect("row style should be preserved");
    assert_eq!(row_style.writing_mode, WritingMode::VerticalRl);
    assert_eq!(row_style.text_orientation, TextOrientation::Upright);
    assert_eq!(
        row_style.line_height_value,
        ComputedLineHeight::Length(ComputedLengthPercentage::from_ch(5.0))
    );

    let cell_style = table.fragment.rows[0].cells[0]
        .style
        .as_ref()
        .expect("cell style should be preserved");
    assert_eq!(cell_style.writing_mode, WritingMode::VerticalRl);
    assert_eq!(cell_style.text_orientation, TextOrientation::Upright);
    assert_eq!(
        cell_style.box_values.height,
        ComputedLengthPercentageOrAuto::LengthPercentage(ComputedLengthPercentage::from_ch(5.0))
    );

    let column_style = table.fragment.columns[0]
        .style
        .as_ref()
        .expect("column style should be preserved");
    assert_eq!(column_style.writing_mode, WritingMode::VerticalRl);
    assert_eq!(column_style.text_orientation, TextOrientation::Sideways);
    assert_eq!(
        column_style.box_values.width,
        ComputedLengthPercentageOrAuto::LengthPercentage(ComputedLengthPercentage::from_ch(5.0))
    );
}

#[test]
fn display_contents_inside_table_flattens_children_with_inherited_style() {
    let root = dom::parse(
        "<html><body><div style=\"display:table;color:red\"><div style=\"display:contents;color:green\">X<div style=\"display:table-cell\">X</div>X<div style=\"display:table-row\">X</div>X</div></div></body></html>",
    );
    let page = build_test_page(&root, &[]);
    let table = &page.children[0].children()[0].children()[0];

    let FormattingBox::Table(table) = table else {
        panic!("expected table formatting box");
    };
    assert_eq!(table.style.border_spacing.horizontal.length_points(), 0.0);
    assert_eq!(table.style.border_spacing.vertical.length_points(), 0.0);
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

#[test]
fn display_contents_inside_table_row_wraps_misparented_row_in_anonymous_cell() {
    let root = dom::parse(
        "<html><body><div style=\"display:table;color:red\"><div style=\"display:table-row\"><div style=\"display:contents;color:green\">X<div style=\"display:table-cell\">X</div>X<div style=\"display:table-row\">X</div>X</div></div></div></body></html>",
    );
    let page = build_test_page(&root, &[]);
    let table = &page.children[0].children()[0].children()[0];

    let FormattingBox::Table(table) = table else {
        panic!("expected table formatting box");
    };
    assert_eq!(table.fragment.rows.len(), 1);
    let row = &table.fragment.rows[0];
    assert_eq!(row.cells.len(), 3);

    assert!(row.cells[0].anonymous);
    assert_eq!(table_cell_text(&row.cells[0]), "X");
    assert_eq!(
        row.cells[1].element.map(|element| element.tag.as_str()),
        Some("div")
    );
    assert_eq!(table_cell_text(&row.cells[1]), "X");
    assert!(row.cells[2].anonymous);
    assert_eq!(
        row.cells[2]
            .children
            .iter()
            .map(FormattingBox::kind)
            .collect::<Vec<_>>(),
        vec![
            FormattingBoxKind::Text,
            FormattingBoxKind::Table,
            FormattingBoxKind::Text
        ]
    );

    let FormattingBox::Table(nested_table) = &row.cells[2].children[1] else {
        panic!("expected nested anonymous table in trailing anonymous cell");
    };
    assert_eq!(nested_table.style.display, Display::TABLE);
    assert_eq!(nested_table.fragment.rows.len(), 1);
    assert_eq!(nested_table.fragment.rows[0].cells.len(), 1);
    assert_eq!(
        table_cell_text(&nested_table.fragment.rows[0].cells[0]),
        "X"
    );

    for cell in &row.cells {
        assert_table_cell_text_color(cell, Color::new(0, 128, 0));
    }
}

fn assert_table_cell_text_color(cell: &TableFragmentCell<'_>, color: Color) {
    for child in &cell.children {
        assert_formatting_box_text_color(child, color);
    }
}

fn assert_formatting_box_text_color(box_: &FormattingBox<'_>, color: Color) {
    if let FormattingBox::Text(text) = box_ {
        assert_eq!(text.style.color, color);
    }
    for child in box_.children() {
        assert_formatting_box_text_color(child, color);
    }
    if let FormattingBox::Table(table) = box_ {
        for row in &table.fragment.rows {
            for cell in &row.cells {
                assert_table_cell_text_color(cell, color);
            }
        }
    }
}

#[test]
fn table_fragment_grid_tracks_rowspan_and_colspan_occupancy() {
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

#[test]
fn table_fragment_preserves_authored_empty_rows() {
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

#[test]
fn inline_table_creates_atomic_inline_box_with_table_children() {
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

#[test]
fn inline_text_boxes_preserve_collapsed_edge_spaces() {
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

#[test]
fn list_items_have_marker_boxes_in_formatting_tree() {
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

#[test]
fn run_in_merges_into_following_block_prelude() {
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

#[test]
fn run_in_sequence_keeps_whitespace_and_out_of_flow_boxes() {
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

#[test]
fn run_in_falls_back_before_bfc_block() {
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

#[test]
fn run_in_recurses_into_deepest_following_block() {
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

#[test]
fn run_in_prelude_sits_after_marker_and_before_generated_before() {
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
        vec![FormattingBoxKind::Inline, FormattingBoxKind::Text]
    );
    let FormattingBox::Inline(before) = &paragraph.children[0] else {
        panic!("expected generated ::before inline box");
    };
    assert!(matches!(
        &before.source,
        BoxSource::GeneratedPseudo(pseudo) if pseudo.kind == GeneratedPseudoKind::Before
    ));
}

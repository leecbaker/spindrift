use super::*;
use crate::css::{
    ComputedLengthPercentage, ComputedLengthPercentageOrAuto, ComputedLineHeight, Css, Edges,
    FontFamily, Stylesheet, TextOrientation,
};
use std::collections::HashMap;
use std::rc::Rc;

fn parent_style() -> ComputedStyle {
    ComputedStyle {
        font_size: 12.0,
        line_height: 14.4,
        color: CssColor::BLACK,
        ..ComputedStyle::initial()
    }
}

fn build_test_page<'a>(root: &'a Node, author_stylesheets: &[Stylesheet]) -> PageBox<'a> {
    let stylesheets =
        Stylesheets::for_document(css::html5_user_agent_stylesheet(), None, author_stylesheets);
    freeze_page_box(build_page_box(root, &stylesheets, &parent_style()))
}

async fn build_test_page_with_font_metrics<'a>(
    root: &'a Node,
    author_stylesheets: &[Stylesheet],
) -> PageBox<'a> {
    let stylesheets =
        Stylesheets::for_document(css::html5_user_agent_stylesheet(), None, author_stylesheets);
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

fn collect_style_accessor_kinds<S: AsRef<ComputedStyle>>(
    box_: &FormattingBoxWith<'_, S>,
    kinds: &mut Vec<FormattingBoxKind>,
) {
    assert!(box_.style().font_size.is_finite());
    kinds.push(box_.kind());
    for child in box_.children() {
        collect_style_accessor_kinds(child, kinds);
    }
}

fn collect_children_mut_accessor_kinds<S: AsRef<ComputedStyle>>(
    box_: &mut FormattingBoxWith<'_, S>,
    kinds: &mut Vec<FormattingBoxKind>,
) {
    kinds.push(box_.kind());
    for child in box_.children_mut() {
        collect_children_mut_accessor_kinds(child, kinds);
    }
}

fn mutate_style_and_collect_accessor_kinds(
    box_: &mut MutableFormattingBox<'_>,
    kinds: &mut Vec<FormattingBoxKind>,
) {
    let original_font_size = box_.style().font_size;
    box_.style_mut().font_size = original_font_size + 1.0;
    assert_eq!(box_.style().font_size, original_font_size + 1.0);
    kinds.push(box_.kind());
    for child in box_.children_mut() {
        mutate_style_and_collect_accessor_kinds(child, kinds);
    }
}

fn collect_element_core_accessor_kinds<S: AsRef<ComputedStyle>>(
    box_: &FormattingBoxWith<'_, S>,
    core_kinds: &mut Vec<FormattingBoxKind>,
    non_core_kinds: &mut Vec<FormattingBoxKind>,
) {
    if let Some(core) = box_.element_core() {
        core_kinds.push(box_.kind());
        assert!(std::ptr::eq(core.children.as_slice(), box_.children()));
        assert_eq!(core.style.as_ref(), box_.style());
        if matches!(box_, FormattingBoxWith::InlineSplitBlockContext(_)) {
            assert!(box_.element_parts().is_none());
        } else {
            let (element, signature, style, children) = box_
                .element_parts()
                .expect("non-split element-backed box exposes element parts");
            assert!(std::ptr::eq(element, core.element));
            assert!(std::ptr::eq(signature, &core.signature));
            assert!(std::ptr::eq(style, core.style.as_ref()));
            assert!(std::ptr::eq(children, core.children.as_slice()));
        }
    } else {
        non_core_kinds.push(box_.kind());
        assert!(box_.element_parts().is_none());
    }
    for child in box_.children() {
        collect_element_core_accessor_kinds(child, core_kinds, non_core_kinds);
    }
}

fn mutate_mutable_element_cores_and_collect_kinds(
    box_: &mut MutableFormattingBox<'_>,
    core_kinds: &mut Vec<FormattingBoxKind>,
) {
    if let Some(core) = box_.element_core_mut() {
        let original_font_size = core.style.font_size;
        core.style.font_size = original_font_size + 1.0;
        assert_eq!(core.style.font_size, original_font_size + 1.0);
        core_kinds.push(box_.kind());
    }
    for child in box_.children_mut() {
        mutate_mutable_element_cores_and_collect_kinds(child, core_kinds);
    }
}

fn replace_frozen_element_core_styles_and_collect_kinds(
    box_: &mut FormattingBox<'_>,
    core_kinds: &mut Vec<FormattingBoxKind>,
) {
    let original_style = box_.element_core().map(|core| Rc::clone(&core.style));
    if let Some(core) = box_.element_core_mut() {
        let mut replacement = core.style.as_ref().clone();
        replacement.font_size += 1.0;
        core.style = Rc::new(replacement);
        core_kinds.push(box_.kind());
    }
    if let Some(original_style) = original_style {
        let replacement = &box_
            .element_core()
            .expect("element-backed box retains core after replacement")
            .style;
        assert!(!Rc::ptr_eq(&original_style, replacement));
        assert_eq!(replacement.font_size, original_style.font_size + 1.0);
        assert!(original_style.font_size.is_finite());
    }
    for child in box_.children_mut() {
        replace_frozen_element_core_styles_and_collect_kinds(child, core_kinds);
    }
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
fn element_box_core_round_trip_preserves_identity_source_marker_and_children() {
    let node = Node::element("div");
    let NodeKind::Element(element) = &node.kind else {
        panic!("expected element node");
    };
    let mut style = ComputedStyle::initial();
    style.font_size = 19.0;
    let source = BoxSource::GeneratedPseudo(Box::new(GeneratedPseudoBox {
        originating_element: element,
        originating_signature: test_signature("div"),
        originating_clear: Clear::None,
        kind: GeneratedPseudoKind::Before,
    }));
    let frozen = freeze_child_boxes(vec![MutableFormattingBox::Block(MutableBlockBox {
        core: ElementBoxCoreWith {
            element,
            signature: test_signature("div"),
            source,
            style: Box::new(style.clone()),
            children: vec![styled_text_box("child", &style)],
        },
        marker: Some(MutableMarkerBox {
            style: Box::new(style.clone()),
        }),
        run_in_children: Vec::new(),
        fieldset: None,
    })]);

    let [FormattingBox::Block(frozen_block)] = frozen.as_slice() else {
        panic!("expected frozen block");
    };
    assert!(std::ptr::eq(frozen_block.core.element, element));
    assert_eq!(frozen_block.core.style.font_size, 19.0);
    assert!(matches!(
        &frozen_block.core.source,
        BoxSource::GeneratedPseudo(pseudo) if pseudo.kind == GeneratedPseudoKind::Before
    ));
    assert!(frozen_block.marker.is_some());
    assert!(formatting_box_contains_text(
        &frozen_block.core.children[0],
        "child"
    ));

    let thawed = clone_frozen_child_boxes_as_mutable(&frozen);
    let [MutableFormattingBox::Block(thawed_block)] = thawed.as_slice() else {
        panic!("expected thawed block");
    };
    assert!(std::ptr::eq(thawed_block.core.element, element));
    assert_eq!(thawed_block.core.style.font_size, 19.0);
    assert!(matches!(
        &thawed_block.core.source,
        BoxSource::GeneratedPseudo(pseudo) if pseudo.kind == GeneratedPseudoKind::Before
    ));
    assert!(thawed_block.marker.is_some());
    assert!(matches!(
        &thawed_block.core.children[0],
        MutableFormattingBox::Text(text) if text.text == "child"
    ));
}

#[test]
fn style_accessor_supports_every_formatting_box_variant() {
    let root = dom::parse(
        r#"<html><body>
            <div>text<span>inline</span><p>block</p></div>
            <span style="position: relative"><div>split context</div></span>
            <span style="display: inline-block">atomic</span>
            <table><tr><td>table</td></tr></table>
            <div style="display: flex">flex</div>
            <img style="display: block" src="x">
        </body></html>"#,
    );
    let stylesheets = Stylesheets::for_document(css::html5_user_agent_stylesheet(), None, &[]);
    let mutable = build_page_box(&root, &stylesheets, &parent_style());
    let frozen = freeze_page_box(build_page_box(&root, &stylesheets, &parent_style()));

    let mut mutable_kinds = Vec::new();
    for box_ in &mutable.children {
        collect_style_accessor_kinds(box_, &mut mutable_kinds);
    }
    let mut frozen_kinds = Vec::new();
    for box_ in &frozen.children {
        collect_style_accessor_kinds(box_, &mut frozen_kinds);
    }

    let expected = [
        FormattingBoxKind::Block,
        FormattingBoxKind::Inline,
        FormattingBoxKind::InlineSplitBlockContext,
        FormattingBoxKind::AnonymousBlock,
        FormattingBoxKind::AtomicInline,
        FormattingBoxKind::Text,
        FormattingBoxKind::Table,
        FormattingBoxKind::Flex,
        FormattingBoxKind::Replaced,
    ];
    for kind in expected {
        assert!(mutable_kinds.contains(&kind), "mutable tree lacks {kind:?}");
        assert!(frozen_kinds.contains(&kind), "frozen tree lacks {kind:?}");
    }
}

#[test]
fn mutable_accessors_support_every_formatting_box_variant() {
    let root = dom::parse(
        r#"<html><body>
            <div>text<span>inline</span><p>block</p></div>
            <span style="position: relative"><div>split context</div></span>
            <span style="display: inline-block">atomic</span>
            <table><tr><td>table</td></tr></table>
            <div style="display: flex">flex</div>
            <img style="display: block" src="x">
        </body></html>"#,
    );
    let stylesheets = Stylesheets::for_document(css::html5_user_agent_stylesheet(), None, &[]);
    let mut mutable = build_page_box(&root, &stylesheets, &parent_style());
    let mut frozen = freeze_page_box(build_page_box(&root, &stylesheets, &parent_style()));

    let mut mutable_kinds = Vec::new();
    for box_ in &mut mutable.children {
        mutate_style_and_collect_accessor_kinds(box_, &mut mutable_kinds);
    }
    let mut frozen_kinds = Vec::new();
    for box_ in &mut frozen.children {
        collect_children_mut_accessor_kinds(box_, &mut frozen_kinds);
    }

    let expected = [
        FormattingBoxKind::Block,
        FormattingBoxKind::Inline,
        FormattingBoxKind::InlineSplitBlockContext,
        FormattingBoxKind::AnonymousBlock,
        FormattingBoxKind::AtomicInline,
        FormattingBoxKind::Text,
        FormattingBoxKind::Table,
        FormattingBoxKind::Flex,
        FormattingBoxKind::Replaced,
    ];
    for kind in expected {
        assert!(mutable_kinds.contains(&kind), "mutable tree lacks {kind:?}");
        assert!(frozen_kinds.contains(&kind), "frozen tree lacks {kind:?}");
    }
}

#[test]
fn element_core_accessors_support_every_formatting_box_variant() {
    let root = dom::parse(
        r#"<html><body>
            <div>text<span>inline</span><p>block</p></div>
            <span style="position: relative"><div>split context</div></span>
            <span style="display: inline-block">atomic</span>
            <table><tr><td>table</td></tr></table>
            <div style="display: flex">flex</div>
            <img style="display: block" src="x">
        </body></html>"#,
    );
    let stylesheets = Stylesheets::for_document(css::html5_user_agent_stylesheet(), None, &[]);
    let mut mutable = build_page_box(&root, &stylesheets, &parent_style());
    let mut frozen = freeze_page_box(build_page_box(&root, &stylesheets, &parent_style()));

    let mut mutable_core_kinds = Vec::new();
    let mut mutable_non_core_kinds = Vec::new();
    for box_ in &mutable.children {
        collect_element_core_accessor_kinds(
            box_,
            &mut mutable_core_kinds,
            &mut mutable_non_core_kinds,
        );
    }
    let mut frozen_core_kinds = Vec::new();
    let mut frozen_non_core_kinds = Vec::new();
    for box_ in &frozen.children {
        collect_element_core_accessor_kinds(
            box_,
            &mut frozen_core_kinds,
            &mut frozen_non_core_kinds,
        );
    }

    let element_backed = [
        FormattingBoxKind::Block,
        FormattingBoxKind::Inline,
        FormattingBoxKind::InlineSplitBlockContext,
        FormattingBoxKind::AtomicInline,
        FormattingBoxKind::Table,
        FormattingBoxKind::Flex,
        FormattingBoxKind::Replaced,
    ];
    for kind in element_backed {
        assert!(
            mutable_core_kinds.contains(&kind),
            "mutable tree lacks {kind:?}"
        );
        assert!(
            frozen_core_kinds.contains(&kind),
            "frozen tree lacks {kind:?}"
        );
    }
    for kind in [FormattingBoxKind::AnonymousBlock, FormattingBoxKind::Text] {
        assert!(
            mutable_non_core_kinds.contains(&kind),
            "mutable tree unexpectedly lacks non-core {kind:?}"
        );
        assert!(
            frozen_non_core_kinds.contains(&kind),
            "frozen tree unexpectedly lacks non-core {kind:?}"
        );
    }

    let mut mutated_mutable_core_kinds = Vec::new();
    for box_ in &mut mutable.children {
        mutate_mutable_element_cores_and_collect_kinds(box_, &mut mutated_mutable_core_kinds);
    }
    let mut replaced_frozen_core_kinds = Vec::new();
    for box_ in &mut frozen.children {
        replace_frozen_element_core_styles_and_collect_kinds(box_, &mut replaced_frozen_core_kinds);
    }
    for kind in element_backed {
        assert!(
            mutated_mutable_core_kinds.contains(&kind),
            "mutable core mutation missed {kind:?}"
        );
        assert!(
            replaced_frozen_core_kinds.contains(&kind),
            "frozen core replacement missed {kind:?}"
        );
    }
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
fn user_agent_ruby_roles_preserve_the_internal_inline_tree() {
    let root = dom::parse(
        "<html><body>X<ruby class=\"rel\"><rbc><rb><span class=\"abs\">X</span></rb></rbc></ruby></body></html>",
    );
    let stylesheet = css::parse_stylesheet(&Css::from_string(
        ".rel { position: relative } .abs { position:absolute; left: 0; top: -1em }",
    ));
    let page = build_test_page(&root, &[stylesheet]);
    let body = &page.children[0].children()[0];
    assert_eq!(body.kind(), FormattingBoxKind::Block);
    assert_eq!(body.children()[0].kind(), FormattingBoxKind::Text);
    let ruby = &body.children()[1];
    let FormattingBox::Inline(ruby) = ruby else {
        panic!("ruby kind: {:?}", ruby.kind())
    };
    assert_eq!(ruby.core.style.display.inner, DisplayInner::Ruby);
    assert_eq!(ruby.core.style.position, Position::Relative);
    let FormattingBox::Inline(rbc) = &ruby.core.children[0] else {
        panic!("rbc kind")
    };
    assert_eq!(
        rbc.core.style.display.inner,
        DisplayInner::RubyBaseContainer
    );
    let FormattingBox::Inline(rb) = &rbc.core.children[0] else {
        panic!("rb kind")
    };
    assert_eq!(rb.core.style.display.inner, DisplayInner::RubyBase);
    let FormattingBox::Block(abs) = &rb.core.children[0] else {
        panic!("out-of-flow descendants are blockified")
    };
    assert!(matches!(abs.core.style.position, Position::Absolute));
}

#[test]
fn positioned_ruby_base_container_preserves_its_positioning_role() {
    let root = dom::parse(
        "<html><body><ruby><rbc class=\"rel\"><rb><span class=\"abs\">X</span></rb></rbc></ruby></body></html>",
    );
    let stylesheet = css::parse_stylesheet(&Css::from_string(
        ".rel { position: relative } .abs { position:absolute; left: 0; top: -1em }",
    ));
    let page = build_test_page(&root, &[stylesheet]);
    let body = &page.children[0].children()[0];
    let FormattingBox::Inline(ruby) = &body.children()[0] else {
        panic!("ruby kind")
    };
    let FormattingBox::Inline(rbc) = &ruby.core.children[0] else {
        panic!("rbc kind")
    };
    assert_eq!(
        rbc.core.style.display.inner,
        DisplayInner::RubyBaseContainer
    );
    assert_eq!(rbc.core.style.position, Position::Relative);
}

#[test]
fn ruby_normalization_pairs_explicit_base_and_annotation_segments() {
    let root = dom::parse(
        "<html><body><ruby><rbc><rb>A</rb><rb>B</rb></rbc><rtc><rt>a</rt><rt>b</rt></rtc></ruby></body></html>",
    );
    let page = build_test_page(&root, &[]);
    let body = &page.children[0].children()[0];
    let FormattingBox::Inline(ruby) = &body.children()[0] else {
        panic!("ruby generates an inline formatting box")
    };
    let normalized = crate::layout::ruby::NormalizedRuby::from_children(&ruby.core.children);

    assert_eq!(normalized.columns.len(), 2);
    assert_eq!(normalized.annotation_level_count, 1);
    assert!(
        normalized
            .columns
            .iter()
            .all(|column| !column.base.is_empty())
    );
    assert!(normalized.columns.iter().all(|column| {
        column.annotations.len() == 1
            && column.annotations[0].starts_span
            && column.annotations[0].span == 1
    }));
}

#[test]
fn ruby_normalization_spans_only_anonymous_single_annotations() {
    let root =
        dom::parse("<html><body><ruby><rb>A</rb> <rb>B</rb><rtc>ab</rtc></ruby></body></html>");
    let page = build_test_page(&root, &[]);
    let body = &page.children[0].children()[0];
    let FormattingBox::Inline(ruby) = &body.children()[0] else {
        panic!("ruby generates an inline formatting box")
    };
    let normalized = crate::layout::ruby::NormalizedRuby::from_children(&ruby.core.children);

    assert_eq!(normalized.columns.len(), 2);
    assert_eq!(normalized.annotation_level_count, 1);
    assert_eq!(normalized.columns[0].annotations[0].span, 2);
    assert!(normalized.columns[0].annotations[0].starts_span);
    assert_eq!(normalized.columns[1].annotations[0].span, 2);
    assert!(!normalized.columns[1].annotations[0].starts_span);
}

#[test]
fn ruby_normalization_excludes_out_of_flow_annotation_content() {
    let root = dom::parse(
        "<html><body><ruby><rb>A</rb><rtc><rt><span class=\"abs\">a</span></rt></rtc></ruby></body></html>",
    );
    let stylesheet = css::parse_stylesheet(&Css::from_string(".abs { position: absolute }"));
    let page = build_test_page(&root, &[stylesheet]);
    let body = &page.children[0].children()[0];
    let FormattingBox::Inline(ruby) = &body.children()[0] else {
        panic!("ruby generates an inline formatting box")
    };
    let normalized = crate::layout::ruby::NormalizedRuby::from_children(&ruby.core.children);

    assert_eq!(normalized.columns.len(), 1);
    assert_eq!(normalized.annotation_level_count, 0);
    assert!(normalized.columns[0].annotations.is_empty());
}

#[test]
fn ruby_normalization_inlinifies_direct_block_children() {
    let root = dom::parse(
        "<html><body><div><ruby>a<div class=\"inline\">b</div>c</ruby></div></body></html>",
    );
    let stylesheet = css::parse_stylesheet(&Css::from_string(
        ".inline { display: block; width: 30px; height: 30px }",
    ));
    let page = build_test_page(&root, &[stylesheet]);
    let body = &page.children[0].children()[0];
    let FormattingBox::Block(div) = &body.children()[0] else {
        panic!("div generates a block formatting box")
    };
    let FormattingBox::Inline(ruby) = &div.core.children[0] else {
        panic!("ruby generates an inline formatting box")
    };
    let normalized = crate::layout::ruby::NormalizedRuby::from_children(&ruby.core.children);
    assert_eq!(normalized.columns.len(), 1);
    assert!(matches!(
        normalized.columns[0].base.boxes.get(1),
        Some(FormattingBox::AtomicInline(box_))
            if box_.core.style.display == Display::new(DisplayOuter::Inline, DisplayInner::FlowRoot)
    ));
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
        &before.core.source,
        BoxSource::GeneratedPseudo(pseudo) if pseudo.kind == GeneratedPseudoKind::Before
    ));
    assert!(matches!(
        &after.core.source,
        BoxSource::GeneratedPseudo(pseudo) if pseudo.kind == GeneratedPseudoKind::After
    ));
}

#[test]
fn gcpm_footnote_detaches_its_body_and_keeps_a_source_ordered_call() {
    let root = dom::parse(
        "<html><body><p>Lead <span id=\"note\">footnote body</span> tail</p></body></html>",
    );
    let stylesheet = css::parse_stylesheet(&Css::from_string(
        "#note { float: footnote } #note::footnote-call { content: '[' counter(footnote) ']' }",
    ));
    let page = build_test_page(&root, &[stylesheet]);

    assert_eq!(page.footnotes.len(), 1);
    let footnote = &page.footnotes[0];
    assert_eq!(footnote.body.style().float, css::Float::None);
    assert!(formatting_box_contains_text(
        &footnote.body,
        "footnote body"
    ));

    fn contains_footnote_call(box_: &FormattingBox<'_>) -> bool {
        box_.element_core().is_some_and(|core| {
            matches!(
                &core.source,
                BoxSource::GeneratedPseudo(pseudo)
                    if pseudo.kind == GeneratedPseudoKind::FootnoteCall
            )
        }) || box_.children().iter().any(contains_footnote_call)
    }
    assert!(page.children.iter().any(contains_footnote_call));

    fn footnote_event_has_call(event: &CounterEventNode<'_>, element: &Element) -> bool {
        (std::ptr::eq(event.element, element)
            && event.source == CounterEventSource::Principal
            && event
                .counter_style
                .counter_increments
                .iter()
                .any(|change| change.name == "footnote" && change.value.get() == 1)
            && event
                .children
                .iter()
                .any(|child| child.source == CounterEventSource::FootnoteCall)
            && event
                .children
                .iter()
                .any(|child| child.source == CounterEventSource::FootnoteMarker))
            || event
                .children
                .iter()
                .any(|child| footnote_event_has_call(child, element))
    }
    assert!(
        page.counter_events
            .iter()
            .any(|event| footnote_event_has_call(event, footnote.element))
    );
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
            if matches!(&box_.core.source, BoxSource::GeneratedPseudo(pseudo)
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
    assert!(!test_div.style().line_height_is_normal());
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
    let stylesheets = Stylesheets::for_document(css::html5_user_agent_stylesheet(), None, &[]);
    let page = build_page_box(&root, &stylesheets, &parent_style());
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
        assert_eq!(style.color, CssColor::new(255, 0, 0));
        assert_eq!(style.writing_mode, WritingMode::VerticalRl);
        assert_eq!(style.margin, Edges::ZERO);
        assert_eq!(style.padding, Edges::ZERO);
        assert_eq!(style.border_widths, Edges::ZERO);
        assert_eq!(style.background_color.color(), Some(CssColor::TRANSPARENT));
        assert!(style.box_values.width.is_auto());
        assert_eq!(style.position, Position::Static);
        assert_eq!(style.float, Float::None);
    }
}

#[test]
fn text_nodes_inherit_only_inherited_properties() {
    let stylesheet = css::parse_stylesheet(&Css::from_string(
        ".parent { color: red; writing-mode: vertical-rl; background: blue; width: 40pt; position: relative }",
    ));
    let root = dom::parse("<html><body><div class=\"parent\">Text</div></body></html>");
    let page = build_test_page(&root, &[stylesheet]);
    let div = &page.children[0].children()[0].children()[0];
    let [FormattingBox::Text(text)] = div.children() else {
        panic!("expected direct text child");
    };

    assert_eq!(text.style.color, CssColor::new(255, 0, 0));
    assert_eq!(text.style.writing_mode, WritingMode::VerticalRl);
    assert_eq!(
        text.style.background_color.color(),
        Some(CssColor::TRANSPARENT)
    );
    assert!(text.style.box_values.width.is_auto());
    assert_eq!(text.style.position, Position::Static);
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
    assert_eq!(context.core.style.position, Position::Relative);
    assert_eq!(context.core.style.z_index, css::ZIndex::StackLevel(2));
    assert_eq!(
        context
            .core
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
    assert_eq!(inline.core.style.position, Position::Relative);
    assert_eq!(
        inline
            .core
            .children
            .iter()
            .map(FormattingBox::kind)
            .collect::<Vec<_>>(),
        vec![FormattingBoxKind::Block]
    );
    assert!(is_floated_box(&inline.core.children[0]));
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
    assert!(before_span.core.children.is_empty());
    assert_eq!(
        before_span.fragment_edges,
        InlineBoxFragmentEdges {
            owns_start: true,
            owns_end: false,
        }
    );
}

#[test]
fn block_abspos_inside_inline_remains_in_inline_formatting_context() {
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
        vec![FormattingBoxKind::Inline]
    );
    let FormattingBox::Inline(span) = &body.children()[0] else {
        panic!("out-of-flow descendants must remain in the inline's formatting context");
    };
    assert!(is_out_of_flow_box(&span.core.children[0]));
    assert_eq!(span.fragment_edges, InlineBoxFragmentEdges::ALL);
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
    let FormattingBox::Inline(before_em) = &before_span.core.children[1] else {
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
    let FormattingBox::Inline(after_em) = &after_span.core.children[0] else {
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
fn flex_children_are_blockified_before_table_fixup() {
    let root = dom::parse(
        "<html><body><div style=\"display:flex\"><span style=\"display:table-cell\">A</span><span style=\"display:table-cell\">B</span></div></body></html>",
    );
    let page = build_test_page(&root, &[]);
    let flex = &page.children[0].children()[0].children()[0];

    assert!(matches!(flex, FormattingBox::Flex(_)));
    assert_eq!(
        flex.children()
            .iter()
            .map(FormattingBox::kind)
            .collect::<Vec<_>>(),
        vec![FormattingBoxKind::Block, FormattingBoxKind::Block]
    );
    assert!(
        flex.children()
            .iter()
            .all(|child| child.style().display == Display::BLOCK)
    );
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
        span.core
            .children
            .iter()
            .map(FormattingBox::kind)
            .collect::<Vec<_>>(),
        vec![FormattingBoxKind::AtomicInline]
    );
    let FormattingBox::AtomicInline(table) = &span.core.children[0] else {
        panic!("expected anonymous inline-table wrapper");
    };
    assert_eq!(table.core.style.display, Display::INLINE_TABLE);
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
    assert_eq!(nested_table.core.style.display, Display::TABLE);
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
fn anonymous_table_cell_splits_block_descendants_out_of_inline_children() {
    let root = dom::parse(
        "<html><body><span style=\"display:table-row\"><span>aaa<span style=\"display:block\"></span><span style=\"display:table-cell\">bbb</span></span></span></body></html>",
    );
    let page = build_test_page(&root, &[]);
    let table = &page.children[0].children()[0].children()[0];

    let FormattingBox::Table(table) = table else {
        panic!("expected anonymous table wrapper");
    };
    let [row] = table.fragment.rows.as_slice() else {
        panic!("expected one generated table row");
    };
    let [cell] = row.cells.as_slice() else {
        panic!("expected one generated table cell");
    };
    assert!(cell.anonymous);
    assert_eq!(
        cell.children
            .iter()
            .map(FormattingBox::kind)
            .collect::<Vec<_>>(),
        vec![
            FormattingBoxKind::AnonymousBlock,
            FormattingBoxKind::Block,
            FormattingBoxKind::AnonymousBlock,
        ]
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
    assert_eq!(paragraph.core.element.tag, "p");
    assert_eq!(paragraph.core.style.position, Position::Absolute);
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
    assert_eq!(
        table.core.style.border_spacing.horizontal.length_points(),
        0.0
    );
    assert_eq!(
        table.core.style.border_spacing.vertical.length_points(),
        0.0
    );
    assert!(!table.core.style.border_spacing.is_author_declared());
    assert_eq!(table.fragment.rows.len(), 3);
    assert_eq!(table.fragment.rows[0].cells.len(), 3);
    assert_eq!(table.fragment.rows[1].cells.len(), 1);
    assert_eq!(table.fragment.rows[2].cells.len(), 1);

    for row in &table.fragment.rows {
        for cell in &row.cells {
            assert_eq!(cell.children[0].style().color, CssColor::new(0, 128, 0));
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
            FormattingBoxKind::AnonymousBlock,
            FormattingBoxKind::Table,
            FormattingBoxKind::AnonymousBlock
        ]
    );

    let FormattingBox::Table(nested_table) = &row.cells[2].children[1] else {
        panic!("expected nested anonymous table in trailing anonymous cell");
    };
    assert_eq!(nested_table.core.style.display, Display::TABLE);
    assert_eq!(nested_table.fragment.rows.len(), 1);
    assert_eq!(nested_table.fragment.rows[0].cells.len(), 1);
    assert_eq!(
        table_cell_text(&nested_table.fragment.rows[0].cells[0]),
        "X"
    );

    for cell in &row.cells {
        assert_table_cell_text_color(cell, CssColor::new(0, 128, 0));
    }
}

fn assert_table_cell_text_color(cell: &TableFragmentCell<'_>, color: CssColor) {
    for child in &cell.children {
        assert_formatting_box_text_color(child, color);
    }
}

fn assert_formatting_box_text_color(box_: &FormattingBox<'_>, color: CssColor) {
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
fn class_selected_inline_table_creates_an_atomic_inline_box() {
    let root = dom::parse(
        "<html><body><div><span>Before</span><table class=\"table\"><td>Cell</td></table></div></body></html>",
    );
    let stylesheet = css::parse_stylesheet(&Css::from_string(".table { display: inline-table; }"));
    let page = build_test_page(&root, &[stylesheet]);
    let container = &page.children[0].children()[0].children()[0];

    assert!(
        container
            .children()
            .iter()
            .any(|child| matches!(child, FormattingBox::AtomicInline(_)))
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
    assert!(paragraph.core.style.before_style.is_some());
    assert_eq!(
        paragraph
            .core
            .children
            .iter()
            .map(FormattingBox::kind)
            .collect::<Vec<_>>(),
        vec![FormattingBoxKind::Inline, FormattingBoxKind::Text]
    );
    let FormattingBox::Inline(before) = &paragraph.core.children[0] else {
        panic!("expected generated ::before inline box");
    };
    assert!(matches!(
        &before.core.source,
        BoxSource::GeneratedPseudo(pseudo) if pseudo.kind == GeneratedPseudoKind::Before
    ));
}

use super::*;

#[test]
fn inline_break_policy_differences_require_inline_item_collection() {
    let mut parent = ComputedStyle::initial();
    let mut child = parent.clone();

    assert!(!inline_break_policy_differs(&parent, &child));
    assert!(!inline_style_affects_line(&parent, &child));

    child.hyphens = css::Hyphens::Auto;
    child.language = css::ContentLanguage::from_html_attribute("en");
    assert!(inline_break_policy_differs(&parent, &child));
    assert!(inline_style_affects_line(&parent, &child));

    parent = child.clone();
    child.hyphens = css::Hyphens::None;
    assert!(inline_break_policy_differs(&parent, &child));

    child = parent.clone();
    child.hyphenate_character = css::HyphenateCharacter::String("=".into());
    assert!(inline_break_policy_differs(&parent, &child));

    child = parent.clone();
    child.hyphenate_limit_chars = css::HyphenateLimitChars {
        total: 6,
        before: 3,
        after: 2,
    };
    assert!(inline_break_policy_differs(&parent, &child));

    child = parent.clone();
    child.word_break = css::WordBreak::BreakAll;
    assert!(inline_break_policy_differs(&parent, &child));

    child = parent.clone();
    child.overflow_wrap = css::OverflowWrap::Anywhere;
    assert!(inline_break_policy_differs(&parent, &child));

    child = parent.clone();
    child.line_break = css::LineBreak::Anywhere;
    assert!(inline_break_policy_differs(&parent, &child));

    child = parent.clone();
    child.text_wrap_mode = css::TextWrapMode::NoWrap;
    assert!(inline_break_policy_differs(&parent, &child));

    child = parent.clone();
    child.text_wrap_style = css::TextWrapStyle::Balance;
    assert!(inline_break_policy_differs(&parent, &child));
}

#[test]
fn word_space_transform_difference_requires_inline_item_collection() {
    let parent = ComputedStyle::initial();
    let mut child = parent.clone();
    child.word_space_transform = css::WordSpaceTransform {
        replacement: Some(css::WordSpaceReplacement::Space),
        auto_phrase: false,
    };

    assert!(inline_style_affects_line(&parent, &child));
}

#[test]
fn tab_size_difference_requires_inline_item_collection() {
    let parent = ComputedStyle::initial();
    let mut child = parent.clone();
    child.tab_size = css::TabSize::Spaces(4.0);

    assert!(inline_style_affects_line(&parent, &child));
}
#[cfg(test)]
mod source_classification_tests {
    use super::*;

    fn test_parent_style() -> ComputedStyle {
        ComputedStyle {
            font_size: 12.0,
            line_height: 14.4,
            color: CssColor::BLACK,
            ..ComputedStyle::initial()
        }
    }

    fn first_element_by_tag<'a>(node: &'a Node, tag: &str) -> Option<&'a Element> {
        match &node.kind {
            NodeKind::Text(_) => None,
            NodeKind::Element(element) => {
                if element.tag == tag {
                    return Some(element);
                }
                element
                    .children
                    .iter()
                    .find_map(|child| first_element_by_tag(child, tag))
            }
        }
    }

    #[tokio::test]
    async fn structural_probes_ignore_generated_pseudo_variable_rules() {
        let stylesheet = css::parse_stylesheet(&css::Css::from_string(
            ".flow { display: block } \
             .run-in { display: run-in } \
             .row { display: table-row } \
             .inline { display: inline } \
             .ruby { display: ruby } \
             .probe::before { content: var(--missing, 'generated') }",
        ));
        let stylesheets = Stylesheets::for_document(
            css::html5_user_agent_stylesheet(),
            None,
            std::slice::from_ref(&stylesheet),
        );
        let parent_style = test_parent_style();
        let mut font_system = FontSystem::start_loading()
            .load_stylesheet_fonts(&stylesheets)
            .finish()
            .await;

        let direct_flow = dom::parse("<div><span class=\"probe flow\"></span></div>");
        assert!(has_direct_flow_child_with_font_metrics(
            first_element_by_tag(&direct_flow, "div").expect("expected direct-flow parent"),
            &parent_style,
            &stylesheets,
            &mut font_system,
        ));

        let run_in = dom::parse("<div><span class=\"probe run-in\"></span></div>");
        assert!(has_direct_run_in_child_with_font_metrics(
            first_element_by_tag(&run_in, "div").expect("expected run-in parent"),
            &parent_style,
            &stylesheets,
            &[],
            &mut font_system,
        ));

        let table = dom::parse("<div><span class=\"probe row\"></span></div>");
        assert!(has_unwrapped_table_internal_descendant_with_font_metrics(
            first_element_by_tag(&table, "div").expect("expected table parent"),
            &parent_style,
            &stylesheets,
            &[],
            &mut font_system,
        ));

        let block_in_inline = dom::parse(
            "<div><span class=\"probe inline\"><span class=\"probe flow\"></span></span></div>",
        );
        assert!(has_block_in_inline_split_boundary_with_font_metrics(
            first_element_by_tag(&block_in_inline, "div").expect("expected block-in-inline parent"),
            &parent_style,
            &stylesheets,
            &[],
            &mut font_system,
        ));

        let ruby = dom::parse("<div><span class=\"probe ruby\"></span></div>");
        assert!(has_ruby_formatting_descendant_with_font_metrics(
            first_element_by_tag(&ruby, "div").expect("expected ruby parent"),
            &parent_style,
            &stylesheets,
            &[],
            &mut font_system,
            &mut HashMap::new(),
        ));

        let later_block = dom::parse(
            "<div><span class=\"probe inline\"></span><span class=\"probe flow\"></span></div>",
        );
        let later_parent =
            first_element_by_tag(&later_block, "div").expect("expected later-block parent");
        let siblings = element_sibling_signature_list(later_parent);
        assert!(has_later_normal_block_flow_child_with_font_metrics(
            later_parent,
            1,
            &siblings,
            &parent_style,
            &stylesheets,
            &[],
            &mut font_system,
        ));
    }

    #[tokio::test]
    async fn direct_float_uses_ordinary_child_traversal_without_reclassifying_normal_flow() {
        let stylesheet = css::parse_stylesheet(&css::Css::from_string(
            ".float { display: inline-block; float: left } \
             .inline { display: inline-block }",
        ));
        let stylesheets = Stylesheets::for_document(
            css::html5_user_agent_stylesheet(),
            None,
            std::slice::from_ref(&stylesheet),
        );
        let parent_style = test_parent_style();
        let mut font_system = FontSystem::start_loading()
            .load_stylesheet_fonts(&stylesheets)
            .finish()
            .await;

        let floated = dom::parse("<div><span class=\"float\">float</span></div>");
        let floated_parent = first_element_by_tag(&floated, "div").expect("expected float parent");
        assert!(!has_ordered_mixed_flow_content_with_font_metrics(
            floated_parent,
            &parent_style,
            &stylesheets,
            &[],
            &mut font_system,
        ));
        assert!(!has_direct_flow_child_with_font_metrics(
            floated_parent,
            &parent_style,
            &stylesheets,
            &mut font_system,
        ));
        assert!(has_direct_float_only_source_with_font_metrics(
            floated_parent,
            &parent_style,
            &stylesheets,
            &mut font_system,
        ));

        let mixed = dom::parse("<div>prefix<span class=\"float\">float</span></div>");
        let mixed_parent = first_element_by_tag(&mixed, "div").expect("expected mixed parent");
        assert!(!has_ordered_mixed_flow_content_with_font_metrics(
            mixed_parent,
            &parent_style,
            &stylesheets,
            &[],
            &mut font_system,
        ));
        assert!(!has_direct_float_only_source_with_font_metrics(
            mixed_parent,
            &parent_style,
            &stylesheets,
            &mut font_system,
        ));

        let inline = dom::parse("<div><span class=\"inline\">inline</span></div>");
        let inline_parent = first_element_by_tag(&inline, "div").expect("expected inline parent");
        assert!(!has_ordered_mixed_flow_content_with_font_metrics(
            inline_parent,
            &parent_style,
            &stylesheets,
            &[],
            &mut font_system,
        ));
    }
}

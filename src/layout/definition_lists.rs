use super::*;

#[derive(Debug, Clone)]
pub(in crate::layout) struct DefinitionListColumnItem<'a> {
    pub(in crate::layout) element: &'a Element,
    pub(in crate::layout) signature: ElementSignature,
    pub(in crate::layout) style: ComputedStyle,
    pub(in crate::layout) children: Option<&'a [box_tree::FormattingBox<'a>]>,
}

pub(in crate::layout) fn definition_list_column_groups_with_font_metrics<'a>(
    element: &'a Element,
    parent_style: &ComputedStyle,
    stylesheets: &Stylesheets<'_>,
    ancestors: &[ElementSignature],
    font_system: &mut FontSystem,
) -> Vec<Vec<DefinitionListColumnItem<'a>>> {
    let mut resolver = DomStyleResolver::with_font_system(font_system);
    definition_list_column_groups_with_resolver(
        element,
        parent_style,
        stylesheets,
        ancestors,
        &mut resolver,
    )
}

pub(in crate::layout) fn definition_list_column_groups_with_resolver<'a>(
    element: &'a Element,
    parent_style: &ComputedStyle,
    stylesheets: &Stylesheets<'_>,
    ancestors: &[ElementSignature],
    resolver: &mut DomStyleResolver<'_>,
) -> Vec<Vec<DefinitionListColumnItem<'a>>> {
    let sibling_tags = element_sibling_signature_list(element);
    let mut element_index = 0usize;
    let mut groups: Vec<Vec<DefinitionListColumnItem<'a>>> = Vec::new();
    let mut current_group_has_description = false;

    for child in &element.children {
        let NodeKind::Element(child_element) = &child.kind else {
            continue;
        };
        let signature =
            ElementSignature::from_sibling_snapshot(element_index, sibling_tags.clone())
                .expect("source child must have a cached sibling signature");
        element_index += 1;
        let style = resolver.style_for_element(
            child_element,
            signature.clone(),
            stylesheets,
            Some(parent_style),
            ancestors,
        );
        let item = DefinitionListColumnItem {
            element: child_element,
            signature,
            style,
            children: None,
        };

        match definition_list_item_kind(child_element) {
            DefinitionListItemKind::Term => {
                if groups.is_empty() || current_group_has_description {
                    groups.push(Vec::new());
                    current_group_has_description = false;
                }
                groups.last_mut().expect("group exists").push(item);
            }
            DefinitionListItemKind::Description => {
                if groups.is_empty() {
                    groups.push(Vec::new());
                }
                current_group_has_description = true;
                groups.last_mut().expect("group exists").push(item);
            }
            DefinitionListItemKind::Other => {
                groups.push(vec![item]);
                current_group_has_description = true;
            }
        }
    }

    groups
}

pub(in crate::layout) fn definition_list_column_groups_from_boxes<'a>(
    child_boxes: &'a [box_tree::FormattingBox<'a>],
) -> Vec<Vec<DefinitionListColumnItem<'a>>> {
    let mut groups: Vec<Vec<DefinitionListColumnItem<'a>>> = Vec::new();
    let mut current_group_has_description = false;

    for child_box in child_boxes {
        let Some((element, signature, style, children)) = child_box.element_parts() else {
            continue;
        };
        let item = DefinitionListColumnItem {
            element,
            signature: signature.clone(),
            style: style.clone(),
            children: Some(children),
        };

        match definition_list_item_kind(element) {
            DefinitionListItemKind::Term => {
                if groups.is_empty() || current_group_has_description {
                    groups.push(Vec::new());
                    current_group_has_description = false;
                }
                groups.last_mut().expect("group exists").push(item);
            }
            DefinitionListItemKind::Description => {
                if groups.is_empty() {
                    groups.push(Vec::new());
                }
                current_group_has_description = true;
                groups.last_mut().expect("group exists").push(item);
            }
            DefinitionListItemKind::Other => {
                groups.push(vec![item]);
                current_group_has_description = true;
            }
        }
    }

    groups
}

#[cfg(test)]
mod tests {
    use super::*;

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
    async fn definition_list_dom_grouping_uses_measured_parent_ch_for_child_font_size() {
        let root =
            dom::parse("<html><body><dl><dt style=\"font-size: 2ch\">Term</dt></dl></body></html>");
        let dl = first_element_by_tag(&root, "dl").expect("expected dl element");
        let stylesheet = css::parse_stylesheet(
            &css::Css::from_string(
                r#"@font-face {
                    font-family: MetricProbe;
                    src: url("tests/resources/fonts/noto-sans-v8-latin-regular.woff");
                }"#,
            )
            .with_base_path(".")
            .expect("current directory should be a valid file URL"),
        );
        let stylesheets = Stylesheets::for_document(
            css::html5_user_agent_stylesheet(),
            None,
            std::slice::from_ref(&stylesheet),
        );
        let mut parent_style = ComputedStyle {
            font_family: css::FontFamily::Names(vec!["MetricProbe".to_string()]),
            font_size: 40.0,
            line_height: 40.0,
            ..ComputedStyle::initial()
        };
        parent_style.line_height_value = css::ComputedLineHeight::from_points(40.0);
        let mut font_system = FontSystem::start_loading()
            .load_stylesheet_fonts(&stylesheets)
            .finish()
            .await;
        let parent_ch_advance = font_system.ch_advance(&parent_style);
        assert!(
            (parent_ch_advance.points() - parent_style.font_size * 0.5).abs() > 0.01,
            "fixture must differ from the generic 0.5em ch fallback"
        );

        let groups = definition_list_column_groups_with_font_metrics(
            dl,
            &parent_style,
            &stylesheets,
            &[],
            &mut font_system,
        );

        assert_eq!(groups.len(), 1);
        assert!((groups[0][0].style.font_size - parent_ch_advance.points() * 2.0).abs() < 0.01);
    }
}

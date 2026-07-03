use super::*;

pub(in crate::css) fn split_pseudo_element_rule(
    selector_text: &str,
    selector_parser: &ReasySelectorParser,
    declarations: &Declarations,
    specificity: u32,
    layer_name: Option<String>,
    scopes: Vec<ScopeRule>,
) -> Vec<ParsedCssRule> {
    // CSS Pseudo-Elements 4 pseudo rules are matched against their originating
    // elements, then applied in pseudo-specific cascade/layout paths.
    // <https://www.w3.org/TR/css-pseudo-4/#pseudo-elements>
    let pseudo_names = [
        (RoutedPseudoElement::Marker, "marker"),
        (RoutedPseudoElement::Before, "before"),
        (RoutedPseudoElement::After, "after"),
        (RoutedPseudoElement::FirstLine, "first-line"),
        (RoutedPseudoElement::FirstLetter, "first-letter"),
    ];
    let mut normal_selectors = Vec::new();
    let mut routed_selectors = Vec::new();
    for selector in split_selector_list(selector_text) {
        let mut routed = false;
        for (pseudo, name) in pseudo_names {
            if let Some(base) = strip_pseudo_selector(selector, name) {
                routed_selectors.push((pseudo, base.to_string()));
                routed = true;
                break;
            }
        }
        if !routed {
            normal_selectors.push(selector.trim().to_string());
        }
    }
    if routed_selectors.is_empty() {
        return Vec::new();
    }

    let mut rules = Vec::new();
    if !normal_selectors.is_empty() {
        let selector_text = normal_selectors.join(", ");
        if let Some(selector) = parse_selector_list_text(&selector_text, selector_parser) {
            rules.push(ParsedCssRule::Style(StyleRule {
                selector_text,
                selector,
                declarations: declarations.clone(),
                specificity,
                order: 0,
                layer_name: layer_name.clone(),
                scopes: scopes.clone(),
            }));
        }
    }
    for (pseudo, _name) in pseudo_names {
        let base_selectors = routed_selectors
            .iter()
            .filter_map(|(routed_pseudo, selector)| (*routed_pseudo == pseudo).then_some(selector))
            .cloned()
            .collect::<Vec<_>>();
        if base_selectors.is_empty() {
            continue;
        }
        let selector_text = base_selectors.join(", ");
        let Some(selector) = parse_selector_list_text(&selector_text, selector_parser) else {
            continue;
        };
        let rule = StyleRule {
            selector_text,
            selector,
            declarations: declarations.clone(),
            specificity,
            order: 0,
            layer_name: layer_name.clone(),
            scopes: scopes.clone(),
        };
        rules.push(match pseudo {
            RoutedPseudoElement::Marker => ParsedCssRule::Marker(rule),
            RoutedPseudoElement::Before => ParsedCssRule::Before(rule),
            RoutedPseudoElement::After => ParsedCssRule::After(rule),
            RoutedPseudoElement::FirstLine => ParsedCssRule::FirstLine(rule),
            RoutedPseudoElement::FirstLetter => ParsedCssRule::FirstLetter(rule),
        });
    }
    rules
}

pub(in crate::css) fn parse_selector_list_text(
    selector_text: &str,
    selector_parser: &ReasySelectorParser,
) -> Option<SelectorList<ReasySelectorImpl>> {
    let mut input = ParserInput::new(selector_text);
    let mut parser = Parser::new(&mut input);
    SelectorList::parse(selector_parser, &mut parser, ParseRelative::No).ok()
}

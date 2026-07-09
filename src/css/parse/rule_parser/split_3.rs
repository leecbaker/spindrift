use super::*;

/// Parses the contents of a CSS `@keyframes` rule.
///
/// Keyframe selectors accept `from`, `to`, and percentages. A selector list
/// creates one step for each valid offset, while invalid selectors are simply
/// ignored as required by CSS Animations error handling:
/// <https://www.w3.org/TR/css-animations-1/#keyframes>
pub(in crate::css) fn parse_keyframes_rule(name: &str, body: &str) -> Option<KeyframesRule> {
    let name = name.trim();
    if name.is_empty() || name.contains(char::is_whitespace) {
        return None;
    }
    let mut steps = Vec::new();
    let mut rest = body;
    while let Some(open) = rest.find('{') {
        let selectors = rest[..open].trim();
        let Some(close) = find_matching_brace(rest, open) else {
            break;
        };
        let declarations = parse_declarations(&rest[open + 1..close]);
        for selector in selectors.split(',') {
            let offset = match selector.trim().to_ascii_lowercase().as_str() {
                "from" => Some(0.0),
                "to" => Some(1.0),
                percentage => percentage
                    .strip_suffix('%')
                    .and_then(|value| value.trim().parse::<f32>().ok())
                    .map(|value| value / 100.0)
                    .filter(|value| (0.0..=1.0).contains(value)),
            };
            if let Some(offset) = offset {
                steps.push(KeyframeStep {
                    offset,
                    declarations: declarations.clone(),
                });
            }
        }
        rest = &rest[close + 1..];
    }
    (!steps.is_empty()).then_some(KeyframesRule {
        name: name.to_string(),
        steps,
    })
}

pub(in crate::css) fn split_pseudo_element_rule(
    selector_text: &str,
    selector_parser: &QuireSelectorParser,
    declarations: &Declarations,
    specificity: u32,
    layer_name: Option<String>,
    scopes: Vec<ScopeRule>,
) -> Vec<ParsedCssRule> {
    // CSS Pseudo-Elements 4 pseudo rules are matched against their originating
    // elements, then applied in pseudo-specific cascade/layout paths.
    // <https://www.w3.org/TR/css-pseudo-4/#pseudo-elements>
    let pseudo_names = [
        (RoutedPseudoElement::BeforeMarker, "before::marker"),
        (RoutedPseudoElement::AfterMarker, "after::marker"),
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
            RoutedPseudoElement::BeforeMarker => ParsedCssRule::BeforeMarker(rule),
            RoutedPseudoElement::AfterMarker => ParsedCssRule::AfterMarker(rule),
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
    selector_parser: &QuireSelectorParser,
) -> Option<SelectorList<QuireSelectorImpl>> {
    let mut input = ParserInput::new(selector_text);
    let mut parser = Parser::new(&mut input);
    SelectorList::parse(selector_parser, &mut parser, ParseRelative::No).ok()
}

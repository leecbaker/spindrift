use super::*;
use crate::css::{FontPaletteDefinition, PropertyRegistrationRule};

#[allow(clippy::too_many_arguments)]
pub(super) fn flatten_rule(
    rule: ParsedCssRule,
    rules: &mut Vec<StyleRule>,
    container_rules: &mut Vec<ContainerRule>,
    marker_rules: &mut Vec<StyleRule>,
    before_marker_rules: &mut Vec<StyleRule>,
    after_marker_rules: &mut Vec<StyleRule>,
    before_rules: &mut Vec<StyleRule>,
    after_rules: &mut Vec<StyleRule>,
    footnote_call_rules: &mut Vec<StyleRule>,
    footnote_marker_rules: &mut Vec<StyleRule>,
    first_line_rules: &mut Vec<StyleRule>,
    first_letter_rules: &mut Vec<StyleRule>,
    keyframes: &mut Vec<KeyframesRule>,
    font_faces: &mut Vec<CssFontFace>,
    counter_styles: &mut Vec<CounterStyleRule>,
    font_feature_values: &mut Vec<FontFeatureValuesRule>,
    font_palette_values: &mut Vec<(String, FontPaletteDefinition)>,
    property_registrations: &mut Vec<PropertyRegistrationRule>,
    page_rules: &mut Vec<ParsedPageRule>,
) {
    match rule {
        ParsedCssRule::Style(rule) => rules.push(rule),
        ParsedCssRule::Container(rule) => container_rules.push(rule),
        ParsedCssRule::Marker(rule) => marker_rules.push(rule),
        ParsedCssRule::BeforeMarker(rule) => before_marker_rules.push(rule),
        ParsedCssRule::AfterMarker(rule) => after_marker_rules.push(rule),
        ParsedCssRule::Before(rule) => before_rules.push(rule),
        ParsedCssRule::After(rule) => after_rules.push(rule),
        ParsedCssRule::FootnoteCall(rule) => footnote_call_rules.push(rule),
        ParsedCssRule::FootnoteMarker(rule) => footnote_marker_rules.push(rule),
        ParsedCssRule::FirstLine(rule) => first_line_rules.push(rule),
        ParsedCssRule::FirstLetter(rule) => first_letter_rules.push(rule),
        ParsedCssRule::Keyframes(rule) => keyframes.push(rule),
        ParsedCssRule::FontFace(rule) => font_faces.push(rule),
        ParsedCssRule::CounterStyle(rule) => counter_styles.push(rule),
        ParsedCssRule::FontFeatureValues(rule) => font_feature_values.push(rule),
        ParsedCssRule::FontPaletteValues(name, definition) => {
            font_palette_values.push((name, definition));
        }
        ParsedCssRule::Property(rule) => property_registrations.push(rule),
        ParsedCssRule::Page(rule) => page_rules.push(rule),
        ParsedCssRule::Nested(nested) => {
            for rule in nested {
                flatten_rule(
                    rule,
                    rules,
                    container_rules,
                    marker_rules,
                    before_marker_rules,
                    after_marker_rules,
                    before_rules,
                    after_rules,
                    footnote_call_rules,
                    footnote_marker_rules,
                    first_line_rules,
                    first_letter_rules,
                    keyframes,
                    font_faces,
                    counter_styles,
                    font_feature_values,
                    font_palette_values,
                    property_registrations,
                    page_rules,
                );
            }
        }
        ParsedCssRule::Ignored => {}
    }
}

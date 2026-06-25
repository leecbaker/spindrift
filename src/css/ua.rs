use super::{Css, Stylesheet, StylesheetOrigin, parse_stylesheet};
use std::sync::OnceLock;

const HTML5_UA_CSS: &str = include_str!("ua/html5_ua.css");
const HTML5_PH_CSS: &str = include_str!("ua/html5_ph.css");

pub(crate) fn html5_user_agent_stylesheet() -> Stylesheet {
    static STYLESHEET: OnceLock<Stylesheet> = OnceLock::new();
    STYLESHEET
        .get_or_init(|| {
            // HTML rendering defines a suggested user-agent stylesheet for
            // embedded content; WeasyPrint extends that sheet for paged media.
            // Keep this parsed as a real UA-origin stylesheet so author rules
            // cascade over it instead of mirroring tag defaults in layout code.
            // https://html.spec.whatwg.org/multipage/rendering.html
            let mut stylesheet = parse_stylesheet(&Css::from_string(HTML5_UA_CSS));
            stylesheet.origin = StylesheetOrigin::UserAgent;
            stylesheet
        })
        .clone()
}

pub(crate) fn html5_presentational_hints_stylesheet() -> Stylesheet {
    static STYLESHEET: OnceLock<Stylesheet> = OnceLock::new();
    STYLESHEET
        .get_or_init(|| {
            // HTML presentational hints are optional in WeasyPrint and map to
            // author-origin declarations with zero specificity. Keeping them
            // as a stylesheet lets the normal Cascade 5 sort handle author CSS
            // overrides instead of adding attribute checks to layout code.
            // https://html.spec.whatwg.org/multipage/rendering.html#presentational-hints
            // https://www.w3.org/TR/css-cascade-5/#cascade-sort
            parse_stylesheet(
                &Css::from_string(HTML5_PH_CSS)
                    .with_author_origin()
                    .with_specificity_override(0),
            )
        })
        .clone()
}

#[cfg(test)]
pub(crate) fn html5_user_agent_source() -> &'static str {
    HTML5_UA_CSS
}

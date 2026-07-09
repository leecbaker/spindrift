use super::{Css, Stylesheet, StylesheetOrigin, parse_stylesheet};
use std::sync::OnceLock;

const HTML5_UA_CSS: &str = include_str!("ua/html5_ua.css");
const HTML5_PH_CSS: &str = include_str!("ua/html5_ph.css");
const HTML_DOCUMENT_IMPORTANT_UA_CSS: &str = "\
/* HTML head metadata cannot be made into rendered table content by author CSS.\n+   https://html.spec.whatwg.org/multipage/rendering.html#hidden-elements */\n+head { display: none !important }\n";

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

/// HTML-specific UA-important rendering rules.
///
/// These rules are deliberately separate from the general HTML UA stylesheet:
/// XML/XHTML inputs retain the ordinary UA cascade, while an HTML `head`
/// remains non-rendered even when author CSS attempts to assign a table role.
pub(crate) fn html_document_important_user_agent_stylesheet() -> Stylesheet {
    static STYLESHEET: OnceLock<Stylesheet> = OnceLock::new();
    STYLESHEET
        .get_or_init(|| {
            let mut stylesheet =
                parse_stylesheet(&Css::from_string(HTML_DOCUMENT_IMPORTANT_UA_CSS));
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
            let mut stylesheet = parse_stylesheet(
                &Css::from_string(HTML5_PH_CSS)
                    .with_author_origin()
                    .with_specificity_override(0),
            );
            stylesheet.html_presentational_hints = true;
            stylesheet
        })
        .clone()
}

#[cfg(test)]
pub(crate) fn html5_user_agent_source() -> &'static str {
    HTML5_UA_CSS
}

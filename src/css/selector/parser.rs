use std::collections::HashMap;

use cssparser::{CowRcStr, Parser as CssParser, SourceLocation};
use selectors::parser::{Parser as SelectorParser, SelectorParseErrorKind};

use super::{CssAtom, LanguageRange, QuirePseudoClass, QuirePseudoElement, QuireSelectorImpl};
use crate::css::types::Direction;

#[derive(Debug, Clone, Default)]
pub(in crate::css) struct QuireSelectorParser {
    pub(in crate::css) default_namespace: Option<CssAtom>,
    pub(in crate::css) namespaces: HashMap<String, CssAtom>,
    allow_parent_selector: bool,
}

impl QuireSelectorParser {
    pub(in crate::css) fn new(
        default_namespace: Option<String>,
        namespaces: HashMap<String, String>,
    ) -> Self {
        Self {
            default_namespace: default_namespace.map(CssAtom),
            namespaces: namespaces
                .into_iter()
                .map(|(prefix, namespace)| (prefix, CssAtom(namespace)))
                .collect(),
            allow_parent_selector: false,
        }
    }

    pub(in crate::css) fn with_parent_selector(mut self) -> Self {
        self.allow_parent_selector = true;
        self
    }
}

impl<'i> SelectorParser<'i> for QuireSelectorParser {
    type Impl = QuireSelectorImpl;
    type Error = SelectorParseErrorKind<'i>;

    fn parse_is_and_where(&self) -> bool {
        true
    }

    /// Parse Selectors 4 child-indexed pseudo-class filters:
    /// `:nth-child(An+B of S)` and `:nth-last-child(An+B of S)`.
    ///
    /// <https://www.w3.org/TR/selectors-4/#child-index>
    fn parse_nth_child_of(&self) -> bool {
        true
    }

    fn parse_has(&self) -> bool {
        true
    }

    fn parse_parent_selector(&self) -> bool {
        self.allow_parent_selector
    }

    fn default_namespace(&self) -> Option<CssAtom> {
        self.default_namespace.clone()
    }

    fn namespace_for_prefix(&self, prefix: &CssAtom) -> Option<CssAtom> {
        self.namespaces.get(prefix.as_str()).cloned()
    }

    fn parse_non_ts_pseudo_class(
        &self,
        location: SourceLocation,
        name: CowRcStr<'i>,
    ) -> Result<QuirePseudoClass, cssparser::ParseError<'i, Self::Error>> {
        match name.as_ref().to_ascii_lowercase().as_str() {
            "link" => Ok(QuirePseudoClass::Link),
            "any-link" => Ok(QuirePseudoClass::AnyLink),
            // Static documents derive their private visited state from a
            // deterministic self-link check during DOM preparation.
            "visited" => Ok(QuirePseudoClass::Visited),
            "target" => Ok(QuirePseudoClass::Target),
            "target-within" => Ok(QuirePseudoClass::TargetWithin),
            "target-current" => Ok(QuirePseudoClass::TargetCurrent),
            "target-before" => Ok(QuirePseudoClass::TargetBefore),
            "target-after" => Ok(QuirePseudoClass::TargetAfter),
            "hover" => Ok(QuirePseudoClass::StaticFalse("hover")),
            "active" => Ok(QuirePseudoClass::StaticFalse("active")),
            "focus" => Ok(QuirePseudoClass::StaticFalse("focus")),
            "focus-visible" => Ok(QuirePseudoClass::StaticFalse("focus-visible")),
            "focus-within" => Ok(QuirePseudoClass::StaticFalse("focus-within")),
            "playing" => Ok(QuirePseudoClass::StaticFalse("playing")),
            "paused" => Ok(QuirePseudoClass::StaticFalse("paused")),
            "seeking" => Ok(QuirePseudoClass::StaticFalse("seeking")),
            "buffering" => Ok(QuirePseudoClass::StaticFalse("buffering")),
            "stalled" => Ok(QuirePseudoClass::StaticFalse("stalled")),
            "muted" => Ok(QuirePseudoClass::StaticFalse("muted")),
            "volume-locked" => Ok(QuirePseudoClass::StaticFalse("volume-locked")),
            "open" => Ok(QuirePseudoClass::Open),
            "popover-open" => Ok(QuirePseudoClass::StaticFalse("popover-open")),
            "modal" => Ok(QuirePseudoClass::StaticFalse("modal")),
            "fullscreen" => Ok(QuirePseudoClass::StaticFalse("fullscreen")),
            "picture-in-picture" => Ok(QuirePseudoClass::StaticFalse("picture-in-picture")),
            "autofill" => Ok(QuirePseudoClass::StaticFalse("autofill")),
            "default" => Ok(QuirePseudoClass::Default),
            "unchecked" => Ok(QuirePseudoClass::Unchecked),
            "placeholder-shown" => Ok(QuirePseudoClass::PlaceholderShown),
            "valid" => Ok(QuirePseudoClass::Valid),
            "invalid" => Ok(QuirePseudoClass::Invalid),
            "in-range" => Ok(QuirePseudoClass::InRange),
            "out-of-range" => Ok(QuirePseudoClass::OutOfRange),
            "user-valid" => Ok(QuirePseudoClass::StaticFalse("user-valid")),
            "user-invalid" => Ok(QuirePseudoClass::StaticFalse("user-invalid")),
            "defined" => Ok(QuirePseudoClass::Defined),
            "enabled" => Ok(QuirePseudoClass::Enabled),
            "disabled" => Ok(QuirePseudoClass::Disabled),
            "checked" => Ok(QuirePseudoClass::Checked),
            "indeterminate" => Ok(QuirePseudoClass::Indeterminate),
            "required" => Ok(QuirePseudoClass::Required),
            "optional" => Ok(QuirePseudoClass::Optional),
            "read-write" => Ok(QuirePseudoClass::ReadWrite),
            "read-only" => Ok(QuirePseudoClass::ReadOnly),
            _ => Err(location.new_custom_error(
                SelectorParseErrorKind::UnsupportedPseudoClassOrElement(name),
            )),
        }
    }

    fn parse_non_ts_functional_pseudo_class<'t>(
        &self,
        name: CowRcStr<'i>,
        parser: &mut CssParser<'i, 't>,
        _after_part: bool,
    ) -> Result<QuirePseudoClass, cssparser::ParseError<'i, Self::Error>> {
        if name.eq_ignore_ascii_case("lang") {
            let ranges = parser.parse_comma_separated(|parser| {
                let argument = if let Ok(argument) = parser.try_parse(|parser| {
                    parser
                        .expect_ident_or_string()
                        .map(|value| value.as_ref().to_string())
                }) {
                    argument
                } else {
                    parser.expect_delim('*')?;
                    "*".to_string()
                };
                LanguageRange::parse(&argument).ok_or_else(|| {
                    parser.new_custom_error(
                        SelectorParseErrorKind::UnsupportedPseudoClassOrElement(name.clone()),
                    )
                })
            })?;
            parser.expect_exhausted()?;
            if ranges.is_empty() {
                return Err(parser.new_custom_error(
                    SelectorParseErrorKind::UnsupportedPseudoClassOrElement(name),
                ));
            }
            return Ok(QuirePseudoClass::Lang(ranges));
        }
        if !name.eq_ignore_ascii_case("dir") {
            return Err(parser.new_custom_error(
                SelectorParseErrorKind::UnsupportedPseudoClassOrElement(name),
            ));
        }
        let direction = {
            let argument = parser.expect_ident()?;
            match argument.as_ref().to_ascii_lowercase().as_str() {
                "ltr" => Direction::Ltr,
                "rtl" => Direction::Rtl,
                _ => {
                    return Err(parser.new_custom_error(
                        SelectorParseErrorKind::UnsupportedPseudoClassOrElement(name),
                    ));
                }
            }
        };
        parser.expect_exhausted()?;
        Ok(QuirePseudoClass::Dir(direction))
    }

    fn parse_pseudo_element(
        &self,
        location: SourceLocation,
        name: CowRcStr<'i>,
    ) -> Result<QuirePseudoElement, cssparser::ParseError<'i, Self::Error>> {
        if name.eq_ignore_ascii_case("before") {
            Ok(QuirePseudoElement::Before)
        } else if name.eq_ignore_ascii_case("after") {
            Ok(QuirePseudoElement::After)
        } else if name.eq_ignore_ascii_case("scroll-marker") {
            Ok(QuirePseudoElement::ScrollMarker)
        } else if name.eq_ignore_ascii_case("scroll-marker-group") {
            Ok(QuirePseudoElement::ScrollMarkerGroup)
        } else if name.eq_ignore_ascii_case("footnote-call") {
            Ok(QuirePseudoElement::FootnoteCall)
        } else if name.eq_ignore_ascii_case("footnote-marker") {
            Ok(QuirePseudoElement::FootnoteMarker)
        } else if name.eq_ignore_ascii_case("marker") {
            Ok(QuirePseudoElement::Marker)
        } else if name.eq_ignore_ascii_case("first-line") {
            Ok(QuirePseudoElement::FirstLine)
        } else if name.eq_ignore_ascii_case("first-letter") {
            Ok(QuirePseudoElement::FirstLetter)
        } else {
            Err(
                location.new_custom_error(SelectorParseErrorKind::UnsupportedPseudoClassOrElement(
                    name,
                )),
            )
        }
    }
}

use std::collections::HashMap;

use cssparser::{CowRcStr, Parser as CssParser, SourceLocation};
use selectors::parser::{Parser as SelectorParser, SelectorParseErrorKind};

use super::{
    CssAtom, LanguageRange, SpindriftPseudoClass, SpindriftPseudoElement, SpindriftSelectorImpl,
};
use crate::css::types::Direction;

#[derive(Debug, Clone, Default)]
pub(in crate::css) struct SpindriftSelectorParser {
    pub(in crate::css) default_namespace: Option<CssAtom>,
    pub(in crate::css) namespaces: HashMap<String, CssAtom>,
    allow_parent_selector: bool,
}

impl SpindriftSelectorParser {
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

impl<'i> SelectorParser<'i> for SpindriftSelectorParser {
    type Impl = SpindriftSelectorImpl;
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
    ) -> Result<SpindriftPseudoClass, cssparser::ParseError<'i, Self::Error>> {
        match name.as_ref().to_ascii_lowercase().as_str() {
            "link" => Ok(SpindriftPseudoClass::Link),
            "any-link" => Ok(SpindriftPseudoClass::AnyLink),
            // Static documents derive their private visited state from a
            // deterministic self-link check during DOM preparation.
            "visited" => Ok(SpindriftPseudoClass::Visited),
            "target" => Ok(SpindriftPseudoClass::Target),
            "target-within" => Ok(SpindriftPseudoClass::TargetWithin),
            "target-current" => Ok(SpindriftPseudoClass::TargetCurrent),
            "target-before" => Ok(SpindriftPseudoClass::TargetBefore),
            "target-after" => Ok(SpindriftPseudoClass::TargetAfter),
            "hover" => Ok(SpindriftPseudoClass::StaticFalse("hover")),
            "active" => Ok(SpindriftPseudoClass::StaticFalse("active")),
            "focus" => Ok(SpindriftPseudoClass::StaticFalse("focus")),
            "focus-visible" => Ok(SpindriftPseudoClass::StaticFalse("focus-visible")),
            "focus-within" => Ok(SpindriftPseudoClass::StaticFalse("focus-within")),
            "playing" => Ok(SpindriftPseudoClass::StaticFalse("playing")),
            "paused" => Ok(SpindriftPseudoClass::StaticFalse("paused")),
            "seeking" => Ok(SpindriftPseudoClass::StaticFalse("seeking")),
            "buffering" => Ok(SpindriftPseudoClass::StaticFalse("buffering")),
            "stalled" => Ok(SpindriftPseudoClass::StaticFalse("stalled")),
            "muted" => Ok(SpindriftPseudoClass::StaticFalse("muted")),
            "volume-locked" => Ok(SpindriftPseudoClass::StaticFalse("volume-locked")),
            "open" => Ok(SpindriftPseudoClass::Open),
            "popover-open" => Ok(SpindriftPseudoClass::StaticFalse("popover-open")),
            "modal" => Ok(SpindriftPseudoClass::StaticFalse("modal")),
            "fullscreen" => Ok(SpindriftPseudoClass::StaticFalse("fullscreen")),
            "picture-in-picture" => Ok(SpindriftPseudoClass::StaticFalse("picture-in-picture")),
            "autofill" => Ok(SpindriftPseudoClass::StaticFalse("autofill")),
            "default" => Ok(SpindriftPseudoClass::Default),
            "unchecked" => Ok(SpindriftPseudoClass::Unchecked),
            "placeholder-shown" => Ok(SpindriftPseudoClass::PlaceholderShown),
            "valid" => Ok(SpindriftPseudoClass::Valid),
            "invalid" => Ok(SpindriftPseudoClass::Invalid),
            "in-range" => Ok(SpindriftPseudoClass::InRange),
            "out-of-range" => Ok(SpindriftPseudoClass::OutOfRange),
            "user-valid" => Ok(SpindriftPseudoClass::StaticFalse("user-valid")),
            "user-invalid" => Ok(SpindriftPseudoClass::StaticFalse("user-invalid")),
            "defined" => Ok(SpindriftPseudoClass::Defined),
            "enabled" => Ok(SpindriftPseudoClass::Enabled),
            "disabled" => Ok(SpindriftPseudoClass::Disabled),
            "checked" => Ok(SpindriftPseudoClass::Checked),
            "indeterminate" => Ok(SpindriftPseudoClass::Indeterminate),
            "required" => Ok(SpindriftPseudoClass::Required),
            "optional" => Ok(SpindriftPseudoClass::Optional),
            "read-write" => Ok(SpindriftPseudoClass::ReadWrite),
            "read-only" => Ok(SpindriftPseudoClass::ReadOnly),
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
    ) -> Result<SpindriftPseudoClass, cssparser::ParseError<'i, Self::Error>> {
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
            return Ok(SpindriftPseudoClass::Lang(ranges));
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
        Ok(SpindriftPseudoClass::Dir(direction))
    }

    fn parse_pseudo_element(
        &self,
        location: SourceLocation,
        name: CowRcStr<'i>,
    ) -> Result<SpindriftPseudoElement, cssparser::ParseError<'i, Self::Error>> {
        if name.eq_ignore_ascii_case("before") {
            Ok(SpindriftPseudoElement::Before)
        } else if name.eq_ignore_ascii_case("after") {
            Ok(SpindriftPseudoElement::After)
        } else if name.eq_ignore_ascii_case("scroll-marker") {
            Ok(SpindriftPseudoElement::ScrollMarker)
        } else if name.eq_ignore_ascii_case("scroll-marker-group") {
            Ok(SpindriftPseudoElement::ScrollMarkerGroup)
        } else if name.eq_ignore_ascii_case("footnote-call") {
            Ok(SpindriftPseudoElement::FootnoteCall)
        } else if name.eq_ignore_ascii_case("footnote-marker") {
            Ok(SpindriftPseudoElement::FootnoteMarker)
        } else if name.eq_ignore_ascii_case("marker") {
            Ok(SpindriftPseudoElement::Marker)
        } else if name.eq_ignore_ascii_case("first-line") {
            Ok(SpindriftPseudoElement::FirstLine)
        } else if name.eq_ignore_ascii_case("first-letter") {
            Ok(SpindriftPseudoElement::FirstLetter)
        } else {
            Err(
                location.new_custom_error(SelectorParseErrorKind::UnsupportedPseudoClassOrElement(
                    name,
                )),
            )
        }
    }
}

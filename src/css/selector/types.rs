use std::collections::hash_map::DefaultHasher;
use std::fmt;
use std::fmt::Write as _;
use std::hash::{Hash, Hasher};

use cssparser::{ToCss, serialize_identifier};
use precomputed_hash::PrecomputedHash;
use selectors::parser::{NonTSPseudoClass, PseudoElement, SelectorImpl};

use super::LanguageRange;
use crate::css::types::Direction;

impl NonTSPseudoClass for QuirePseudoClass {
    type Impl = QuireSelectorImpl;

    fn is_active_or_hover(&self) -> bool {
        false
    }

    fn is_user_action_state(&self) -> bool {
        false
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum QuirePseudoElement {
    Before,
    After,
    FootnoteCall,
    FootnoteMarker,
    Marker,
    FirstLine,
    FirstLetter,
}

impl ToCss for QuirePseudoElement {
    fn to_css<W>(&self, dest: &mut W) -> fmt::Result
    where
        W: fmt::Write,
    {
        dest.write_str(match self {
            QuirePseudoElement::Before => "::before",
            QuirePseudoElement::After => "::after",
            QuirePseudoElement::FootnoteCall => "::footnote-call",
            QuirePseudoElement::FootnoteMarker => "::footnote-marker",
            QuirePseudoElement::Marker => "::marker",
            QuirePseudoElement::FirstLine => "::first-line",
            QuirePseudoElement::FirstLetter => "::first-letter",
        })
    }
}

impl PseudoElement for QuirePseudoElement {
    type Impl = QuireSelectorImpl;
}

#[derive(Clone, Debug, Default, Eq, Hash, PartialEq)]
pub(crate) struct CssAtom(pub(in crate::css) String);

impl CssAtom {
    pub(in crate::css) fn as_str(&self) -> &str {
        &self.0
    }
}

impl From<&str> for CssAtom {
    fn from(value: &str) -> Self {
        Self(value.to_string())
    }
}

impl ToCss for CssAtom {
    fn to_css<W>(&self, dest: &mut W) -> fmt::Result
    where
        W: fmt::Write,
    {
        serialize_identifier(&self.0, dest)
    }
}

impl PrecomputedHash for CssAtom {
    fn precomputed_hash(&self) -> u32 {
        let mut hasher = DefaultHasher::new();
        self.0.hash(&mut hasher);
        hasher.finish() as u32
    }
}

#[derive(Clone, Debug, Default, Eq, Hash, PartialEq)]
pub(crate) struct CssString(String);

impl AsRef<str> for CssString {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl From<&str> for CssString {
    fn from(value: &str) -> Self {
        Self(value.to_string())
    }
}

impl ToCss for CssString {
    fn to_css<W>(&self, dest: &mut W) -> fmt::Result
    where
        W: fmt::Write,
    {
        dest.write_char('"')?;
        write!(cssparser::CssStringWriter::new(dest), "{}", self.0)?;
        dest.write_char('"')
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct QuireSelectorImpl;

impl SelectorImpl for QuireSelectorImpl {
    type ExtraMatchingData<'a> = ();
    type AttrValue = CssString;
    type Identifier = CssAtom;
    type LocalName = CssAtom;
    type NamespaceUrl = CssAtom;
    type NamespacePrefix = CssAtom;
    type BorrowedLocalName = CssAtom;
    type BorrowedNamespaceUrl = CssAtom;
    type NonTSPseudoClass = QuirePseudoClass;
    type PseudoElement = QuirePseudoElement;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum QuirePseudoClass {
    Link,
    AnyLink,
    Visited,
    Dir(Direction),
    Lang(Vec<LanguageRange>),
    StaticFalse(&'static str),
    Target,
    TargetWithin,
    Open,
    Defined,
    Enabled,
    Disabled,
    Checked,
    Indeterminate,
    Default,
    Unchecked,
    PlaceholderShown,
    Valid,
    Invalid,
    InRange,
    OutOfRange,
    Required,
    Optional,
    ReadWrite,
    ReadOnly,
}

impl ToCss for QuirePseudoClass {
    fn to_css<W>(&self, dest: &mut W) -> fmt::Result
    where
        W: fmt::Write,
    {
        match self {
            Self::Link => dest.write_str(":link"),
            Self::AnyLink => dest.write_str(":any-link"),
            Self::Visited => dest.write_str(":visited"),
            Self::Dir(Direction::Ltr) => dest.write_str(":dir(ltr)"),
            Self::Dir(Direction::Rtl) => dest.write_str(":dir(rtl)"),
            Self::StaticFalse(name) => write!(dest, ":{name}"),
            Self::Target => dest.write_str(":target"),
            Self::TargetWithin => dest.write_str(":target-within"),
            Self::Open => dest.write_str(":open"),
            Self::Defined => dest.write_str(":defined"),
            Self::Enabled => dest.write_str(":enabled"),
            Self::Disabled => dest.write_str(":disabled"),
            Self::Checked => dest.write_str(":checked"),
            Self::Indeterminate => dest.write_str(":indeterminate"),
            Self::Default => dest.write_str(":default"),
            Self::Unchecked => dest.write_str(":unchecked"),
            Self::PlaceholderShown => dest.write_str(":placeholder-shown"),
            Self::Valid => dest.write_str(":valid"),
            Self::Invalid => dest.write_str(":invalid"),
            Self::InRange => dest.write_str(":in-range"),
            Self::OutOfRange => dest.write_str(":out-of-range"),
            Self::Required => dest.write_str(":required"),
            Self::Optional => dest.write_str(":optional"),
            Self::ReadWrite => dest.write_str(":read-write"),
            Self::ReadOnly => dest.write_str(":read-only"),
            Self::Lang(ranges) => {
                dest.write_str(":lang(")?;
                for (index, range) in ranges.iter().enumerate() {
                    if index > 0 {
                        dest.write_str(", ")?;
                    }
                    range.to_css(dest)?;
                }
                dest.write_char(')')
            }
        }
    }
}

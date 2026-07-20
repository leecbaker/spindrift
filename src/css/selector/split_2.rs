use super::*;

impl LanguageRange {
    pub(in crate::css) fn parse(value: &str) -> Option<Self> {
        let value = value.trim();
        if value.is_empty() || is_valid_extended_language_range(value) {
            Some(Self(value.to_ascii_lowercase()))
        } else {
            None
        }
    }

    pub(in crate::css) fn as_str(&self) -> &str {
        &self.0
    }
}

impl ToCss for LanguageRange {
    fn to_css<W>(&self, dest: &mut W) -> fmt::Result
    where
        W: fmt::Write,
    {
        if self.0.is_empty() || self.0.contains('*') {
            serialize_string(&self.0, dest)
        } else {
            serialize_identifier(&self.0, dest)
        }
    }
}

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

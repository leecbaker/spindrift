use std::sync::Arc;

use icu_locale_core::Locale;

use super::*;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ResolvedLanguage {
    Unresolved,
    /// A document language resolved from this element or its inheritance
    /// source. [`ContentLanguage`] retains both the authored spelling and its
    /// one-time BCP 47 validation, so inheriting it does not copy or reparse a
    /// language tag.
    Resolved(ContentLanguage),
}

impl ResolvedLanguage {
    pub(crate) fn from_html_attribute(value: &str) -> Self {
        Self::Resolved(ContentLanguage::from_html_attribute(value))
    }

    pub(crate) fn from_computed(value: &ContentLanguage) -> Self {
        Self::Resolved(value.clone())
    }

    pub(crate) fn as_computed_language(&self) -> ContentLanguage {
        match self {
            Self::Resolved(language) => language.clone(),
            Self::Unresolved => ContentLanguage::Unknown,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ElementSignature {
    pub(crate) selector: ElementSiblingSignature,
    pub sibling_index: Option<usize>,
    pub sibling_signatures: ElementSiblingSignatureList,
    pub html_direction: Option<Direction>,
    pub resolved_direction: Option<Direction>,
    pub resolved_language: ResolvedLanguage,
    /// HTML `<picture>` can select width/height presentation attributes from a
    /// preceding `<source>` without exposing them as `<img>` selector attrs.
    pub(crate) selected_image_dimensions: Option<crate::dom::ImageDimensionAttributes>,
}

impl std::ops::Deref for ElementSignature {
    type Target = ElementSelectorSnapshot;

    fn deref(&self) -> &Self::Target {
        &self.selector
    }
}

impl std::ops::DerefMut for ElementSignature {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.selector
    }
}

/// A document content-language value after the host-language inheritance
/// algorithm has run.
///
/// HTML distinguishes an explicitly unknown language from a tagged language
/// whose tag is not recognized by the user agent.  The latter must retain its
/// spelling for selectors and round-tripping, while typography may use only a
/// well-formed BCP 47 value.
/// <https://html.spec.whatwg.org/multipage/dom.html#the-lang-and-xml:lang-attributes>
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) enum ContentLanguage {
    #[default]
    Unknown,
    Tagged(LanguageTag),
}

impl ContentLanguage {
    pub(crate) fn from_html_attribute(value: &str) -> Self {
        let value = value.trim();
        if value.is_empty() {
            Self::Unknown
        } else {
            Self::Tagged(LanguageTag::new(value))
        }
    }

    /// The valid tag available to typography consumers. Malformed authored
    /// tags intentionally have no typography language.
    pub(crate) fn as_deref(&self) -> Option<&str> {
        match self {
            Self::Unknown => None,
            Self::Tagged(tag) => tag.locale().map(|_| tag.as_str()),
        }
    }

    #[cfg(test)]
    pub(crate) fn shares_tag_storage_with(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Unknown, Self::Unknown) => true,
            (Self::Tagged(left), Self::Tagged(right)) => Arc::ptr_eq(&left.source, &right.source),
            _ => false,
        }
    }
}

/// An authored BCP 47 language tag together with its one-time syntax check.
///
/// The raw spelling is retained because HTML requires an unrecognized tag to
/// remain distinct from every other tag.  Underscores are deliberately not
/// normalized here: they are not BCP 47 separators for HTML's `lang` value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LanguageTag {
    source: Arc<str>,
    locale: Option<Locale>,
}

impl LanguageTag {
    pub(crate) fn new(value: impl AsRef<str>) -> Self {
        let source: Arc<str> = Arc::from(value.as_ref());
        let locale = (!source.contains('_'))
            .then(|| source.parse::<Locale>().ok())
            .flatten();
        Self { source, locale }
    }

    pub(crate) fn as_str(&self) -> &str {
        &self.source
    }

    pub(crate) fn locale(&self) -> Option<&Locale> {
        self.locale.as_ref()
    }
}

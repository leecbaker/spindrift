use super::*;
use crate::css::quotes::{ResolvedAutoQuotes, resolved_auto_quotes_for_language};

/// Computed CSS `content` value.
///
/// CSS Generated Content Level 3 defines `content` as controlling whether an
/// element renders normal contents, generated anonymous inline contents, or a
/// replaced image:
/// <https://www.w3.org/TR/css-content-3/#content-property>.
#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Content {
    Normal,
    None,
    List {
        parts: GeneratedContent,
        alt: Option<GeneratedAltText>,
    },
    Replacement {
        image: GeneratedContentPart,
        alt: Option<GeneratedAltText>,
    },
}

impl Content {
    pub(crate) fn generated_parts(&self) -> Option<&[GeneratedContentPart]> {
        match self {
            Self::List { parts, .. } => Some(parts),
            Self::Replacement { image, .. } => Some(std::slice::from_ref(image)),
            Self::Normal | Self::None => None,
        }
    }

    pub(crate) fn is_generated(&self) -> bool {
        matches!(self, Self::List { .. } | Self::Replacement { .. })
    }

    pub(crate) fn alt(&self) -> Option<&[GeneratedAltTextPart]> {
        match self {
            Self::List { alt, .. } | Self::Replacement { alt, .. } => alt.as_deref(),
            Self::Normal | Self::None => None,
        }
    }
}

/// Computed generated `content` parts for elements and tree-abiding
/// pseudo-elements.
///
/// CSS Generated Content Level 3 defines `<content-list>` as a sequence of
/// strings, images, attributes, and counters that generates anonymous inline
/// content:
/// <https://www.w3.org/TR/css-content-3/#typedef-content-list>.
pub(crate) type GeneratedContent = Vec<GeneratedContentPart>;
pub(crate) type GeneratedAltText = Vec<GeneratedAltTextPart>;

/// A same-document target used by generated-content cross references.
///
/// CSS Generated Content Level 3 permits a literal URL as well as an
/// originating-element attribute such as `attr(href)`. Keeping the latter
/// unevaluated until layout is necessary because a tree-abiding pseudo-element
/// has no independent DOM attributes:
/// <https://www.w3.org/TR/css-content-3/#cross-references>.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) enum TargetReference {
    Fragment(String),
    Attribute(String),
}

impl TargetReference {
    /// Returns the same-document fragment identifier for a literal target.
    /// Attribute targets need their originating element and are resolved by
    /// layout instead.
    pub(crate) fn literal_fragment_id(&self) -> Option<&str> {
        match self {
            Self::Fragment(target) => target.strip_prefix('#').filter(|target| !target.is_empty()),
            Self::Attribute(_) => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum GeneratedContentPart {
    Text(String),
    Contents,
    Attr {
        name: String,
        fallback: Option<String>,
    },
    Counter {
        name: String,
        style: Option<ListStyleType>,
    },
    Counters {
        name: String,
        separator: String,
        style: Option<ListStyleType>,
    },
    TargetCounter {
        target: TargetReference,
        name: String,
        style: Option<ListStyleType>,
    },
    TargetText {
        target: TargetReference,
        keyword: NamedStringTargetTextKeyword,
    },
    Image {
        image: ComputedImage,
    },
    Quote(GeneratedQuote),
    Leader(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum GeneratedAltTextPart {
    Text(String),
    Attr {
        name: String,
        fallback: Option<String>,
    },
    Counter {
        name: String,
        style: Option<ListStyleType>,
    },
    Counters {
        name: String,
        separator: String,
        style: Option<ListStyleType>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum GeneratedQuote {
    Open,
    Close,
    NoOpen,
    NoClose,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Quotes {
    Auto(AutoQuoteResolution),
    None,
    Pairs(Vec<(String, String)>),
}

/// Whether the `quotes: auto` value has captured its parent content language.
///
/// CSS Generated Content Level 3 resolves `auto` from that language at
/// computed-value time. The resolved form retains only references to static
/// quote data, not the language text itself:
/// <https://www.w3.org/TR/css-content-3/#quotes-property>.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AutoQuoteResolution {
    Unresolved,
    Resolved(ResolvedAutoQuotes),
}

impl Quotes {
    pub(crate) fn auto() -> Self {
        Self::Auto(AutoQuoteResolution::Unresolved)
    }

    /// Return the value inherited by ordinary `quotes` inheritance.
    ///
    /// CSS Generated Content Level 3 defines `quotes: auto` as resolving from
    /// the parent content language, while `match-parent` reuses the parent's
    /// quote system:
    /// <https://www.w3.org/TR/css-content-3/#quotes-property>.
    pub(crate) fn inherited(&self) -> Self {
        match self {
            Self::Auto(_) => Self::auto(),
            Self::None => Self::None,
            Self::Pairs(pairs) => Self::Pairs(pairs.clone()),
        }
    }

    pub(crate) fn resolve_auto_language(&mut self, language: Option<&str>) {
        if let Self::Auto(resolution @ AutoQuoteResolution::Unresolved) = self {
            *resolution =
                AutoQuoteResolution::Resolved(resolved_auto_quotes_for_language(language));
        }
    }

    /// Return the static quotation marks selected for a resolved `quotes: auto`
    /// value. An uncomputed initial style falls back to the default system.
    pub(crate) fn auto_quote_pair(&self, depth: usize) -> (&'static str, &'static str) {
        let Self::Auto(resolution) = self else {
            unreachable!("only `Quotes::Auto` has an automatic quote pair")
        };
        match resolution {
            AutoQuoteResolution::Unresolved => {
                resolved_auto_quotes_for_language(None).pair_at_depth(depth)
            }
            AutoQuoteResolution::Resolved(system) => system.pair_at_depth(depth),
        }
    }
}

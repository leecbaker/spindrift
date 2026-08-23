use std::num::NonZeroU32;

use super::*;

/// Computed `footnote-display` value.
///
/// GCPM selects block, inline, or UA-compacted layout after the footnote body
/// has been moved into the page's footnote area:
/// <https://www.w3.org/TR/css-gcpm-3/#footnote-display>.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FootnoteDisplay {
    Block,
    Inline,
    Compact,
}

/// Computed `footnote-policy` value.
///
/// This controls the page-break retry point when a footnote body cannot fit
/// alongside its call:
/// <https://www.w3.org/TR/css-gcpm-3/#footnote-policy>.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FootnotePolicy {
    Auto,
    Line,
    Block,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PageBreak {
    Auto,
    /// Generic `avoid` value from CSS Break, applying to every fragmentation context.
    ///
    /// <https://www.w3.org/TR/css-break-3/#valdef-break-before-avoid>.
    Avoid,
    /// Page-specific `avoid-page` value, including legacy `page-break-*` avoids.
    ///
    /// <https://www.w3.org/TR/css-break-3/#valdef-break-before-avoid-page>.
    AvoidPage,
    /// Column-specific `avoid-column` value.
    ///
    /// <https://www.w3.org/TR/css-break-3/#valdef-break-before-avoid-column>.
    AvoidColumn,
    Page,
    Column,
    Left,
    Right,
    Recto,
    Verso,
}

impl PageBreak {
    /// Return whether this value forces a page fragmentainer break.
    ///
    /// CSS Break has forced break values for multiple fragmentation contexts.
    /// Quire's paged-media callers use this page-specific predicate so
    /// `break-before: column` does not accidentally become a page break:
    /// <https://www.w3.org/TR/css-break-3/#forced-breaks>.
    pub(crate) fn is_forced(self) -> bool {
        matches!(
            self,
            Self::Page | Self::Left | Self::Right | Self::Recto | Self::Verso
        )
    }

    /// Return whether this value avoids page fragmentation.
    ///
    /// `avoid-column` is intentionally excluded so page layout does not keep
    /// content together for a column-only constraint:
    /// <https://www.w3.org/TR/css-break-3/#break-between>.
    pub(crate) fn avoids_page(self) -> bool {
        matches!(self, Self::Avoid | Self::AvoidPage)
    }

    pub(crate) fn avoids_column(self) -> bool {
        matches!(self, Self::Avoid | Self::AvoidColumn)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct BookmarkLabel {
    pub parts: Vec<BookmarkLabelPart>,
}

impl BookmarkLabel {
    pub fn content_text() -> Self {
        Self {
            parts: vec![BookmarkLabelPart::ContentText],
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum BookmarkLabelPart {
    String(String),
    ContentText,
    Attr(String),
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct NamedStringSet {
    pub name: String,
    pub parts: Vec<NamedStringPart>,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum NamedStringPart {
    String(String),
    ContentText,
    ContentFirstLetter,
    ContentMarker,
    BeforeContent,
    AfterContent,
    Attr {
        name: String,
        fallback: Option<String>,
    },
    Image(ComputedImage),
    Quote(GeneratedQuote),
    Leader(String),
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
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum NamedStringTargetTextKeyword {
    Content,
    Before,
    After,
    FirstLetter,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CssBookmarkState {
    Open,
    Closed,
}

/// A custom identifier used to select a named `@page` rule.
/// <https://www.w3.org/TR/css-page-3/#page-property>
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct PageName(String);

impl PageName {
    pub(crate) fn new(value: String) -> Self {
        Self(value)
    }

    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }
}

/// Computed `page` state, including the distinction between an omitted
/// declaration and an explicit `page: auto` needed by named-page propagation.
/// <https://www.w3.org/TR/css-page-3/#using-named-pages>
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum PageAssignment {
    Unspecified,
    Auto,
    Named(PageName),
}

/// The computed `break-inside` avoidance target.
///
/// `avoid` applies to every fragmentainer; the page and column variants are
/// deliberately retained as distinct computed values rather than collapsed
/// into independent booleans.
/// <https://www.w3.org/TR/css-break-3/#propdef-break-inside>
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum BreakInsideAvoidance {
    #[default]
    Auto,
    Avoid,
    AvoidPage,
    AvoidColumn,
}

impl BreakInsideAvoidance {
    pub(crate) const fn avoids_page(self) -> bool {
        matches!(self, Self::Avoid | Self::AvoidPage)
    }

    pub(crate) const fn avoids_column(self) -> bool {
        matches!(self, Self::Avoid | Self::AvoidColumn)
    }

    pub(crate) fn parse_modern(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "auto" => Some(Self::Auto),
            "avoid" => Some(Self::Avoid),
            "avoid-page" => Some(Self::AvoidPage),
            "avoid-column" => Some(Self::AvoidColumn),
            _ => None,
        }
    }

    pub(crate) fn parse_legacy_page(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "auto" => Some(Self::Auto),
            "avoid" => Some(Self::AvoidPage),
            _ => None,
        }
    }
}

impl PageAssignment {
    pub(crate) const fn is_specified(&self) -> bool {
        !matches!(self, Self::Unspecified)
    }

    pub(crate) fn specified_name(&self) -> Option<&PageName> {
        match self {
            Self::Named(name) => Some(name),
            Self::Unspecified | Self::Auto => None,
        }
    }

    pub(crate) fn effective_name(&self, inherited: Option<String>) -> Option<String> {
        match self {
            Self::Unspecified => inherited,
            Self::Auto => None,
            Self::Named(name) => Some(name.0.clone()),
        }
    }
}

/// A custom identifier for a GCPM running element.
/// <https://www.w3.org/TR/css-gcpm-3/#running-elements>
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct RunningElementName(String);

impl RunningElementName {
    pub(crate) fn new(value: String) -> Self {
        Self(value)
    }

    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }
}

/// Computed GCPM bookmark level.
/// <https://www.w3.org/TR/css-gcpm-3/#bookmarks>
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BookmarkLevel {
    None,
    Level(NonZeroU32),
}

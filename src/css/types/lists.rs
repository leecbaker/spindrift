use super::*;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ListStyleType {
    Disc,
    Circle,
    Square,
    DisclosureOpen,
    DisclosureClosed,
    Decimal,
    String(String),
    Anonymous(Box<CounterStyleRule>),
    Named(String),
    None,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ListStylePosition {
    Outside,
    Inside,
}

/// Side-selection mode for outside list markers.
///
/// CSS Lists Level 3 defines `marker-side` to choose whether an outside marker
/// is positioned from the list item's own directionality or its parent's:
/// <https://www.w3.org/TR/css-lists-3/#marker-side>.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MarkerSide {
    MatchSelf,
    MatchParent,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum MarkerContent {
    Auto,
    None,
    Parts(Vec<MarkerContentPart>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum MarkerContentPart {
    Text(String),
    Quote(GeneratedQuote),
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

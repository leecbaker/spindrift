use super::html_form_state;
use super::types::{Direction, ElementSignature, ResolvedLanguage, ScopeRule};
use cssparser::{
    CowRcStr, Parser as CssParser, SourceLocation, ToCss, serialize_identifier, serialize_string,
};
use precomputed_hash::PrecomputedHash;
use selectors::attr::{AttrSelectorOperation, CaseSensitivity, NamespaceConstraint};
use selectors::context::{
    MatchingContext, MatchingForInvalidation, MatchingMode, NeedsSelectorFlags, QuirksMode,
    SelectorCaches,
};
use selectors::matching::{ElementSelectorFlags, matches_selector, matches_selector_list};
use selectors::parser::{
    NonTSPseudoClass, Parser as SelectorParser, PseudoElement, SelectorImpl, SelectorList,
    SelectorParseErrorKind,
};
use selectors::{Element as SelectorElement, OpaqueElement};
use std::borrow::Cow;
use std::collections::HashMap;
use std::collections::hash_map::DefaultHasher;
use std::fmt;
use std::fmt::Write as _;
use std::hash::{Hash, Hasher};
use std::rc::Rc;

mod split_1;
pub(crate) use self::split_1::*;
mod split_2;
pub(crate) use self::split_2::*;

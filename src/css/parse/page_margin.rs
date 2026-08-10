use super::*;
use crate::css::{LayerName, LayerOrder};
use cssparser::{
    AtRuleParser, BasicParseErrorKind, CowRcStr, DeclarationParser, Parser, ParserState,
    RuleBodyItemParser, Token,
};

/// A syntactically parsed `@page` rule before stylesheet-level cascade metadata
/// has been assigned. Origin, layer order, and rule order belong to the
/// stylesheet collector rather than the at-rule grammar.
#[derive(Debug)]
pub(in crate::css) struct ParsedPageRule {
    pub(in crate::css) selectors: Vec<PageSelector>,
    pub(in crate::css) declarations: Declarations,
    pub(in crate::css) margin_boxes: HashMap<String, Declarations>,
    pub(in crate::css) footnote_area: Option<Declarations>,
    pub(in crate::css) layer: Option<LayerName>,
}

/// Parse CSS Paged Media's page-selector list from CSS tokens.
///
/// CSS Paged Media requires selector components to be adjacent. GCPM's
/// existing `:nth()` extension uses CSS Syntax's shared `an+b` parser.
/// <https://www.w3.org/TR/css-page-3/#page-selectors>
/// <https://www.w3.org/TR/css-gcpm-3/#document-page-selectors>
pub(super) fn parse_page_selector_list<'i, 't>(
    input: &mut Parser<'i, 't>,
) -> Result<Vec<PageSelector>, cssparser::ParseError<'i, ()>> {
    input.skip_whitespace();
    if input.is_exhausted() {
        return Ok(Vec::new());
    }
    input.parse_comma_separated(parse_page_selector)
}

fn parse_page_selector<'i, 't>(
    input: &mut Parser<'i, 't>,
) -> Result<PageSelector, cssparser::ParseError<'i, ()>> {
    let first = input.next_including_whitespace_and_comments()?.clone();
    let mut page_type = None;
    let mut pseudos = Vec::new();
    match first {
        Token::Ident(name) => page_type = Some(name.to_string()),
        Token::Colon => pseudos.push(parse_page_pseudo(input)?),
        _ => return Err(input.new_custom_error(())),
    }
    while !input.is_exhausted() {
        if !matches!(
            input.next_including_whitespace_and_comments()?,
            Token::Colon
        ) {
            return Err(input.new_custom_error(()));
        }
        pseudos.push(parse_page_pseudo(input)?);
    }
    Ok(PageSelector { page_type, pseudos })
}

fn parse_page_pseudo<'i, 't>(
    input: &mut Parser<'i, 't>,
) -> Result<PagePseudo, cssparser::ParseError<'i, ()>> {
    match input.next_including_whitespace_and_comments()?.clone() {
        Token::Ident(name) if name.eq_ignore_ascii_case("first") => Ok(PagePseudo::First),
        Token::Ident(name) if name.eq_ignore_ascii_case("left") => Ok(PagePseudo::Left),
        Token::Ident(name) if name.eq_ignore_ascii_case("right") => Ok(PagePseudo::Right),
        Token::Ident(name) if name.eq_ignore_ascii_case("blank") => Ok(PagePseudo::Blank),
        Token::Function(name) if name.eq_ignore_ascii_case("nth") => {
            input.parse_nested_block(|input| {
                let (a, b) = cssparser::parse_nth(input)?;
                input.expect_exhausted()?;
                Ok(PagePseudo::Nth { a, b })
            })
        }
        _ => Err(input.new_custom_error(())),
    }
}

#[derive(Clone, Copy)]
pub(super) enum PageNestedAtRule {
    MarginBox(&'static str),
    Footnote,
}

fn page_nested_at_rule(name: &str) -> Option<PageNestedAtRule> {
    let margin_box = PAGE_MARGIN_BOX_NAMES
        .iter()
        .copied()
        .find(|known| name.eq_ignore_ascii_case(known));
    margin_box.map(PageNestedAtRule::MarginBox).or_else(|| {
        name.eq_ignore_ascii_case("footnote")
            .then_some(PageNestedAtRule::Footnote)
    })
}

pub(super) const PAGE_MARGIN_BOX_NAMES: &[&str] = &[
    "top-left-corner",
    "top-left",
    "top-center",
    "top-right",
    "top-right-corner",
    "right-top",
    "right-middle",
    "right-bottom",
    "bottom-right-corner",
    "bottom-right",
    "bottom-center",
    "bottom-left",
    "bottom-left-corner",
    "left-bottom",
    "left-middle",
    "left-top",
];

pub(super) struct PageRuleBodyParser<'a> {
    declarations: Declarations,
    margin_boxes: HashMap<String, Declarations>,
    footnote_area: Option<Declarations>,
    base_url: Option<&'a url::Url>,
    root_url: Option<&'a url::Url>,
}

impl<'a> PageRuleBodyParser<'a> {
    pub(super) fn new(base_url: Option<&'a url::Url>, root_url: Option<&'a url::Url>) -> Self {
        Self {
            declarations: Declarations::new().with_urls(base_url, root_url),
            margin_boxes: HashMap::new(),
            footnote_area: None,
            base_url,
            root_url,
        }
    }

    pub(super) fn finish(
        self,
        selectors: Vec<PageSelector>,
        layer: Option<LayerName>,
    ) -> ParsedPageRule {
        ParsedPageRule {
            selectors,
            declarations: self.declarations,
            margin_boxes: self.margin_boxes,
            footnote_area: self.footnote_area,
            layer,
        }
    }
}

impl<'i> DeclarationParser<'i> for PageRuleBodyParser<'_> {
    type Declaration = ();
    type Error = BasicParseErrorKind<'i>;

    fn parse_value<'t>(
        &mut self,
        name: CowRcStr<'i>,
        input: &mut Parser<'i, 't>,
        declaration_start: &ParserState,
    ) -> Result<Self::Declaration, cssparser::ParseError<'i, Self::Error>> {
        let mut collector = DeclarationCollector;
        self.declarations.extend(
            std::iter::once(collector.parse_value(name, input, declaration_start)?).collect(),
        );
        Ok(())
    }
}

impl<'i> AtRuleParser<'i> for PageRuleBodyParser<'_> {
    type Prelude = PageNestedAtRule;
    type AtRule = ();
    type Error = BasicParseErrorKind<'i>;

    fn parse_prelude<'t>(
        &mut self,
        name: CowRcStr<'i>,
        input: &mut Parser<'i, 't>,
    ) -> Result<Self::Prelude, cssparser::ParseError<'i, Self::Error>> {
        let Some(kind) = page_nested_at_rule(&name) else {
            return Err(input.new_custom_error(BasicParseErrorKind::AtRuleInvalid(name)));
        };
        input.expect_exhausted()?;
        Ok(kind)
    }

    fn rule_without_block(
        &mut self,
        _prelude: Self::Prelude,
        _start: &ParserState,
    ) -> Result<Self::AtRule, ()> {
        Err(())
    }

    fn parse_block<'t>(
        &mut self,
        prelude: Self::Prelude,
        _start: &ParserState,
        input: &mut Parser<'i, 't>,
    ) -> Result<Self::AtRule, cssparser::ParseError<'i, Self::Error>> {
        let declarations = parse_declarations_from_parser(input, self.base_url, self.root_url);
        match prelude {
            PageNestedAtRule::MarginBox(name) => self
                .margin_boxes
                .entry(name.to_string())
                .or_insert_with(|| Declarations::new().with_urls(self.base_url, self.root_url))
                .extend(declarations),
            PageNestedAtRule::Footnote => self
                .footnote_area
                .get_or_insert_with(|| Declarations::new().with_urls(self.base_url, self.root_url))
                .extend(declarations),
        }
        Ok(())
    }
}

impl<'i> cssparser::QualifiedRuleParser<'i> for PageRuleBodyParser<'_> {
    type Prelude = ();
    type QualifiedRule = ();
    type Error = BasicParseErrorKind<'i>;
}

impl<'i> RuleBodyItemParser<'i, (), BasicParseErrorKind<'i>> for PageRuleBodyParser<'_> {
    fn parse_declarations(&self) -> bool {
        true
    }

    fn parse_qualified(&self) -> bool {
        false
    }
}

pub(crate) fn cascade_page_declarations(
    page_rules: &[PageRule],
    page_number: usize,
) -> Declarations {
    cascade_page_rule_declarations(page_rules.iter().filter_map(|rule| {
        rule.matching_specificity(page_number, None, false, Direction::Ltr)
            .map(|specificity| {
                (
                    rule.origin,
                    specificity,
                    rule.layer_order.clone(),
                    rule.order,
                    &rule.declarations,
                )
            })
    }))
}

/// Cascades declarations in the CSS page context.
///
/// CSS Paged Media 3 adds page-selector specificity to the page context, while
/// CSS Cascade Level 5 keeps origin/importance/layers before specificity:
/// <https://www.w3.org/TR/css-page-3/#cascading-and-page-context> and
/// <https://www.w3.org/TR/css-cascade-5/#cascade-sort>.
pub(super) fn cascade_page_rule_declarations<'a>(
    declarations: impl Iterator<
        Item = (
            StylesheetOrigin,
            PageSpecificity,
            Option<LayerOrder>,
            usize,
            &'a Declarations,
        ),
    >,
) -> Declarations {
    let mut candidates = Vec::new();
    let mut declaration_order = 0usize;
    for (origin, specificity, layer_order, rule_order, declarations) in declarations {
        for (name, value) in declarations {
            let important = page_declaration_is_important(value);
            let candidate_key = PageCascadeKey {
                important,
                origin,
                origin_rank: super::super::cascade::origin_importance_rank(origin, important),
                layer_order: layer_order.clone(),
                specificity,
                rule_order,
                declaration_order,
            };
            declaration_order += 1;
            candidates.push(PageCascadedDeclaration {
                name: name.clone(),
                value: value.clone(),
                key: candidate_key,
            });
        }
    }
    candidates.sort_by(|left, right| compare_page_cascade_keys(&left.key, &right.key));

    let mut active: Vec<PageCascadedDeclaration> = Vec::with_capacity(candidates.len());
    for candidate in candidates {
        if page_declaration_is_revert(&candidate.value) {
            active.retain(|existing| {
                !super::super::cascade::declarations_affect_same_property(
                    &existing.name,
                    &candidate.name,
                ) || !same_or_stronger_reverted_page_origin(existing, &candidate)
            });
        } else if page_declaration_is_revert_layer(&candidate.value) {
            active.retain(|existing| {
                !super::super::cascade::declarations_affect_same_property(
                    &existing.name,
                    &candidate.name,
                ) || !same_page_cascade_layer(existing, &candidate)
            });
        } else {
            active.push(candidate);
        }
    }

    let mut winners: Vec<(String, String, PageCascadeKey)> = Vec::new();
    for candidate in active {
        if let Some(existing) = winners
            .iter_mut()
            .find(|(existing_name, _, _)| existing_name == &candidate.name)
        {
            *existing = (candidate.name, candidate.value, candidate.key);
        } else {
            winners.push((candidate.name, candidate.value, candidate.key));
        }
    }
    winners
        .into_iter()
        .map(|(name, value, _)| (name, value))
        .collect()
}

#[derive(Debug, Clone)]
struct PageCascadedDeclaration {
    name: String,
    value: String,
    key: PageCascadeKey,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct PageCascadeKey {
    important: bool,
    origin_rank: u8,
    specificity: PageSpecificity,
    rule_order: usize,
    declaration_order: usize,
    origin: StylesheetOrigin,
    layer_order: Option<LayerOrder>,
}

fn page_declaration_is_important(value: &str) -> bool {
    value
        .trim_end()
        .to_ascii_lowercase()
        .ends_with("!important")
}

fn page_declaration_is_revert_layer(value: &str) -> bool {
    trim_css_value(value).eq_ignore_ascii_case("revert-layer")
}

fn page_declaration_is_revert(value: &str) -> bool {
    trim_css_value(value).eq_ignore_ascii_case("revert")
}

/// Returns whether a prior page declaration is erased by a later `revert`.
///
/// CSS Paged Media uses normal cascade origins for page-context declarations,
/// and CSS Cascade Level 5 defines `revert` in terms of those origins:
/// <https://www.w3.org/TR/css-page-3/#cascading-and-page-context> and
/// <https://www.w3.org/TR/css-cascade-5/#revert>.
fn same_or_stronger_reverted_page_origin(
    prior: &PageCascadedDeclaration,
    rollback: &PageCascadedDeclaration,
) -> bool {
    match rollback.key.origin {
        StylesheetOrigin::Author => prior.key.origin == StylesheetOrigin::Author,
        StylesheetOrigin::User => {
            matches!(
                prior.key.origin,
                StylesheetOrigin::User | StylesheetOrigin::Author
            )
        }
        StylesheetOrigin::UserAgent => prior.key.origin == StylesheetOrigin::UserAgent,
    }
}

fn same_page_cascade_layer(
    left: &PageCascadedDeclaration,
    right: &PageCascadedDeclaration,
) -> bool {
    left.key.origin == right.key.origin
        && left.key.important == right.key.important
        && left.key.layer_order == right.key.layer_order
}

fn compare_page_cascade_keys(left: &PageCascadeKey, right: &PageCascadeKey) -> std::cmp::Ordering {
    left.origin_rank
        .cmp(&right.origin_rank)
        .then_with(|| {
            compare_page_layer_order(
                left.layer_order.as_ref(),
                right.layer_order.as_ref(),
                left.important,
            )
        })
        .then_with(|| left.specificity.cmp(&right.specificity))
        .then_with(|| left.rule_order.cmp(&right.rule_order))
        .then_with(|| left.declaration_order.cmp(&right.declaration_order))
}

fn compare_page_layer_order(
    left: Option<&LayerOrder>,
    right: Option<&LayerOrder>,
    important: bool,
) -> std::cmp::Ordering {
    match (important, left, right) {
        (false, Some(left), Some(right)) => left.cmp(right),
        (false, Some(_), None) => std::cmp::Ordering::Less,
        (false, None, Some(_)) => std::cmp::Ordering::Greater,
        (false, None, None) => std::cmp::Ordering::Equal,
        (true, Some(left), Some(right)) => right.cmp(left),
        (true, Some(_), None) => std::cmp::Ordering::Greater,
        (true, None, Some(_)) => std::cmp::Ordering::Less,
        (true, None, None) => std::cmp::Ordering::Equal,
    }
}

use super::*;
/// Provenance for a general block-axis percentage basis.
///
/// Formatting contexts can expose a definite content block-size for descendant
/// percentage heights through ordinary CSS Sizing rules or through
/// context-specific relayout. More specialized layout modes can use their own
/// source enum when the exact reason affects correctness.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(in crate::layout) enum BlockSizeBasisSource {
    /// The page area's initial containing block, used only to resolve the
    /// document root's own percentage block size.
    ///
    /// <https://www.w3.org/TR/CSS2/visudet.html#root-height>
    InitialContainingBlock,
    ContainingBlock,
    InlineBlock,
    TableWrapper,
    TableCell,
    FlexItem,
    GridItem,
    AbsolutePositioned,
}

pub(in crate::layout) type BlockSizePercentageBasis =
    PercentageBasis<ContentBoxLength, BlockSizeBasisSource>;

/// Why a formatting-context root supplies its descendant block-axis
/// percentage basis.
///
/// A content-sized block has an indefinite percentage basis, but that alone
/// is not enough information for intrinsic sizing. CSS Sizing requires a
/// cyclic percentage in a non-replaced preferred or maximum block-size to be
/// treated as the property's initial value for its intrinsic contribution.
/// Keeping that reason at the formatting-context boundary prevents a replayed
/// used height from accidentally becoming a new definite basis.
/// <https://drafts.csswg.org/css-sizing-3/#cyclic-percentage-contribution>
#[derive(Debug, Clone, Copy, PartialEq)]
pub(in crate::layout) enum DescendantBlockPercentageContext {
    Definite {
        value: ContentBoxLength,
        source: BlockSizeBasisSource,
    },
    Indefinite,
    ContentSized,
}

impl DescendantBlockPercentageContext {
    pub(in crate::layout) fn definite(
        value: ContentBoxLength,
        source: BlockSizeBasisSource,
    ) -> Self {
        Self::Definite { value, source }
    }

    pub(in crate::layout) fn from_percentage_basis(basis: BlockSizePercentageBasis) -> Self {
        match basis {
            PercentageBasis::Definite { value, source } => Self::Definite { value, source },
            PercentageBasis::Indefinite => Self::Indefinite,
        }
    }

    /// Classify a formatting context from its already-resolved used content
    /// height.  An absent height is a content-sized, not merely unknown,
    /// percentage boundary.
    pub(in crate::layout) fn formatting_context(
        used_content_height: Option<ContentBoxLength>,
        source: BlockSizeBasisSource,
    ) -> Self {
        used_content_height.map_or(Self::ContentSized, |value| Self::definite(value, source))
    }

    /// Returns the ordinary definiteness-only view for properties whose
    /// cyclic fallback is unchanged.
    pub(in crate::layout) fn percentage_basis(self) -> BlockSizePercentageBasis {
        match self {
            Self::Definite { value, source } => PercentageBasis::definite_from(value, source),
            Self::Indefinite | Self::ContentSized => PercentageBasis::indefinite(),
        }
    }

    pub(in crate::layout) fn is_definite(self) -> bool {
        matches!(self, Self::Definite { .. })
    }

    pub(in crate::layout) fn is_content_sized(self) -> bool {
        matches!(self, Self::ContentSized)
    }
}

/// The block-axis percentage context active at nested formatting-context
/// boundaries.
///
/// This is intentionally the sole stored stack.  Callers that only need the
/// ordinary CSS percentage basis must derive it through
/// [`Self::current_percentage_basis`], so a cyclic `ContentSized` boundary
/// cannot diverge from a parallel definiteness-only stack.
/// <https://drafts.csswg.org/css-sizing-3/#percentage-sizing>
#[derive(Debug, Clone, Default, PartialEq)]
pub(in crate::layout) struct BlockPercentageContextStack {
    entries: Vec<DescendantBlockPercentageContext>,
}

impl BlockPercentageContextStack {
    pub(in crate::layout) fn current_context(&self) -> DescendantBlockPercentageContext {
        self.entries
            .last()
            .copied()
            .unwrap_or(DescendantBlockPercentageContext::Indefinite)
    }

    pub(in crate::layout) fn current_percentage_basis(&self) -> BlockSizePercentageBasis {
        self.current_context().percentage_basis()
    }

    /// Returns the containing context's ordinary basis while an inner
    /// formatting-context scope is active.
    pub(in crate::layout) fn parent_percentage_basis(&self) -> BlockSizePercentageBasis {
        self.entries
            .iter()
            .rev()
            .nth(1)
            .copied()
            .unwrap_or(DescendantBlockPercentageContext::Indefinite)
            .percentage_basis()
    }

    pub(in crate::layout) fn push_context(&mut self, context: DescendantBlockPercentageContext) {
        self.entries.push(context);
    }

    /// Push a legacy definiteness-only boundary as a semantic context.
    ///
    /// New formatting-context boundaries should use [`Self::push_context`]
    /// directly so they can retain `ContentSized` when appropriate.
    pub(in crate::layout) fn push_percentage_basis(&mut self, basis: BlockSizePercentageBasis) {
        self.push_context(DescendantBlockPercentageContext::from_percentage_basis(
            basis,
        ));
    }

    pub(in crate::layout) fn pop(&mut self) -> DescendantBlockPercentageContext {
        self.entries
            .pop()
            .expect("block percentage context stack is balanced")
    }

    pub(in crate::layout) fn clear(&mut self) {
        self.entries.clear();
    }
}

pub(in crate::layout) fn percentage_basis_from_points(
    value: Option<f32>,
) -> PercentageBasis<ContentBoxLength> {
    value
        .map(content_box_pt)
        .map(PercentageBasis::definite)
        .unwrap_or_else(PercentageBasis::indefinite)
}

pub(in crate::layout) fn block_size_percentage_basis_from_points(
    value: Option<f32>,
    source: BlockSizeBasisSource,
) -> BlockSizePercentageBasis {
    value
        .map(|value| PercentageBasis::definite_from(content_box_pt(value), source))
        .unwrap_or_else(PercentageBasis::indefinite)
}

/// Resolves a computed `<length-percentage>` against a used percentage basis.
///
/// CSS Values and Units Level 4 defines computed `<length-percentage>` values
/// whose percentage component is resolved later against a property-specific
/// basis:
/// <https://www.w3.org/TR/css-values-4/#mixed-percentages>.
pub(in crate::layout) fn used_length_percentage<T, Source>(
    value: css::ComputedLengthPercentage,
    percentage_basis: PercentageBasis<T, Source>,
) -> LayoutLength
where
    T: SemanticLengthExt,
{
    value
        .used_length_with_percentage_basis(percentage_basis)
        .unwrap_or_else(|| layout_pt(value.length_points()))
}

/// Resolves a computed length only when its CSS percentage basis is definite.
///
/// Callers that need a scalar used length may extract layout points after this
/// boundary; callers with an indefinite basis must follow their property's
/// CSS fallback behavior instead.
pub(in crate::layout) fn used_length_percentage_with_basis<T, Source>(
    value: css::ComputedLengthPercentage,
    percentage_basis: PercentageBasis<T, Source>,
) -> Option<LayoutLength>
where
    T: SemanticLengthExt,
{
    value.used_length_with_percentage_basis(percentage_basis)
}

/// Resolves a computed `<length-percentage> | auto` value, preserving `auto`.
///
/// CSS Cascade defines computed values and CSS 2.2 visual formatting defines
/// the later used-value stage where `auto` may be resolved by the formatting
/// context:
/// <https://www.w3.org/TR/css-cascade-5/#computed> and
/// <https://www.w3.org/TR/CSS22/visudet.html>.
pub(in crate::layout) fn used_length_percentage_or_auto<T, Source>(
    value: css::ComputedLengthPercentageOrAuto,
    percentage_basis: PercentageBasis<T, Source>,
) -> Option<LayoutLength>
where
    T: SemanticLengthExt,
{
    match value {
        css::ComputedLengthPercentageOrAuto::Auto
        | css::ComputedLengthPercentageOrAuto::Stretch => None,
        css::ComputedLengthPercentageOrAuto::LengthPercentage(value) => {
            value.used_length_with_percentage_basis(percentage_basis)
        }
        css::ComputedLengthPercentageOrAuto::MinContent
        | css::ComputedLengthPercentageOrAuto::MaxContent
        | css::ComputedLengthPercentageOrAuto::FitContent(_)
        | css::ComputedLengthPercentageOrAuto::CalcSize(_) => None,
    }
}

/// Resolves a computed `<length-percentage> | auto` value against an optional basis.
///
/// CSS Sizing defines percentages as definite only when the relevant
/// containing block axis is definite. Intrinsic sizing paths pass `None` so
/// unresolved percentages behave like `auto` rather than accidentally using an
/// available-size constraint as a containing block:
/// <https://www.w3.org/TR/css-sizing-3/#definite> and
/// <https://www.w3.org/TR/css-sizing-3/#intrinsic-sizes>.
pub(in crate::layout) fn used_length_percentage_or_auto_with_basis<Source>(
    value: css::ComputedLengthPercentageOrAuto,
    percentage_basis: PercentageBasis<ContentBoxLength, Source>,
) -> Option<LayoutLength> {
    match value {
        css::ComputedLengthPercentageOrAuto::Auto
        | css::ComputedLengthPercentageOrAuto::Stretch => None,
        css::ComputedLengthPercentageOrAuto::LengthPercentage(value) => {
            used_length_percentage_with_basis(value, percentage_basis)
        }
        css::ComputedLengthPercentageOrAuto::MinContent
        | css::ComputedLengthPercentageOrAuto::MaxContent
        | css::ComputedLengthPercentageOrAuto::FitContent(_)
        | css::ComputedLengthPercentageOrAuto::CalcSize(_) => None,
    }
}

#[cfg(test)]
mod tests {
    use super::super::*;

    fn length_auto(value: f32) -> css::ComputedLengthPercentageOrAuto {
        css::ComputedLengthPercentageOrAuto::LengthPercentage(
            css::ComputedLengthPercentage::from_points(value),
        )
    }

    fn percent_auto(value: f32) -> css::ComputedLengthPercentageOrAuto {
        css::ComputedLengthPercentageOrAuto::LengthPercentage(
            css::ComputedLengthPercentage::from_percent(value),
        )
    }

    #[test]
    fn percentage_basis_carries_definite_typed_values_without_indefinite_numbers() {
        let definite = PercentageBasis::definite_from(
            content_box_pt(42.0),
            BlockSizeBasisSource::ContainingBlock,
        );
        let indefinite: BlockSizePercentageBasis = PercentageBasis::indefinite();

        assert!(definite.is_definite());
        assert_eq!(definite.points(), Some(42.0));
        assert_eq!(
            definite
                .map_value(|value| content_box_pt(value.points() * 2.0))
                .points(),
            Some(84.0)
        );
        assert!(!indefinite.is_definite());
        assert_eq!(indefinite.points(), None);
    }

    #[test]
    fn used_lengths_resolve_percentage_against_basis() {
        let value = css::ComputedLengthPercentage::from_affine(layout_pt(12.0), 0.25, true);
        let used: LayoutLength =
            used_length_percentage(value.clone(), PercentageBasis::definite(layout_pt(200.0)));
        assert_eq!(used.points(), 62.0);
        assert_eq!(
            used_length_percentage_with_basis(
                value.clone(),
                PercentageBasis::definite(content_box_pt(200.0)),
            )
            .map(layout_points),
            Some(62.0)
        );
        assert_eq!(
            used_length_percentage_with_basis(
                value,
                PercentageBasis::<ContentBoxLength>::indefinite(),
            ),
            None
        );
    }
    #[test]
    fn used_length_or_auto_keeps_fixed_lengths_under_an_indefinite_basis() {
        let fixed: LayoutLength = used_length_percentage_or_auto(
            length_auto(12.0),
            PercentageBasis::<ContentBoxLength>::indefinite(),
        )
        .expect("fixed lengths resolve without a percentage basis");
        assert_eq!(fixed.points(), 12.0);

        assert_eq!(
            used_length_percentage_or_auto(
                percent_auto(0.5),
                PercentageBasis::<ContentBoxLength>::indefinite(),
            ),
            None,
        );

        let percentage: LayoutLength = used_length_percentage_or_auto(
            percent_auto(0.5),
            PercentageBasis::definite(content_box_pt(200.0)),
        )
        .expect("a definite basis resolves percentages");
        assert_eq!(percentage.points(), 100.0);
    }

    #[test]
    fn unresolved_metric_expression_is_not_silently_treated_as_zero() {
        let unresolved = css::ComputedLengthPercentageOrAuto::LengthPercentage(
            css::ComputedLengthPercentage::sum(
                css::ComputedLengthPercentage::from_points(12.0),
                css::ComputedLengthPercentage::from_em(1.0),
            ),
        );

        assert_eq!(
            used_length_percentage_or_auto(
                unresolved.clone(),
                PercentageBasis::<ContentBoxLength>::indefinite(),
            ),
            None,
        );
        assert_eq!(
            used_content_box_size_with_basis(
                unresolved,
                BoxSizing::ContentBox,
                PercentageBasis::<ContentBoxLength>::indefinite(),
                non_content_pt(0.0),
            ),
            None,
        );
    }

    #[test]
    fn block_percentage_context_stack_derives_ordinary_basis_without_losing_reason() {
        let mut stack = BlockPercentageContextStack::default();
        assert_eq!(
            stack.current_context(),
            DescendantBlockPercentageContext::Indefinite
        );
        assert!(!stack.current_percentage_basis().is_definite());

        let definite = DescendantBlockPercentageContext::definite(
            content_box_pt(144.0),
            BlockSizeBasisSource::InitialContainingBlock,
        );
        stack.push_context(definite);
        assert_eq!(stack.current_context(), definite);
        assert_eq!(stack.current_percentage_basis().points(), Some(144.0));

        stack.push_context(DescendantBlockPercentageContext::ContentSized);
        assert_eq!(
            stack.current_context(),
            DescendantBlockPercentageContext::ContentSized
        );
        assert!(!stack.current_percentage_basis().is_definite());

        let snapshot = stack.clone();
        assert_eq!(snapshot, stack);
        assert_eq!(stack.pop(), DescendantBlockPercentageContext::ContentSized);
        assert_eq!(stack.current_context(), definite);
        assert_eq!(
            snapshot.current_context(),
            DescendantBlockPercentageContext::ContentSized
        );
    }
}

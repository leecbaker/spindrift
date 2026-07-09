use super::*;

/// Provenance for a general block-axis percentage basis.
///
/// Formatting contexts can expose a definite content block-size for descendant
/// percentage heights through ordinary CSS Sizing rules or through
/// context-specific relayout. More specialized layout modes can use their own
/// source enum when the exact reason affects correctness.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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

/// Used physical margin or padding edges for a layout formatting context.
///
/// CSS Box Model defines physical box edges and percentage resolution for
/// margin and padding:
/// <https://www.w3.org/TR/CSS22/box.html#margin-properties> and
/// <https://www.w3.org/TR/CSS22/box.html#padding-properties>.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(in crate::layout) struct UsedEdges {
    pub(in crate::layout) top: LayoutLength,
    pub(in crate::layout) right: LayoutLength,
    pub(in crate::layout) bottom: LayoutLength,
    pub(in crate::layout) left: LayoutLength,
}

impl UsedEdges {
    /// Converts used edge lengths back to the renderer's existing edge shape.
    ///
    /// CSS Box Model defines the physical edge order used here:
    /// <https://www.w3.org/TR/css-box-3/#box-model>.
    pub(in crate::layout) fn to_css_edges(self) -> css::Edges {
        css::Edges {
            top: self.top.points(),
            right: self.right.points(),
            bottom: self.bottom.points(),
            left: self.left.points(),
        }
    }
}

/// Used margin and padding edges for a box in a specific containing block.
///
/// CSS 2.2 resolves margin and padding percentages against the containing
/// block width:
/// <https://www.w3.org/TR/CSS22/box.html#margin-properties> and
/// <https://www.w3.org/TR/CSS22/box.html#padding-properties>.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(in crate::layout) struct UsedBoxEdges {
    pub(in crate::layout) margin: UsedEdges,
    pub(in crate::layout) padding: UsedEdges,
}

/// Used physical box metrics after margin and padding percentages are resolved.
///
/// CSS Box Model lays out content, padding, border, and margin as nested
/// physical edges; CSS 2.2 resolves margin and padding percentages against the
/// containing block width before used geometry is computed:
/// <https://www.w3.org/TR/css-box-3/#box-model> and
/// <https://www.w3.org/TR/CSS22/box.html#box-dimensions>.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(in crate::layout) struct UsedBoxMetrics {
    pub(in crate::layout) margin: css::Edges,
    pub(in crate::layout) padding: css::Edges,
    pub(in crate::layout) border: css::Edges,
}

impl UsedBoxMetrics {
    /// Returns horizontal padding and border in non-content box-model space.
    pub(in crate::layout) fn horizontal_non_content_length(self) -> NonContentLength {
        non_content_pt(
            self.border.left + self.border.right + self.padding.left + self.padding.right,
        )
    }

    /// Returns vertical padding and border in non-content box-model space.
    pub(in crate::layout) fn vertical_non_content_length(self) -> NonContentLength {
        non_content_pt(
            self.border.top + self.border.bottom + self.padding.top + self.padding.bottom,
        )
    }
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

/// Resolves CSS Sizing Level 4 stretch-fit sizing to a content-box size.
///
/// `stretch` attempts to make the margin box fill the available space. Layout
/// callers pass the already-resolved available margin-box size, used margins,
/// and padding plus border for the relevant axis, and the content box is
/// floored at zero:
/// <https://drafts.csswg.org/css-sizing-4/#stretch-fit-sizing>.
/// Resolves CSS Sizing Level 4 stretch-fit sizing to a content-box length.
///
/// `stretch` attempts to make the margin box fill the available space. The
/// result is a CSS content-box length in Quire's PDF-point layout scalar, with
/// padding and border represented as an explicit semantic non-content length:
/// <https://drafts.csswg.org/css-sizing-4/#stretch-fit-sizing> and
/// <https://www.w3.org/TR/css-sizing-3/#box-sizing>.
pub(in crate::layout) fn stretch_fit_content_box_size(
    available_margin_box_size: LayoutLength,
    margin_size: LayoutLength,
    non_content_size: NonContentLength,
) -> ContentBoxLength {
    content_box_pt(
        (available_margin_box_size.points() - margin_size.points() - non_content_size.points())
            .max(0.0),
    )
}

/// Resolves a computed gap for flex layout.
///
/// CSS Box Alignment defines `normal` gaps as zero for flex containers and
/// resolves percentage gaps against the corresponding content box dimension:
/// <https://www.w3.org/TR/css-align-3/#gaps>.
pub(in crate::layout) fn used_flex_gap<Source>(
    value: css::ComputedGap,
    percentage_basis: PercentageBasis<ContentBoxLength, Source>,
) -> LayoutLength {
    used_flex_gap_with_basis(value, percentage_basis)
}

/// Resolves a flex gap against a definite or indefinite percentage basis.
///
/// CSS Box Alignment treats the percentage component of a cyclic gap as zero
/// when the relevant flex axis is indefinite, while preserving any
/// non-percentage length component:
/// <https://www.w3.org/TR/css-align-3/#gap-percent>.
pub(in crate::layout) fn used_flex_gap_with_basis<Source>(
    value: css::ComputedGap,
    percentage_basis: PercentageBasis<ContentBoxLength, Source>,
) -> LayoutLength {
    match value {
        css::ComputedGap::Normal => layout_pt(0.0),
        css::ComputedGap::LengthPercentage(value) => percentage_basis
            .points()
            .map(|basis| {
                used_length_percentage(
                    value.clone(),
                    PercentageBasis::definite(layout_pt(basis.max(0.0))),
                )
            })
            .unwrap_or_else(|| value.length_max_zero()),
    }
}

/// Resolves a computed column gap for multi-column layout.
///
/// CSS Multi-column Layout defines `column-gap: normal` as `1em`; CSS Box
/// Alignment supplies the shared length-percentage gap syntax:
/// <https://www.w3.org/TR/css-multicol-1/#cgap> and
/// <https://www.w3.org/TR/css-align-3/#gaps>.
pub(in crate::layout) fn used_multicol_column_gap<Source>(
    value: css::ComputedGap,
    percentage_basis: PercentageBasis<ContentBoxLength, Source>,
    font_size: f32,
) -> LayoutLength {
    match value {
        css::ComputedGap::Normal => layout_pt(font_size.max(0.0)),
        css::ComputedGap::LengthPercentage(value) => percentage_basis
            .points()
            .map(|basis| {
                used_length_percentage(
                    value.clone(),
                    PercentageBasis::definite(layout_pt(basis.max(0.0))),
                )
            })
            .unwrap_or_else(|| value.length_max_zero()),
    }
}

/// Resolves the number of columns for the current multi-column formatting context.
///
/// CSS Multi-column Layout derives the used column count from `column-count`,
/// `column-width`, the available inline size, and the used column gap:
/// <https://www.w3.org/TR/css-multicol-1/#pseudo-algorithm>.
pub(in crate::layout) fn used_multicol_column_count(
    style: &ComputedStyle,
    available_width: f32,
    gap: f32,
) -> Option<usize> {
    let specified_count = style.column_count.filter(|count| *count > 0);
    let specified_width = match &style.column_width {
        css::ComputedColumnWidth::Auto => None,
        css::ComputedColumnWidth::Length(width) => {
            width.length_if_no_percent().filter(|width| *width > 0.0)
        }
    };
    match (specified_count, specified_width) {
        (None, None) if matches!(style.column_height, css::ComputedColumnHeight::Length(_)) => {
            Some(1)
        }
        (None, None) => None,
        (Some(count), None) => Some(count),
        (count, Some(width)) => {
            let fitting_count = ((available_width + gap) / (width + gap)).floor().max(1.0) as usize;
            Some(count.map_or(fitting_count, |count| count.min(fitting_count)))
        }
    }
}

/// Return the intrinsic inline sizes contributed by a size-contained multicol.
///
/// Size containment ignores the contents of the principal box, but it does
/// not erase the multicol formatting context's authored column widths and
/// gaps. With no content contribution, a definite `column-width` and maximum
/// `column-count` form both intrinsic inline sizes. An automatic column width
/// contributes zero per column, but gaps between an authored number of
/// columns remain part of the formatting context's intrinsic geometry.
/// <https://www.w3.org/TR/css-contain-1/#containment-size>
/// <https://www.w3.org/TR/css-multicol-1/#pseudo-algorithm>
pub(in crate::layout) fn size_contained_multicol_intrinsic_inline_sizes(
    style: &ComputedStyle,
) -> Option<(f32, f32)> {
    if !intrinsic_inline_size_is_contained(style) {
        return None;
    }
    let column_width = match &style.column_width {
        css::ComputedColumnWidth::Auto => 0.0,
        css::ComputedColumnWidth::Length(column_width) => column_width
            .length_if_no_percent()
            .filter(|width| *width > 0.0)
            .unwrap_or(0.0),
    };
    let count = style.column_count.unwrap_or(1).max(1);
    let gap = used_multicol_column_gap(
        style.column_gap.clone(),
        PercentageBasis::definite(content_box_pt(0.0)),
        style.font_size,
    )
    .points();
    let inline_size = column_width * count as f32 + gap * count.saturating_sub(1) as f32;
    Some((inline_size, inline_size))
}

/// Whether CSS containment suppresses intrinsic contributions on a box's
/// logical inline axis.
///
/// `size` contains both axes; `inline-size` contains this axis only.
/// <https://drafts.csswg.org/css-contain-3/#inline-size-containment>
pub(in crate::layout) fn intrinsic_inline_size_is_contained(style: &ComputedStyle) -> bool {
    style.contain.size || style.contain.inline_size
}

/// Whether containment suppresses this box's physical-width contribution.
///
/// Physical `width` is logical inline size only in horizontal writing. In an
/// orthogonal flow it is logical block size, which `inline-size` containment
/// must leave available to ancestors.
/// <https://drafts.csswg.org/css-writing-modes-4/#logical-to-physical>
pub(in crate::layout) fn intrinsic_physical_width_is_contained(style: &ComputedStyle) -> bool {
    style.contain.size
        || (style.contain.inline_size && style.writing_mode == WritingMode::HorizontalTb)
}

/// Whether containment suppresses this box's physical-height contribution.
///
/// This is the counterpart to [`intrinsic_physical_width_is_contained`]. An
/// inline-size-contained vertical writing-mode box has its physical height on
/// its logical inline axis, while its physical width remains a block-axis
/// contribution.
/// <https://drafts.csswg.org/css-contain-3/#inline-size-containment>
/// <https://drafts.csswg.org/css-writing-modes-4/#logical-to-physical>
pub(in crate::layout) fn intrinsic_physical_height_is_contained(style: &ComputedStyle) -> bool {
    style.contain.size
        || (style.contain.inline_size && style.writing_mode != WritingMode::HorizontalTb)
}

/// Return the authored intrinsic fallback on the logical inline axis.
///
/// The computed `contain-intrinsic-size` longhands are physical, while
/// `inline-size` containment is logical, so the selected component follows
/// the element writing mode at this boundary.
pub(in crate::layout) fn contained_intrinsic_logical_inline_size(
    style: &ComputedStyle,
) -> Option<css::ComputedLengthPercentage> {
    match style.writing_mode {
        WritingMode::HorizontalTb => style.contain_intrinsic_size.width.clone(),
        WritingMode::VerticalRl
        | WritingMode::VerticalLr
        | WritingMode::SidewaysRl
        | WritingMode::SidewaysLr => style.contain_intrinsic_size.height.clone(),
    }
}

/// Resolves used padding edges for the current containing block.
///
/// CSS 2.2 says padding percentages on all sides refer to the containing
/// block's width:
/// <https://www.w3.org/TR/CSS22/box.html#padding-properties>.
pub(in crate::layout) fn used_padding_edges(
    style: &ComputedStyle,
    inline_basis: PercentageBasis<LayoutLength>,
) -> UsedEdges {
    let padding = style.box_values.padding.clone();
    UsedEdges {
        top: used_padding_edge(padding.top, style.padding.top, inline_basis),
        right: used_padding_edge(padding.right, style.padding.right, inline_basis),
        bottom: used_padding_edge(padding.bottom, style.padding.bottom, inline_basis),
        left: used_padding_edge(padding.left, style.padding.left, inline_basis),
    }
}

/// Resolves one padding edge, using the typed percentage component when present.
///
/// CSS 2.2 padding percentages resolve against the containing block width:
/// <https://www.w3.org/TR/CSS22/box.html#padding-properties>.
/// CSS Sizing resolves cyclic percentage contributions against zero during
/// intrinsic sizing, while preserving fixed lengths in the same calculation:
/// <https://drafts.csswg.org/css-sizing/#cyclic-percentage-contribution>.
pub(in crate::layout) fn used_padding_edge(
    value: css::ComputedLengthPercentage,
    legacy_length: f32,
    basis: PercentageBasis<LayoutLength>,
) -> LayoutLength {
    css::clamp_used_layout_length(layout_pt(
        value
            .length_if_no_percent()
            .map(|_| legacy_length)
            .unwrap_or_else(|| used_length_percentage(value, basis).points())
            .max(0.0),
    ))
}

/// Resolves used margin edges for the current containing block.
///
/// CSS 2.2 says margin percentages on all sides refer to the containing block's
/// width. Auto margins are resolved by the formatting context; this helper
/// returns zero for auto edges when a caller only needs occupied non-auto
/// margin space:
/// <https://www.w3.org/TR/CSS22/box.html#margin-properties>.
pub(in crate::layout) fn used_margin_edges(
    style: &ComputedStyle,
    inline_basis: PercentageBasis<LayoutLength>,
) -> UsedEdges {
    let margin = style.box_values.margin.clone();
    UsedEdges {
        top: used_margin_edge(margin.top, style.margin.top, inline_basis),
        right: used_margin_edge(margin.right, style.margin.right, inline_basis),
        bottom: used_margin_edge(margin.bottom, style.margin.bottom, inline_basis),
        left: used_margin_edge(margin.left, style.margin.left, inline_basis),
    }
}

/// Resolves padding edges for intrinsic size contributions.
///
/// CSS Sizing defines intrinsic size contributions in terms of the box's outer
/// size, but cyclic percentages in margins and padding resolve against zero
/// for those contributions. This helper uses computed padding values rather
/// than any cached used edges from a previous layout pass:
/// <https://www.w3.org/TR/css-sizing-3/#intrinsic-contribution>.
pub(in crate::layout) fn intrinsic_padding_edges(style: &ComputedStyle) -> UsedEdges {
    let padding = style.box_values.padding.clone();
    UsedEdges {
        top: intrinsic_padding_edge(padding.top),
        right: intrinsic_padding_edge(padding.right),
        bottom: intrinsic_padding_edge(padding.bottom),
        left: intrinsic_padding_edge(padding.left),
    }
}

/// Resolves one padding edge for intrinsic size contributions.
///
/// CSS Sizing resolves cyclic percentage padding contributions against zero,
/// preserving fixed length components in the same value:
/// <https://www.w3.org/TR/css-sizing-3/#intrinsic-contribution>.
pub(in crate::layout) fn intrinsic_padding_edge(
    value: css::ComputedLengthPercentage,
) -> LayoutLength {
    used_length_percentage(value, PercentageBasis::definite(layout_pt(0.0))).max(layout_pt(0.0))
}

/// Resolves margin edges for intrinsic size contributions.
///
/// CSS Sizing bases intrinsic contributions on outer size, treats auto margins
/// as zero, and resolves cyclic percentage margin contributions against zero:
/// <https://www.w3.org/TR/css-sizing-3/#intrinsic-contribution>.
pub(in crate::layout) fn intrinsic_margin_edges(style: &ComputedStyle) -> UsedEdges {
    let margin = style.box_values.margin.clone();
    UsedEdges {
        top: intrinsic_margin_edge(margin.top),
        right: intrinsic_margin_edge(margin.right),
        bottom: intrinsic_margin_edge(margin.bottom),
        left: intrinsic_margin_edge(margin.left),
    }
}

/// Resolves one margin edge for intrinsic size contributions.
///
/// CSS Sizing resolves cyclic percentage margin contributions against zero and
/// treats auto margins as zero:
/// <https://www.w3.org/TR/css-sizing-3/#intrinsic-contribution>.
pub(in crate::layout) fn intrinsic_margin_edge(
    value: css::ComputedLengthPercentageOrAuto,
) -> LayoutLength {
    match value {
        css::ComputedLengthPercentageOrAuto::LengthPercentage(value) => {
            used_length_percentage(value, PercentageBasis::definite(layout_pt(0.0)))
        }
        css::ComputedLengthPercentageOrAuto::Auto
        | css::ComputedLengthPercentageOrAuto::MinContent
        | css::ComputedLengthPercentageOrAuto::MaxContent
        | css::ComputedLengthPercentageOrAuto::FitContent(_)
        | css::ComputedLengthPercentageOrAuto::Stretch
        | css::ComputedLengthPercentageOrAuto::CalcSize(_) => layout_pt(0.0),
    }
}

/// Resolves one margin edge, preserving formatting-context handling for `auto`.
///
/// CSS 2.2 margin percentages resolve against the containing block width:
/// <https://www.w3.org/TR/CSS22/box.html#margin-properties>.
/// CSS Sizing resolves cyclic percentage contributions against zero during
/// intrinsic sizing, while preserving fixed lengths in the same calculation:
/// <https://drafts.csswg.org/css-sizing/#cyclic-percentage-contribution>.
pub(in crate::layout) fn used_margin_edge(
    value: css::ComputedLengthPercentageOrAuto,
    legacy_length: f32,
    basis: PercentageBasis<LayoutLength>,
) -> LayoutLength {
    match value {
        css::ComputedLengthPercentageOrAuto::Auto => layout_pt(0.0),
        css::ComputedLengthPercentageOrAuto::LengthPercentage(value) => value
            .length_if_no_percent()
            .map(|_| layout_pt(legacy_length))
            .unwrap_or_else(|| used_length_percentage(value, basis)),
        css::ComputedLengthPercentageOrAuto::MinContent
        | css::ComputedLengthPercentageOrAuto::MaxContent
        | css::ComputedLengthPercentageOrAuto::FitContent(_)
        | css::ComputedLengthPercentageOrAuto::Stretch
        | css::ComputedLengthPercentageOrAuto::CalcSize(_) => layout_pt(legacy_length),
    }
}

/// Synchronize resolved fixed box edges with the renderer's legacy edge cache.
///
/// The typed computed box values retain selected-font metric units until the
/// used-value stage. Inline box edges are consumed immediately after that
/// resolution, while older layout code still reads the scalar edge cache for
/// fixed values. Project only fixed values here; percentage edges must remain
/// deferred to their containing-block basis.
/// <https://www.w3.org/TR/css-values-4/#font-relative-lengths> and
/// <https://www.w3.org/TR/CSS22/box.html#margin-properties>
pub(in crate::layout) fn synchronize_resolved_fixed_box_edge_cache(style: &mut ComputedStyle) {
    let margin = style.box_values.margin.clone();
    let padding = style.box_values.padding.clone();
    if let Some(value) = margin.top.length_if_no_percent() {
        style.margin.top = value;
    }
    if let Some(value) = margin.right.length_if_no_percent() {
        style.margin.right = value;
    }
    if let Some(value) = margin.bottom.length_if_no_percent() {
        style.margin.bottom = value;
    }
    if let Some(value) = margin.left.length_if_no_percent() {
        style.margin.left = value;
    }
    if let Some(value) = padding.top.length_if_no_percent() {
        style.padding.top = value;
    }
    if let Some(value) = padding.right.length_if_no_percent() {
        style.padding.right = value;
    }
    if let Some(value) = padding.bottom.length_if_no_percent() {
        style.padding.bottom = value;
    }
    if let Some(value) = padding.left.length_if_no_percent() {
        style.padding.left = value;
    }
}

/// Remove an item's margins after its parent layout algorithm has already
/// positioned its margin box.
///
/// Flex and Grid replay an item through an independent normal-flow formatting
/// context after their layout algorithms consume the item's margins. Clear
/// both representations because normal-flow used-value resolution rebuilds
/// the scalar edge cache from the typed computed values.
/// <https://www.w3.org/TR/css-flexbox-1/#layout-algorithm> and
/// <https://www.w3.org/TR/css-grid-1/#grid-item-placement>.
pub(in crate::layout) fn suppress_replayed_item_margins(style: &mut ComputedStyle) {
    style.margin = css::Edges::ZERO;
    style.box_values.margin = css::CssEdges::all(css::ComputedLengthPercentageOrAuto::ZERO);
}

/// Freezes an item's already-resolved physical padding for normal-flow replay.
///
/// Flex and Grid resolve padding percentages against the containing block's
/// logical inline size before placing an item. Replaying the item against its
/// own content box must retain those used edges instead of resolving the
/// percentage against a different basis.
/// <https://www.w3.org/TR/css-box-3/#padding-physical> and
/// <https://www.w3.org/TR/css-flexbox-1/#layout-algorithm>
pub(in crate::layout) fn freeze_replayed_item_padding(
    style: &mut ComputedStyle,
    padding: css::Edges,
) {
    style.padding = padding;
    style.box_values.padding = css::CssEdges {
        top: css::ComputedLengthPercentage::from_points(padding.top),
        right: css::ComputedLengthPercentage::from_points(padding.right),
        bottom: css::ComputedLengthPercentage::from_points(padding.bottom),
        left: css::ComputedLengthPercentage::from_points(padding.left),
    };
}

/// Resolves both margin and padding edges for a box.
///
/// CSS 2.2 defines the used-value resolution for margin and padding:
/// <https://www.w3.org/TR/CSS22/box.html#margin-properties> and
/// <https://www.w3.org/TR/CSS22/box.html#padding-properties>.
pub(in crate::layout) fn used_box_edges(
    style: &ComputedStyle,
    inline_basis: PercentageBasis<LayoutLength>,
) -> UsedBoxEdges {
    UsedBoxEdges {
        margin: used_margin_edges(style, inline_basis),
        padding: used_padding_edges(style, inline_basis),
    }
}

/// Resolves margin and padding edges for intrinsic size contributions.
///
/// CSS Sizing's cyclic-percentage contribution rules are distinct from final
/// used-value resolution, so intrinsic sizing callers must not reuse normal
/// used edges resolved against a concrete containing block:
/// <https://www.w3.org/TR/css-sizing-3/#intrinsic-contribution>.
pub(in crate::layout) fn intrinsic_box_edges(style: &ComputedStyle) -> UsedBoxEdges {
    UsedBoxEdges {
        margin: intrinsic_margin_edges(style),
        padding: intrinsic_padding_edges(style),
    }
}

/// Return used box metrics without mutating the caller's style.
///
/// This is useful for intrinsic sizing paths that need resolved non-content
/// edges but must not overwrite the computed style carried by a formatting box.
/// CSS separates computed and used values:
/// <https://www.w3.org/TR/css-cascade-5/#value-stages>.
pub(in crate::layout) fn used_box_metrics(
    style: &ComputedStyle,
    inline_basis: PercentageBasis<LayoutLength>,
) -> UsedBoxMetrics {
    let used_edges = used_box_edges(style, inline_basis);
    UsedBoxMetrics {
        margin: used_edges.margin.to_css_edges(),
        padding: used_edges.padding.to_css_edges(),
        border: used_border_widths(style),
    }
}

/// Return box metrics for intrinsic size contributions.
///
/// The returned margin and padding edges are resolved from computed values with
/// a zero cyclic-percentage basis, while borders remain ordinary used border
/// widths:
/// <https://www.w3.org/TR/css-sizing-3/#intrinsic-contribution>.
pub(in crate::layout) fn intrinsic_box_metrics(style: &ComputedStyle) -> UsedBoxMetrics {
    let intrinsic_edges = intrinsic_box_edges(style);
    UsedBoxMetrics {
        margin: intrinsic_edges.margin.to_css_edges(),
        padding: intrinsic_edges.padding.to_css_edges(),
        border: used_border_widths(style),
    }
}

/// Resolve a temporary layout style's margin and padding and return box metrics.
///
/// Layout code often needs both the mutated style, so later code can consume
/// used edge values, and the derived non-content sizes. Keeping those steps
/// together avoids call sites accidentally mixing computed and used edges:
/// <https://www.w3.org/TR/css-cascade-5/#used>.
pub(in crate::layout) fn apply_used_box_metrics(
    style: &mut ComputedStyle,
    inline_basis: PercentageBasis<LayoutLength>,
) -> UsedBoxMetrics {
    let metrics = used_box_metrics(style, inline_basis);
    style.margin = metrics.margin;
    style.padding = metrics.padding;
    metrics
}

/// Resolved horizontal geometry for a normal-flow block-level box.
///
/// CSS 2.2 defines one equation for the used inline margins, borders, padding,
/// and content width of block-level non-replaced boxes in normal flow. Keeping
/// the result together avoids callers mixing a content width resolved against
/// one basis with a border box positioned from another:
/// <https://www.w3.org/TR/CSS22/visudet.html#blockwidth>.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(in crate::layout) struct NormalFlowBlockWidth {
    pub(in crate::layout) content_width: ContentBoxLength,
    pub(in crate::layout) border_box_width: BorderBoxLength,
    pub(in crate::layout) border_box_x: f32,
}

/// Width inputs for resolving a block container's requested content size.
///
/// `available_outer_width` is the margin-adjusted stretch-fit size used by
/// `auto` and intrinsic sizing keywords. `percentage_basis` is the containing
/// block inline size used by length-percentage properties:
/// <https://www.w3.org/TR/CSS22/visudet.html#blockwidth>.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(in crate::layout) struct BlockContentWidthInputs {
    pub(in crate::layout) available_outer_width: LayoutLength,
    pub(in crate::layout) percentage_basis: PercentageBasis<LayoutLength>,
    pub(in crate::layout) horizontal_non_content: NonContentLength,
}

/// Return the outer available inline size used by `width:auto` block boxes.
///
/// CSS 2.2 resolves margin percentages against the containing block width, but
/// `width:auto` itself follows the block-width equation after non-auto margins
/// have their used values. Negative margins therefore increase this available
/// space rather than being clamped away:
/// <https://www.w3.org/TR/CSS22/visudet.html#blockwidth> and
/// <https://www.w3.org/TR/CSS22/box.html#margin-properties>.
pub(in crate::layout) fn normal_flow_block_available_outer_width(
    style: &ComputedStyle,
    containing_inline_size: LayoutLength,
) -> LayoutLength {
    layout_pt(containing_inline_size.points() - style.margin.left - style.margin.right)
}

/// Resolve content width, border-box width, and border-box x for a normal block.
///
/// Percentages in `width`, `min-width`, and `max-width` use the containing
/// block width as their percentage basis. Only `width:auto` consumes the
/// margin-adjusted space from the CSS 2.2 block-width equation:
/// <https://www.w3.org/TR/CSS22/visudet.html#blockwidth>.
pub(in crate::layout) fn resolve_normal_flow_block_width(
    style: &mut ComputedStyle,
    containing_left: f32,
    containing_right: f32,
    requested_content_width: PhysicalContentWidth,
    horizontal_non_content: NonContentLength,
    containing_direction: Direction,
    resolve_auto_margins: bool,
) -> NormalFlowBlockWidth {
    let containing_inline_size = (containing_right - containing_left).max(0.0);
    let content_width = constrain_width_with_stretch_fit(
        style,
        requested_content_width.content_box_length(),
        layout_pt(containing_inline_size),
        layout_pt(style.margin.left + style.margin.right),
        horizontal_non_content,
    );
    let border_box_width = content_box_to_border_box_length(content_width, horizontal_non_content);
    if resolve_auto_margins {
        resolve_normal_flow_block_auto_margins(
            style,
            containing_inline_size,
            border_box_width.points(),
            containing_direction,
        );
    }
    let border_box_x = normal_flow_block_outer_x(
        containing_left,
        containing_right,
        style,
        border_box_width.points(),
        containing_direction,
    );

    NormalFlowBlockWidth {
        content_width,
        border_box_width,
        border_box_x,
    }
}

/// Resolve the requested content width for a normal-flow block-level box.
///
/// Lengths and percentages use the containing block as their percentage basis,
/// while `auto` fills the margin-adjusted available space. This preserves
/// negative margins as required by the CSS 2.2 block-width equation:
/// <https://www.w3.org/TR/CSS22/visudet.html#blockwidth>.
pub(in crate::layout) fn used_normal_flow_block_content_box_width(
    style: &ComputedStyle,
    containing_inline_size: LayoutLength,
    horizontal_non_content: NonContentLength,
) -> ContentBoxLength {
    used_content_box_size(
        style.box_values.width.clone(),
        style.box_sizing,
        PercentageBasis::definite(content_box_pt(containing_inline_size.points())),
        horizontal_non_content,
    )
    .unwrap_or_else(|| {
        content_box_pt(
            (normal_flow_block_available_outer_width(style, containing_inline_size).points()
                - horizontal_non_content.points())
            .max(0.0),
        )
    })
}

/// Resolve horizontal `auto` margins for a normal-flow block with a used width.
///
/// CSS 2.2 defines the block width equation over horizontal margins, borders,
/// padding, and width. Once the used border-box width is known, auto horizontal
/// margins absorb remaining inline space; when no horizontal margin is auto,
/// the over-constrained side is handled during positioning:
/// <https://www.w3.org/TR/CSS22/visudet.html#blockwidth>.
pub(in crate::layout) fn resolve_normal_flow_block_auto_margins(
    style: &mut ComputedStyle,
    containing_inline_size: f32,
    border_box_width: f32,
    containing_direction: Direction,
) {
    let left_auto = style.box_values.margin.clone().left.is_auto();
    let right_auto = style.box_values.margin.clone().right.is_auto();
    if has_auto_width(style) || (!left_auto && !right_auto) {
        return;
    }

    resolve_normal_flow_auto_margins_for_known_width(
        style,
        containing_inline_size,
        border_box_width,
        containing_direction,
    );
}

/// Resolve horizontal `auto` margins when the formatting context has already
/// resolved a concrete border-box width.
///
/// CSS table wrappers with `width:auto` can shrink-wrap to their final grid
/// width, so they need the same CSS 2.2 block-width auto-margin equation after
/// table width resolution rather than normal block auto-width fill. When the
/// equation is over-constrained, CSS first treats any auto horizontal margins
/// as zero and then ignores the containing block's end-side margin:
/// <https://www.w3.org/TR/CSS22/visudet.html#blockwidth> and
/// <https://drafts.csswg.org/css-tables-3/#computing-the-table-width>.
pub(in crate::layout) fn resolve_normal_flow_auto_margins_for_known_width(
    style: &mut ComputedStyle,
    containing_inline_size: f32,
    border_box_width: f32,
    containing_direction: Direction,
) {
    let left_auto = style.box_values.margin.clone().left.is_auto();
    let right_auto = style.box_values.margin.clone().right.is_auto();
    if !left_auto && !right_auto {
        return;
    }

    let free_space =
        containing_inline_size - style.margin.left - border_box_width - style.margin.right;
    if free_space < 0.0 {
        if left_auto {
            style.margin.left = 0.0;
        }
        if right_auto {
            style.margin.right = 0.0;
        }
        match containing_direction {
            Direction::Ltr => {
                style.margin.right = containing_inline_size - style.margin.left - border_box_width;
            }
            Direction::Rtl => {
                style.margin.left = containing_inline_size - border_box_width - style.margin.right;
            }
        }
    } else if left_auto && right_auto {
        style.margin.left = free_space / 2.0;
        style.margin.right = free_space / 2.0;
    } else if left_auto {
        style.margin.left = free_space;
    } else if right_auto {
        style.margin.right = free_space;
    }
}

/// Return the normal-flow block border-box left edge after margin resolution.
///
/// CSS 2.2 block-width resolution treats a fixed-width block with no `auto`
/// horizontal inputs as over-constrained when the equation does not balance.
/// In that case the ignored side depends on the containing block's
/// `direction`: `margin-right` is ignored for LTR and `margin-left` for RTL.
/// Given an already resolved border box width, this helper positions the box
/// from the side that is not ignored:
/// <https://www.w3.org/TR/CSS22/visudet.html#blockwidth>.
pub(in crate::layout) fn normal_flow_block_outer_x(
    containing_left: f32,
    containing_right: f32,
    style: &ComputedStyle,
    border_box_width: f32,
    containing_direction: Direction,
) -> f32 {
    match containing_direction {
        Direction::Ltr => containing_left + style.margin.left,
        Direction::Rtl => containing_right - style.margin.right - border_box_width,
    }
}

/// Returns whether `width` is computed as `auto`.
///
/// CSS 2.2 block width calculations depend on whether `width` is `auto`:
/// <https://www.w3.org/TR/CSS22/visudet.html#blockwidth>.
pub(in crate::layout) fn has_auto_width(style: &ComputedStyle) -> bool {
    style.box_values.width.clone().is_auto()
}

/// Returns whether `height` is computed as `auto`.
///
/// CSS 2.2 block height calculations depend on whether `height` is `auto`:
/// <https://www.w3.org/TR/CSS22/visudet.html#normal-block>.
pub(in crate::layout) fn has_auto_height(style: &ComputedStyle) -> bool {
    style.box_values.height.clone().is_auto()
}

/// Resolves used content width, falling back to filling available space for `auto`.
///
/// CSS 2.2 defines block-width used-value resolution and CSS Box Sizing defines
/// how `box-sizing` changes the content-box size:
/// <https://www.w3.org/TR/CSS22/visudet.html#blockwidth> and
/// <https://www.w3.org/TR/css-sizing-3/#box-sizing>.
pub(in crate::layout) fn used_content_box_width(
    style: &ComputedStyle,
    available_outer_width: LayoutLength,
    horizontal_non_content: NonContentLength,
) -> ContentBoxLength {
    used_content_box_size(
        style.box_values.width.clone(),
        style.box_sizing,
        PercentageBasis::definite(crate::units::layout_to_content_box_length(
            available_outer_width,
        )),
        horizontal_non_content,
    )
    .unwrap_or_else(|| {
        content_box_pt((available_outer_width.points() - horizontal_non_content.points()).max(0.0))
    })
}

/// Resolves a specified content width against a typed physical availability.
///
/// CSS percentage widths use the containing block's inline size; this entry
/// point preserves the resulting content-box quantity until the caller enters
/// a coordinate or rendering algorithm.
pub(in crate::layout) fn used_content_box_width_or_auto(
    style: &ComputedStyle,
    available_outer_width: LayoutLength,
    horizontal_non_content: NonContentLength,
) -> Option<ContentBoxLength> {
    used_content_box_size(
        style.box_values.width.clone(),
        style.box_sizing,
        PercentageBasis::definite(crate::units::layout_to_content_box_length(
            available_outer_width,
        )),
        horizontal_non_content,
    )
}

/// Resolves a specified content width against a typed percentage basis.
pub(in crate::layout) fn used_content_box_width_or_auto_with_basis<Source>(
    style: &ComputedStyle,
    available_outer_width: PercentageBasis<ContentBoxLength, Source>,
    horizontal_non_content: NonContentLength,
) -> Option<ContentBoxLength> {
    used_content_box_size_with_basis(
        style.box_values.width.clone(),
        style.box_sizing,
        available_outer_width,
        horizontal_non_content,
    )
}

/// Resolves a specified content height against a typed physical availability.
///
/// CSS percentage heights use the containing block's block-size basis; an
/// automatic height remains unresolved for its formatting context to handle.
pub(in crate::layout) fn used_content_box_height_or_auto(
    style: &ComputedStyle,
    available_outer_height: LayoutLength,
    vertical_non_content: NonContentLength,
) -> Option<ContentBoxLength> {
    used_content_box_size(
        style.box_values.height.clone(),
        style.box_sizing,
        PercentageBasis::definite(crate::units::layout_to_content_box_length(
            available_outer_height,
        )),
        vertical_non_content,
    )
}

/// Resolves a specified content height against a typed percentage basis.
pub(in crate::layout) fn used_content_box_height_or_auto_with_basis<Source>(
    style: &ComputedStyle,
    available_outer_height: PercentageBasis<ContentBoxLength, Source>,
    vertical_non_content: NonContentLength,
) -> Option<ContentBoxLength> {
    used_content_box_size_with_basis(
        style.box_values.height.clone(),
        style.box_sizing,
        available_outer_height,
        vertical_non_content,
    )
}

/// Transfer a definite non-replaced content width through its preferred aspect
/// ratio to obtain an automatic content height.
///
/// The ratio applies in the box defined by `box-sizing`; converting both
/// dimensions through the content box keeps borders and padding from being
/// counted asymmetrically. Constraint application remains with the formatting
/// context because its percentage basis and intrinsic contributions are
/// context-dependent.
/// <https://www.w3.org/TR/css-sizing-4/#aspect-ratio>
pub(in crate::layout) fn non_replaced_aspect_ratio_content_height(
    style: &ComputedStyle,
    content_width: f32,
    horizontal_non_content: f32,
    vertical_non_content: f32,
) -> Option<f32> {
    let calc_size = style.box_values.height.clone().calc_size_with_auto_basis();
    if !style.box_values.height.clone().is_auto() && calc_size.is_none() {
        return None;
    }
    let ratio = style.aspect_ratio.preferred_ratio_for_non_replaced(false)?;
    if ratio <= 0.0 || !ratio.is_finite() {
        return None;
    }
    let height = match (
        style.box_sizing,
        style.aspect_ratio.uses_content_box_for_non_replaced(),
    ) {
        (_, true) | (BoxSizing::ContentBox, false) => content_width / ratio,
        (BoxSizing::BorderBox, false) => {
            let border_box_width = content_width + horizontal_non_content;
            (border_box_width / ratio) - vertical_non_content
        }
    };
    Some(
        calc_size
            .map(|value| {
                value
                    .used_value(
                        height,
                        0.0,
                        0.0,
                        0.0,
                        0.0,
                        PercentageBasis::definite(layout_pt(0.0)),
                    )
                    .points()
            })
            .unwrap_or(height)
            .max(0.0),
    )
}

/// Transfer a definite non-replaced content height through its preferred
/// aspect ratio to obtain an automatic content width.
///
/// This is the reciprocal of [`non_replaced_aspect_ratio_content_height`];
/// keeping both conversions here makes each formatting context choose a used
/// size without reimplementing `box-sizing` arithmetic.
/// <https://www.w3.org/TR/css-sizing-4/#aspect-ratio>
pub(in crate::layout) fn non_replaced_aspect_ratio_content_width(
    style: &ComputedStyle,
    content_height: f32,
    horizontal_non_content: f32,
    vertical_non_content: f32,
) -> Option<f32> {
    let calc_size = style.box_values.width.clone().calc_size_with_auto_basis();
    if !style.box_values.width.clone().is_auto() && calc_size.is_none() {
        return None;
    }
    let ratio = style.aspect_ratio.preferred_ratio_for_non_replaced(false)?;
    if ratio <= 0.0 || !ratio.is_finite() {
        return None;
    }
    let width = match (
        style.box_sizing,
        style.aspect_ratio.uses_content_box_for_non_replaced(),
    ) {
        (_, true) | (BoxSizing::ContentBox, false) => content_height * ratio,
        (BoxSizing::BorderBox, false) => {
            let border_box_height = content_height + vertical_non_content;
            (border_box_height * ratio) - horizontal_non_content
        }
    };
    Some(
        calc_size
            .map(|value| {
                value
                    .used_value(
                        width,
                        0.0,
                        0.0,
                        0.0,
                        0.0,
                        PercentageBasis::definite(layout_pt(0.0)),
                    )
                    .points()
            })
            .unwrap_or(width)
            .max(0.0),
    )
}

/// Resolves a width/height value to a typed content-box used size.
///
/// CSS Box Sizing defines conversion between border-box and content-box sizes:
/// <https://www.w3.org/TR/css-sizing-3/#box-sizing>.
///
/// The returned value is a CSS content-box length in Quire's PDF-point layout
/// scalar. Callers should keep this typed until they cross a layout/paint or
/// external adapter boundary.
pub(in crate::layout) fn used_content_box_size<Source>(
    value: css::ComputedLengthPercentageOrAuto,
    box_sizing: BoxSizing,
    percentage_basis: PercentageBasis<ContentBoxLength, Source>,
    non_content: NonContentLength,
) -> Option<ContentBoxLength> {
    used_content_box_size_with_basis(value, box_sizing, percentage_basis, non_content)
}

/// Resolves a width/height value to a typed content-box used size.
///
/// CSS Sizing treats pure lengths as definite without a percentage basis, while
/// percentage sizes are definite only when the containing block axis is
/// definite. CSS Box Sizing then maps the specified content-box or border-box
/// value into the content-box coordinate space:
/// <https://www.w3.org/TR/css-sizing-3/#definite> and
/// <https://www.w3.org/TR/css-sizing-3/#box-sizing>.
pub(in crate::layout) fn used_content_box_size_with_basis<Source>(
    value: css::ComputedLengthPercentageOrAuto,
    box_sizing: BoxSizing,
    percentage_basis: PercentageBasis<ContentBoxLength, Source>,
    non_content: NonContentLength,
) -> Option<ContentBoxLength> {
    let specified = match value {
        css::ComputedLengthPercentageOrAuto::Auto => return None,
        css::ComputedLengthPercentageOrAuto::Stretch => {
            return percentage_basis.points().map(|basis| {
                stretch_fit_content_box_size(layout_pt(basis), layout_pt(0.0), non_content)
            });
        }
        css::ComputedLengthPercentageOrAuto::LengthPercentage(value) => {
            layout_points(used_length_percentage_with_basis(value, percentage_basis)?)
        }
        css::ComputedLengthPercentageOrAuto::MinContent
        | css::ComputedLengthPercentageOrAuto::MaxContent
        | css::ComputedLengthPercentageOrAuto::FitContent(_)
        | css::ComputedLengthPercentageOrAuto::CalcSize(_) => return None,
    };
    Some(used_content_box_size_from_specified(
        specified,
        box_sizing,
        non_content,
    ))
}

fn used_content_box_size_from_specified(
    specified: f32,
    box_sizing: BoxSizing,
    non_content: NonContentLength,
) -> ContentBoxLength {
    match box_sizing {
        BoxSizing::BorderBox => {
            border_box_to_content_box_length(border_box_pt(specified), non_content)
        }
        BoxSizing::ContentBox => content_box_pt(specified.max(0.0)),
    }
}

/// Resolves used `min-width`.
///
/// CSS 2.2 defines min/max width constraints:
/// <https://www.w3.org/TR/CSS22/visudet.html#min-max-widths>.
pub(in crate::layout) fn used_min_width<T, Source>(
    style: &ComputedStyle,
    percentage_basis: PercentageBasis<T, Source>,
) -> Option<ContentBoxLength>
where
    T: SemanticLengthExt,
{
    used_length_percentage_or_auto(style.box_values.min_width.clone(), percentage_basis)
        .map(|value| content_box_pt(value.points().max(0.0)))
}

/// Resolves used `max-width`.
///
/// CSS 2.2 defines min/max width constraints:
/// <https://www.w3.org/TR/CSS22/visudet.html#min-max-widths>.
pub(in crate::layout) fn used_max_width<T, Source>(
    style: &ComputedStyle,
    percentage_basis: PercentageBasis<T, Source>,
) -> Option<ContentBoxLength>
where
    T: SemanticLengthExt,
{
    used_length_percentage_or_auto(style.box_values.max_width.clone(), percentage_basis)
        .map(|value| content_box_pt(value.points().max(0.0)))
}

/// Return whether a sizing value needs intrinsic min/max-content contributions.
///
/// CSS Sizing defines these keywords in terms of intrinsic size contributions,
/// so callers must measure contents before resolving them:
/// <https://www.w3.org/TR/css-sizing-3/#sizing-values>.
pub(in crate::layout) fn needs_intrinsic_width_contribution(
    value: css::ComputedLengthPercentageOrAuto,
) -> bool {
    matches!(
        value,
        css::ComputedLengthPercentageOrAuto::MinContent
            | css::ComputedLengthPercentageOrAuto::MaxContent
            | css::ComputedLengthPercentageOrAuto::FitContent(_)
    ) || matches!(&value, css::ComputedLengthPercentageOrAuto::CalcSize(value) if value.needs_intrinsic_size())
        // A percentage-dependent width has no used value during intrinsic
        // sizing. CSS Sizing therefore measures the contents instead of
        // prematurely using the fixed component of `calc()`, including an
        // authored `0%` component.
        // <https://www.w3.org/TR/css-sizing-3/#intrinsic-sizes>.
        || matches!(&value, css::ComputedLengthPercentageOrAuto::LengthPercentage(value) if value.needs_percentage_basis())
}

/// Return whether a block-axis sizing value needs intrinsic min/max-content contributions.
///
/// CSS Sizing defines block-axis `min-content` and `max-content` sizes in
/// terms of the content's intrinsic block size. Normal block containers have
/// equivalent min-content and max-content block sizes, but callers still need
/// the laid-out content height before resolving these keywords:
/// <https://www.w3.org/TR/css-sizing-3/#sizing-values> and
/// <https://github.com/w3c/csswg-drafts/issues/3973>.
pub(in crate::layout) fn needs_intrinsic_height_contribution(
    value: css::ComputedLengthPercentageOrAuto,
) -> bool {
    matches!(
        value,
        css::ComputedLengthPercentageOrAuto::MinContent
            | css::ComputedLengthPercentageOrAuto::MaxContent
            | css::ComputedLengthPercentageOrAuto::FitContent(_)
    ) || matches!(value, css::ComputedLengthPercentageOrAuto::CalcSize(value) if value.needs_intrinsic_size())
}

/// Resolves used `min-height`.
///
/// CSS 2.2 defines min/max height constraints:
/// <https://www.w3.org/TR/CSS22/visudet.html#min-max-heights>.
pub(in crate::layout) fn used_min_height<T, Source>(
    style: &ComputedStyle,
    percentage_basis: PercentageBasis<T, Source>,
) -> Option<ContentBoxLength>
where
    T: SemanticLengthExt,
{
    used_length_percentage_or_auto(style.box_values.min_height.clone(), percentage_basis)
        .map(|value| content_box_pt(value.points().max(0.0)))
}

/// Resolves used `max-height`.
///
/// CSS 2.2 defines min/max height constraints:
/// <https://www.w3.org/TR/CSS22/visudet.html#min-max-heights>.
pub(in crate::layout) fn used_max_height<T, Source>(
    style: &ComputedStyle,
    percentage_basis: PercentageBasis<T, Source>,
) -> Option<ContentBoxLength>
where
    T: SemanticLengthExt,
{
    used_length_percentage_or_auto(style.box_values.max_height.clone(), percentage_basis)
        .map(|value| content_box_pt(value.points().max(0.0)))
}

/// Applies non-intrinsic used min/max width constraints to a content width.
///
/// CSS 2.2 defines min/max width constraint application. Intrinsic sizing
/// keywords need content measurements and are intentionally ignored here; use
/// `constrain_width_with_intrinsic` when min/max-content contributions are
/// available:
/// <https://www.w3.org/TR/CSS22/visudet.html#min-max-widths> and
/// <https://www.w3.org/TR/css-sizing-3/#sizing-values>.
pub(in crate::layout) trait ConstraintPercentageBasis {
    fn into_layout_basis(self) -> PercentageBasis<LayoutLength>;
}

impl<T, Source> ConstraintPercentageBasis for PercentageBasis<T, Source>
where
    T: crate::units::IntoLayoutLength,
{
    fn into_layout_basis(self) -> PercentageBasis<LayoutLength> {
        match self {
            PercentageBasis::Definite { value, .. } => {
                PercentageBasis::definite(crate::units::IntoLayoutLength::into_layout_length(value))
            }
            PercentageBasis::Indefinite => PercentageBasis::indefinite(),
        }
    }
}

/// Applies used width constraints to a typed content-box size.
///
pub(in crate::layout) fn constrain_content_width<B>(
    style: &ComputedStyle,
    value: ContentBoxLength,
    percentage_basis: B,
) -> ContentBoxLength
where
    B: ConstraintPercentageBasis,
{
    let percentage_basis = percentage_basis.into_layout_basis();
    content_box_pt(constrain(
        value.points(),
        used_min_width(style, percentage_basis).map(SemanticLengthExt::points),
        used_max_width(style, percentage_basis).map(SemanticLengthExt::points),
    ))
}

/// Apply width constraints, resolving `stretch` against the available margin
/// box rather than treating it as an ordinary length.
///
/// CSS Sizing defines stretch-fit sizing on the margin box. Normal-flow block
/// width resolution reaches this point after margin values are known, which is
/// the shared boundary where a stretch min/max constraint can retain that
/// distinction from a content-box length.
/// <https://drafts.csswg.org/css-sizing-4/#stretch-fit-sizing>
pub(in crate::layout) fn constrain_width_with_stretch_fit(
    style: &ComputedStyle,
    value: ContentBoxLength,
    available_margin_box_width: LayoutLength,
    horizontal_margin: LayoutLength,
    horizontal_non_content: NonContentLength,
) -> ContentBoxLength {
    let stretch = || {
        stretch_fit_content_box_size(
            available_margin_box_width,
            horizontal_margin,
            horizontal_non_content,
        )
        .points()
    };
    let min = if style.box_values.min_width == css::ComputedLengthPercentageOrAuto::Stretch {
        Some(stretch())
    } else {
        used_min_width(style, PercentageBasis::definite(available_margin_box_width))
            .map(SemanticLengthExt::points)
    };
    let max = if style.box_values.max_width == css::ComputedLengthPercentageOrAuto::Stretch {
        Some(stretch())
    } else {
        used_max_width(style, PercentageBasis::definite(available_margin_box_width))
            .map(SemanticLengthExt::points)
    };
    content_box_pt(constrain(value.points(), min, max))
}

/// Applies min/max width constraints, including intrinsic sizing keywords.
///
/// CSS Sizing defines `min-content`, `max-content`, and `fit-content()` in
/// terms of a box's intrinsic contributions. This helper keeps the tentative
/// width in content-box space and converts definite constraint arguments
/// through `box-sizing` before clamping:
/// <https://www.w3.org/TR/css-sizing-3/#sizing-values>,
/// <https://www.w3.org/TR/css-sizing-3/#fit-content-size>, and
/// <https://www.w3.org/TR/css-sizing-3/#box-sizing>.
pub(in crate::layout) fn constrain_width_with_intrinsic<Source>(
    style: &ComputedStyle,
    value: ContentBoxLength,
    min_content: ContentBoxLength,
    max_content: ContentBoxLength,
    percentage_basis: PercentageBasis<ContentBoxLength, Source>,
    horizontal_non_content: NonContentLength,
) -> ContentBoxLength {
    let min_content = content_box_pt(min_content.points().max(0.0));
    let max_content = content_box_pt(max_content.points().max(min_content.points()).max(0.0));
    let percentage_basis = match percentage_basis {
        PercentageBasis::Definite { value, .. } => PercentageBasis::definite(value),
        PercentageBasis::Indefinite => PercentageBasis::indefinite(),
    };
    let min_constraint = intrinsic_width_constraint(
        style.box_values.min_width.clone(),
        style.box_sizing,
        percentage_basis,
        horizontal_non_content,
        min_content,
        max_content,
    );
    let max_constraint = intrinsic_width_constraint(
        style.box_values.max_width.clone(),
        style.box_sizing,
        percentage_basis,
        horizontal_non_content,
        min_content,
        max_content,
    );
    content_box_pt(
        constrain(
            value.points().max(0.0),
            min_constraint.map(SemanticLengthExt::points),
            max_constraint.map(SemanticLengthExt::points),
        )
        .max(0.0),
    )
}

pub(in crate::layout) fn intrinsic_width_constraint(
    value: css::ComputedLengthPercentageOrAuto,
    box_sizing: BoxSizing,
    percentage_basis: PercentageBasis<ContentBoxLength>,
    horizontal_non_content: NonContentLength,
    min_content: ContentBoxLength,
    max_content: ContentBoxLength,
) -> Option<ContentBoxLength> {
    match value {
        css::ComputedLengthPercentageOrAuto::Auto => None,
        css::ComputedLengthPercentageOrAuto::MinContent => Some(min_content),
        css::ComputedLengthPercentageOrAuto::MaxContent => Some(max_content),
        css::ComputedLengthPercentageOrAuto::FitContent(limit) => {
            let stretch = limit
                .map(|limit| {
                    intrinsic_constraint_length(
                        limit,
                        box_sizing,
                        percentage_basis,
                        horizontal_non_content,
                    )
                })
                .unwrap_or_else(|| {
                    percentage_basis.points().map(|basis| {
                        stretch_fit_content_box_size(
                            layout_pt(basis),
                            layout_pt(0.0),
                            horizontal_non_content,
                        )
                    })
                })?;
            Some(content_box_pt(
                max_content
                    .points()
                    .min(min_content.points().max(stretch.points()))
                    .max(0.0),
            ))
        }
        css::ComputedLengthPercentageOrAuto::LengthPercentage(value) => {
            intrinsic_constraint_length(value, box_sizing, percentage_basis, horizontal_non_content)
        }
        css::ComputedLengthPercentageOrAuto::Stretch => percentage_basis.points().map(|basis| {
            stretch_fit_content_box_size(layout_pt(basis), layout_pt(0.0), horizontal_non_content)
        }),
        css::ComputedLengthPercentageOrAuto::CalcSize(value) => calc_size_intrinsic_constraint(
            value,
            box_sizing,
            percentage_basis,
            horizontal_non_content,
            min_content,
            max_content,
        ),
    }
}

fn intrinsic_constraint_length(
    value: css::ComputedLengthPercentage,
    box_sizing: BoxSizing,
    percentage_basis: PercentageBasis<ContentBoxLength>,
    horizontal_non_content: NonContentLength,
) -> Option<ContentBoxLength> {
    // Intrinsic max-size contributions use an indefinite basis for cyclic
    // percentages. An authored `0%` remains a percentage in CSS syntax, so
    // `calc(45px + 0%)` is just as cyclic as `50%` here and max-width must
    // fall back to its initial value rather than impose a 45px cap. Numeric
    // resolution alone cannot express that distinction after canonicalizing
    // a length-percentage expression.
    // <https://www.w3.org/TR/css-sizing-3/#intrinsic-contribution>
    if !percentage_basis.is_definite() && value.contains_percentage() {
        return None;
    }
    let specified = used_length_percentage_with_basis(value, percentage_basis)?.points();
    Some(used_content_box_size_from_specified(
        specified,
        box_sizing,
        horizontal_non_content,
    ))
}

/// Resolves a calc-size constraint after intrinsic contributions are known.
///
/// Intrinsic sizing supplies a zero percentage basis for cyclic percentages,
/// while the retained calc-size basis is selected from the same content-box
/// contributions as CSS Sizing's intrinsic keywords. The calculation itself
/// happens in the property's specified box space before CSS box-sizing maps it
/// back to the content box:
/// <https://drafts.csswg.org/css-values-5/#calc-size> and
/// <https://www.w3.org/TR/css-sizing-3/#intrinsic-contribution>.
pub(in crate::layout) fn calc_size_intrinsic_constraint(
    value: css::CalcSize,
    box_sizing: BoxSizing,
    percentage_basis: PercentageBasis<ContentBoxLength>,
    non_content: NonContentLength,
    min_content: ContentBoxLength,
    max_content: ContentBoxLength,
) -> Option<ContentBoxLength> {
    let percentage_basis_points = percentage_basis.points().unwrap_or(0.0);
    let to_specified = |content: ContentBoxLength| match box_sizing {
        BoxSizing::ContentBox => content.points(),
        BoxSizing::BorderBox => content.points() + non_content.points(),
    };
    let min_content = to_specified(min_content);
    let max_content = to_specified(max_content).max(min_content);
    let stretch = stretch_fit_content_box_size(
        layout_pt(percentage_basis_points),
        layout_pt(0.0),
        non_content,
    )
    .points();
    let fit_content = max_content.min(min_content.max(stretch));
    let basis = match &value.basis {
        css::CalcSizeBasis::Auto | css::CalcSizeBasis::MinContent => min_content,
        css::CalcSizeBasis::MaxContent => max_content,
        css::CalcSizeBasis::FitContent => fit_content,
        css::CalcSizeBasis::Stretch => stretch,
        css::CalcSizeBasis::LengthPercentage(value) => {
            used_length_percentage_with_basis(value.clone(), percentage_basis)?.points()
        }
    };
    Some(used_content_box_size_from_specified(
        value
            .used_value(
                basis,
                min_content,
                max_content,
                fit_content,
                stretch,
                percentage_basis,
            )
            .max(layout_pt(0.0))
            .points(),
        box_sizing,
        non_content,
    ))
}

/// Return non-replaced intrinsic inline-size contributions in content-box space.
///
/// CSS Sizing defines intrinsic contributions for non-replaced boxes by
/// treating cyclic percentage preferred and max sizes as their initial values
/// while resolving min-size percentages against zero. Preferred sizes such as
/// `fit-content(<length-percentage>)` are applied to the measured content
/// contribution before min/max constraints:
/// <https://www.w3.org/TR/css-sizing-3/#intrinsic-contribution> and
/// <https://www.w3.org/TR/css-sizing-3/#valdef-width-fit-content-length-percentage>.
pub(in crate::layout) fn non_replaced_intrinsic_width_contributions(
    style: &ComputedStyle,
    min_content: ContentBoxLength,
    max_content: ContentBoxLength,
    horizontal_non_content: NonContentLength,
) -> (ContentBoxLength, ContentBoxLength) {
    let intrinsic_min_content = content_box_pt(min_content.points().max(0.0));
    let intrinsic_max_content = content_box_pt(
        max_content
            .points()
            .max(intrinsic_min_content.points())
            .max(0.0),
    );
    let preferred_width = non_replaced_intrinsic_preferred_width(
        style.box_values.width.clone(),
        style.box_sizing,
        horizontal_non_content,
        intrinsic_min_content,
        intrinsic_max_content,
    );
    // Constraints select their `calc-size()` intrinsic bases from the raw
    // content contributions. A ratio-derived preferred width is instead the
    // tentative size to which those constraints apply. Conflating the two
    // would make `min-width: calc-size(auto, size - 50px)` use the unbounded
    // raw contribution as the final intrinsic contribution.
    // <https://drafts.csswg.org/css-sizing-3/#intrinsic-contribution> and
    // <https://drafts.csswg.org/css-values-5/#calc-size>.
    // CSS Sizing resolves cyclic percentage min sizes against zero, while a
    // cyclic percentage preferred or max size is treated as its initial value.
    // Keep those distinct at the typed percentage-basis boundary: fixed
    // components still resolve through `Indefinite`, but max-width and
    // fit-content percentage components remain absent instead of becoming a
    // spurious zero cap.
    // <https://www.w3.org/TR/css-sizing-3/#intrinsic-contribution>
    let intrinsic_min_percentage_basis = PercentageBasis::definite(content_box_pt(0.0));
    let intrinsic_max_percentage_basis = PercentageBasis::indefinite();
    let min_constraint = intrinsic_width_constraint(
        style.box_values.min_width.clone(),
        style.box_sizing,
        intrinsic_min_percentage_basis,
        horizontal_non_content,
        intrinsic_min_content,
        intrinsic_max_content,
    )
    .map(SemanticLengthExt::points);
    let max_constraint = intrinsic_width_constraint(
        style.box_values.max_width.clone(),
        style.box_sizing,
        intrinsic_max_percentage_basis,
        horizontal_non_content,
        intrinsic_min_content,
        intrinsic_max_content,
    )
    .map(SemanticLengthExt::points);
    // A definite preferred block size transfers through a preferred aspect
    // ratio when computing a non-replaced box's intrinsic inline
    // contributions.  Resolve block min/max constraints first, in content-box
    // space, so `box-sizing: border-box` and padding affect the transferred
    // inline size exactly once.
    // <https://drafts.csswg.org/css-sizing-4/#aspect-ratio>
    let transferred_aspect_width = (!matches!(
        style.box_values.width.clone(),
        css::ComputedLengthPercentageOrAuto::LengthPercentage(_)
    ))
    .then(|| {
        let vertical_non_content = intrinsic_box_metrics(style).vertical_non_content_length();
        used_content_box_size(
            style.box_values.height.clone(),
            style.box_sizing,
            PercentageBasis::definite(content_box_pt(0.0)),
            vertical_non_content,
        )
        .map(|height| {
            constrain_height_with_intrinsic(
                style,
                height,
                height,
                height,
                PercentageBasis::definite(content_box_pt(0.0)),
                vertical_non_content,
            )
            .points()
        })
        .and_then(|height| {
            non_replaced_aspect_ratio_content_width(
                style,
                height,
                horizontal_non_content.points(),
                vertical_non_content.points(),
            )
        })
    })
    .flatten();
    let preferred_min_content = preferred_width
        .map(|value| value.0)
        .unwrap_or(intrinsic_min_content)
        .points();
    let preferred_max_content = preferred_width
        .map(|value| value.1)
        .unwrap_or(intrinsic_max_content)
        .points();
    // A definite block size and automatic inline size select the aspect-ratio
    // transfer as the inline preferred size. The content contribution remains
    // available above solely for the automatic min-size constraint.
    // <https://drafts.csswg.org/css-sizing-4/#aspect-ratio>.
    let (min_content, max_content) = if style.box_values.width.clone().is_auto() {
        transferred_aspect_width
            .map(|width| (width, width))
            .unwrap_or((preferred_min_content, preferred_max_content))
    } else {
        (
            preferred_min_content.max(transferred_aspect_width.unwrap_or(0.0)),
            preferred_max_content.max(transferred_aspect_width.unwrap_or(0.0)),
        )
    };
    (
        content_box_pt(constrain(min_content, min_constraint, max_constraint).max(0.0)),
        content_box_pt(constrain(max_content, min_constraint, max_constraint).max(0.0)),
    )
}

/// Apply non-replaced intrinsic inline-size constraints to content widths.
///
/// Transitional raw compatibility wrapper for
/// `non_replaced_intrinsic_width_contributions`.
pub(in crate::layout) fn constrain_non_replaced_intrinsic_widths(
    style: &ComputedStyle,
    min_content: f32,
    max_content: f32,
    horizontal_non_content: f32,
) -> (f32, f32) {
    let (min_content, max_content) = non_replaced_intrinsic_width_contributions(
        style,
        content_box_pt(min_content),
        content_box_pt(max_content),
        non_content_pt(horizontal_non_content),
    );
    (min_content.points(), max_content.points())
}

fn non_replaced_intrinsic_preferred_width(
    value: css::ComputedLengthPercentageOrAuto,
    box_sizing: BoxSizing,
    horizontal_non_content: NonContentLength,
    min_content: ContentBoxLength,
    max_content: ContentBoxLength,
) -> Option<(ContentBoxLength, ContentBoxLength)> {
    match value {
        css::ComputedLengthPercentageOrAuto::LengthPercentage(value) => {
            non_cyclic_content_box_length(value, box_sizing, horizontal_non_content)
                .map(|value| (value, value))
        }
        css::ComputedLengthPercentageOrAuto::MinContent => Some((min_content, min_content)),
        css::ComputedLengthPercentageOrAuto::MaxContent => Some((max_content, max_content)),
        css::ComputedLengthPercentageOrAuto::FitContent(Some(limit)) => {
            non_replaced_intrinsic_fit_content_width(
                limit,
                box_sizing,
                horizontal_non_content,
                min_content,
                max_content,
            )
            .map(|value| (value, value))
        }
        css::ComputedLengthPercentageOrAuto::CalcSize(value) => {
            let min_content_points = min_content.points();
            let max_content_points = max_content.points();
            let intrinsic_basis = |min_size: f32, max_size: f32| match &value.basis {
                css::CalcSizeBasis::Auto
                | css::CalcSizeBasis::FitContent
                | css::CalcSizeBasis::MinContent => (min_size, max_size),
                css::CalcSizeBasis::MaxContent => (max_size, max_size),
                css::CalcSizeBasis::Stretch => (0.0, 0.0),
                css::CalcSizeBasis::LengthPercentage(basis) => {
                    let basis = used_length_percentage(
                        basis.clone(),
                        PercentageBasis::definite(layout_pt(0.0)),
                    )
                    .points();
                    (basis, basis)
                }
            };
            let (min_basis, max_basis) = intrinsic_basis(min_content_points, max_content_points);
            let min_specified = value
                .used_value(
                    min_basis,
                    min_content_points,
                    max_content_points,
                    min_basis,
                    0.0,
                    PercentageBasis::definite(layout_pt(0.0)),
                )
                .points();
            let max_specified = value
                .used_value(
                    max_basis,
                    min_content_points,
                    max_content_points,
                    max_basis,
                    0.0,
                    PercentageBasis::definite(layout_pt(0.0)),
                )
                .points();
            Some((
                used_content_box_size_from_specified(
                    min_specified.max(0.0),
                    box_sizing,
                    horizontal_non_content,
                ),
                used_content_box_size_from_specified(
                    max_specified.max(0.0),
                    box_sizing,
                    horizontal_non_content,
                ),
            ))
        }
        css::ComputedLengthPercentageOrAuto::Auto
        | css::ComputedLengthPercentageOrAuto::FitContent(None)
        | css::ComputedLengthPercentageOrAuto::Stretch => None,
    }
}

fn non_replaced_intrinsic_fit_content_width(
    limit: css::ComputedLengthPercentage,
    box_sizing: BoxSizing,
    horizontal_non_content: NonContentLength,
    min_content: ContentBoxLength,
    max_content: ContentBoxLength,
) -> Option<ContentBoxLength> {
    let limit = non_cyclic_content_box_length(limit, box_sizing, horizontal_non_content)?;
    Some(content_box_pt(
        max_content
            .points()
            .min(min_content.points().max(limit.points()))
            .max(0.0),
    ))
}

fn non_cyclic_content_box_length(
    value: css::ComputedLengthPercentage,
    box_sizing: BoxSizing,
    horizontal_non_content: NonContentLength,
) -> Option<ContentBoxLength> {
    let specified = value.length_if_no_percent()?;
    Some(used_content_box_size_from_specified(
        specified,
        box_sizing,
        horizontal_non_content,
    ))
}

/// Applies used min/max height constraints to a content height.
///
/// CSS 2.2 defines min/max height constraint application:
/// <https://www.w3.org/TR/CSS22/visudet.html#min-max-heights>.
/// Applies used height constraints to a typed content-box size.
///
pub(in crate::layout) fn constrain_content_height<B>(
    style: &ComputedStyle,
    value: ContentBoxLength,
    percentage_basis: B,
) -> ContentBoxLength
where
    B: ConstraintPercentageBasis,
{
    let percentage_basis = percentage_basis.into_layout_basis();
    let vertical_non_content = intrinsic_box_metrics(style).vertical_non_content_length();
    let content_box_constraint = |value: ContentBoxLength| {
        used_content_box_size_from_specified(
            value.points().max(0.0),
            style.box_sizing,
            vertical_non_content,
        )
        .points()
    };
    content_box_pt(constrain(
        value.points(),
        used_min_height(style, percentage_basis).map(content_box_constraint),
        used_max_height(style, percentage_basis).map(content_box_constraint),
    ))
}

/// Apply block-size constraints, resolving `stretch` against the available
/// containing-block size before margins are applied.
/// <https://drafts.csswg.org/css-sizing-4/#stretch-fit-sizing>
pub(in crate::layout) fn constrain_height_with_stretch_fit(
    style: &ComputedStyle,
    value: ContentBoxLength,
    available_margin_box_height: LayoutLength,
    _vertical_margin: LayoutLength,
    vertical_non_content: NonContentLength,
) -> ContentBoxLength {
    let stretch = || {
        stretch_fit_content_box_size(
            available_margin_box_height,
            layout_pt(0.0),
            vertical_non_content,
        )
        .points()
    };
    let min = if style.box_values.min_height == css::ComputedLengthPercentageOrAuto::Stretch {
        Some(stretch())
    } else {
        used_min_height(
            style,
            PercentageBasis::definite(available_margin_box_height),
        )
        .map(|value| {
            used_content_box_size_from_specified(
                value.points(),
                style.box_sizing,
                vertical_non_content,
            )
            .points()
        })
    };
    let max = if style.box_values.max_height == css::ComputedLengthPercentageOrAuto::Stretch {
        Some(stretch())
    } else {
        used_max_height(
            style,
            PercentageBasis::definite(available_margin_box_height),
        )
        .map(|value| {
            used_content_box_size_from_specified(
                value.points(),
                style.box_sizing,
                vertical_non_content,
            )
            .points()
        })
    };
    content_box_pt(constrain(value.points(), min, max))
}

/// Applies min/max height constraints, including intrinsic sizing keywords.
///
/// CSS 2.2 applies `min-height`/`max-height` by recalculating the used height
/// with the constraining property substituted for `height`. CSS Sizing extends
/// those properties with intrinsic keywords; for normal block containers the
/// min-content and max-content block sizes are both the laid-out content block
/// size measured with cyclic block-axis percentages unresolved:
/// <https://www.w3.org/TR/CSS22/visudet.html#min-max-heights>,
/// <https://www.w3.org/TR/css-sizing-3/#sizing-values>, and
/// <https://www.w3.org/TR/css-sizing-3/#percentage-sizing>.
pub(in crate::layout) fn constrain_height_with_intrinsic<Source>(
    style: &ComputedStyle,
    value: ContentBoxLength,
    min_content: ContentBoxLength,
    max_content: ContentBoxLength,
    percentage_basis: PercentageBasis<ContentBoxLength, Source>,
    vertical_non_content: NonContentLength,
) -> ContentBoxLength {
    let min_content = content_box_pt(min_content.points().max(0.0));
    let max_content = content_box_pt(max_content.points().max(min_content.points()).max(0.0));
    let percentage_basis = match percentage_basis {
        PercentageBasis::Definite { value, .. } => PercentageBasis::definite(value),
        PercentageBasis::Indefinite => PercentageBasis::indefinite(),
    };
    let min_constraint = intrinsic_height_constraint(
        style.box_values.min_height.clone(),
        style.box_sizing,
        percentage_basis,
        vertical_non_content,
        min_content,
        max_content,
    );
    let max_constraint = intrinsic_height_constraint(
        style.box_values.max_height.clone(),
        style.box_sizing,
        percentage_basis,
        vertical_non_content,
        min_content,
        max_content,
    );
    content_box_pt(
        constrain(
            value.points().max(0.0),
            min_constraint.map(SemanticLengthExt::points),
            max_constraint.map(SemanticLengthExt::points),
        )
        .max(0.0),
    )
}

fn intrinsic_height_constraint(
    value: css::ComputedLengthPercentageOrAuto,
    box_sizing: BoxSizing,
    percentage_basis: PercentageBasis<ContentBoxLength>,
    vertical_non_content: NonContentLength,
    min_content: ContentBoxLength,
    max_content: ContentBoxLength,
) -> Option<ContentBoxLength> {
    match value {
        css::ComputedLengthPercentageOrAuto::Auto => None,
        css::ComputedLengthPercentageOrAuto::MinContent => Some(min_content),
        css::ComputedLengthPercentageOrAuto::MaxContent => Some(max_content),
        css::ComputedLengthPercentageOrAuto::FitContent(limit) => {
            let stretch = limit
                .map(|limit| {
                    intrinsic_constraint_length(
                        limit,
                        box_sizing,
                        percentage_basis,
                        vertical_non_content,
                    )
                })
                .unwrap_or_else(|| {
                    percentage_basis.points().map(|basis| {
                        stretch_fit_content_box_size(
                            layout_pt(basis),
                            layout_pt(0.0),
                            vertical_non_content,
                        )
                    })
                })?;
            Some(content_box_pt(
                max_content
                    .points()
                    .min(min_content.points().max(stretch.points()))
                    .max(0.0),
            ))
        }
        css::ComputedLengthPercentageOrAuto::LengthPercentage(value) => {
            intrinsic_constraint_length(value, box_sizing, percentage_basis, vertical_non_content)
        }
        css::ComputedLengthPercentageOrAuto::Stretch => percentage_basis.points().map(|basis| {
            stretch_fit_content_box_size(layout_pt(basis), layout_pt(0.0), vertical_non_content)
        }),
        css::ComputedLengthPercentageOrAuto::CalcSize(value) => calc_size_intrinsic_constraint(
            value,
            box_sizing,
            percentage_basis,
            vertical_non_content,
            min_content,
            max_content,
        ),
    }
}

/// Resolves the physical `left` inset for positioned layout.
///
/// CSS 2.2 positioned layout defines offsets and percentage bases:
/// <https://www.w3.org/TR/CSS22/visuren.html#position-props>.
pub(in crate::layout) fn used_inset_left(
    style: &ComputedStyle,
    containing_block: ContainingBlock,
) -> Option<f32> {
    used_length_percentage_or_auto(
        style.box_values.inset_left.clone(),
        PercentageBasis::definite(layout_pt(containing_block.width())),
    )
    .map(SemanticLengthExt::points)
}

/// Resolves the physical `right` inset for positioned layout.
///
/// CSS 2.2 positioned layout defines offsets and percentage bases:
/// <https://www.w3.org/TR/CSS22/visuren.html#position-props>.
pub(in crate::layout) fn used_inset_right(
    style: &ComputedStyle,
    containing_block: ContainingBlock,
) -> Option<f32> {
    used_length_percentage_or_auto(
        style.box_values.inset_right.clone(),
        PercentageBasis::definite(layout_pt(containing_block.width())),
    )
    .map(SemanticLengthExt::points)
}

/// Resolves the physical `top` inset for positioned layout.
///
/// CSS 2.2 positioned layout defines offsets and percentage bases:
/// <https://www.w3.org/TR/CSS22/visuren.html#position-props>.
pub(in crate::layout) fn used_inset_top(
    style: &ComputedStyle,
    containing_block: ContainingBlock,
) -> Option<f32> {
    used_length_percentage_or_auto(
        style.box_values.inset_top.clone(),
        PercentageBasis::definite(layout_pt(containing_block.height())),
    )
    .map(SemanticLengthExt::points)
}

/// Resolves the physical `bottom` inset for positioned layout.
///
/// CSS 2.2 positioned layout defines offsets and percentage bases:
/// <https://www.w3.org/TR/CSS22/visuren.html#position-props>.
pub(in crate::layout) fn used_inset_bottom(
    style: &ComputedStyle,
    containing_block: ContainingBlock,
) -> Option<f32> {
    used_length_percentage_or_auto(
        style.box_values.inset_bottom.clone(),
        PercentageBasis::definite(layout_pt(containing_block.height())),
    )
    .map(SemanticLengthExt::points)
}

/// Replaces computed width with a definite used width for a temporary layout style.
///
/// CSS Cascade separates computed values from used values:
/// <https://www.w3.org/TR/css-cascade-5/#value-stages>.
pub(in crate::layout) fn set_style_used_width(style: &mut ComputedStyle, width: f32) {
    let width = width.max(0.0);
    style.box_values.width = css::ComputedLengthPercentageOrAuto::LengthPercentage(
        css::ComputedLengthPercentage::from_points(width),
    );
}

/// Replaces computed height with a definite used height for a temporary layout style.
///
/// CSS Cascade separates computed values from used values:
/// <https://www.w3.org/TR/css-cascade-5/#value-stages>.
pub(in crate::layout) fn set_style_used_height(style: &mut ComputedStyle, height: f32) {
    let height = height.max(0.0);
    style.box_values.height = css::ComputedLengthPercentageOrAuto::LengthPercentage(
        css::ComputedLengthPercentage::from_points(height),
    );
}

/// Freezes a temporary replay style to a resolved border-box width.
///
/// Flexbox resolves the item's border-box geometry before normal-flow replay.
/// The replay style uses `box-sizing: border-box`, so the CSS min/max values
/// must retain that same box-model space rather than subtracting its padding
/// and borders a second time:
/// <https://www.w3.org/TR/css-flexbox-1/#layout-algorithm> and
/// <https://www.w3.org/TR/css-sizing-3/#box-sizing>.
pub(in crate::layout) fn set_style_used_width_bounds(style: &mut ComputedStyle, width: f32) {
    let width = css::ComputedLengthPercentageOrAuto::LengthPercentage(
        css::ComputedLengthPercentage::from_points(width.max(0.0)),
    );
    style.box_values.min_width = width.clone();
    style.box_values.max_width = width;
}

/// Freezes a temporary replay style to a resolved border-box height.
///
/// The replay style uses `box-sizing: border-box`, so preserve the supplied
/// border-box size without a second non-content conversion.
/// <https://www.w3.org/TR/css-flexbox-1/#layout-algorithm> and
/// <https://www.w3.org/TR/css-sizing-3/#box-sizing>.
pub(in crate::layout) fn set_style_used_height_bounds(style: &mut ComputedStyle, height: f32) {
    let height = css::ComputedLengthPercentageOrAuto::LengthPercentage(
        css::ComputedLengthPercentage::from_points(height.max(0.0)),
    );
    style.box_values.min_height = height.clone();
    style.box_values.max_height = height;
}

/// Restores `width: auto` on a temporary layout style.
///
/// CSS 2.2 uses `auto` as the initial width value in normal flow:
/// <https://www.w3.org/TR/CSS22/visudet.html#the-width-property>.
pub(in crate::layout) fn set_style_auto_width(style: &mut ComputedStyle) {
    style.box_values.width = css::ComputedLengthPercentageOrAuto::Auto;
}

/// Restores `height: auto` on a temporary layout style.
///
/// CSS 2.2 uses `auto` as the initial height value in normal flow:
/// <https://www.w3.org/TR/CSS22/visudet.html#the-height-property>.
pub(in crate::layout) fn set_style_auto_height(style: &mut ComputedStyle) {
    style.box_values.height = css::ComputedLengthPercentageOrAuto::Auto;
}

/// Clears physical positioned offsets on a temporary layout style.
///
/// CSS 2.2 defines physical inset properties for positioned boxes:
/// <https://www.w3.org/TR/CSS22/visuren.html#position-props>.
pub(in crate::layout) fn clear_style_insets(style: &mut ComputedStyle) {
    style.box_values.inset_left = css::ComputedLengthPercentageOrAuto::Auto;
    style.box_values.inset_top = css::ComputedLengthPercentageOrAuto::Auto;
    style.box_values.inset_right = css::ComputedLengthPercentageOrAuto::Auto;
    style.box_values.inset_bottom = css::ComputedLengthPercentageOrAuto::Auto;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn length(points: f32) -> css::ComputedLengthPercentageOrAuto {
        css::ComputedLengthPercentageOrAuto::LengthPercentage(
            css::ComputedLengthPercentage::from_points(points),
        )
    }

    fn percent(percent: f32) -> css::ComputedLengthPercentageOrAuto {
        css::ComputedLengthPercentageOrAuto::LengthPercentage(
            css::ComputedLengthPercentage::from_percent(percent),
        )
    }

    #[test]
    fn stretch_fit_content_size_is_non_negative_after_borders() {
        let non_content = 200.0;
        let content = stretch_fit_content_box_size(
            layout_pt(0.0),
            layout_pt(0.0),
            non_content_pt(non_content),
        )
        .points();

        assert_eq!(content, 0.0);
        assert_eq!(content + non_content, 200.0);
    }

    #[test]
    fn used_content_box_size_keeps_content_box_specified_size() {
        let content = used_content_box_size(
            length(120.0),
            BoxSizing::ContentBox,
            PercentageBasis::definite(content_box_pt(300.0)),
            non_content_pt(40.0),
        )
        .expect("content-box length should resolve");

        assert_eq!(content.points(), 120.0);
    }

    #[test]
    fn used_content_box_size_subtracts_border_box_non_content_and_clamps() {
        let content = used_content_box_size(
            length(100.0),
            BoxSizing::BorderBox,
            PercentageBasis::definite(content_box_pt(300.0)),
            non_content_pt(150.0),
        )
        .expect("border-box length should resolve");

        assert_eq!(content.points(), 0.0);
    }

    #[test]
    fn stretch_fit_content_box_size_returns_typed_non_negative_content() {
        let content =
            stretch_fit_content_box_size(layout_pt(100.0), layout_pt(30.0), non_content_pt(120.0));

        assert_eq!(content.points(), 0.0);
    }

    #[test]
    fn used_content_box_size_indefinite_basis_resolves_lengths_not_percentages() {
        let length_content = used_content_box_size_with_basis(
            length(42.0),
            BoxSizing::ContentBox,
            PercentageBasis::<ContentBoxLength>::indefinite(),
            non_content_pt(10.0),
        )
        .expect("length-only value should resolve without a basis");
        let percentage_content = used_content_box_size_with_basis(
            percent(0.5),
            BoxSizing::ContentBox,
            PercentageBasis::<ContentBoxLength>::indefinite(),
            non_content_pt(10.0),
        );

        assert_eq!(length_content.points(), 42.0);
        assert!(percentage_content.is_none());
    }

    #[test]
    fn inline_size_containment_is_logical_for_physical_width_contributions() {
        let mut horizontal = ComputedStyle::initial();
        horizontal.contain.inline_size = true;
        assert!(intrinsic_inline_size_is_contained(&horizontal));
        assert!(intrinsic_physical_width_is_contained(&horizontal));

        let mut vertical = horizontal;
        vertical.writing_mode = WritingMode::VerticalRl;
        assert!(intrinsic_inline_size_is_contained(&vertical));
        assert!(!intrinsic_physical_width_is_contained(&vertical));
        assert!(intrinsic_physical_height_is_contained(&vertical));
    }
}

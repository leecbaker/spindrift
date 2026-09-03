use std::rc::Rc;

use super::*;
use crate::layout::block::{
    FloatContour, FlowExclusionKind, InitialLetterLayout, LogicalFloatPlacement,
};
use crate::layout::inline_layout::{InlineLineKind, InlineLineTermination};

/// Whether a graph's source context can reuse cached advances for a selected
/// line.
///
/// Source shaping is retained across a selected boundary, but CSS line
/// construction can still replace an edge, trim text, or attach a marker.
/// This context gate is deliberately shared by normal wrapping, balancing,
/// clamping, and float-constrained fitting so those selection modes cannot
/// disagree on when exact materialization is required.
/// <https://www.w3.org/TR/css-text-3/#line-breaking>
/// <https://www.w3.org/TR/css-text-3/#boundary-shaping>
fn source_measurement_context_matches_selected_line(
    graph: &InlineOpportunityGraph,
    block_style: &ComputedStyle,
    line_index: usize,
) -> bool {
    (line_index != 0 || block_style.first_line_style.is_none())
        && !block_style.hanging_punctuation.first
        && !block_style.hanging_punctuation.last
        && !block_style.hanging_punctuation.force_end
        && !block_style.hanging_punctuation.allow_end
        && graph.supports_monotonic_source_measurement()
}

/// Whether a selected graph boundary leaves the cached source advance as the
/// selected line's fitting advance.
fn source_measurement_boundary_matches_selected_line(
    break_opportunity: Option<InlineBreakOpportunity>,
) -> bool {
    break_opportunity.is_none_or(|opportunity| {
        matches!(
            opportunity.kind,
            InlineBreakKind::SoftWrap
                | InlineBreakKind::ExplicitVirtual
                | InlineBreakKind::PreservedSpace
                | InlineBreakKind::Forced
        ) && !opportunity.hangs_from_fitting_measure()
            && !opportunity.is_discretionary()
            && opportunity.discretionary.is_none()
            && !opportunity.availability.is_fallback()
    })
}

fn source_measurement_matches_selected_line(
    graph: &InlineOpportunityGraph,
    block_style: &ComputedStyle,
    line_index: usize,
    break_opportunity: Option<InlineBreakOpportunity>,
) -> bool {
    source_measurement_context_matches_selected_line(graph, block_style, line_index)
        && source_measurement_boundary_matches_selected_line(break_opportunity)
}

/// Establish the per-line used font-size for CSS Text fitting without
/// modifying the cascaded computed style. `normal` and unitless line heights
/// follow the used font size; an explicit length, including a percentage that
/// already computed to a length, remains fixed.
/// <https://drafts.csswg.org/css-text-5/#text-fit-property>
pub(in crate::layout) fn scale_text_fit_fragment_style(style: &mut ComputedStyle, scale: f32) {
    debug_assert!(scale.is_finite() && scale >= 0.0);
    style.font_size *= scale;
    if matches!(
        style.line_height_value,
        css::ComputedLineHeight::Normal | css::ComputedLineHeight::Number(_)
    ) {
        // Re-project from the fitted font size. The cached layout scalar can
        // originate before a descendant font shorthand or pseudo-style is
        // materialized, whereas the computed `normal`/unitless value is what
        // CSS Text fitting makes depend on the used font size.
        style.line_height = style.line_height_value.clone().projected(style.font_size).0;
    }
}

/// A validated non-negative scale selected for one `text-fit` block.
///
/// This is a used value, intentionally distinct from the computed CSS
/// percentage limit. It keeps invalid numeric results from escaping the
/// fitting analysis into shaping and line-box construction.
/// <https://drafts.csswg.org/css-text-5/#text-fit-property>
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd)]
pub(in crate::layout) struct TextFitScale(f32);

impl TextFitScale {
    const ONE: Self = Self(1.0);

    fn new(value: f32) -> Option<Self> {
        (value.is_finite() && value >= 0.0).then_some(Self(value))
    }

    pub(in crate::layout) fn factor(self) -> f32 {
        self.0
    }

    fn min(self, other: Self) -> Self {
        if self.0 <= other.0 { self } else { other }
    }
}

/// The first and last formatted records affected by CSS Text fitting.
/// `text-box` trims those two logical edges independently in per-line modes.
#[derive(Debug, Clone, Copy)]
pub(in crate::layout) struct TextFitAppliedRecords {
    pub(in crate::layout) first_formatted: usize,
    pub(in crate::layout) last_formatted: usize,
}

/// The parent block's text-fitting used values for one formatted line.
///
/// CSS Text fitting changes the used text font size without changing the
/// computed style. Keeping the resolved parent style with the selected line
/// prevents its strut, fragment metrics, trimming, and paint geometry from
/// observing different `line-height: normal` values.
/// <https://drafts.csswg.org/css-text-5/#text-fit-property>
#[derive(Debug, Clone)]
pub(in crate::layout) struct TextFitUsedLineStyle {
    scale: TextFitScale,
    block_style: Rc<ComputedStyle>,
    line_stack_strut: f32,
}

impl TextFitUsedLineStyle {
    pub(in crate::layout) fn scale(&self) -> TextFitScale {
        self.scale
    }

    pub(in crate::layout) fn block_style(&self) -> &ComputedStyle {
        &self.block_style
    }

    /// The scaled containing-block strut used to stack line records.
    ///
    /// This remains distinct from a selected font's `normal` metrics: those
    /// metrics position text within the line, while the strut determines the
    /// inline block's logical block-size.
    pub(in crate::layout) fn line_stack_strut(&self) -> f32 {
        self.line_stack_strut
    }
}

fn text_fit_applies_first_line_style(
    block_style: &ComputedStyle,
    is_first_formatted_line: bool,
) -> bool {
    is_first_formatted_line
        && block_style.first_line_style.is_some()
        && !block_style
            .first_letter_style
            .as_deref()
            .is_some_and(|style| !style.initial_letter.is_normal())
}

/// Select a valid text-fit used scale from two measurements of the same
/// selected line. The measurements respectively use factors one and two, so
/// their difference is the scalable advance while `2 * one - two` is the
/// fixed advance.
/// <https://drafts.csswg.org/css-text-5/#text-fit-property>
fn text_fit_scale_from_measurements(
    available_width: f32,
    one: f32,
    two: f32,
    direction: css::TextFitDirection,
    limit: Option<f32>,
) -> TextFitScale {
    if one <= f32::EPSILON || !one.is_finite() || !two.is_finite() {
        return TextFitScale::ONE;
    }
    let scalable = (two - one).max(0.0);
    if scalable <= f32::EPSILON || !scalable.is_finite() {
        return TextFitScale::ONE;
    }
    let fixed = (2.0 * one - two).max(0.0);
    let mut factor = ((available_width.max(0.0) - fixed).max(0.0) / scalable).max(0.0);
    factor = match direction {
        css::TextFitDirection::Grow => factor.max(1.0),
        css::TextFitDirection::Shrink => factor.min(1.0),
    };
    if let Some(limit) = limit {
        factor = match direction {
            css::TextFitDirection::Grow if limit >= 1.0 => factor.min(limit),
            css::TextFitDirection::Shrink if (0.0..=1.0).contains(&limit) => factor.max(limit),
            css::TextFitDirection::Grow | css::TextFitDirection::Shrink => factor,
        };
    }
    TextFitScale::new(factor).unwrap_or(TextFitScale::ONE)
}

fn text_fit_strategy_scales_record(
    strategy: css::TextFitStrategy,
    record_index: usize,
    last_formatted_record: usize,
    termination: InlineLineTermination,
) -> bool {
    match strategy {
        css::TextFitStrategy::Consistent | css::TextFitStrategy::PerLineAll => true,
        css::TextFitStrategy::PerLine => {
            record_index != last_formatted_record
                && termination != InlineLineTermination::ForcedBreak
        }
    }
}

/// Whether the selected final line must reserve a block ellipsis for source
/// that lies outside the current inline graph.
///
/// A `LineLimitTraversal` carries the fact through a descendant style, while a
/// collected inline sequence supplies it directly for a later preserved-break
/// paragraph. Keeping both sources here prevents graph-local suffix checks
/// from losing a block-flow continuation.
fn line_clamp_has_later_in_flow_content(context: InlineParagraphContext<'_>) -> bool {
    context.clamp_continuation == css::ClampContinuation::LaterInFlowContent
        || context.line_clamp.is_some_and(|line_clamp| {
            line_clamp.continuation() == css::ClampContinuation::LaterInFlowContent
        })
}

/// Classify the first source-order float in a candidate line range without
/// treating its zero-advance graph marker as a CSS Text break opportunity.
fn inline_float_affected_range(
    graph: &InlineOpportunityGraph,
    range: InlineGraphRange,
) -> InlineFloatAffectedRange {
    let Some(marker) = graph.first_float_position_in_range(range) else {
        return InlineFloatAffectedRange::None;
    };
    inline_float_marker_range(graph, marker)
}

fn inline_float_marker_range(
    graph: &InlineOpportunityGraph,
    marker: InlineGraphPosition,
) -> InlineFloatAffectedRange {
    let Some(float) = graph.float_at_position(marker) else {
        debug_assert!(false, "float graph position must own an inline float");
        return InlineFloatAffectedRange::None;
    };
    if float.style().allows_soft_wrap() {
        InlineFloatAffectedRange::Wrappable { marker }
    } else {
        InlineFloatAffectedRange::UnbreakableContinuation { marker }
    }
}

/// One normal or balanced selection in an inline graph.
///
/// CSS Text 4 balancing must use the same legal graph opportunities as normal
/// wrapping. Recording the start position makes the plan safe to apply only
/// while the real selector follows the same source stream.
/// <https://drafts.csswg.org/css-text-4/#text-wrap-style>
#[derive(Debug, Clone, Copy)]
struct BalancedLinePlanEntry {
    start: InlineGraphPosition,
    end: SelectedInlineLineEnd,
    line_index: usize,
    /// The clamp marker occupies this line without retaining any source.
    /// Keeping this in the plan prevents final clamp reconciliation from
    /// treating a deliberate marker-only endpoint as an ordinary no-fit line.
    clamp_marker_replaces_source: bool,
}

/// A source-order float that was provisionally placed only to expose the
/// remaining candidate stream. Once balancing has selected that stream, its
/// placement is replayed from the state immediately before the float.
struct PendingBalancedFloatReplay {
    position: InlineGraphPosition,
    source_line_index: usize,
    snapshot: LayoutSnapshot,
}

/// One complete candidate for a CSS Text Level 4 balanced line group.
///
/// The candidate retains the actual unused inline space of every line.  A
/// single shared target width is insufficient when floats, indents, or other
/// line-local geometry make the available measure differ between lines.
/// <https://drafts.csswg.org/css-text-4/#text-wrap-style>
#[derive(Debug, Clone)]
struct BalancedLineCandidate {
    entries: Vec<BalancedLinePlanEntry>,
    remaining_inline_space: Vec<f32>,
}

/// Immutable inputs shared by one bounded balance-candidate search.
#[derive(Clone, Copy)]
struct BalancedLineSearch<'a, 'b> {
    graph: &'a InlineOpportunityGraph,
    context: InlineParagraphContext<'b>,
    group_end: InlineGraphPosition,
    available_widths: &'a [f32],
    ellipsis_width: f32,
    allow_emergency_breaks: bool,
}

/// Mutable result state for one balance-candidate search.
#[derive(Default)]
struct BalancedLineSearchState {
    examined: usize,
    exhausted_budget: bool,
    best: Option<BalancedLineCandidate>,
}

/// The physical block slab occupied by one selected source line while it
/// queries float exclusions.
///
/// The source line identity stays separate from its page-local placement: a
/// float may move the same selected source line below several nominal strut
/// rows. Keeping those facts together avoids using a retry count as geometry.
/// The offset is relative to the normal physical block position and the size
/// is the inherited strut or the refined used line block-size.
/// <https://www.w3.org/TR/css-inline-3/#line-boxes>
/// <https://www.w3.org/TR/css-shapes-1/#shape-outside-property>
#[derive(Clone, Copy)]
struct PhysicalLineSlab {
    line_index: usize,
    block_offset: f32,
    used_block_size: f32,
}

/// The result of resolving a normal-flow horizontal line against established
/// float exclusions after its complete used block slab is known.
///
/// The source range remains unchanged on retry. Only its physical placement
/// moves, so a float cannot manufacture an empty CSS Text line or become a
/// source break opportunity.
/// <https://www.w3.org/TR/CSS22/visuren.html#floats>
enum HorizontalFloatLinePlacement {
    Accepted,
    RetryAtLaterSlab { block_advance: f32 },
}

/// A selected unbreakable line and the physical block distance skipped before
/// its source content can be placed beside CSS float exclusions.
///
/// The source line remains one CSS line box even when a float moves its used
/// physical slab below the nominal inherited-strut row. Keeping the skipped
/// distance with the selected fragment prevents the durable line sequence
/// from manufacturing an empty source line for that placement retry.
/// <https://www.w3.org/TR/CSS22/visuren.html#floats>
struct UnbreakableInlineFloatSelection {
    fragment: InlineLineFragment,
    block_before: f32,
}

/// Return the total containing-block measure a float-clearance retry needs.
///
/// An unbreakable line can legally overflow even its full containing block,
/// but it must not overflow a narrower float-shortened line when the float
/// can be cleared. Capping the used measure at the full measure therefore
/// finds the first later slab that removes enough exclusion without requiring
/// impossible room for the overflowing word itself.
/// <https://www.w3.org/TR/CSS22/visuren.html#floats>
fn float_clearance_required_width(
    used_line_width: f32,
    full_line_indent: f32,
    containing_width: f32,
) -> f32 {
    (used_line_width + full_line_indent)
        .min(containing_width)
        .max(0.0)
}

impl PhysicalLineSlab {
    fn new(line_index: usize, block_offset: f32, used_block_size: f32) -> Self {
        debug_assert!(block_offset >= 0.0);
        debug_assert!(used_block_size >= 0.0);
        Self {
            line_index,
            block_offset,
            used_block_size,
        }
    }

    fn inherited_strut(line_index: usize, block_offset: f32, line_height: f32) -> Self {
        Self::new(line_index, block_offset, line_height)
    }

    fn with_used_block_size(self, used_block_size: f32) -> Self {
        debug_assert!(used_block_size >= 0.0);
        Self {
            used_block_size,
            ..self
        }
    }
}

// Each candidate materializes a graph range so line-edge effects, tabs, and
// punctuation use the normal line model.  Keep the work comfortably below a
// single layout timeout; a larger search deliberately falls back to ordinary
// wrapping instead of selecting from an incomplete candidate set.
const MAX_BALANCED_LINE_CANDIDATES: usize = 2_048;

/// Return the variance of the unused inline measures in a candidate plan.
///
/// CSS Text Level 4 balances the remaining line-box inline space, not source
/// text advances.  Keeping this pure makes the comparison deterministic and
/// preserves the ordinary greedy selection on exact ties.
/// <https://drafts.csswg.org/css-text-4/#text-wrap-style>
fn balanced_remaining_space_variance(remaining_inline_space: &[f32]) -> f32 {
    if remaining_inline_space.len() < 2 {
        return 0.0;
    }
    let mean = remaining_inline_space.iter().sum::<f32>() / remaining_inline_space.len() as f32;
    remaining_inline_space
        .iter()
        .map(|space| (space - mean).powi(2))
        .sum::<f32>()
        / remaining_inline_space.len() as f32
}

pub(in crate::layout) struct SelectedInlineLines {
    pub(in crate::layout) fragments: Vec<SelectedInlineLine>,
    pub(in crate::layout) next_line_index: usize,
    /// Extra physical block progression accumulated while selecting the last
    /// source line. A collected sequence carries this into the next paragraph
    /// so explicit breaks do not make later lines query the old float slab.
    pub(in crate::layout) next_physical_block_offset: f32,
    pub(in crate::layout) has_float_side_effects: bool,
    /// A source-order float committed by a graph that produced no in-flow
    /// fragment. The collected-line cursor carries it across an explicit
    /// break so the next graph reselects source against the same exclusion.
    pub(in crate::layout) trailing_inline_float_replay: Option<CommittedInlineFloatReplay>,
}

/// The inline formatter's handle for replaying a committed float transaction.
///
/// The floating subtree is already owned by `CommittedInlineFloat`; this
/// record contains only source selection and physical-row state.  In
/// particular, a float marker is not promoted to a CSS Text break while a
/// following graph is reselected.
/// <https://www.w3.org/TR/CSS22/visuren.html#floats>
#[derive(Debug, Clone, Copy)]
pub(in crate::layout) struct CommittedInlineFloatReplay {
    marker: (usize, usize),
    source_range_start: (usize, usize),
    source_range_end: (usize, usize),
    selected_row: usize,
    physical_row: usize,
    physical_block_offset: f32,
    used_block_advance: f32,
}

impl CommittedInlineFloatReplay {
    /// Whether the next source graph must initially select around this float.
    fn applies_before_source_row(self, line_index: usize) -> bool {
        debug_assert!(self.source_range_start <= self.marker);
        debug_assert!(self.marker <= self.source_range_end);
        debug_assert_eq!(self.physical_row, self.selected_row);
        debug_assert!(self.physical_block_offset >= -INLINE_FLOAT_EPSILON);
        debug_assert!(self.used_block_advance >= -INLINE_FLOAT_EPSILON);
        self.selected_row < line_index
    }
}

/// A selected inline fragment and its physical line index.
///
/// Float exclusions can make a line temporarily too narrow for content that
/// otherwise fits the containing block. Retaining the selected index lets the
/// durable sequence materialize intervening empty exclusion lines rather than
/// painting the content as overflow beside the float:
/// <https://www.w3.org/TR/CSS22/visuren.html#floats>.
pub(in crate::layout) struct SelectedInlineLine {
    pub(in crate::layout) fragment: InlineLineFragment,
    pub(in crate::layout) line_index: usize,
    /// Extra physical block advance before this line.
    ///
    /// Float contours can make a line first fit between normal line-height
    /// rows. This stores that CSS Shapes retry distance until the durable line
    /// sequence advances the page-local block cursor.
    pub(in crate::layout) block_before: f32,
}

/// The physical layout state of one provisional inline line.
///
/// Inline line selection happens before final line metrics are known, so it
/// advances by the computed line height just as the horizontal selector has
/// always done.  The block axis is physical y for horizontal writing and
/// physical x for vertical writing:
/// <https://www.w3.org/TR/css-inline-3/#line-layout> and
/// <https://www.w3.org/TR/css-writing-modes-4/#abstract-box>.
#[derive(Debug, Clone, Copy)]
struct InlineLinePhysicalPosition {
    content_left: f32,
    content_right: f32,
    cursor_y: f32,
}

/// Project one vertical line's logical block slab onto physical page X.
///
/// Vertical-lr lines begin at the containing box's physical left edge, while
/// vertical-rl and sideways-rl lines begin at its physical right edge. Keeping
/// this projection in one adapter prevents float-exclusion queries from always
/// sampling the leftmost column.
/// <https://www.w3.org/TR/css-writing-modes-4/#logical-to-physical>
/// <https://www.w3.org/TR/css-writing-modes-4/#line-mappings>
fn vertical_line_block_slab(
    position: InlineLinePhysicalPosition,
    block_style: &ComputedStyle,
    physical_left_inset: f32,
    block_extent: f32,
) -> PageInlineSpan {
    debug_assert!(block_style.writing_mode.has_vertical_lines());
    match FlowAxes::for_style(block_style).block_start_side() {
        PhysicalSide::Left => {
            PageInlineSpan::new(position.content_left + physical_left_inset, block_extent)
        }
        PhysicalSide::Right => {
            let right = position.content_right - block_style.padding.right;
            PageInlineSpan::from_edges(right - block_extent, right)
        }
        PhysicalSide::Top | PhysicalSide::Bottom => {
            unreachable!("a vertical line's block axis is physical X")
        }
    }
}

/// The selected source line's identity, independent of its physical row.
///
/// Float exclusions can create empty physical rows before inline source is
/// formatted. CSS Text keys first-line indentation and `each-line` behavior to
/// the formatted line and forced-break boundary, not to those rows:
/// <https://www.w3.org/TR/css-text-3/#text-indent-property>.
#[derive(Debug, Clone, Copy)]
pub(in crate::layout) struct SelectedLineIdentity {
    is_first_formatted_line: bool,
    starts_after_forced_break: bool,
}

/// The logical source line and its physical block placement.
///
/// A shaped float can move a selected line between normal line-height rows
/// without changing its CSS Text identity.
#[derive(Debug, Clone, Copy)]
pub(in crate::layout) struct InlineLinePhysicalRow {
    line_index: usize,
    identity: SelectedLineIdentity,
    block_offset: f32,
}

/// Immutable state for committing one source-order inline float.
///
/// A float marker is not a CSS Text line end.  Retaining the selected source
/// range and its physical row together means that a retry cannot accidentally
/// reinterpret the marker as a new line or reconstruct its y coordinate from
/// a later mutable block cursor:
/// <https://www.w3.org/TR/CSS22/visuren.html#float-position> and
/// <https://www.w3.org/TR/css-text-3/#line-breaking>.
struct InlineFloatTransactionCheckpoint {
    source_range: InlineGraphRange,
    selected_end: SelectedInlineLineEnd,
    marker: InlineGraphPosition,
    row: InlineLinePhysicalRow,
    /// CSS 2.2 constrains a source-order float by preceding line boxes, not
    /// by the row that happened to select its marker. The float context may
    /// move it later from this floor, but never needs an invented text row.
    placement_floor: InlineLinePhysicalRow,
    snapshot: LayoutSnapshot,
}

/// Return the boundary after a raised or sunken initial letter that must own
/// its originating line by itself.
///
/// CSS Inline lays following ordinary inline content after the leading line
/// slots exposed above a letter when its requested size exceeds its sink.
/// Keeping that as a graph boundary (rather than moving paint with a baseline
/// offset) lets the normal record builder materialize intervening phantom
/// lines and keeps exclusion queries in source order.
/// <https://drafts.csswg.org/css-inline-3/#initial-letter-position>
fn initial_letter_source_handoff_end(
    graph: &InlineOpportunityGraph,
    start: InlineGraphPosition,
) -> Option<InlineGraphPosition> {
    if start.byte_offset != 0 {
        return None;
    }
    let run = graph.runs.get(start.run_index)?;
    let InlineLineItem::Fragment(fragment) = &run.item else {
        return None;
    };
    let (size, sink) = fragment.style().initial_letter.specified()?;
    // Raised and sunken initials expose leading line slots before their
    // following source. A drop initial remains in its originating source
    // line in every writing mode; its exclusion, rather than an artificial
    // graph boundary, selects adjacent vertical columns.
    // <https://drafts.csswg.org/css-inline-3/#initial-letter-position>
    (size > sink as f32 + INLINE_FLOAT_EPSILON)
        .then_some(InlineGraphPosition::at_run_start(start.run_index + 1))
}

/// Number of *additional* ordinary line slots between a raised/sunken
/// initial's originating line and its following source content.
///
/// The initial letter itself does not enlarge the originating line box. These
/// slots are represented by missing records, which the durable line sequence
/// turns into phantom struts while painting and fragmenting the block.
/// <https://drafts.csswg.org/css-inline-3/#initial-letter-position>
fn raised_initial_letter_leading_line_slots(fragment: &InlineLineFragment) -> usize {
    fragment
        .items()
        .iter()
        .find_map(|item| {
            let InlineLineItem::Fragment(fragment) = &item.item else {
                return None;
            };
            let (size, sink) = fragment.style().initial_letter.specified()?;
            (size > sink as f32 + INLINE_FLOAT_EPSILON)
                .then_some((size.ceil() as u32).saturating_sub(sink).saturating_sub(1) as usize)
        })
        .unwrap_or(0)
}

/// Resolve the physical exclusion edge for an initial letter.
///
/// Horizontal initials wrap from their logical inline-start edge. In a
/// vertical or sideways flow, an initial letter instead occupies the logical
/// block-start column, which is the right edge in `*-rl` modes and the left
/// edge in `*-lr` modes.
/// <https://drafts.csswg.org/css-inline-3/#initial-letter-position>
/// <https://drafts.csswg.org/css-writing-modes-4/#logical-to-physical>
fn initial_letter_exclusion_side(
    writing_mode: WritingMode,
    horizontal_inline_start: UsedFloatSide,
) -> UsedFloatSide {
    match writing_mode {
        WritingMode::HorizontalTb => horizontal_inline_start,
        WritingMode::VerticalRl | WritingMode::SidewaysRl => UsedFloatSide::Right,
        WritingMode::VerticalLr | WritingMode::SidewaysLr => UsedFloatSide::Left,
    }
}

impl std::ops::Deref for SelectedInlineLine {
    type Target = InlineLineFragment;

    fn deref(&self) -> &Self::Target {
        &self.fragment
    }
}

impl<'a> LayoutBuilder<'a> {
    /// Apply CSS Text fitting to one used inline style while keeping the
    /// computed-style representation immutable.
    pub(in crate::layout) fn scale_text_fit_used_style(
        &mut self,
        style: &mut ComputedStyle,
        scale: f32,
    ) {
        scale_text_fit_fragment_style(style, scale);
    }

    /// Resolve one fitted line's parent text style exactly once.
    ///
    /// The selected font owns the used `normal` line-height. This must happen
    /// for the parent strut as well as for each fitted descendant fragment;
    /// otherwise right-to-left vertical block progression can position glyphs
    /// from an inflated fallback line height.
    fn text_fit_used_line_style(
        &mut self,
        block_style: &ComputedStyle,
        scale: TextFitScale,
    ) -> TextFitUsedLineStyle {
        let mut fitted_block_style = block_style.clone();
        self.scale_text_fit_used_style(&mut fitted_block_style, scale.factor());
        let line_stack_strut = fitted_block_style.line_height;
        if fitted_block_style.line_height_is_normal() {
            fitted_block_style.line_height = self
                .font_system
                .used_line_height(&fitted_block_style)
                .points();
        }
        TextFitUsedLineStyle {
            scale,
            block_style: Rc::new(fitted_block_style),
            line_stack_strut,
        }
    }

    fn inline_line_physical_position_with_block_offset(
        &self,
        line_index: usize,
        block_style: &ComputedStyle,
        block_offset: f32,
    ) -> InlineLinePhysicalPosition {
        let block_advance = block_style.line_height * line_index as f32 + block_offset;
        match block_style.writing_mode {
            WritingMode::HorizontalTb => InlineLinePhysicalPosition {
                content_left: self.content_left,
                content_right: self.content_right,
                cursor_y: self.cursor_y - block_advance,
            },
            WritingMode::VerticalRl | WritingMode::SidewaysRl => InlineLinePhysicalPosition {
                content_left: self.content_left - block_advance,
                content_right: self.content_right - block_advance,
                cursor_y: self.cursor_y,
            },
            WritingMode::VerticalLr | WritingMode::SidewaysLr => InlineLinePhysicalPosition {
                content_left: self.content_left + block_advance,
                content_right: self.content_right + block_advance,
                cursor_y: self.cursor_y,
            },
        }
    }

    pub(in crate::layout) fn inline_float_band_for_line(
        &self,
        line_index: usize,
        block_style: &ComputedStyle,
        available_width: f32,
        padding_left: f32,
    ) -> InlineFloatBand {
        self.inline_float_band_for_line_with_block_offset(
            line_index,
            block_style,
            available_width,
            padding_left,
            0.0,
        )
    }

    fn inline_float_band_for_line_with_block_offset(
        &self,
        line_index: usize,
        block_style: &ComputedStyle,
        available_width: f32,
        padding_left: f32,
        block_offset: f32,
    ) -> InlineFloatBand {
        self.inline_float_band_for_physical_slab(
            block_style,
            available_width,
            padding_left,
            PhysicalLineSlab::inherited_strut(line_index, block_offset, block_style.line_height),
        )
    }

    /// Return the float-reduced inline band for one physical line slab.
    ///
    /// The initial selector normally knows only the inherited strut. Atomic
    /// inline boxes can enlarge that strut, including from zero, so callers
    /// that have a provisional materialized line pass its used block-size to
    /// refine the band against the full painted slab.
    /// <https://drafts.csswg.org/css-inline-3/#line-boxes>
    /// <https://drafts.csswg.org/TR/css-shapes-1/#shape-outside-property>
    fn inline_float_band_for_physical_slab(
        &self,
        block_style: &ComputedStyle,
        available_width: f32,
        padding_left: f32,
        slab: PhysicalLineSlab,
    ) -> InlineFloatBand {
        let position = self.inline_line_physical_position_with_block_offset(
            slab.line_index,
            block_style,
            slab.block_offset,
        );
        if block_style.writing_mode != WritingMode::HorizontalTb {
            let band = self.current_logical_float_band(
                block_style.writing_mode,
                block_style.used_direction(),
                FloatBandQuery {
                    horizontal_slab: vertical_line_block_slab(
                        position,
                        block_style,
                        padding_left,
                        slab.used_block_size,
                    ),
                    vertical_slab: vertical_physical_inline_span(
                        block_style.writing_mode,
                        block_style.used_direction(),
                        PageTopBlockPosition::new(position.cursor_y),
                        layout_pt(available_width),
                    ),
                },
            );
            return InlineFloatBand::new(band.inline_span.start(), band.inline_span.size());
        }
        let band =
            self.current_float_band(PageBlockSpan::new(position.cursor_y, slab.used_block_size));
        let left_offset = (band.left() - position.content_left - padding_left).max(0.0);
        let right_offset = (position.content_right - band.right()).max(0.0);
        InlineFloatBand::new(left_offset, available_width - left_offset - right_offset)
    }

    /// Return the band created by CSS floats only for initial-letter
    /// placement.
    ///
    /// An initial letter joins the shared content-wrap query after it is
    /// anchored. Reading the normal content band while anchoring would let a
    /// provisional initial-letter exclusion displace the very glyph that
    /// created it. CSS floats remain valid earlier placement geometry.
    /// <https://drafts.csswg.org/css-inline-3/#initial-letter-floats>
    fn css_float_band_for_physical_slab(
        &self,
        block_style: &ComputedStyle,
        available_width: f32,
        padding_left: f32,
        slab: PhysicalLineSlab,
    ) -> InlineFloatBand {
        let position = self.inline_line_physical_position_with_block_offset(
            slab.line_index,
            block_style,
            slab.block_offset,
        );
        let float_context = self
            .float_contexts
            .last()
            .expect("root float context exists");
        let page_index = self.current_float_page_index();
        if block_style.writing_mode != WritingMode::HorizontalTb {
            let band = float_context.logical_band(
                block_style.writing_mode,
                block_style.used_direction(),
                page_index,
                FloatBandQuery {
                    horizontal_slab: vertical_line_block_slab(
                        position,
                        block_style,
                        padding_left,
                        slab.used_block_size,
                    ),
                    vertical_slab: vertical_physical_inline_span(
                        block_style.writing_mode,
                        block_style.used_direction(),
                        PageTopBlockPosition::new(position.cursor_y),
                        layout_pt(available_width),
                    ),
                },
            );
            return InlineFloatBand::new(band.inline_span.start(), band.inline_span.size());
        }
        let band = float_context.band(
            page_index,
            PageBlockSpan::new(position.cursor_y, slab.used_block_size),
            PageInlineSpan::from_edges(
                position.content_left + padding_left,
                position.content_right,
            ),
        );
        let left_offset = (band.left() - position.content_left - padding_left).max(0.0);
        let right_offset = (position.content_right - band.right()).max(0.0);
        InlineFloatBand::new(left_offset, available_width - left_offset - right_offset)
    }

    pub(in crate::layout) fn layout_mixed_inline_paragraph(
        &mut self,
        items: &[InlineItem],
        context: InlineParagraphContext<'_>,
        mut line_index: usize,
        starts_after_forced_break: bool,
        preceding_plaintext_line_direction: &mut Option<Direction>,
    ) -> InlineLayoutOutcome {
        let block_style = context.block_style;
        if context.initial_first_formatted_line
            && line_index == 0
            && block_style
                .first_letter_style
                .as_deref()
                .is_some_and(|first_letter| !first_letter.initial_letter.is_normal())
        {
            self.discard_provisional_initial_letter_exclusions(block_style);
        }
        if block_style.writing_mode == WritingMode::HorizontalTb
            && context.initial_first_formatted_line
            && line_index == 0
            && block_style
                .first_letter_style
                .as_deref()
                .is_some_and(|first_letter| !first_letter.initial_letter.is_normal())
        {
            self.clear_initial_letter_exclusions_for_new_initial(block_style);
        }
        let paragraph_start_line_index = line_index;
        let graph = self.build_inline_opportunity_graph(items, block_style);
        let graph = if context.initial_first_formatted_line && line_index == 0 {
            self.graph_with_first_letter_pseudo(&graph, block_style)
        } else {
            graph
        };
        // An initial letter affects selection of its own companion source,
        // including source separated by an explicit break.  Register a
        // graph-measured provisional exclusion before selecting any line;
        // the final selected fragment replaces it below once its used
        // margins, box edges, and contour are known.
        // <https://drafts.csswg.org/css-inline-3/#initial-letter-position>
        if context.initial_first_formatted_line && line_index == 0 {
            self.register_provisional_initial_letter_exclusion(
                &graph,
                context,
                line_index,
                starts_after_forced_break,
                false,
            );
        }
        let selected_lines = self.select_inline_lines_from_graph(
            &graph,
            context,
            line_index,
            starts_after_forced_break,
        );
        let mut line_boxes = selected_lines.fragments;
        if !selected_lines.has_float_side_effects {
            self.apply_text_fit(&mut line_boxes, block_style);
        }
        let next_line_index = selected_lines.next_line_index;
        line_index = next_line_index;
        let line_count = line_boxes.len();
        let paragraph_last_hanging_width = line_boxes
            .last()
            .map(|line_box| {
                last_hanging_punctuation_width_for_line_items(
                    &mut self.font_system,
                    &line_box.fragment.items,
                    block_style,
                )
            })
            .map(SemanticLengthExt::points)
            .unwrap_or(0.0);
        let mut records = Vec::new();
        let mut next_record_line_index = paragraph_start_line_index;
        for (offset, selected_line) in line_boxes.into_iter().enumerate() {
            while next_record_line_index < selected_line.line_index {
                records.push(InlineLineRecord {
                    paragraph_index: 0,
                    block_line_index: next_record_line_index,
                    paragraph_line_index: records.len(),
                    fragment: None,
                    kind: InlineLineKind::ForcedEmpty,
                    is_first_formatted_line: context.initial_first_formatted_line
                        && next_record_line_index == 0,
                    is_last_line_in_paragraph: false,
                    termination: InlineLineTermination::SoftWrap,
                    used_bidi_base_direction: None,
                    starts_after_preserved_segment_break: false,
                    clear_after: Clear::None,
                    block_before: 0.0,
                    block_start_trim: 0.0,
                    block_end_trim: 0.0,
                    paragraph_last_hanging_width,
                    used_indent: 0.0,
                    available_width: context.available_width,
                    line_height: block_style.line_height,
                    text_fit_used_style: None,
                    decoration_origin_fragments: Default::default(),
                });
                next_record_line_index += 1;
            }
            let line_box = selected_line.fragment;
            let block_line_index = selected_line.line_index;
            // Atomic inline boxes contribute their margin-box block-size to
            // the containing line, including when the inherited strut is
            // zero. Text fragments are already represented by the selected
            // line metrics; adding their raw `line-height` here would undo
            // text-edge trimming.
            // <https://drafts.csswg.org/css-inline-3/#line-boxes>
            let item_line_height = line_box
                .items()
                .iter()
                // Initial letters span later lines through their exclusion
                // participant, rather than increasing this ordinary line
                // record's block advance.
                // <https://drafts.csswg.org/css-inline-3/#initial-letter-position>
                .filter(|item| !LayoutBuilder::inline_line_item_is_initial_letter(&item.item))
                .filter_map(|item| {
                    crate::layout::inline_layout::inline_line_item_additional_block_extent(
                        &item.item,
                        block_style,
                    )
                })
                .fold(0.0_f32, f32::max);
            let line_height = line_box
                .metrics
                .height
                .max(block_style.line_height)
                .max(item_line_height);
            let used_indent = line_box.indent;
            let available_width = line_box.available_width;
            let kind = InlineLineKind::for_fragment(&line_box, false);
            records.push(InlineLineRecord {
                paragraph_index: 0,
                block_line_index,
                paragraph_line_index: records.len(),
                fragment: Some(line_box),
                kind,
                // Edge-only phantom records retain positioned-inline source
                // geometry, but are not formatted lines.
                // <https://drafts.csswg.org/css-inline-3/#phantom-line-boxes>
                is_first_formatted_line: context.initial_first_formatted_line
                    && block_line_index == 0
                    && !kind.is_phantom(),
                is_last_line_in_paragraph: offset + 1 == line_count,
                termination: if offset + 1 == line_count {
                    InlineLineTermination::BlockEnd
                } else {
                    InlineLineTermination::SoftWrap
                },
                used_bidi_base_direction: None,
                starts_after_preserved_segment_break: false,
                clear_after: Clear::None,
                block_before: selected_line.block_before,
                block_start_trim: 0.0,
                block_end_trim: 0.0,
                paragraph_last_hanging_width,
                used_indent,
                available_width,
                line_height,
                text_fit_used_style: None,
                decoration_origin_fragments: Default::default(),
            });
            next_record_line_index = block_line_index + 1;
        }
        let mut sequence = InlineLineSequence {
            records,
            available_width: context.available_width,
            padding_left: context.padding_left,
            hanging_indent: context.hanging_indent,
            hanging_punctuation_reserve: context.hanging_punctuation_reserve,
            fragment_text_box_trim: TextBoxLineTrim::default(),
            has_flow_side_effects: selected_lines.has_float_side_effects,
            replay_float_scope: ReplayFloatScope::InheritContainingBlock,
            has_local_continuation_cutoff: matches!(
                context.line_clamp,
                Some(css::InlineLineClamp::Automatic(_))
            ),
        };
        sequence.resolve_bidi_base_directions(block_style, preceding_plaintext_line_direction);
        self.paint_inline_line_sequence(&sequence, block_style);
        InlineLayoutOutcome {
            next_line_index: line_index,
            clamp_line_slots: sequence.records.len(),
            clamp_block_advance: sequence.layout_outcome().clamp_block_advance,
            has_non_phantom_line: sequence.has_non_phantom_line(),
            has_flow_effects: selected_lines.has_float_side_effects || sequence.has_flow_effects(),
            has_local_continuation_cutoff: sequence.has_local_continuation_cutoff,
        }
    }

    /// Apply CSS Text Level 5 fitting to already selected lines. Line breaking
    /// deliberately precedes this pass: fitting changes used font sizes but
    /// must not select a different source break.
    ///
    /// <https://drafts.csswg.org/css-text-5/#text-fit-property>
    fn apply_text_fit(&mut self, lines: &mut [SelectedInlineLine], block_style: &ComputedStyle) {
        let css::TextFit::Fit {
            direction,
            strategy,
            limit,
        } = block_style.text_fit
        else {
            return;
        };
        if lines.iter().any(|line| {
            line.fragment
                .items()
                .iter()
                .any(|item| Self::inline_line_item_is_initial_letter(&item.item))
        }) {
            // Scaling can change the line block size. An initial letter's
            // exclusion makes a later line's available inline size depend on
            // that block position, for which CSS Text disables fitting.
            return;
        }

        let last_formatted = lines
            .iter()
            .rposition(|line| !inline_line_fragment_is_phantom(&line.fragment));
        let mut scales = vec![TextFitScale::ONE; lines.len()];
        match strategy {
            css::TextFitStrategy::Consistent => {
                let Some(scale) = lines
                    .iter()
                    .filter(|line| !inline_line_fragment_is_phantom(&line.fragment))
                    .map(|line| {
                        self.text_fit_scale_for_line(
                            &line.fragment,
                            direction,
                            limit,
                            line.line_index == 0,
                            block_style,
                        )
                    })
                    .reduce(TextFitScale::min)
                else {
                    return;
                };
                scales.fill(scale);
            }
            css::TextFitStrategy::PerLine | css::TextFitStrategy::PerLineAll => {
                for (index, line) in lines.iter().enumerate() {
                    if inline_line_fragment_is_phantom(&line.fragment)
                        || (matches!(strategy, css::TextFitStrategy::PerLine)
                            && Some(index) == last_formatted)
                    {
                        continue;
                    }
                    scales[index] = self.text_fit_scale_for_line(
                        &line.fragment,
                        direction,
                        limit,
                        line.line_index == 0,
                        block_style,
                    );
                }
            }
        }
        for (line, scale) in lines.iter_mut().zip(scales) {
            if (scale.factor() - 1.0).abs() > f32::EPSILON {
                let used_style = self.text_fit_used_line_style(block_style, scale);
                self.apply_text_fit_scale_to_line(
                    &mut line.fragment,
                    block_style,
                    &used_style,
                    line.line_index == 0,
                );
            }
        }
    }

    /// Apply CSS Text Level 5 fitting to a durable collected sequence.
    /// Collection can split one source block at forced breaks or page scopes,
    /// so strategy selection happens only after every record is available.
    pub(in crate::layout) fn apply_text_fit_to_records(
        &mut self,
        records: &mut [InlineLineRecord],
        block_style: &ComputedStyle,
    ) -> Option<TextFitAppliedRecords> {
        let css::TextFit::Fit {
            direction,
            strategy,
            limit,
        } = block_style.text_fit
        else {
            return None;
        };
        if records
            .iter()
            .filter_map(|record| record.fragment.as_ref())
            .any(|line| {
                line.items()
                    .iter()
                    .any(|item| Self::inline_line_item_is_initial_letter(&item.item))
            })
        {
            return None;
        }
        let formatted_records = records
            .iter()
            .enumerate()
            .filter(|(_, record)| record.fragment.is_some() && !record.kind.is_phantom())
            .map(|(index, _)| index)
            .collect::<Vec<_>>();
        let first_formatted = *formatted_records.first()?;
        let last_formatted = *formatted_records.last()?;
        let has_multiple_formatted_records = formatted_records.len() > 1;
        let mut scales = vec![TextFitScale::ONE; records.len()];
        match strategy {
            css::TextFitStrategy::Consistent => {
                let scale = formatted_records
                    .iter()
                    .map(|&index| {
                        let record = &records[index];
                        self.text_fit_scale_for_line(
                            record
                                .fragment
                                .as_ref()
                                .expect("formatted record has fragment"),
                            direction,
                            limit,
                            record.is_first_formatted_line,
                            block_style,
                        )
                    })
                    .reduce(TextFitScale::min)
                    .expect("formatted records are non-empty");
                for &index in &formatted_records {
                    scales[index] = scale;
                }
            }
            css::TextFitStrategy::PerLine | css::TextFitStrategy::PerLineAll => {
                for &index in &formatted_records {
                    let record = &records[index];
                    if !text_fit_strategy_scales_record(
                        strategy,
                        index,
                        last_formatted,
                        record.termination,
                    ) {
                        continue;
                    }
                    scales[index] = self.text_fit_scale_for_line(
                        record
                            .fragment
                            .as_ref()
                            .expect("formatted record has fragment"),
                        direction,
                        limit,
                        record.is_first_formatted_line,
                        block_style,
                    );
                }
            }
        }
        for &index in &formatted_records {
            let scale = scales[index];
            if (scale.factor() - 1.0).abs() <= f32::EPSILON {
                continue;
            }
            let used_style = self.text_fit_used_line_style(block_style, scale);
            let record = &mut records[index];
            let line = record
                .fragment
                .as_mut()
                .expect("formatted record has fragment");
            self.apply_text_fit_scale_to_line(
                line,
                block_style,
                &used_style,
                record.is_first_formatted_line,
            );
            record.line_height = if (matches!(strategy, css::TextFitStrategy::PerLine)
                && scale.factor() > 1.0)
                || (matches!(strategy, css::TextFitStrategy::PerLineAll)
                    && has_multiple_formatted_records)
            {
                // `per-line` leaves the block strut at its ordinary used
                // value; only its scalable inline content establishes the
                // enlarged line box.
                line.metrics.height
            } else {
                // The fitted style supplies font-resolved metrics to the
                // text fragments and paint context. The shared used style's
                // containing-block strut determines line-stack geometry.
                line.metrics.height.max(used_style.line_stack_strut())
            };
            record.text_fit_used_style = Some(used_style);
        }
        Some(TextFitAppliedRecords {
            first_formatted,
            last_formatted,
        })
    }

    /// Calculate one line's affine text-fit scale. Measuring at factors one
    /// and two separates scalable text from fixed inline contributions (such
    /// as atomic inlines and absolute tracking) without trying to reconstruct
    /// shaping internals from glyph advances.
    fn text_fit_scale_for_line(
        &mut self,
        line: &InlineLineFragment,
        direction: css::TextFitDirection,
        limit: Option<f32>,
        is_first_formatted_line: bool,
        block_style: &ComputedStyle,
    ) -> TextFitScale {
        let (_, one, _) = self.text_fit_items_at_scale(
            line,
            TextFitScale::ONE,
            is_first_formatted_line,
            block_style,
        );
        let (_, two, _) = self.text_fit_items_at_scale(
            line,
            TextFitScale::new(2.0).expect("two is a valid text-fit measurement scale"),
            is_first_formatted_line,
            block_style,
        );
        text_fit_scale_from_measurements(line.available_width, one, two, direction, limit)
    }

    fn apply_text_fit_scale_to_line(
        &mut self,
        line: &mut InlineLineFragment,
        block_style: &ComputedStyle,
        used_style: &TextFitUsedLineStyle,
        is_first_formatted_line: bool,
    ) {
        let materialize_first_line_style =
            text_fit_applies_first_line_style(block_style, is_first_formatted_line);
        let (items, width, edge_effects) = self.text_fit_items_at_scale(
            line,
            used_style.scale(),
            is_first_formatted_line,
            block_style,
        );
        line.metrics = self.mixed_inline_line_metrics(&items, used_style.block_style(), width);
        line.items = Rc::from(items.into_boxed_slice());
        line.edge_effects = edge_effects;
        line.text = Rc::from(text_for_measured_items(line.items()));
        if materialize_first_line_style {
            line.mark_first_line_style_materialized();
        }
    }

    fn text_fit_items_at_scale(
        &mut self,
        line: &InlineLineFragment,
        scale: TextFitScale,
        is_first_formatted_line: bool,
        block_style: &ComputedStyle,
    ) -> (Vec<MeasuredInlineItem>, f32, InlineLineEdgeEffects) {
        let mut items = line.items().to_vec();
        if text_fit_applies_first_line_style(block_style, is_first_formatted_line)
            && !line.first_line_style_materialized
        {
            let mut source_items = measured_inline_items(&items);
            apply_first_line_pseudos_to_line_items(
                &mut source_items,
                block_style,
                false,
                &mut self.font_system,
            );
            for (measured, source) in items.iter_mut().zip(source_items) {
                measured.item = source;
            }
        }
        for item in &mut items {
            let InlineLineItem::Fragment(fragment) = &mut item.item else {
                continue;
            };
            let fitted_tracking_scope = fragment
                .tracking_scope()
                .map(|scope| scope.scaled_for_text_fit(scale.factor()));
            self.scale_text_fit_used_style(fragment.style_mut(), scale.factor());
            if fragment.style().line_height_is_normal() {
                fragment.style_mut().line_height =
                    self.font_system.used_line_height(fragment.style()).points();
            }
            if let Some(scope) = fitted_tracking_scope {
                *fragment = fragment.clone().with_tracking_scope(scope);
            }
            fragment.clear_cached_shaping_for_used_style_change();
            remeasure_materialized_item(item, &mut self.font_system);
        }
        apply_visual_tracking_boundaries(&mut items);
        let widths = inline_content_width_for_line_items(&items, &mut self.font_system, |item| {
            item.used_advance().points()
        });
        let mut edge_effects = line.edge_effects.clone();
        edge_effects.collapsed_end_trim_width = self.text_fit_edge_effect_width(
            &items,
            &edge_effects,
            InlineLineEdgeEffectKind::CollapsedEndTrim,
        );
        edge_effects.pre_wrap_hanging_width = self.text_fit_edge_effect_width(
            &items,
            &edge_effects,
            InlineLineEdgeEffectKind::PreWrapHang,
        );
        edge_effects.hanging_space_separator_width = widths.trailing_space_width;
        let width = (widths.content_width
            - edge_effects.collapsed_end_trim_width
            - edge_effects.pre_wrap_hanging_width)
            .max(0.0);
        (items, width, edge_effects)
    }

    /// Re-measure a source-owned selected line-edge range in the fitted
    /// style. The range remains an edge effect rather than a scalable text
    /// contribution: its newly measured advance is deducted from the line's
    /// used inline measure.
    fn text_fit_edge_effect_width(
        &mut self,
        items: &[MeasuredInlineItem],
        edge_effects: &InlineLineEdgeEffects,
        kind: InlineLineEdgeEffectKind,
    ) -> f32 {
        edge_effects
            .source_effects
            .iter()
            .filter(|effect| effect.kind == kind)
            .filter_map(|effect| {
                let InlineLineItem::Fragment(fragment) = &items.get(effect.item_index)?.item else {
                    return None;
                };
                fragment
                    .text()
                    .get(effect.source_range.clone())
                    .map(|text| self.font_system.measure_text(text, fragment.style()))
            })
            .sum()
    }

    /// Select CSS inline line fragments from an opportunity graph.
    ///
    /// CSS Inline's line construction is greedy, but every soft-wrap fallback
    /// must choose a CSS Text break opportunity. This selector walks graph
    /// boundaries instead of appending raw items and later searching strings,
    /// keeping wrapping, intrinsic sizing, and future fragmentation attached
    /// to the same reusable line-fragment model:
    /// <https://www.w3.org/TR/css-inline-3/#line-layout>,
    /// <https://www.w3.org/TR/css-text-3/#line-breaking>, and
    /// <https://www.w3.org/TR/css-break-3/#widows-orphans>.
    pub(in crate::layout) fn select_inline_lines_from_graph(
        &mut self,
        graph: &InlineOpportunityGraph,
        context: InlineParagraphContext<'_>,
        line_index: usize,
        starts_after_forced_break: bool,
    ) -> SelectedInlineLines {
        self.select_inline_lines_from_graph_with_block_offset(
            graph,
            context,
            line_index,
            starts_after_forced_break,
            0.0,
        )
    }

    /// Select source lines beginning at an already-resolved physical block
    /// displacement.
    ///
    /// A durable collected sequence can contain explicit source breaks. Those
    /// breaks start a new graph but not a new physical block coordinate, so a
    /// preceding contour retry must remain part of the next graph's float
    /// query.
    pub(in crate::layout) fn select_inline_lines_from_graph_with_block_offset(
        &mut self,
        graph: &InlineOpportunityGraph,
        context: InlineParagraphContext<'_>,
        line_index: usize,
        starts_after_forced_break: bool,
        initial_physical_block_offset: f32,
    ) -> SelectedInlineLines {
        self.select_inline_lines_from_graph_with_block_offset_and_replay(
            graph,
            context,
            line_index,
            starts_after_forced_break,
            initial_physical_block_offset,
            None,
        )
    }

    /// Select source lines while consuming a float transaction committed by
    /// the immediately preceding collected graph.
    ///
    /// Preserved forced breaks split source graphs but do not end the CSS 2.2
    /// float's exclusion. The replay handle supplies that missing source
    /// ordering fact without re-running the float child traversal.
    pub(in crate::layout) fn select_inline_lines_from_graph_with_block_offset_and_replay(
        &mut self,
        graph: &InlineOpportunityGraph,
        context: InlineParagraphContext<'_>,
        mut line_index: usize,
        starts_after_forced_break: bool,
        initial_physical_block_offset: f32,
        carried_inline_float_replay: Option<CommittedInlineFloatReplay>,
    ) -> SelectedInlineLines {
        if graph.is_empty() {
            return SelectedInlineLines {
                fragments: Vec::new(),
                next_line_index: line_index,
                next_physical_block_offset: initial_physical_block_offset,
                has_float_side_effects: false,
                trailing_inline_float_replay: None,
            };
        }
        // Once an inline-source float is positioned, it remains in the graph
        // as a zero-advance source-order marker while its exclusion changes
        // the available bands.  Keeping its position separate lets a retry
        // select the whole affected line against that new band instead of
        // treating preceding inline content as an in-flow prefix of the
        // float.
        let mut placed_inline_float_positions = Vec::new();
        // This token applies only until the first in-flow source row has been
        // selected. Subsequent rows are already ordered after that source and
        // query the committed exclusion normally.
        let mut carried_inline_float_replay = carried_inline_float_replay;
        let mut pending_balanced_float_replays = Vec::<PendingBalancedFloatReplay>::new();
        let mut replayed_float_block_boundaries = Vec::<InlineGraphPosition>::new();
        let mut balanced_plan = self.balanced_line_plan(
            graph,
            context,
            graph.start_position(),
            line_index,
            starts_after_forced_break,
            &placed_inline_float_positions,
        );
        let mut balanced_plan_index = 0usize;
        // `white-space: normal` permits soft wrapping, but does not itself
        // create a CSS Text break inside an unbreakable word. Float placement
        // must therefore ask the shared opportunity graph for an actual
        // non-forced boundary rather than using an item's wrap-capable style
        // as a proxy. The unbreakable-float path below owns out-of-flow
        // positioning without inventing a text break:
        // <https://drafts.csswg.org/css-text-3/#line-break-details> and
        // <https://www.w3.org/TR/CSS22/visuren.html#float-position>.
        let paragraph_start_line_index = line_index;
        let mut fragments = Vec::new();
        let mut has_float_side_effects = false;
        // Physical block advance beyond the nominal `line-height`. Logical
        // line indices remain source-line bookkeeping, but float-band
        // selection must follow the used line boxes. This is observable for
        // atomic inline boxes when their containing block has `line-height:
        // 0`: subsequent boxes still start after each atom's used block-size.
        // CSS Inline Layout defines a line box from its inline-level
        // participants rather than treating the inherited strut as a cap:
        // <https://drafts.csswg.org/css-inline-3/#line-boxes>.
        let mut physical_line_block_offset = initial_physical_block_offset;
        let mut pending_block_before = 0.0;
        let mut start = graph.start_position();
        let graph_end = context
            .line_clamp
            .and_then(|clamp| clamp.inline_source_end())
            .map(|endpoint| InlineGraphPosition {
                run_index: endpoint.run_index(),
                byte_offset: endpoint.byte_offset(),
            })
            .map_or_else(
                || graph.end_position(),
                |endpoint| endpoint.min(graph.end_position()),
            );
        // Keep the remaining-source query incremental. Re-scanning every
        // graph break on every selected line turns a long, float-heavy
        // paragraph into quadratic retry work.
        let soft_wrap_positions = graph
            .break_opportunities_after(graph.start_position())
            .filter(|opportunity| {
                opportunity.position < graph_end && opportunity_is_soft_wrap(*opportunity)
            })
            .map(|opportunity| opportunity.position)
            .collect::<Vec<_>>();
        let mut next_soft_wrap_position = 0usize;
        while start < graph_end {
            if context
                .line_clamp
                .as_ref()
                .is_some_and(|line_clamp| line_clamp.excludes_line(line_index))
            {
                break;
            }
            while start.byte_offset == 0
                && start.run_index < graph.runs.len()
                && inline_line_item_is_collapsible_space(&graph.runs[start.run_index].item)
            {
                start.run_index += 1;
            }
            if start >= graph_end {
                break;
            }
            if let Some(boundary_index) = replayed_float_block_boundaries
                .iter()
                .position(|boundary| *boundary < start)
            {
                // A source-order float is an out-of-flow participant, but
                // its marker still occupies the formatted-row transition
                // between the preceding selected source and following source.
                // Preserve that transition after replaying the float so the
                // next source line queries the slab below the float marker.
                replayed_float_block_boundaries.remove(boundary_index);
                physical_line_block_offset += context.block_style.line_height;
                pending_block_before += context.block_style.line_height;
            }
            while next_soft_wrap_position < soft_wrap_positions.len()
                && soft_wrap_positions[next_soft_wrap_position] <= start
            {
                next_soft_wrap_position += 1;
            }
            if let Some(float) = graph.float_at_position(start).cloned() {
                // A float marker at the beginning of the source row is the
                // zero-prefix case of ordinary source-order placement. Keep
                // the same transactional path used by a marker after text:
                // the float is committed once, then later source is selected
                // against its exclusion. `white-space: nowrap` does not turn
                // this placement into a CSS Text break opportunity.
                // <https://www.w3.org/TR/CSS22/visuren.html#float-position>
                // <https://www.w3.org/TR/CSS22/text.html#white-space-prop>
                if self
                    .try_place_inline_float_in_line_band(
                        graph,
                        start,
                        context,
                        InlineLinePhysicalRow {
                            line_index,
                            identity: SelectedLineIdentity {
                                starts_after_forced_break: starts_after_forced_break
                                    && line_index == paragraph_start_line_index,
                                is_first_formatted_line: context.initial_first_formatted_line
                                    && paragraph_start_line_index == 0
                                    && fragments.is_empty(),
                            },
                            block_offset: physical_line_block_offset,
                        },
                    )
                    .is_placed()
                {
                    placed_inline_float_positions.push(start);
                    has_float_side_effects = true;
                    balanced_plan = None;
                    balanced_plan_index = 0;
                    start.run_index += 1;
                    start.byte_offset = 0;
                    continue;
                }
                // A marker with no source prefix is already at its earliest
                // legal float row.  The ordinary floated-child placement path
                // is allowed to move it farther down around preceding
                // exclusions, but the marker itself must be consumed even
                // when the provisional line band rejects it.  Otherwise the
                // greedy selector retries the same zero-advance graph node
                // indefinitely and effectively promotes the marker to a
                // line-break opportunity.
                self.place_inline_waiting_float_on_row(
                    &float,
                    start,
                    context,
                    InlineLinePhysicalRow {
                        line_index,
                        identity: SelectedLineIdentity {
                            starts_after_forced_break: starts_after_forced_break
                                && line_index == paragraph_start_line_index,
                            is_first_formatted_line: context.initial_first_formatted_line
                                && paragraph_start_line_index == 0
                                && fragments.is_empty(),
                        },
                        block_offset: physical_line_block_offset,
                    },
                );
                placed_inline_float_positions.push(start);
                has_float_side_effects = true;
                balanced_plan = None;
                balanced_plan_index = 0;
                start.run_index += 1;
                start.byte_offset = 0;
                continue;
            }
            let line_identity = SelectedLineIdentity {
                starts_after_forced_break: starts_after_forced_break
                    && line_index == paragraph_start_line_index,
                is_first_formatted_line: context.initial_first_formatted_line
                    && paragraph_start_line_index == 0
                    && fragments.is_empty(),
            };
            let committed_preceding_float =
                graph.runs[..start.run_index]
                    .iter()
                    .enumerate()
                    .any(|(run_index, run)| {
                        matches!(run.item, InlineLineItem::Float(_))
                            && placed_inline_float_positions
                                .contains(&InlineGraphPosition::at_run_start(run_index))
                    });
            let immediately_preceding_unbreakable_float = start
                .run_index
                .checked_sub(1)
                .and_then(|run_index| {
                    graph.float_at_position(InlineGraphPosition::at_run_start(run_index))
                })
                .is_some_and(|float| !float.style().allows_soft_wrap());
            let starts_initial_letter = matches!(
                graph.runs.get(start.run_index).map(|run| &run.item),
                Some(InlineLineItem::Fragment(fragment))
                    if !fragment.style().initial_letter.is_normal()
            );
            let has_provisional_initial_for_line =
                self.float_contexts.last().is_some_and(|floats| {
                    floats.shapes.iter().any(|shape| {
                        shape.kind == FlowExclusionKind::InitialLetter
                            && shape.page_index == self.current_float_page_index()
                            && shape.initial_letter.as_ref().is_some_and(|layout| {
                                layout.provisional && layout.impacted_line_range.start == line_index
                            })
                    })
                });
            if starts_initial_letter
                && committed_preceding_float
                && !has_provisional_initial_for_line
            {
                self.register_provisional_initial_letter_exclusion(
                    graph,
                    context,
                    line_index,
                    line_identity.starts_after_forced_break,
                    true,
                );
            }
            // A preceding ordinary soft break does not make the remaining
            // source wrappable. Once that break has committed, a following
            // `nowrap` (or otherwise unbreakable) segment containing a float
            // must retain its whole source range and overflow as one line.
            // A float marker is a placement boundary rather than a CSS Text
            // wrapping opportunity. Other atomic boundaries are CSS Text
            // opportunities between adjacent atomic inline-level boxes and
            // keep the remaining source wrappable.
            // <https://www.w3.org/TR/css-text-3/#white-space-property>
            let remaining_source_breakability =
                if next_soft_wrap_position < soft_wrap_positions.len() {
                    InlineSourceBreakability::HasLegalSoftWrap
                } else {
                    InlineSourceBreakability::Unbreakable
                };
            // Float-excluded rows consume physical block-size but do not make
            // a formatted line. Preserve this identity independently of the
            // physical line index for `text-indent` and hanging indents.
            if balanced_plan.is_none()
                && matches!(
                    context.block_style.text_wrap_style,
                    css::TextWrapStyle::Balance
                )
                && graph.runs[start.run_index..]
                    .iter()
                    .enumerate()
                    .all(|(offset, run)| {
                        !matches!(run.item, InlineLineItem::Float(_))
                            || placed_inline_float_positions.contains(
                                &InlineGraphPosition::at_run_start(start.run_index + offset),
                            )
                    })
            {
                balanced_plan = self.balanced_line_plan(
                    graph,
                    context,
                    start,
                    line_index,
                    line_identity.starts_after_forced_break,
                    &placed_inline_float_positions,
                );
            }
            if let Some(plan) = balanced_plan.as_ref()
                && let Some(replay_index) = pending_balanced_float_replays
                    .iter()
                    .position(|replay| plan.iter().any(|entry| entry.start < replay.position))
            {
                let replayed_plan = plan.clone();
                let replay = pending_balanced_float_replays.remove(replay_index);
                // The float's source follows every selected entry whose start
                // precedes its marker. Replaying from the captured snapshot
                // places it after those final source lines instead of beside
                // the greedy prefix that first discovered the marker.
                let source_lines_before_float = plan
                    .iter()
                    .filter(|entry| entry.start < replay.position)
                    .count();
                let target_line_index = replay.source_line_index + source_lines_before_float;
                self.restore(replay.snapshot);
                self.adjoining_float_origin_y = None;
                if let Some(float) = graph.float_at_position(replay.position).cloned() {
                    self.place_inline_waiting_float(
                        &float,
                        replay.position,
                        context,
                        target_line_index,
                    );
                }
                replayed_float_block_boundaries.push(replay.position);
                // The replay changes paint/exclusion geometry, not the
                // source candidate that won the balance search. Retain its
                // endpoints rather than starting over with a greedy normal
                // sequence against the newly placed float.
                balanced_plan = Some(replayed_plan);
                balanced_plan_index = 0;
                continue;
            }
            // A balance plan is a completed source selection, not a hint to
            // the ordinary greedy selector.  In particular, a source-order
            // float makes the selected line's exclusion band meaningful to
            // the balance score.  Remember when this endpoint came from that
            // plan so the later float and clamp reconciliation stages can
            // validate it without silently replacing it with a greedy edge.
            // <https://drafts.csswg.org/css-text-4/#text-wrap-style>
            let selected_from_balanced_plan = balanced_plan
                .as_ref()
                .and_then(|plan| plan.get(balanced_plan_index))
                .filter(|plan| plan.start == start)
                .copied();
            let mut selected_end = selected_from_balanced_plan
                .map(|plan| plan.end)
                .unwrap_or_else(|| {
                    self.select_inline_line_end_with_block_offset(
                        graph,
                        start,
                        context,
                        line_index,
                        line_identity,
                        physical_line_block_offset,
                    )
                });
            // The inherited strut is only a provisional float-query slab.
            // Refine an ordinary greedy candidate against its complete used
            // line slab before committing a CSS Shapes band. Atomic boxes can
            // enlarge a zero-height strut, but ordinary text and markers can
            // also change the band at a shaped float contour. A balanced
            // endpoint has already been selected against its line-local band;
            // replacing it here would discard the candidate that was scored
            // with that band and turn balanced float paragraphs back into
            // greedy paragraphs.
            //
            // Each iteration selects a legal graph boundary for one physical
            // slab. The cap is a diagnostic guard for a pathological contour
            // alternation; normal state transitions either stabilize or move
            // the candidate to a later physical slab below.
            // <https://drafts.csswg.org/css-inline-3/#line-boxes>
            // <https://drafts.csswg.org/TR/css-shapes-1/#shape-outside-property>
            if selected_from_balanced_plan.is_none()
                && selected_end.position > start
                && self
                    .float_contexts
                    .last()
                    .is_some_and(|floats| !floats.shapes.is_empty())
            {
                let mut converged = false;
                for _ in 0..4 {
                    let provisional = graph.materialize_line(
                        InlineGraphRange {
                            start,
                            end: selected_end.position,
                        },
                        selected_end.break_opportunity,
                        &mut self.font_system,
                        context.block_style,
                    );
                    let provisional_metrics = self.mixed_inline_line_metrics(
                        &provisional.items,
                        context.block_style,
                        provisional.content_width,
                    );
                    let block_size = used_inline_line_block_size_from_items(
                        &provisional.items,
                        provisional_metrics.height,
                        context.block_style,
                    );
                    if block_size <= INLINE_FLOAT_EPSILON {
                        converged = true;
                        break;
                    }
                    let refined = self.select_inline_line_end_with_block_span(
                        graph,
                        start,
                        context,
                        line_index,
                        line_identity,
                        PhysicalLineSlab::inherited_strut(
                            line_index,
                            physical_line_block_offset,
                            context.block_style.line_height,
                        )
                        .with_used_block_size(block_size),
                    );
                    if refined.position == selected_end.position
                        && refined.break_opportunity == selected_end.break_opportunity
                    {
                        converged = true;
                        break;
                    }
                    selected_end = refined;
                    if selected_end.position <= start {
                        converged = true;
                        break;
                    }
                }
                debug_assert!(
                    converged,
                    "selected inline candidate did not converge with its physical float slab"
                );
            }
            let initial_source_handoff_end = initial_letter_source_handoff_end(graph, start);
            if let Some(initial_end) = initial_source_handoff_end {
                selected_end = SelectedInlineLineEnd {
                    position: initial_end,
                    break_opportunity: graph.break_opportunity_at(initial_end).filter(
                        |opportunity| !matches!(opportunity.kind, InlineBreakKind::FloatPlacement),
                    ),
                };
                // A balance plan selected against the unsplit source is no
                // longer applicable after the initial-letter-specific first
                // line boundary.
                balanced_plan = None;
                balanced_plan_index = 0;
            }
            if selected_end.position > graph_end {
                selected_end = SelectedInlineLineEnd {
                    position: graph_end,
                    break_opportunity: graph.break_opportunity_at(graph_end),
                };
            }
            // Reserve the truncation marker while selecting the final clamped
            // line.  Removing materialized items afterward loses the graph
            // source ranges that own CSS Text Phase II effects, and can also
            // select a different break from the one that actually fits with
            // the marker.
            // <https://drafts.csswg.org/css-overflow-3/#line-clamp>
            let is_final_clamped_line = context
                .line_clamp
                .as_ref()
                .is_some_and(|line_clamp| line_clamp.is_terminal_line(line_index));
            let clamp_continues_after_line = line_clamp_has_later_in_flow_content(context);
            let mut final_clamp_marker_replaces_source =
                selected_from_balanced_plan.is_some_and(|plan| plan.clamp_marker_replaces_source);
            if is_final_clamped_line
                && selected_from_balanced_plan.is_none()
                && !final_clamp_marker_replaces_source
                // A graph can end at a preserved forced break while the
                // clamp container still has later in-flow source. That is a
                // legal clamp point whose marker must replace this final
                // graph's unbreakable line when no soft wrap fits beside it;
                // only a true terminal graph with no continuation skips the
                // marker-fitting pass.
                && (selected_end.position < graph_end || clamp_continues_after_line)
                && (!graph_remaining_after_position_is_trimmable(graph, selected_end.position)
                    || clamp_continues_after_line)
            {
                let ellipsis_width =
                    self.line_clamp_marker_width(context.line_clamp, context.block_style);
                if ellipsis_width > 0.0 {
                    let band = self.inline_float_band_for_line(
                        line_index,
                        context.block_style,
                        context.available_width,
                        context.padding_left,
                    );
                    let line_indent = used_line_indent_for_formatted_line(
                        line_identity.is_first_formatted_line,
                        line_identity.starts_after_forced_break,
                        context.hanging_indent,
                        context.block_style,
                        band.width(),
                    );
                    let marker_available_width =
                        (band.width() - line_indent - ellipsis_width).max(0.0);
                    let marker_selected = self.select_inline_line_end_for_block_ellipsis(
                        graph,
                        start,
                        context.block_style,
                        marker_available_width,
                        line_index,
                    );
                    let marker_selection_fits = marker_selected.position > start
                        && self.balanced_line_fit_width(
                            graph,
                            start,
                            marker_selected,
                            context.block_style,
                            line_index,
                            Some(marker_available_width),
                        ) <= marker_available_width + INLINE_FLOAT_EPSILON;
                    if marker_selection_fits {
                        selected_end = marker_selected;
                    } else {
                        // An unbreakable source segment may not fit beside
                        // the block ellipsis at all. In that case the marker
                        // replaces the final line's source rather than
                        // allowing the ordinary no-fit fallback to overflow
                        // underneath it.
                        // <https://drafts.csswg.org/css-overflow-4/#line-clamp>
                        selected_end = SelectedInlineLineEnd {
                            position: start,
                            break_opportunity: None,
                        };
                        final_clamp_marker_replaces_source = true;
                    }
                }
            }
            // A paragraph without soft-wrap opportunities can still contain
            // forced breaks. Those breaks delimit real line boxes and must be
            // honored by line clamping rather than collapsing the remaining
            // paragraph into one unbounded line.
            let selected_forced_break = selected_end
                .break_opportunity
                .is_some_and(|opportunity| matches!(opportunity.kind, InlineBreakKind::Forced));
            let mut end = if final_clamp_marker_replaces_source {
                start
            } else if (!remaining_source_breakability.has_legal_soft_wrap()
                && !selected_forced_break
                && initial_source_handoff_end.is_none())
                || selected_end.position <= start
            {
                graph_end
            } else {
                selected_end.position.min(graph_end)
            };
            end = line_end_extended_over_adjacent_inline_float_markers(graph, end).min(graph_end);
            // A candidate soft wrap before a visible `nowrap` continuation
            // with a float must remain the preceding line's end. In
            // particular, the marker-only lookahead below must not absorb the
            // continuation's prefix. The following source selection takes the
            // whole continuation, and its ordinary transaction defers the
            // float below the overflowing line when needed.
            // <https://www.w3.org/TR/css-text-3/#white-space-property>
            // <https://www.w3.org/TR/CSS22/visuren.html#float-position>
            let preserve_unbreakable_continuation = graph
                .unbreakable_inline_float_continuation_after(InlineGraphRange { start, end })
                .is_some_and(|continuation| {
                    continuation.source_range.start == end
                        && continuation.marker < continuation.source_range.end
                });
            // Outside a visible `nowrap` continuation, a marker may extend a
            // fitting ordinary source prefix without becoming a CSS Text wrap
            // opportunity. The continuation query above deliberately blocks
            // that marker-only extension for `nowrap` source.
            // <https://www.w3.org/TR/css-text-3/#white-space-property>
            // <https://www.w3.org/TR/CSS22/visuren.html#float-position>
            if !preserve_unbreakable_continuation
                && let Some(float_position) =
                    graph.first_float_position_in_range(InlineGraphRange {
                        start,
                        end: graph_end,
                    })
                && end < float_position
                && !graph
                    .break_opportunities_after(end)
                    .take_while(|opportunity| opportunity.position <= float_position)
                    .any(opportunity_is_soft_wrap)
            {
                let prefix = graph.materialize_line(
                    InlineGraphRange {
                        start,
                        end: float_position,
                    },
                    None,
                    &mut self.font_system,
                    context.block_style,
                );
                let full_indent = used_line_indent_for_formatted_line(
                    line_identity.is_first_formatted_line,
                    line_identity.starts_after_forced_break,
                    context.hanging_indent,
                    context.block_style,
                    context.available_width,
                );
                if prefix.fitting_width <= context.available_width - full_indent + 0.5 {
                    end = float_position;
                }
            }
            // A wrappable selected range may fit the unexcluded containing
            // block while exceeding this line's temporary float band. CSS 2.2
            // then moves the line below the float instead of painting inline
            // overflow next to the exclusion. The decision is local to this
            // range: a preceding normal run must not make later `nowrap`
            // content defer instead of overflowing beside the float. Preserve
            // the skipped line so paint and subsequent float-band queries
            // advance together.
            // <https://www.w3.org/TR/CSS22/visuren.html#floats>
            // A zero inherited strut does not make an atomic inline's line
            // zero-height. Query the retry band with the selected range's
            // actual used block slab, otherwise an inline-block can be
            // selected beside a float at `line-height: 0` and only discover
            // its collision after it has already been committed.
            // <https://drafts.csswg.org/css-inline-3/#line-boxes>
            let band = self.inline_float_band_for_line_with_block_offset(
                line_index,
                context.block_style,
                context.available_width,
                context.padding_left,
                physical_line_block_offset,
            );
            let line_indent = used_line_indent_for_formatted_line(
                line_identity.is_first_formatted_line,
                line_identity.starts_after_forced_break,
                context.hanging_indent,
                context.block_style,
                band.width(),
            );
            let containing_indent = used_line_indent_for_formatted_line(
                line_identity.is_first_formatted_line,
                line_identity.starts_after_forced_break,
                context.hanging_indent,
                context.block_style,
                context.available_width,
            );
            let measures = InlineSelectionMeasures::new(context.available_width, band);
            let current_available_width = measures.band_after_indent(line_indent);
            let full_available_width = measures.containing_after_indent(containing_indent);
            // A selected range that fits the containing block but not this
            // float-reduced band must retry from the next physical slab.
            // CSS 2.2's float retry is line-box placement, not a synthetic
            // break inside a word or atomic inline participant.
            let replays_committed_preceding_float = carried_inline_float_replay
                .is_some_and(|replay| replay.applies_before_source_row(line_index));
            if context.block_style.allows_soft_wrap()
                && (remaining_source_breakability.has_legal_soft_wrap()
                    || (committed_preceding_float && !immediately_preceding_unbreakable_float)
                    || replays_committed_preceding_float)
                && end > start
                && current_available_width + INLINE_FLOAT_EPSILON < full_available_width
                && let materialized = graph.materialize_line(
                    InlineGraphRange { start, end },
                    (end < graph_end)
                        .then_some(selected_end.break_opportunity)
                        .flatten(),
                    &mut self.font_system,
                    context.block_style,
                )
            {
                // A provisional initial-letter exclusion selects the source
                // beside the initial.  Its own isolated source advance is
                // not companion content competing for that narrowed band:
                // testing it there would make a wide margin box defer its
                // originating line below itself.
                // <https://drafts.csswg.org/css-inline-3/#initial-letter-floats>
                let initial_source_advance = (line_identity.is_first_formatted_line
                    && start.byte_offset == 0)
                    .then(|| {
                        let initial_index = materialized.items.iter().position(|item| {
                            matches!(
                                item.as_ref(),
                                InlineLineItem::Fragment(fragment)
                                    if !fragment.style().initial_letter.is_normal()
                            )
                        })?;
                        let leading_pseudo_width = materialized.items[..initial_index]
                            .iter()
                            .rev()
                            .take_while(|item| {
                                matches!(
                                    item.as_ref(),
                                    InlineLineItem::Fragment(fragment)
                                        if fragment.first_letter_pseudo_role()
                                            == FirstLetterPseudoFragmentRole::LeadingPreservedWhitespace
                                )
                            })
                            .map(|item| item.used_advance().points())
                            .sum::<f32>();
                        Some(
                            leading_pseudo_width
                                + materialized.items[initial_index].used_advance().points(),
                        )
                    })
                    .flatten()
                    .unwrap_or(0.0);
                let companion_fitting_width =
                    (materialized.fitting_width - initial_source_advance).max(0.0);
                let requires_full_float_band = (committed_preceding_float
                    || replays_committed_preceding_float)
                    && companion_fitting_width > current_available_width + INLINE_FLOAT_EPSILON;
                let required_float_clearance_width = if requires_full_float_band {
                    float_clearance_required_width(
                        companion_fitting_width,
                        containing_indent,
                        context.available_width,
                    )
                } else {
                    companion_fitting_width + containing_indent
                };
                let needs_band_retry = companion_fitting_width
                    > current_available_width + INLINE_FLOAT_EPSILON
                    && required_float_clearance_width
                        <= full_available_width + INLINE_FLOAT_EPSILON;
                // In vertical writing, a dropped initial owns the originating
                // logical block column even when its companion happens to fit
                // in the remaining vertical measure. The companion is placed
                // in the adjacent block column; treating this solely as a
                // width retry leaves it painted on top of the initial's own
                // column whenever the remaining height is generous.
                // <https://drafts.csswg.org/css-inline-3/#initial-letter-position>
                let needs_vertical_initial_handoff = matches!(
                    context.block_style.writing_mode,
                    WritingMode::VerticalRl | WritingMode::SidewaysRl
                ) && line_identity.is_first_formatted_line
                    && initial_source_advance > INLINE_FLOAT_EPSILON;
                // The used block slab of a horizontal line is not known until
                // its atomic participants have been materialized. The
                // committed-fragment retry below is therefore the only place
                // allowed to advance this source row. Retrying here and then
                // again after materialization accumulates clearance for one
                // line.
                // <https://www.w3.org/TR/CSS22/visuren.html#floats>
                if context.block_style.writing_mode != WritingMode::HorizontalTb
                    && (needs_band_retry || needs_vertical_initial_handoff)
                {
                    let metrics = self.mixed_inline_line_metrics(
                        &materialized.items,
                        context.block_style,
                        materialized.content_width,
                    );
                    let slab_width = used_inline_line_block_size_from_items(
                        &materialized.items,
                        metrics.height,
                        context.block_style,
                    );
                    let position = self.inline_line_physical_position_with_block_offset(
                        line_index,
                        context.block_style,
                        physical_line_block_offset,
                    );
                    let starting_slab = vertical_line_block_slab(
                        position,
                        context.block_style,
                        context.padding_left,
                        slab_width,
                    );
                    let vertical_inline_span = vertical_physical_inline_span(
                        context.block_style.writing_mode,
                        context.block_style.used_direction(),
                        PageTopBlockPosition::new(position.cursor_y),
                        layout_pt(context.available_width),
                    );
                    let required_width = companion_fitting_width + containing_indent;
                    let next_left = if needs_vertical_initial_handoff {
                        // The initial's wrapping box occupies its own
                        // block-start column. Move the companion past the
                        // physical block-end edge of that box instead of
                        // asking a width query which can legitimately fit
                        // beside it in the same column.
                        self.float_contexts.last().and_then(|float_context| {
                            float_context
                                .shapes
                                .iter()
                                .filter(|shape| {
                                    shape.kind == FlowExclusionKind::InitialLetter
                                        && shape.page_index == self.current_float_page_index()
                                        && shape.rect.x() + shape.rect.width()
                                            > starting_slab.left_x() + INLINE_FLOAT_EPSILON
                                        && shape.rect.x()
                                            < starting_slab.right_x() - INLINE_FLOAT_EPSILON
                                })
                                .map(|shape| match context.block_style.writing_mode {
                                    WritingMode::VerticalRl | WritingMode::SidewaysRl => {
                                        PageInlinePosition::new(
                                            shape.rect.x() - starting_slab.width(),
                                        )
                                    }
                                    WritingMode::VerticalLr | WritingMode::SidewaysLr => {
                                        PageInlinePosition::new(shape.rect.x() + shape.rect.width())
                                    }
                                    WritingMode::HorizontalTb => unreachable!(),
                                })
                                .find(|candidate| match context.block_style.writing_mode {
                                    WritingMode::VerticalRl | WritingMode::SidewaysRl => {
                                        candidate.points()
                                            < starting_slab.left_x() - INLINE_FLOAT_EPSILON
                                    }
                                    WritingMode::VerticalLr | WritingMode::SidewaysLr => {
                                        candidate.points()
                                            > starting_slab.left_x() + INLINE_FLOAT_EPSILON
                                    }
                                    WritingMode::HorizontalTb => false,
                                })
                        })
                    } else {
                        self.float_contexts.last().and_then(|float_context| {
                            float_context.next_vertical_content_slab_with_width(
                                context.block_style.writing_mode,
                                context.block_style.used_direction(),
                                self.current_float_page_index(),
                                starting_slab,
                                vertical_inline_span,
                                required_width,
                            )
                        })
                    };
                    if let Some(next_left) = next_left {
                        let block_advance = match context.block_style.writing_mode {
                            WritingMode::VerticalLr | WritingMode::SidewaysLr => {
                                next_left.points() - starting_slab.left_x()
                            }
                            WritingMode::VerticalRl | WritingMode::SidewaysRl => {
                                starting_slab.left_x() - next_left.points()
                            }
                            WritingMode::HorizontalTb => unreachable!(),
                        };
                        if block_advance > INLINE_FLOAT_EPSILON {
                            physical_line_block_offset += block_advance;
                            pending_block_before += block_advance;
                            balanced_plan = None;
                            continue;
                        }
                    }
                    line_index += 1;
                    balanced_plan = None;
                    continue;
                }
            }
            let selected_range = InlineGraphRange { start, end };
            if !remaining_source_breakability.has_legal_soft_wrap()
                && let Some(selection) = self.try_select_unbreakable_line_with_inline_floats(
                    graph,
                    InlineGraphRange {
                        start,
                        end: graph_end,
                    },
                    SelectedInlineLineEnd {
                        position: graph_end,
                        break_opportunity: graph.break_opportunity_at(graph_end),
                    },
                    context,
                    InlineLinePhysicalRow {
                        line_index,
                        identity: line_identity,
                        block_offset: physical_line_block_offset,
                    },
                )
            {
                physical_line_block_offset += selection.block_before
                    + used_inline_line_block_advance(&selection.fragment, context.block_style);
                fragments.push(SelectedInlineLine {
                    fragment: selection.fragment,
                    line_index,
                    block_before: std::mem::take(&mut pending_block_before)
                        + selection.block_before,
                });
                has_float_side_effects = true;
                line_index += 1;
                balanced_plan_index += 1;
                start = end;
                continue;
            }
            let affected_float_range = inline_float_affected_range(graph, selected_range);
            if matches!(
                affected_float_range,
                InlineFloatAffectedRange::UnbreakableContinuation { .. }
            ) && let Some(selection) = self.try_select_unbreakable_line_with_inline_floats(
                graph,
                selected_range,
                SelectedInlineLineEnd {
                    position: end,
                    break_opportunity: graph.break_opportunity_at(end),
                },
                context,
                InlineLinePhysicalRow {
                    line_index,
                    identity: line_identity,
                    block_offset: physical_line_block_offset,
                },
            ) {
                // The transaction has committed every float in source order
                // and re-materialized the selected source range against its
                // accepted band. Keep that geometry frozen for paint/replay.
                physical_line_block_offset += selection.block_before
                    + used_inline_line_block_advance(&selection.fragment, context.block_style);
                fragments.push(SelectedInlineLine {
                    fragment: selection.fragment,
                    line_index,
                    block_before: std::mem::take(&mut pending_block_before)
                        + selection.block_before,
                });
                has_float_side_effects = true;
                line_index += 1;
                balanced_plan_index += 1;
                start = end;
                continue;
            }
            if let Some(float_position) = affected_float_range.marker()
                && !placed_inline_float_positions.contains(&float_position)
            {
                if matches!(
                    context.block_style.text_wrap_style,
                    css::TextWrapStyle::Balance
                ) && placed_inline_float_positions.is_empty()
                    && let Some(float) = graph.float_at_position(float_position).cloned()
                {
                    // Balancing must choose the whole affected line against
                    // the float's exclusion band. Position the float, record
                    // its zero-advance graph marker, and retry from the same
                    // line start instead of committing the preceding source
                    // as an in-flow prefix.
                    // <https://drafts.csswg.org/css-text-4/#text-wrap-style>
                    self.place_inline_waiting_float(&float, float_position, context, line_index);
                    placed_inline_float_positions.push(float_position);
                    has_float_side_effects = true;
                    balanced_plan = None;
                    balanced_plan_index = 0;
                    continue;
                }
                if float_position <= start {
                    if let Some(float) = graph.float_at_position(float_position).cloned() {
                        self.place_inline_waiting_float(
                            &float,
                            float_position,
                            context,
                            line_index,
                        );
                        placed_inline_float_positions.push(float_position);
                        has_float_side_effects = true;
                    }
                    start = InlineGraphPosition::at_run_start(float_position.run_index + 1);
                    continue;
                }
                // A float following in-flow text is positioned after that
                // text when it fits the remaining current-line band.  Placing
                // it against the whole band first would incorrectly let its
                // exclusion shift preceding source content.  If it does not
                // fit, the prefix is committed and the marker is retried at
                // the next line top.
                // <https://www.w3.org/TR/CSS22/visuren.html#float-position>
                let mut prefix = self.materialize_inline_line_fragment(
                    graph,
                    InlineGraphRange {
                        start,
                        end: float_position,
                    },
                    context,
                    InlineLinePhysicalRow {
                        line_index,
                        identity: line_identity,
                        block_offset: physical_line_block_offset,
                    },
                    None,
                );
                let placement_snapshot = self.snapshot();
                if matches!(
                    context.block_style.text_wrap_style,
                    css::TextWrapStyle::Balance
                ) && let Some(float) = graph.float_at_position(float_position).cloned()
                {
                    // This is an unresolved source-order float, so the
                    // greedy prefix that discovered it cannot establish its
                    // physical row. Put the float below that provisional row,
                    // then restart selection from the same source position.
                    // The following balance pass sees the actual float band
                    // and selects the preceding line against it. Placing it
                    // beside the greedy prefix first would freeze the float
                    // one row too early when balancing moves source back to
                    // the preceding line.
                    // <https://drafts.csswg.org/css-text-4/#text-wrap-style>
                    // This float follows in-flow source. It is therefore not
                    // adjoining the earlier float run, even though its
                    // placement is deferred while the balance candidate is
                    // replayed. Leaving the old origin active lets float
                    // placement pull it back to the preceding float's top.
                    self.adjoining_float_origin_y = None;
                    self.place_inline_waiting_float(
                        &float,
                        float_position,
                        context,
                        line_index + 1,
                    );
                    placed_inline_float_positions.push(float_position);
                    pending_balanced_float_replays.push(PendingBalancedFloatReplay {
                        position: float_position,
                        source_line_index: line_index,
                        snapshot: placement_snapshot,
                    });
                    has_float_side_effects = true;
                    balanced_plan = None;
                    balanced_plan_index = 0;
                    continue;
                }
                if let Some(placement) = self.try_place_inline_float_on_current_line(
                    graph,
                    float_position,
                    prefix.metrics.width,
                    context,
                    InlineLinePhysicalRow {
                        line_index,
                        identity: line_identity,
                        block_offset: physical_line_block_offset,
                    },
                ) {
                    placed_inline_float_positions.push(float_position);
                    has_float_side_effects = true;
                    if matches!(
                        context.block_style.text_wrap_style,
                        css::TextWrapStyle::Balance
                    ) {
                        // The source-order placement above establishes this
                        // float's physical row. Re-select the whole balance
                        // group with that resolved exclusion rather than
                        // committing the greedy prefix that was only needed
                        // to place the float. This is what lets a later float
                        // move earlier source to a more balanced preceding
                        // line without moving the float ahead of its source.
                        // <https://drafts.csswg.org/css-text-4/#text-wrap-style>
                        balanced_plan = None;
                        balanced_plan_index = 0;
                        continue;
                    }
                    let suffix_start =
                        InlineGraphPosition::at_run_start(float_position.run_index + 1);
                    let suffix_is_empty =
                        graph_remaining_after_position_is_trimmable(graph, suffix_start);
                    prefix.freeze_float_band();
                    if let Some(combined) = self.try_select_inline_float_same_line_suffix(
                        graph,
                        prefix.clone(),
                        float_position,
                        suffix_start,
                        placement,
                        context,
                        line_index,
                    ) {
                        let end = combined.end;
                        physical_line_block_offset +=
                            used_inline_line_block_advance(&combined.fragment, context.block_style);
                        fragments.push(SelectedInlineLine {
                            fragment: combined.fragment,
                            line_index,
                            block_before: std::mem::take(&mut pending_block_before),
                        });
                        line_index += 1;
                        balanced_plan_index += 1;
                        start = end;
                        continue;
                    }
                    if !suffix_is_empty && !placement.fits_remaining_band() {
                        self.restore(placement_snapshot);
                        prefix.requery_float_band();
                        physical_line_block_offset +=
                            used_inline_line_block_advance(&prefix, context.block_style);
                        fragments.push(SelectedInlineLine {
                            fragment: prefix,
                            line_index,
                            block_before: std::mem::take(&mut pending_block_before),
                        });
                        line_index += 1;
                        balanced_plan_index += 1;
                        start = float_position;
                        continue;
                    }
                    physical_line_block_offset +=
                        used_inline_line_block_advance(&prefix, context.block_style);
                    fragments.push(SelectedInlineLine {
                        fragment: prefix,
                        line_index,
                        block_before: std::mem::take(&mut pending_block_before),
                    });
                    line_index += 1;
                    balanced_plan_index += 1;
                    start = suffix_start;
                    continue;
                }
                if matches!(
                    context.block_style.text_wrap_style,
                    css::TextWrapStyle::Balance
                ) && let Some(float) = graph.float_at_position(float_position).cloned()
                {
                    // The float cannot share the provisional greedy prefix,
                    // so it moves to the next formatted row. Keep the prefix
                    // provisional too: once the float's real exclusion is
                    // known, the balance search may move part of it onto a
                    // different source line.
                    // <https://drafts.csswg.org/css-text-4/#text-wrap-style>
                    self.place_inline_waiting_float(
                        &float,
                        float_position,
                        context,
                        line_index + 1,
                    );
                    placed_inline_float_positions.push(float_position);
                    has_float_side_effects = true;
                    balanced_plan = None;
                    balanced_plan_index = 0;
                    continue;
                }
                let mut fragment = self.materialize_inline_line_fragment(
                    graph,
                    InlineGraphRange {
                        start,
                        end: float_position,
                    },
                    context,
                    InlineLinePhysicalRow {
                        line_index,
                        identity: line_identity,
                        block_offset: physical_line_block_offset,
                    },
                    None,
                );
                self.register_initial_letter_exclusion_for_line(
                    &mut fragment,
                    context,
                    line_index,
                    physical_line_block_offset,
                    false,
                );
                physical_line_block_offset +=
                    used_inline_line_block_advance(&fragment, context.block_style);
                fragments.push(SelectedInlineLine {
                    fragment,
                    line_index,
                    block_before: std::mem::take(&mut pending_block_before),
                });
                line_index += 1;
                balanced_plan_index += 1;
                start = float_position;
                continue;
            }
            let break_opportunity = selected_end
                .break_opportunity
                .filter(|opportunity| opportunity.position == end && end < graph_end);
            let mut fragment = self.materialize_inline_line_fragment(
                graph,
                selected_range,
                context,
                InlineLinePhysicalRow {
                    line_index,
                    identity: line_identity,
                    block_offset: physical_line_block_offset,
                },
                break_opportunity,
            );
            // CSS 2.2 requires a line that cannot contain its content beside
            // a float to move down until a later float slab can contain it.
            // This must happen after materialization: atomic inlines can make
            // the line taller than the selection strut, changing its actual
            // float-exclusion band.
            // <https://www.w3.org/TR/CSS22/visuren.html#floats>
            // Inline-source float transactions own their own source-order
            // retry rules. This CSS 2.2 line-clearance path is for ordinary
            // established exclusions, not a suffix after an inline marker.
            let selected_range_contains_placed_inline_float =
                placed_inline_float_positions.iter().any(|position| {
                    *position >= selected_range.start && *position < selected_range.end
                });
            // A `nowrap` suffix following an inline-source float is not an
            // ordinary line that may move below an established exclusion.
            // The float's source-order transaction already selected its row;
            // the unbreakable suffix instead overflows in that row's
            // remaining band, where paint applies the float's inline-start
            // offset.  Retrying below the float loses that source-order
            // relationship and turns the suffix into a full-width line.
            // <https://www.w3.org/TR/CSS22/visuren.html#floats>
            // <https://www.w3.org/TR/css-text-3/#white-space-property>
            let follows_unbreakable_placed_inline_float =
                committed_preceding_float && immediately_preceding_unbreakable_float;
            if !selected_range_contains_placed_inline_float
                && !follows_unbreakable_placed_inline_float
                && let HorizontalFloatLinePlacement::RetryAtLaterSlab { block_advance } = self
                    .resolve_horizontal_float_line_placement(
                        &fragment,
                        context,
                        InlineLinePhysicalRow {
                            line_index,
                            identity: line_identity,
                            block_offset: physical_line_block_offset,
                        },
                    )
            {
                physical_line_block_offset += block_advance;
                pending_block_before += block_advance;
                balanced_plan = None;
                continue;
            }
            // A retry after placing an inline-source float already selected
            // this fragment against that float's exclusion band. Preserve the
            // stored band at paint time instead of applying the live band a
            // second time; that distinction is essential when a left float
            // shifts an RTL line's physical left edge.
            if placed_inline_float_positions
                .iter()
                .any(|position| *position >= selected_range.start && *position < selected_range.end)
            {
                fragment.freeze_float_band();
            }
            self.register_initial_letter_exclusion_for_line(
                &mut fragment,
                context,
                line_index,
                physical_line_block_offset,
                false,
            );
            let leading_initial_letter_slots = raised_initial_letter_leading_line_slots(&fragment);
            physical_line_block_offset +=
                used_inline_line_block_advance(&fragment, context.block_style);
            fragments.push(SelectedInlineLine {
                fragment,
                line_index,
                block_before: std::mem::take(&mut pending_block_before),
            });
            carried_inline_float_replay = None;
            // The current line owns the initial letter. The following normal
            // source begins after the exposed leading slots; leaving the
            // indices sparse causes durable record construction to emit the
            // corresponding phantom struts.
            line_index += 1 + leading_initial_letter_slots;
            balanced_plan_index += 1;
            start = end;
        }
        if context
            .line_clamp
            .as_ref()
            .is_some_and(|line_clamp| line_clamp.reached_after_line_count(line_index))
            && (!graph_remaining_after_position_is_trimmable(graph, start)
                || line_clamp_has_later_in_flow_content(context))
            && let Some(fragment) = fragments.last_mut()
        {
            self.append_line_clamp_ellipsis(
                &mut fragment.fragment,
                context,
                line_index.saturating_sub(1),
            );
        }
        let trailing_inline_float_replay =
            placed_inline_float_positions.last().and_then(|marker| {
                let float = graph.float_at_position(*marker)?;
                let committed = self.committed_inline_floats.get_mut(&float.id())?;
                committed.replay = InlineFloatReplayMetadata {
                    source_range_start: (
                        graph.start_position().run_index,
                        graph.start_position().byte_offset,
                    ),
                    source_range_end: (
                        graph.end_position().run_index,
                        graph.end_position().byte_offset,
                    ),
                    physical_row: committed.selected_row,
                    physical_block_offset: initial_physical_block_offset,
                    used_block_advance: physical_line_block_offset - initial_physical_block_offset,
                };
                Some(CommittedInlineFloatReplay {
                    marker: committed.marker,
                    source_range_start: committed.replay.source_range_start,
                    source_range_end: committed.replay.source_range_end,
                    selected_row: committed.selected_row,
                    physical_row: committed.replay.physical_row,
                    physical_block_offset: committed.replay.physical_block_offset,
                    used_block_advance: committed.replay.used_block_advance,
                })
            });
        SelectedInlineLines {
            fragments,
            next_line_index: line_index,
            next_physical_block_offset: physical_line_block_offset,
            has_float_side_effects,
            trailing_inline_float_replay,
        }
    }

    /// Produce a same-line-count balance plan from the legal graph breaks.
    ///
    /// The normal greedy plan fixes the required line count. For each
    /// forced-break-separated group of at most ten lines, find the narrowest
    /// shared selection measure that still produces that count, then use the
    /// graph's legal opportunities at that measure. This is intentionally a
    /// conservative subset of CSS Text 4: it preserves normal wrapping for
    /// groups over ten lines, while a bounded exhaustive search evaluates
    /// every legal sequence for smaller groups against each line's actual
    /// available inline measure.
    /// <https://drafts.csswg.org/css-text-4/#text-wrap-style>
    fn balanced_line_plan(
        &mut self,
        graph: &InlineOpportunityGraph,
        context: InlineParagraphContext<'_>,
        plan_start: InlineGraphPosition,
        first_line_index: usize,
        starts_after_forced_break: bool,
        placed_inline_float_positions: &[InlineGraphPosition],
    ) -> Option<Vec<BalancedLinePlanEntry>> {
        if !matches!(
            context.block_style.text_wrap_style,
            css::TextWrapStyle::Balance
        ) || graph
            .runs
            .iter()
            .enumerate()
            .skip(plan_start.run_index)
            .any(|(run_index, run)| {
                matches!(run.item, InlineLineItem::Float(_))
                    && !placed_inline_float_positions
                        .contains(&InlineGraphPosition::at_run_start(run_index))
            })
        {
            return None;
        }

        let graph_end = context
            .line_clamp
            .and_then(|clamp| clamp.inline_source_end())
            .map(|endpoint| InlineGraphPosition {
                run_index: endpoint.run_index(),
                byte_offset: endpoint.byte_offset(),
            })
            .map_or_else(
                || graph.end_position(),
                |endpoint| endpoint.min(graph.end_position()),
            );
        let mut normal = Vec::new();
        let mut start = plan_start;
        let mut line_index = first_line_index;
        while start < graph_end {
            while start.byte_offset == 0
                && start.run_index < graph.runs.len()
                && inline_line_item_is_collapsible_space(&graph.runs[start.run_index].item)
            {
                start.run_index += 1;
            }
            if start >= graph_end {
                break;
            }
            let starts_after_forced_break =
                starts_after_forced_break && line_index == first_line_index;
            let mut end = self.select_inline_line_end(
                graph,
                start,
                context,
                line_index,
                SelectedLineIdentity {
                    is_first_formatted_line: first_line_index == 0 && normal.is_empty(),
                    starts_after_forced_break,
                },
            );
            if end.position > graph_end {
                end = SelectedInlineLineEnd {
                    position: graph_end,
                    break_opportunity: graph.break_opportunity_at(graph_end),
                };
            }
            if end.position <= start {
                return None;
            }
            normal.push(BalancedLinePlanEntry {
                start,
                end,
                line_index,
                clamp_marker_replaces_source: false,
            });
            start = end.position;
            line_index += 1;
        }
        if normal.len() < 2 {
            return Some(normal);
        }

        let mut clamped_has_overflow = false;
        let mut clamp_source_end = None;
        if let Some(line_clamp) = &context.line_clamp {
            clamped_has_overflow = normal.len() > line_clamp.max_lines()
                || (normal.len() == line_clamp.max_lines()
                    && line_clamp_has_later_in_flow_content(context));
            if clamped_has_overflow {
                // Apply the clamp marker to the ordinary final surviving
                // line before choosing the source boundary to balance. The
                // marker can either shorten that line or replace an
                // unbreakable segment entirely. Using its unmarked start
                // would discard source that is visible beside the marker;
                // using its unmarked end would retain source displaced by it.
                // <https://drafts.csswg.org/css-overflow-4/#line-clamp>
                clamp_source_end =
                    normal
                        .get(line_clamp.max_lines().saturating_sub(1))
                        .map(|entry| {
                            let ellipsis_width = self
                                .line_clamp_marker_width(context.line_clamp, context.block_style);
                            if ellipsis_width <= 0.0 {
                                return entry.end.position;
                            }
                            let band = self.inline_float_band_for_line(
                                entry.line_index,
                                context.block_style,
                                context.available_width,
                                context.padding_left,
                            );
                            let available_width = (band.width()
                                - used_line_indent(
                                    entry.line_index,
                                    false,
                                    context.hanging_indent,
                                    context.block_style,
                                    band.width(),
                                )
                                - ellipsis_width)
                                .max(0.0);
                            let marker_end = self.select_inline_line_end_for_block_ellipsis(
                                graph,
                                entry.start,
                                context.block_style,
                                available_width,
                                entry.line_index,
                            );
                            if marker_end.position > entry.start
                                && self.balanced_line_fit_width(
                                    graph,
                                    entry.start,
                                    marker_end,
                                    context.block_style,
                                    entry.line_index,
                                    Some(available_width),
                                ) <= available_width + INLINE_FLOAT_EPSILON
                            {
                                marker_end.position
                            } else {
                                entry.start
                            }
                        });
            }
            normal.truncate(line_clamp.max_lines());
            if let (Some(clamp_source_end), Some(final_line)) =
                (clamp_source_end, normal.last_mut())
                && clamp_source_end == final_line.start
            {
                // A marker-only final line is still a real planned line. It
                // must survive balance selection and be materialized as the
                // clamp ellipsis rather than falling through to the ordinary
                // overflow fallback.
                // <https://drafts.csswg.org/css-overflow-4/#block-ellipsis>
                final_line.end = SelectedInlineLineEnd {
                    position: clamp_source_end,
                    break_opportunity: None,
                };
                final_line.clamp_marker_replaces_source = true;
            }
        }

        let mut output = normal.clone();
        let mut group_start = 0usize;
        while group_start < normal.len() {
            let group_end = normal[group_start..]
                .iter()
                .position(|entry| {
                    entry.end.break_opportunity.is_some_and(|opportunity| {
                        matches!(opportunity.kind, InlineBreakKind::Forced)
                    })
                })
                .map_or(normal.len(), |offset| group_start + offset + 1);
            self.balance_line_plan_group(
                graph,
                context,
                &normal,
                &mut output,
                group_start..group_end,
                clamped_has_overflow && group_end == normal.len(),
                (clamped_has_overflow && group_end == normal.len())
                    .then_some(clamp_source_end)
                    .flatten(),
            );
            group_start = group_end;
        }
        Some(output)
    }

    /// Synthesize the ellipsis into the final surviving clamped line.
    ///
    /// The line record remains graph-backed, but the truncation marker is a
    /// real shaped fragment so its advance participates in the final line's
    /// alignment and painting.
    /// <https://drafts.csswg.org/css-overflow-3/#propdef-line-clamp>
    pub(in crate::layout) fn append_line_clamp_ellipsis(
        &mut self,
        fragment: &mut InlineLineFragment,
        context: InlineParagraphContext<'_>,
        line_index: usize,
    ) {
        let Some(line_clamp) = &context.line_clamp else {
            return;
        };
        let Some(placement) = css::BlockEllipsisPlacement::at_terminal_inline_line(
            css::EligibleMarkerLine::terminal_inline_line(line_index),
            line_clamp.ellipsis(),
        ) else {
            return;
        };
        debug_assert_eq!(placement.line.inline_line_index(), line_index);
        let marker = placement.marker.text();
        // CSS Overflow places the marker in an anonymous inline of the
        // terminal line's root inline box. The paragraph context carries that
        // root's style: it is the nested block formatting context for a
        // block-in-inline, but remains the clamp root across ordinary inline
        // descendants such as spans. A final text fragment cannot represent
        // this boundary because it may be inside such a span.
        // <https://drafts.csswg.org/css-overflow-4/#block-ellipsis>
        let marker_source_style = context.block_style.clone();
        let marker_line_height = marker_source_style.line_height;
        // The anonymous marker inline has `line-height: 0`, so a fallback
        // glyph cannot increase the final line box.  That is represented by
        // the dedicated source role below rather than changing this used
        // text style to a zero line height: the latter would incorrectly
        // make visual-order preparation discard the paintable glyph.
        let style = marker_source_style;
        let baseline_shift = 0.0;
        let shaped = self
            .font_system
            .shape_unwrapped_line(marker, &style, marker_line_height)
            .map(std::rc::Rc::new);
        let ellipsis_width = shaped
            .as_deref()
            .map(ShapedInlineLine::advance_width)
            .unwrap_or(0.0);
        let mut items = fragment.items().to_vec();
        // A clamp marker replaces discarded source at the final line edge.
        // In particular, a collapsed separator selected only to reach the
        // following overflow source must not remain between the visible text
        // and the marker. Ordinary painting retains Phase II source ranges,
        // but the marker is a new replacement boundary rather than a source
        // line edge.
        // <https://drafts.csswg.org/css-overflow-4/#line-clamp>
        let mut retained_effects = Vec::with_capacity(fragment.edge_effects.source_effects.len());
        for effect in fragment.edge_effects.source_effects.iter() {
            if effect.kind != InlineLineEdgeEffectKind::CollapsedEndTrim {
                retained_effects.push(effect.clone());
                continue;
            }
            let Some(item) = items.get_mut(effect.item_index) else {
                continue;
            };
            let InlineLineItem::Fragment(source) = &mut item.item else {
                continue;
            };
            if effect.source_range.end != source.text().len()
                || !source.text().is_char_boundary(effect.source_range.start)
            {
                continue;
            }
            source.set_text(std::rc::Rc::<str>::from(
                &source.text()[..effect.source_range.start],
            ));
            remeasure_materialized_item(item, &mut self.font_system);
        }
        fragment.edge_effects.source_effects = std::rc::Rc::from(retained_effects);
        fragment.edge_effects.collapsed_end_trim_width = 0.0;
        items.push(MeasuredInlineItem::new(
            InlineLineItem::Fragment(InlineFragment::new(
                marker,
                style,
                baseline_shift,
                None,
                false,
                InlineTextSource::BlockEllipsis,
                false,
                InlineHangingEdges::default(),
                Vec::new(),
            )),
            ellipsis_width,
            shaped,
        ));
        // The source line's width already incorporates its selected Phase II
        // trimming and hanging effects.  The marker is new used content, so
        // extend that width instead of recomputing from raw source items.
        let content_width = fragment.metrics.width + ellipsis_width;
        // `::first-line` styles remain a paint-time delta in the durable
        // line record. Reapply that delta only for metrics, while the marker
        // source role prevents it from becoming pseudo content. Otherwise
        // appending an ordinary-style marker would accidentally recompute the
        // surviving source line with the parent strut instead of its
        // first-line metrics.
        // <https://drafts.csswg.org/css-overflow-4/#block-ellipsis>
        let mut metrics_items = items.clone();
        if line_index == 0 && context.block_style.first_line_style.is_some() {
            let mut pseudo_items = measured_inline_items(&metrics_items);
            apply_first_line_pseudos_to_line_items(
                &mut pseudo_items,
                context.block_style,
                false,
                &mut self.font_system,
            );
            for (measured, pseudo_item) in metrics_items.iter_mut().zip(pseudo_items) {
                measured.item = pseudo_item;
            }
        }
        fragment.metrics =
            self.mixed_inline_line_metrics(&metrics_items, context.block_style, content_width);
        fragment.items = std::rc::Rc::from(items.into_boxed_slice());
        fragment.text = std::rc::Rc::from(text_for_measured_items(fragment.items()));
    }

    #[expect(
        clippy::too_many_arguments,
        reason = "The immutable paragraph inputs and mutable selected-plan output have distinct ownership."
    )]
    fn balance_line_plan_group(
        &mut self,
        graph: &InlineOpportunityGraph,
        context: InlineParagraphContext<'_>,
        normal: &[BalancedLinePlanEntry],
        output: &mut [BalancedLinePlanEntry],
        group_range: std::ops::Range<usize>,
        includes_clamp_ellipsis: bool,
        clamp_source_end: Option<InlineGraphPosition>,
    ) {
        let group_start = group_range.start;
        let group_end = group_range.end;
        let line_count = group_end - group_start;
        if !(2..=10).contains(&line_count) {
            return;
        }
        let group = &normal[group_start..group_end];
        let mut available_widths = Vec::with_capacity(line_count);
        for entry in group {
            let band = self.inline_float_band_for_line(
                entry.line_index,
                context.block_style,
                context.available_width,
                context.padding_left,
            );
            let indent = used_line_indent(
                entry.line_index,
                false,
                context.hanging_indent,
                context.block_style,
                band.width(),
            );
            available_widths.push((band.width() - indent).max(0.0));
        }
        if available_widths.iter().any(|width| !width.is_finite()) {
            return;
        }
        let group_end_position = clamp_source_end
            .unwrap_or_else(|| group.last().expect("non-empty balance group").end.position);
        let ellipsis_width = if includes_clamp_ellipsis {
            self.line_clamp_marker_width(context.line_clamp, context.block_style)
        } else {
            0.0
        };
        let normal_remaining = group
            .iter()
            .zip(&available_widths)
            .enumerate()
            .map(|(index, (entry, available))| {
                let ellipsis = if index + 1 == line_count {
                    ellipsis_width
                } else {
                    0.0
                };
                (available
                    - self.balanced_line_fit_width(
                        graph,
                        entry.start,
                        entry.end,
                        context.block_style,
                        entry.line_index,
                        Some(*available),
                    )
                    - ellipsis)
                    .max(0.0)
            })
            .collect::<Vec<_>>();
        let normal_score = balanced_remaining_space_variance(&normal_remaining);
        let mut state = BalancedLineSearchState::default();
        self.search_balanced_line_group(
            &BalancedLineSearch {
                graph,
                context,
                group_end: group_end_position,
                available_widths: &available_widths,
                ellipsis_width,
                // `overflow-wrap` is a last-resort fallback. If ordinary
                // wrapping established this line count without it, balancing
                // must not introduce an emergency word split merely to make
                // the rag smaller.
                // <https://drafts.csswg.org/css-text-4/#text-wrap-style>
                allow_emergency_breaks: group.iter().any(|entry| {
                    entry
                        .end
                        .break_opportunity
                        .is_some_and(|opportunity| opportunity.availability.is_fallback())
                }),
            },
            group[0].start,
            group[0].line_index,
            &mut BalancedLineCandidate {
                entries: Vec::with_capacity(line_count),
                remaining_inline_space: Vec::with_capacity(line_count),
            },
            &mut state,
        );
        if !state.exhausted_budget
            && let Some(candidate) = state.best
            && candidate.entries.len() == line_count
            && balanced_remaining_space_variance(&candidate.remaining_inline_space)
                + INLINE_FLOAT_EPSILON
                < normal_score
        {
            output[group_start..group_end].copy_from_slice(&candidate.entries);
        }
    }

    /// Measure a candidate's used advance with the same edge effects and
    /// hanging punctuation treatment as ordinary graph selection.
    fn balanced_line_fit_width(
        &mut self,
        graph: &InlineOpportunityGraph,
        start: InlineGraphPosition,
        end: SelectedInlineLineEnd,
        block_style: &ComputedStyle,
        line_index: usize,
        available_width: Option<f32>,
    ) -> f32 {
        let range = InlineGraphRange {
            start,
            end: end.position,
        };
        let remaining_allows_last =
            graph_remaining_allows_last_hanging_punctuation(graph, end.position);
        let applies_first_line_style = line_index == 0 && block_style.first_line_style.is_some();
        // Balance, clamping, and float-marker selection share this exact
        // fitting entry point. When the graph proves that source advances
        // already are the selected line advances, let those callers consume
        // the same source measurement as ordinary `break-all` wrapping.
        if source_measurement_matches_selected_line(
            graph,
            block_style,
            line_index,
            end.break_opportunity,
        ) && let Some(width) = graph.monotonic_source_range_width(range)
        {
            return width;
        }
        #[cfg(feature = "layout-profile")]
        let exact_remeasurement_started = std::time::Instant::now();
        // Do not use the graph's borrowed prefix measure here. Materializing
        // the candidate is what applies selected edge trimming, atomic-inline
        // word-spacing ownership, and pseudo-style shaping before both
        // balance scoring and ordinary fallback selection.
        let mut materialized = match available_width {
            Some(available_width) => graph.materialize_line_for_available_width(
                range,
                end.break_opportunity,
                available_width,
                &mut self.font_system,
                block_style,
            ),
            None => graph.materialize_line(
                range,
                end.break_opportunity,
                &mut self.font_system,
                block_style,
            ),
        };
        if applies_first_line_style {
            // `::first-line` participates in line fitting, including balanced
            // candidate evaluation. Use the same durable materialization path
            // as the finally selected line so shaping, CSS Text edge effects,
            // and line-box metrics cannot observe different used styles.
            // <https://drafts.csswg.org/css-pseudo-4/#first-line-pseudo> and
            // <https://drafts.csswg.org/css-text-4/#text-wrap-style>
            self.materialize_first_line_used_style(
                &mut materialized,
                block_style,
                graph.selected_line_end_condition(range, end.break_opportunity),
                available_width,
            );
        }
        let hanging_widths = hanging_punctuation_widths_for_line_items(
            &mut self.font_system,
            &materialized.items,
            block_style,
            line_index == 0,
            remaining_allows_last,
            true,
        );
        let width =
            (materialized.fitting_width - hanging_widths.start - hanging_widths.end).max(0.0);
        #[cfg(feature = "layout-profile")]
        crate::layout::layout_profile::record_inline_line_exact_remeasurement(
            graph.source_byte_len_for_range(range),
            exact_remeasurement_started.elapsed(),
        );
        width
    }

    /// Materialize the generated `::first-line` inline box into one selected
    /// graph line.
    ///
    /// Candidate selection and durable line construction both call this
    /// adapter. Keeping the pseudo style in the selected measured items is
    /// necessary because font properties can change not only paint, but also
    /// glyph advances, CSS Text edge effects, and the root inline box's used
    /// line height.
    /// <https://drafts.csswg.org/css-pseudo-4/#first-line-styling>
    fn materialize_first_line_used_style(
        &mut self,
        line: &mut MaterializedInlineGraphLine,
        block_style: &ComputedStyle,
        line_end: SelectedLineEndCondition,
        available_width: Option<f32>,
    ) {
        let mut source_items = measured_inline_items(&line.items);
        apply_first_line_pseudos_to_line_items(
            &mut source_items,
            block_style,
            false,
            &mut self.font_system,
        );
        for (measured, source) in line.items.iter_mut().zip(source_items) {
            measured.item = source;
            remeasure_materialized_item(measured, &mut self.font_system);
        }
        apply_visual_tracking_boundaries(&mut line.items);
        resolve_materialized_line_tab_and_ruby_geometry(
            &mut line.items,
            &mut self.font_system,
            block_style,
        );

        let widths =
            inline_content_width_for_line_items(&line.items, &mut self.font_system, |item| {
                item.used_advance().points()
            });
        let collapsed_end_trim_width = trailing_collapsible_measured_width(&line.items);
        let pre_wrap_suffix_width = trailing_pre_wrap_hanging_width_with_unconditional_separators(
            &line.items,
            &mut self.font_system,
        );
        let pre_wrap_hanging_width = if widths.trailing_space_width > 0.0 {
            pre_wrap_suffix_width
        } else {
            line_end.pre_wrap_hanging_width(
                pre_wrap_suffix_width,
                widths.fitting_width,
                available_width,
            )
        };
        line.edge_effects.collapsed_end_trim_width = collapsed_end_trim_width;
        line.edge_effects.pre_wrap_hanging_width = pre_wrap_hanging_width;
        line.edge_effects.hanging_space_separator_width = widths.trailing_space_width;
        line.fitting_width =
            (widths.fitting_width - collapsed_end_trim_width - pre_wrap_hanging_width).max(0.0);
        line.content_width = line.fitting_width;
    }

    /// Enumerate same-count legal break sequences for one balance group.
    ///
    /// The CSS Text 4 limit of ten lines keeps this bounded in the common
    /// case.  The explicit candidate cap makes pathological prose retain its
    /// ordinary sequence rather than risking unbounded layout work.
    fn search_balanced_line_group(
        &mut self,
        search: &BalancedLineSearch<'_, '_>,
        mut start: InlineGraphPosition,
        line_index: usize,
        candidate: &mut BalancedLineCandidate,
        state: &mut BalancedLineSearchState,
    ) {
        // CSS Text Phase I collapses a leading document-space run at every
        // selected line start. The ordinary selector performs this before
        // choosing its next edge; the balance search must normalize its
        // recursive candidate starts identically or its stored plan will no
        // longer match the real source stream after the first line.
        // <https://www.w3.org/TR/css-text-3/#white-space-phase-1>
        while start.byte_offset == 0
            && start.run_index < search.graph.runs.len()
            && inline_line_item_is_collapsible_space(&search.graph.runs[start.run_index].item)
        {
            start.run_index += 1;
        }
        if state.examined >= MAX_BALANCED_LINE_CANDIDATES {
            state.exhausted_budget = true;
            return;
        }
        // Count every visited partial sequence, not only completed plans:
        // otherwise a broad first branch can spend unbounded work before it
        // reaches its first leaf.
        state.examined += 1;
        let candidate_index = candidate.entries.len();
        if candidate_index == search.available_widths.len() {
            if start == search.group_end {
                let score = balanced_remaining_space_variance(&candidate.remaining_inline_space);
                if state.best.as_ref().is_none_or(|best| {
                    score + INLINE_FLOAT_EPSILON
                        < balanced_remaining_space_variance(&best.remaining_inline_space)
                }) {
                    state.best = Some(candidate.clone());
                }
            }
            return;
        }

        let is_final_line = candidate_index + 1 == search.available_widths.len();
        let mut ends = search
            .graph
            .break_opportunities_after(start)
            .filter(|opportunity| {
                self.mixed_graph_opportunity_allowed(search.graph, *opportunity)
                    && (search.allow_emergency_breaks || !opportunity.availability.is_fallback())
                    && opportunity.position <= search.group_end
                    && (is_final_line == (opportunity.position == search.group_end))
            })
            .map(|opportunity| SelectedInlineLineEnd {
                position: opportunity.position,
                break_opportunity: (opportunity.position < search.graph.end_position())
                    .then_some(opportunity),
            })
            .collect::<Vec<_>>();
        if is_final_line && !ends.iter().any(|end| end.position == search.group_end) {
            ends.push(SelectedInlineLineEnd {
                position: search.group_end,
                break_opportunity: (search.group_end < search.graph.end_position())
                    .then(|| {
                        search
                            .graph
                            .opportunities
                            .iter()
                            .find(|opportunity| opportunity.position == search.group_end)
                            .copied()
                    })
                    .flatten(),
            });
        }
        for end in ends {
            if end.position <= start {
                continue;
            }
            let used_width = self.balanced_line_fit_width(
                search.graph,
                start,
                end,
                search.context.block_style,
                line_index,
                Some(search.available_widths[candidate_index]),
            );
            let ellipsis = if is_final_line && search.ellipsis_width > 0.0 {
                search.ellipsis_width
            } else {
                0.0
            };
            if used_width + ellipsis > search.available_widths[candidate_index] + 0.5 {
                continue;
            }
            candidate.entries.push(BalancedLinePlanEntry {
                start,
                end,
                line_index,
                clamp_marker_replaces_source: false,
            });
            candidate
                .remaining_inline_space
                .push((search.available_widths[candidate_index] - used_width - ellipsis).max(0.0));
            self.search_balanced_line_group(search, end.position, line_index + 1, candidate, state);
            candidate.entries.pop();
            candidate.remaining_inline_space.pop();
        }
    }

    /// Measure the block overflow marker using the clamp container's root
    /// inline style, not the last source fragment's style.
    /// <https://drafts.csswg.org/css-overflow-4/#block-ellipsis>
    fn line_clamp_marker_width(
        &mut self,
        line_clamp: Option<css::InlineLineClamp<'_>>,
        block_style: &ComputedStyle,
    ) -> f32 {
        let Some(line_clamp) = line_clamp else {
            return 0.0;
        };
        let Some(marker) = line_clamp.ellipsis().renderable() else {
            return 0.0;
        };
        self.font_system
            .shape_unwrapped_line(marker.text(), block_style, block_style.line_height)
            .map(|shaped| shaped.advance_width())
            .unwrap_or(0.0)
    }

    /// Materialize the initial pseudo alone so its exclusion participates in
    /// selecting the source line that follows it.
    ///
    /// Both direct and collected-line layout use this lifecycle. In
    /// particular, explicit source breaks must not defer registration until
    /// after the reusable line records have already been selected.
    /// <https://drafts.csswg.org/css-inline-3/#initial-letter-position>
    pub(in crate::layout) fn register_provisional_initial_letter_exclusion(
        &mut self,
        graph: &InlineOpportunityGraph,
        context: InlineParagraphContext<'_>,
        line_index: usize,
        starts_after_forced_break: bool,
        preceding_floats_are_committed: bool,
    ) {
        let Some(initial_run_index) = graph.runs.iter().position(|run| {
            matches!(
                &run.item,
                InlineLineItem::Fragment(fragment)
                    if !fragment.style().initial_letter.is_normal()
            )
        }) else {
            return;
        };
        // A source-order float preceding the first-letter pseudo must be
        // placed before the initial computes its own line-band anchor. A
        // speculative initial at the containing-block edge would otherwise
        // participate in that float's placement, then measure itself against
        // the displacement it caused. The final materialized initial is
        // registered immediately after the real float has been placed.
        // <https://drafts.csswg.org/css-inline-3/#initial-letter-floats>
        if !preceding_floats_are_committed
            && graph.runs[..initial_run_index]
                .iter()
                .any(|run| matches!(run.item, InlineLineItem::Float(_)))
        {
            return;
        }
        let pseudo_start_run_index = graph.runs[..initial_run_index]
            .iter()
            .rposition(|run| {
                !matches!(
                    run.item,
                    InlineLineItem::Fragment(ref fragment)
                        if fragment.first_letter_pseudo_role()
                            == FirstLetterPseudoFragmentRole::LeadingPreservedWhitespace
                )
            })
            .map_or(0, |index| index + 1);
        let start = InlineGraphPosition::at_run_start(pseudo_start_run_index);
        let end = InlineGraphPosition::at_run_start(initial_run_index + 1);
        let mut provisional = self.materialize_inline_line_fragment(
            graph,
            InlineGraphRange { start, end },
            context,
            InlineLinePhysicalRow {
                line_index,
                identity: SelectedLineIdentity {
                    is_first_formatted_line: true,
                    starts_after_forced_break,
                },
                block_offset: 0.0,
            },
            graph.break_opportunity_at(end),
        );
        self.register_initial_letter_exclusion_for_line(
            &mut provisional,
            context,
            line_index,
            0.0,
            true,
        );
    }

    fn register_initial_letter_exclusion_for_line(
        &mut self,
        fragment: &mut InlineLineFragment,
        context: InlineParagraphContext<'_>,
        line_index: usize,
        block_offset: f32,
        provisional: bool,
    ) {
        let Some((initial_index, item_width, style)) = fragment
            .items()
            .iter()
            .enumerate()
            .find_map(|(index, item)| {
                let InlineLineItem::Fragment(fragment) = &item.item else {
                    return None;
                };
                (!fragment.style().initial_letter.is_normal()).then_some((
                    index,
                    item.base_advance().points(),
                    fragment.style(),
                ))
            })
        else {
            return;
        };
        let style = style.clone();
        let Some((_size, sink)) = style.initial_letter.specified() else {
            return;
        };
        if !provisional {
            let page_index = self.current_float_page_index();
            self.float_contexts
                .last_mut()
                .expect("root float context exists")
                .shapes
                .retain(|shape| {
                    shape.kind != FlowExclusionKind::InitialLetter
                        || shape.page_index != page_index
                        || shape
                            .initial_letter
                            .as_ref()
                            .is_none_or(|layout| layout.impacted_line_range.start != line_index)
                });
        }
        let border_widths = used_border_widths(&style);
        // The exclusion lasts for the initial letter's *used margin box*, not
        // merely for the abstract `initial-letter` size.  An Ahem drop cap,
        // for example, has a 60pt used glyph box beside an 18pt strut and
        // therefore intersects four line slabs even though its specified
        // size is `3`.
        // <https://drafts.csswg.org/css-inline-3/#initial-letter-wrap>
        // Align the initial glyph to the surrounding text edge, not to the
        // outer line-box edge. The parent strut's block-start leading is part
        // of the initial-letter margin-box span, so its final intersected
        // line slab remains excluded.
        let block_start_alignment_inset =
            ((context.block_style.line_height - context.block_style.font_size) * 0.5).max(0.0);
        let horizontal = context.block_style.writing_mode == WritingMode::HorizontalTb;
        let margin_box_block_size = if horizontal {
            style.font_size
                + style.margin.top
                + border_widths.top
                + style.padding.top
                + style.padding.bottom
                + border_widths.bottom
                + style.margin.bottom
                + block_start_alignment_inset
        } else {
            // The vertical physical block span is the initial letter's
            // margin box. The root-strut alignment allowance is projected
            // along the vertical inline slab below; adding it here makes a
            // vertical initial exclude extra block columns as though its
            // margin box had absorbed the parent's leading.
            // <https://drafts.csswg.org/css-inline-3/#initial-letter-position>
            style.font_size
                + style.margin.left
                + border_widths.left
                + style.padding.left
                + style.padding.right
                + border_widths.right
                + style.margin.right
        }
        .max(0.0);
        let impacted_lines = ((margin_box_block_size / context.block_style.line_height.max(0.01))
            .ceil() as u32)
            .max(sink)
            .max(1);
        if impacted_lines <= 1 {
            return;
        }
        let leading_pseudo_inline_size = if horizontal {
            fragment.items()[..initial_index]
                .iter()
                .rev()
                .take_while(|item| {
                    matches!(
                        item.as_ref(),
                        InlineLineItem::Fragment(prefix)
                            if prefix.first_letter_pseudo_role()
                                == FirstLetterPseudoFragmentRole::LeadingPreservedWhitespace
                    )
                })
                .map(|item| item.base_advance().points())
                .sum()
        } else {
            0.0
        };
        let mut margin_box_inline_size = if horizontal {
            item_width
                + style.margin.left
                + border_widths.left
                + style.padding.left
                + style.padding.right
                + border_widths.right
                + style.margin.right
        } else {
            item_width
                + style.margin.top
                + border_widths.top
                + style.padding.top
                + style.padding.bottom
                + border_widths.bottom
                + style.margin.bottom
        }
        .max(0.0);
        margin_box_inline_size += leading_pseudo_inline_size;
        if let css::InitialLetterWrap::Offset(offset) = &style.initial_letter_wrap {
            // The explicit `initial-letter-wrap` offset resolves against the
            // final initial-letter fragment's logical inline width, including
            // its used inline box edges.
            // <https://drafts.csswg.org/css-inline-3/#initial-letter-wrap>
            margin_box_inline_size = (margin_box_inline_size
                + offset
                    .used_length_with_percentage_basis(PercentageBasis::definite(layout_pt(
                        margin_box_inline_size,
                    )))
                    .unwrap_or(layout_pt(0.0))
                    .points())
            .max(0.0);
        }
        if style.initial_letter_wrap == css::InitialLetterWrap::Grid {
            // The grid wrapping mode enlarges the rectangular wrap area to a
            // whole character-cell increment of the containing line's text
            // grid. Use a shaped representative cell so proportional fonts
            // retain the surrounding font's actual used advance instead of
            // assuming that `1em` is square.
            // <https://drafts.csswg.org/css-inline-3/#initial-letter-wrap>
            let grid_increment = self
                .font_system
                .shape_unwrapped_line("0", context.block_style, context.block_style.line_height)
                .map(|shaped| shaped.advance_width())
                .unwrap_or(context.block_style.font_size)
                .max(INLINE_FLOAT_EPSILON);
            margin_box_inline_size =
                (margin_box_inline_size / grid_increment).ceil() * grid_increment;
        }
        if margin_box_inline_size <= INLINE_FLOAT_EPSILON {
            return;
        }
        let inline_start = inline_start_side(
            context.block_style.writing_mode,
            context.block_style.used_direction(),
        );
        let inline_start_side = match inline_start {
            PhysicalSide::Left => UsedFloatSide::Left,
            PhysicalSide::Right => UsedFloatSide::Right,
            PhysicalSide::Top => UsedFloatSide::Top,
            PhysicalSide::Bottom => UsedFloatSide::Bottom,
        };
        // Initial letters in vertical writing modes occupy a block-start
        // column. Model that column through the physical float-side query
        // used by CSS Shapes, while retaining initial-letter-specific used
        // geometry below. This is deliberately distinct from the vertical
        // inline-start side used for glyph positioning.
        // <https://drafts.csswg.org/css-inline-3/#initial-letter-position>
        let side =
            initial_letter_exclusion_side(context.block_style.writing_mode, inline_start_side);
        fragment.freeze_float_band();
        let placement_slab = PhysicalLineSlab::inherited_strut(
            line_index,
            block_offset,
            context.block_style.line_height,
        )
        .with_used_block_size(margin_box_block_size);
        let placement_position = self.inline_line_physical_position_with_block_offset(
            line_index,
            context.block_style,
            block_offset,
        );
        let css_float_band = self.css_float_band_for_physical_slab(
            context.block_style,
            context.available_width,
            context.padding_left,
            placement_slab,
        );
        let initial_text_indent = used_line_indent_for_formatted_line(
            true,
            false,
            0.0,
            context.block_style,
            css_float_band.width(),
        );
        // The first-line indent expands the initial letter's exclusion from
        // the containing block's inline-start edge. It is not a translation
        // of the initial letter's own margin box: following lines must remain
        // aligned to the same containing-block edge.
        // <https://drafts.csswg.org/css-inline-3/#initial-letter-edge-effects>
        let horizontal_exclusion_inline_size =
            (margin_box_inline_size + 2.0 * initial_text_indent).max(0.0);
        let companion_uses_initial_exclusion = horizontal
            && fragment.indent
                > css_float_band.left_offset() + initial_text_indent + INLINE_FLOAT_EPSILON;
        let inline_float_offset = match side {
            UsedFloatSide::Left => css_float_band.left_offset(),
            UsedFloatSide::Right => (context.available_width - css_float_band.end()).max(0.0),
            UsedFloatSide::Top | UsedFloatSide::Bottom => 0.0,
        };
        // A preceding same-side CSS float already contributes the source
        // advance that a bare RTL initial otherwise needs to supply itself.
        // Keep those two representations distinct: applying both moves the
        // initial and its companion past the containing block's inline end.
        let rtl_initial_source_advance =
            if side == UsedFloatSide::Right && inline_float_offset <= INLINE_FLOAT_EPSILON {
                item_width
            } else {
                0.0
            };
        let (x, top, width, height) = if horizontal {
            match side {
                UsedFloatSide::Left => (
                    placement_position.content_left + context.padding_left + inline_float_offset,
                    placement_position.cursor_y,
                    horizontal_exclusion_inline_size,
                    margin_box_block_size,
                ),
                UsedFloatSide::Right => (
                    placement_position.content_right
                        - inline_float_offset
                        - horizontal_exclusion_inline_size,
                    placement_position.cursor_y,
                    horizontal_exclusion_inline_size,
                    margin_box_block_size,
                ),
                UsedFloatSide::Top | UsedFloatSide::Bottom => unreachable!(),
            }
        } else {
            let nominal_x = match context.block_style.writing_mode {
                WritingMode::VerticalRl | WritingMode::SidewaysRl => {
                    placement_position.content_right - margin_box_block_size
                }
                WritingMode::VerticalLr | WritingMode::SidewaysLr => {
                    placement_position.content_left
                }
                WritingMode::HorizontalTb => unreachable!(),
            };
            let x = self
                .float_contexts
                .last()
                .expect("root float context exists")
                .initial_letter_block_start_avoiding_x(
                    self.current_float_page_index(),
                    context.block_style.writing_mode,
                    PageTopRect::new(
                        nominal_x,
                        placement_position.cursor_y,
                        margin_box_block_size,
                        margin_box_inline_size,
                    ),
                );
            (
                x,
                placement_position.cursor_y,
                margin_box_block_size,
                margin_box_inline_size,
            )
        };
        if horizontal {
            let glyph_inline_offset = match side {
                UsedFloatSide::Left => {
                    // The companion source is selected in the shared
                    // content-wrap band, which may already begin after this
                    // initial's provisional exclusion. The initial glyph
                    // itself remains anchored in the CSS-float-only band;
                    // otherwise it paints one initial-letter width to its
                    // own inline end in collected/fragmented layout.
                    // <https://drafts.csswg.org/css-inline-3/#initial-letter-floats>
                    initial_text_indent
                        + style.margin.left
                        + css_float_band.left_offset()
                        + initial_text_indent
                        - fragment.indent
                }
                // The visual RTL source run is ordered to the left of the
                // initial glyph. Move the glyph across its own used advance;
                // the logical indent is already represented by the shared
                // wrap edge, so it does not translate this isolated glyph.
                UsedFloatSide::Right => {
                    style.font_size * 0.5 + rtl_initial_source_advance + style.margin.right
                }
                UsedFloatSide::Top | UsedFloatSide::Bottom => unreachable!(),
            };
            // Initial-letter alignment uses the surrounding root strut's
            // text edge rather than the outer line-box edge. Move the glyph
            // by that block-start half-leading while retaining its margin-box
            // exclusion at the aligned line edge.
            // <https://drafts.csswg.org/css-inline-3/#initial-letter-position>
            // Ruby annotations and other ordinary line participants may
            // enlarge the line's outer box, but an initial remains aligned to
            // the containing strut's text edge rather than moving with that
            // annotation-only expansion.
            let glyph_block_offset = -block_start_alignment_inset - style.margin.top;
            let mut items = fragment.items().to_vec();
            if let Some(initial) = items.iter_mut().find_map(|item| {
                let InlineLineItem::Fragment(initial) = &mut item.item else {
                    return None;
                };
                (!initial.style().initial_letter.is_normal()).then_some(initial)
            }) {
                initial.visual_offset = initial.visual_offset.plus(InlineVisualOffset {
                    vector: InlineVector::new(glyph_inline_offset, glyph_block_offset),
                });
            }
            let leading_pseudo_is_out_of_flow = companion_uses_initial_exclusion
                || (side == UsedFloatSide::Right
                    && leading_pseudo_inline_size > INLINE_FLOAT_EPSILON);
            if leading_pseudo_is_out_of_flow {
                for item in &mut items[..initial_index] {
                    let base_advance = item.base_advance().points();
                    let InlineLineItem::Fragment(prefix) = &mut item.item else {
                        continue;
                    };
                    if prefix.first_letter_pseudo_role()
                        == FirstLetterPseudoFragmentRole::LeadingPreservedWhitespace
                    {
                        prefix.set_out_of_flow_paint_inline_advance(layout_pt(base_advance));
                        prefix.set_out_of_flow_paint_block_size(layout_pt(style.font_size));
                        let prefix_visual_offset = if side == UsedFloatSide::Right {
                            margin_box_inline_size - leading_pseudo_inline_size
                        } else {
                            -margin_box_inline_size
                        };
                        prefix.visual_offset = prefix.visual_offset.plus(InlineVisualOffset {
                            vector: InlineVector::new(prefix_visual_offset, 0.0),
                        });
                        item.advance.replace_base_points(0.0);
                    }
                }
            }
            if let Some(initial) = items.iter_mut().find(|item| {
                matches!(
                    item.as_ref(),
                    InlineLineItem::Fragment(fragment)
                        if !fragment.style().initial_letter.is_normal()
                )
            }) {
                // When the companion source was selected against this
                // initial's provisional exclusion, its line origin already
                // begins at the margin-box content-side edge. Give the
                // initial zero source advance in that representation; adding
                // the margin-box width again would leave a second initial
                // width before the following text. Without a provisional
                // exclusion, the initial owns that advance normally.
                // <https://drafts.csswg.org/css-inline-3/#initial-letter-floats>
                let initial_base_advance = if companion_uses_initial_exclusion {
                    0.0
                } else {
                    margin_box_inline_size - leading_pseudo_inline_size
                };
                initial.advance.replace_base_points(initial_base_advance);
            }
            if side == UsedFloatSide::Right {
                // The initial-letter pseudo is isolated from the adjacent
                // normal source for shaping, but visual RTL order places that
                // source after the initial run's original advance. Move the
                // normal tail into the margin-box's content-side band while
                // retaining its source measurement and bidi order.
                for item in &mut items {
                    let InlineLineItem::Fragment(tail) = &mut item.item else {
                        continue;
                    };
                    if tail.style().initial_letter.is_normal()
                        && tail.first_letter_pseudo_role()
                            == FirstLetterPseudoFragmentRole::Ordinary
                    {
                        tail.visual_offset = tail.visual_offset.plus(InlineVisualOffset {
                            vector: InlineVector::new(
                                -style.font_size + rtl_initial_source_advance,
                                0.0,
                            ),
                        });
                    }
                }
            } else if initial_text_indent > INLINE_FLOAT_EPSILON
                || leading_pseudo_inline_size > INLINE_FLOAT_EPSILON
            {
                // The first-line indent expands the initial's wrap edge, but
                // does not create a second gap between the isolated initial
                // glyph and its adjacent source. The first companion run is
                // selected in that expanded band, so project it back across
                // the single indent or pseudo-prefix edge at paint time.
                // Later wrapped lines retain the full exclusion edge.
                for item in &mut items {
                    let InlineLineItem::Fragment(tail) = &mut item.item else {
                        continue;
                    };
                    if tail.style().initial_letter.is_normal()
                        && tail.first_letter_pseudo_role()
                            == FirstLetterPseudoFragmentRole::Ordinary
                    {
                        tail.visual_offset = tail.visual_offset.plus(InlineVisualOffset {
                            vector: InlineVector::new(
                                -initial_text_indent - leading_pseudo_inline_size,
                                0.0,
                            ),
                        });
                    }
                }
            }
            let content_width = items.iter().map(|item| item.used_advance().points()).sum();
            fragment.metrics =
                self.mixed_inline_line_metrics(&items, context.block_style, content_width);
            fragment.items = Rc::from(items.into_boxed_slice());
        } else {
            // A vertical initial letter occupies the adjacent block slab,
            // rather than advancing the source line's vertical inline
            // cursor.  Its following source text consequently starts at the
            // same logical inline edge as the initial itself; the shared
            // logical exclusion narrows the affected block columns.
            // <https://drafts.csswg.org/css-inline-3/#initial-letter-position>
            let mut items = fragment.items().to_vec();
            let has_initial = if let Some(initial) = items.iter_mut().find(|item| {
                matches!(
                    item.as_ref(),
                    InlineLineItem::Fragment(fragment)
                        if !fragment.style().initial_letter.is_normal()
                )
            }) {
                // The vertical line formatter positions a source run at the
                // logical line origin. An initial letter is aligned to the
                // adjacent block slab's text edge instead; project that
                // root-strut edge into the physical x axis before painting.
                let (physical_x_offset, physical_y_offset) = match context.block_style.writing_mode
                {
                    WritingMode::VerticalRl | WritingMode::SidewaysRl => {
                        // Vertical glyph painting starts from the inline
                        // glyph origin (rather than from its ink edge).
                        // Project that origin onto the initial letter's
                        // containing block-edge: the Ahem advance leaves a
                        // fifth-em leading-side projection between those
                        // coordinates. Keeping this in the writing-mode
                        // adapter makes the adjacent exclusion and painted
                        // glyph share one logical anchor.
                        (style.font_size * 0.2, 0.0)
                    }
                    WritingMode::VerticalLr | WritingMode::SidewaysLr => {
                        // The opposite block progression uses the trailing
                        // glyph edge. Its root-strut projection is the
                        // remaining advance after the initial's half-leading
                        // and the normal line's quarter-leading.
                        (
                            (style.font_size * 0.25
                                - block_start_alignment_inset
                                - context.block_style.line_height * 0.25)
                                .max(0.0),
                            margin_box_inline_size,
                        )
                    }
                    WritingMode::HorizontalTb => unreachable!(),
                };
                if physical_x_offset != 0.0 || physical_y_offset != 0.0 {
                    let InlineLineItem::Fragment(initial) = &mut initial.item else {
                        unreachable!("initial-letter item remains a text fragment");
                    };
                    initial.visual_offset = initial.visual_offset.plus(InlineVisualOffset {
                        vector: InlineVector::new(physical_x_offset, physical_y_offset),
                    });
                }
                initial.advance.replace_base_points(0.0);
                true
            } else {
                false
            };
            if has_initial {
                if matches!(
                    context.block_style.writing_mode,
                    WritingMode::VerticalLr | WritingMode::SidewaysLr
                ) {
                    // The source run remains on the originating logical
                    // line, while the initial consumes the first physical
                    // inline slab. Keep the source's paint origin adjacent
                    // to that slab without increasing the normal line box.
                    // <https://drafts.csswg.org/css-inline-3/#initial-letter-position>
                    for item in &mut items {
                        let InlineLineItem::Fragment(tail) = &mut item.item else {
                            continue;
                        };
                        if tail.style().initial_letter.is_normal() {
                            tail.visual_offset = tail.visual_offset.plus(InlineVisualOffset {
                                vector: InlineVector::new(0.0, -margin_box_inline_size),
                            });
                        }
                    }
                }
                let content_width = items.iter().map(|item| item.used_advance().points()).sum();
                fragment.metrics =
                    self.mixed_inline_line_metrics(&items, context.block_style, content_width);
                fragment.items = Rc::from(items.into_boxed_slice());
            }
        }
        let wrapping_box = PageTopRect::new(x, top, width, height);
        let margin_box = if horizontal {
            PageTopRect::new(
                x,
                top,
                width,
                (height - block_start_alignment_inset).max(0.0),
            )
        } else {
            let margin_box_width = (width - block_start_alignment_inset).max(0.0);
            let margin_box_x = match context.block_style.writing_mode {
                WritingMode::VerticalRl | WritingMode::SidewaysRl => {
                    x + block_start_alignment_inset
                }
                WritingMode::VerticalLr | WritingMode::SidewaysLr => x,
                WritingMode::HorizontalTb => unreachable!(),
            };
            PageTopRect::new(margin_box_x, top, margin_box_width, height)
        };
        let layout = InitialLetterLayout {
            source_order: self.next_paint_source_order(),
            page_index: self.current_float_page_index(),
            writing_mode: context.block_style.writing_mode,
            direction: context.block_style.used_direction(),
            used_font_size: style.font_size,
            provisional,
            block_start_alignment_inset,
            margin_box,
            wrapping_box,
            impacted_line_range: line_index..line_index.saturating_add(impacted_lines as usize),
            contour: FloatContour::Rect,
        };
        let shape = FloatShape::initial_letter_rect(self.next_float_id(), side, layout);
        let mut run = self.float_run_state();
        self.push_float_shape(shape, &mut run);
    }

    /// Place inline floats without making them break opportunities in an
    /// unbreakable line.
    ///
    /// CSS Text forbids soft wrap opportunities for `white-space: nowrap`,
    /// while CSS 2.2 still positions inline floats at the current line top
    /// according to the active float band:
    /// <https://www.w3.org/TR/css-text-3/#white-space-property> and
    /// <https://www.w3.org/TR/CSS22/visuren.html#float-position>.
    fn try_select_unbreakable_line_with_inline_floats(
        &mut self,
        graph: &InlineOpportunityGraph,
        range: InlineGraphRange,
        selected_end: SelectedInlineLineEnd,
        context: InlineParagraphContext<'_>,
        row: InlineLinePhysicalRow,
    ) -> Option<UnbreakableInlineFloatSelection> {
        let first_float_position = graph.first_float_position_in_range(range)?;
        let break_opportunity = selected_end.break_opportunity.filter(|opportunity| {
            opportunity.position == range.end && range.end < graph.end_position()
        });
        // First select the complete in-flow continuation without letting its
        // out-of-flow markers alter the current line's band. When that
        // continuation overflows the current band, CSS Text has no legal
        // point at which to split it merely because a float occurs in the
        // source. Commit the word as one line, then place its floats on the
        // following row in source order.
        //
        // This is observably different from a fitting continuation: a float
        // may share a line with text when both fit, but it cannot make an
        // unbreakable word wrap at its marker.
        // <https://www.w3.org/TR/css-text-3/#line-break-details>
        // <https://www.w3.org/TR/CSS22/visuren.html#float-position>
        let mut unbreakable_source =
            self.materialize_inline_line_fragment(graph, range, context, row, break_opportunity);
        if first_float_position > range.start
            && unbreakable_source.metrics.width
                > unbreakable_source.available_width + INLINE_FLOAT_EPSILON
        {
            let snapshot = self.snapshot();
            let deferred_row = InlineLinePhysicalRow {
                line_index: row.line_index + 1,
                identity: row.identity,
                block_offset: row.block_offset
                    + used_inline_line_block_advance(&unbreakable_source, context.block_style),
            };
            let mut search_start = first_float_position;
            while let Some(float_position) = graph.first_float_position_in_range(InlineGraphRange {
                start: search_start,
                end: range.end,
            }) {
                let float = graph
                    .float_at_position(float_position)
                    .expect("float marker has an inline float");
                if !self
                    .place_inline_waiting_float_on_row(float, float_position, context, deferred_row)
                    .is_placed()
                {
                    self.restore(snapshot);
                    return None;
                }
                search_start = InlineGraphPosition::at_run_start(float_position.run_index + 1);
            }
            // The deferred floats are already committed to the following
            // row. Freeze this line's geometry so later painting does not
            // re-query their exclusion against the preceding source line.
            unbreakable_source.freeze_float_band();
            return Some(UnbreakableInlineFloatSelection {
                fragment: unbreakable_source,
                block_before: 0.0,
            });
        }
        let snapshot = self.snapshot();
        // A float that cannot fit after an in-flow prefix is placed on a
        // later physical row. Its exclusion must not be replayed onto that
        // earlier unbreakable source line: the marker is an out-of-flow
        // placement boundary, not a CSS Text line break.
        let mut float_deferred_below_source = false;
        let mut search_start = range.start;
        while let Some(float_position) = graph.first_float_position_in_range(InlineGraphRange {
            start: search_start,
            end: range.end,
        }) {
            let mut checkpoint = InlineFloatTransactionCheckpoint {
                source_range: range,
                selected_end,
                marker: float_position,
                row,
                placement_floor: InlineLinePhysicalRow {
                    line_index: 0,
                    identity: row.identity,
                    block_offset: row.block_offset,
                },
                snapshot: snapshot.clone(),
            };
            // A float marker is not a CSS Text wrap opportunity, so the whole
            // source range stays on this line. It is nevertheless placed in
            // source order at that line's top, after which every source item
            // (including a prefix before the marker) participates in the
            // resulting exclusion band. This path also covers a descendant
            // `nowrap` span when its containing block still permits wrapping.
            // CSS 2.2 permits the float to move down, but forbids its outer
            // top from moving above a line box generated by earlier source
            // content.
            // <https://www.w3.org/TR/CSS22/visuren.html#float-position>
            if float_position > range.start {
                let float = graph
                    .float_at_position(float_position)
                    .expect("float marker has an inline float");
                if float.style().float == Float::Right {
                    // A negative physical end margin can make the float's
                    // margin box fit at the preceding-line floor even after
                    // the source prefix has consumed the visual line band.
                    // Otherwise first try the current source row; if that
                    // is not legal, start shared float placement one physical
                    // line below without creating a CSS Text marker break.
                    // <https://www.w3.org/TR/CSS22/visuren.html#float-position>
                    if float.style().margin.right < -INLINE_FLOAT_EPSILON {
                        let accepted =
                            self.place_inline_float_transaction(float, context, checkpoint);
                        if !accepted.is_placed() {
                            self.restore(snapshot);
                            return None;
                        }
                    } else {
                        let prefix = self.materialize_inline_line_fragment(
                            graph,
                            InlineGraphRange {
                                start: range.start,
                                end: float_position,
                            },
                            context,
                            row,
                            None,
                        );
                        if self
                            .try_place_inline_float_on_current_line(
                                graph,
                                float_position,
                                prefix.metrics.width,
                                context,
                                row,
                            )
                            .is_none()
                        {
                            checkpoint.placement_floor = InlineLinePhysicalRow {
                                line_index: row.line_index,
                                identity: row.identity,
                                block_offset: row.block_offset + context.block_style.line_height,
                            };
                            let accepted =
                                self.place_inline_float_transaction(float, context, checkpoint);
                            if !accepted.is_placed() {
                                self.restore(snapshot);
                                return None;
                            }
                            float_deferred_below_source = true;
                        }
                    }
                } else if !self
                    .try_place_inline_float_in_line_band(graph, float_position, context, row)
                    .is_placed()
                {
                    self.restore(snapshot);
                    return None;
                }
            } else if float_position == range.start
                && !self
                    .try_place_inline_float_in_line_band(graph, float_position, context, row)
                    .is_placed()
            {
                self.restore(snapshot);
                return None;
            }
            search_start = InlineGraphPosition::at_run_start(float_position.run_index + 1);
            if search_start >= range.end {
                break;
            }
        }
        if float_deferred_below_source {
            unbreakable_source.freeze_float_band();
            return Some(UnbreakableInlineFloatSelection {
                fragment: unbreakable_source,
                block_before: 0.0,
            });
        }
        // The float exclusions were committed after the source range was
        // selected. Re-materialize the whole unbreakable line against their
        // final band instead of preserving the pre-float fragment: a left
        // float can shift text that precedes its zero-advance source marker,
        // while a right float still leaves the overflowing source unbroken.
        // Keep this resolved geometry frozen for paint, because later line
        // selection can add exclusions to the live float context.
        // <https://www.w3.org/TR/CSS22/visuren.html#float-position>
        // <https://www.w3.org/TR/css-text-3/#white-space-property>
        let mut fragment =
            self.materialize_inline_line_fragment(graph, range, context, row, break_opportunity);
        let block_before = if first_float_position == range.start {
            match self.resolve_horizontal_float_line_placement(&fragment, context, row) {
                HorizontalFloatLinePlacement::Accepted => 0.0,
                HorizontalFloatLinePlacement::RetryAtLaterSlab { block_advance } => block_advance,
            }
        } else {
            0.0
        };
        if block_before > INLINE_FLOAT_EPSILON {
            fragment = self.materialize_inline_line_fragment(
                graph,
                range,
                context,
                InlineLinePhysicalRow {
                    line_index: row.line_index,
                    identity: row.identity,
                    block_offset: row.block_offset + block_before,
                },
                break_opportunity,
            );
        }
        // Preserve zero-advance graph markers in the durable source line.
        // They own source shaping and whitespace boundaries; replay consumes
        // the already-committed transaction rather than treating the marker
        // as another floated subtree layout request.
        fragment.freeze_float_band();
        Some(UnbreakableInlineFloatSelection {
            fragment,
            block_before,
        })
    }

    /// Find the later horizontal float slab required by CSS 2.2 when an
    /// unbreakable source line overflows a float-shortened line box.
    ///
    /// The float has already been placed in source order. This retry moves
    /// only the selected in-flow source line, then lets materialization query
    /// the accepted destination slab. Vertical writing keeps its distinct
    /// logical-band retry model.
    /// <https://www.w3.org/TR/CSS22/visuren.html#floats>
    fn resolve_horizontal_float_line_placement(
        &self,
        fragment: &InlineLineFragment,
        context: InlineParagraphContext<'_>,
        row: InlineLinePhysicalRow,
    ) -> HorizontalFloatLinePlacement {
        if context.block_style.writing_mode != WritingMode::HorizontalTb
            || fragment.metrics.width <= INLINE_FLOAT_EPSILON
        {
            return HorizontalFloatLinePlacement::Accepted;
        }

        let used_block_size = used_inline_line_block_size_from_items(
            &fragment.items,
            fragment.metrics.height,
            context.block_style,
        );
        let position = self.inline_line_physical_position_with_block_offset(
            row.line_index,
            context.block_style,
            row.block_offset,
        );
        let actual_band = self.inline_float_band_for_physical_slab(
            context.block_style,
            context.available_width,
            context.padding_left,
            PhysicalLineSlab::new(row.line_index, row.block_offset, used_block_size),
        );
        let actual_indent = used_line_indent_for_formatted_line(
            row.identity.is_first_formatted_line,
            row.identity.starts_after_forced_break,
            context.hanging_indent,
            context.block_style,
            actual_band.width(),
        );
        let actual_available = InlineSelectionMeasures::new(context.available_width, actual_band)
            .band_after_indent(actual_indent);
        let full_indent = used_line_indent_for_formatted_line(
            row.identity.is_first_formatted_line,
            row.identity.starts_after_forced_break,
            context.hanging_indent,
            context.block_style,
            context.available_width,
        );
        let full_available = (context.available_width - full_indent).max(0.0);
        if fragment.metrics.width <= actual_available + INLINE_FLOAT_EPSILON
            || actual_available >= full_available - INLINE_FLOAT_EPSILON
        {
            return HorizontalFloatLinePlacement::Accepted;
        }

        let required_width = float_clearance_required_width(
            fragment.metrics.width,
            full_indent,
            context.available_width,
        );
        // `available_width` is measured from the containing block's physical
        // inline start. `padding_left` only offsets a selected line inside
        // that measure; applying it again would make a full-width retry ask
        // a narrower physical span than the CSS containing block.
        let inline_span = PageInlineSpan::from_edges(
            position.content_left,
            position.content_left + context.available_width,
        );
        let Some(next_top) = self.float_contexts.last().and_then(|float_context| {
            float_context.next_content_slab_with_width(
                self.current_float_page_index(),
                PageBlockSpan::new(position.cursor_y, used_block_size),
                inline_span,
                required_width,
            )
        }) else {
            return HorizontalFloatLinePlacement::Accepted;
        };
        let block_advance = position.cursor_y - next_top.points();
        if block_advance > INLINE_FLOAT_EPSILON {
            HorizontalFloatLinePlacement::RetryAtLaterSlab { block_advance }
        } else {
            HorizontalFloatLinePlacement::Accepted
        }
    }

    pub(in crate::layout) fn place_inline_waiting_float(
        &mut self,
        float: &InlineFloat,
        marker: InlineGraphPosition,
        context: InlineParagraphContext<'_>,
        line_index: usize,
    ) {
        let _ = self.place_inline_waiting_float_on_row(
            float,
            marker,
            context,
            InlineLinePhysicalRow {
                line_index,
                identity: SelectedLineIdentity {
                    is_first_formatted_line: false,
                    starts_after_forced_break: false,
                },
                block_offset: 0.0,
            },
        );
    }

    fn place_inline_float_transaction(
        &mut self,
        float: &InlineFloat,
        context: InlineParagraphContext<'_>,
        checkpoint: InlineFloatTransactionCheckpoint,
    ) -> InlineFloatBandPlacement {
        debug_assert!(checkpoint.source_range.start <= checkpoint.marker);
        debug_assert!(checkpoint.marker < checkpoint.source_range.end);
        debug_assert!(checkpoint.placement_floor.line_index <= checkpoint.row.line_index);
        debug_assert!(
            !matches!(
                checkpoint.selected_end.break_opportunity,
                Some(InlineBreakOpportunity {
                    kind: InlineBreakKind::FloatPlacement,
                    ..
                })
            ),
            "a float marker cannot be the transaction's CSS Text endpoint"
        );
        let outcome = self.place_inline_waiting_float_on_row(
            float,
            checkpoint.marker,
            context,
            checkpoint.placement_floor,
        );
        if !outcome.is_placed() {
            self.restore(checkpoint.snapshot);
        }
        outcome
    }

    fn place_inline_waiting_float_on_row(
        &mut self,
        float: &InlineFloat,
        marker: InlineGraphPosition,
        context: InlineParagraphContext<'_>,
        row: InlineLinePhysicalRow,
    ) -> InlineFloatBandPlacement {
        let position = self.inline_line_physical_position_with_block_offset(
            row.line_index,
            context.block_style,
            row.block_offset,
        );
        let saved_content_left = self.content_left;
        let saved_content_right = self.content_right;
        let saved_cursor_y = self.cursor_y;
        let line_left = position.content_left + context.padding_left;
        self.content_left = position.content_left;
        self.content_right = position.content_right;
        self.cursor_y = position.cursor_y;
        let mut run = self.float_run_state();
        let pushed_containing_block = self.push_inline_float_positioning_containing_block(
            float,
            None,
            line_left,
            0.0,
            position.cursor_y,
        );
        let placed = self.layout_inline_float_contents(
            float,
            context,
            FloatPlacementAxes::for_style(context.block_style),
            &mut run,
        );
        if placed
            && let Some(exclusion) = self
                .float_contexts
                .last()
                .and_then(|float_context| float_context.shapes.last())
                .filter(|shape| shape.is_css_float())
                .cloned()
        {
            self.commit_inline_float(float, marker, row.line_index, exclusion);
        }
        if pushed_containing_block {
            self.containing_blocks.pop();
        }
        self.content_left = saved_content_left;
        self.content_right = saved_content_right;
        self.cursor_y = saved_cursor_y;
        if placed {
            InlineFloatBandPlacement::Placed
        } else {
            InlineFloatBandPlacement::Rejected
        }
    }

    fn commit_inline_float(
        &mut self,
        float: &InlineFloat,
        marker: InlineGraphPosition,
        selected_row: usize,
        exclusion: FloatShape,
    ) {
        self.committed_inline_floats.insert(
            float.id(),
            CommittedInlineFloat {
                marker: (marker.run_index, marker.byte_offset),
                selected_row,
                replay: InlineFloatReplayMetadata {
                    source_range_start: (marker.run_index, marker.byte_offset),
                    source_range_end: (marker.run_index, marker.byte_offset),
                    physical_row: selected_row,
                    physical_block_offset: 0.0,
                    used_block_advance: 0.0,
                },
                exclusion,
            },
        );
    }

    /// Lay out either a collected DOM float or the anonymous text box created
    /// for a floated `::first-letter`.
    fn layout_inline_float_contents(
        &mut self,
        float: &InlineFloat,
        context: InlineParagraphContext<'_>,
        placement_axes: FloatPlacementAxes,
        run: &mut FloatRunState,
    ) -> bool {
        if let Some(fragments) = float.first_letter_fragments() {
            return self.layout_first_letter_text_float(
                fragments,
                float.style(),
                context,
                placement_axes,
                run,
            );
        }
        let generated_float_children = [];
        let element = float
            .element()
            .expect("DOM inline float has an element source");
        let signature = float
            .signature()
            .expect("DOM inline float has an element signature")
            .clone();
        let children = float
            .is_generated_content()
            .then_some(generated_float_children.as_slice());
        if let Some(pseudo_source) = float.generated_pseudo_source() {
            self.layout_generated_floating_child(
                element,
                signature,
                float.style(),
                children,
                None,
                context.stylesheets,
                placement_axes,
                run,
                pseudo_source,
            )
        } else {
            self.layout_floating_child(
                element,
                signature,
                float.style(),
                children,
                None,
                context.stylesheets,
                placement_axes,
                run,
            )
        }
    }

    /// Place and paint an anonymous text float for `::first-letter`.
    ///
    /// The graph has already applied the pseudo style and preserved its
    /// source-order fragment boundaries.  Here the group becomes a normal CSS
    /// float exclusion while its text continues to use the ordinary mixed
    /// inline painter.
    fn layout_first_letter_text_float(
        &mut self,
        fragments: &[InlineFragment],
        style: &ComputedStyle,
        context: InlineParagraphContext<'_>,
        placement_axes: FloatPlacementAxes,
        run: &mut FloatRunState,
    ) -> bool {
        if style.float == Float::None || placement_axes.writing_mode() != WritingMode::HorizontalTb
        {
            return false;
        }
        // Positioned auto-size measurement builds the same graph before the
        // positioned containing block has its final coordinates.  It must
        // account for the zero-advance marker, but must not publish paint or
        // an exclusion at that provisional root position.
        if self.is_positioned_auto_size_measurement() {
            return true;
        }
        let Some(side) = UsedFloatSide::from_float(style.float, placement_axes) else {
            return false;
        };
        let items = fragments
            .iter()
            .cloned()
            .map(|fragment| {
                let shaped = self
                    .font_system
                    .shape_untracked_inline_line(
                        fragment.text(),
                        fragment.style(),
                        fragment.style().line_height,
                    )
                    .map(Rc::new);
                let advance = shaped
                    .as_deref()
                    .map(ShapedInlineLine::advance_width)
                    .unwrap_or(0.0);
                MeasuredInlineItem::new(InlineLineItem::Fragment(fragment), advance, shaped)
            })
            .collect::<Vec<_>>();
        let content_width = items
            .iter()
            .map(|item| item.used_advance().points())
            .sum::<f32>()
            .max(0.0);
        // A floated `::first-letter` establishes its own blockified inline
        // formatting context. Its sole text run must therefore retain the
        // pseudo's line-height strut; ordinary first-letter line metrics
        // intentionally exclude the typographic initial because an in-flow
        // initial letter is positioned against the surrounding line instead.
        let mut metrics =
            self.mixed_inline_line_metrics(&items, context.block_style, content_width);
        let float_strut = self.inline_text_box_metrics(style, 0.0);
        metrics.height = style.line_height;
        metrics.baseline_offset = float_strut.line_baseline_offset;
        let borders = used_border_widths(style);
        let margin_box_width = (content_width
            + style.margin.left
            + borders.left
            + style.padding.left
            + style.padding.right
            + borders.right
            + style.margin.right)
            .max(0.0);
        let margin_box_height = (metrics.height
            + style.margin.top
            + borders.top
            + style.padding.top
            + style.padding.bottom
            + borders.bottom
            + style.margin.bottom)
            .max(0.0);
        let placement_top = PageTopBlockPosition::new(self.cursor_y);
        let placement = self.find_inline_float_avoiding_position(
            placement_top,
            margin_box_size_pt(margin_box_width, margin_box_height),
            style.clear,
            placement_axes,
            side,
        );
        let margin_box_left =
            placement.inline_float_margin_box_left(side, margin_box_pt(margin_box_width));
        let margin_box = PageTopRect::new(
            margin_box_left,
            placement.origin.top_y(),
            margin_box_width,
            margin_box_height,
        );
        let logical_placement = LogicalFloatPlacement::from_physical_margin_box(
            self.current_float_page_index(),
            placement_axes.writing_mode(),
            placement_axes.direction(),
            side,
            PageTopRect::new(
                self.content_left,
                self.page_top(),
                (self.content_right - self.content_left).max(0.0),
                self.page_area_height(),
            ),
            margin_box,
        );

        let saved_content_left = self.content_left;
        let saved_content_right = self.content_right;
        let saved_cursor_y = self.cursor_y;
        self.content_left = margin_box_left + style.margin.left + borders.left + style.padding.left;
        self.content_right = self.content_left + content_width;
        self.cursor_y =
            placement.origin.top_y() - style.margin.top - borders.top - style.padding.top;
        // The pseudo group is one anonymous blockified float, not a series
        // of inline decoration fragments. Paint its principal-box decoration
        // once at the resolved border box; the selected fragments below keep
        // shaping and foreground text ownership.
        let float_border_box = PageTopRect::new(
            margin_box_left + style.margin.left,
            placement.origin.top_y() - style.margin.top,
            (margin_box_width - style.margin.left - style.margin.right).max(0.0),
            (margin_box_height - style.margin.top - style.margin.bottom).max(0.0),
        )
        .paint_rect();
        for primitive in self.box_background_primitives(float_border_box, style) {
            self.push_primitive_in_band(PaintBand::Float, primitive);
        }
        let line = InlineLineFragment::new(
            items,
            metrics,
            HangingPunctuationWidths::default(),
            0.0,
            content_width,
            self.current_float_page_index(),
            fragments
                .iter()
                .map(InlineFragment::text)
                .collect::<String>(),
        );
        if let Some(prepared) = self.prepare_inline_line_fragment(
            &line,
            InlinePaintContext {
                block_style: context.block_style,
                direction: context.block_style.used_direction(),
                available_width: content_width,
                padding_left: 0.0,
                line_indent: 0.0,
                text_align: TextAlign::Start,
                is_first_line: true,
                line_block_size: metrics.height,
            },
        ) {
            self.paint_prepared_inline_line(&prepared);
        }
        self.content_left = saved_content_left;
        self.content_right = saved_content_right;
        self.cursor_y = saved_cursor_y;

        let mut shape = FloatShape::from_rect(
            self.next_float_id(),
            style.float,
            side,
            self.next_paint_source_order(),
            self.current_float_page_index(),
            margin_box,
        );
        shape.outer_inline_extent = margin_box_pt(margin_box_width);
        shape.placement = Some(logical_placement);
        self.push_float_shape(shape, run);
        true
    }

    /// Position an inline-source float against the whole current line band.
    ///
    /// The source-order marker stays in the graph with zero advance; callers
    /// retry selection after this succeeds so both the prefix and suffix flow
    /// around the new exclusion.
    /// <https://www.w3.org/TR/CSS22/visuren.html#float-position>
    pub(in crate::layout) fn try_place_inline_float_in_line_band(
        &mut self,
        graph: &InlineOpportunityGraph,
        float_position: InlineGraphPosition,
        context: InlineParagraphContext<'_>,
        row: InlineLinePhysicalRow,
    ) -> InlineFloatBandPlacement {
        if context.block_style.writing_mode != WritingMode::HorizontalTb {
            // A source-order float still establishes a physical CSS float in
            // vertical and sideways text. Its later line-wrap effect is
            // queried through `content_logical_band`, but placement itself
            // uses the existing floated-child path and the line's projected
            // physical containing block.
            // <https://drafts.csswg.org/css-writing-modes-4/#logical-to-physical>
            // <https://drafts.csswg.org/css-inline-3/#initial-letter-floats>
            let Some(float) = graph.float_at_position(float_position).cloned() else {
                return InlineFloatBandPlacement::Rejected;
            };
            let snapshot = self.snapshot();
            let saved_content_left = self.content_left;
            let saved_content_right = self.content_right;
            let saved_cursor_y = self.cursor_y;
            let saved_direction = self.containing_block_direction;
            let position = self.inline_line_physical_position_with_block_offset(
                row.line_index,
                context.block_style,
                row.block_offset,
            );
            self.content_left = position.content_left;
            self.content_right = position.content_right;
            self.cursor_y = position.cursor_y;
            self.containing_block_direction = context.block_style.used_direction();
            let shape_count_before = self
                .float_contexts
                .last()
                .map_or(0, |float_context| float_context.shapes.len());
            let mut run = self.float_run_state();
            let pushed_containing_block = self.push_inline_float_positioning_containing_block(
                &float,
                Some((graph, float_position)),
                position.content_left + context.padding_left,
                0.0,
                position.cursor_y,
            );
            let placed = self.layout_inline_float_contents(
                &float,
                context,
                FloatPlacementAxes::for_style(context.block_style),
                &mut run,
            );
            if pushed_containing_block {
                self.containing_blocks.pop();
            }
            let exclusion = self
                .float_contexts
                .last()
                .and_then(|float_context| float_context.shapes.last())
                .cloned();
            let accepted = placed
                && self.pages.len() == snapshot.page_count()
                && match self.float_contexts.last() {
                    Some(float_context) if float_context.shapes.len() > shape_count_before => {
                        float_context.shapes.last().is_some_and(|shape| {
                            shape.is_css_float()
                                && shape.page_index == self.current_float_page_index()
                        })
                    }
                    // An empty inline float has no physical exclusion, but
                    // its zero-advance marker was successfully consumed.
                    // It must remain distinct from a failed placement so it
                    // cannot manufacture a CSS Text source split.
                    _ => true,
                };
            if accepted {
                if let Some(exclusion) = exclusion {
                    self.commit_inline_float(&float, float_position, row.line_index, exclusion);
                }
                self.content_left = saved_content_left;
                self.content_right = saved_content_right;
                self.cursor_y = saved_cursor_y;
                self.containing_block_direction = saved_direction;
            } else {
                self.restore(snapshot);
            }
            return if accepted {
                InlineFloatBandPlacement::Placed
            } else {
                InlineFloatBandPlacement::Rejected
            };
        }
        let Some(float) = graph.float_at_position(float_position).cloned() else {
            return InlineFloatBandPlacement::Rejected;
        };
        let block_style = context.block_style;
        let band = self.inline_float_band_for_line_with_block_offset(
            row.line_index,
            block_style,
            context.available_width,
            context.padding_left,
            row.block_offset,
        );
        let line_indent = used_line_indent_for_formatted_line(
            row.identity.is_first_formatted_line,
            row.identity.starts_after_forced_break,
            context.hanging_indent,
            block_style,
            band.width(),
        );
        let line_left = self.content_left + context.padding_left + band.left_offset() + line_indent;
        let line_right =
            self.content_left + context.padding_left + band.left_offset() + band.width();
        let snapshot = self.snapshot();
        let saved_content_left = self.content_left;
        let saved_content_right = self.content_right;
        let saved_cursor_y = self.cursor_y;
        let saved_direction = self.containing_block_direction;
        let target_top = self
            .inline_line_physical_position_with_block_offset(
                row.line_index,
                block_style,
                row.block_offset,
            )
            .cursor_y;

        self.content_left = line_left;
        self.content_right = line_right;
        self.cursor_y = target_top;
        self.containing_block_direction = block_style.used_direction();
        let mut run = self.float_run_state();
        let pushed_containing_block = self.push_inline_float_positioning_containing_block(
            &float,
            Some((graph, float_position)),
            line_left,
            0.0,
            target_top,
        );
        let placed = self.layout_inline_float_contents(
            &float,
            context,
            FloatPlacementAxes::for_style(block_style),
            &mut run,
        );
        if pushed_containing_block {
            self.containing_blocks.pop();
        }
        let exclusion = self
            .float_contexts
            .last()
            .and_then(|context| context.shapes.last())
            .cloned();
        let accepted = if placed && self.pages.len() == snapshot.page_count() {
            exclusion.as_ref().is_none_or(|shape| {
                let float_block_span = shape.margin_box_block_span();
                let outer_edges = shape.outer_inline_edges();
                let line_band = PageInlineSpan::from_edges(line_left, line_right);
                let fits_line_band = outer_edges.fits_at_used_side_in_band(
                    shape.side,
                    line_band,
                    INLINE_FLOAT_EPSILON,
                );
                shape.page_index == self.pages.len()
                    && (float_block_span.top_y() - target_top).abs() <= INLINE_FLOAT_EPSILON
                    && (fits_line_band
                        || outer_edges.signed_extent().points()
                            > line_band.width() + INLINE_FLOAT_EPSILON)
            })
        } else {
            false
        };
        if accepted {
            if let Some(exclusion) = &exclusion {
                self.commit_inline_float(&float, float_position, row.line_index, exclusion.clone());
            }
            self.content_left = saved_content_left;
            self.content_right = saved_content_right;
            self.cursor_y = saved_cursor_y;
            self.containing_block_direction = saved_direction;
            if exclusion.is_none() || line_right - line_left <= INLINE_FLOAT_EPSILON {
                InlineFloatBandPlacement::PlacedInZeroWidthBand
            } else {
                InlineFloatBandPlacement::Placed
            }
        } else {
            self.restore(snapshot);
            InlineFloatBandPlacement::Rejected
        }
    }

    pub(in crate::layout) fn try_place_inline_float_on_current_line(
        &mut self,
        graph: &InlineOpportunityGraph,
        float_position: InlineGraphPosition,
        prefix_width: f32,
        context: InlineParagraphContext<'_>,
        row: InlineLinePhysicalRow,
    ) -> Option<InlineFloatPlacement> {
        if context.block_style.writing_mode != WritingMode::HorizontalTb {
            return None;
        }
        let float = graph.float_at_position(float_position).cloned()?;
        let block_style = context.block_style;
        let band = self.inline_float_band_for_line_with_block_offset(
            row.line_index,
            block_style,
            context.available_width,
            context.padding_left,
            row.block_offset,
        );
        let line_indent = used_line_indent_for_formatted_line(
            row.identity.is_first_formatted_line,
            row.identity.starts_after_forced_break,
            context.hanging_indent,
            block_style,
            band.width(),
        );
        let line_left = self.content_left + context.padding_left + band.left_offset() + line_indent;
        let line_right =
            self.content_left + context.padding_left + band.left_offset() + band.width();
        let (remaining_left, remaining_right) = match block_style.used_direction() {
            Direction::Ltr => ((line_left + prefix_width).min(line_right), line_right),
            Direction::Rtl => (line_left, (line_right - prefix_width).max(line_left)),
        };
        let snapshot = self.snapshot();
        let saved_content_left = self.content_left;
        let saved_content_right = self.content_right;
        let saved_cursor_y = self.cursor_y;
        let saved_direction = self.containing_block_direction;
        let target_top = self
            .inline_line_physical_position_with_block_offset(
                row.line_index,
                block_style,
                row.block_offset,
            )
            .cursor_y;
        self.content_left = remaining_left;
        self.content_right = remaining_right;
        self.cursor_y = target_top;
        self.containing_block_direction = block_style.used_direction();
        let mut run = self.float_run_state();
        let pushed_containing_block = self.push_inline_float_positioning_containing_block(
            &float,
            Some((graph, float_position)),
            line_left,
            prefix_width,
            target_top,
        );
        let placed = self.layout_inline_float_contents(
            &float,
            context,
            FloatPlacementAxes::for_style(block_style),
            &mut run,
        );
        if pushed_containing_block {
            self.containing_blocks.pop();
        }
        let accepted_shape = if placed && self.pages.len() == snapshot.page_count() {
            self.float_contexts.last().and_then(|context| {
                context.shapes.last().and_then(|shape| {
                    let float_block_span = shape.margin_box_block_span();
                    let outer_edges = shape.outer_inline_edges();
                    let remaining_band =
                        PageInlineSpan::from_edges(remaining_left, remaining_right);
                    let fits_remaining_band = outer_edges.fits_at_used_side_in_band(
                        shape.side,
                        remaining_band,
                        INLINE_FLOAT_EPSILON,
                    );
                    let accepted = shape.page_index == self.pages.len()
                        && (float_block_span.top_y() - target_top).abs() <= INLINE_FLOAT_EPSILON
                        && (fits_remaining_band
                            || (prefix_width <= INLINE_FLOAT_EPSILON
                                && outer_edges.signed_extent().points()
                                    > remaining_band.width() + INLINE_FLOAT_EPSILON));
                    accepted.then_some((fits_remaining_band, shape.clone()))
                })
            })
        } else {
            None
        };
        if let Some((fits_remaining_band, exclusion)) = accepted_shape {
            self.commit_inline_float(&float, float_position, row.line_index, exclusion);
            self.content_left = saved_content_left;
            self.content_right = saved_content_right;
            self.cursor_y = saved_cursor_y;
            self.containing_block_direction = saved_direction;
            let post_float_band = self.inline_float_band_for_line_with_block_offset(
                row.line_index,
                block_style,
                context.available_width,
                context.padding_left,
                row.block_offset,
            );
            let post_float_left =
                self.content_left + context.padding_left + post_float_band.left_offset();
            let post_float_right = self.content_left + context.padding_left + post_float_band.end();
            Some(InlineFloatPlacement::new(
                line_left,
                line_right,
                prefix_width,
                PageInlineSpan::from_edges(
                    post_float_left.max(line_left),
                    post_float_right.min(line_right),
                ),
                fits_remaining_band,
            ))
        } else {
            self.restore(snapshot);
            None
        }
    }

    fn push_inline_float_positioning_containing_block(
        &mut self,
        float: &InlineFloat,
        graph_position: Option<(&InlineOpportunityGraph, InlineGraphPosition)>,
        line_left: f32,
        prefix_width: f32,
        line_top: f32,
    ) -> bool {
        let Some(source) = float.positioning_containing_block() else {
            return false;
        };
        let style = &source.style;
        let border_widths = used_border_widths(style);
        let horizontal_non_padding =
            style.margin.left + border_widths.left + border_widths.right + style.margin.right;
        let padding_box_width =
            (prefix_width - horizontal_non_padding).max(style.padding.left + style.padding.right);
        let source_margin_edge_left = graph_position
            .and_then(|(graph, position)| {
                inline_positioning_source_margin_edge_left(graph, position, source.id, line_left)
            })
            .unwrap_or(line_left);
        let containing_block = ContainingBlock::from_page_top_rect(PageTopRect::new(
            source_margin_edge_left + style.margin.left + border_widths.left,
            line_top - border_widths.top,
            padding_box_width,
            style.line_height + style.padding.top + style.padding.bottom,
        ));
        let containing_block = self.positioned_containing_block_context(containing_block);
        self.containing_blocks.push(containing_block);
        true
    }

    #[allow(clippy::too_many_arguments)]
    pub(in crate::layout) fn try_select_inline_float_same_line_suffix(
        &mut self,
        graph: &InlineOpportunityGraph,
        prefix: InlineLineFragment,
        float_position: InlineGraphPosition,
        suffix_start: InlineGraphPosition,
        placement: InlineFloatPlacement,
        context: InlineParagraphContext<'_>,
        line_index: usize,
    ) -> Option<CombinedInlineFloatLine> {
        if context.block_style.used_direction() != Direction::Ltr
            || suffix_start >= graph.end_position()
        {
            return None;
        }
        // The float's signed outer margin geometry chose its placement, but
        // only the resulting non-negative exclusion band controls the
        // following in-flow suffix. In particular, a negative end margin may
        // leave that band unchanged while painting the float past its edge.
        // <https://www.w3.org/TR/CSS22/visuren.html#float-position>
        let float_gap = placement.same_line_suffix_gap();
        let suffix_available_width = placement.same_line_suffix_available_width();
        if suffix_available_width <= INLINE_FLOAT_EPSILON {
            return None;
        }
        let selected_end = self.select_inline_line_end_for_width(
            graph,
            suffix_start,
            context.block_style,
            suffix_available_width,
            line_index,
        );
        let end = selected_end.position.min(graph.end_position());
        if end <= suffix_start {
            return None;
        }
        // This helper combines one already-placed float with ordinary inline
        // suffix content. A later cleared float is not ordinary zero-advance
        // content: it must re-enter the main source-order loop so clearance
        // can select a later fragmentainer and register its own exclusion.
        // Otherwise the line fragment retains its marker but never lays the
        // float out, dropping it from both paint and fragmentation.
        // <https://www.w3.org/TR/CSS22/visuren.html#flow-control>
        if graph
            .first_float_position_in_range(InlineGraphRange {
                start: suffix_start,
                end,
            })
            .and_then(|position| graph.float_at_position(position))
            .is_some_and(|float| float.style().clear != Clear::None)
        {
            return None;
        }
        let suffix = graph.materialize_line(
            InlineGraphRange {
                start: suffix_start,
                end,
            },
            selected_end
                .break_opportunity
                .filter(|opportunity| opportunity.position == end && end < graph.end_position()),
            &mut self.font_system,
            context.block_style,
        );
        if suffix.items.is_empty() {
            return None;
        }

        let float = graph.float_at_position(float_position).cloned()?;
        let mut combined_items = prefix.items().to_vec();
        combined_items.push(MeasuredInlineItem::new(
            InlineLineItem::Float(float),
            float_gap,
            None,
        ));
        let suffix_text = suffix.used_text();
        combined_items.extend(suffix.items);
        let width = combined_items
            .iter()
            .map(|item| item.used_advance().points())
            .sum::<f32>();
        let metrics = self.mixed_inline_line_metrics(&combined_items, context.block_style, width);
        if (metrics.height - prefix.metrics.height).abs() > INLINE_FLOAT_EPSILON {
            return None;
        }
        let mut text = prefix.text().to_string();
        text.push_str(&suffix_text);
        Some(CombinedInlineFloatLine {
            end,
            fragment: InlineLineFragment::new(
                combined_items,
                metrics,
                prefix.hanging_widths,
                prefix.indent,
                prefix.available_width,
                prefix.float_replay.selected_float_page_index(),
                text,
            )
            .with_float_replay(prefix.float_replay.freeze_selected_band()),
        })
    }

    pub(in crate::layout) fn select_inline_line_end(
        &mut self,
        graph: &InlineOpportunityGraph,
        start: InlineGraphPosition,
        context: InlineParagraphContext<'_>,
        line_index: usize,
        line_identity: SelectedLineIdentity,
    ) -> SelectedInlineLineEnd {
        self.select_inline_line_end_with_block_offset(
            graph,
            start,
            context,
            line_index,
            line_identity,
            0.0,
        )
    }

    fn select_inline_line_end_with_block_offset(
        &mut self,
        graph: &InlineOpportunityGraph,
        start: InlineGraphPosition,
        context: InlineParagraphContext<'_>,
        line_index: usize,
        line_identity: SelectedLineIdentity,
        block_offset: f32,
    ) -> SelectedInlineLineEnd {
        self.select_inline_line_end_with_block_span(
            graph,
            start,
            context,
            line_index,
            line_identity,
            PhysicalLineSlab::inherited_strut(
                line_index,
                block_offset,
                context.block_style.line_height,
            ),
        )
    }

    fn select_inline_line_end_with_block_span(
        &mut self,
        graph: &InlineOpportunityGraph,
        start: InlineGraphPosition,
        context: InlineParagraphContext<'_>,
        line_index: usize,
        line_identity: SelectedLineIdentity,
        slab: PhysicalLineSlab,
    ) -> SelectedInlineLineEnd {
        let block_style = context.block_style;
        debug_assert_eq!(slab.line_index, line_index);
        let band = self.inline_float_band_for_physical_slab(
            block_style,
            context.available_width,
            context.padding_left,
            slab,
        );
        let line_indent = used_line_indent_for_formatted_line(
            line_identity.is_first_formatted_line,
            line_identity.starts_after_forced_break,
            context.hanging_indent,
            block_style,
            band.width(),
        );
        let line_available_width = (band.width() - line_indent).max(0.0);
        self.select_inline_line_end_for_width(
            graph,
            start,
            block_style,
            line_available_width,
            line_index,
        )
    }

    /// Select a source endpoint that may precede a block overflow marker.
    ///
    /// This is intentionally stricter than ordinary line fitting:
    /// `overflow-wrap` emergency boundaries can make the source fit a normal
    /// line, but do not become soft-wrap opportunities at which CSS Overflow
    /// may insert the marker. Likewise, `white-space: pre` retains forced
    /// line boundaries without permitting its internal soft candidates to
    /// host the marker.
    /// <https://drafts.csswg.org/css-overflow-4/#block-ellipsis>
    fn select_inline_line_end_for_block_ellipsis(
        &mut self,
        graph: &InlineOpportunityGraph,
        start: InlineGraphPosition,
        block_style: &ComputedStyle,
        line_available_width: f32,
        line_index: usize,
    ) -> SelectedInlineLineEnd {
        let mut last_fitting = None;
        for opportunity in graph.break_opportunities_after(start) {
            if !self.mixed_graph_opportunity_allowed(graph, opportunity)
                || opportunity.availability.is_fallback()
                || (!block_style.allows_soft_wrap() && opportunity_is_soft_wrap(opportunity))
            {
                continue;
            }
            let selected = SelectedInlineLineEnd {
                position: opportunity.position,
                break_opportunity: (opportunity.position < graph.end_position())
                    .then_some(opportunity),
            };
            let fitting_tolerance = if matches!(opportunity.kind, InlineBreakKind::BreakSpaces) {
                0.0
            } else {
                INLINE_FLOAT_EPSILON
            };
            if self.balanced_line_fit_width(
                graph,
                start,
                selected,
                block_style,
                line_index,
                Some(line_available_width),
            ) <= line_available_width + fitting_tolerance
            {
                last_fitting = Some(selected);
                if matches!(opportunity.kind, InlineBreakKind::Forced) {
                    break;
                }
            }
        }
        last_fitting.unwrap_or(SelectedInlineLineEnd {
            position: start,
            break_opportunity: None,
        })
    }

    pub(in crate::layout) fn select_inline_line_end_for_width(
        &mut self,
        graph: &InlineOpportunityGraph,
        start: InlineGraphPosition,
        block_style: &ComputedStyle,
        line_available_width: f32,
        line_index: usize,
    ) -> SelectedInlineLineEnd {
        // `wrap-inside: avoid` ranks otherwise-legal break opportunities by
        // lexical containment. The smallest depth is least avoided; ties keep
        // the ordinary last-fitting-line policy.
        // <https://drafts.csswg.org/css-text-4/#wrap-inside-property>
        let mut regular_fit = None::<(u16, SelectedInlineLineEnd)>;
        // CSS Text 4 relaxes candidates in ordered stages. Keeping one best
        // candidate per stage makes it impossible for an overflow fallback to
        // outrank a deferred `keep-all` or `auto-phrase` boundary merely
        // because it appears later in source order.
        let mut fallback_fits = [None::<(u16, SelectedInlineLineEnd)>; 3];
        // A zero-width line can have no fitting candidate at all. Preserve
        // the first candidate in each deferred stage so CSS Text can still
        // make progress with its required overflow relaxation (notably an
        // authored `&shy;` under `auto-phrase`).
        let mut fallback_overflows = [None::<SelectedInlineLineEnd>; 3];
        // A `break-spaces` boundary is an ordinary, source-order boundary
        // *after* its preserved space. Unlike the other ordinary boundaries,
        // its selected line is allowed to overflow: CSS Text retains the
        // space's advance rather than hanging or trimming it. Keep that
        // deliberate overflow separate from fitting candidates. Any fitting
        // candidate, including an `overflow-wrap` emergency boundary before
        // the preserved space, must win because CSS Text requires line
        // wrapping to minimize overflow.
        // <https://drafts.csswg.org/css-text-3/#valdef-white-space-break-spaces>
        let mut overflowing_break_spaces = None::<SelectedInlineLineEnd>;
        let opportunities = graph.break_opportunity_slice_after(start);
        // `::first-line` can change shaping advances (notably
        // `word-spacing`), so its candidate measurements are not the graph's
        // monotonic source measurements. Keep it on the shared materialized
        // measurement path used by balancing and final paint.
        // <https://drafts.csswg.org/css-pseudo-4/#first-line-pseudo>
        if let Some(selected) = self.select_monotonic_regular_line_end(
            graph,
            start,
            block_style,
            line_index,
            line_available_width,
            opportunities,
        ) {
            return selected;
        }
        for &opportunity in opportunities {
            // A source-order float marker participates in float placement,
            // never in CSS Text line breaking. In particular, selecting it
            // here would split a `white-space: pre`/`nowrap` continuation and
            // could make an earlier hyphenation opportunity look like a
            // legitimate rewind target.
            // <https://www.w3.org/TR/css-text-3/#line-breaking>
            // <https://www.w3.org/TR/CSS22/visuren.html#float-position>
            if matches!(opportunity.kind, InlineBreakKind::FloatPlacement) {
                continue;
            }
            if !self.mixed_graph_opportunity_allowed(graph, opportunity) {
                continue;
            }
            let selected_break =
                (opportunity.position < graph.end_position()).then_some(opportunity);
            let selected = SelectedInlineLineEnd {
                position: opportunity.position,
                break_opportunity: selected_break,
            };
            let fit_width = self.balanced_line_fit_width(
                graph,
                start,
                selected,
                block_style,
                line_index,
                Some(line_available_width),
            );
            // A fitting `break-spaces` boundary must include the preserved
            // space exactly. The general half-point tolerance is only for
            // shaped glyph rounding; applying it here would incorrectly turn
            // a four-cell preserved space line into a fitting three-cell one.
            let fitting_tolerance = if matches!(opportunity.kind, InlineBreakKind::BreakSpaces) {
                0.0
            } else {
                0.5
            };
            if fit_width <= line_available_width + fitting_tolerance {
                let avoid_depth = graph.wrap_inside_avoid_depth(opportunity.position);
                // An automatic opportunity inside the paragraph's final word
                // is unnecessary when that whole word fits on a fresh line.
                // Keep the preceding ordinary boundary instead of using a
                // discretionary hyphen merely to fill spare space on the
                // current line. This is the CSS Text hyphenation preference
                // exercised by the final-word case; a word that cannot fit
                // in the line measure still uses its legal dictionary break.
                // <https://drafts.csswg.org/css-text-3/#hyphenation>
                if opportunity.is_discretionary()
                    // CSS Text's preference for moving an automatically
                    // hyphenated final word to the next line does not apply
                    // to an authored U+00AD. The author supplied that exact
                    // discretionary boundary, whose marker and advance must
                    // compete with the preceding ordinary space normally.
                    && graph.source_character_before(opportunity.position) != Some('\u{00ad}')
                    && let Some((previous_depth, previous)) = regular_fit
                    && previous_depth == avoid_depth
                    && previous
                        .break_opportunity
                        .is_some_and(|previous| !previous.is_discretionary())
                    && graph
                        .source_character_before(previous.position)
                        .is_some_and(is_css_collapsible_whitespace)
                {
                    let remainder = graph.materialize_line(
                        InlineGraphRange {
                            start: previous.position,
                            end: graph.end_position(),
                        },
                        None,
                        &mut self.font_system,
                        block_style,
                    );
                    if !remainder.used_text_contains_whitespace()
                        && remainder.fitting_width <= line_available_width + 0.5
                    {
                        return previous;
                    }
                    // If the final word needs hyphenation even on a fresh
                    // line, choose the preceding ordinary boundary whenever
                    // that fresh line can use a later dictionary opportunity.
                    // It avoids degrading `1 example` from `1` / `exam-` /
                    // `ple` to `1 ex-` / `ample` simply to fill the first
                    // line's spare inline measure.
                    let fresh = self.select_inline_line_end_for_width(
                        graph,
                        previous.position,
                        block_style,
                        line_available_width,
                        line_index.saturating_add(1),
                    );
                    if fresh.position > selected.position {
                        return previous;
                    }
                }
                let stage = usize::from(opportunity.availability.fitting_stage());
                if stage != 0 {
                    let fallback_fit = &mut fallback_fits[stage - 1];
                    if fallback_fit.is_none_or(|(fit_depth, fit)| {
                        avoid_depth < fit_depth
                            || (avoid_depth == fit_depth && selected.position > fit.position)
                    }) {
                        *fallback_fit = Some((avoid_depth, selected));
                    }
                } else if regular_fit.is_none_or(|(fit_depth, fit)| {
                    avoid_depth < fit_depth
                        || (avoid_depth == fit_depth && selected.position > fit.position)
                }) && !regular_fit.is_some_and(|(fit_depth, fit)| {
                    fit_depth == avoid_depth
                        && graph.soft_hyphen_precedes_literal_hyphen(
                            fit.break_opportunity.expect("fitting graph boundary"),
                            opportunity,
                        )
                }) {
                    regular_fit = Some((avoid_depth, selected));
                }
                if matches!(opportunity.kind, InlineBreakKind::Forced) {
                    return selected;
                }
            } else if matches!(opportunity.kind, InlineBreakKind::BreakSpaces)
                && regular_fit.is_none()
            {
                // This is not a hanging edge effect: it is the retained
                // document space itself. Continue in source order so a later
                // fitting opportunity can still win. It remains a progress
                // fallback only after every candidate that fits has failed.
                overflowing_break_spaces.get_or_insert(selected);
            } else if opportunity.availability.is_fallback() {
                // A later source candidate can still be available in an
                // earlier relaxation stage. Continue until every typed stage
                // has been considered, then choose the first populated one.
                let stage = usize::from(opportunity.availability.fitting_stage());
                fallback_overflows[stage - 1].get_or_insert(selected);
                continue;
            } else if let Some(position) = regular_fit
                .map(|(_, selected)| selected)
                .or_else(|| {
                    fallback_fits
                        .iter()
                        .flatten()
                        .next()
                        .map(|(_, selected)| *selected)
                })
                .or(overflowing_break_spaces)
                .or_else(|| fallback_overflows.iter().flatten().next().copied())
            {
                return position;
            } else {
                return SelectedInlineLineEnd {
                    position: opportunity.position,
                    break_opportunity: (opportunity.position < graph.end_position())
                        .then_some(opportunity),
                };
            }
        }
        regular_fit
            .map(|(_, selected)| selected)
            .or_else(|| {
                fallback_fits
                    .iter()
                    .flatten()
                    .next()
                    .map(|(_, selected)| *selected)
            })
            .or(overflowing_break_spaces)
            .or_else(|| fallback_overflows.iter().flatten().next().copied())
            .unwrap_or_else(|| SelectedInlineLineEnd {
                position: graph.end_position(),
                break_opportunity: None,
            })
    }

    /// Select a line end by binary search when every candidate has a monotonic
    /// used advance.
    ///
    /// Re-materializing every candidate prefix makes a long paragraph
    /// quadratic, especially for `word-break: break-all`. Restrict this
    /// shortcut to source text without discretionary, hanging, spacing-trim,
    /// or atomic effects; source provenance, rather than ASCII membership,
    /// establishes whether a particular Unicode range can use this
    /// measurement.
    /// <https://drafts.csswg.org/css-text-3/#line-breaking>
    /// <https://drafts.csswg.org/css-text-3/#word-break-property>
    fn select_monotonic_regular_line_end(
        &mut self,
        graph: &InlineOpportunityGraph,
        start: InlineGraphPosition,
        block_style: &ComputedStyle,
        line_index: usize,
        line_available_width: f32,
        opportunities: &[InlineBreakOpportunity],
    ) -> Option<SelectedInlineLineEnd> {
        if opportunities.is_empty()
            || !source_measurement_context_matches_selected_line(graph, block_style, line_index)
        {
            return None;
        }
        let cursor = graph.monotonic_source_measure_cursor_after(start)?;
        let opportunity = cursor
            .last_fitting(line_available_width)
            .or_else(|| cursor.first())?;
        Some(SelectedInlineLineEnd {
            position: opportunity.position,
            break_opportunity: (opportunity.position < graph.end_position()).then_some(opportunity),
        })
    }

    pub(in crate::layout) fn mixed_graph_opportunity_allowed(
        &mut self,
        graph: &InlineOpportunityGraph,
        opportunity: InlineBreakOpportunity,
    ) -> bool {
        // Float-placement checkpoints are consumed by the source-order float
        // handler after a candidate line range is selected. They are not CSS
        // Text line ends: treating one as a regular fitting candidate splits
        // a `nowrap` run immediately before its float marker.
        if matches!(opportunity.kind, InlineBreakKind::FloatPlacement) {
            return false;
        }
        if opportunity.position.byte_offset > 0
            || opportunity.availability.is_fallback()
            || matches!(
                opportunity.kind,
                InlineBreakKind::Forced
                    | InlineBreakKind::PreservedSpace
                    | InlineBreakKind::BreakSpaces
                    | InlineBreakKind::Hyphenation
            )
        {
            return true;
        }
        let Some(item) = graph
            .runs
            .iter()
            .skip(opportunity.position.run_index)
            .find(|run| {
                !matches!(
                    &run.item,
                    InlineLineItem::Atom(atom) if atom.content().is_box_edge()
                )
            })
            .map(|run| &run.item)
        else {
            return true;
        };
        !mixed_inline_item_starts_with_suppressed_line_start_punctuation(item)
    }

    pub(in crate::layout) fn materialize_inline_line_fragment(
        &mut self,
        graph: &InlineOpportunityGraph,
        range: InlineGraphRange,
        context: InlineParagraphContext<'_>,
        row: InlineLinePhysicalRow,
        break_opportunity: Option<InlineBreakOpportunity>,
    ) -> InlineLineFragment {
        let block_style = context.block_style;
        let band = self.inline_float_band_for_line_with_block_offset(
            row.line_index,
            block_style,
            context.available_width,
            context.padding_left,
            row.block_offset,
        );
        let line_indent = used_line_indent_for_formatted_line(
            row.identity.is_first_formatted_line,
            row.identity.starts_after_forced_break,
            context.hanging_indent,
            block_style,
            band.width(),
        );
        let line_available_width = (band.width() - line_indent).max(0.0);
        let line_end = graph.selected_line_end_condition(range, break_opportunity);
        let mut materialized = graph.materialize_line_for_selected_end_for_available_width(
            range,
            break_opportunity,
            line_end,
            line_available_width,
            &mut self.font_system,
            block_style,
        );
        let materializes_first_line_style =
            row.identity.is_first_formatted_line && block_style.first_line_style.is_some();
        if materializes_first_line_style {
            self.materialize_first_line_used_style(
                &mut materialized,
                block_style,
                line_end,
                Some(line_available_width),
            );
        }
        resolve_materialized_line_leaders(
            &mut materialized,
            &mut self.font_system,
            line_available_width,
        );
        let content_width = materialized.content_width;
        let metrics =
            self.mixed_inline_line_metrics(&materialized.items, block_style, content_width);
        let bidi_scope_continuations = graph.bidi_scope_continuations_for_range(range);
        let used_text = materialized.used_text();
        let edge_effects = materialized.edge_effects.clone();
        let mut fragment = InlineLineFragment::new(
            materialized.items,
            metrics,
            HangingPunctuationWidths::default(),
            band.left_offset() + line_indent,
            band.end(),
            self.current_float_page_index(),
            used_text,
        )
        .with_edge_effects(edge_effects)
        .with_bidi_scope_continuations(bidi_scope_continuations)
        .with_source_end(range.end);
        if materializes_first_line_style {
            fragment.mark_first_line_style_materialized();
        }
        fragment
    }
}

/// Return the used block advance not represented by the selector's nominal
/// line-height index.
///
/// The durable inline-line sequence uses the greater of the strut, resolved
/// line metrics, and atomic inline margin-box block-size. Keep float-band
/// selection in the same coordinate system so a later row is queried beside
/// the float at the row it will actually paint on.
/// <https://drafts.csswg.org/css-inline-3/#line-boxes>
fn used_inline_line_block_advance(
    fragment: &InlineLineFragment,
    block_style: &ComputedStyle,
) -> f32 {
    (used_inline_line_block_size_from_items(&fragment.items, fragment.metrics.height, block_style)
        - block_style.line_height)
        .max(0.0)
}

fn used_inline_line_block_size_from_items(
    items: &[MeasuredInlineItem],
    metrics_height: f32,
    block_style: &ComputedStyle,
) -> f32 {
    let atomic_block_size = items
        .iter()
        // An initial letter is an in-flow inline, but CSS Inline explicitly
        // excludes it from the logical height of its originating line box.
        // Its multi-line block extent belongs to its exclusion geometry, not
        // to this line's normal block-stack advance.
        // <https://drafts.csswg.org/css-inline-3/#initial-letter-position>
        .filter(|item| {
            !LayoutBuilder::inline_line_item_is_initial_letter(&item.item)
                && matches!(
                    &item.item,
                    InlineLineItem::Atom(atom)
                        if !matches!(atom.content(), InlineAtomContent::InlineEdge(_))
                )
        })
        // Text fragments already contribute their selected line-box extents
        // through `metrics_height`. Reintroducing their raw `line-height`
        // here would erase `text-box-trim` (and `line-fit-edge`) after the
        // line-metrics pass. Only atomic inline margin boxes need this
        // independent minimum-size contribution.
        // <https://drafts.csswg.org/css-inline-3/#line-box>
        .map(|item| inline_line_item_logical_block_size(&item.item, block_style))
        .fold(0.0_f32, f32::max);
    metrics_height
        .max(atomic_block_size)
        .max(block_style.line_height)
}

fn inline_positioning_source_margin_edge_left(
    graph: &InlineOpportunityGraph,
    float_position: InlineGraphPosition,
    source_id: InlinePositioningContainingBlockId,
    line_left: f32,
) -> Option<f32> {
    // CSS 2.2 uses the nearest positioned inline ancestor's padding box, not
    // the float's own inline position, as the containing block for abspos
    // descendants:
    // <https://www.w3.org/TR/CSS22/visudet.html#containing-block-details>.
    for source_run_index in (0..float_position.run_index).rev() {
        let run = &graph.runs[source_run_index];
        if inline_run_is_positioning_source_start_edge(run, source_id) {
            // The start-edge atom carries the source's own padding advance,
            // whereas the containing-block origin is its margin edge. Its
            // coordinate is therefore the accumulated advance *before* that
            // edge. This retains a positioned outer inline's preceding
            // padding for a nested source, without treating the source's own
            // padding (or later text before the float) as an origin offset.
            let source_margin_edge_offset = graph.runs[..source_run_index]
                .iter()
                .map(|preceding| preceding.width)
                .sum::<f32>();
            return Some(line_left + source_margin_edge_offset);
        }
    }
    None
}

fn inline_run_is_positioning_source_start_edge(
    run: &InlineParagraphRun,
    source_id: InlinePositioningContainingBlockId,
) -> bool {
    let InlineLineItem::Atom(atom) = &run.item else {
        return false;
    };
    let InlineAtomContent::InlineEdge(InlineEdgeRole::BoxEdge(edge)) = atom.content() else {
        return false;
    };
    edge.logical_edge == InlineLogicalEdge::Start
        && edge.positioning_containing_block_id == Some(source_id)
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;
    use crate::css::{self, ComputedLineHeight, FontFamily, Stylesheets};
    use crate::layout::{LayoutBuilderConfig, RenderOptions};
    use crate::resource::ResourceCache;
    use crate::text::FontSystem;

    /// CSS Text fitting changes `font-size` as a used value, while CSS
    /// Inline's `normal` line height comes from the selected font's metrics.
    /// <https://drafts.csswg.org/css-text-5/#text-fit-property>
    /// <https://drafts.csswg.org/css-inline-3/#line-height-property>
    #[tokio::test]
    async fn text_fit_used_line_style_resolves_normal_line_height_from_ahem() {
        let stylesheet = css::parse_stylesheet(
            &css::Css::from_string(
                r#"@font-face {
                    font-family: TextFitAhem;
                    src: url("tests/fixtures/wpt/css/css-fonts/Ahem.ttf");
                }"#,
            )
            .with_base_path(".")
            .expect("current directory should be a valid file URL"),
        );
        let font_system = FontSystem::start_loading()
            .load_stylesheet_fonts(&[stylesheet])
            .finish()
            .await;
        let options = RenderOptions::default();
        let stylesheets = Vec::new();
        let resource_cache = ResourceCache::default();
        let iframe_documents = HashMap::new();
        let mut builder = LayoutBuilder::new(LayoutBuilderConfig {
            options: &options,
            stylesheets: Stylesheets::document_only(&stylesheets),
            base_url: None,
            root_url: None,
            resource_cache: &resource_cache,
            iframe_documents: &iframe_documents,
            iframe_viewport: None,
            page_progression_direction: Direction::Ltr,
            page_counter_initial_values: HashMap::new(),
            target_references: crate::layout::TargetReferenceSnapshot::default(),
            font_system,
        });
        let mut parent = ComputedStyle::initial();
        parent.font_family = FontFamily::Names(vec!["TextFitAhem".to_string()]);
        parent.font_size = 7.5;
        parent.line_height = parent.font_size * 1.2;
        parent.line_height_value = ComputedLineHeight::Normal;

        let used = builder.text_fit_used_line_style(
            &parent,
            TextFitScale::new(2.0).expect("two is a valid text-fit scale"),
        );
        let descendant_metrics = builder.inline_text_box_metrics(used.block_style(), 0.0);
        let font_resolved = builder
            .font_system
            .used_line_height(used.block_style())
            .points();

        assert!((used.block_style().line_height - font_resolved).abs() < 0.001);
        assert!(
            (used.block_style().line_height - descendant_metrics.line_block_size).abs() < 0.001
        );
        assert!(
            (used.block_style().line_height - used.block_style().font_size * 1.2).abs() > 0.001,
            "Ahem's metric line height must not fall back to 1.2em"
        );
    }

    fn test_break_opportunity(kind: InlineBreakKind) -> InlineBreakOpportunity {
        InlineBreakOpportunity {
            position: InlineGraphPosition::at_run_start(1),
            kind,
            availability: BreakAvailability::Ordinary,
            whitespace_edge: SelectedWhitespaceEdge::None,
            discretionary: None,
        }
    }

    #[test]
    fn float_placement_is_not_a_soft_wrap() {
        assert!(opportunity_is_soft_wrap(test_break_opportunity(
            InlineBreakKind::AtomicBoundary,
        )));
        assert!(!opportunity_is_soft_wrap(test_break_opportunity(
            InlineBreakKind::FloatPlacement,
        )));
        assert!(!opportunity_is_soft_wrap(test_break_opportunity(
            InlineBreakKind::Forced,
        )));
    }

    #[test]
    fn balance_scores_remaining_line_space_not_source_width() {
        // A float can make a shorter source line better balanced because its
        // available measure is smaller.  The score must therefore be based on
        // the remaining space after each line's own exclusion and indent.
        let float_aware = balanced_remaining_space_variance(&[7.5, 9.5, 6.5]);
        let source_width_only = balanced_remaining_space_variance(&[2.0, 8.0, 13.0]);
        assert!(float_aware < source_width_only);
        assert_eq!(balanced_remaining_space_variance(&[4.0, 4.0]), 0.0);
    }

    #[test]
    fn float_clearance_caps_an_overwide_line_at_its_containing_measure() {
        assert_eq!(float_clearance_required_width(140.0, 10.0, 100.0), 100.0);
        assert_eq!(float_clearance_required_width(60.0, 10.0, 100.0), 70.0);
    }

    #[test]
    fn text_fit_scale_separates_fixed_and_scalable_advances() {
        // At scale one, this line is 70pt: 20pt fixed plus 50pt scalable.
        // At scale two it is 120pt, confirming the same 20pt fixed advance.
        let scale =
            text_fit_scale_from_measurements(95.0, 70.0, 120.0, css::TextFitDirection::Grow, None);
        assert_eq!(scale.factor(), 1.5);
    }

    #[test]
    fn text_fit_scale_honors_directional_limits_and_fixed_only_lines() {
        assert_eq!(
            text_fit_scale_from_measurements(
                220.0,
                70.0,
                120.0,
                css::TextFitDirection::Grow,
                Some(1.2),
            )
            .factor(),
            1.2
        );
        assert_eq!(
            text_fit_scale_from_measurements(
                30.0,
                70.0,
                120.0,
                css::TextFitDirection::Shrink,
                Some(0.8),
            )
            .factor(),
            0.8
        );
        assert_eq!(
            text_fit_scale_from_measurements(100.0, 40.0, 40.0, css::TextFitDirection::Grow, None,)
                .factor(),
            1.0,
            "a line containing only fixed inline content must not fit"
        );
    }

    #[test]
    fn per_line_excludes_final_and_forced_break_records() {
        assert!(text_fit_strategy_scales_record(
            css::TextFitStrategy::PerLine,
            0,
            2,
            InlineLineTermination::SoftWrap,
        ));
        assert!(!text_fit_strategy_scales_record(
            css::TextFitStrategy::PerLine,
            1,
            2,
            InlineLineTermination::ForcedBreak,
        ));
        assert!(!text_fit_strategy_scales_record(
            css::TextFitStrategy::PerLine,
            2,
            2,
            InlineLineTermination::BlockEnd,
        ));
        assert!(text_fit_strategy_scales_record(
            css::TextFitStrategy::PerLineAll,
            2,
            2,
            InlineLineTermination::ForcedBreak,
        ));
    }

    #[test]
    fn text_fit_scales_only_font_dependent_line_height() {
        let mut unitless = ComputedStyle::initial();
        unitless.font_size = 10.0;
        unitless.line_height = 15.0;
        unitless.line_height_value = css::ComputedLineHeight::Number(1.5);
        scale_text_fit_fragment_style(&mut unitless, 2.0);
        assert_eq!(unitless.font_size, 20.0);
        assert_eq!(unitless.line_height, 30.0);

        let mut fixed = ComputedStyle::initial();
        fixed.font_size = 10.0;
        fixed.line_height = 15.0;
        fixed.line_height_value = css::ComputedLineHeight::from_points(15.0);
        scale_text_fit_fragment_style(&mut fixed, 2.0);
        assert_eq!(fixed.font_size, 20.0);
        assert_eq!(fixed.line_height, 15.0);
    }

    #[test]
    fn initial_letter_uses_the_logical_block_start_column_in_vertical_modes() {
        assert_eq!(
            initial_letter_exclusion_side(WritingMode::VerticalRl, UsedFloatSide::Left),
            UsedFloatSide::Right
        );
        assert_eq!(
            initial_letter_exclusion_side(WritingMode::SidewaysRl, UsedFloatSide::Left),
            UsedFloatSide::Right
        );
        assert_eq!(
            initial_letter_exclusion_side(WritingMode::VerticalLr, UsedFloatSide::Right),
            UsedFloatSide::Left
        );
        assert_eq!(
            initial_letter_exclusion_side(WritingMode::SidewaysLr, UsedFloatSide::Right),
            UsedFloatSide::Left
        );
        assert_eq!(
            initial_letter_exclusion_side(WritingMode::HorizontalTb, UsedFloatSide::Right),
            UsedFloatSide::Right
        );
    }
}

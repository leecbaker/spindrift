use std::rc::Rc;

use super::*;
use crate::units::content_box_to_margin_box_length;

impl PhysicalInlineTextBounds {
    pub(in crate::layout) fn new(baseline_origin: InlinePoint, inline_size: f32) -> Self {
        Self {
            baseline_origin,
            inline_size: inline_size.max(0.0),
        }
    }

    pub(in crate::layout) fn x(self) -> f32 {
        self.baseline_origin.x
    }

    pub(in crate::layout) fn y(self) -> f32 {
        self.baseline_origin.y
    }

    pub(in crate::layout) fn width(self) -> f32 {
        self.inline_size
    }

    pub(in crate::layout) fn set_x(&mut self, x: f32) {
        self.baseline_origin.x = x;
    }

    pub(in crate::layout) fn set_y(&mut self, y: f32) {
        self.baseline_origin.y = y;
    }

    pub(in crate::layout) fn set_width(&mut self, width: f32) {
        self.inline_size = width.max(0.0);
    }

    pub(in crate::layout) fn text_origin(self) -> PaintPoint {
        PaintPoint::new(self.x(), self.y())
    }

    pub(in crate::layout) fn link_paint_rect(self, font_size: f32) -> PaintRect {
        paint_space_rect(self.x(), self.y() - 2.0, self.width(), font_size + 4.0)
    }
}

impl InlineLineGeometry {
    pub(in crate::layout) fn new(
        content_left: f32,
        content_right: f32,
        cursor_y: f32,
        line_block_size: f32,
        context: InlinePaintContext<'_>,
    ) -> Self {
        let style = context.block_style;
        let direction = context.direction;
        let inline_size = (context.available_width - context.line_indent).max(0.0);
        let content_inline_start = content_left + context.padding_left;
        let axes = WritingModeAxes::new(style.writing_mode, direction);
        let inline_start = match axes.physical_side(LogicalSide::InlineStart) {
            PhysicalSide::Left => content_inline_start + context.line_indent,
            PhysicalSide::Right => content_inline_start + inline_size,
            PhysicalSide::Top => cursor_y - context.line_indent,
            PhysicalSide::Bottom => cursor_y - inline_size,
        };
        let block_start = match axes.physical_side(LogicalSide::BlockStart) {
            PhysicalSide::Top | PhysicalSide::Bottom => cursor_y,
            PhysicalSide::Left => content_inline_start,
            // A vertical right-to-left line's logical block-start is the
            // physical right edge of its containing block.  Place the line's
            // own block extent immediately before that edge; subsequent lines
            // advance left through the physical line stack.
            // <https://www.w3.org/TR/css-writing-modes-4/#abstract-box>
            PhysicalSide::Right => content_right - line_block_size.max(0.0),
        };
        Self {
            writing_mode: style.writing_mode,
            direction,
            inline_start_offset: context.line_indent,
            inline_start,
            inline_size,
            block_start,
            line_block_size: line_block_size.max(0.0),
            text_box_line_trim: TextBoxLineTrim::default(),
        }
    }

    pub(in crate::layout) fn alignment_offset(
        self,
        content_inline_size: f32,
        align: TextAlign,
    ) -> f32 {
        let free_space = (self.inline_size - content_inline_size).max(0.0);
        match align {
            TextAlign::Left if self.line_left_is_inline_end() => free_space,
            TextAlign::Right if self.line_right_is_inline_end() => free_space,
            TextAlign::Center => free_space / 2.0,
            TextAlign::End => free_space,
            TextAlign::Left
            | TextAlign::Right
            | TextAlign::Start
            | TextAlign::Justify
            | TextAlign::JustifyAll => 0.0,
        }
    }

    pub(in crate::layout) fn hanging_punctuation_offset(
        self,
        hanging_widths: HangingPunctuationWidths,
    ) -> f32 {
        match self.direction {
            Direction::Ltr => -hanging_widths.start,
            Direction::Rtl => hanging_widths.end,
        }
    }

    pub(in crate::layout) fn visual_line_origin(
        self,
        logical_inline_start: f32,
        line_inline_size: f32,
    ) -> f32 {
        self.physical_inline_origin(logical_inline_start, line_inline_size)
    }

    pub(in crate::layout) fn visual_line_item_rect(
        self,
        line_logical_inline_start: f32,
        line_physical_origin: f32,
        visual_inline_start: f32,
        inline_size: f32,
        horizontal_y: f32,
        block_size: f32,
    ) -> PhysicalInlineRect {
        let axes = WritingModeAxes::new(self.writing_mode, self.direction);
        if !axes.swaps_physical_axes() {
            PhysicalInlineRect::new(InlineRect::new(
                InlinePoint::new(line_physical_origin + visual_inline_start, horizontal_y),
                InlineSize::new(inline_size, block_size),
            ))
        } else {
            PhysicalInlineRect::new(InlineRect::new(
                InlinePoint::new(
                    self.block_start,
                    self.physical_inline_origin(
                        line_logical_inline_start + visual_inline_start,
                        inline_size,
                    ),
                ),
                InlineSize::new(block_size, inline_size),
            ))
        }
    }

    pub(in crate::layout) fn position_visual_text_group(
        self,
        group: &mut PreparedInlineTextGroup,
        line_logical_inline_start: f32,
        line_physical_origin: f32,
        visual_inline_start: f32,
    ) {
        if !WritingModeAxes::new(self.writing_mode, self.direction).swaps_physical_axes() {
            group.set_x(line_physical_origin + visual_inline_start);
        } else {
            group.set_x(self.block_start);
            group.set_y(self.vertical_text_inline_origin(
                line_logical_inline_start + visual_inline_start,
                group.width(),
            ));
        }
    }

    pub(in crate::layout) fn physical_inline_origin(
        self,
        logical_inline_start: f32,
        inline_size: f32,
    ) -> f32 {
        match WritingModeAxes::new(self.writing_mode, self.direction)
            .physical_side(LogicalSide::InlineStart)
        {
            // Page-top inline coordinates increase upward, unlike CSS physical
            // Y coordinates. Preserve that local coordinate convention at the
            // paint boundary while obtaining the side from the shared map.
            PhysicalSide::Right | PhysicalSide::Top => {
                self.inline_start - logical_inline_start - inline_size
            }
            PhysicalSide::Left | PhysicalSide::Bottom => self.inline_start + logical_inline_start,
        }
    }

    pub(in crate::layout) fn vertical_text_inline_origin(
        self,
        logical_inline_start: f32,
        inline_size: f32,
    ) -> f32 {
        // `sideways-lr` fixes the line's physical orientation bottom-to-top.
        // `direction` still selects bidi ordering and logical alignment, but
        // must not reverse the PDF run origin a second time after the
        // sideways-left text matrix has established that physical line.
        // <https://drafts.csswg.org/css-writing-modes-4/#valdef-writing-mode-sideways-lr>
        let (placement, placement_logical_inline_start) =
            if self.writing_mode == WritingMode::SidewaysLr && self.direction == Direction::Rtl {
                (
                    Self {
                        direction: Direction::Ltr,
                        inline_start: self.inline_start - self.inline_size,
                        ..self
                    },
                    self.inline_size - logical_inline_start - inline_size,
                )
            } else {
                (self, logical_inline_start)
            };
        let origin = placement.physical_inline_origin(placement_logical_inline_start, inline_size);
        if WritingModeAxes::new(self.writing_mode, placement.direction)
            .physical_side(LogicalSide::InlineStart)
            == PhysicalSide::Top
        {
            origin + inline_size
        } else {
            origin
        }
    }

    /// CSS Writing Modes maps `text-align: left` and `right` through the
    /// line-left and line-right sides, which are not always physical left and
    /// right.  In vertical writing, the two sides are physical top/bottom;
    /// `sideways-lr` reverses their order.
    /// <https://www.w3.org/TR/css-writing-modes-4/#line-directions>
    fn line_left_side(self) -> PhysicalSide {
        FlowAxes::new(self.writing_mode, self.direction).line_left_side()
    }

    fn line_right_side(self) -> PhysicalSide {
        FlowAxes::new(self.writing_mode, self.direction).line_right_side()
    }

    fn line_left_is_inline_end(self) -> bool {
        WritingModeAxes::new(self.writing_mode, self.direction)
            .physical_side(LogicalSide::InlineEnd)
            == self.line_left_side()
    }

    fn line_right_is_inline_end(self) -> bool {
        WritingModeAxes::new(self.writing_mode, self.direction)
            .physical_side(LogicalSide::InlineEnd)
            == self.line_right_side()
    }
}

#[derive(Debug, Clone, Copy)]
/// Shared inputs for laying out one inline paragraph run.
///
/// CSS Inline Layout forms line boxes from consecutive inline-level content
/// inside a block container; these values are the containing line box measure
/// and block style used by the word and mixed inline layout paths:
/// <https://www.w3.org/TR/css-inline-3/#line-layout>.
pub(in crate::layout) struct InlineParagraphContext<'a> {
    pub(in crate::layout) block_style: &'a ComputedStyle,
    /// The resolved layout clamp for this block formatting context. This is
    /// distinct from the immutable computed `line-clamp` declaration.
    pub(in crate::layout) line_clamp: Option<css::InlineLineClamp<'a>>,
    /// Whether an enclosing block-flow traversal has established that source
    /// continues after this inline graph. Preserved breaks and block children
    /// split one clamped stream into several independent inline graphs.
    /// <https://drafts.csswg.org/css-overflow-4/#line-clamp>
    pub(in crate::layout) clamp_continuation: css::ClampContinuation,
    pub(in crate::layout) stylesheets: &'a Stylesheets<'a>,
    /// Whether the first formatted line of the originating block container
    /// is still available to this anonymous inline run.
    ///
    /// Anonymous blocks produced by mixed inline/block content do not restart
    /// their parent's first formatted line for `text-indent`, even though a
    /// real descendant block establishes its own formatting context.
    pub(in crate::layout) initial_first_formatted_line: bool,
    pub(in crate::layout) available_width: f32,
    pub(in crate::layout) padding_left: f32,
    pub(in crate::layout) hanging_indent: f32,
    pub(in crate::layout) hanging_punctuation_reserve: f32,
}

#[derive(Debug, Clone, Copy)]
pub(in crate::layout) struct InlinePaintContext<'a> {
    pub(in crate::layout) block_style: &'a ComputedStyle,
    pub(in crate::layout) direction: Direction,
    pub(in crate::layout) available_width: f32,
    pub(in crate::layout) padding_left: f32,
    pub(in crate::layout) line_indent: f32,
    pub(in crate::layout) text_align: TextAlign,
    pub(in crate::layout) is_first_line: bool,
    /// Used logical block advance selected for this line record. This can
    /// differ from the glyph/baseline metrics used for vertical alignment.
    pub(in crate::layout) line_block_size: f32,
}

#[derive(Debug, Clone)]
pub(in crate::layout) struct InlineAtomData {
    pub(in crate::layout) content: InlineAtomContent,
    /// The one-way used style that selected this atom's footprint and paint.
    /// A source style, when the atom needs to reconstruct descendants, is
    /// retained by the owning formatting-box/replay context instead.
    pub(in crate::layout) style: Rc<css::ZoomedLayoutStyle>,
    pub(in crate::layout) outside_marker: Option<ListMarker>,
    pub(in crate::layout) content_inline_offset: f32,
    pub(in crate::layout) content_inline_paint_width: Option<f32>,
    pub(in crate::layout) escaped_positioned_layers: Option<Rc<[PositionedPaintLayer]>>,
    pub(in crate::layout) link_target: Option<Rc<str>>,
    pub(in crate::layout) alt_text: Option<Rc<str>>,
    /// The containing inline scope. Atomic boxes use their parent scope so an
    /// atom's own style cannot own tracking on its outside boundaries.
    pub(in crate::layout) tracking_scope: Option<Rc<InlineTrackingScope>>,
    /// Fragment-local foreground used to resolve deferred `currentcolor` at a
    /// selected inline box edge. The lexical style remains stable for source
    /// edge-ownership matching.
    pub(in crate::layout) current_color_override: Option<CssColor>,
}

#[derive(Debug, Clone)]
pub(in crate::layout) struct InlineAtom {
    pub(in crate::layout) data: Rc<InlineAtomData>,
    pub(in crate::layout) size: InlineSize,
    pub(in crate::layout) baseline: InlineAtomBaseline,
    pub(in crate::layout) baseline_placement: InlineBaselinePlacement,
    pub(in crate::layout) visual_offset: InlineVisualOffset,
    /// Placement resolved only after this atom has been selected into a parent
    /// inline line. Source ruby geometry remains independent of visual
    /// neighbors and bidi ordering.
    pub(in crate::layout) ruby_placement: Option<ruby::ResolvedRubyPlacement>,
}

/// Fully resolved parent-facing geometry for one atomic inline.
///
/// The physical margin box, exported baseline set, and eventual inline
/// baseline placement are selected by the formatting context that produced
/// the atom. [`InlineAtomContent`] describes semantics and paint only; it is
/// deliberately absent from this record so changing paint representation
/// cannot silently change line geometry.
/// <https://drafts.csswg.org/css-inline-3/#atomic-inline>
#[derive(Debug, Clone, Copy)]
pub(in crate::layout) struct ResolvedInlineAtomGeometry {
    pub(in crate::layout) physical_margin_box_size: MarginBoxSize,
    pub(in crate::layout) baseline: InlineAtomBaseline,
    pub(in crate::layout) baseline_placement: InlineBaselinePlacement,
}

impl ResolvedInlineAtomGeometry {
    pub(in crate::layout) fn exported_from_logical_block_start(
        style: &css::ZoomedLayoutStyle,
        physical_margin_box_size: MarginBoxSize,
        border_box_block_size: LayoutLength,
        baseline_offset: LayoutLength,
        baseline_placement: InlineBaselinePlacement,
    ) -> Self {
        Self {
            physical_margin_box_size,
            baseline: InlineAtomBaseline::Physical {
                source: InlineAtomBaselineSource::BorderBox,
                baselines: crate::layout::baseline::PhysicalBaselineSets::default()
                    .with_first_from_logical_block_start(
                        block_start_side(style.writing_mode),
                        border_box_block_size,
                        baseline_offset,
                        BaselineMetric::Alphabetic,
                    ),
                missing_axis_synthesis: AtomicInlineBaselineSynthesisSource::MarginBox,
            },
            baseline_placement,
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub(in crate::layout) fn from_resolved_boxes(
        style: &css::ZoomedLayoutStyle,
        content: ContentBoxSize,
        horizontal_non_content: NonContentLength,
        vertical_non_content: NonContentLength,
        horizontal_margins: LayoutLength,
        vertical_margins: LayoutLength,
        baseline_offset: LayoutLength,
        baseline_placement: InlineBaselinePlacement,
    ) -> Self {
        let width = content_box_to_margin_box_length(
            content_box_pt(content.width),
            horizontal_non_content,
            horizontal_margins,
        );
        let height = content_box_to_margin_box_length(
            content_box_pt(content.height),
            vertical_non_content,
            vertical_margins,
        );
        let border_box_block_size = if style.writing_mode.has_vertical_lines() {
            content_box_to_border_box_length(content_box_pt(content.width), horizontal_non_content)
        } else {
            content_box_to_border_box_length(content_box_pt(content.height), vertical_non_content)
        };
        Self::exported_from_logical_block_start(
            style,
            margin_box_size_pt(width.points(), height.points()),
            layout_pt(border_box_block_size.points()),
            baseline_offset,
            baseline_placement,
        )
    }

    fn inline_layout_size(self) -> InlineSize {
        InlineSize::new(
            self.physical_margin_box_size.width,
            self.physical_margin_box_size.height,
        )
    }
}

/// Input to the inline-atom used-value boundary.
///
/// Inline collection is the one exceptional producer that may receive either
/// a fresh computed source or an already normalized layout style. In both
/// cases the atom itself stores only the latter, so replay cannot turn a
/// measured footprint back into a cascade parent.
pub(in crate::layout) trait IntoInlineAtomUsedStyle {
    fn into_inline_atom_used_style(self) -> css::ZoomedLayoutStyle;
}

impl IntoInlineAtomUsedStyle for css::ZoomedLayoutStyle {
    fn into_inline_atom_used_style(self) -> css::ZoomedLayoutStyle {
        self
    }
}

impl IntoInlineAtomUsedStyle for ComputedStyle {
    fn into_inline_atom_used_style(self) -> css::ZoomedLayoutStyle {
        css::LayoutStyle::from_computed(&self).into_zoomed()
    }
}

/// The baseline an atomic inline contributes to its containing line.
///
/// Atomic inline boxes carry physical margin-box dimensions for paint. Most
/// exported baselines are measured from an alignment-source box's logical
/// block-start edge. CSS 2.2 makes an `inline-table` use its table box rather
/// than its wrapper as that source. The source remains distinct from the
/// margin-box coordinate used for line sizing and from the paint-placement
/// coordinate used to replay a captured atom:
/// <https://www.w3.org/TR/CSS22/tables.html#table-display>
/// <https://drafts.csswg.org/css-inline-3/#inline-block-baseline>.
#[derive(Debug, Clone, Copy)]
pub(in crate::layout) enum InlineAtomBaseline {
    /// Tate-chu-yoko does not export a child alphabetic baseline. Its one-em
    /// square is aligned to the parent text's central baseline after parent
    /// metrics are known.
    /// <https://drafts.csswg.org/css-writing-modes-4/#text-combine-layout>
    TextCombineParentCentral,
    Physical {
        source: InlineAtomBaselineSource,
        baselines: crate::layout::baseline::PhysicalBaselineSets,
        missing_axis_synthesis: AtomicInlineBaselineSynthesisSource,
    },
    /// An atom without a content-derived baseline defers synthesis until its
    /// enclosing alignment context selects a baseline metric.
    ///
    /// CSS Inline synthesizes atomic-inline baselines from margin edges,
    /// while Flexbox synthesizes a flex item's missing alignment baseline
    /// from border edges:
    /// <https://drafts.csswg.org/css-inline-3/#synthesize-baselines>
    /// <https://drafts.csswg.org/css-flexbox-1/#align-items-property>.
    Synthesized {
        source: AtomicInlineBaselineSynthesisSource,
    },
}

/// The box edges used when an atomic inline baseline must be synthesized.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::layout) enum AtomicInlineBaselineSynthesisSource {
    MarginBox,
    BorderBox,
}

/// The box from which an atomic inline exports its baseline.
///
/// This identifies only the baseline source. Atomic inline line layout still
/// uses the margin box for both variants.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::layout) enum InlineAtomBaselineSource {
    BorderBox,
    TableBox,
}

/// The two baseline coordinates consumed at the atomic-inline boundary.
///
/// Line metrics must use `margin_box`, while captured-fragment replay uses
/// `paint_placement`. Keeping these together prevents a table-box baseline
/// from accidentally replacing the margin-box coordinate that encloses the
/// inline-table wrapper's margins.
#[derive(Debug, Clone, Copy)]
pub(in crate::layout) struct AtomicInlineBaselineCoordinates {
    pub(in crate::layout) margin_box: AtomicInlineMarginBoxBaselineOffset,
    pub(in crate::layout) paint_placement: AtomicInlinePaintPlacementBaselineOffset,
}

impl InlineAtom {
    #[allow(clippy::arc_with_non_send_sync, clippy::too_many_arguments)]
    pub(in crate::layout) fn new(
        content: InlineAtomContent,
        style: impl IntoInlineAtomUsedStyle,
        escaped_positioned_layers: Option<Box<[PositionedPaintLayer]>>,
        size: InlineSize,
        baseline_offset: f32,
        baseline_shift: f32,
        link_target: Option<String>,
        alt_text: Option<String>,
    ) -> Self {
        let style = style.into_inline_atom_used_style();
        let margin_box_block_size = if style.writing_mode.has_vertical_lines() {
            size.width
        } else {
            size.height
        };
        let block_margins = if style.writing_mode.has_vertical_lines() {
            style.margin.left + style.margin.right
        } else {
            style.margin.top + style.margin.bottom
        };
        let geometry = ResolvedInlineAtomGeometry::exported_from_logical_block_start(
            &style,
            margin_box_size_pt(size.width, size.height),
            layout_pt((margin_box_block_size - block_margins).max(0.0)),
            layout_pt(baseline_offset),
            InlineBaselinePlacement::from_inherited_glyph_displacement(
                glyph_baseline_displacement_pt(baseline_shift),
            ),
        );
        Self::from_resolved_geometry(
            content,
            style,
            escaped_positioned_layers,
            geometry,
            link_target,
            alt_text,
        )
    }

    #[allow(clippy::arc_with_non_send_sync, clippy::too_many_arguments)]
    pub(in crate::layout) fn from_resolved_geometry(
        content: InlineAtomContent,
        style: impl IntoInlineAtomUsedStyle,
        escaped_positioned_layers: Option<Box<[PositionedPaintLayer]>>,
        geometry: ResolvedInlineAtomGeometry,
        link_target: Option<String>,
        alt_text: Option<String>,
    ) -> Self {
        let style = style.into_inline_atom_used_style();
        Self {
            data: Rc::new(InlineAtomData {
                content,
                style: Rc::new(style),
                outside_marker: None,
                content_inline_offset: 0.0,
                content_inline_paint_width: None,
                escaped_positioned_layers: escaped_positioned_layers.map(Rc::from),
                link_target: link_target.map(Rc::from),
                alt_text: alt_text.map(Rc::from),
                tracking_scope: None,
                current_color_override: None,
            }),
            size: geometry.inline_layout_size(),
            baseline: geometry.baseline,
            baseline_placement: geometry.baseline_placement,
            visual_offset: InlineVisualOffset::zero(),
            ruby_placement: None,
        }
    }

    pub(in crate::layout) fn with_visual_offset(
        mut self,
        visual_offset: InlineVisualOffset,
    ) -> Self {
        self.visual_offset = visual_offset;
        self
    }

    pub(in crate::layout) fn baseline_shift(&self) -> f32 {
        self.baseline_placement.glyph_displacement().points()
    }

    pub(in crate::layout) fn add_baseline_placement(&mut self, placement: InlineBaselinePlacement) {
        self.baseline_placement = self.baseline_placement.with_added(placement);
    }

    pub(in crate::layout) fn with_ruby_placement(
        mut self,
        placement: ruby::ResolvedRubyPlacement,
    ) -> Self {
        self.ruby_placement = Some(placement);
        self
    }

    pub(in crate::layout) fn ruby_placement(&self) -> Option<&ruby::ResolvedRubyPlacement> {
        self.ruby_placement.as_ref()
    }

    pub(in crate::layout) fn with_tracking_scope(mut self, scope: Rc<InlineTrackingScope>) -> Self {
        Rc::make_mut(&mut self.data).tracking_scope = Some(scope);
        self
    }

    /// Update a CSS Text autospace marker after its lexical owner or
    /// first-line style has been resolved. The atom remains non-painting and
    /// keeps a zero block extent; only its logical inline advance and owner
    /// style participate in line layout.
    /// <https://drafts.csswg.org/css-text-4/#text-autospace-property>
    pub(in crate::layout) fn set_text_autospace_advance(
        &mut self,
        style: &ComputedStyle,
        advance: LayoutLength,
    ) {
        let data = Rc::make_mut(&mut self.data);
        let InlineAtomContent::InlineEdge(InlineEdgeRole::TextAutospace(spacing)) =
            &mut data.content
        else {
            unreachable!("only text-autospace atoms have a style-dependent advance");
        };
        *spacing = InlineTextBoundarySpacing::new(advance);
        data.style = Rc::new(style.clone().into_inline_atom_used_style());
        self.size = InlineSize::new(advance.points(), 0.0);
    }

    /// Turn a transparent inline-box edge into a metrics-only line strut.
    /// The atom retains its zero advance and never paints.
    pub(in crate::layout) fn mark_metrics_only_strut(&mut self) {
        let data = Rc::make_mut(&mut self.data);
        let InlineAtomContent::InlineEdge(InlineEdgeRole::BoxEdge(edge)) = &data.content else {
            unreachable!("only a transparent inline-box edge can become a metrics strut");
        };
        debug_assert_eq!(edge.advance, 0.0);
        debug_assert_eq!(edge.paint_extent, 0.0);
        debug_assert!(!edge.is_positioning_marker());
        data.content = InlineAtomContent::InlineEdge(InlineEdgeRole::MetricsOnlyStrut);
    }

    pub(in crate::layout) fn tracking_scope(&self) -> Option<&Rc<InlineTrackingScope>> {
        self.data.tracking_scope.as_ref()
    }

    pub(in crate::layout) fn line_relative_scope(&self) -> Option<&InlineTrackingScope> {
        self.tracking_scope()
            .and_then(|scope| scope.nearest_line_relative_scope())
    }

    pub(in crate::layout) fn line_relative_alignment(
        &self,
    ) -> Option<InlineScopeLineRelativeAlignment> {
        // An inline edge represents its owning regular inline box, but its
        // tracking scope is deliberately still the parent while the start
        // edge is processed. Do not let the edge's style turn that
        // parent-side marker into an independently line-relative atom.
        // Non-edge atoms are atomic inlines, for which `vertical-align`
        // applies directly to the atom.
        (!self.content().is_inline_edge())
            .then(|| {
                InlineScopeLineRelativeAlignment::from_baseline_shift(
                    &self.style().vertical_align.baseline_shift,
                )
            })
            .flatten()
            .or_else(|| {
                self.line_relative_scope()
                    .and_then(InlineTrackingScope::line_relative_alignment)
            })
    }

    pub(in crate::layout) fn with_outside_marker(mut self, marker: Option<ListMarker>) -> Self {
        Rc::make_mut(&mut self.data).outside_marker = marker;
        self
    }

    pub(in crate::layout) fn with_content_inline_offset(mut self, offset: f32) -> Self {
        Rc::make_mut(&mut self.data).content_inline_offset = offset;
        self
    }

    pub(in crate::layout) fn with_content_inline_paint_width(mut self, width: f32) -> Self {
        Rc::make_mut(&mut self.data).content_inline_paint_width = Some(width);
        self
    }

    pub(in crate::layout) fn content(&self) -> &InlineAtomContent {
        &self.data.content
    }

    pub(in crate::layout) fn style(&self) -> &ComputedStyle {
        &self.data.style
    }

    pub(in crate::layout) fn set_current_color_override(&mut self, color: CssColor) {
        Rc::make_mut(&mut self.data).current_color_override = Some(color);
    }

    pub(in crate::layout) fn current_color_override(&self) -> Option<CssColor> {
        self.data.current_color_override
    }

    /// Resolve this atom's baseline from its own alignment-source block start.
    ///
    /// The caller supplies the border-box block size because atoms retain
    /// physical dimensions and their containing inline formatting context
    /// owns the logical-axis projection.
    pub(in crate::layout) fn baseline_offset_from_alignment_source_block_start(
        &self,
        border_box_block_size: f32,
        containing_style: &ComputedStyle,
    ) -> AtomicInlineBaselineSourceOffset {
        match self.baseline {
            InlineAtomBaseline::TextCombineParentCentral => {
                unreachable!("text-combine baseline requires parent central-baseline geometry")
            }
            InlineAtomBaseline::Physical { baselines, .. } => baselines
                .first_from_logical_block_start_with_metric(
                    block_start_side(containing_style.writing_mode),
                    layout_pt(border_box_block_size),
                )
                .map(|(baseline, _)| baseline)
                .map(LayoutLength::points)
                .map(atomic_inline_baseline_source_pt)
                // The enclosing resolver observes the missing axis and uses
                // the atom's explicit synthesis policy. This value is not
                // consumed by margin-box synthesis, but retaining a valid
                // coordinate keeps the shared coordinate adapter total.
                .unwrap_or_else(|| atomic_inline_baseline_source_pt(border_box_block_size)),
            InlineAtomBaseline::Synthesized { .. } => {
                atomic_inline_baseline_source_pt(border_box_block_size)
            }
        }
    }

    /// Return the source metric of this atom's content-derived baseline in
    /// the containing inline axis. `None` means that CSS must synthesize a
    /// baseline from the selected edge model in that alignment context.
    pub(in crate::layout) fn content_derived_baseline_metric(
        &self,
        containing_style: &ComputedStyle,
    ) -> Option<BaselineMetric> {
        match self.baseline {
            InlineAtomBaseline::TextCombineParentCentral => Some(BaselineMetric::Central),
            InlineAtomBaseline::Physical { baselines, .. } => baselines
                .first_from_logical_block_start_with_metric(
                    block_start_side(containing_style.writing_mode),
                    layout_pt(inline_atom_logical_border_block_size(
                        self,
                        containing_style,
                    )),
                )
                .map(|(_, metric)| metric),
            InlineAtomBaseline::Synthesized { .. } => None,
        }
    }

    /// Resolve the distinct line-metric and captured-fragment baseline
    /// coordinates for this atom.
    ///
    /// Every atomic inline contributes a baseline measured from its logical
    /// margin-box start to line sizing, including an `inline-table` whose
    /// exported source is its table box. Painting replays each atom from its
    /// own alignment-source box, so the margin-box coordinate is converted
    /// back to that border- or table-box coordinate instead of moving sibling
    /// line participants.
    /// <https://drafts.csswg.org/css-inline-3/#line-boxes>
    /// <https://www.w3.org/TR/CSS22/tables.html#table-display>
    pub(in crate::layout) fn resolve_baseline_coordinates(
        &self,
        border_box_block_size: f32,
        margin_box_block_size: f32,
        block_start_margin: f32,
        block_end_margin: f32,
        containing_style: &ComputedStyle,
    ) -> AtomicInlineBaselineCoordinates {
        let exported_source_box = match self.baseline {
            InlineAtomBaseline::Physical { source, .. } => Some(source),
            InlineAtomBaseline::TextCombineParentCentral
            | InlineAtomBaseline::Synthesized { .. } => None,
        };
        let synthesized_source = match self.baseline {
            InlineAtomBaseline::TextCombineParentCentral => {
                unreachable!("text-combine coordinates are resolved from parent text metrics")
            }
            InlineAtomBaseline::Synthesized { source } => Some(source),
            InlineAtomBaseline::Physical {
                missing_axis_synthesis,
                ..
            } if self
                .content_derived_baseline_metric(containing_style)
                .is_none() =>
            {
                Some(missing_axis_synthesis)
            }
            InlineAtomBaseline::Physical { .. } => None,
        };
        let source = self.baseline_offset_from_alignment_source_block_start(
            border_box_block_size,
            containing_style,
        );
        let (margin_box, paint_placement) = match synthesized_source {
            Some(AtomicInlineBaselineSynthesisSource::MarginBox) => (
                atomic_inline_margin_box_baseline_pt(margin_box_block_size),
                // The synthesized baseline is at the margin-box line-under
                // edge. Replaying the border box crosses that atom's own
                // block-end margin; the block-start margin remains in the
                // margin-box alignment coordinate above.
                atomic_inline_paint_placement_baseline_pt(border_box_block_size + block_end_margin),
            ),
            Some(AtomicInlineBaselineSynthesisSource::BorderBox) => (
                atomic_inline_margin_box_baseline_pt(block_start_margin + source.points()),
                atomic_inline_paint_placement_baseline_pt(source.points()),
            ),
            None => {
                debug_assert!(exported_source_box.is_some());
                (
                    atomic_inline_margin_box_baseline_pt(block_start_margin + source.points()),
                    atomic_inline_paint_placement_baseline_pt(source.points()),
                )
            }
        };
        AtomicInlineBaselineCoordinates {
            margin_box,
            paint_placement,
        }
    }

    /// Mark an exported baseline as originating from an inline-table's table
    /// box rather than its wrapper box.
    ///
    /// CSS 2.2 requires `vertical-align` on `inline-table` to use the table
    /// box. Its wrapper margins remain part of the atomic inline's outer
    /// geometry: they rebase the line-metric coordinate, but not the
    /// table-box coordinate used for captured-fragment replay.
    /// <https://www.w3.org/TR/CSS22/tables.html#table-display>
    pub(in crate::layout) fn with_exported_table_box_baseline(mut self) -> Self {
        let InlineAtomBaseline::Physical {
            baselines,
            missing_axis_synthesis,
            ..
        } = self.baseline
        else {
            unreachable!("only an exported baseline can originate from a table box");
        };
        self.baseline = InlineAtomBaseline::Physical {
            source: InlineAtomBaselineSource::TableBox,
            baselines,
            missing_axis_synthesis,
        };
        self
    }

    /// Store a finalized Flex baseline record without prematurely choosing a
    /// physical axis. The enclosing inline formatting context owns that
    /// writing-mode projection.
    pub(in crate::layout) fn with_flex_exported_baselines(
        mut self,
        baselines: crate::layout::baseline::PhysicalBaselineSets,
    ) -> Self {
        self.baseline = InlineAtomBaseline::Physical {
            source: InlineAtomBaselineSource::BorderBox,
            baselines,
            missing_axis_synthesis: AtomicInlineBaselineSynthesisSource::MarginBox,
        };
        self
    }

    pub(in crate::layout) fn with_synthesized_border_box_block_end_baseline(mut self) -> Self {
        self.baseline = InlineAtomBaseline::Synthesized {
            source: AtomicInlineBaselineSynthesisSource::BorderBox,
        };
        self
    }

    /// Mark an atomic inline that has no content-derived baseline to
    /// synthesize its alphabetic baseline from its line-under margin edge.
    ///
    /// CSS Inline Layout Level 3 Appendix A.3 makes this the fallback for an
    /// atomic inline whose contents cannot supply a baseline.  Keeping it
    /// distinct from the legacy border-box synthesis is important: the former
    /// changes only this atom's placement for authored block-axis margins,
    /// whereas the latter remains for internal non-content inline artifacts.
    /// <https://drafts.csswg.org/css-inline-3/#synthesize-baselines>
    pub(in crate::layout) fn with_synthesized_margin_box_block_end_baseline(mut self) -> Self {
        self.baseline = InlineAtomBaseline::Synthesized {
            source: AtomicInlineBaselineSynthesisSource::MarginBox,
        };
        self
    }

    /// Select the parent-dependent central baseline used by a tate-chu-yoko
    /// square. This is explicit at construction so paint content cannot
    /// determine line geometry implicitly.
    pub(in crate::layout) fn with_text_combine_parent_central_baseline(mut self) -> Self {
        self.baseline = InlineAtomBaseline::TextCombineParentCentral;
        self
    }

    pub(in crate::layout) fn outside_marker(&self) -> Option<&ListMarker> {
        self.data.outside_marker.as_ref()
    }

    pub(in crate::layout) fn content_inline_offset(&self) -> f32 {
        self.data.content_inline_offset
    }

    pub(in crate::layout) fn content_inline_paint_width(&self) -> Option<f32> {
        self.data.content_inline_paint_width
    }

    pub(in crate::layout) fn escaped_positioned_layers(&self) -> Option<&[PositionedPaintLayer]> {
        self.data.escaped_positioned_layers.as_deref()
    }

    /// Keep a positioned descendant with the inline source atom that owns
    /// its containing block.  The atom replays these layers only if its
    /// source range survives line selection, which makes a discarded clamp
    /// continuation discard the descendant as well.
    pub(in crate::layout) fn append_escaped_positioned_layers(
        &mut self,
        layers: Vec<PositionedPaintLayer>,
    ) {
        if layers.is_empty() {
            return;
        }
        let data = Rc::make_mut(&mut self.data);
        let mut retained = data
            .escaped_positioned_layers
            .as_deref()
            .unwrap_or_default()
            .to_vec();
        retained.extend(layers);
        data.escaped_positioned_layers = Some(Rc::from(retained));
    }

    pub(in crate::layout) fn link_target(&self) -> Option<&str> {
        self.data.link_target.as_deref()
    }

    pub(in crate::layout) fn alt_text(&self) -> Option<&str> {
        self.data.alt_text.as_deref()
    }
}

/// Stable source identity for an inline float.
///
/// Inline-source floats normally originate from a DOM element, but CSS 2
/// permits `float` on `::first-letter`.  That pseudo has no element identity,
/// so its source-order marker is keyed by its first-letter group instead.
/// <https://www.w3.org/TR/CSS21/selector.html#first-letter>
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(in crate::layout) enum InlineFloatId {
    Element(ElementId),
    FirstLetter(FirstLetterPseudoGroupId),
}

/// The content owned by an inline-source float.
#[derive(Debug, Clone)]
#[allow(clippy::large_enum_variant)]
pub(in crate::layout) enum InlineFloatContents {
    Element {
        element: Element,
        signature: ElementSignature,
        generated_content: bool,
        generated_pseudo_source: Option<box_tree::CounterEventSource>,
    },
    /// The complete stream-selected `::first-letter` text.  This remains a
    /// text payload instead of impersonating a DOM element so punctuation and
    /// text split across transparent inline boundaries keep their ownership.
    FirstLetterText {
        fragments: Rc<[InlineFragment]>,
        group_id: FirstLetterPseudoGroupId,
    },
}

#[derive(Debug, Clone)]
pub(in crate::layout) struct InlineFloatData {
    pub(in crate::layout) contents: InlineFloatContents,
    pub(in crate::layout) style: ComputedStyle,
    pub(in crate::layout) positioning_containing_block:
        Option<InlinePositioningContainingBlockSource>,
}

/// Source inline box that establishes a containing block for abspos descendants.
///
/// CSS 2.2 defines an absolutely positioned descendant's containing block from
/// the padding boxes of its nearest positioned inline ancestor:
/// <https://www.w3.org/TR/CSS22/visudet.html#containing-block-details>.
#[derive(Debug, Clone, PartialEq)]
pub(in crate::layout) struct InlinePositioningContainingBlockSource {
    pub(in crate::layout) id: InlinePositioningContainingBlockId,
    /// Used padding-box geometry of the positioned inline ancestor.
    pub(in crate::layout) style: Box<css::ZoomedLayoutStyle>,
}

impl InlinePositioningContainingBlockSource {
    /// Borrow this source while collecting descendants inside its lexical
    /// inline scope. Deferred descendants must call [`Self::as_borrowed`]
    /// and promote that view back to an owned source before the scope ends.
    pub(in crate::layout) fn as_borrowed(
        &self,
    ) -> BorrowedInlinePositioningContainingBlockSource<'_> {
        BorrowedInlinePositioningContainingBlockSource {
            id: self.id,
            style: self.style.as_ref(),
        }
    }
}

/// A positioned inline containing-block source borrowed from its active
/// lexical inline scope.
///
/// This keeps the complete resolved style out of recursive collector frames.
/// A deferred descendant promotes this view to
/// [`InlinePositioningContainingBlockSource`] before its parent scope returns.
#[derive(Debug, Clone, Copy)]
pub(in crate::layout) struct BorrowedInlinePositioningContainingBlockSource<'a> {
    pub(in crate::layout) id: InlinePositioningContainingBlockId,
    pub(in crate::layout) style: &'a css::ZoomedLayoutStyle,
}

impl BorrowedInlinePositioningContainingBlockSource<'_> {
    pub(in crate::layout) fn into_owned(self) -> InlinePositioningContainingBlockSource {
        InlinePositioningContainingBlockSource {
            id: self.id,
            style: Box::new(self.style.clone()),
        }
    }
}

/// Stable identity for one collected inline containing-block source.
///
/// This distinguishes nested positioned inline ancestors that share identical
/// computed styles, matching the nearest-ancestor rule from CSS 2.2:
/// <https://www.w3.org/TR/CSS22/visudet.html#containing-block-details>.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(in crate::layout) struct InlinePositioningContainingBlockId(pub(in crate::layout) usize);

#[derive(Debug, Clone)]
pub(in crate::layout) struct InlineFloat {
    pub(in crate::layout) data: Rc<InlineFloatData>,
}

impl InlineFloat {
    pub(in crate::layout) fn new(
        element: Element,
        signature: ElementSignature,
        style: ComputedStyle,
        generated_content: bool,
        generated_pseudo_source: Option<box_tree::CounterEventSource>,
        positioning_containing_block: Option<InlinePositioningContainingBlockSource>,
    ) -> Self {
        Self {
            data: Rc::new(InlineFloatData {
                contents: InlineFloatContents::Element {
                    element,
                    signature,
                    generated_content,
                    generated_pseudo_source,
                },
                style,

                positioning_containing_block,
            }),
        }
    }

    pub(in crate::layout) fn first_letter(
        fragments: Vec<InlineFragment>,
        group_id: FirstLetterPseudoGroupId,
        style: ComputedStyle,
    ) -> Self {
        Self {
            data: Rc::new(InlineFloatData {
                contents: InlineFloatContents::FirstLetterText {
                    fragments: Rc::from(fragments.into_boxed_slice()),
                    group_id,
                },
                style,

                positioning_containing_block: None,
            }),
        }
    }

    pub(in crate::layout) fn id(&self) -> InlineFloatId {
        match &self.data.contents {
            InlineFloatContents::Element { element, .. } => InlineFloatId::Element(element.id),
            InlineFloatContents::FirstLetterText { group_id, .. } => {
                InlineFloatId::FirstLetter(*group_id)
            }
        }
    }

    pub(in crate::layout) fn element(&self) -> Option<&Element> {
        match &self.data.contents {
            InlineFloatContents::Element { element, .. } => Some(element),
            InlineFloatContents::FirstLetterText { .. } => None,
        }
    }

    pub(in crate::layout) fn signature(&self) -> Option<&ElementSignature> {
        match &self.data.contents {
            InlineFloatContents::Element { signature, .. } => Some(signature),
            InlineFloatContents::FirstLetterText { .. } => None,
        }
    }

    pub(in crate::layout) fn style(&self) -> &ComputedStyle {
        &self.data.style
    }

    pub(in crate::layout) fn is_generated_content(&self) -> bool {
        matches!(
            self.data.contents,
            InlineFloatContents::Element {
                generated_content: true,
                ..
            }
        )
    }

    /// Returns the counter-event source when this float is a tree-abiding
    /// generated pseudo-element rather than an originating principal box.
    ///
    /// Generated boxes retain their own source position and counter scope
    /// during float replay. <https://www.w3.org/TR/css-pseudo-4/#generated-content>
    pub(in crate::layout) fn generated_pseudo_source(
        &self,
    ) -> Option<box_tree::CounterEventSource> {
        match &self.data.contents {
            InlineFloatContents::Element {
                generated_pseudo_source,
                ..
            } => *generated_pseudo_source,
            InlineFloatContents::FirstLetterText { .. } => None,
        }
    }

    pub(in crate::layout) fn first_letter_fragments(&self) -> Option<&[InlineFragment]> {
        match &self.data.contents {
            InlineFloatContents::FirstLetterText { fragments, .. } => Some(fragments),
            InlineFloatContents::Element { .. } => None,
        }
    }

    pub(in crate::layout) fn positioning_containing_block(
        &self,
    ) -> Option<&InlinePositioningContainingBlockSource> {
        self.data.positioning_containing_block.as_ref()
    }
}

/// Stable identity for a static-position source in an inline item stream.
///
/// Deferred positioned replay must select the exact normal-flow source that
/// produced its line geometry. An item index is not stable across inline-box
/// splitting, bidi reordering, or trimming. Element identity alone is also
/// insufficient because a principal box and its tree-abiding generated
/// pseudos share the same originating element.
///
/// The block identity is retained for block-in-inline compatibility paths
/// that do not yet have an element-level source marker.
/// <https://drafts.csswg.org/css-position-3/#static-position>
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(in crate::layout) enum InlineStaticPositionSourceId {
    Element {
        element: crate::dom::ElementId,
        source: InlineStaticPositionBoxSource,
    },
    Block,
}

/// The element-backed box role that owns an inline static-position source.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(in crate::layout) enum InlineStaticPositionBoxSource {
    Principal,
    GeneratedPseudo(box_tree::GeneratedPseudoKind),
}

/// CSS Text participation of a temporary static-position hypothetical box.
/// Ordinary inline out-of-flow sources remain absent from text adjacency;
/// genuinely atomic inline sources keep the atomic boundary policy while
/// their measured margin box participates in fitting.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(in crate::layout) enum StaticPositionHypotheticalBoundary {
    Transparent,
    Atomic,
}

impl InlineStaticPositionSourceId {
    pub(in crate::layout) fn for_element(element: &Element) -> Self {
        Self::Element {
            element: element.id,
            source: InlineStaticPositionBoxSource::Principal,
        }
    }

    pub(in crate::layout) fn for_box_source(
        element: &Element,
        source: &box_tree::BoxSource<'_>,
    ) -> Self {
        let source = match source {
            box_tree::BoxSource::Principal => InlineStaticPositionBoxSource::Principal,
            box_tree::BoxSource::GeneratedPseudo(pseudo) => {
                InlineStaticPositionBoxSource::GeneratedPseudo(pseudo.kind)
            }
        };
        Self::Element {
            element: element.id,
            source,
        }
    }

    pub(in crate::layout) fn for_generated_pseudo(
        element: &Element,
        kind: box_tree::GeneratedPseudoKind,
    ) -> Self {
        Self::Element {
            element: element.id,
            source: InlineStaticPositionBoxSource::GeneratedPseudo(kind),
        }
    }

    /// Return the generated-box role only for its originating element.
    /// Positioned descendants laid out while replaying a pseudo are principal
    /// boxes of their own elements and must not inherit the pseudo's identity.
    pub(in crate::layout) fn box_source_for_element(
        self,
        candidate: crate::dom::ElementId,
    ) -> Option<InlineStaticPositionBoxSource> {
        match self {
            Self::Element { element, source } if element == candidate => Some(source),
            Self::Element { .. } | Self::Block => None,
        }
    }
}

#[derive(Debug, Clone)]
pub(in crate::layout) enum InlineAtomContent {
    Canvas,
    /// An inline iframe keeps its embedding element identity so the final
    /// paint pass can composite the matching isolated browsing context.
    Iframe(crate::dom::ElementId),
    Image(DecodedPngImage),
    /// A generated `content` gradient preserves its CSS image identity until
    /// paint.  The raster is used only when native PDF shading cannot express
    /// the gradient exactly.
    Gradient {
        image: BackgroundImage,
        fallback: DecodedPngImage,
    },
    Svg {
        asset: Option<SharedSvgAsset>,
    },
    /// Non-painting hypothetical normal-flow box for an out-of-flow source.
    ///
    /// CSS Positioned Layout resolves auto insets from the static-position
    /// rectangle associated with the source's inline boundary. Inline layout
    /// carries this atom only through temporary line selection and preparation.
    /// Non-atomic sources use zero inline advance so a soft wrap cannot migrate
    /// the anchor; atomic sources retain their indivisible hypothetical size.
    /// The atom must never paint:
    /// <https://www.w3.org/TR/CSS22/visudet.html#abs-non-replaced-height> and
    /// <https://www.w3.org/TR/css-position-3/#staticpos-rect>.
    StaticPositionHypothetical {
        source: InlineStaticPositionSourceId,
        boundary: StaticPositionHypotheticalBoundary,
    },
    InlineBox {
        sequence: inline_layout::InlineLineSequence,
    },
    /// A coupled ruby base level and its first-level interlinear annotation.
    ///
    /// The parent inline graph receives one column-group advance, while paint
    /// replays the base and annotation sequences at their separate logical
    /// block positions. This is deliberately distinct from `InlineBox`: ruby
    /// annotations must not become parent-line text or justification
    /// opportunities.
    /// <https://drafts.csswg.org/css-ruby-1/#ruby-layout>
    Ruby {
        /// Source text on the base side of this paired ruby column. The
        /// inline opportunity graph uses it only to apply UAX #14 at the
        /// boundary between adjacent ruby bases; annotation sidecars remain
        /// excluded from the parent text stream.
        base_text: String,
        base: RubyInlineLevel,
        annotations: Vec<RubyInlineLevel>,
        /// Used placement of each annotation level. This remains parallel to
        /// `annotations`, making a missing side a type-visible construction
        /// error rather than an implicit default at paint time.
        annotation_sides: Vec<css::RubyAnnotationSide>,
        base_block_size: f32,
        /// Shared logical block extent of each annotation level in the
        /// normalized ruby column group. Every column uses the same level
        /// stack so empty anonymous counterparts cannot move a sibling
        /// annotation independently.
        annotation_block_sizes: Vec<f32>,
    },
    /// A tate-chu-yoko run shaped in horizontal typographic mode and carried
    /// as one vertical-em atomic inline.  CSS Writing Modes performs this
    /// composition before line breaking, then compresses the horizontal run
    /// to the used em square:
    /// <https://drafts.csswg.org/css-writing-modes-4/#text-combine-layout>.
    TextCombineUpright {
        composition: Box<TextCombineComposition>,
    },
    /// Paint captured from an independent formatting context. Table-cell
    /// fragments retain their final content-coordinate context so replay does
    /// not infer writing mode from the enclosing inline line.
    InlineFragment {
        fragment: Box<PaintFragment>,
        replay_coordinates: AtomicInlineFragmentReplayCoordinates,
        table_cell_context: Option<table::TableCellContentCoordinateContext>,
        /// `true` when the captured descendants already carry the atom's
        /// contents-only overflow clip. The outer atomic stacking context
        /// must then leave the principal decoration outside that clip.
        contents_overflow_clip_applied: bool,
    },
    InlineEdge(InlineEdgeRole),
    Leader(String),
}

/// Source semantics retained by a tate-chu-yoko composition.
///
/// The parent inline graph treats the composition as one measured item, but
/// CSS Writing Modes requires line breaking at either edge to use the
/// composition's actual text and requires its horizontal contents to form an
/// independent bidi paragraph.
/// <https://drafts.csswg.org/css-writing-modes-4/#text-combine-layout>
#[derive(Debug, Clone)]
pub(in crate::layout) struct TextCombineSource {
    /// Text after CSS Text transformation and before TCY-only full-width
    /// reversal. This is the text exposed to surrounding UAX #14 resolution.
    pub(in crate::layout) boundary_text: String,
    /// The computed source style owns the internal bidi controls and paragraph
    /// direction. It is deliberately distinct from the horizontal used style.
    pub(in crate::layout) style: Box<ComputedStyle>,
}

/// Horizontally formatted paint content of one tate-chu-yoko composition.
#[derive(Debug, Clone)]
pub(in crate::layout) struct TextCombineLayout {
    pub(in crate::layout) sequence: inline_layout::InlineLineSequence,
    pub(in crate::layout) horizontal_style: Box<ComputedStyle>,
    pub(in crate::layout) inline_scale: f32,
}

/// The one-em square exposed by a tate-chu-yoko composition to its parent.
///
/// This is a logical inline and block extent, not the uncompressed horizontal
/// child width. Keeping it typed prevents paint measurement from escaping into
/// parent line fitting.
#[derive(Debug, Clone, Copy)]
pub(in crate::layout) struct TextCombineSquareGeometry {
    extent: LayoutLength,
}

impl TextCombineSquareGeometry {
    pub(in crate::layout) fn new(extent: LayoutLength) -> Self {
        debug_assert!(extent.points() >= 0.0);
        Self { extent }
    }

    pub(in crate::layout) fn points(self) -> f32 {
        self.extent.points()
    }
}

/// Complete retained representation of one tate-chu-yoko composition.
#[derive(Debug, Clone)]
pub(in crate::layout) struct TextCombineComposition {
    pub(in crate::layout) source: TextCombineSource,
    pub(in crate::layout) layout: TextCombineLayout,
    pub(in crate::layout) square: TextCombineSquareGeometry,
}

/// A raw captured atomic fragment's border-box origin on its scratch canvas.
///
/// This is deliberately distinct from a parent-line margin-box position:
/// captured descendants are positioned in the formatting context's border-box
/// coordinate system.
#[derive(Debug, Clone, Copy)]
struct ScratchAtomicBorderBoxOrigin(PaintPoint);

/// The final border-box origin selected by the parent inline line.
#[derive(Debug, Clone, Copy)]
struct FinalAtomicBorderBoxOrigin(PaintPoint);

impl FinalAtomicBorderBoxOrigin {
    fn from_prepared_border_box(border_box: PhysicalInlineRect) -> Self {
        Self(PaintPoint::new(border_box.x(), border_box.y()))
    }
}

/// Capture-space coordinates for one atomic inline formatting context.
///
/// Atomic layout paints descendants on an off-page scratch canvas, while the
/// parent inline layout later places the atom's border box after resolving the
/// margin-box participant. This frame retains only border-box origins, making
/// it impossible for an outer margin to enter captured-fragment replay:
/// <https://www.w3.org/TR/CSS22/visuren.html#inline-boxes>.
#[derive(Debug, Clone, Copy)]
pub(in crate::layout) struct AtomicInlineCaptureFrame {
    scratch_border_box_origin: ScratchAtomicBorderBoxOrigin,
}

impl AtomicInlineCaptureFrame {
    /// Build a frame from the scratch border box. The replay target is also a
    /// border box; outer CSS margins belong solely to parent line layout.
    pub(in crate::layout) fn for_scratch_border_box(scratch_border_box_origin: PaintPoint) -> Self {
        Self {
            scratch_border_box_origin: ScratchAtomicBorderBoxOrigin(scratch_border_box_origin),
        }
    }

    pub(in crate::layout) fn replay_coordinates(self) -> AtomicInlineFragmentReplayCoordinates {
        AtomicInlineFragmentReplayCoordinates {
            scratch_border_box_origin: self.scratch_border_box_origin,
        }
    }

    /// Bind a positioned layer's axis policy to this capture frame exactly
    /// once. Final inline placement can then apply one typed translation to
    /// the complete stacking context and its links.
    pub(in crate::layout) fn resolve_positioned_replay(
        self,
        replay: EscapedAtomReplay,
    ) -> EscapedAtomReplay {
        replay.resolve_from_capture_origin(self.scratch_border_box_origin.0)
    }
}

/// Coordinate contract for replaying an atomic inline's raw captured paint.
///
/// The capture frame owns the scratch-to-border-box bridge. Replay receives a
/// final border-box origin from inline line placement and must not infer a
/// second normalization from the captured fragment.
#[derive(Debug, Clone, Copy)]
pub(in crate::layout) struct AtomicInlineFragmentReplayCoordinates {
    scratch_border_box_origin: ScratchAtomicBorderBoxOrigin,
}

impl AtomicInlineFragmentReplayCoordinates {
    /// A fragment that has already been normalized to its border-box origin.
    pub(in crate::layout) fn border_box_local() -> Self {
        AtomicInlineCaptureFrame::for_scratch_border_box(PaintPoint::new(0.0, 0.0))
            .replay_coordinates()
    }

    /// Translate the raw captured fragment directly into a final parent-line
    /// border box. This is the only boundary that combines scratch-local and
    /// parent-line paint coordinates.
    pub(in crate::layout) fn replay_translation(
        self,
        final_border_box: PhysicalInlineRect,
    ) -> PaintTranslation {
        let scratch_origin = self.scratch_border_box_origin.0;
        let final_origin = FinalAtomicBorderBoxOrigin::from_prepared_border_box(final_border_box);
        debug_assert!(scratch_origin.x.is_finite());
        debug_assert!(scratch_origin.y.is_finite());
        debug_assert!(final_border_box.x().is_finite());
        debug_assert!(final_border_box.y().is_finite());
        let translation = PaintTranslation::new(
            final_origin.0.x - scratch_origin.x,
            final_origin.0.y - scratch_origin.y,
        );
        let replayed_origin = translation.transform_point(scratch_origin);
        debug_assert!(
            replayed_origin_matches_border_box(replayed_origin, final_origin.0, scratch_origin,),
            "atomic-inline replay must map its scratch border box to the final border box"
        );
        translation
    }
}

/// Account for the two rounding operations in `final - captured + captured`.
///
/// This verifies the coordinate contract without mistaking normal `f32`
/// cancellation for a misplaced paint fragment. Eight ULPs covers the two
/// arithmetic operations plus the transform adapter's scalar addition.
fn replayed_origin_matches_border_box(
    replayed: PaintPoint,
    expected: PaintPoint,
    captured: PaintPoint,
) -> bool {
    fn coordinate_matches(replayed: f32, expected: f32, captured: f32) -> bool {
        const ROUND_TRIP_ULPS: f32 = 8.0;
        let scale = replayed
            .abs()
            .max(expected.abs())
            .max(captured.abs())
            .max(1.0);
        (replayed - expected).abs() <= ROUND_TRIP_ULPS * f32::EPSILON * scale
    }

    coordinate_matches(replayed.x, expected.x, captured.x)
        && coordinate_matches(replayed.y, expected.y, captured.y)
}

/// A measured ruby base or annotation level together with the role that
/// establishes its line-box metrics.
///
/// Ruby annotations are separate inline formatting contexts: their `rt`/
/// `rtc` line-height must not be replaced with the containing ruby box's
/// line-height when they are replayed beside the base level.
/// <https://drafts.csswg.org/css-ruby-1/#ruby-layout>
#[derive(Debug, Clone)]
pub(in crate::layout) struct RubyInlineLevel {
    pub(in crate::layout) sequence: inline_layout::InlineLineSequence,
    pub(in crate::layout) style: Box<ComputedStyle>,
    /// `ruby-overhang` applies to the annotation container, not the `rt`
    /// segment. Keep this source explicit after anonymous ruby normalization.
    pub(in crate::layout) overhang_policy: css::RubyOverhang,
    /// Logical inline extent occupied after `ruby-align: space-around`
    /// distribution. This can be narrower than its column: the remaining
    /// space is the equal half-opportunity at each content edge.
    pub(in crate::layout) paint_inline_size: ruby::RubyPaintInlineSpan,
    /// Logical inline extent available for this level's paint replay. A
    /// spanning annotation owns the combined width of its paired base
    /// columns while the parent line still advances column by column.
    pub(in crate::layout) containing_inline_size: ruby::RubyColumnInlineSpan,
    /// Whether this level starts its paired base span. Continuation columns
    /// retain an empty sidecar only to preserve annotation-level indexing.
    pub(in crate::layout) starts_span: bool,
    /// Number of adjacent base columns this level covers when it starts a
    /// span. Non-starting continuation entries use zero-width paint.
    pub(in crate::layout) column_span: usize,
}

/// One owned inline box edge fragment carried through inline layout.
///
/// CSS 2.2 includes inline-axis margin, border, and padding at the start and
/// end of inline boxes, and negative margins reduce the advance without
/// removing border or padding paint. Split inline boxes also own only the
/// relevant start/end decoration under `box-decoration-break: slice`:
/// <https://www.w3.org/TR/CSS22/box.html#margin-properties>,
/// <https://www.w3.org/TR/CSS22/visuren.html#inline-formatting>, and
/// <https://www.w3.org/TR/css-break-3/#break-decoration>.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(in crate::layout) struct InlineBoxEdgeFragment {
    pub(in crate::layout) logical_edge: InlineLogicalEdge,
    pub(in crate::layout) physical_side: PhysicalSide,
    pub(in crate::layout) positioning_containing_block_id:
        Option<InlinePositioningContainingBlockId>,
    pub(in crate::layout) advance: f32,
    pub(in crate::layout) paint_extent: f32,
}

impl InlineBoxEdgeFragment {
    /// Whether this owned box edge makes an otherwise empty line box exist.
    ///
    /// CSS 2.2 treats an inline box with a non-zero inline-axis margin,
    /// border, or padding as line-generating content. `advance` retains the
    /// signed sum of those components, while `paint_extent` retains border
    /// and padding when a negative margin cancels that sum. Together they
    /// distinguish a structural zero-width marker from a decorated edge
    /// without storing a second, potentially inconsistent flag.
    /// <https://www.w3.org/TR/CSS22/visuren.html#inline-formatting>
    pub(in crate::layout) fn contributes_to_line_box(self) -> bool {
        self.advance != 0.0 || self.paint_extent != 0.0
    }

    /// Whether this atom is the zero-advance positioning marker placed inside
    /// a positioned bidi isolate rather than an inline box-decoration edge.
    pub(in crate::layout) fn is_positioning_marker(self) -> bool {
        self.positioning_containing_block_id.is_some()
            && self.advance == 0.0
            && self.paint_extent == 0.0
    }
}

#[cfg(test)]
mod inline_box_edge_fragment_tests {
    use super::*;

    fn edge(
        advance: f32,
        paint_extent: f32,
        positioning_containing_block_id: Option<InlinePositioningContainingBlockId>,
    ) -> InlineBoxEdgeFragment {
        InlineBoxEdgeFragment {
            logical_edge: InlineLogicalEdge::Start,
            physical_side: PhysicalSide::Left,
            positioning_containing_block_id,
            advance,
            paint_extent,
        }
    }

    #[test]
    fn box_edge_line_contribution_preserves_signed_and_painted_geometry() {
        assert!(!edge(0.0, 0.0, None).contributes_to_line_box());
        assert!(edge(10.0, 0.0, None).contributes_to_line_box());
        assert!(edge(-10.0, 0.0, None).contributes_to_line_box());
        assert!(edge(0.0, 10.0, None).contributes_to_line_box());

        let marker = edge(0.0, 0.0, Some(InlinePositioningContainingBlockId(1)));
        assert!(marker.is_positioning_marker());
        assert!(!marker.contributes_to_line_box());
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(in crate::layout) enum InlineLogicalEdge {
    Start,
    End,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(in crate::layout) enum InlineEdgeRole {
    BoxEdge(InlineBoxEdgeFragment),
    /// A zero-advance empty inline scope that contributes only its resolved
    /// strut metrics to the line. It has no box geometry, paint, or text.
    ///
    /// CSS Inline line layout uses an inline box's font metrics even when the
    /// box has no glyph-bearing descendants. Keeping this distinct from a
    /// transparent box edge lets list-marker anchoring observe that first
    /// line without manufacturing a decoration or inline advance.
    /// <https://drafts.csswg.org/css-inline-3/#line-height>
    MetricsOnlyStrut,
    /// A CSS Text `text-autospace` advance bound to one logical text edge.
    ///
    /// The atom is only the graph carrier: it has no source text, paint, or
    /// UAX #14 atomic-object role. Its semantic advance remains typed until
    /// the inline-layout boundary converts it to a physical inline size.
    /// <https://drafts.csswg.org/css-text-4/#text-autospace-property>
    TextAutospace(InlineTextBoundarySpacing),
}

/// The non-text advance selected at one logical `text-autospace` boundary.
///
/// CSS Text defines this as `1/8ic` owned by the innermost inline containing
/// the boundary. Keeping it distinct from an authored space prevents it from
/// entering extraction or line-break text while preserving a CSS length's
/// layout unit.
/// <https://drafts.csswg.org/css-text-4/#text-autospace-property>
#[derive(Debug, Clone, Copy, PartialEq)]
pub(in crate::layout) struct InlineTextBoundarySpacing {
    advance: LayoutLength,
}

impl InlineTextBoundarySpacing {
    pub(in crate::layout) const fn new(advance: LayoutLength) -> Self {
        Self { advance }
    }

    pub(in crate::layout) const fn advance(self) -> LayoutLength {
        self.advance
    }
}

impl InlineAtomContent {
    /// Return source text whose first and last characters participate in CSS
    /// Text line breaking at this atom's outer boundaries.
    ///
    /// Ruby columns and tate-chu-yoko remain indivisible layout participants,
    /// but neither is a replacement character for UAX #14 at those edges.
    /// No graph position is exposed inside the returned text.
    /// <https://drafts.csswg.org/css-writing-modes-4/#text-combine-layout>
    /// <https://drafts.csswg.org/css-ruby-1/#line-breaks>
    pub(in crate::layout) fn textual_boundary_text(&self) -> Option<&str> {
        match self {
            Self::Ruby { base_text, .. } => Some(base_text),
            Self::TextCombineUpright { composition } => {
                Some(composition.source.boundary_text.as_str())
            }
            _ => None,
        }
    }

    /// Return the computed style that owns textual boundary policy when it is
    /// more specific than the atom's ordinary used layout style.
    pub(in crate::layout) fn textual_boundary_style(&self) -> Option<&ComputedStyle> {
        match self {
            Self::TextCombineUpright { composition } => Some(&composition.source.style),
            _ => None,
        }
    }

    /// Tracking used at this atom's typographic boundary, independently of
    /// the lexical scope retained for boundary ownership.
    ///
    /// CSS Writing Modes requires spacing inside and after a tate-chu-yoko
    /// composition to be zero; the preceding external unit still contributes
    /// its own half of the surrounding tracking boundary.
    /// <https://drafts.csswg.org/css-writing-modes-4/#text-combine-layout>
    pub(in crate::layout) fn ignores_boundary_letter_spacing(&self) -> bool {
        matches!(self, Self::TextCombineUpright { .. })
    }

    pub(in crate::layout) fn is_inline_edge(&self) -> bool {
        matches!(self, Self::InlineEdge(_))
    }

    pub(in crate::layout) fn is_box_edge(&self) -> bool {
        matches!(self, Self::InlineEdge(InlineEdgeRole::BoxEdge(_)))
    }
}

#[derive(Debug, Clone)]
pub(in crate::layout) enum InlineItem {
    Word(Box<InlineWord>),
    Atom(Box<InlineAtom>),
    /// Zero-advance source-order marker for deferred static-position replay.
    ///
    /// This is structural state rather than an inline atom: it is transparent
    /// to CSS Text adjacency and line metrics, but remains in the collected
    /// stream until hypothetical replay replaces the exact matching source
    /// with a measured normal-flow atom.
    /// <https://drafts.csswg.org/css-position-3/#static-position>
    StaticPositionSourceMarker(InlineStaticPositionSourceId),
    Float(Box<InlineFloat>),
    Break(InlineBreak),
    PageScopeStart(Option<String>),
    PageScopeEnd,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::layout) struct InlineBreak {
    pub(in crate::layout) clear: Clear,
    /// Whether this boundary originated in an explicit inline break element or
    /// in a preserved CSS Text segment break.  Phase II line-edge treatment
    /// must not infer this from visual position after the stream is split.
    /// <https://drafts.csswg.org/css-text-3/#white-space-phase-1>
    pub(in crate::layout) origin: InlineBreakOrigin,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(in crate::layout) enum InlineBreakOrigin {
    #[default]
    Explicit,
    PreservedSegment,
}

impl Default for InlineBreak {
    fn default() -> Self {
        Self {
            clear: Clear::None,
            origin: InlineBreakOrigin::Explicit,
        }
    }
}

impl AsRef<InlineItem> for InlineItem {
    fn as_ref(&self) -> &InlineItem {
        self
    }
}

#[derive(Debug, Clone)]
#[allow(clippy::large_enum_variant)]
pub(in crate::layout) enum InlineLineItem {
    Fragment(InlineFragment),
    Atom(InlineAtom),
    Float(InlineFloat),
}

impl AsRef<InlineLineItem> for InlineLineItem {
    fn as_ref(&self) -> &InlineLineItem {
        self
    }
}

/// One successful source-order inline-float placement.
///
/// `exclusion` is the physical float shape registered for later line-band
/// queries.  Its `id` and `source_order` identify the already-captured float
/// paint subtree, so replay can consume this placement without re-laying out
/// the floated DOM element.
#[derive(Debug, Clone)]
pub(in crate::layout) struct CommittedInlineFloat {
    /// `(run_index, byte_offset)` in the inline opportunity graph.  Keeping
    /// the source identity as scalars avoids leaking the graph's private
    /// representation into the layout-wide transaction state.
    pub(in crate::layout) marker: (usize, usize),
    pub(in crate::layout) selected_row: usize,
    /// Source and physical-row data retained for an inline formatter replay.
    ///
    /// The exclusion itself remains the cross-traversal ownership record;
    /// this metadata only lets a later source graph (for example after a
    /// preserved forced break) select its first line against that committed
    /// exclusion without laying out or painting the float again.
    pub(in crate::layout) replay: InlineFloatReplayMetadata,
    pub(in crate::layout) exclusion: FloatShape,
}

/// Durable replay metadata for a source-order inline float.
///
/// CSS 2.2 float placement is source ordered even when an explicit break
/// splits one inline formatting context into several opportunity graphs.  The
/// formatter therefore retains the selected source range and the physical
/// placement row independently of the float's paint ownership:
/// <https://www.w3.org/TR/CSS22/visuren.html#floats>.
#[derive(Debug, Clone, Copy)]
pub(in crate::layout) struct InlineFloatReplayMetadata {
    pub(in crate::layout) source_range_start: (usize, usize),
    pub(in crate::layout) source_range_end: (usize, usize),
    pub(in crate::layout) physical_row: usize,
    pub(in crate::layout) physical_block_offset: f32,
    pub(in crate::layout) used_block_advance: f32,
}

impl CommittedInlineFloat {
    /// Validate the durable transaction at the point ordinary child traversal
    /// consumes it instead of replaying the floated subtree.
    pub(in crate::layout) fn is_valid(&self) -> bool {
        self.marker.0 != usize::MAX
            && self.marker.1 != usize::MAX
            && self.selected_row != usize::MAX
            && self.replay.physical_row != usize::MAX
            && self.exclusion.is_css_float()
    }
}

/// The logical sequence of anonymous multicol fragmentainers.
///
/// CSS Fragmentation gives one fragmentation context one block-flow direction;
/// descendants can use different writing modes without changing that sequence.
/// The page contexts remain a scratch-layout implementation detail: placement
/// clients must obtain them through this sequence rather than derive a column
/// direction from a physical page cursor.
/// <https://www.w3.org/TR/css-break-3/#fragmentation-model>
/// <https://www.w3.org/TR/css-multicol-1/#the-multi-column-model>
#[derive(Debug, Clone, Copy, PartialEq)]
pub(in crate::layout) struct FragmentainerSequence {
    flow_axes: FlowAxes,
    /// Context used by the initial anonymous fragmentainers in a partial row.
    initial_context: PageContext,
    /// Number of fragmentainers that share `initial_context` before later
    /// rows use `continuation_context`.
    initial_fragmentainer_count: usize,
    continuation_context: PageContext,
}

/// The source-local logical block interval owned by one anonymous
/// fragmentainer in a multicolumn sequence.
///
/// This interval is deliberately independent of a final page rectangle. A
/// temporary first row may have a shorter capacity than later rows, while the
/// final replay can place either row on a different physical page or column.
/// Keeping the two facts together at the sequence boundary prevents a caller
/// from deriving source progress from physical X/Y coordinates.
/// <https://www.w3.org/TR/css-break-3/#fragmentation-model>
#[derive(Debug, Clone, Copy, PartialEq)]
pub(in crate::layout) struct FragmentainerBlockInterval {
    start: f32,
    capacity: f32,
}

impl FragmentainerBlockInterval {
    pub(in crate::layout) fn start(self) -> f32 {
        self.start
    }

    pub(in crate::layout) fn capacity(self) -> f32 {
        self.capacity
    }
}

/// One anonymous fragmentainer selected from a [`FragmentainerSequence`].
///
/// This is the boundary between the logical multicolumn sequence and the
/// page-shaped scratch canvas used to lay out its contents.  Consumers use the
/// logical block edges and physical content rectangle from this value instead
/// of reconstructing either from a page cursor.
/// <https://www.w3.org/TR/css-break-3/#fragmentation-model>
/// <https://www.w3.org/TR/css-multicol-1/#the-multi-column-model>
#[derive(Debug, Clone, Copy, PartialEq)]
pub(in crate::layout) struct FragmentainerPlacement {
    ordinal: usize,
    flow_axes: FlowAxes,
    logical_block_interval: FragmentainerBlockInterval,
    /// Final destination geometry selected by the owning fragmentation
    /// context. This deliberately does not follow the scratch context once
    /// a multicolumn formatter supplies a replay placement.
    content_rect: PageTopRect,
    scratch_context: PageContext,
}

impl FragmentainerPlacement {
    fn for_scratch_context(
        ordinal: usize,
        flow_axes: FlowAxes,
        logical_block_interval: FragmentainerBlockInterval,
        scratch_context: PageContext,
    ) -> Self {
        Self {
            ordinal,
            flow_axes,
            logical_block_interval,
            content_rect: PageTopRect::new(
                scratch_context.left(),
                scratch_context.top(),
                scratch_context.area_width(),
                scratch_context.area_height(),
            ),
            scratch_context,
        }
    }

    pub(in crate::layout) fn ordinal(self) -> usize {
        self.ordinal
    }

    pub(in crate::layout) fn flow_axes(self) -> FlowAxes {
        self.flow_axes
    }

    pub(in crate::layout) fn content_rect(self) -> PageTopRect {
        self.content_rect
    }

    pub(in crate::layout) fn scratch_context(self) -> PageContext {
        self.scratch_context
    }

    pub(in crate::layout) fn logical_block_capacity(self) -> f32 {
        self.logical_block_interval.capacity()
    }

    pub(in crate::layout) fn logical_block_start(self) -> f32 {
        self.logical_block_interval.start()
    }

    /// Physical edge at logical block start in the owning fragmentation
    /// context. Callers must use this instead of interpreting a page cursor
    /// as a block coordinate.
    pub(in crate::layout) fn block_start_edge(self) -> f32 {
        match self.flow_axes.block_start_side() {
            PhysicalSide::Top => self.content_rect.top_y(),
            PhysicalSide::Bottom => self.content_rect.bottom_y(),
            PhysicalSide::Left => self.content_rect.x(),
            PhysicalSide::Right => self.content_rect.x() + self.content_rect.width(),
        }
    }

    pub(in crate::layout) fn block_end_edge(self) -> f32 {
        match self.flow_axes.block_start_side() {
            PhysicalSide::Top => self.content_rect.bottom_y(),
            PhysicalSide::Bottom => self.content_rect.top_y(),
            PhysicalSide::Left => self.content_rect.x() + self.content_rect.width(),
            PhysicalSide::Right => self.content_rect.x(),
        }
    }
}

impl FragmentainerSequence {
    pub(in crate::layout) fn new(
        flow_axes: FlowAxes,
        initial_context: PageContext,
        initial_fragmentainer_count: usize,
        continuation_context: PageContext,
    ) -> Self {
        Self {
            flow_axes,
            initial_context,
            initial_fragmentainer_count,
            continuation_context,
        }
    }

    pub(in crate::layout) fn context_for_fragmentainer(self, index: usize) -> PageContext {
        self.placement_for_fragmentainer(index).scratch_context()
    }

    pub(in crate::layout) fn placement_for_fragmentainer(
        self,
        index: usize,
    ) -> FragmentainerPlacement {
        let (context, interval) = self.fragmentainer_context_and_interval(index);
        FragmentainerPlacement::for_scratch_context(index, self.flow_axes, interval, context)
    }

    /// Select the ordinal that owns a continuous logical block position.
    ///
    /// A wrapper child such as a table caption may consume several outer
    /// columns without materializing each scratch page. Its following sibling
    /// still starts in the column containing the resulting source position,
    /// including when the first row has a distinct capacity.
    /// <https://www.w3.org/TR/css-break-3/#fragmentation-model>
    pub(in crate::layout) fn placement_for_logical_block_position(
        self,
        position: f32,
    ) -> FragmentainerPlacement {
        let position = position.max(0.0);
        let first = self
            .initial_context
            .logical_block_size(self.flow_axes.writing_mode())
            .max(0.0);
        let initial_span = first * self.initial_fragmentainer_count as f32;
        let index = if first > 0.01 && position < initial_span {
            (position / first).floor() as usize
        } else {
            let continuation = self
                .continuation_context
                .logical_block_size(self.flow_axes.writing_mode())
                .max(0.0);
            if continuation <= 0.01 {
                self.initial_fragmentainer_count
            } else {
                self.initial_fragmentainer_count
                    + ((position - initial_span).max(0.0) / continuation).floor() as usize
            }
        };
        self.placement_for_fragmentainer(index)
    }

    /// Bind one ordinal to the physical rectangle selected during committed
    /// multicolumn replay.
    ///
    /// The caller may supply final geometry, but cannot independently choose
    /// its axes, source interval, or scratch context. This keeps speculative
    /// layout and replay tied to the same sequence ordinal.
    /// <https://www.w3.org/TR/css-break-3/#fragmentation-model>
    pub(in crate::layout) fn placement_at_destination(
        self,
        index: usize,
        content_rect: PageTopRect,
    ) -> FragmentainerPlacement {
        let mut placement = self.placement_for_fragmentainer(index);
        placement.content_rect = content_rect;
        placement
    }

    pub(in crate::layout) fn current_placement(
        self,
        completed_fragmentainers: usize,
    ) -> FragmentainerPlacement {
        self.placement_for_fragmentainer(completed_fragmentainers)
    }

    pub(in crate::layout) fn continuation_context(self) -> PageContext {
        self.continuation_context
    }

    fn fragmentainer_context_and_interval(
        self,
        index: usize,
    ) -> (PageContext, FragmentainerBlockInterval) {
        let first_capacity = self
            .initial_context
            .logical_block_size(self.flow_axes.writing_mode())
            .max(0.0);
        let continuation_capacity = self
            .continuation_context
            .logical_block_size(self.flow_axes.writing_mode())
            .max(0.0);
        let initial_count = self.initial_fragmentainer_count;
        let (context, start, capacity) = if index < initial_count {
            (
                self.initial_context,
                index as f32 * first_capacity,
                first_capacity,
            )
        } else {
            (
                self.continuation_context,
                initial_count as f32 * first_capacity
                    + (index - initial_count) as f32 * continuation_capacity,
                continuation_capacity,
            )
        };
        (context, FragmentainerBlockInterval { start, capacity })
    }
}

/// A temporary fragmentainer materialized through Quire's page cursor.
///
/// Multi-column layout uses isolated page-shaped canvases as anonymous column
/// boxes. This keeps all existing nested fragmentation paths on one cursor
/// model while resolving target-specific breaks as column breaks.
/// <https://www.w3.org/TR/css-multicol-1/#the-multi-column-model>
#[derive(Debug, Clone, Copy, PartialEq)]
pub(in crate::layout) struct FragmentainerOverride {
    pub(in crate::layout) kind: FragmentainerKind,
    pub(in crate::layout) sequence: FragmentainerSequence,
    pub(in crate::layout) relax_widows_orphans: bool,
}

impl FragmentainerOverride {
    pub(in crate::layout) fn context_for_fragmentainer(self, index: usize) -> PageContext {
        self.sequence.context_for_fragmentainer(index)
    }

    pub(in crate::layout) fn placement_for_fragmentainer(
        self,
        index: usize,
    ) -> FragmentainerPlacement {
        self.sequence.placement_for_fragmentainer(index)
    }

    pub(in crate::layout) fn continuation_context(self) -> PageContext {
        self.sequence.continuation_context()
    }
}

/// The typed content-box containing block of one anonymous multicol column.
///
/// A nested multicol formatting context can be measured in a temporary page
/// whose physical coordinates do not describe its parent column. Retaining
/// both the semantic inline size and source paint origin prevents its
/// percentages and committed paint geometry from diverging:
/// <https://www.w3.org/TR/css-multicol-1/#column-box>.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(in crate::layout) struct MulticolColumnContainingBlock {
    pub(in crate::layout) inline_size: LogicalInlineContentSize,
    pub(in crate::layout) content_left: f32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::layout) struct AnchorText {
    pub(in crate::layout) content: String,
    pub(in crate::layout) before: String,
    pub(in crate::layout) after: String,
}

/// Target data captured at the target element's first fragment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::layout) struct TargetAnchor {
    pub(in crate::layout) page_index: usize,
    pub(in crate::layout) text: AnchorText,
    pub(in crate::layout) counters: HashMap<String, Vec<i32>>,
}

/// Immutable target state exchanged between complete fresh layout passes.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(in crate::layout) struct TargetReferenceSnapshot {
    pub(in crate::layout) anchors: HashMap<String, TargetAnchor>,
    pub(in crate::layout) total_pages: usize,
}

#[cfg(test)]
mod atomic_inline_capture_frame_tests {
    use super::*;

    #[test]
    fn replay_origin_invariant_allows_f32_subtract_add_rounding() {
        let captured = PaintPoint::new(20.0, 20.0);
        let expected = PaintPoint::new(20.0, 39.999_996);
        let replayed = PaintTranslation::new(expected.x - captured.x, expected.y - captured.y)
            .transform_point(captured);

        assert!(replayed_origin_matches_border_box(
            replayed, expected, captured
        ));
    }

    #[test]
    fn replay_origin_invariant_rejects_meaningful_coordinate_errors() {
        assert!(!replayed_origin_matches_border_box(
            PaintPoint::new(20.0, 40.01),
            PaintPoint::new(20.0, 40.0),
            PaintPoint::new(20.0, 20.0),
        ));
    }

    #[test]
    fn border_box_capture_retains_the_scratch_border_origin() {
        let frame = AtomicInlineCaptureFrame::for_scratch_border_box(PaintPoint::new(12.0, 34.0));
        assert_eq!(
            frame.scratch_border_box_origin.0,
            PaintPoint::new(12.0, 34.0)
        );
    }

    #[test]
    fn replay_translation_maps_the_scratch_border_box_to_the_final_border_box() {
        let frame = AtomicInlineCaptureFrame::for_scratch_border_box(PaintPoint::new(12.0, 34.0));
        let translation = frame
            .replay_coordinates()
            .replay_translation(PhysicalInlineRect::new(InlineRect::new(
                InlinePoint::new(20.0, 30.0),
                InlineSize::new(10.0, 8.0),
            )));
        assert_eq!(
            translation.transform_point(PaintPoint::new(12.0, 34.0)),
            PaintPoint::new(20.0, 30.0),
        );
    }
}

#[cfg(test)]
mod fragmentainer_sequence_tests {
    use super::*;

    #[test]
    fn placement_retains_ordinal_axes_and_scratch_content_geometry() {
        let options = RenderOptions::default();
        let context = PageContext::from_options(&options);
        let sequence = FragmentainerSequence::new(
            FlowAxes::new(WritingMode::VerticalRl, Direction::Rtl),
            context,
            1,
            context,
        );
        let placement = sequence.placement_for_fragmentainer(2);

        assert_eq!(placement.ordinal(), 2);
        assert_eq!(
            placement.flow_axes().writing_mode(),
            WritingMode::VerticalRl
        );
        assert_eq!(placement.scratch_context(), context);
        assert_eq!(placement.content_rect().x(), context.left());
        assert_eq!(placement.content_rect().top_y(), context.top());
        assert_eq!(
            placement.logical_block_capacity(),
            context.logical_block_size(WritingMode::VerticalRl),
        );
        assert_eq!(
            placement.logical_block_interval.start,
            2.0 * context.logical_block_size(WritingMode::VerticalRl),
        );
    }

    #[test]
    fn destination_placement_projects_block_edges_from_outer_axes() {
        let context = PageContext::from_options(&RenderOptions::default());
        let rect = PageTopRect::new(20.0, 100.0, 30.0, 40.0);

        let lr = FragmentainerSequence::new(
            FlowAxes::new(WritingMode::VerticalLr, Direction::Rtl),
            context,
            1,
            context,
        )
        .placement_at_destination(3, rect);
        assert_eq!(lr.block_start_edge(), 20.0);
        assert_eq!(lr.block_end_edge(), 50.0);

        let rl = FragmentainerSequence::new(
            FlowAxes::new(WritingMode::VerticalRl, Direction::Rtl),
            context,
            1,
            context,
        )
        .placement_at_destination(3, rect);
        assert_eq!(rl.block_start_edge(), 50.0);
        assert_eq!(rl.block_end_edge(), 20.0);
    }

    #[test]
    fn first_fragment_variation_precedes_the_continuation_intervals() {
        let base = PageContext::from_options(&RenderOptions::default());
        let initial = PageContext {
            margins: PageMargins::from_points(
                0.0,
                base.size.width() - 40.0,
                base.size.height() - 150.0,
                0.0,
            ),
            ..base
        };
        let continuation = PageContext {
            margins: PageMargins::from_points(
                0.0,
                base.size.width() - 30.0,
                base.size.height() - 150.0,
                0.0,
            ),
            ..base
        };
        let sequence = FragmentainerSequence::new(
            FlowAxes::new(WritingMode::VerticalRl, Direction::Rtl),
            initial,
            1,
            continuation,
        );

        let first = sequence.placement_for_fragmentainer(0);
        let second = sequence.placement_for_fragmentainer(1);
        let third = sequence.placement_for_fragmentainer(2);

        assert_eq!(first.logical_block_capacity(), 40.0);
        assert_eq!(second.logical_block_capacity(), 30.0);
        assert_eq!(second.logical_block_interval.start, 40.0);
        assert_eq!(third.logical_block_interval.start, 70.0);
    }
}

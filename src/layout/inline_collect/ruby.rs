use std::rc::Rc;

use super::*;
use crate::layout::ruby as layout_ruby;

impl<'a> LayoutBuilder<'a> {
    #[allow(clippy::too_many_arguments)]
    pub(super) fn collect_normalized_ruby_items(
        &mut self,
        children: &[box_tree::FormattingBox<'_>],
        ruby_style: &ComputedStyle,
        stylesheets: &Stylesheets<'_>,
        link: Option<String>,
        placement: InlinePlacement,
        block_style: &ComputedStyle,
        first_letter_style: Option<&ComputedStyle>,
        propagated_decoration_layers: Vec<css::TextDecorationLayer>,
        output: &mut Vec<InlineItem>,
    ) -> bool {
        let normalized = layout_ruby::NormalizedRuby::from_children(children);
        debug_assert!(
            normalized
                .columns
                .iter()
                .all(|column| column.annotations.len() == normalized.annotation_level_count)
        );
        let mut ruby_atoms = Vec::with_capacity(normalized.columns.len());
        let mut pending_first_letter_style = first_letter_style;
        for column in &normalized.columns {
            let Some((mut atom, has_base_content)) = self.ruby_inline_atom(
                column,
                &normalized.annotation_container_styles,
                ruby_style,
                stylesheets,
                link.clone(),
                placement,
                block_style,
                pending_first_letter_style,
                propagated_decoration_layers.clone(),
            ) else {
                return false;
            };
            if has_base_content {
                pending_first_letter_style = None;
            }
            atom.baseline_shift += self
                .vertical_align_baseline_shift_for_atom(&atom, block_style)
                .glyph_displacement()
                .get();
            ruby_atoms.push(InlineItem::Atom(Box::new(atom)));
        }
        normalize_ruby_column_group_metrics(&mut ruby_atoms, block_style);
        normalize_ruby_annotation_span_inline_sizes(&mut ruby_atoms, block_style);
        if ruby_atoms.is_empty() {
            return false;
        }
        output.extend(ruby_atoms);
        true
    }

    /// Build a coupled ruby base/annotation atom from normalized in-flow
    /// segments.  This is the materialization boundary between CSS Ruby's
    /// paired levels and the parent inline graph.
    ///
    /// The graph currently keeps a whole ruby group together; later work can
    /// replace this atom with per-column graph ranges without changing the
    /// normalization or paint representation.
    /// <https://drafts.csswg.org/css-ruby-1/#ruby-layout>
    #[allow(clippy::too_many_arguments)]
    fn ruby_inline_atom(
        &mut self,
        column: &layout_ruby::RubyColumn<'_>,
        annotation_container_styles: &[Option<Rc<ComputedStyle>>],
        ruby_style: &ComputedStyle,
        stylesheets: &Stylesheets<'_>,
        link: Option<String>,
        placement: InlinePlacement,
        block_style: &ComputedStyle,
        first_letter_style: Option<&ComputedStyle>,
        propagated_decoration_layers: Vec<css::TextDecorationLayer>,
    ) -> Option<(InlineAtom, bool)> {
        // A Ruby level is not an independently originating block line. Its
        // own `::first-line` rules must therefore remain dormant; the parent
        // block applies its selected overlay to the complete ruby formatting
        // context once the parent first line is known.
        // <https://drafts.csswg.org/css-pseudo-4/#first-line-pseudo>
        let mut base_style = column.base.style.as_deref().unwrap_or(ruby_style).clone();
        base_style.first_line_style = None;
        base_style.suppress_inapplicable_transform();
        let mut base_items = Vec::new();
        self.collect_inline_box_items(
            &column.base.boxes,
            stylesheets,
            link.clone(),
            placement.baseline_shift(),
            placement.visual_offset,
            block_style,
            propagated_decoration_layers.clone(),
            &mut base_items,
        );
        let has_base_content = base_items.iter().any(inline_item_has_typographic_content);
        if let Some(first_letter_style) = first_letter_style {
            apply_first_letter_style_to_ruby_base_items(&mut base_items, first_letter_style);
        }
        // Ruby's no-break-inside default means this temporary measurement can
        // use a deliberately unbounded inline span.  Its selected fragments
        // are replayed into the final coupled width below.
        let unconstrained_inline_size = 1_000_000.0;
        let base_items_for_distribution = base_items.clone();
        let mut base = RubyInlineLevel {
            sequence: self.collect_ruby_level_line_sequence(
                base_items,
                &base_style,
                unconstrained_inline_size,
                0.0,
                0.0,
            ),
            style: Box::new(base_style.clone()),
            overhang_policy: ruby_style.ruby_overhang,
            paint_inline_size: layout_ruby::RubyPaintInlineSpan::default(),
            containing_inline_size: layout_ruby::RubyColumnInlineSpan::default(),
            starts_span: true,
            column_span: 1,
        };
        let mut annotations = Vec::with_capacity(column.annotations.len());
        let mut annotation_sides = Vec::with_capacity(column.annotations.len());
        let mut annotation_items_for_distribution = Vec::with_capacity(column.annotations.len());
        for (annotation_index, annotation) in column.annotations.iter().enumerate() {
            let annotation_container_style = annotation_container_styles
                .get(annotation_index)
                .and_then(Option::as_deref)
                .unwrap_or(ruby_style);
            let mut annotation_style = annotation
                .segment
                .style
                .as_deref()
                .unwrap_or(ruby_style)
                .clone();
            annotation_style.first_line_style = None;
            annotation_style.suppress_inapplicable_transform();
            annotation_sides.push(annotation_style.ruby_position.interlinear_side());
            let mut annotation_items = Vec::new();
            // A structurally present annotation containing generated `" "`
            // is real ruby content. Only the explicitly synthesized empty
            // counterpart has no inner formatting context to collect.
            // <https://drafts.csswg.org/css-ruby-1/#anon-gen-ruby>
            if annotation.starts_span && !annotation.segment.is_empty() {
                self.collect_inline_box_items(
                    &annotation.segment.boxes,
                    stylesheets,
                    link.clone(),
                    placement.baseline_shift(),
                    placement.visual_offset,
                    block_style,
                    propagated_decoration_layers.clone(),
                    &mut annotation_items,
                );
            }
            annotation_items_for_distribution.push(annotation_items.clone());
            annotations.push(RubyInlineLevel {
                sequence: self.collect_ruby_level_line_sequence(
                    annotation_items,
                    &annotation_style,
                    unconstrained_inline_size,
                    0.0,
                    0.0,
                ),
                style: Box::new(annotation_style.clone()),
                overhang_policy: annotation_container_style.ruby_overhang,
                paint_inline_size: layout_ruby::RubyPaintInlineSpan::default(),
                containing_inline_size: layout_ruby::RubyColumnInlineSpan::default(),
                starts_span: annotation.starts_span,
                column_span: annotation.span,
            });
        }
        if !has_base_content
            && !annotations.iter().any(|sequence| {
                sequence
                    .sequence
                    .records
                    .iter()
                    .any(|record| record.fragment.is_some())
            })
        {
            return None;
        }
        let sequence_inline_size = |sequence: &inline_layout::InlineLineSequence| {
            sequence
                .records
                .iter()
                .filter_map(|record| record.fragment.as_ref())
                .map(|fragment| fragment.metrics.width)
                .fold(0.0, f32::max)
        };
        // The source atom is conservatively measured at the widest level so
        // candidate line fitting never accepts an annotation that cannot fit.
        // The paired base-column span remains distinct: selected-line ruby
        // overhang later borrows adjacent inline space and reduces this
        // provisional advance without changing source geometry.
        let provisional_inline_size = layout_ruby::RubyInlineSpan::new(
            column
                .annotations
                .iter()
                .zip(annotations.iter())
                // A spanning annotation is sized and aligned across the complete
                // paired base range. It must not inflate each base column (and
                // thereby manufacture parent-line justification opportunities);
                // excess annotation width overhangs the spanned range.
                // <https://drafts.csswg.org/css-ruby-1/#ruby-overhang>
                .filter(|(annotation, _)| annotation.span == 1)
                .map(|(_, sequence)| sequence_inline_size(&sequence.sequence))
                .fold(sequence_inline_size(&base.sequence), f32::max),
        )
        .points();
        let column_inline_size = sequence_inline_size(&base.sequence);
        base.paint_inline_size =
            layout_ruby::RubyPaintInlineSpan::new(sequence_inline_size(&base.sequence));
        base.containing_inline_size = layout_ruby::RubyColumnInlineSpan::new(column_inline_size);
        for annotation in &mut annotations {
            annotation.paint_inline_size =
                layout_ruby::RubyPaintInlineSpan::new(sequence_inline_size(&annotation.sequence));
            annotation.containing_inline_size =
                layout_ruby::RubyColumnInlineSpan::new(column_inline_size);
        }
        self.distribute_ruby_level_space_around(
            &mut base,
            &base_items_for_distribution,
            column_inline_size,
        );
        for ((annotation, source_items), pairing) in annotations
            .iter_mut()
            .zip(annotation_items_for_distribution.iter())
            .zip(column.annotations.iter())
        {
            // A spanning annotation is positioned by the column group that
            // owns its full span. This per-column atom only has its local
            // span available today, so retain its natural alignment until
            // group-level span paint is materialized below.
            if pairing.span == 1 {
                self.distribute_ruby_level_space_around(
                    annotation,
                    source_items,
                    column_inline_size,
                );
            }
        }
        let base_block_size = base.sequence.total_height().max(base_style.line_height);
        let annotation_block_sizes = annotations
            .iter()
            .map(|annotation| annotation.sequence.total_height())
            .collect::<Vec<_>>();
        let annotation_block_size = annotation_block_sizes.iter().sum::<f32>();
        let base_baseline = base
            .sequence
            .records
            .iter()
            .find_map(|record| {
                record
                    .fragment
                    .as_ref()
                    .map(|fragment| fragment.metrics.baseline_offset)
            })
            .unwrap_or_else(|| {
                self.font_system
                    .rendered_first_line_baseline_offset(ruby_style)
                    .points()
            });
        Some((
            InlineAtom::new(
                InlineAtomContent::Ruby {
                    base_text: column.base.boundary_text(),
                    base,
                    annotations,
                    annotation_sides,
                    base_block_size,
                    annotation_block_sizes,
                },
                ruby_style.clone(),
                None,
                InlineSize::new(
                    provisional_inline_size,
                    base_block_size + annotation_block_size,
                ),
                annotation_block_size + base_baseline,
                placement.baseline_shift(),
                link,
                None,
            )
            .with_visual_offset(placement.visual_offset),
            has_base_content,
        ))
    }

    /// Select one ruby base or annotation level in its own float context.
    ///
    /// CSS Ruby positions the complete ruby container against parent floats.
    /// Its captured base and annotation levels are then painted inside that
    /// already-positioned atom, so allowing either phase to inherit the
    /// parent's float exclusions would apply the same band a second time.
    /// Floats and positioned descendants authored *inside* ruby are retained
    /// on the generic overlay path before this local sequence is built.
    /// <https://drafts.csswg.org/css-ruby-1/#ruby-layout>
    /// <https://www.w3.org/TR/CSS22/visuren.html#floats>
    fn collect_ruby_level_line_sequence(
        &mut self,
        items: Vec<InlineItem>,
        style: &ComputedStyle,
        available_width: f32,
        padding_left: f32,
        hanging_indent: f32,
    ) -> inline_layout::InlineLineSequence {
        self.with_replay_float_scope(ReplayFloatScope::IsolatedFormattingContext, |layout| {
            layout.collect_inline_line_sequence_with_text_box_trim(
                items,
                style,
                available_width,
                padding_left,
                hanging_indent,
            )
        })
        .with_replay_float_scope(ReplayFloatScope::IsolatedFormattingContext)
    }

    /// Apply the selected `ruby-align` distribution to a level.
    ///
    /// The CSS Ruby UA rule delegates its inner opportunities to
    /// `text-justify: ruby`; this implementation uses the existing
    /// typographic-unit justification path for CJK-wide units, then reserves
    /// half an equal opportunity at each edge of the level.
    /// <https://drafts.csswg.org/css-ruby-1/#ruby-align-property>
    fn distribute_ruby_level_space_around(
        &mut self,
        level: &mut RubyInlineLevel,
        items: &[InlineItem],
        containing_inline_size: f32,
    ) {
        let ruby_align = level.style.ruby_align;
        if matches!(ruby_align, css::RubyAlign::Start | css::RubyAlign::Center) {
            return;
        }
        let natural_inline_size = ruby_line_sequence_inline_size(&level.sequence);
        let Some(unit_count) = ruby_distribution_unit_count(items) else {
            return;
        };
        let free_space = (containing_inline_size - natural_inline_size).max(0.0);
        if free_space <= 0.0 {
            return;
        }
        let core_inline_size = match ruby_align {
            // `space-between` distributes only across interior CJK unit
            // boundaries. A single unit has no opportunity and remains
            // centered by the selected-line alignment geometry.
            css::RubyAlign::SpaceBetween if unit_count > 1 => containing_inline_size,
            css::RubyAlign::SpaceBetween => return,
            // `space-around` has one extra opportunity split across the two
            // edges. With N CJK units, the N equal shares leave N-1 internal
            // gaps and one split edge gap.
            css::RubyAlign::SpaceAround => {
                let per_opportunity = free_space / unit_count as f32;
                (containing_inline_size - per_opportunity).max(natural_inline_size)
            }
            css::RubyAlign::Start | css::RubyAlign::Center => unreachable!(),
        };
        let mut distribution_style = (*level.style).clone();
        distribution_style.text_align = TextAlign::JustifyAll;
        distribution_style.text_justify = TextJustify::InterCharacter;
        level.sequence = self.collect_ruby_level_line_sequence(
            items.to_vec(),
            &distribution_style,
            core_inline_size,
            0.0,
            0.0,
        );
        *level.style = distribution_style;
        level.paint_inline_size = layout_ruby::RubyPaintInlineSpan::new(core_inline_size);
    }
}

pub(super) fn inline_item_has_typographic_content(item: &InlineItem) -> bool {
    match item {
        InlineItem::Word(word) => !word.text.trim().is_empty(),
        InlineItem::Atom(atom) => !atom.content().is_inline_edge(),
        InlineItem::Float(_)
        | InlineItem::Break(_)
        | InlineItem::PageScopeStart(_)
        | InlineItem::PageScopeEnd => false,
    }
}

/// Whether a ruby subtree needs the generic inline scope so its positioned or
/// floated descendants retain their normal containing-block/float ownership.
/// Such descendants are excluded from ruby's anonymous base/annotation box
/// generation and therefore cannot be captured in the coupled paint atom.
pub(super) fn ruby_has_out_of_flow_descendant(children: &[box_tree::FormattingBox<'_>]) -> bool {
    children.iter().any(|child| {
        if let Some((_, _, style, descendants)) = child.element_parts() {
            matches!(style.position, Position::Absolute | Position::Fixed)
                || style.float != Float::None
                || ruby_has_out_of_flow_descendant(descendants)
        } else {
            match child {
                box_tree::FormattingBox::AnonymousBlock(box_) => {
                    ruby_has_out_of_flow_descendant(&box_.children)
                }
                box_tree::FormattingBox::Text(_) => false,
                box_tree::FormattingBox::InlineSplitBlockContext(box_) => {
                    ruby_has_out_of_flow_descendant(&box_.core.children)
                }
                box_tree::FormattingBox::Block(_)
                | box_tree::FormattingBox::Inline(_)
                | box_tree::FormattingBox::AtomicInline(_)
                | box_tree::FormattingBox::Table(_)
                | box_tree::FormattingBox::Flex(_)
                | box_tree::FormattingBox::Replaced(_) => false,
            }
        }
    })
}

/// Clone only the positioned/float branch of a ruby subtree for the generic
/// positioned-inline collector. The ruby formatter consumes in-flow bases and
/// annotations itself, but CSS Ruby does not remove out-of-flow descendants
/// from their normal containing-block and float ownership.
/// <https://drafts.csswg.org/css-ruby-1/#anon-gen-ruby>
pub(super) fn ruby_out_of_flow_overlay<'a>(
    box_: &box_tree::FormattingBox<'a>,
) -> box_tree::FormattingBox<'a> {
    fn has_out_of_flow_style(style: &ComputedStyle) -> bool {
        matches!(style.position, Position::Absolute | Position::Fixed) || style.float != Float::None
    }

    if box_
        .element_parts()
        .is_some_and(|(_, _, style, _)| has_out_of_flow_style(style))
    {
        return box_.clone();
    }

    match box_.clone() {
        box_tree::FormattingBox::Inline(mut box_) => {
            box_.core.children = box_
                .core
                .children
                .iter()
                .filter(|child| ruby_has_out_of_flow_descendant(std::slice::from_ref(*child)))
                .map(ruby_out_of_flow_overlay)
                .collect();
            box_tree::FormattingBox::Inline(box_)
        }
        box_tree::FormattingBox::Block(mut box_) => {
            box_.core.children = box_
                .core
                .children
                .iter()
                .filter(|child| ruby_has_out_of_flow_descendant(std::slice::from_ref(*child)))
                .map(ruby_out_of_flow_overlay)
                .collect();
            box_tree::FormattingBox::Block(box_)
        }
        box_tree::FormattingBox::InlineSplitBlockContext(mut box_) => {
            box_.core.children = box_
                .core
                .children
                .iter()
                .filter(|child| ruby_has_out_of_flow_descendant(std::slice::from_ref(*child)))
                .map(ruby_out_of_flow_overlay)
                .collect();
            box_tree::FormattingBox::InlineSplitBlockContext(box_)
        }
        box_tree::FormattingBox::AnonymousBlock(mut box_) => {
            box_.children = box_
                .children
                .iter()
                .filter(|child| ruby_has_out_of_flow_descendant(std::slice::from_ref(*child)))
                .map(ruby_out_of_flow_overlay)
                .collect();
            box_tree::FormattingBox::AnonymousBlock(box_)
        }
        box_ => box_,
    }
}

/// Materialize `::first-letter` inside the base level of a ruby container.
///
/// The generic graph pass receives a ruby container through transparent inline
/// edges. Preserve the pseudo's tree-abiding ownership at the ruby boundary
/// before its annotation levels are removed from the parent stream.
/// <https://drafts.csswg.org/css-pseudo-4/#first-letter-pseudo>
fn apply_first_letter_style_to_ruby_base_items(
    output: &mut Vec<InlineItem>,
    first_letter_style: &ComputedStyle,
) {
    let Some(index) = output.iter().position(|item| {
        matches!(item, InlineItem::Word(word) if crate::layout::first_letter_byte_range(&word.text).is_some())
    }) else {
        return;
    };
    let InlineItem::Word(word) = &output[index] else {
        unreachable!("the selected ruby first-letter item is a word")
    };
    let range = crate::layout::first_letter_byte_range(&word.text)
        .expect("selected ruby word has a typographic first letter");
    let word = (**word).clone();
    let mut replacement = Vec::with_capacity(3);
    if range.start > 0 {
        let mut prefix = word.clone();
        prefix.text = word.text[..range.start].to_owned();
        replacement.push(InlineItem::Word(Box::new(prefix)));
    }
    let mut letter = word.clone();
    letter.text = word.text[range.clone()].to_owned();
    letter.style = Rc::new(first_letter_style.clone());
    letter.mergeable = false;
    replacement.push(InlineItem::Word(Box::new(letter)));
    if range.end < word.text.len() {
        let mut suffix = word;
        suffix.text = suffix.text[range.end..].to_owned();
        replacement.push(InlineItem::Word(Box::new(suffix)));
    }
    output.splice(index..=index, replacement);
}

fn ruby_line_sequence_inline_size(sequence: &inline_layout::InlineLineSequence) -> f32 {
    sequence
        .records
        .iter()
        .filter_map(|record| record.fragment.as_ref())
        .map(|fragment| fragment.metrics.width)
        .fold(0.0, f32::max)
}

/// Count typographic units eligible for the UA default `text-justify: ruby`
/// behavior. Ruby distributes only CJK-wide units; Latin and Bopomofo content
/// has no ruby justification opportunities and is therefore centered.
fn ruby_distribution_unit_count(items: &[InlineItem]) -> Option<usize> {
    let mut count = 0usize;
    for item in items {
        let InlineItem::Word(word) = item else {
            return None;
        };
        for range in crate::text::CursiveProtectedUnitRanges::new(&word.text) {
            let unit = &word.text[range];
            if !unit
                .chars()
                .filter(|character| {
                    !crate::text::character_is_unicode_mark(*character)
                        && !crate::text::character_is_unicode_control(*character)
                })
                .all(crate::text::character_is_ruby_justification_eligible)
            {
                return None;
            }
            count += 1;
        }
    }
    (count > 1).then_some(count)
}

/// Give all columns of one normalized ruby container a common block-axis
/// metric stack. CSS Ruby places annotation levels across the column group,
/// not independently inside each base. In particular, an anonymous empty
/// base must export the same base baseline as its non-empty siblings.
///
/// This runs after every column has been measured, while the columns are
/// still consecutive source items. The parent opportunity graph therefore
/// retains one base-level participant per column, but its line metrics see a
/// single coupled ruby level stack.
/// <https://drafts.csswg.org/css-ruby-1/#ruby-layout>
fn normalize_ruby_column_group_metrics(
    ruby_atoms: &mut [InlineItem],
    containing_style: &ComputedStyle,
) {
    let mut base_block_size = 0.0f32;
    let mut annotation_block_sizes: Vec<f32> = Vec::new();
    let mut base_baseline = 0.0f32;

    for item in ruby_atoms.iter() {
        let InlineItem::Atom(atom) = item else {
            continue;
        };
        let InlineAtomContent::Ruby {
            base_block_size: column_base_block_size,
            annotation_block_sizes: column_annotation_block_sizes,
            ..
        } = atom.content()
        else {
            continue;
        };
        base_block_size = base_block_size.max(*column_base_block_size);
        for (index, block_size) in column_annotation_block_sizes.iter().enumerate() {
            if annotation_block_sizes.len() <= index {
                annotation_block_sizes.push(0.0);
            }
            annotation_block_sizes[index] = annotation_block_sizes[index].max(*block_size);
        }
        base_baseline = base_baseline.max(
            atom.baseline_offset_from_alignment_source_block_start(
                inline_atom_logical_border_block_size(atom, containing_style),
                containing_style,
            )
            .points()
                - column_annotation_block_sizes.iter().sum::<f32>(),
        );
    }

    let base_metrics = layout_ruby::RubyLevelMetrics {
        before_baseline: layout_ruby::RubyBlockExtent::new(base_baseline),
        after_baseline: layout_ruby::RubyBlockExtent::new(
            (base_block_size - base_baseline).max(0.0),
        ),
        baseline: layout_ruby::RubyBaselineOffset::new(base_baseline),
    };
    let annotation_levels = annotation_block_sizes
        .iter()
        .copied()
        .map(|block_extent| layout_ruby::RubyLevelMetrics {
            // Annotation sequences are replayed from their own line-box
            // baseline. Their group metric records the level extent here;
            // paint applies that local baseline exactly once.
            before_baseline: layout_ruby::RubyBlockExtent::default(),
            after_baseline: layout_ruby::RubyBlockExtent::new(block_extent),
            baseline: layout_ruby::RubyBaselineOffset::default(),
        })
        .collect::<Vec<_>>();
    let annotations_block_extent = annotation_levels
        .iter()
        .map(|level| level.block_extent().points())
        .sum::<f32>();
    let metrics = layout_ruby::RubyColumnGroupMetrics {
        base: base_metrics,
        annotation_levels,
        exported_baseline: layout_ruby::RubyBaselineOffset::new(
            annotations_block_extent + base_metrics.baseline.points(),
        ),
    };
    let group_block_size = metrics.base.block_extent().points() + annotations_block_extent;
    for item in ruby_atoms {
        let InlineItem::Atom(atom) = item else {
            continue;
        };
        let content = Rc::make_mut(&mut atom.data);
        let InlineAtomContent::Ruby {
            base_block_size: column_base_block_size,
            annotation_block_sizes: column_annotation_block_sizes,
            ..
        } = &mut content.content
        else {
            continue;
        };
        *column_base_block_size = metrics.base.block_extent().points();
        *column_annotation_block_sizes = metrics
            .annotation_levels
            .iter()
            .map(|level| level.block_extent().points())
            .collect();
        atom.size.height = group_block_size;
        atom.baseline = InlineAtomBaseline::Exported {
            source: InlineAtomBaselineSource::BorderBox,
            offset_from_source_box_block_start: atomic_inline_baseline_source_pt(
                metrics.exported_baseline.points(),
            ),
        };
    }
}

/// Assign the combined base-column width to annotations that begin a ruby
/// span. The parent graph retains separate base advances, while the sidecar
/// paints once from the first covered column across the complete paired range.
/// <https://drafts.csswg.org/css-ruby-1/#ruby-annotation-pairing>
fn normalize_ruby_annotation_span_inline_sizes(
    ruby_atoms: &mut [InlineItem],
    containing_style: &ComputedStyle,
) {
    let column_inline_sizes = ruby_atoms
        .iter()
        .filter_map(|item| {
            let InlineItem::Atom(atom) = item else {
                return None;
            };
            matches!(atom.content(), InlineAtomContent::Ruby { .. })
                .then(|| inline_atom_logical_border_inline_size(atom, containing_style))
        })
        .collect::<Vec<_>>();

    for (column_index, item) in ruby_atoms.iter_mut().enumerate() {
        let InlineItem::Atom(atom) = item else {
            continue;
        };
        let content = Rc::make_mut(&mut atom.data);
        let InlineAtomContent::Ruby { annotations, .. } = &mut content.content else {
            continue;
        };
        for annotation in annotations {
            if annotation.starts_span && annotation.column_span > 1 {
                annotation.containing_inline_size = layout_ruby::RubyColumnInlineSpan::new(
                    column_inline_sizes[column_index..column_index + annotation.column_span]
                        .iter()
                        .sum(),
                );
            }
        }
    }
}

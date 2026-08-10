use super::*;
/// Return an atomic inline's logical inline-size in the containing line.
///
/// CSS Writing Modes maps inline-level layout to logical axes before painting
/// physical boxes. Atomic inline boxes keep physical dimensions internally,
/// so line measurement must remap them through the parent writing mode:
/// <https://www.w3.org/TR/css-writing-modes-4/#abstract-box> and
/// <https://www.w3.org/TR/css-inline-3/#atomic-inline>.
pub(in crate::layout) fn inline_atom_logical_inline_size(
    atom: &InlineAtom,
    containing_style: &ComputedStyle,
) -> f32 {
    if let InlineAtomContent::InlineEdge(InlineEdgeRole::BoxEdge(edge)) = atom.content() {
        // Box-edge atoms retain a physical line-height so they can contribute
        // to the line box and paint their decoration. Their `advance`,
        // however, is already in the containing line's logical inline
        // coordinate. Projecting the physical atom height again in vertical
        // writing modes would turn a zero-width lexical scope marker into a
        // one-line advance.
        // <https://www.w3.org/TR/css-inline-3/#inline-boxes> and
        // <https://www.w3.org/TR/css-writing-modes-4/#abstract-box>
        return edge.advance;
    }
    if let InlineAtomContent::InlineEdge(InlineEdgeRole::TextAutospace(spacing)) = atom.content() {
        // Text autospace is already a logical boundary advance. Its physical
        // carrier has a zero block size, so projecting that carrier through a
        // vertical writing mode would incorrectly erase the selected `1/8ic`.
        return spacing.advance().points();
    }
    match containing_style.writing_mode {
        WritingMode::HorizontalTb => atom.size.width,
        WritingMode::VerticalRl
        | WritingMode::VerticalLr
        | WritingMode::SidewaysRl
        | WritingMode::SidewaysLr => atom.size.height,
    }
}

/// Return an atomic inline's logical block-size in the containing line.
///
/// Atomic inline boxes are stored as physical margin boxes, but line box
/// ascent/descent calculations use the logical block axis selected by the
/// parent inline formatting context:
/// <https://www.w3.org/TR/css-writing-modes-4/#abstract-box> and
/// <https://www.w3.org/TR/css-inline-3/#line-box>.
pub(in crate::layout) fn inline_atom_logical_block_size(
    atom: &InlineAtom,
    containing_style: &ComputedStyle,
) -> f32 {
    match containing_style.writing_mode {
        WritingMode::HorizontalTb => atom.size.height,
        WritingMode::VerticalRl
        | WritingMode::VerticalLr
        | WritingMode::SidewaysRl
        | WritingMode::SidewaysLr => atom.size.width,
    }
}

pub(in crate::layout) fn inline_atom_logical_inline_start_margin(
    atom: &InlineAtom,
    containing_style: &ComputedStyle,
) -> f32 {
    inline_atom_margin_for_side(
        atom,
        inline_start_side(
            containing_style.writing_mode,
            containing_style.used_direction(),
        ),
    )
}

pub(in crate::layout) fn inline_atom_logical_inline_end_margin(
    atom: &InlineAtom,
    containing_style: &ComputedStyle,
) -> f32 {
    inline_atom_margin_for_side(
        atom,
        inline_end_side(
            containing_style.writing_mode,
            containing_style.used_direction(),
        ),
    )
}

pub(in crate::layout) fn inline_atom_logical_block_start_margin(
    atom: &InlineAtom,
    containing_style: &ComputedStyle,
) -> f32 {
    inline_atom_margin_for_side(atom, block_start_side(containing_style.writing_mode))
}

pub(in crate::layout) fn inline_atom_logical_block_end_margin(
    atom: &InlineAtom,
    containing_style: &ComputedStyle,
) -> f32 {
    inline_atom_margin_for_side(atom, block_end_side(containing_style.writing_mode))
}

/// Return the block-start margin by which a baseline-participating atom moves
/// its containing line's paint anchor.
///
/// An ordinary atomic inline contributes its margin box to the line's
/// baseline metrics, but its captured content is replayed from the border-box
/// origin. Keeping that conversion at the atom alone leaves the line anchor
/// one margin too far toward block-end. Line-relative `vertical-align` values
/// instead align their margin boxes directly to the resolved line box, and an
/// `inline-table` exports a table-box baseline whose captured wrapper already
/// owns this margin. Neither participates in this anchor adjustment:
/// <https://drafts.csswg.org/css-inline-3/#line-layout>
/// <https://www.w3.org/TR/CSS22/tables.html#table-display>
pub(in crate::layout) fn inline_atom_line_anchor_block_start_margin(
    atom: &InlineAtom,
    containing_style: &ComputedStyle,
) -> f32 {
    if atom
        .style()
        .vertical_align
        .clone()
        .has_line_relative_baseline_shift()
        || matches!(atom.baseline, InlineAtomBaseline::ExportedTableBox { .. })
        || matches!(atom.content(), InlineAtomContent::InlineEdge(_))
    {
        0.0
    } else {
        inline_atom_logical_block_start_margin(atom, containing_style)
    }
}

pub(in crate::layout) fn inline_atom_logical_border_inline_size(
    atom: &InlineAtom,
    containing_style: &ComputedStyle,
) -> f32 {
    (inline_atom_logical_inline_size(atom, containing_style)
        - inline_atom_logical_inline_start_margin(atom, containing_style)
        - inline_atom_logical_inline_end_margin(atom, containing_style))
    .max(0.0)
}

pub(in crate::layout) fn inline_atom_logical_border_block_size(
    atom: &InlineAtom,
    containing_style: &ComputedStyle,
) -> f32 {
    (inline_atom_logical_block_size(atom, containing_style)
        - inline_atom_logical_block_start_margin(atom, containing_style)
        - inline_atom_logical_block_end_margin(atom, containing_style))
    .max(0.0)
}

/// Return an atomic inline's baseline offset from its logical margin-box
/// block-start edge in the containing line.
///
/// CSS Inline aligns atomic inline boxes by their exported baseline when one
/// exists, otherwise by a synthesized border-box block-end baseline. Margins
/// are outside the ordinary principal box but are part of the line
/// participant. `inline-table` exports from its table box instead of its
/// wrapper, so the atom carries that exceptional reference explicitly. This
/// is the single place that resolves either reference into line coordinates:
/// <https://www.w3.org/TR/CSS22/tables.html#table-display>
/// <https://drafts.csswg.org/css-inline-3/#inline-block-baseline>.
pub(in crate::layout) fn inline_atom_logical_margin_box_baseline_offset(
    atom: &InlineAtom,
    containing_style: &ComputedStyle,
) -> f32 {
    let border_box_block_size = inline_atom_logical_border_block_size(atom, containing_style);
    atom.baseline_offset_from_margin_box_block_start(
        border_box_block_size,
        inline_atom_logical_block_start_margin(atom, containing_style),
        containing_style,
    )
}

/// Return the baseline coordinate used to place an atom's border-box content.
///
/// The line-layout baseline contribution for an `inline-table` comes from its
/// table box, not its wrapper. Its captured fragment retains the wrapper
/// margins for paint replay, so placement must not add the block-start margin
/// a second time. Ordinary atomic inlines, whose captured content excludes
/// their outer margins, continue to use their margin-box baseline.
/// <https://www.w3.org/TR/CSS22/tables.html#table-display>
pub(in crate::layout) fn inline_atom_logical_content_placement_baseline_offset(
    atom: &InlineAtom,
    containing_style: &ComputedStyle,
) -> f32 {
    let baseline = atom.baseline_offset_from_border_box_block_start(
        inline_atom_logical_border_block_size(atom, containing_style),
        containing_style,
    );
    match atom.baseline {
        InlineAtomBaseline::ExportedTableBox { .. } => baseline,
        InlineAtomBaseline::Exported { .. }
        | InlineAtomBaseline::FlexExported { .. }
        | InlineAtomBaseline::SynthesizedBorderBoxBlockEnd => {
            inline_atom_logical_block_start_margin(atom, containing_style) + baseline
        }
    }
}

/// Return a line item's logical block-size in its containing line.
///
/// Text fragments expose `line-height` in the line block axis. Atomic inline
/// boxes expose their physical margin boxes, which must be converted to the
/// parent logical block axis before line metrics are resolved:
/// <https://www.w3.org/TR/css-inline-3/#line-box>.
pub(in crate::layout) fn inline_line_item_logical_block_size(
    item: &InlineLineItem,
    containing_style: &ComputedStyle,
) -> f32 {
    match item {
        InlineLineItem::Fragment(fragment) => fragment.style().line_height,
        InlineLineItem::Atom(atom)
            if matches!(
                atom.content(),
                InlineAtomContent::InlineEdge(InlineEdgeRole::BoxEdge(_))
            ) =>
        {
            atom.style().line_height
        }
        InlineLineItem::Atom(atom) => inline_atom_logical_block_size(atom, containing_style),
        InlineLineItem::Float(_) => 0.0,
    }
}

pub(in crate::layout) fn inline_atom_margin_for_side(atom: &InlineAtom, side: PhysicalSide) -> f32 {
    match side {
        PhysicalSide::Top => atom.style().margin.top,
        PhysicalSide::Right => atom.style().margin.right,
        PhysicalSide::Bottom => atom.style().margin.bottom,
        PhysicalSide::Left => atom.style().margin.left,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tcy_raw_composition_width_does_not_escape_its_one_em_layout_footprint() {
        let mut vertical_style = ComputedStyle::initial();
        vertical_style.font_size = 60.0;
        let mut horizontal_style = vertical_style.clone();
        horizontal_style.writing_mode = WritingMode::HorizontalTb;
        let composition = inline_layout::InlineLineSequence {
            // The nested horizontal sequence remains 3em wide for paint and
            // geometric compression, but it is not its parent's layout size.
            available_width: 180.0,
            ..inline_layout::InlineLineSequence::default()
        };
        let atom = InlineAtom::new(
            InlineAtomContent::TextCombineUpright {
                sequence: composition,
                horizontal_style: Box::new(horizontal_style),
                inline_scale: 1.0 / 3.0,
            },
            vertical_style.clone(),
            None,
            InlineSize::new(60.0, 60.0),
            30.0,
            0.0,
            None,
            None,
        );

        for writing_mode in [WritingMode::VerticalRl, WritingMode::VerticalLr] {
            vertical_style.writing_mode = writing_mode;
            assert_eq!(
                inline_atom_logical_inline_size(&atom, &vertical_style),
                60.0,
                "{writing_mode:?} must expose TCY's measured one-em square to parent layout"
            );
        }
    }
}

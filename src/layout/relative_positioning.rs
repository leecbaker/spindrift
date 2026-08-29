use super::*;

#[derive(Debug, Clone, Copy)]
pub(in crate::layout) struct RelativeOffset {
    pub(in crate::layout) vector: ContainerVector,
}

impl RelativeOffset {
    pub(in crate::layout) fn zero() -> Self {
        Self {
            vector: ContainerVector::zero(),
        }
    }

    pub(in crate::layout) fn x(self) -> f32 {
        self.vector.x
    }

    pub(in crate::layout) fn y(self) -> f32 {
        self.vector.y
    }

    /// Whether a relative-position translation has no observable visual
    /// effect. This keeps the decision at the relative-positioning boundary,
    /// rather than comparing generic coordinates at each paint caller.
    /// <https://drafts.csswg.org/css-position-3/#relative-positioning>
    pub(in crate::layout) fn is_zero(self) -> bool {
        self.x().abs() <= 0.01 && self.y().abs() <= 0.01
    }
}

pub(in crate::layout) fn relative_position_offset(
    style: &ComputedStyle,
    containing_block: ContainingBlock,
) -> RelativeOffset {
    if !matches!(style.position, Position::Relative | Position::Sticky) {
        return RelativeOffset::zero();
    }
    let left = used_inset_left(style, containing_block);
    let right = used_inset_right(style, containing_block);
    let top = used_inset_top(style, containing_block);
    let bottom = used_inset_bottom(style, containing_block);
    RelativeOffset {
        vector: ContainerVector::new(
            left.unwrap_or_else(|| -right.unwrap_or(0.0)),
            bottom.unwrap_or_else(|| -top.unwrap_or(0.0)),
        ),
    }
}

/// Resolve a relative-position translation from explicit percentage bases.
///
/// Table tracks have a final used block size even where their parent table
/// part's `height` remains indefinite. Keeping the percentage basis explicit
/// prevents that used geometry from accidentally resolving a percentage inset:
/// <https://drafts.csswg.org/css-position-3/#relative-positioning> and
/// <https://drafts.csswg.org/css-sizing-3/#definite>.
pub(in crate::layout) fn relative_position_offset_with_bases(
    style: &ComputedStyle,
    inline_basis: PercentageBasis<ContentBoxLength>,
    block_basis: PercentageBasis<ContentBoxLength>,
) -> RelativeOffset {
    if !matches!(style.position, Position::Relative | Position::Sticky) {
        return RelativeOffset::zero();
    }
    let left = used_length_percentage_or_auto_with_basis(
        style.box_values.inset_left.clone(),
        inline_basis,
    )
    .map(|length| length.points());
    let right = used_length_percentage_or_auto_with_basis(
        style.box_values.inset_right.clone(),
        inline_basis,
    )
    .map(|length| length.points());
    let top =
        used_length_percentage_or_auto_with_basis(style.box_values.inset_top.clone(), block_basis)
            .map(|length| length.points());
    let bottom = used_length_percentage_or_auto_with_basis(
        style.box_values.inset_bottom.clone(),
        block_basis,
    )
    .map(|length| length.points());
    // CSS 2.1 9.4.3/9.3.2: relative positioning offsets the visual box while
    // preserving its normal-flow space. Opposing insets over-constrain the axis;
    // for left-to-right content, `left` wins horizontally, and `top` wins
    // vertically.
    let x = left.unwrap_or_else(|| -right.unwrap_or(0.0));
    let y = bottom.unwrap_or_else(|| -top.unwrap_or(0.0));
    RelativeOffset {
        vector: ContainerVector::new(x, y),
    }
}

impl<'a> LayoutBuilder<'a> {
    /// Resolve a normal-flow box's relative-positioning offset.
    ///
    /// A flex or grid item's replayed formatting context has an already used
    /// physical content box. Descendants in normal flow use that box for their
    /// relative-position percentage bases, without treating it as an absolute
    /// positioning containing block:
    /// <https://www.w3.org/TR/css-position-3/#relative-positioning>.
    pub(in crate::layout) fn normal_flow_relative_position_offset(
        &self,
        style: &ComputedStyle,
    ) -> RelativeOffset {
        let Some(containing_block) = self.normal_flow_relative_containing_blocks.last() else {
            return relative_position_offset(style, self.current_containing_block());
        };
        if !matches!(style.position, Position::Relative | Position::Sticky) {
            return RelativeOffset::zero();
        }

        let width_basis =
            PercentageBasis::definite(containing_block.physical_content_width.content_box_length());
        let height_basis = containing_block
            .physical_content_height
            .map(|height| PercentageBasis::definite(height.content_box_length()))
            .unwrap_or_else(PercentageBasis::indefinite);
        let left = used_length_percentage_or_auto_with_basis(
            style.box_values.inset_left.clone(),
            width_basis,
        )
        .map(|length| length.points());
        let right = used_length_percentage_or_auto_with_basis(
            style.box_values.inset_right.clone(),
            width_basis,
        )
        .map(|length| length.points());
        let top = used_length_percentage_or_auto_with_basis(
            style.box_values.inset_top.clone(),
            height_basis,
        )
        .map(|length| length.points());
        let bottom = used_length_percentage_or_auto_with_basis(
            style.box_values.inset_bottom.clone(),
            height_basis,
        )
        .map(|length| length.points());
        RelativeOffset {
            vector: ContainerVector::new(
                left.unwrap_or_else(|| -right.unwrap_or(0.0)),
                bottom.unwrap_or_else(|| -top.unwrap_or(0.0)),
            ),
        }
    }
}

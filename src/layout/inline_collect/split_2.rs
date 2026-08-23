use super::*;

mod generated_content;
pub(in crate::layout) use generated_content::FrozenInlineReplayInput;
mod split_1;
mod split_3;
mod split_4;

/// The physical block-axis extent needed to capture a block-level
/// static-position source from an inline collection.
///
/// In a vertical containing flow, physical width is the source's logical
/// block-size.  The line-selection marker has no such extent of its own, so
/// carrying the real hypothetical margin-box extent prevents it from being
/// replaced by an unrelated line-height approximation.
/// <https://drafts.csswg.org/css-position-3/#staticpos-rect>
/// <https://www.w3.org/TR/css-writing-modes-4/#logical-to-physical>
#[derive(Debug, Clone, Copy)]
pub(in crate::layout) enum BlockStaticPositionPlaceholderGeometry {
    Horizontal,
    Vertical {
        physical_margin_box_block_extent: MarginBoxLength,
    },
}

impl BlockStaticPositionPlaceholderGeometry {
    /// Recover the vertical source's physical margin-box span from the
    /// zero-footprint line marker at its logical block-end.
    ///
    /// The marker is an edge, not the source box.  `vertical-lr` advances its
    /// block axis to the physical right, so its block-start lies one measured
    /// margin-box extent to the marker's left.  `vertical-rl` advances to the
    /// physical left, so the same marker is the span's left edge.
    fn vertical_margin_box_inline_span_from_block_end_marker(
        self,
        marker_block_end_x: f32,
        writing_mode: WritingMode,
    ) -> PageInlineSpan {
        let Self::Vertical {
            physical_margin_box_block_extent,
            ..
        } = self
        else {
            unreachable!(
                "a vertical block static-position marker requires a measured physical block extent"
            );
        };
        let width = physical_margin_box_block_extent.points();
        let left = match writing_mode {
            WritingMode::VerticalLr | WritingMode::SidewaysLr => marker_block_end_x - width,
            WritingMode::VerticalRl | WritingMode::SidewaysRl => marker_block_end_x,
            WritingMode::HorizontalTb => {
                unreachable!("a horizontal block static-position marker has no vertical span")
            }
        };
        PageInlineSpan::new(left, width)
    }
}

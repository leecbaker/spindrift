use super::*;

/// Intrinsic item measurements carried from a formatting algorithm's content
/// probe into its layout-engine adapter.
///
/// The caller defines whether the width/height pair is logical or physical.
/// Keeping that coordinate-system choice outside this shared transport type
/// lets Flexbox and Grid preserve their distinct Writing Modes boundaries.
/// <https://www.w3.org/TR/css-sizing-3/#intrinsic> and
/// <https://www.w3.org/TR/css-writing-modes-4/#abstract-box>.
#[derive(Debug, Clone, Copy)]
pub(in crate::layout) struct IntrinsicItemMetrics {
    pub(in crate::layout) width: ContentBoxLength,
    pub(in crate::layout) height: ContentBoxLength,
    pub(in crate::layout) min_width: ContentBoxLength,
    pub(in crate::layout) min_height: ContentBoxLength,
    pub(in crate::layout) content_width: ContentBoxLength,
    pub(in crate::layout) content_height: ContentBoxLength,
    pub(in crate::layout) preferred_aspect_ratio: Option<f32>,
    pub(in crate::layout) first_baseline: Option<f32>,
    pub(in crate::layout) last_baseline: Option<f32>,
}

impl IntrinsicItemMetrics {
    pub(in crate::layout) fn fixed(width: f32, height: f32) -> Self {
        let width = content_box_pt(width);
        let height = content_box_pt(height);
        Self {
            width,
            height,
            min_width: width,
            min_height: height,
            content_width: width,
            content_height: height,
            preferred_aspect_ratio: None,
            first_baseline: None,
            last_baseline: None,
        }
    }

    pub(in crate::layout) fn zero() -> Self {
        Self::fixed(0.0, 0.0)
    }

    /// Suppress exported block-axis baselines for a layout-contained item.
    ///
    /// CSS Containment prevents a layout-contained principal box from
    /// participating in baseline alignment.
    /// <https://www.w3.org/TR/css-contain-1/#containment-layout>.
    pub(in crate::layout) fn clear_block_baselines(&mut self) {
        self.first_baseline = None;
        self.last_baseline = None;
    }

    /// Swap the carried width/height pairs without changing aspect-ratio
    /// semantics, which remain owned by the caller's CSS adapter.
    pub(in crate::layout) fn swapped_axes(self) -> Self {
        Self {
            width: self.height,
            height: self.width,
            min_width: self.min_height,
            min_height: self.min_width,
            content_width: self.content_height,
            content_height: self.content_width,
            ..self
        }
    }
}

/// Resolve Taffy's leaf size from definite dimensions, a preferred aspect
/// ratio, and mode-specific preferred dimensions.
///
/// A definite dimension wins. Otherwise a definite opposite dimension may
/// transfer through the preferred ratio; finally the layout mode's own
/// preferred size is used.
/// <https://www.w3.org/TR/css-sizing-4/#aspect-ratio>.
pub(in crate::layout) fn measure_intrinsic_item_leaf(
    known_dimensions: taffy_layout::Size<Option<f32>>,
    preferred_aspect_ratio: Option<f32>,
    preferred_dimensions: taffy_layout::Size<f32>,
) -> taffy_layout::Size<f32> {
    let preferred_aspect_ratio = preferred_aspect_ratio.filter(|ratio| *ratio > 0.0);
    let width = known_dimensions
        .width
        .or_else(|| {
            preferred_aspect_ratio
                .and_then(|ratio| known_dimensions.height.map(|height| height * ratio))
        })
        .unwrap_or(preferred_dimensions.width)
        .max(0.0);
    let height = known_dimensions
        .height
        .or_else(|| {
            preferred_aspect_ratio
                .and_then(|ratio| known_dimensions.width.map(|width| width / ratio))
        })
        .unwrap_or(preferred_dimensions.height)
        .max(0.0);
    taffy_layout::Size { width, height }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn metrics_fixed_and_zero_initializers_preserve_all_dimensions() {
        let fixed = IntrinsicItemMetrics::fixed(12.0, 18.0);
        assert_eq!(fixed.width, content_box_pt(12.0));
        assert_eq!(fixed.min_width, content_box_pt(12.0));
        assert_eq!(fixed.content_height, content_box_pt(18.0));
        assert_eq!(IntrinsicItemMetrics::zero().height, content_box_pt(0.0));
    }

    #[test]
    fn metrics_clear_block_baselines_and_swap_dimension_pairs() {
        let mut metrics = IntrinsicItemMetrics {
            width: content_box_pt(10.0),
            height: content_box_pt(20.0),
            min_width: content_box_pt(3.0),
            min_height: content_box_pt(4.0),
            content_width: content_box_pt(30.0),
            content_height: content_box_pt(40.0),
            preferred_aspect_ratio: Some(2.0),
            first_baseline: Some(5.0),
            last_baseline: Some(6.0),
        };
        metrics.clear_block_baselines();
        let swapped = metrics.swapped_axes();

        assert_eq!(swapped.width, content_box_pt(20.0));
        assert_eq!(swapped.min_width, content_box_pt(4.0));
        assert_eq!(swapped.content_height, content_box_pt(30.0));
        assert_eq!(swapped.preferred_aspect_ratio, Some(2.0));
        assert_eq!(swapped.first_baseline, None);
        assert_eq!(swapped.last_baseline, None);
    }

    #[test]
    fn leaf_measurement_prefers_known_dimensions_and_transfers_ratio() {
        let preferred = taffy_layout::Size {
            width: 10.0,
            height: 20.0,
        };
        assert_eq!(
            measure_intrinsic_item_leaf(
                taffy_layout::Size {
                    width: Some(30.0),
                    height: Some(40.0),
                },
                Some(2.0),
                preferred,
            ),
            taffy_layout::Size {
                width: 30.0,
                height: 40.0,
            }
        );
        assert_eq!(
            measure_intrinsic_item_leaf(
                taffy_layout::Size {
                    width: None,
                    height: Some(12.0),
                },
                Some(2.0),
                preferred,
            ),
            taffy_layout::Size {
                width: 24.0,
                height: 12.0,
            }
        );
        assert_eq!(
            measure_intrinsic_item_leaf(
                taffy_layout::Size {
                    width: Some(24.0),
                    height: None,
                },
                Some(2.0),
                preferred,
            ),
            taffy_layout::Size {
                width: 24.0,
                height: 12.0,
            }
        );
    }

    #[test]
    fn leaf_measurement_uses_preferred_dimensions_for_invalid_ratios() {
        let preferred = taffy_layout::Size {
            width: 10.0,
            height: 20.0,
        };
        for ratio in [Some(0.0), Some(-1.0), Some(f32::NAN), None] {
            assert_eq!(
                measure_intrinsic_item_leaf(
                    taffy_layout::Size {
                        width: None,
                        height: Some(12.0),
                    },
                    ratio,
                    preferred,
                ),
                taffy_layout::Size {
                    width: 10.0,
                    height: 12.0,
                }
            );
        }
    }
}

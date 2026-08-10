//! CSS-independent retained contours for clipping a box's contents.
//!
//! A box clip always retains a conservative rectangle separately from its
//! exact edge.  The rectangle is useful for culling, links, and fragmented
//! paint bookkeeping; the contour is the edge that PDF must actually clip.

use super::geometry::{PaintClip, PaintTranslation};
use super::paths::{RenderedPathClip, RenderedPathFillRule};
use super::shapes::RenderedRoundedRect;

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ResolvedBoxContentClip {
    pub(crate) bounds: PaintClip,
    pub(crate) contour: BoxContentContour,
}

impl ResolvedBoxContentClip {
    pub(crate) fn translated(mut self, offset: PaintTranslation) -> Self {
        self.bounds = self.bounds.translated(offset);
        self.contour = self.contour.translated(offset);
        self
    }

    /// Materialize this contour as a path when an atomic image or SVG needs
    /// to carry the clip directly.  Effect scopes keep rounded contours in
    /// their compact form; this adapter is only a paint-backend boundary.
    pub(crate) fn path_clip(&self) -> Option<RenderedPathClip> {
        match &self.contour {
            BoxContentContour::Rect => Some(RenderedPathClip::new(
                super::paths::paint_rect_path_commands(self.bounds.paint_rect()),
                RenderedPathFillRule::NonZero,
                Vec::new(),
            )),
            BoxContentContour::Rounded(rounded) => Some(RenderedPathClip::new(
                crate::layout::shaped_rect_path_commands(
                    rounded.paint_rect(),
                    rounded.radii,
                    rounded.corner_shapes,
                ),
                RenderedPathFillRule::NonZero,
                Vec::new(),
            )),
            BoxContentContour::Path(path) => Some(path.clone()),
            BoxContentContour::Empty => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum BoxContentContour {
    Rect,
    Rounded(RenderedRoundedRect),
    Path(RenderedPathClip),
    /// A collapsed CSS contour.  This is intentionally distinct from no
    /// contour: it suppresses all contained paint.
    Empty,
}

impl BoxContentContour {
    fn translated(self, offset: PaintTranslation) -> Self {
        match self {
            Self::Rect => Self::Rect,
            Self::Rounded(rounded) => Self::Rounded(rounded.translated(offset)),
            Self::Path(path) => Self::Path(path.translated(offset)),
            Self::Empty => Self::Empty,
        }
    }
}

/// The distinct kinds of CSS overflow clipping retained by a paint effect.
///
/// Rectangular and axis-selective clips remain separate because they express
/// overflow-axis semantics.  A `Contoured` clip combines its conservative
/// bounds and its exact CSS box edge as one logical effect.
#[allow(dead_code)] // formatter migration consumes this as legacy fields retire
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum OverflowClipEffect {
    Rect(PaintClip),
    AxisSelective(super::geometry::AxisSelectivePaintClip),
    Union(super::geometry::PaintClipUnion),
    Contoured(ResolvedBoxContentClip),
}

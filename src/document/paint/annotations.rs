use std::rc::Rc;

use super::geometry::{PaintClip, PaintRect, PaintTransform, PaintTranslation};

/// A resolved document link annotation in page-local PDF-point coordinates.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct LinkAnnotation {
    pub(crate) rect: PaintRect,
    pub(crate) target: Rc<str>,
}

impl LinkAnnotation {
    pub(crate) fn from_paint_rect(rect: PaintRect, target: impl Into<Rc<str>>) -> Self {
        Self {
            rect,
            target: target.into(),
        }
    }

    /// Returns the link rectangle's horizontal position in PDF points.
    pub(crate) fn x(&self) -> f32 {
        self.rect.origin.x
    }

    /// Returns the link rectangle's vertical position in PDF points.
    pub(crate) fn y(&self) -> f32 {
        self.rect.origin.y
    }

    /// Returns the link rectangle's width in PDF points.
    pub(crate) fn width(&self) -> f32 {
        self.rect.size.width
    }

    /// Returns the link rectangle's height in PDF points.
    pub(crate) fn height(&self) -> f32 {
        self.rect.size.height
    }

    pub(crate) fn paint_rect(&self) -> PaintRect {
        self.rect
    }

    pub(crate) fn translated(mut self, offset: PaintTranslation) -> Self {
        self.rect = offset.transform_rect(&self.rect);
        self
    }

    pub(in crate::document) fn transformed(&self, transform: PaintTransform) -> Self {
        let clip = transform.apply_clip_to_aabb(PaintClip::from_paint_rect(self.rect));
        Self::from_paint_rect(clip.paint_rect(), Rc::clone(&self.target))
    }

    /// Restrict an annotation to visible page-local paint before it is handed
    /// to the PDF backend. Scene-plane fragments may replay one source link
    /// under several disjoint clips; each retained fragment must contribute
    /// only its visible rectangle.
    pub(in crate::document) fn clipped_to(self, clip: PaintClip) -> Option<Self> {
        let clipped = PaintClip::from_paint_rect(self.rect).intersect(clip)?;
        Some(Self::from_paint_rect(clipped.paint_rect(), self.target))
    }
}

pub(crate) type RenderedLink = LinkAnnotation;

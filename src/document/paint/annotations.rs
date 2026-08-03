use std::rc::Rc;

use super::geometry::{PaintClip, PaintRect, PaintTransform, PaintTranslation};

/// A resolved document link annotation in page-local PDF-point coordinates.
#[derive(Debug, Clone, PartialEq)]
pub struct LinkAnnotation {
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
    ///
    /// ```no_run
    /// # fn inspect(link: &quire::LinkAnnotation) {
    /// let x = link.x();
    /// # let _ = x;
    /// # }
    /// ```
    pub fn x(&self) -> f32 {
        self.rect.origin.x
    }

    /// Returns the link rectangle's vertical position in PDF points.
    ///
    /// ```no_run
    /// # fn inspect(link: &quire::LinkAnnotation) {
    /// let y = link.y();
    /// # let _ = y;
    /// # }
    /// ```
    pub fn y(&self) -> f32 {
        self.rect.origin.y
    }

    /// Returns the link rectangle's width in PDF points.
    ///
    /// ```no_run
    /// # fn inspect(link: &quire::LinkAnnotation) {
    /// let width = link.width();
    /// # let _ = width;
    /// # }
    /// ```
    pub fn width(&self) -> f32 {
        self.rect.size.width
    }

    /// Returns the link rectangle's height in PDF points.
    ///
    /// ```no_run
    /// # fn inspect(link: &quire::LinkAnnotation) {
    /// let height = link.height();
    /// # let _ = height;
    /// # }
    /// ```
    pub fn height(&self) -> f32 {
        self.rect.size.height
    }

    /// Returns the resolved external URL or internal fragment target.
    ///
    /// ```no_run
    /// # fn inspect(link: &quire::LinkAnnotation) {
    /// assert!(!link.target().is_empty());
    /// # }
    /// ```
    pub fn target(&self) -> &str {
        &self.target
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
}

pub(crate) type RenderedLink = LinkAnnotation;

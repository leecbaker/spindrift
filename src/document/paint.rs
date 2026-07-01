use super::Page;
use crate::{Color, Error, Result};
use std::borrow::Cow;

/// Page-local paint coordinates before PDF serialization.
///
/// Paint primitives are expressed with a bottom-left origin and an upward
/// `y` axis. That matches PDF default user space for unrotated pages, but this
/// marker keeps the CSS painting model boundary distinct from final PDF
/// serialization and future page-rotation or form-XObject coordinate changes:
/// <https://www.w3.org/TR/css2/visuren.html#painting-order> and
/// ISO 32000-2:2020, 8.3 "Coordinate Systems".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PaintSpace {}

/// PDF default user-space coordinates.
///
/// For current unrotated page content streams this is numerically identical to
/// [`PaintSpace`]. Keeping a separate marker documents the serialization
/// boundary where page boxes, page rotation, and PDF form coordinate systems
/// would be applied:
/// ISO 32000-2:2020, 8.3 "Coordinate Systems".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PdfUserSpace {}

/// A point in page-local paint coordinates.
pub type PaintPoint = euclid::Point2D<f32, PaintSpace>;
/// A page-local paint-space translation vector.
///
/// This represents movement within the already resolved CSS painting plane:
/// `x` moves right and `y` moves upward from the physical bottom-left of the
/// page. Use this for translating captured paint fragments and display-list
/// primitives; layout top-edge coordinates should be converted before creating
/// a paint-space vector:
/// <https://www.w3.org/TR/css2/visuren.html#painting-order>.
pub(crate) type PaintVector = euclid::Vector2D<f32, PaintSpace>;
/// A size in page-local paint coordinates.
pub type PaintSize = euclid::Size2D<f32, PaintSpace>;
/// A bottom-left-origin rectangle in page-local paint coordinates.
pub type PaintRect = euclid::Rect<f32, PaintSpace>;
/// A point in PDF user space.
pub(crate) type PdfPoint = euclid::Point2D<f32, PdfUserSpace>;
/// A size in PDF user space.
pub(crate) type PdfSize = euclid::Size2D<f32, PdfUserSpace>;
/// A bottom-left-origin rectangle in PDF user space.
pub(crate) type PdfRect = euclid::Rect<f32, PdfUserSpace>;

/// Convert a paint-space rectangle into current PDF user-space coordinates.
///
/// This is intentionally identity today. It names the final boundary so future
/// page rotation, crop-box, or form-XObject conversions do not get embedded in
/// individual PDF drawing routines.
pub(crate) fn paint_rect_to_pdf(rect: PaintRect) -> PdfRect {
    PdfRect::new(
        PdfPoint::new(rect.origin.x, rect.origin.y),
        PdfSize::new(rect.size.width, rect.size.height),
    )
}

/// Convert a paint-space point into current PDF user-space coordinates.
///
/// Like [`paint_rect_to_pdf`], this is identity for current unrotated pages
/// but names the boundary for path, stroke, text, and annotation emission.
pub(crate) fn paint_point_to_pdf(point: PaintPoint) -> PdfPoint {
    PdfPoint::new(point.x, point.y)
}

#[allow(dead_code)]
impl Page {
    pub(crate) fn paint_checkpoint(&self) -> PaintCheckpoint {
        PaintCheckpoint {
            operations: self.operations.clone(),
            paint_tree: self.paint_tree.clone(),
            rects: self.rects.clone(),
            rounded_rects: self.rounded_rects.clone(),
            paths: self.paths.clone(),
            strokes: self.strokes.clone(),
            images: self.images.clone(),
            lines: self.lines.clone(),
            links: self.links.clone(),
        }
    }

    pub(crate) fn take_paint_fragment_since(
        &mut self,
        checkpoint: PaintCheckpoint,
    ) -> PaintFragment {
        if let (Some(current_tree), Some(checkpoint_tree)) =
            (&self.paint_tree, &checkpoint.paint_tree)
        {
            let fragment = current_tree.fragment_since(checkpoint_tree, self);
            self.operations = checkpoint.operations;
            self.paint_tree = checkpoint.paint_tree;
            self.rects = checkpoint.rects;
            self.rounded_rects = checkpoint.rounded_rects;
            self.paths = checkpoint.paths;
            self.strokes = checkpoint.strokes;
            self.images = checkpoint.images;
            self.lines = checkpoint.lines;
            self.links = checkpoint.links;
            return fragment;
        }

        // CSS positioned/fixed layout first lays out an independent box, then
        // paints it into the stacking context slot selected by CSS 2.2 Appendix
        // E. Capture by primitive identity rather than operation index: block
        // backgrounds may be inserted earlier in paint order and shift indexes
        // for primitives that existed before the checkpoint.
        let mut checkpoint_primitives = checkpoint
            .paint_primitives_for_operations(&checkpoint.operations)
            .into_iter()
            .map(Some)
            .collect::<Vec<_>>();
        let primitives = self
            .paint_primitives_for_operations(&self.operations)
            .into_iter()
            .filter(|primitive| {
                let Some(position) =
                    checkpoint_primitives
                        .iter()
                        .position(|checkpoint_primitive| {
                            checkpoint_primitive.as_ref() == Some(primitive)
                        })
                else {
                    return true;
                };
                checkpoint_primitives[position] = None;
                false
            })
            .collect();
        let links = self.links[checkpoint.links.len()..].to_vec();

        self.operations = checkpoint.operations;
        self.paint_tree = checkpoint.paint_tree;
        self.rects = checkpoint.rects;
        self.rounded_rects = checkpoint.rounded_rects;
        self.paths = checkpoint.paths;
        self.strokes = checkpoint.strokes;
        self.images = checkpoint.images;
        self.lines = checkpoint.lines;
        self.links = checkpoint.links;
        PaintFragment::from_primitives(primitives, links)
    }

    pub(crate) fn paint_tree_fragment_since(
        &self,
        checkpoint: &PaintCheckpoint,
    ) -> Option<PaintFragment> {
        let (Some(current_tree), Some(checkpoint_tree)) =
            (&self.paint_tree, &checkpoint.paint_tree)
        else {
            return None;
        };
        Some(PaintFragment {
            display_list: PaintDisplayList {
                bands: current_tree.operation_node_fragment_since(checkpoint_tree),
            },
            links: Vec::new(),
        })
    }

    pub(crate) fn replace_paint_tree_since_with_context(
        &mut self,
        checkpoint: &PaintCheckpoint,
        band: PaintBand,
        context: PaintStackingContext,
    ) {
        let Some(checkpoint_tree) = &checkpoint.paint_tree else {
            return;
        };
        let Some(tree) = &mut self.paint_tree else {
            return;
        };
        tree.root.bands = checkpoint_tree.root.bands.clone();
        tree.root.bands.push_context_in_band(band, context);
        self.operations = tree.flattened_operations();
        self.links = tree.transformed_links();
    }

    pub(crate) fn prepend_recorded_primitives_to_fragment(
        &mut self,
        fragment: &mut PaintFragment,
        band: PaintBand,
        primitives: impl IntoIterator<Item = PaintPrimitive>,
    ) {
        let items = primitives
            .into_iter()
            .map(|primitive| PaintDisplayItem::Operation(self.record_paint_primitive(primitive)))
            .collect::<Vec<_>>();
        fragment.display_list.bands.bands[band.index()].splice(0..0, items);
    }

    pub(crate) fn append_recorded_primitives_to_fragment(
        &mut self,
        fragment: &mut PaintFragment,
        band: PaintBand,
        primitives: impl IntoIterator<Item = PaintPrimitive>,
    ) {
        fragment.display_list.bands.extend_band(
            band,
            primitives.into_iter().map(|primitive| {
                PaintDisplayItem::Operation(self.record_paint_primitive(primitive))
            }),
        );
    }

    /// Drains the page's current paint stream into a reusable fragment.
    ///
    /// CSS Fragmentation can split an out-of-flow positioned box across
    /// multiple page fragmentainers. During positioned layout, temporary pages
    /// are used to compute each fragment; the fragments are then replayed in
    /// the positioned stacking level instead of remaining as normal-flow page
    /// content:
    /// <https://www.w3.org/TR/css-break-3/#fragmentation-model> and
    /// <https://www.w3.org/TR/CSS22/zindex.html>.
    pub(crate) fn take_paint_fragment(&mut self) -> PaintFragment {
        let fragment = self.paint_fragment();
        self.operations.clear();
        if let Some(tree) = &mut self.paint_tree {
            tree.clear();
        }
        self.rects.clear();
        self.rounded_rects.clear();
        self.paths.clear();
        self.strokes.clear();
        self.images.clear();
        self.lines.clear();
        self.links.clear();
        fragment
    }

    #[allow(dead_code)]
    pub(crate) fn push_rect(&mut self, rect: RenderedRect) -> usize {
        self.push_rect_in_band(PaintBand::InFlowBlock, rect)
    }

    pub(crate) fn push_rect_in_band(&mut self, band: PaintBand, rect: RenderedRect) -> usize {
        let (index, operation) = self.record_rect(rect);
        self.operations.push(operation);
        self.push_paint_tree_operation_in_band(band, operation);
        index
    }

    #[allow(dead_code)]
    pub(crate) fn push_rounded_rect(&mut self, rect: RenderedRoundedRect) -> usize {
        self.push_rounded_rect_in_band(PaintBand::InFlowBlock, rect)
    }

    pub(crate) fn push_rounded_rect_in_band(
        &mut self,
        band: PaintBand,
        rect: RenderedRoundedRect,
    ) -> usize {
        let (index, operation) = self.record_rounded_rect(rect);
        self.operations.push(operation);
        self.push_paint_tree_operation_in_band(band, operation);
        index
    }

    #[allow(dead_code)]
    pub(crate) fn push_path(&mut self, path: RenderedPath) -> usize {
        self.push_path_in_band(PaintBand::InFlowBlock, path)
    }

    pub(crate) fn push_path_in_band(&mut self, band: PaintBand, path: RenderedPath) -> usize {
        let (index, operation) = self.record_path(path);
        self.operations.push(operation);
        self.push_paint_tree_operation_in_band(band, operation);
        index
    }

    #[allow(dead_code)]
    pub(crate) fn push_stroke(&mut self, stroke: RenderedStroke) -> usize {
        self.push_stroke_in_band(PaintBand::InFlowBlock, stroke)
    }

    pub(crate) fn push_stroke_in_band(&mut self, band: PaintBand, stroke: RenderedStroke) -> usize {
        let (index, operation) = self.record_stroke(stroke);
        self.operations.push(operation);
        self.push_paint_tree_operation_in_band(band, operation);
        index
    }

    pub(crate) fn push_line(&mut self, line: RenderedLine) -> usize {
        self.push_line_in_band(PaintBand::InFlowBlock, line)
    }

    pub(crate) fn push_line_in_band(&mut self, band: PaintBand, line: RenderedLine) -> usize {
        let (index, operation) = self.record_line(line);
        self.operations.push(operation);
        self.push_paint_tree_operation_in_band(band, operation);
        index
    }

    #[allow(dead_code)]
    pub(crate) fn push_image(&mut self, image: RenderedImage) -> usize {
        self.push_image_in_band(PaintBand::InFlowBlock, image)
    }

    pub(crate) fn push_image_in_band(&mut self, band: PaintBand, image: RenderedImage) -> usize {
        let (index, operation) = self.record_image(image);
        self.operations.push(operation);
        self.push_paint_tree_operation_in_band(band, operation);
        index
    }

    pub(crate) fn push_link(&mut self, link: RenderedLink) {
        self.links.push(link.clone());
        if let Some(tree) = &mut self.paint_tree {
            tree.push_link(PaintBand::Inline, link);
        }
    }

    /// Return the CSS painting order used for PDF content stream emission.
    ///
    /// CSS 2.2 Appendix E defines painting as an ordered sequence, and PDF content
    /// streams preserve visible stacking by serializing drawing operators in that
    /// order. New layout code records this order explicitly in `operations`; the
    /// synthesized fallback keeps older externally-built pages renderable.
    pub fn paint_operations(&self) -> Cow<'_, [PaintOperation]> {
        if let Some(tree) = &self.paint_tree
            && !tree.is_empty()
        {
            return Cow::Owned(tree.flattened_operations());
        }
        if !self.operations.is_empty() {
            return Cow::Borrowed(&self.operations);
        }

        let mut operations = Vec::new();
        operations.extend(
            self.images
                .iter()
                .enumerate()
                .filter(|(_, image)| image.background)
                .map(|(index, _)| PaintOperation::Image(index)),
        );
        operations.extend((0..self.rects.len()).map(PaintOperation::Rect));
        operations.extend((0..self.rounded_rects.len()).map(PaintOperation::RoundedRect));
        operations.extend((0..self.paths.len()).map(PaintOperation::Path));
        operations.extend((0..self.strokes.len()).map(PaintOperation::Stroke));
        operations.extend(
            self.images
                .iter()
                .enumerate()
                .filter(|(_, image)| !image.background)
                .map(|(index, _)| PaintOperation::Image(index)),
        );
        operations.extend((0..self.lines.len()).map(PaintOperation::Line));
        Cow::Owned(operations)
    }

    pub(crate) fn validate_paint_operations(&self, page_index: usize) -> Result<()> {
        if self.operations.is_empty() {
            return Ok(());
        }

        let mut rects_seen = vec![false; self.rects.len()];
        let mut rounded_rects_seen = vec![false; self.rounded_rects.len()];
        let mut paths_seen = vec![false; self.paths.len()];
        let mut strokes_seen = vec![false; self.strokes.len()];
        let mut images_seen = vec![false; self.images.len()];
        let mut lines_seen = vec![false; self.lines.len()];

        for (operation_index, operation) in self.operations.iter().enumerate() {
            match operation {
                PaintOperation::Rect(index) => mark_operation_index(
                    &mut rects_seen,
                    *index,
                    self.rects.len(),
                    page_index,
                    operation_index,
                    "rect",
                )?,
                PaintOperation::Stroke(index) => mark_operation_index(
                    &mut strokes_seen,
                    *index,
                    self.strokes.len(),
                    page_index,
                    operation_index,
                    "stroke",
                )?,
                PaintOperation::RoundedRect(index) => mark_operation_index(
                    &mut rounded_rects_seen,
                    *index,
                    self.rounded_rects.len(),
                    page_index,
                    operation_index,
                    "rounded rect",
                )?,
                PaintOperation::Path(index) => mark_operation_index(
                    &mut paths_seen,
                    *index,
                    self.paths.len(),
                    page_index,
                    operation_index,
                    "path",
                )?,
                PaintOperation::Image(index) => mark_operation_index(
                    &mut images_seen,
                    *index,
                    self.images.len(),
                    page_index,
                    operation_index,
                    "image",
                )?,
                PaintOperation::Line(index) => mark_operation_index(
                    &mut lines_seen,
                    *index,
                    self.lines.len(),
                    page_index,
                    operation_index,
                    "line",
                )?,
            }
        }

        ensure_all_operations_referenced(&rects_seen, page_index, "rect")?;
        ensure_all_operations_referenced(&rounded_rects_seen, page_index, "rounded rect")?;
        ensure_all_operations_referenced(&paths_seen, page_index, "path")?;
        ensure_all_operations_referenced(&strokes_seen, page_index, "stroke")?;
        ensure_all_operations_referenced(&images_seen, page_index, "image")?;
        ensure_all_operations_referenced(&lines_seen, page_index, "line")?;
        Ok(())
    }

    pub(crate) fn paint_tree(&self) -> Option<&PagePaintTree> {
        self.paint_tree.as_ref().filter(|tree| !tree.is_empty())
    }

    pub(crate) fn finalize_paint_tree_for_public_view(&mut self) {
        let Some(tree) = &self.paint_tree else {
            return;
        };
        if tree.is_empty() {
            return;
        }
        self.operations = tree.flattened_operations();
        self.links = tree.transformed_links();
    }

    pub(crate) fn record_rect(&mut self, rect: RenderedRect) -> (usize, PaintOperation) {
        let index = self.rects.len();
        self.rects.push(rect);
        (index, PaintOperation::Rect(index))
    }

    pub(crate) fn record_rounded_rect(
        &mut self,
        rect: RenderedRoundedRect,
    ) -> (usize, PaintOperation) {
        let index = self.rounded_rects.len();
        self.rounded_rects.push(rect);
        (index, PaintOperation::RoundedRect(index))
    }

    pub(crate) fn record_path(&mut self, path: RenderedPath) -> (usize, PaintOperation) {
        let index = self.paths.len();
        self.paths.push(path);
        (index, PaintOperation::Path(index))
    }

    pub(crate) fn record_stroke(&mut self, stroke: RenderedStroke) -> (usize, PaintOperation) {
        let index = self.strokes.len();
        self.strokes.push(stroke);
        (index, PaintOperation::Stroke(index))
    }

    pub(crate) fn record_line(&mut self, line: RenderedLine) -> (usize, PaintOperation) {
        let index = self.lines.len();
        self.lines.push(line);
        (index, PaintOperation::Line(index))
    }

    pub(crate) fn record_image(&mut self, image: RenderedImage) -> (usize, PaintOperation) {
        let index = self.images.len();
        self.images.push(image);
        (index, PaintOperation::Image(index))
    }

    pub(crate) fn paint_primitives_for_operations(
        &self,
        operations: &[PaintOperation],
    ) -> Vec<PaintPrimitive> {
        operations
            .iter()
            .filter_map(|operation| self.paint_primitive(operation))
            .collect()
    }

    pub(crate) fn paint_fragment(&self) -> PaintFragment {
        if let Some(tree) = &self.paint_tree {
            return PaintFragment {
                display_list: PaintDisplayList {
                    bands: tree.root.bands.primitive_node_copy(self),
                },
                links: Vec::new(),
            };
        }
        PaintFragment::from_primitives(
            self.paint_primitives_for_operations(&self.paint_operations()),
            self.links.clone(),
        )
    }

    pub(crate) fn record_paint_primitive(&mut self, primitive: PaintPrimitive) -> PaintOperation {
        // CSS positioned layout can replay fixed-position descendants on multiple
        // pages. Copying the primitive into each page keeps PDF resource indexes
        // page-local while preserving the already computed paint order.
        match primitive {
            PaintPrimitive::Rect(rect) => self.record_rect(rect).1,
            PaintPrimitive::RoundedRect(rect) => self.record_rounded_rect(rect).1,
            PaintPrimitive::Path(path) => self.record_path(path).1,
            PaintPrimitive::Stroke(stroke) => self.record_stroke(stroke).1,
            PaintPrimitive::Image(image) => self.record_image(image).1,
            PaintPrimitive::Line(line) => self.record_line(line).1,
        }
    }

    pub(crate) fn record_paint_fragment(
        &mut self,
        fragment: &PaintFragment,
        offset: PaintVector,
    ) -> RecordedPaintFragment {
        let translated = fragment.clone().translated(offset);
        let operations = translated
            .flattened_primitives()
            .into_iter()
            .map(|primitive| self.record_paint_primitive(primitive))
            .collect::<Vec<_>>();
        let mut operation_iter = operations.iter().copied();
        let display_list = translated
            .display_list
            .into_operation_nodes(&mut operation_iter)
            .with_links(PaintBand::Inline, translated.links.clone());
        let mut links = Vec::new();
        display_list
            .bands
            .push_transformed_links(PaintTransform::identity(), &mut links);
        self.links.extend(links);
        RecordedPaintFragment {
            operations,
            display_list,
        }
    }

    pub(crate) fn append_paint_fragment(&mut self, fragment: &PaintFragment, offset: PaintVector) {
        let recorded = self.record_paint_fragment(fragment, offset);
        self.append_recorded_paint_fragment(recorded);
    }

    pub(crate) fn prepend_recorded_paint_fragment(&mut self, recorded: RecordedPaintFragment) {
        if let Some(tree) = &mut self.paint_tree {
            tree.prepend_display_list(recorded.display_list);
        }
        self.operations.splice(0..0, recorded.operations);
    }

    pub(crate) fn append_recorded_paint_fragment(&mut self, recorded: RecordedPaintFragment) {
        if let Some(tree) = &mut self.paint_tree {
            tree.append_display_list(recorded.display_list);
        }
        self.operations.extend(recorded.operations);
    }

    pub(crate) fn sort_paint_tree_stacking_contexts(&mut self) {
        if let Some(tree) = &mut self.paint_tree {
            tree.sort_stacking_contexts();
        }
    }

    fn paint_primitive(&self, operation: &PaintOperation) -> Option<PaintPrimitive> {
        match operation {
            PaintOperation::Rect(index) => {
                self.rects.get(*index).cloned().map(PaintPrimitive::Rect)
            }
            PaintOperation::RoundedRect(index) => self
                .rounded_rects
                .get(*index)
                .copied()
                .map(PaintPrimitive::RoundedRect),
            PaintOperation::Path(index) => {
                self.paths.get(*index).cloned().map(PaintPrimitive::Path)
            }
            PaintOperation::Stroke(index) => self
                .strokes
                .get(*index)
                .copied()
                .map(PaintPrimitive::Stroke),
            PaintOperation::Image(index) => {
                self.images.get(*index).cloned().map(PaintPrimitive::Image)
            }
            PaintOperation::Line(index) => {
                self.lines.get(*index).cloned().map(PaintPrimitive::Line)
            }
        }
    }

    fn push_paint_tree_operation_in_band(&mut self, band: PaintBand, operation: PaintOperation) {
        if let Some(tree) = &mut self.paint_tree {
            tree.push_operation(band, operation);
        }
    }
}

fn mark_operation_index(
    seen: &mut [bool],
    primitive_index: usize,
    primitive_len: usize,
    page_index: usize,
    operation_index: usize,
    primitive_name: &str,
) -> Result<()> {
    let Some(already_seen) = seen.get_mut(primitive_index) else {
        return Err(Error::InvalidInput(format!(
            "page {} paint operation {} references missing {} {} ({} available)",
            page_index + 1,
            operation_index,
            primitive_name,
            primitive_index,
            primitive_len
        )));
    };
    if *already_seen {
        return Err(Error::InvalidInput(format!(
            "page {} paint operation {} references duplicate {} {}",
            page_index + 1,
            operation_index,
            primitive_name,
            primitive_index
        )));
    }
    *already_seen = true;
    Ok(())
}

fn ensure_all_operations_referenced(
    seen: &[bool],
    page_index: usize,
    primitive_name: &str,
) -> Result<()> {
    if let Some(index) = seen.iter().position(|referenced| !referenced) {
        return Err(Error::InvalidInput(format!(
            "page {} has unreferenced {} {} while paint operations are present",
            page_index + 1,
            primitive_name,
            index
        )));
    }
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PaintOperation {
    Rect(usize),
    RoundedRect(usize),
    Path(usize),
    Stroke(usize),
    Image(usize),
    Line(usize),
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct PaintCheckpoint {
    operations: Vec<PaintOperation>,
    paint_tree: Option<PagePaintTree>,
    rects: Vec<RenderedRect>,
    rounded_rects: Vec<RenderedRoundedRect>,
    paths: Vec<RenderedPath>,
    strokes: Vec<RenderedStroke>,
    images: Vec<RenderedImage>,
    lines: Vec<RenderedLine>,
    links: Vec<RenderedLink>,
}

impl PaintCheckpoint {
    fn paint_primitives_for_operations(
        &self,
        operations: &[PaintOperation],
    ) -> Vec<PaintPrimitive> {
        operations
            .iter()
            .filter_map(|operation| self.paint_primitive(operation))
            .collect()
    }

    fn paint_primitive(&self, operation: &PaintOperation) -> Option<PaintPrimitive> {
        match operation {
            PaintOperation::Rect(index) => {
                self.rects.get(*index).cloned().map(PaintPrimitive::Rect)
            }
            PaintOperation::RoundedRect(index) => self
                .rounded_rects
                .get(*index)
                .copied()
                .map(PaintPrimitive::RoundedRect),
            PaintOperation::Path(index) => {
                self.paths.get(*index).cloned().map(PaintPrimitive::Path)
            }
            PaintOperation::Stroke(index) => self
                .strokes
                .get(*index)
                .copied()
                .map(PaintPrimitive::Stroke),
            PaintOperation::Image(index) => {
                self.images.get(*index).cloned().map(PaintPrimitive::Image)
            }
            PaintOperation::Line(index) => {
                self.lines.get(*index).cloned().map(PaintPrimitive::Line)
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum PaintPrimitive {
    Rect(RenderedRect),
    RoundedRect(RenderedRoundedRect),
    Path(RenderedPath),
    Stroke(RenderedStroke),
    Image(RenderedImage),
    Line(RenderedLine),
}

impl PaintPrimitive {
    pub(crate) fn translated(self, offset: PaintVector) -> Self {
        match self {
            Self::Rect(rect) => Self::Rect(rect.translated(offset)),
            Self::RoundedRect(rect) => Self::RoundedRect(rect.translated(offset)),
            Self::Path(path) => Self::Path(path.translated(offset)),
            Self::Stroke(stroke) => Self::Stroke(stroke.translated(offset)),
            Self::Image(image) => Self::Image(image.translated(offset)),
            Self::Line(line) => Self::Line(line.translated(offset)),
        }
    }

    fn clipped_to_rect(self, clip: PaintClip) -> Option<Self> {
        match self {
            Self::Rect(mut rect) => {
                let clipped = PaintClip::from_paint_rect(rect.paint_rect()).intersect(clip)?;
                rect.set_paint_rect(clipped.paint_rect());
                Some(Self::Rect(rect))
            }
            Self::RoundedRect(mut rect) => {
                let clipped = PaintClip::from_paint_rect(rect.paint_rect()).intersect(clip)?;
                rect.rect = clipped.paint_rect();
                Some(Self::RoundedRect(rect))
            }
            Self::Image(image) => rect_bounds(image.paint_rect())
                .and_then(|bounds| bounds.intersect(clip))
                .map(|_| Self::Image(image)),
            Self::Path(path) => path_bounds(&path)
                .and_then(|bounds| bounds.intersect(clip))
                .map(|_| Self::Path(path)),
            Self::Stroke(stroke) => stroke
                .paint_bounds()
                .intersect(clip)
                .map(|_| Self::Stroke(stroke)),
            Self::Line(line) => line
                .paint_bounds()
                .intersect(clip)
                .map(|_| Self::Line(line)),
        }
    }

    fn bounds(&self) -> Option<PaintClip> {
        match self {
            Self::Rect(rect) => rect_bounds(rect.paint_rect()),
            Self::RoundedRect(rect) => rect_bounds(rect.paint_rect()),
            Self::Image(image) => rect_bounds(image.paint_rect()),
            Self::Stroke(stroke) => Some(stroke.paint_bounds()),
            Self::Line(line) => Some(line.paint_bounds()),
            Self::Path(path) => path_bounds(path),
        }
    }
}

/// Fragment-local CSS display list used before primitives are flattened into a page stream.
///
/// CSS painting order is a tree of stacking contexts, not just a page-wide
/// sequence. CSS 2.2 Appendix E defines the recursive stacking order, and CSS
/// Positioned Layout defines positioned boxes with stack levels. Reasyprint
/// stores that recursive structure in captured fragments, then flattens it to
/// PDF drawing operators because PDF content streams paint sequentially
/// (ISO 32000-1:2008, §8.2).
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct PaintDisplayList {
    bands: PaintBandList,
}

impl PaintDisplayList {
    fn from_primitives(primitives: Vec<PaintPrimitive>) -> Self {
        let mut bands = PaintBandList::default();
        for primitive in primitives {
            let band = if matches!(primitive, PaintPrimitive::Line(_)) {
                PaintBand::Inline
            } else {
                PaintBand::InFlowBlock
            };
            bands.extend_band(band, [PaintDisplayItem::Primitive(primitive)]);
        }
        Self { bands }
    }

    fn is_empty(&self) -> bool {
        self.bands.is_empty()
    }

    fn flattened_primitives(&self) -> Vec<PaintPrimitive> {
        let mut primitives = Vec::new();
        self.push_flattened_primitives(&mut primitives);
        primitives
    }

    fn push_flattened_primitives(&self, primitives: &mut Vec<PaintPrimitive>) {
        self.bands.push_flattened_primitives(primitives);
    }

    pub(crate) fn translated(self, offset: PaintVector) -> Self {
        Self {
            bands: self.bands.translated(offset),
        }
    }

    fn into_operation_nodes(self, operations: &mut impl Iterator<Item = PaintOperation>) -> Self {
        Self {
            bands: self.bands.into_operation_nodes(operations),
        }
    }

    fn with_links(mut self, band: PaintBand, links: Vec<RenderedLink>) -> Self {
        self.bands
            .extend_band(band, links.into_iter().map(PaintDisplayItem::Link));
        self
    }
}

/// Page-level durable paint tree used as the private CSS paint-order source.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct PagePaintTree {
    pub(crate) root: PaintStackingContext,
}

#[allow(dead_code)]
impl PagePaintTree {
    pub(crate) fn new() -> Self {
        Self {
            root: PaintStackingContext::root(),
        }
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.root.bands.is_empty()
    }

    pub(crate) fn clear(&mut self) {
        *self = Self::new();
    }

    pub(crate) fn flattened_operations(&self) -> Vec<PaintOperation> {
        self.root.bands.flattened_operations()
    }

    pub(crate) fn push_operation(&mut self, band: PaintBand, operation: PaintOperation) {
        self.root.bands.push_operation(band, operation);
    }

    pub(crate) fn push_link(&mut self, band: PaintBand, link: RenderedLink) {
        self.root.bands.push_link(band, link);
    }

    pub(crate) fn push_context(&mut self, context: PaintStackingContext) {
        self.root.bands.push_context(context);
    }

    pub(crate) fn sort_stacking_contexts(&mut self) {
        self.root.bands.sort_stacking_contexts();
    }

    pub(crate) fn prepend_display_list(&mut self, display_list: PaintDisplayList) {
        self.root.bands.prepend_bands(display_list.bands);
    }

    pub(crate) fn append_display_list(&mut self, display_list: PaintDisplayList) {
        self.root.bands.append_bands(display_list.bands);
    }

    fn fragment_since(&self, checkpoint: &Self, page: &Page) -> PaintFragment {
        PaintFragment {
            display_list: PaintDisplayList {
                bands: self.root.bands.fragment_since(&checkpoint.root.bands, page),
            },
            links: Vec::new(),
        }
    }

    fn operation_node_fragment_since(&self, checkpoint: &Self) -> PaintBandList {
        self.root
            .bands
            .operation_node_fragment_since(&checkpoint.root.bands)
    }

    pub(crate) fn transformed_links(&self) -> Vec<RenderedLink> {
        let mut links = Vec::new();
        self.root
            .push_transformed_links(PaintTransform::identity(), &mut links);
        links
    }
}

/// CSS painting-order band inside one stacking context.
///
/// CSS 2.2 Appendix E defines stacking-context painting as a sequence of
/// ordered bands. Keeping the band identity until flattening lets positioned
/// and fragmented descendants be replayed in their spec slot instead of being
/// spliced into an already-flat PDF primitive stream:
/// <https://www.w3.org/TR/CSS22/zindex.html>.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PaintBand {
    PageBackground,
    BackgroundBorder,
    NegativeZ,
    InFlowBlock,
    Float,
    Inline,
    AutoZeroZ,
    PositiveZ,
    Outline,
}

impl PaintBand {
    pub(crate) const ORDER: [Self; 9] = [
        Self::PageBackground,
        Self::BackgroundBorder,
        Self::NegativeZ,
        Self::InFlowBlock,
        Self::Float,
        Self::Inline,
        Self::AutoZeroZ,
        Self::PositiveZ,
        Self::Outline,
    ];

    pub(crate) const fn index(self) -> usize {
        match self {
            Self::PageBackground => 0,
            Self::BackgroundBorder => 1,
            Self::NegativeZ => 2,
            Self::InFlowBlock => 3,
            Self::Float => 4,
            Self::Inline => 5,
            Self::AutoZeroZ => 6,
            Self::PositiveZ => 7,
            Self::Outline => 8,
        }
    }
}

/// Ordered paint-band buckets for a fragment-local display list.
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct PaintBandList {
    pub(crate) bands: [Vec<PaintDisplayItem>; 9],
}

#[allow(dead_code)]
impl PaintBandList {
    fn is_empty(&self) -> bool {
        self.bands.iter().all(Vec::is_empty)
    }

    fn extend_band(&mut self, band: PaintBand, items: impl IntoIterator<Item = PaintDisplayItem>) {
        self.bands[band.index()].extend(items);
    }

    fn push_operation(&mut self, band: PaintBand, operation: PaintOperation) {
        self.bands[band.index()].push(PaintDisplayItem::Operation(operation));
    }

    fn push_link(&mut self, band: PaintBand, link: RenderedLink) {
        self.bands[band.index()].push(PaintDisplayItem::Link(link));
    }

    pub(crate) fn push_context(&mut self, context: PaintStackingContext) {
        let band = context.stack_level.paint_band();
        self.push_context_in_band(band, context);
    }

    pub(crate) fn push_context_in_band(&mut self, band: PaintBand, context: PaintStackingContext) {
        self.bands[band.index()].push(PaintDisplayItem::StackingContext(context));
    }

    fn sort_stacking_contexts(&mut self) {
        for band in [
            PaintBand::NegativeZ,
            PaintBand::AutoZeroZ,
            PaintBand::PositiveZ,
        ] {
            self.bands[band.index()].sort_by_key(|item| match item {
                PaintDisplayItem::StackingContext(context) => {
                    (context.stack_level.sort_key(), context.source_order)
                }
                PaintDisplayItem::Operation(_)
                | PaintDisplayItem::Primitive(_)
                | PaintDisplayItem::Link(_) => ((0, 0), 0),
            });
        }
    }

    fn prepend_bands(&mut self, bands: PaintBandList) {
        for band in PaintBand::ORDER {
            let target = &mut self.bands[band.index()];
            let source = bands.bands[band.index()].clone();
            target.splice(0..0, source);
        }
    }

    fn append_bands(&mut self, bands: PaintBandList) {
        for band in PaintBand::ORDER {
            self.bands[band.index()].extend(bands.bands[band.index()].clone());
        }
    }

    fn fragment_since(&self, checkpoint: &Self, page: &Page) -> Self {
        let mut bands = PaintBandList::default();
        for band in PaintBand::ORDER {
            let current = &self.bands[band.index()];
            let checkpoint = &checkpoint.bands[band.index()];
            let start = shared_prefix_len(current, checkpoint);
            bands.bands[band.index()].extend(
                current[start..]
                    .iter()
                    .cloned()
                    .filter_map(|item| item.into_primitive_node(page)),
            );
        }
        bands
    }

    fn operation_node_fragment_since(&self, checkpoint: &Self) -> Self {
        let mut bands = PaintBandList::default();
        for band in PaintBand::ORDER {
            let current = &self.bands[band.index()];
            let checkpoint = &checkpoint.bands[band.index()];
            let start = shared_prefix_len(current, checkpoint);
            bands.bands[band.index()].extend(current[start..].iter().cloned());
        }
        bands
    }

    fn into_items_in_order(self) -> Vec<PaintDisplayItem> {
        let mut ordered = Vec::new();
        for band in PaintBand::ORDER {
            ordered.extend(self.bands[band.index()].clone());
        }
        ordered
    }

    fn push_flattened_primitives(&self, primitives: &mut Vec<PaintPrimitive>) {
        for band in PaintBand::ORDER {
            for item in &self.bands[band.index()] {
                match item {
                    PaintDisplayItem::Operation(_) | PaintDisplayItem::Link(_) => {}
                    PaintDisplayItem::Primitive(primitive) => primitives.push(primitive.clone()),
                    PaintDisplayItem::StackingContext(context) => {
                        context.push_flattened_primitives(primitives);
                    }
                }
            }
        }
    }

    fn flattened_operations(&self) -> Vec<PaintOperation> {
        let mut operations = Vec::new();
        self.push_flattened_operations(&mut operations);
        operations
    }

    fn push_flattened_operations(&self, operations: &mut Vec<PaintOperation>) {
        for band in PaintBand::ORDER {
            for item in &self.bands[band.index()] {
                match item {
                    PaintDisplayItem::Operation(operation) => operations.push(*operation),
                    PaintDisplayItem::StackingContext(context) => {
                        context.bands.push_flattened_operations(operations);
                    }
                    PaintDisplayItem::Primitive(_) | PaintDisplayItem::Link(_) => {}
                }
            }
        }
    }

    pub(crate) fn translated(self, offset: PaintVector) -> Self {
        Self {
            bands: self.bands.map(|items| {
                items
                    .into_iter()
                    .map(|item| item.translated(offset))
                    .collect()
            }),
        }
    }

    fn into_operation_nodes(self, operations: &mut impl Iterator<Item = PaintOperation>) -> Self {
        Self {
            bands: self.bands.map(|items| {
                items
                    .into_iter()
                    .filter_map(|item| item.into_operation_node(operations))
                    .collect()
            }),
        }
    }

    fn primitive_node_copy(&self, page: &Page) -> Self {
        Self {
            bands: self.bands.clone().map(|items| {
                items
                    .into_iter()
                    .filter_map(|item| item.into_primitive_node(page))
                    .collect()
            }),
        }
    }

    fn push_transformed_links(&self, transform: PaintTransform, links: &mut Vec<RenderedLink>) {
        for band in PaintBand::ORDER {
            for item in &self.bands[band.index()] {
                item.push_transformed_links(transform, links);
            }
        }
    }
}

fn shared_prefix_len(left: &[PaintDisplayItem], right: &[PaintDisplayItem]) -> usize {
    left.iter()
        .zip(right)
        .take_while(|(left, right)| left == right)
        .count()
}

/// One item in a fragment-local display list.
///
/// The `StackingContext` variant represents the recursive units described by
/// CSS 2.2 Appendix E and CSS Positioned Layout stack levels:
/// <https://www.w3.org/TR/CSS22/zindex.html> and
/// <https://www.w3.org/TR/css-position-3/#painting-order>.
#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum PaintDisplayItem {
    Operation(PaintOperation),
    Primitive(PaintPrimitive),
    StackingContext(PaintStackingContext),
    Link(RenderedLink),
}

#[allow(dead_code)]
impl PaintDisplayItem {
    fn translated(self, offset: PaintVector) -> Self {
        match self {
            Self::Operation(operation) => Self::Operation(operation),
            Self::Primitive(primitive) => Self::Primitive(primitive.translated(offset)),
            Self::StackingContext(context) => Self::StackingContext(context.translated(offset)),
            Self::Link(link) => Self::Link(link.translated(offset)),
        }
    }

    fn into_operation_node(
        self,
        operations: &mut impl Iterator<Item = PaintOperation>,
    ) -> Option<Self> {
        match self {
            Self::Primitive(_) => operations.next().map(Self::Operation),
            Self::StackingContext(context) => Some(Self::StackingContext(
                context.into_operation_nodes(operations),
            )),
            Self::Operation(operation) => Some(Self::Operation(operation)),
            Self::Link(link) => Some(Self::Link(link)),
        }
    }

    fn into_primitive_node(self, page: &Page) -> Option<Self> {
        match self {
            Self::Operation(operation) => page.paint_primitive(&operation).map(Self::Primitive),
            Self::StackingContext(context) => {
                Some(Self::StackingContext(context.into_primitive_nodes(page)))
            }
            Self::Primitive(primitive) => Some(Self::Primitive(primitive)),
            Self::Link(link) => Some(Self::Link(link)),
        }
    }

    fn push_transformed_links(&self, transform: PaintTransform, links: &mut Vec<RenderedLink>) {
        match self {
            Self::Link(link) => links.push(link.transformed(transform)),
            Self::StackingContext(context) => context.push_transformed_links(transform, links),
            Self::Operation(_) | Self::Primitive(_) => {}
        }
    }
}

/// CSS positioned stack level for one stacking-context node.
///
/// `auto` is distinct from integer `0` in CSS Positioned Layout even though
/// both paint in the auto/zero band of the parent stacking context:
/// <https://www.w3.org/TR/css-position-3/#painting-order>.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum StackLevel {
    #[allow(dead_code)]
    Auto,
    Integer(i32),
}

impl StackLevel {
    pub(crate) fn from_z_index(z_index: i32) -> Self {
        Self::Integer(z_index)
    }

    pub(crate) fn from_optional_z_index(z_index: Option<i32>) -> Self {
        z_index.map_or(Self::Auto, Self::Integer)
    }

    pub(crate) fn paint_band(self) -> PaintBand {
        match self {
            Self::Integer(value) if value < 0 => PaintBand::NegativeZ,
            Self::Integer(value) if value > 0 => PaintBand::PositiveZ,
            Self::Auto | Self::Integer(0) => PaintBand::AutoZeroZ,
            Self::Integer(_) => PaintBand::AutoZeroZ,
        }
    }

    pub(crate) fn sort_key(self) -> (i32, i32) {
        match self {
            Self::Integer(value) => (value, 0),
            Self::Auto => (0, 0),
        }
    }
}

/// Effects applied to a whole stacking context before PDF emission.
///
/// CSS Transforms, CSS Color opacity, and CSS Overflow act on stacking-context
/// contents as a group. The current flattening path keeps these defaults until
/// PDF group and matrix emission are wired through page streams:
/// <https://www.w3.org/TR/css-transforms-1/>,
/// <https://www.w3.org/TR/css-color-4/#transparency>, and
/// <https://www.w3.org/TR/css-overflow-3/>.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct PaintEffects {
    pub(crate) opacity: f32,
    pub(crate) transform: Option<PaintTransform>,
    pub(crate) overflow_clip: Option<PaintClip>,
    pub(crate) absolute_clip: Option<PaintClip>,
    pub(crate) clip_path: PaintClipPathEffect,
    pub(crate) mask: PaintMaskEffect,
    pub(crate) filter: PaintFilterEffect,
    pub(crate) blend_mode: PaintBlendMode,
    pub(crate) isolation: bool,
}

impl Default for PaintEffects {
    fn default() -> Self {
        Self {
            opacity: 1.0,
            transform: None,
            overflow_clip: None,
            absolute_clip: None,
            clip_path: PaintClipPathEffect::None,
            mask: PaintMaskEffect::None,
            filter: PaintFilterEffect::None,
            blend_mode: PaintBlendMode::Normal,
            isolation: false,
        }
    }
}

impl PaintEffects {
    pub(crate) fn needs_group(self) -> bool {
        self.opacity < 1.0
            || self.filter.is_active()
            || self.mask.is_active()
            || self.blend_mode != PaintBlendMode::Normal
            || self.isolation
    }

    pub(crate) fn without_group_effects(mut self) -> Self {
        self.opacity = 1.0;
        self.filter = PaintFilterEffect::None;
        self.mask = PaintMaskEffect::None;
        self.blend_mode = PaintBlendMode::Normal;
        self.isolation = false;
        self
    }

    pub(crate) fn ordered_steps(self) -> Vec<PaintEffectStep> {
        let mut steps = Vec::new();
        if let Some(clip) = self.absolute_clip {
            steps.push(PaintEffectStep::Clip(clip));
        }
        if let Some(clip) = self.overflow_clip {
            steps.push(PaintEffectStep::Clip(clip));
        }
        if self.clip_path.is_active() {
            steps.push(PaintEffectStep::ClipPath(self.clip_path));
        }
        if let Some(transform) = self.transform {
            steps.push(PaintEffectStep::Transform(transform));
        }
        if self.filter.is_active() {
            steps.push(PaintEffectStep::Filter(self.filter));
        }
        if self.mask.is_active() {
            steps.push(PaintEffectStep::Mask(self.mask));
        }
        if self.opacity < 1.0 {
            steps.push(PaintEffectStep::Opacity(self.opacity));
        }
        if self.blend_mode != PaintBlendMode::Normal {
            steps.push(PaintEffectStep::Blend(self.blend_mode));
        }
        if self.isolation {
            steps.push(PaintEffectStep::Isolation);
        }
        steps
    }
}

/// Shape source for a context-level CSS `clip-path`.
///
/// The current renderer records the source category so stacking and PDF group
/// construction are deterministic. Geometry emission is implemented later for
/// each non-`None` variant:
/// <https://www.w3.org/TR/css-masking-1/#the-clip-path>.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PaintClipPathEffect {
    None,
    Inset,
    Shape,
    Url,
    WillChange,
}

impl PaintClipPathEffect {
    pub(crate) const fn is_active(self) -> bool {
        !matches!(self, Self::None)
    }
}

/// Masking source recorded for context-level PDF grouping.
///
/// CSS Masking allows image and generated-image masks. Quire currently records
/// the presence of a mask for isolation/grouping and leaves shape/raster
/// emission as a remaining conformance step:
/// <https://www.w3.org/TR/css-masking-1/#the-mask-image>.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PaintMaskEffect {
    None,
    MaskImage,
    WillChange,
}

impl PaintMaskEffect {
    pub(crate) const fn is_active(self) -> bool {
        !matches!(self, Self::None)
    }
}

/// Filter source recorded for context-level PDF grouping.
///
/// Filter function rendering is not complete yet; this type distinguishes real
/// authored filters from `will-change` pre-isolation.
/// <https://www.w3.org/TR/filter-effects-1/#FilterProperty>.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PaintFilterEffect {
    None,
    FilterList,
    WillChange,
}

impl PaintFilterEffect {
    pub(crate) const fn is_active(self) -> bool {
        !matches!(self, Self::None)
    }
}

/// PDF-facing blend mode selected by CSS `mix-blend-mode`.
///
/// The current content writer uses this to force isolated group construction;
/// future PDF ExtGState output can map these variants to `/BM` names.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum PaintBlendMode {
    Normal,
    Multiply,
    Screen,
    Overlay,
    Darken,
    Lighten,
    ColorDodge,
    ColorBurn,
    HardLight,
    SoftLight,
    Difference,
    Exclusion,
    Hue,
    Saturation,
    Color,
    Luminosity,
}

impl PaintBlendMode {
    pub(crate) const fn pdf_name(self) -> Option<&'static str> {
        match self {
            Self::Normal => None,
            Self::Multiply => Some("Multiply"),
            Self::Screen => Some("Screen"),
            Self::Overlay => Some("Overlay"),
            Self::Darken => Some("Darken"),
            Self::Lighten => Some("Lighten"),
            Self::ColorDodge => Some("ColorDodge"),
            Self::ColorBurn => Some("ColorBurn"),
            Self::HardLight => Some("HardLight"),
            Self::SoftLight => Some("SoftLight"),
            Self::Difference => Some("Difference"),
            Self::Exclusion => Some("Exclusion"),
            Self::Hue => Some("Hue"),
            Self::Saturation => Some("Saturation"),
            Self::Color => Some("Color"),
            Self::Luminosity => Some("Luminosity"),
        }
    }

    pub(crate) fn resource_name(self) -> Option<String> {
        self.pdf_name().map(|name| format!("GSblend{name}"))
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum PaintEffectStep {
    Clip(PaintClip),
    ClipPath(PaintClipPathEffect),
    Transform(PaintTransform),
    Filter(PaintFilterEffect),
    Mask(PaintMaskEffect),
    Opacity(f32),
    Blend(PaintBlendMode),
    Isolation,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct PaintTransform {
    pub(crate) a: f32,
    pub(crate) b: f32,
    pub(crate) c: f32,
    pub(crate) d: f32,
    pub(crate) e: f32,
    pub(crate) f: f32,
}

impl PaintTransform {
    pub(crate) const fn identity() -> Self {
        Self {
            a: 1.0,
            b: 0.0,
            c: 0.0,
            d: 1.0,
            e: 0.0,
            f: 0.0,
        }
    }

    /// Build a paint-space translation transform.
    ///
    /// CSS Transforms applies translation functions in the element's current
    /// painting coordinate system; by this point Quire has already projected
    /// layout geometry into [`PaintSpace`]:
    /// <https://www.w3.org/TR/css-transforms-1/#transform-functions>.
    pub(crate) fn translate(offset: PaintVector) -> Self {
        Self {
            a: 1.0,
            b: 0.0,
            c: 0.0,
            d: 1.0,
            e: offset.x,
            f: offset.y,
        }
    }

    pub(crate) fn multiply(self, right: Self) -> Self {
        Self {
            a: self.a * right.a + self.c * right.b,
            b: self.b * right.a + self.d * right.b,
            c: self.a * right.c + self.c * right.d,
            d: self.b * right.c + self.d * right.d,
            e: self.a * right.e + self.c * right.f + self.e,
            f: self.b * right.e + self.d * right.f + self.f,
        }
    }

    /// Apply this transform to a page-local paint point.
    ///
    /// CSS Transforms maps already-painted geometry into the parent painting
    /// coordinate system. Keeping the input and output as [`PaintPoint`]
    /// prevents transform effects from crossing into layout top-edge or PDF
    /// user-space coordinates by accident:
    /// <https://www.w3.org/TR/css-transforms-1/#transform-rendering>.
    pub(crate) fn apply_point(self, point: PaintPoint) -> PaintPoint {
        PaintPoint::new(
            self.a * point.x + self.c * point.y + self.e,
            self.b * point.x + self.d * point.y + self.f,
        )
    }

    /// Transform a paint-space clip rectangle and return its axis-aligned bounds.
    ///
    /// CSS rectangular clips are transformed with the element, while PDF
    /// annotation bounds and effect isolation need a conservative axis-aligned
    /// paint rectangle after transform application:
    /// <https://www.w3.org/TR/css-transforms-1/#transform-rendering>.
    pub(crate) fn apply_clip_to_aabb(self, clip: PaintClip) -> PaintClip {
        let points = [
            self.apply_point(clip.bottom_left()),
            self.apply_point(clip.bottom_right()),
            self.apply_point(clip.top_left()),
            self.apply_point(clip.top_right()),
        ];
        let min_x = points
            .iter()
            .map(|point| point.x)
            .fold(f32::INFINITY, f32::min);
        let max_x = points
            .iter()
            .map(|point| point.x)
            .fold(f32::NEG_INFINITY, f32::max);
        let min_y = points
            .iter()
            .map(|point| point.y)
            .fold(f32::INFINITY, f32::min);
        let max_y = points
            .iter()
            .map(|point| point.y)
            .fold(f32::NEG_INFINITY, f32::max);
        PaintClip::from_paint_rect(PaintRect::new(
            PaintPoint::new(min_x, min_y),
            PaintSize::new((max_x - min_x).max(0.0), (max_y - min_y).max(0.0)),
        ))
    }
}

/// Axis-aligned paint clipping rectangle.
///
/// CSS Overflow clips box contents to a rectangular overflow clip edge in the
/// untransformed local coordinate space, and CSS Transforms then maps that
/// clipped output into parent coordinates:
/// <https://www.w3.org/TR/css-overflow-3/#overflow-clip-edge> and
/// <https://www.w3.org/TR/css-transforms-1/#transform-rendering>.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct PaintClip {
    rect: PaintRect,
}

impl PaintClip {
    pub(crate) fn new(x: f32, y: f32, width: f32, height: f32) -> Self {
        Self::from_paint_rect(PaintRect::new(
            PaintPoint::new(x, y),
            PaintSize::new(width.max(0.0), height.max(0.0)),
        ))
    }

    pub(crate) fn from_paint_rect(rect: PaintRect) -> Self {
        Self { rect }
    }

    pub(crate) fn from_paint_point(point: PaintPoint) -> Self {
        Self::from_paint_rect(PaintRect::new(point, PaintSize::new(0.0, 0.0)))
    }

    pub(crate) fn x(self) -> f32 {
        self.rect.origin.x
    }

    pub(crate) fn y(self) -> f32 {
        self.rect.origin.y
    }

    pub(crate) fn width(self) -> f32 {
        self.rect.size.width
    }

    pub(crate) fn height(self) -> f32 {
        self.rect.size.height
    }

    pub(crate) fn paint_rect(self) -> PaintRect {
        self.rect
    }

    pub(crate) fn bottom_left(self) -> PaintPoint {
        self.rect.origin
    }

    pub(crate) fn bottom_right(self) -> PaintPoint {
        PaintPoint::new(self.x() + self.width(), self.y())
    }

    pub(crate) fn top_left(self) -> PaintPoint {
        PaintPoint::new(self.x(), self.y() + self.height())
    }

    pub(crate) fn top_right(self) -> PaintPoint {
        PaintPoint::new(self.x() + self.width(), self.y() + self.height())
    }

    fn translated(mut self, offset: PaintVector) -> Self {
        self.rect.origin += offset;
        self
    }

    pub(crate) fn intersect(self, other: Self) -> Option<Self> {
        let left = self.x().max(other.x());
        let right = (self.x() + self.width()).min(other.x() + other.width());
        let bottom = self.y().max(other.y());
        let top = (self.y() + self.height()).min(other.y() + other.height());
        (right > left && top > bottom).then_some(Self::new(
            left,
            bottom,
            right - left,
            top - bottom,
        ))
    }

    fn union(self, other: Self) -> Self {
        let left = self.x().min(other.x());
        let right = (self.x() + self.width()).max(other.x() + other.width());
        let bottom = self.y().min(other.y());
        let top = (self.y() + self.height()).max(other.y() + other.height());
        Self::new(
            left,
            bottom,
            (right - left).max(0.0),
            (top - bottom).max(0.0),
        )
    }
}

/// Nested stacking context captured during layout before PDF emission.
///
/// CSS 2.2 Appendix E paints each stacking context atomically at its parent
/// stack level. The current node keeps normal-flow content and child stacking
/// contexts together so descendants with large `z-index` values cannot escape
/// an ancestor stacking context when the fragment is replayed onto a page.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct PaintStackingContext {
    pub(crate) source_order: usize,
    pub(crate) stack_level: StackLevel,
    pub(crate) bands: PaintBandList,
    pub(crate) effects: PaintEffects,
    pub(crate) bounds: Option<PaintClip>,
}

impl PaintStackingContext {
    fn root() -> Self {
        Self {
            source_order: 0,
            stack_level: StackLevel::Auto,
            bands: PaintBandList::default(),
            effects: PaintEffects::default(),
            bounds: None,
        }
    }

    /// Build a stacking-context node for an independently painted fragment.
    ///
    /// CSS 2.2 Appendix E requires descendant stacking contexts to be painted
    /// atomically inside the parent stack level:
    /// <https://www.w3.org/TR/CSS22/zindex.html>.
    pub(crate) fn new(
        z_index: i32,
        content: PaintFragment,
        child_contexts: Vec<PaintStackingContext>,
    ) -> Self {
        Self::new_with_stack_level(StackLevel::from_z_index(z_index), content, child_contexts)
    }

    pub(crate) fn new_with_stack_level(
        stack_level: StackLevel,
        content: PaintFragment,
        child_contexts: Vec<PaintStackingContext>,
    ) -> Self {
        let mut bands = PaintBandList::default();
        bands.extend_band(
            PaintBand::BackgroundBorder,
            content.display_list.bands.into_items_in_order(),
        );
        for context in child_contexts {
            bands.push_context(context);
        }
        bands.sort_stacking_contexts();
        Self::with_bands(stack_level, bands)
    }

    /// Build a stacking-context node for an independently painted fragment
    /// while preserving the fragment's internal CSS paint bands.
    ///
    /// CSS Positioned Layout assigns the outer stack level in the parent
    /// context, but CSS 2.2 Appendix E still applies recursively inside that
    /// positioned context:
    /// <https://www.w3.org/TR/css-position-3/#painting-order> and
    /// <https://www.w3.org/TR/CSS22/zindex.html>.
    pub(crate) fn from_banded_fragment_with_stack_level(
        stack_level: StackLevel,
        content: PaintFragment,
        child_contexts: Vec<PaintStackingContext>,
    ) -> Self {
        let mut bands = content.display_list.bands;
        if matches!(stack_level, StackLevel::Integer(_)) {
            let in_flow = std::mem::take(&mut bands.bands[PaintBand::InFlowBlock.index()]);
            bands
                .bands
                .get_mut(PaintBand::BackgroundBorder.index())
                .expect("background band index should exist")
                .extend(in_flow);
        }
        for link in content.links {
            bands.push_link(PaintBand::Inline, link);
        }
        for context in child_contexts {
            bands.push_context(context);
        }
        bands.sort_stacking_contexts();
        Self::with_bands(stack_level, bands)
    }

    /// Build an atomic stacking-context node while preserving the fragment's
    /// existing paint-band structure.
    ///
    /// CSS Transforms and CSS Color opacity create stacking contexts whose
    /// descendants still follow CSS 2.2 Appendix E inside the isolated group:
    /// <https://www.w3.org/TR/css-transforms-1/#transform-rendering> and
    /// <https://www.w3.org/TR/css-color-4/#transparency>.
    pub(crate) fn from_banded_fragment(
        content: PaintFragment,
        child_contexts: Vec<PaintStackingContext>,
    ) -> Self {
        let mut bands = content.display_list.bands;
        for link in content.links {
            bands.push_link(PaintBand::Inline, link);
        }
        for context in child_contexts {
            bands.push_context(context);
        }
        bands.sort_stacking_contexts();
        Self::with_bands(StackLevel::Auto, bands)
    }

    fn with_bands(stack_level: StackLevel, bands: PaintBandList) -> Self {
        Self {
            source_order: 0,
            stack_level,
            bands,
            effects: PaintEffects::default(),
            bounds: None,
        }
    }

    pub(crate) fn with_source_order(mut self, source_order: usize) -> Self {
        self.source_order = source_order;
        self
    }

    pub(crate) fn with_effects(mut self, effects: PaintEffects) -> Self {
        self.effects = effects;
        self
    }

    pub(crate) fn with_bounds(mut self, bounds: PaintClip) -> Self {
        self.bounds = Some(bounds);
        self
    }

    pub(crate) fn with_links(mut self, links: Vec<RenderedLink>) -> Self {
        for link in links {
            self.bands.push_link(PaintBand::Inline, link);
        }
        self
    }

    /// Return bounds after context-level clipping and transforms.
    ///
    /// CSS applies overflow/absolute clipping in the context's local coordinate
    /// space and then maps the painted result through transforms. PDF opacity
    /// groups need a Form XObject `/BBox` covering that composed output:
    /// <https://www.w3.org/TR/css-overflow-3/#overflow-clip-edge>,
    /// <https://www.w3.org/TR/css-transforms-1/#transform-rendering>, and
    /// ISO 32000-1:2008 §11.6.6.
    pub(crate) fn effect_bounds(&self, fallback: PaintClip) -> PaintClip {
        let mut bounds = self.bounds.unwrap_or(fallback);
        for clip in [self.effects.absolute_clip, self.effects.overflow_clip]
            .into_iter()
            .flatten()
        {
            bounds =
                bounds
                    .intersect(clip)
                    .unwrap_or(PaintClip::new(bounds.x(), bounds.y(), 0.0, 0.0));
        }
        if let Some(transform) = self.effects.transform {
            bounds = transform.apply_clip_to_aabb(bounds);
        }
        bounds
    }

    fn push_flattened_primitives(&self, primitives: &mut Vec<PaintPrimitive>) {
        self.bands.push_flattened_primitives(primitives);
    }

    pub(crate) fn translated(self, offset: PaintVector) -> Self {
        Self {
            source_order: self.source_order,
            stack_level: self.stack_level,
            bands: self.bands.translated(offset),
            effects: self.effects,
            bounds: self.bounds.map(|bounds| bounds.translated(offset)),
        }
    }

    fn into_operation_nodes(self, operations: &mut impl Iterator<Item = PaintOperation>) -> Self {
        Self {
            source_order: self.source_order,
            stack_level: self.stack_level,
            bands: self.bands.into_operation_nodes(operations),
            effects: self.effects,
            bounds: self.bounds,
        }
    }

    fn into_primitive_nodes(self, page: &Page) -> Self {
        Self {
            source_order: self.source_order,
            stack_level: self.stack_level,
            bands: self.bands.primitive_node_copy(page),
            effects: self.effects,
            bounds: self.bounds,
        }
    }

    fn push_transformed_links(
        &self,
        parent_transform: PaintTransform,
        links: &mut Vec<RenderedLink>,
    ) {
        let transform = if let Some(transform) = self.effects.transform {
            parent_transform.multiply(transform)
        } else {
            parent_transform
        };
        self.bands.push_transformed_links(transform, links);
    }
}

pub(crate) struct RecordedPaintFragment {
    operations: Vec<PaintOperation>,
    display_list: PaintDisplayList,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct PaintFragment {
    display_list: PaintDisplayList,
    pub links: Vec<RenderedLink>,
}

impl PaintFragment {
    /// Build a fragment from page-local primitives in their current paint order.
    ///
    /// The primitive sequence represents one CSS paint-order band as defined by
    /// CSS 2.2 Appendix E before it is eventually serialized as ordered PDF
    /// drawing operators.
    pub(crate) fn from_primitives(
        primitives: Vec<PaintPrimitive>,
        links: Vec<RenderedLink>,
    ) -> Self {
        Self {
            display_list: PaintDisplayList::from_primitives(primitives),
            links,
        }
    }

    /// Build a fragment whose root is a captured CSS stacking context.
    ///
    /// This preserves the recursive stacking relationship from CSS 2.2
    /// Appendix E until the fragment is flattened for the PDF page content
    /// stream.
    pub(crate) fn from_stacking_context(context: PaintStackingContext) -> Self {
        Self::from_stacking_context_in_band(context.stack_level.paint_band(), context)
    }

    pub(crate) fn from_stacking_context_in_band(
        band: PaintBand,
        context: PaintStackingContext,
    ) -> Self {
        Self {
            display_list: PaintDisplayList {
                bands: {
                    let mut bands = PaintBandList::default();
                    bands.push_context_in_band(band, context);
                    bands
                },
            },
            links: Vec::new(),
        }
    }

    pub(crate) fn flattened_primitives(&self) -> Vec<PaintPrimitive> {
        self.display_list.flattened_primitives()
    }

    pub(crate) fn prepend_primitives_in_band(
        &mut self,
        band: PaintBand,
        primitives: impl IntoIterator<Item = PaintPrimitive>,
    ) {
        let existing_primitives = self.flattened_primitives();
        let items = primitives
            .into_iter()
            .filter(|primitive| {
                !primitive_is_covered_by_later_opaque_rect(primitive, &existing_primitives)
            })
            .map(PaintDisplayItem::Primitive)
            .collect::<Vec<_>>();
        if items.is_empty() {
            return;
        }
        self.display_list.bands.bands[band.index()].splice(0..0, items);
    }

    pub(crate) fn append_primitives_in_band(
        &mut self,
        band: PaintBand,
        primitives: impl IntoIterator<Item = PaintPrimitive>,
    ) {
        self.display_list.bands.extend_band(
            band,
            primitives.into_iter().map(PaintDisplayItem::Primitive),
        );
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.display_list.is_empty() && self.links.is_empty()
    }

    pub(crate) fn first_line_y(&self) -> Option<f32> {
        self.flattened_primitives()
            .into_iter()
            .find_map(|primitive| match primitive {
                PaintPrimitive::Line(line) => Some(line.y()),
                _ => None,
            })
    }

    pub(crate) fn last_line_y(&self) -> Option<f32> {
        self.flattened_primitives()
            .into_iter()
            .rev()
            .find_map(|primitive| match primitive {
                PaintPrimitive::Line(line) => Some(line.y()),
                _ => None,
            })
    }

    pub(crate) fn bounds(&self) -> Option<PaintClip> {
        let mut bounds: Option<PaintClip> = None;
        for primitive in self.flattened_primitives() {
            let Some(primitive_bounds) = primitive.bounds() else {
                continue;
            };
            bounds = Some(match bounds {
                Some(existing) => existing.union(primitive_bounds),
                None => primitive_bounds,
            });
        }
        for link in &self.links {
            let link_bounds = PaintClip::from_paint_rect(link.paint_rect());
            bounds = Some(match bounds {
                Some(existing) => existing.union(link_bounds),
                None => link_bounds,
            });
        }
        bounds
    }

    pub(crate) fn translated(mut self, offset: PaintVector) -> Self {
        self.display_list = self.display_list.translated(offset);
        self.links = self
            .links
            .into_iter()
            .map(|link| link.translated(offset))
            .collect();
        self
    }

    /// Return a fragment whose flattened public primitive data is clipped to a
    /// rectangular page-local slice.
    ///
    /// Context effects preserve the same clip for PDF output; this helper keeps
    /// `Document` inspection data aligned with fragmented paint:
    /// <https://www.w3.org/TR/css-break-3/#box-splitting>.
    pub(crate) fn clipped_to_rect(self, clip: PaintClip) -> Self {
        let primitives = self
            .flattened_primitives()
            .into_iter()
            .filter_map(|primitive| primitive.clipped_to_rect(clip))
            .collect::<Vec<_>>();
        let links = self
            .links
            .into_iter()
            .filter_map(|mut link| {
                let clipped = PaintClip::from_paint_rect(link.paint_rect()).intersect(clip)?;
                link.rect = clipped.paint_rect();
                Some(link)
            })
            .collect::<Vec<_>>();
        Self::from_primitives(primitives, links)
    }
}

fn primitive_is_covered_by_later_opaque_rect(
    primitive: &PaintPrimitive,
    later_primitives: &[PaintPrimitive],
) -> bool {
    let PaintPrimitive::Rect(rect) = primitive else {
        return false;
    };
    if rect.stroke.is_some() || rect.fill.is_none_or(|fill| fill.a < 1.0) {
        return false;
    }
    later_primitives.iter().any(|later| {
        let PaintPrimitive::Rect(later) = later else {
            return false;
        };
        later.stroke.is_none()
            && later.fill.is_some_and(|fill| fill.a >= 1.0)
            && same_rect_geometry(rect, later)
    })
}

fn same_rect_geometry(left: &RenderedRect, right: &RenderedRect) -> bool {
    (left.x() - right.x()).abs() < 0.001
        && (left.y() - right.y()).abs() < 0.001
        && (left.width() - right.width()).abs() < 0.001
        && (left.height() - right.height()).abs() < 0.001
}

#[derive(Debug, Clone, PartialEq)]
pub struct RenderedRect {
    rect: PaintRect,
    pub fill: Option<Color>,
    pub stroke: Option<Color>,
    pub stroke_width: f32,
}

impl RenderedRect {
    pub fn new(
        x: f32,
        y: f32,
        width: f32,
        height: f32,
        fill: Option<Color>,
        stroke: Option<Color>,
        stroke_width: f32,
    ) -> Self {
        Self {
            rect: PaintRect::new(
                PaintPoint::new(x, y),
                PaintSize::new(width.max(0.0), height.max(0.0)),
            ),
            fill,
            stroke,
            stroke_width,
        }
    }

    pub(crate) fn from_paint_rect(rect: PaintRect, fill: Option<Color>) -> Self {
        Self {
            rect,
            fill,
            stroke: None,
            stroke_width: 0.0,
        }
    }

    pub fn x(&self) -> f32 {
        self.rect.origin.x
    }

    pub fn y(&self) -> f32 {
        self.rect.origin.y
    }

    pub fn width(&self) -> f32 {
        self.rect.size.width
    }

    pub fn height(&self) -> f32 {
        self.rect.size.height
    }

    pub(crate) fn paint_rect(&self) -> PaintRect {
        self.rect
    }

    pub(crate) fn set_paint_rect(&mut self, rect: PaintRect) {
        self.rect = rect;
    }

    fn translated(mut self, offset: PaintVector) -> Self {
        self.rect.origin += offset;
        self
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RenderedRoundedRect {
    rect: PaintRect,
    pub radii: RenderedRoundedRectRadii,
    pub fill: Option<Color>,
    pub stroke: Option<Color>,
    pub stroke_width: f32,
}

impl RenderedRoundedRect {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        x: f32,
        y: f32,
        width: f32,
        height: f32,
        radii: RenderedRoundedRectRadii,
        fill: Option<Color>,
        stroke: Option<Color>,
        stroke_width: f32,
    ) -> Self {
        Self::from_paint_rect(
            PaintRect::new(
                PaintPoint::new(x, y),
                PaintSize::new(width.max(0.0), height.max(0.0)),
            ),
            radii,
            fill,
            stroke,
            stroke_width,
        )
    }

    pub(crate) fn from_paint_rect(
        rect: PaintRect,
        radii: RenderedRoundedRectRadii,
        fill: Option<Color>,
        stroke: Option<Color>,
        stroke_width: f32,
    ) -> Self {
        Self {
            rect,
            radii,
            fill,
            stroke,
            stroke_width,
        }
    }

    pub fn x(self) -> f32 {
        self.rect.origin.x
    }

    pub fn y(self) -> f32 {
        self.rect.origin.y
    }

    pub fn width(self) -> f32 {
        self.rect.size.width
    }

    pub fn height(self) -> f32 {
        self.rect.size.height
    }

    pub(crate) fn paint_rect(self) -> PaintRect {
        self.rect
    }

    fn translated(mut self, offset: PaintVector) -> Self {
        self.rect.origin += offset;
        self
    }
}

/// A generic PDF path paint primitive used when a CSS feature cannot be
/// represented by a rectangle, rounded rectangle, or single stroke.
///
/// CSS Backgrounds and Borders Level 3 models border areas as curved regions,
/// and PDF content streams represent those regions with path construction and
/// painting operators: <https://www.w3.org/TR/css-backgrounds-3/#borders> and
/// ISO 32000-1:2008, 8.5 "Path Construction and Painting".
#[derive(Debug, Clone, PartialEq)]
pub struct RenderedPath {
    pub clip: Option<RenderedPathClip>,
    pub commands: Vec<RenderedPathCommand>,
    pub fill: Option<Color>,
    pub fill_rule: RenderedPathFillRule,
    pub stroke: Option<Color>,
    pub stroke_width: f32,
}

impl RenderedPath {
    pub(crate) fn new(
        commands: Vec<RenderedPathCommand>,
        fill: Option<Color>,
        fill_rule: RenderedPathFillRule,
        stroke: Option<Color>,
        stroke_width: f32,
        clip: Option<RenderedPathClip>,
    ) -> Self {
        Self {
            clip,
            commands,
            fill,
            fill_rule,
            stroke,
            stroke_width,
        }
    }

    fn translated(mut self, offset: PaintVector) -> Self {
        if let Some(clip) = &mut self.clip {
            for command in &mut clip.commands {
                command.translate(offset);
            }
            for nested_clip in &mut clip.additional_clips {
                for command in &mut nested_clip.commands {
                    command.translate(offset);
                }
            }
        }
        for command in &mut self.commands {
            command.translate(offset);
        }
        self
    }
}

/// A PDF path clipping scope applied before painting a vector path.
///
/// PDF clipping paths are established with `W`/`W*` and the current path, then
/// later drawing is limited to that region until the graphics state is
/// restored. CSS border side painting uses this to isolate one side of a
/// rounded border ring when side colors or styles differ:
/// <https://www.w3.org/TR/css-backgrounds-3/#corner-shaping> and ISO
/// 32000-1:2008, 8.5.4 "Clipping Path Operators".
#[derive(Debug, Clone, PartialEq)]
pub struct RenderedPathClip {
    pub commands: Vec<RenderedPathCommand>,
    pub fill_rule: RenderedPathFillRule,
    pub additional_clips: Vec<RenderedPathClipPath>,
}

impl RenderedPathClip {
    pub(crate) fn new(
        commands: Vec<RenderedPathCommand>,
        fill_rule: RenderedPathFillRule,
        additional_clips: Vec<RenderedPathClipPath>,
    ) -> Self {
        Self {
            commands,
            fill_rule,
            additional_clips,
        }
    }
}

/// One additional PDF clipping path intersected with an active clip scope.
///
/// CSS rounded patterned borders need the intersection of a side transition
/// region and the rounded border ring. PDF models this by applying multiple
/// clipping paths in sequence within one graphics state:
/// ISO 32000-1:2008, 8.5.4 "Clipping Path Operators".
#[derive(Debug, Clone, PartialEq)]
pub struct RenderedPathClipPath {
    pub commands: Vec<RenderedPathCommand>,
    pub fill_rule: RenderedPathFillRule,
}

impl RenderedPathClipPath {
    pub(crate) fn new(commands: Vec<RenderedPathCommand>, fill_rule: RenderedPathFillRule) -> Self {
        Self {
            commands,
            fill_rule,
        }
    }
}

/// A PDF-compatible path construction command.
///
/// The variants map directly to PDF `m`, `l`, `c`, and `h` operators from ISO
/// 32000-1:2008, 8.5.2 "Path Construction Operators".
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum RenderedPathCommand {
    MoveTo(PaintPoint),
    LineTo(PaintPoint),
    CurveTo {
        control_1: PaintPoint,
        control_2: PaintPoint,
        end: PaintPoint,
    },
    Close,
}

impl RenderedPathCommand {
    pub(crate) fn move_to(point: PaintPoint) -> Self {
        Self::MoveTo(point)
    }

    pub(crate) fn line_to(point: PaintPoint) -> Self {
        Self::LineTo(point)
    }

    pub(crate) fn curve_to(control_1: PaintPoint, control_2: PaintPoint, end: PaintPoint) -> Self {
        Self::CurveTo {
            control_1,
            control_2,
            end,
        }
    }

    pub(crate) fn typed_points(self) -> RenderedPathCommandPoints {
        match self {
            Self::MoveTo(point) => RenderedPathCommandPoints::MoveTo(point),
            Self::LineTo(point) => RenderedPathCommandPoints::LineTo(point),
            Self::CurveTo {
                control_1,
                control_2,
                end,
            } => RenderedPathCommandPoints::CurveTo {
                control_1,
                control_2,
                end,
            },
            Self::Close => RenderedPathCommandPoints::Close,
        }
    }

    fn translate(&mut self, offset: PaintVector) {
        match self {
            Self::MoveTo(point) | Self::LineTo(point) => {
                *point += offset;
            }
            Self::CurveTo {
                control_1,
                control_2,
                end,
            } => {
                *control_1 += offset;
                *control_2 += offset;
                *end += offset;
            }
            Self::Close => {}
        }
    }
}

/// Typed paint-space points for a rendered path command.
///
/// The public command enum keeps scalar fields for compatibility, while this
/// view gives the PDF backend explicit paint-space coordinates before the
/// final conversion to PDF user space.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum RenderedPathCommandPoints {
    MoveTo(PaintPoint),
    LineTo(PaintPoint),
    CurveTo {
        control_1: PaintPoint,
        control_2: PaintPoint,
        end: PaintPoint,
    },
    Close,
}

/// Fill rule for a PDF path.
///
/// PDF defines nonzero winding (`f`) and even-odd (`f*`) fill operators; CSS
/// border rings use even-odd filling so the padding-edge subpath cuts out the
/// content area without depending on subpath winding direction.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum RenderedPathFillRule {
    #[default]
    NonZero,
    EvenOdd,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RenderedRoundedRectRadii {
    pub top_left: RenderedCornerRadius,
    pub top_right: RenderedCornerRadius,
    pub bottom_right: RenderedCornerRadius,
    pub bottom_left: RenderedCornerRadius,
}

impl RenderedRoundedRectRadii {
    pub const ZERO: Self = Self {
        top_left: RenderedCornerRadius::ZERO,
        top_right: RenderedCornerRadius::ZERO,
        bottom_right: RenderedCornerRadius::ZERO,
        bottom_left: RenderedCornerRadius::ZERO,
    };
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RenderedCornerRadius {
    size: PaintSize,
}

impl RenderedCornerRadius {
    pub const ZERO: Self = Self {
        size: PaintSize::new(0.0, 0.0),
    };

    pub fn new(x: f32, y: f32) -> Self {
        Self {
            size: PaintSize::new(x.max(0.0), y.max(0.0)),
        }
    }

    pub fn x(&self) -> f32 {
        self.size.width
    }

    pub fn y(&self) -> f32 {
        self.size.height
    }

    pub(crate) fn inset(&mut self, inset: f32) {
        self.size.width = (self.size.width - inset).max(0.0);
        self.size.height = (self.size.height - inset).max(0.0);
    }

    pub(crate) fn scale(&mut self, factor: f32) {
        self.size.width *= factor;
        self.size.height *= factor;
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RenderedStroke {
    start: PaintPoint,
    end: PaintPoint,
    pub width: f32,
    pub color: Color,
    pub dash: Option<(f32, f32)>,
}

impl RenderedStroke {
    pub fn new(
        x1: f32,
        y1: f32,
        x2: f32,
        y2: f32,
        width: f32,
        color: Color,
        dash: Option<(f32, f32)>,
    ) -> Self {
        Self::from_paint_points(
            PaintPoint::new(x1, y1),
            PaintPoint::new(x2, y2),
            width,
            color,
            dash,
        )
    }

    #[allow(dead_code)]
    pub(crate) fn from_paint_points(
        start: PaintPoint,
        end: PaintPoint,
        width: f32,
        color: Color,
        dash: Option<(f32, f32)>,
    ) -> Self {
        Self {
            start,
            end,
            width,
            color,
            dash,
        }
    }

    pub fn x1(&self) -> f32 {
        self.start.x
    }

    pub fn y1(&self) -> f32 {
        self.start.y
    }

    pub fn x2(&self) -> f32 {
        self.end.x
    }

    pub fn y2(&self) -> f32 {
        self.end.y
    }

    pub(crate) fn paint_points(self) -> (PaintPoint, PaintPoint) {
        (self.start, self.end)
    }

    pub(crate) fn paint_bounds(self) -> PaintClip {
        let (start, end) = self.paint_points();
        let half = self.width / 2.0;
        let left = start.x.min(end.x) - half;
        let right = start.x.max(end.x) + half;
        let bottom = start.y.min(end.y) - half;
        let top = start.y.max(end.y) + half;
        PaintClip::from_paint_rect(PaintRect::new(
            PaintPoint::new(left, bottom),
            PaintSize::new((right - left).max(0.0), (top - bottom).max(0.0)),
        ))
    }

    fn translated(mut self, offset: PaintVector) -> Self {
        self.start += offset;
        self.end += offset;
        self
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct RenderedLine {
    pub text: String,
    origin: PaintPoint,
    pub font_size: f32,
    pub font_id: Option<usize>,
    pub color: Color,
    pub runs: Vec<RenderedTextRun>,
}

impl RenderedLine {
    pub fn new(
        text: String,
        x: f32,
        y: f32,
        font_size: f32,
        font_id: Option<usize>,
        color: Color,
        runs: Vec<RenderedTextRun>,
    ) -> Self {
        Self::from_paint_origin(text, PaintPoint::new(x, y), font_size, font_id, color, runs)
    }

    pub fn x(&self) -> f32 {
        self.origin.x
    }

    pub fn y(&self) -> f32 {
        self.origin.y
    }

    pub(crate) fn from_paint_origin(
        text: String,
        origin: PaintPoint,
        font_size: f32,
        font_id: Option<usize>,
        color: Color,
        runs: Vec<RenderedTextRun>,
    ) -> Self {
        Self {
            text,
            origin,
            font_size,
            font_id,
            color,
            runs,
        }
    }

    pub(crate) fn origin(&self) -> PaintPoint {
        self.origin
    }

    pub(crate) fn paint_bounds(&self) -> PaintClip {
        let origin = self.origin();
        PaintClip::from_paint_rect(PaintRect::new(
            PaintPoint::new(origin.x, origin.y - self.font_size),
            PaintSize::new(rendered_line_width(self), self.font_size * 1.35),
        ))
    }

    fn translated(mut self, offset: PaintVector) -> Self {
        self.origin += offset;
        self
    }

    pub(crate) fn translate_origin(&mut self, offset: PaintVector) {
        self.origin += offset;
    }
}

fn rendered_line_width(line: &RenderedLine) -> f32 {
    line.runs.iter().fold(0.0_f32, |width, run| {
        let run_width = if run.text_matrix.is_identity() {
            run.glyphs
                .as_ref()
                .map(|glyphs| glyphs.iter().map(|glyph| glyph.x_advance).sum::<f32>())
                .unwrap_or_else(|| run.text.chars().count() as f32 * run.font_size * 0.5)
        } else {
            run.font_size
        };
        width.max(run.x_offset + run_width)
    })
}

fn rect_bounds(rect: PaintRect) -> Option<PaintClip> {
    (rect.size.width > 0.0 && rect.size.height > 0.0).then_some(PaintClip::from_paint_rect(rect))
}

fn path_bounds(path: &RenderedPath) -> Option<PaintClip> {
    let mut bounds: Option<PaintClip> = None;
    for command in &path.commands {
        for point in command_points(*command) {
            let point = PaintClip::from_paint_point(point);
            bounds = Some(match bounds {
                Some(existing) => existing.union(point),
                None => point,
            });
        }
    }
    bounds.map(|bounds| {
        let outset = path.stroke_width.max(0.0) / 2.0;
        PaintClip::new(
            bounds.x() - outset,
            bounds.y() - outset,
            bounds.width() + outset * 2.0,
            bounds.height() + outset * 2.0,
        )
    })
}

fn command_points(command: RenderedPathCommand) -> Vec<PaintPoint> {
    match command.typed_points() {
        RenderedPathCommandPoints::MoveTo(point) | RenderedPathCommandPoints::LineTo(point) => {
            vec![point]
        }
        RenderedPathCommandPoints::CurveTo {
            control_1,
            control_2,
            end,
        } => vec![control_1, control_2, end],
        RenderedPathCommandPoints::Close => Vec::new(),
    }
}

/// A shaped text run positioned relative to a [`RenderedLine`] origin.
///
/// CSS inline layout produces line boxes containing adjacent font/style runs.
/// Keeping runs explicit lets the PDF backend emit each run with its selected
/// embedded font instead of inferring runs from flattened text.
#[derive(Debug, Clone, PartialEq)]
pub struct RenderedTextRun {
    pub text: String,
    pub x_offset: f32,
    pub y_offset: f32,
    pub text_matrix: RenderedTextMatrix,
    pub font_size: f32,
    pub font_id: Option<usize>,
    pub glyphs: Option<Vec<RenderedGlyph>>,
}

/// PDF text matrix orientation for one shaped text run.
///
/// CSS Writing Modes can place the same shaped glyph stream on a horizontal
/// or vertical baseline. Keeping the 2x2 matrix with the run lets layout own
/// writing-mode placement while PDF emission only applies the selected text
/// matrix:
/// <https://www.w3.org/TR/css-writing-modes-4/#text-flow> and
/// ISO 32000-2:2020, 9.4.4 "Text Space Details".
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RenderedTextMatrix {
    pub a: f32,
    pub b: f32,
    pub c: f32,
    pub d: f32,
}

impl RenderedTextMatrix {
    pub const IDENTITY: Self = Self {
        a: 1.0,
        b: 0.0,
        c: 0.0,
        d: 1.0,
    };
    pub const ROTATE_CW: Self = Self {
        a: 0.0,
        b: -1.0,
        c: 1.0,
        d: 0.0,
    };
    pub const ROTATE_CCW: Self = Self {
        a: 0.0,
        b: 1.0,
        c: -1.0,
        d: 0.0,
    };

    pub(crate) fn is_identity(self) -> bool {
        self == Self::IDENTITY
    }
}

/// Shaped glyph data kept with painted text for PDF emission.
///
/// CSS Fonts requires text to be shaped with the selected font face before
/// glyph emission; PDF text objects then encode glyph IDs with positioning and
/// ToUnicode extraction data.
#[derive(Debug, Clone, PartialEq)]
pub struct RenderedGlyph {
    pub id: u16,
    pub x_advance: f32,
    pub nominal_x_advance: f32,
    pub x_offset: f32,
    pub y_offset: f32,
    pub unicode: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RenderedLink {
    rect: PaintRect,
    pub target: String,
}

impl RenderedLink {
    pub(crate) fn from_paint_rect(rect: PaintRect, target: String) -> Self {
        Self { rect, target }
    }

    pub fn x(&self) -> f32 {
        self.rect.origin.x
    }

    pub fn y(&self) -> f32 {
        self.rect.origin.y
    }

    pub fn width(&self) -> f32 {
        self.rect.size.width
    }

    pub fn height(&self) -> f32 {
        self.rect.size.height
    }

    pub(crate) fn paint_rect(&self) -> PaintRect {
        self.rect
    }

    pub(crate) fn translated(mut self, offset: PaintVector) -> Self {
        self.rect.origin += offset;
        self
    }

    fn transformed(&self, transform: PaintTransform) -> Self {
        let clip = transform.apply_clip_to_aabb(PaintClip::from_paint_rect(self.rect));
        Self::from_paint_rect(clip.paint_rect(), self.target.clone())
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct RenderedImage {
    pub background: bool,
    rect: PaintRect,
    pub pixel_width: u32,
    pub pixel_height: u32,
    pub source_rect: Option<RenderedImageSourceRect>,
    pub interpolate: bool,
    pub rgb: Vec<u8>,
    pub alpha: Option<Vec<u8>>,
    pub alt_text: Option<String>,
}

impl RenderedImage {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn from_paint_rect(
        rect: PaintRect,
        background: bool,
        pixel_width: u32,
        pixel_height: u32,
        source_rect: Option<RenderedImageSourceRect>,
        interpolate: bool,
        rgb: Vec<u8>,
        alpha: Option<Vec<u8>>,
        alt_text: Option<String>,
    ) -> Self {
        Self {
            background,
            rect,
            pixel_width,
            pixel_height,
            source_rect,
            interpolate,
            rgb,
            alpha,
            alt_text,
        }
    }

    pub fn x(&self) -> f32 {
        self.rect.origin.x
    }

    pub fn y(&self) -> f32 {
        self.rect.origin.y
    }

    pub fn width(&self) -> f32 {
        self.rect.size.width
    }

    pub fn height(&self) -> f32 {
        self.rect.size.height
    }

    pub(crate) fn paint_rect(&self) -> PaintRect {
        self.rect
    }

    pub(crate) fn set_paint_rect(&mut self, rect: PaintRect) {
        self.rect = rect;
    }
}

impl RenderedImage {
    fn translated(mut self, offset: PaintVector) -> Self {
        self.rect.origin += offset;
        self
    }
}

/// Pixel-space source rectangle for drawing a cropped PDF image XObject.
///
/// CSS Border Images use nine-slice scaling: each destination border segment
/// maps to a source image slice. PDF image XObjects have fixed pixel data, so
/// source cropping is normalized before resource emission:
/// <https://www.w3.org/TR/css-backgrounds-3/#border-images> and ISO
/// 32000-1:2008, 8.9 "Images".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct RenderedImageSourceRect {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

impl RenderedImageSourceRect {
    pub fn x(&self) -> u32 {
        self.x
    }

    pub fn y(&self) -> u32 {
        self.y
    }

    pub fn width(&self) -> u32 {
        self.width
    }

    pub fn height(&self) -> u32 {
        self.height
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paint_clip_round_trips_through_typed_rect() {
        let rect = PaintRect::new(PaintPoint::new(10.0, 20.0), PaintSize::new(30.0, 40.0));
        let clip = PaintClip::from_paint_rect(rect);

        assert_eq!(clip, PaintClip::new(10.0, 20.0, 30.0, 40.0));
        assert_eq!(clip.paint_rect(), rect);
    }

    #[test]
    fn rendered_rect_exposes_paint_rect() {
        let rect = PaintRect::new(PaintPoint::new(3.0, 4.0), PaintSize::new(5.0, 6.0));
        let rendered = RenderedRect::from_paint_rect(rect, Some(Color::BLACK));

        assert_eq!(rendered.paint_rect(), rect);
        assert_eq!(rendered.fill, Some(Color::BLACK));
    }

    #[test]
    fn rendered_image_exposes_paint_rect() {
        let rect = PaintRect::new(PaintPoint::new(3.0, 4.0), PaintSize::new(5.0, 6.0));
        let image = RenderedImage::from_paint_rect(
            rect,
            false,
            5,
            6,
            None,
            false,
            Vec::new(),
            None,
            Some("alt".to_string()),
        );

        assert_eq!(image.paint_rect(), rect);
        assert_eq!(image.width(), 5.0);
        assert_eq!(image.height(), 6.0);
    }

    #[test]
    fn paint_rect_to_pdf_is_identity_for_unrotated_pages() {
        let rect = PaintRect::new(PaintPoint::new(7.0, 8.0), PaintSize::new(9.0, 10.0));

        assert_eq!(
            paint_rect_to_pdf(rect),
            PdfRect::new(PdfPoint::new(7.0, 8.0), PdfSize::new(9.0, 10.0))
        );
    }

    #[test]
    fn paint_point_to_pdf_is_identity_for_unrotated_pages() {
        assert_eq!(
            paint_point_to_pdf(PaintPoint::new(11.0, 12.0)),
            PdfPoint::new(11.0, 12.0)
        );
    }

    #[test]
    fn paint_transform_maps_typed_points_and_clips() {
        let transform = PaintTransform::translate(PaintVector::new(5.0, -2.0));

        assert_eq!(
            transform.apply_point(PaintPoint::new(10.0, 20.0)),
            PaintPoint::new(15.0, 18.0)
        );
        assert_eq!(
            transform.apply_clip_to_aabb(PaintClip::from_paint_rect(PaintRect::new(
                PaintPoint::new(10.0, 20.0),
                PaintSize::new(30.0, 40.0),
            ))),
            PaintClip::from_paint_rect(PaintRect::new(
                PaintPoint::new(15.0, 18.0),
                PaintSize::new(30.0, 40.0),
            ))
        );
    }

    #[test]
    fn path_commands_expose_typed_paint_points() {
        assert_eq!(
            RenderedPathCommand::move_to(PaintPoint::new(1.0, 2.0)).typed_points(),
            RenderedPathCommandPoints::MoveTo(PaintPoint::new(1.0, 2.0))
        );
        assert_eq!(
            RenderedPathCommand::curve_to(
                PaintPoint::new(1.0, 2.0),
                PaintPoint::new(3.0, 4.0),
                PaintPoint::new(5.0, 6.0),
            )
            .typed_points(),
            RenderedPathCommandPoints::CurveTo {
                control_1: PaintPoint::new(1.0, 2.0),
                control_2: PaintPoint::new(3.0, 4.0),
                end: PaintPoint::new(5.0, 6.0),
            }
        );
    }

    #[test]
    fn stroke_and_line_expose_typed_paint_points() {
        let stroke = RenderedStroke::from_paint_points(
            PaintPoint::new(1.0, 2.0),
            PaintPoint::new(3.0, 4.0),
            1.0,
            Color::BLACK,
            None,
        );
        assert_eq!(
            stroke.paint_points(),
            (PaintPoint::new(1.0, 2.0), PaintPoint::new(3.0, 4.0))
        );

        let line = RenderedLine::from_paint_origin(
            "text".to_string(),
            PaintPoint::new(5.0, 6.0),
            10.0,
            None,
            Color::BLACK,
            Vec::new(),
        );
        assert_eq!(line.origin(), PaintPoint::new(5.0, 6.0));
    }

    #[test]
    fn stroke_line_and_link_expose_typed_paint_bounds() {
        let stroke = RenderedStroke::from_paint_points(
            PaintPoint::new(10.0, 20.0),
            PaintPoint::new(30.0, 40.0),
            4.0,
            Color::BLACK,
            None,
        );
        assert_eq!(stroke.paint_bounds(), PaintClip::new(8.0, 18.0, 24.0, 24.0));

        let link = RenderedLink::from_paint_rect(
            PaintRect::new(PaintPoint::new(1.0, 2.0), PaintSize::new(3.0, 4.0)),
            "https://example.com".to_string(),
        );
        assert_eq!(
            link.paint_rect(),
            PaintRect::new(PaintPoint::new(1.0, 2.0), PaintSize::new(3.0, 4.0))
        );
    }
}

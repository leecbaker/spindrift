use super::*;

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

    pub(crate) fn replace_paint_tree_since_with_fragment(
        &mut self,
        checkpoint: &PaintCheckpoint,
        fragment: PaintFragment,
    ) {
        let Some(checkpoint_tree) = &checkpoint.paint_tree else {
            return;
        };
        let Some(tree) = &mut self.paint_tree else {
            return;
        };
        tree.root.bands = checkpoint_tree.root.bands.clone();
        tree.root.bands.append_bands(fragment.display_list.bands);
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

    pub(crate) fn push_rect_in_band(&mut self, band: PaintBand, rect: RenderedRect) -> usize {
        let (index, operation) = self.record_rect(rect);
        self.operations.push(operation);
        self.push_paint_tree_operation_in_band(band, operation);
        index
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

    pub(crate) fn push_path_in_band(&mut self, band: PaintBand, path: RenderedPath) -> usize {
        let (index, operation) = self.record_path(path);
        self.operations.push(operation);
        self.push_paint_tree_operation_in_band(band, operation);
        index
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
        let mut last_index = None;
        for line in split_rendered_line_at_font_run_boundaries(line) {
            if let Some(index) = self.try_merge_with_previous_line(&line) {
                last_index = Some(index);
                continue;
            }
            let (index, operation) = self.record_line(line);
            self.operations.push(operation);
            self.push_paint_tree_operation_in_band(band, operation);
            last_index = Some(index);
        }
        last_index.expect("at least one rendered line segment")
    }

    pub(in crate::document) fn try_merge_with_previous_line(
        &mut self,
        line: &RenderedLine,
    ) -> Option<usize> {
        let Some(PaintOperation::Line(index)) = self.operations.last().copied() else {
            return None;
        };
        let previous = self.lines.get_mut(index)?;
        if rendered_lines_can_merge_as_inline_continuation(previous, line) {
            previous.append_same_line_continuation(line);
            return Some(index);
        }
        if !rendered_lines_can_merge_with_word_gap(previous, line) {
            return None;
        }
        previous.append_same_line_with_gap(line);
        Some(index)
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
            .with_links(PaintBand::Inline, translated.links);
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

    pub(in crate::document) fn paint_primitive(
        &self,
        operation: &PaintOperation,
    ) -> Option<PaintPrimitive> {
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

    pub(in crate::document) fn push_paint_tree_operation_in_band(
        &mut self,
        band: PaintBand,
        operation: PaintOperation,
    ) {
        if let Some(tree) = &mut self.paint_tree {
            tree.push_operation(band, operation);
        }
    }
}

pub(in crate::document) fn mark_operation_index(
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

pub(in crate::document) fn ensure_all_operations_referenced(
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
    pub(in crate::document) operations: Vec<PaintOperation>,
    pub(in crate::document) paint_tree: Option<PagePaintTree>,
    pub(in crate::document) rects: Vec<RenderedRect>,
    pub(in crate::document) rounded_rects: Vec<RenderedRoundedRect>,
    pub(in crate::document) paths: Vec<RenderedPath>,
    pub(in crate::document) strokes: Vec<RenderedStroke>,
    pub(in crate::document) images: Vec<RenderedImage>,
    pub(in crate::document) lines: Vec<RenderedLine>,
    pub(in crate::document) links: Vec<RenderedLink>,
}

impl PaintCheckpoint {
    pub(in crate::document) fn paint_primitives_for_operations(
        &self,
        operations: &[PaintOperation],
    ) -> Vec<PaintPrimitive> {
        operations
            .iter()
            .filter_map(|operation| self.paint_primitive(operation))
            .collect()
    }

    pub(in crate::document) fn paint_primitive(
        &self,
        operation: &PaintOperation,
    ) -> Option<PaintPrimitive> {
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

    pub(in crate::document) fn clipped_to_rect(self, clip: PaintClip) -> Option<Self> {
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

    pub(in crate::document) fn bounds(&self) -> Option<PaintClip> {
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
    pub(in crate::document) bands: PaintBandList,
}

impl PaintDisplayList {
    pub(in crate::document) fn from_primitives(primitives: Vec<PaintPrimitive>) -> Self {
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

    pub(in crate::document) fn is_empty(&self) -> bool {
        self.bands.is_empty()
    }

    pub(in crate::document) fn flattened_primitives(&self) -> Vec<PaintPrimitive> {
        let mut primitives = Vec::new();
        self.push_flattened_primitives(&mut primitives);
        primitives
    }

    pub(in crate::document) fn push_flattened_primitives(
        &self,
        primitives: &mut Vec<PaintPrimitive>,
    ) {
        self.bands.push_flattened_primitives(primitives);
    }

    pub(crate) fn translated(self, offset: PaintVector) -> Self {
        Self {
            bands: self.bands.translated(offset),
        }
    }

    pub(in crate::document) fn into_operation_nodes(
        self,
        operations: &mut impl Iterator<Item = PaintOperation>,
    ) -> Self {
        Self {
            bands: self.bands.into_operation_nodes(operations),
        }
    }

    pub(in crate::document) fn with_links(
        mut self,
        band: PaintBand,
        links: Vec<RenderedLink>,
    ) -> Self {
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

use super::Page;
use crate::{Color, Error, Result};
use std::borrow::Cow;

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
        x_offset: f32,
        y_offset: f32,
    ) -> RecordedPaintFragment {
        let translated = fragment.clone().translated(x_offset, y_offset);
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

    pub(crate) fn append_paint_fragment(
        &mut self,
        fragment: &PaintFragment,
        x_offset: f32,
        y_offset: f32,
    ) {
        let recorded = self.record_paint_fragment(fragment, x_offset, y_offset);
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
    fn translated(self, x_offset: f32, y_offset: f32) -> Self {
        match self {
            Self::Rect(rect) => Self::Rect(rect.translated(x_offset, y_offset)),
            Self::RoundedRect(rect) => Self::RoundedRect(rect.translated(x_offset, y_offset)),
            Self::Path(path) => Self::Path(path.translated(x_offset, y_offset)),
            Self::Stroke(stroke) => Self::Stroke(stroke.translated(x_offset, y_offset)),
            Self::Image(image) => Self::Image(image.translated(x_offset, y_offset)),
            Self::Line(line) => Self::Line(line.translated(x_offset, y_offset)),
        }
    }

    fn bounds(&self) -> Option<PaintClip> {
        match self {
            Self::Rect(rect) => rect_bounds(rect.x, rect.y, rect.width, rect.height),
            Self::RoundedRect(rect) => rect_bounds(rect.x, rect.y, rect.width, rect.height),
            Self::Image(image) => rect_bounds(image.x, image.y, image.width, image.height),
            Self::Stroke(stroke) => {
                let half = stroke.width / 2.0;
                let left = stroke.x1.min(stroke.x2) - half;
                let right = stroke.x1.max(stroke.x2) + half;
                let bottom = stroke.y1.min(stroke.y2) - half;
                let top = stroke.y1.max(stroke.y2) + half;
                rect_bounds(left, bottom, right - left, top - bottom)
            }
            Self::Line(line) => {
                let width = rendered_line_width(line);
                rect_bounds(
                    line.x,
                    line.y - line.font_size,
                    width,
                    line.font_size * 1.35,
                )
            }
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

    fn translated(self, x_offset: f32, y_offset: f32) -> Self {
        Self {
            bands: self.bands.translated(x_offset, y_offset),
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
    pub(crate) const ORDER: [Self; 8] = [
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
            Self::BackgroundBorder => 0,
            Self::NegativeZ => 1,
            Self::InFlowBlock => 2,
            Self::Float => 3,
            Self::Inline => 4,
            Self::AutoZeroZ => 5,
            Self::PositiveZ => 6,
            Self::Outline => 7,
        }
    }
}

/// Ordered paint-band buckets for a fragment-local display list.
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct PaintBandList {
    pub(crate) bands: [Vec<PaintDisplayItem>; 8],
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

    fn translated(self, x_offset: f32, y_offset: f32) -> Self {
        Self {
            bands: self.bands.map(|items| {
                items
                    .into_iter()
                    .map(|item| item.translated(x_offset, y_offset))
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
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum PaintDisplayItem {
    Operation(PaintOperation),
    Primitive(PaintPrimitive),
    StackingContext(PaintStackingContext),
    Link(RenderedLink),
}

#[allow(dead_code)]
impl PaintDisplayItem {
    fn translated(self, x_offset: f32, y_offset: f32) -> Self {
        match self {
            Self::Operation(operation) => Self::Operation(operation),
            Self::Primitive(primitive) => Self::Primitive(primitive.translated(x_offset, y_offset)),
            Self::StackingContext(context) => {
                Self::StackingContext(context.translated(x_offset, y_offset))
            }
            Self::Link(link) => Self::Link(link.translated(x_offset, y_offset)),
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
    fn from_z_index(z_index: i32) -> Self {
        Self::Integer(z_index)
    }

    fn paint_band(self) -> PaintBand {
        match self {
            Self::Integer(value) if value < 0 => PaintBand::NegativeZ,
            Self::Integer(value) if value > 0 => PaintBand::PositiveZ,
            Self::Auto | Self::Integer(0) => PaintBand::AutoZeroZ,
            Self::Integer(_) => PaintBand::AutoZeroZ,
        }
    }

    fn sort_key(self) -> (i32, i32) {
        match self {
            Self::Integer(value) => (value, 1),
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
}

impl Default for PaintEffects {
    fn default() -> Self {
        Self {
            opacity: 1.0,
            transform: None,
            overflow_clip: None,
            absolute_clip: None,
        }
    }
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

    pub(crate) const fn translate(x: f32, y: f32) -> Self {
        Self {
            a: 1.0,
            b: 0.0,
            c: 0.0,
            d: 1.0,
            e: x,
            f: y,
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

    pub(crate) fn apply_point(self, x: f32, y: f32) -> (f32, f32) {
        (
            self.a * x + self.c * y + self.e,
            self.b * x + self.d * y + self.f,
        )
    }

    pub(crate) fn apply_clip_to_aabb(self, clip: PaintClip) -> PaintClip {
        let points = [
            self.apply_point(clip.x, clip.y),
            self.apply_point(clip.x + clip.width, clip.y),
            self.apply_point(clip.x, clip.y + clip.height),
            self.apply_point(clip.x + clip.width, clip.y + clip.height),
        ];
        let min_x = points.iter().map(|(x, _)| *x).fold(f32::INFINITY, f32::min);
        let max_x = points
            .iter()
            .map(|(x, _)| *x)
            .fold(f32::NEG_INFINITY, f32::max);
        let min_y = points.iter().map(|(_, y)| *y).fold(f32::INFINITY, f32::min);
        let max_y = points
            .iter()
            .map(|(_, y)| *y)
            .fold(f32::NEG_INFINITY, f32::max);
        PaintClip {
            x: min_x,
            y: min_y,
            width: (max_x - min_x).max(0.0),
            height: (max_y - min_y).max(0.0),
        }
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
    pub(crate) x: f32,
    pub(crate) y: f32,
    pub(crate) width: f32,
    pub(crate) height: f32,
}

impl PaintClip {
    fn translated(mut self, x_offset: f32, y_offset: f32) -> Self {
        self.x += x_offset;
        self.y += y_offset;
        self
    }

    fn union(self, other: Self) -> Self {
        let left = self.x.min(other.x);
        let right = (self.x + self.width).max(other.x + other.width);
        let bottom = self.y.min(other.y);
        let top = (self.y + self.height).max(other.y + other.height);
        Self {
            x: left,
            y: bottom,
            width: (right - left).max(0.0),
            height: (top - bottom).max(0.0),
        }
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
        let mut bands = PaintBandList::default();
        bands.extend_band(
            PaintBand::BackgroundBorder,
            content.display_list.bands.into_items_in_order(),
        );
        for context in child_contexts {
            bands.push_context(context);
        }
        bands.sort_stacking_contexts();
        Self::with_bands(StackLevel::from_z_index(z_index), bands)
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

    fn push_flattened_primitives(&self, primitives: &mut Vec<PaintPrimitive>) {
        self.bands.push_flattened_primitives(primitives);
    }

    fn translated(self, x_offset: f32, y_offset: f32) -> Self {
        Self {
            source_order: self.source_order,
            stack_level: self.stack_level,
            bands: self.bands.translated(x_offset, y_offset),
            effects: self.effects,
            bounds: self
                .bounds
                .map(|bounds| bounds.translated(x_offset, y_offset)),
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

    pub(crate) fn from_primitives_in_band(
        band: PaintBand,
        primitives: Vec<PaintPrimitive>,
        links: Vec<RenderedLink>,
    ) -> Self {
        let mut bands = PaintBandList::default();
        bands.extend_band(
            band,
            primitives.into_iter().map(PaintDisplayItem::Primitive),
        );
        Self {
            display_list: PaintDisplayList { bands },
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
                PaintPrimitive::Line(line) => Some(line.y),
                _ => None,
            })
    }

    pub(crate) fn last_line_y(&self) -> Option<f32> {
        self.flattened_primitives()
            .into_iter()
            .rev()
            .find_map(|primitive| match primitive {
                PaintPrimitive::Line(line) => Some(line.y),
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
            let link_bounds = PaintClip {
                x: link.x,
                y: link.y,
                width: link.width,
                height: link.height,
            };
            bounds = Some(match bounds {
                Some(existing) => existing.union(link_bounds),
                None => link_bounds,
            });
        }
        bounds
    }

    pub(crate) fn translated(mut self, x_offset: f32, y_offset: f32) -> Self {
        self.display_list = self.display_list.translated(x_offset, y_offset);
        self.links = self
            .links
            .into_iter()
            .map(|link| link.translated(x_offset, y_offset))
            .collect();
        self
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
    (left.x - right.x).abs() < 0.001
        && (left.y - right.y).abs() < 0.001
        && (left.width - right.width).abs() < 0.001
        && (left.height - right.height).abs() < 0.001
}

#[derive(Debug, Clone, PartialEq)]
pub struct RenderedRect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    pub fill: Option<Color>,
    pub stroke: Option<Color>,
    pub stroke_width: f32,
}

impl RenderedRect {
    fn translated(mut self, x_offset: f32, y_offset: f32) -> Self {
        self.x += x_offset;
        self.y += y_offset;
        self
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RenderedRoundedRect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    pub radii: RenderedRoundedRectRadii,
    pub fill: Option<Color>,
    pub stroke: Option<Color>,
    pub stroke_width: f32,
}

impl RenderedRoundedRect {
    fn translated(mut self, x_offset: f32, y_offset: f32) -> Self {
        self.x += x_offset;
        self.y += y_offset;
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
    fn translated(mut self, x_offset: f32, y_offset: f32) -> Self {
        if let Some(clip) = &mut self.clip {
            for command in &mut clip.commands {
                command.translate(x_offset, y_offset);
            }
            for nested_clip in &mut clip.additional_clips {
                for command in &mut nested_clip.commands {
                    command.translate(x_offset, y_offset);
                }
            }
        }
        for command in &mut self.commands {
            command.translate(x_offset, y_offset);
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

/// A PDF-compatible path construction command.
///
/// The variants map directly to PDF `m`, `l`, `c`, and `h` operators from ISO
/// 32000-1:2008, 8.5.2 "Path Construction Operators".
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum RenderedPathCommand {
    MoveTo(f32, f32),
    LineTo(f32, f32),
    CurveTo {
        x1: f32,
        y1: f32,
        x2: f32,
        y2: f32,
        x3: f32,
        y3: f32,
    },
    Close,
}

impl RenderedPathCommand {
    fn translate(&mut self, x_offset: f32, y_offset: f32) {
        match self {
            Self::MoveTo(x, y) | Self::LineTo(x, y) => {
                *x += x_offset;
                *y += y_offset;
            }
            Self::CurveTo {
                x1,
                y1,
                x2,
                y2,
                x3,
                y3,
            } => {
                *x1 += x_offset;
                *y1 += y_offset;
                *x2 += x_offset;
                *y2 += y_offset;
                *x3 += x_offset;
                *y3 += y_offset;
            }
            Self::Close => {}
        }
    }
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
    pub x: f32,
    pub y: f32,
}

impl RenderedCornerRadius {
    pub const ZERO: Self = Self { x: 0.0, y: 0.0 };
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RenderedStroke {
    pub x1: f32,
    pub y1: f32,
    pub x2: f32,
    pub y2: f32,
    pub width: f32,
    pub color: Color,
    pub dash: Option<(f32, f32)>,
}

impl RenderedStroke {
    fn translated(mut self, x_offset: f32, y_offset: f32) -> Self {
        self.x1 += x_offset;
        self.y1 += y_offset;
        self.x2 += x_offset;
        self.y2 += y_offset;
        self
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct RenderedLine {
    pub text: String,
    pub x: f32,
    pub y: f32,
    pub font_size: f32,
    pub font_id: Option<usize>,
    pub color: Color,
    pub runs: Vec<RenderedTextRun>,
}

impl RenderedLine {
    fn translated(mut self, x_offset: f32, y_offset: f32) -> Self {
        self.x += x_offset;
        self.y += y_offset;
        self
    }
}

fn rendered_line_width(line: &RenderedLine) -> f32 {
    line.runs.iter().fold(0.0_f32, |width, run| {
        let run_width = run
            .glyphs
            .as_ref()
            .map(|glyphs| glyphs.iter().map(|glyph| glyph.x_advance).sum::<f32>())
            .unwrap_or_else(|| run.text.chars().count() as f32 * run.font_size * 0.5);
        width.max(run.x_offset + run_width)
    })
}

fn rect_bounds(x: f32, y: f32, width: f32, height: f32) -> Option<PaintClip> {
    (width > 0.0 && height > 0.0).then_some(PaintClip {
        x,
        y,
        width,
        height,
    })
}

fn path_bounds(path: &RenderedPath) -> Option<PaintClip> {
    let mut bounds: Option<PaintClip> = None;
    for command in &path.commands {
        for (x, y) in command_points(*command) {
            let point = PaintClip {
                x,
                y,
                width: 0.0,
                height: 0.0,
            };
            bounds = Some(match bounds {
                Some(existing) => existing.union(point),
                None => point,
            });
        }
    }
    bounds.map(|mut bounds| {
        let outset = path.stroke_width.max(0.0) / 2.0;
        bounds.x -= outset;
        bounds.y -= outset;
        bounds.width += outset * 2.0;
        bounds.height += outset * 2.0;
        bounds
    })
}

fn command_points(command: RenderedPathCommand) -> Vec<(f32, f32)> {
    match command {
        RenderedPathCommand::MoveTo(x, y) | RenderedPathCommand::LineTo(x, y) => vec![(x, y)],
        RenderedPathCommand::CurveTo {
            x1,
            y1,
            x2,
            y2,
            x3,
            y3,
        } => vec![(x1, y1), (x2, y2), (x3, y3)],
        RenderedPathCommand::Close => Vec::new(),
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
    pub font_size: f32,
    pub font_id: Option<usize>,
    pub glyphs: Option<Vec<RenderedGlyph>>,
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
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    pub target: String,
}

impl RenderedLink {
    fn translated(mut self, x_offset: f32, y_offset: f32) -> Self {
        self.x += x_offset;
        self.y += y_offset;
        self
    }

    fn transformed(&self, transform: PaintTransform) -> Self {
        let clip = transform.apply_clip_to_aabb(PaintClip {
            x: self.x,
            y: self.y,
            width: self.width,
            height: self.height,
        });
        Self {
            x: clip.x,
            y: clip.y,
            width: clip.width,
            height: clip.height,
            target: self.target.clone(),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct RenderedImage {
    pub background: bool,
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    pub pixel_width: u32,
    pub pixel_height: u32,
    pub source_rect: Option<RenderedImageSourceRect>,
    pub interpolate: bool,
    pub rgb: Vec<u8>,
    pub alpha: Option<Vec<u8>>,
    pub alt_text: Option<String>,
}

impl RenderedImage {
    fn translated(mut self, x_offset: f32, y_offset: f32) -> Self {
        self.x += x_offset;
        self.y += y_offset;
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

use std::borrow::Cow;
use std::rc::Rc;

use super::annotations::RenderedLink;
use super::display_list::{PagePaintTree, PaintBand, PaintDisplayItem, PaintDisplayList};
use super::effects::{PaintEffectScope, PaintEffects};
use super::fragments::{PaintFragment, RecordedPaintFragment};
use super::geometry::{
    PaintClip, PaintPoint, PaintTransform, PaintTranslation, Projective3dPaintTransform,
    rect_bounds,
};
use super::images::RenderedImage;
use super::paths::{
    RenderedPath, RenderedPathClip, RenderedPathFillRule, paint_rect_path_commands, path_bounds,
};
use super::patterns::{RenderedGradientPattern, RenderedImagePattern, RenderedSvgPattern};
use super::shapes::{RenderedRect, RenderedRoundedRect, RenderedStroke};
use super::stacking::PaintStackingContext;
use super::text::{
    RenderedLine, RenderedTextPaintSegment, rendered_lines_can_merge_as_exact_paint_continuation,
    rendered_lines_can_merge_as_inline_continuation,
    rendered_lines_can_merge_as_tracking_continuation, rendered_lines_can_merge_with_word_gap,
    split_rendered_line_at_font_run_boundaries,
};
use crate::document::Page;
use crate::{Error, Result};

impl Page {
    /// Paint a document-canvas rectangle beneath every document stacking
    /// context band.
    ///
    /// The propagated root/body background is canvas paint, rather than a
    /// background of the root element's ordinary box. Keep it in the page
    /// paint tree even when a later opaque primitive covers it: layout tests,
    /// accessibility inspection, and alternative backends observe this tree
    /// as the CSS paint model, not as a PDF-only optimization:
    /// <https://www.w3.org/TR/css-backgrounds-3/#special-backgrounds> and
    /// <https://www.w3.org/TR/CSS22/zindex.html>.
    pub(crate) fn push_document_canvas_rect(&mut self, rect: RenderedRect) {
        // A propagated root/body background is the document canvas, below
        // ordinary root descendants and their backgrounds. PageBackground is
        // the first document paint band, so adding it after layout cannot
        // accidentally cover already-recorded in-flow content.
        // <https://www.w3.org/TR/css-backgrounds-3/#special-backgrounds>
        self.push_rect_in_band(PaintBand::PageBackground, rect);
    }

    /// Capture a stable insertion position in one root paint band.
    ///
    /// Some non-stacking boxes own paint that belongs between their own
    /// decoration and descendant content. In particular, CSS Multicol says a
    /// column rule is painted just above its multicol container's border while
    /// column boxes remain in the same stacking context. Capturing the current
    /// band length lets layout insert that local paint after descendants have
    /// been laid out, without manufacturing a stacking context or moving the
    /// descendants into an atomic subtree:
    /// <https://www.w3.org/TR/css-multicol-1/#column-gaps-and-rules> and
    /// <https://www.w3.org/TR/css-multicol-1/#stacking-context>.
    pub(crate) fn paint_band_insertion_point(&self, band: PaintBand) -> PaintBandInsertionPoint {
        PaintBandInsertionPoint {
            band,
            item_index: self.paint_tree.root.bands.bands[band.index()].len(),
        }
    }

    /// Insert page-local primitives at a previously captured paint position.
    ///
    /// Primitive arrays remain append-only; only their operation nodes move in
    /// paint order.
    pub(crate) fn insert_primitives_at_paint_band_point(
        &mut self,
        point: PaintBandInsertionPoint,
        primitives: impl IntoIterator<Item = PaintPrimitive>,
    ) {
        let operations = primitives
            .into_iter()
            .map(|primitive| self.record_paint_primitive(primitive))
            .collect::<Vec<_>>();
        if operations.is_empty() {
            return;
        }
        let band = &mut self.paint_tree.root.bands.bands[point.band.index()];
        debug_assert!(
            point.item_index <= band.len(),
            "paint insertion point must belong to the active page"
        );
        band.splice(
            point.item_index.min(band.len())..point.item_index.min(band.len()),
            operations.into_iter().map(PaintDisplayItem::Operation),
        );
    }

    pub(crate) fn paint_checkpoint(&self) -> PaintCheckpoint {
        PaintCheckpoint {
            paint_tree: self.paint_tree.clone(),
            rects: self.rects.clone(),
            rounded_rects: self.rounded_rects.clone(),
            paths: self.paths.clone(),
            strokes: self.strokes.clone(),
            images: self.images.clone(),
            svg_pattern_images: self.svg_pattern_images.clone(),
            image_patterns: self.image_patterns.clone(),
            gradient_patterns: self.gradient_patterns.clone(),
            svg_patterns: self.svg_patterns.clone(),
            opaque_text_coverages: self.opaque_text_coverages.clone(),
            svg_text_outlines: self.svg_text_outlines.clone(),
            lines: self.lines.clone(),
            links: self.links.clone(),
        }
    }

    pub(crate) fn take_paint_fragment_since(
        &mut self,
        checkpoint: PaintCheckpoint,
    ) -> PaintFragment {
        let fragment = self.paint_tree.fragment_since(&checkpoint.paint_tree, self);
        self.paint_tree = checkpoint.paint_tree;
        self.rects = checkpoint.rects;
        self.rounded_rects = checkpoint.rounded_rects;
        self.paths = checkpoint.paths;
        self.strokes = checkpoint.strokes;
        self.images = checkpoint.images;
        self.svg_pattern_images = checkpoint.svg_pattern_images;
        self.image_patterns = checkpoint.image_patterns;
        self.gradient_patterns = checkpoint.gradient_patterns;
        self.svg_patterns = checkpoint.svg_patterns;
        self.opaque_text_coverages = checkpoint.opaque_text_coverages;
        self.svg_text_outlines = checkpoint.svg_text_outlines;
        self.lines = checkpoint.lines;
        self.links = checkpoint.links;
        fragment
    }

    /// Translate the concrete primitives recorded after a checkpoint while
    /// preserving their existing paint-tree operation nodes.
    ///
    /// A recorded display-list operation indexes this page's primitive arrays,
    /// so translating an extracted fragment alone cannot move it: operation
    /// nodes intentionally carry no geometry. This adapter changes the
    /// appended primitive suffix in place, retaining its paint order and
    /// operation identity.
    pub(crate) fn translate_recorded_primitives_since(
        &mut self,
        checkpoint: &PaintCheckpoint,
        offset: PaintTranslation,
    ) {
        for rect in &mut self.rects[checkpoint.rects.len()..] {
            *rect = rect.clone().translated(offset);
        }
        for rect in &mut self.rounded_rects[checkpoint.rounded_rects.len()..] {
            *rect = (*rect).translated(offset);
        }
        for path in &mut self.paths[checkpoint.paths.len()..] {
            *path = path.clone().translated(offset);
        }
        for stroke in &mut self.strokes[checkpoint.strokes.len()..] {
            *stroke = (*stroke).translated(offset);
        }
        for image in &mut self.images[checkpoint.images.len()..] {
            *image = image.clone().translated(offset);
        }
        for pattern in &mut self.image_patterns[checkpoint.image_patterns.len()..] {
            *pattern = pattern
                .clone()
                .translated_geometry_preserving_tile_origin(offset);
        }
        for pattern in &mut self.gradient_patterns[checkpoint.gradient_patterns.len()..] {
            *pattern = pattern
                .clone()
                .translated_geometry_preserving_tile_origin(offset);
        }
        for pattern in &mut self.svg_patterns[checkpoint.svg_patterns.len()..] {
            *pattern = pattern
                .clone()
                .translated_geometry_preserving_tile_origin(offset);
        }
        for line in &mut self.lines[checkpoint.lines.len()..] {
            *line = line.clone().translated(offset);
        }
        for link in &mut self.links[checkpoint.links.len()..] {
            *link = link.clone().translated(offset);
        }
    }

    pub(crate) fn paint_tree_fragment_since(&self, checkpoint: &PaintCheckpoint) -> PaintFragment {
        PaintFragment {
            display_list: PaintDisplayList {
                bands: self
                    .paint_tree
                    .operation_node_fragment_since(&checkpoint.paint_tree),
            },
            links: Vec::new(),
        }
    }

    pub(crate) fn replace_paint_tree_since_with_context(
        &mut self,
        checkpoint: &PaintCheckpoint,
        band: PaintBand,
        context: PaintStackingContext,
    ) {
        self.paint_tree.root.bands = checkpoint.paint_tree.root.bands.clone();
        self.paint_tree
            .root
            .bands
            .push_context_in_band(band, context);
        // Captured atomic fragments can be committed after later source
        // siblings (notably an overhanging nested float). Restore the
        // per-band stacking-context order at the commit boundary rather than
        // relying on capture completion order.
        self.paint_tree.sort_stacking_contexts();
        self.links = self.paint_tree.transformed_links(self);
    }

    pub(crate) fn replace_paint_tree_since_with_fragment(
        &mut self,
        checkpoint: &PaintCheckpoint,
        fragment: PaintFragment,
    ) {
        self.paint_tree.root.bands = checkpoint.paint_tree.root.bands.clone();
        self.paint_tree
            .root
            .bands
            .append_bands(fragment.display_list.bands);
        self.links = self.paint_tree.transformed_links(self);
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

    /// Insert paint primitives that may still be translated by an enclosing
    /// fragmentation replay before they become page-owned resources.
    ///
    /// Fragmentation moves CSS background positioning areas together with
    /// their painting clips. Retaining primitives here lets the final replay
    /// update tiled-image phase before PDF resource materialization.
    /// <https://www.w3.org/TR/css-break-3/#break-decoration>
    pub(crate) fn prepend_primitives_to_fragment(
        &mut self,
        fragment: &mut PaintFragment,
        band: PaintBand,
        primitives: impl IntoIterator<Item = PaintPrimitive>,
    ) {
        let items = primitives
            .into_iter()
            .map(PaintDisplayItem::Primitive)
            .collect::<Vec<_>>();
        fragment.display_list.bands.bands[band.index()].splice(0..0, items);
    }

    /// Append paint primitives that remain transformable until the enclosing
    /// fragment is committed to its final page destination.
    pub(crate) fn append_primitives_to_fragment(
        &mut self,
        fragment: &mut PaintFragment,
        band: PaintBand,
        primitives: impl IntoIterator<Item = PaintPrimitive>,
    ) {
        fragment.display_list.bands.extend_band(
            band,
            primitives.into_iter().map(PaintDisplayItem::Primitive),
        );
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
        self.paint_tree.clear();
        self.rects.clear();
        self.rounded_rects.clear();
        self.paths.clear();
        self.strokes.clear();
        self.images.clear();
        self.svg_pattern_images.clear();
        self.image_patterns.clear();
        self.gradient_patterns.clear();
        self.svg_patterns.clear();
        self.opaque_text_coverages.clear();
        self.svg_text_outlines.clear();
        self.lines.clear();
        self.links.clear();
        fragment
    }

    pub(crate) fn push_rect_in_band(&mut self, band: PaintBand, rect: RenderedRect) -> usize {
        let (index, operation) = self.record_rect(rect);
        self.push_paint_tree_operation_in_band(band, operation);
        index
    }

    pub(crate) fn push_rounded_rect_in_band(
        &mut self,
        band: PaintBand,
        rect: RenderedRoundedRect,
    ) -> usize {
        let (index, operation) = self.record_rounded_rect(rect);
        self.push_paint_tree_operation_in_band(band, operation);
        index
    }

    pub(crate) fn push_path_in_band(&mut self, band: PaintBand, path: RenderedPath) -> usize {
        let (index, operation) = self.record_path(path);
        self.push_paint_tree_operation_in_band(band, operation);
        index
    }

    /// Record an SVG compositing subtree without flattening its opacity or
    /// blend boundaries into individual paths.
    pub(crate) fn push_svg_group_in_band(
        &mut self,
        band: PaintBand,
        group: crate::svg::SvgPaintGroup,
    ) {
        let scope = self.record_svg_group(group);
        self.paint_tree
            .root
            .bands
            .push_effect_scope_in_band(band, scope);
    }

    fn record_svg_group(&mut self, group: crate::svg::SvgPaintGroup) -> PaintEffectScope {
        let items = group
            .items
            .into_iter()
            .map(|item| match item {
                crate::svg::SvgPaintItem::Path(path) => {
                    let operation = self.record_path(*path).1;
                    PaintDisplayItem::Operation(operation)
                }
                crate::svg::SvgPaintItem::RasterImage(image) => {
                    let operation = self.record_image(*image).1;
                    PaintDisplayItem::Operation(operation)
                }
                crate::svg::SvgPaintItem::OutlinedText(outlined) => {
                    PaintDisplayItem::Operation(self.record_svg_text_outline(*outlined).1)
                }
                crate::svg::SvgPaintItem::Group(group)
                | crate::svg::SvgPaintItem::NestedSvg(group) => {
                    PaintDisplayItem::EffectScope(self.record_svg_group(*group))
                }
            })
            .collect();
        PaintEffectScope::new(
            PaintEffects {
                opacity: group.opacity,
                blend_mode: group.blend_mode,
                isolation: group.isolation,
                ..PaintEffects::default()
            },
            group.bounds,
            items,
        )
    }

    pub(crate) fn push_stroke_in_band(&mut self, band: PaintBand, stroke: RenderedStroke) -> usize {
        let (index, operation) = self.record_stroke(stroke);
        self.push_paint_tree_operation_in_band(band, operation);
        index
    }

    pub(crate) fn push_line(&mut self, line: RenderedLine) -> usize {
        self.push_line_in_band(PaintBand::InFlowBlock, line)
    }

    pub(crate) fn push_line_in_band(&mut self, band: PaintBand, line: RenderedLine) -> usize {
        let mut last_index = None;
        for line in split_rendered_line_at_font_run_boundaries(line) {
            if let Some(index) = self.try_merge_with_previous_line(band, &line) {
                last_index = Some(index);
                continue;
            }
            let (index, operation) = self.record_line(line);
            self.push_paint_tree_operation_in_band(band, operation);
            last_index = Some(index);
        }
        last_index.expect("at least one rendered line segment")
    }

    /// Record a shaped line inside a rectangular overflow clip scope.
    ///
    /// Text operators paint whole glyph runs, so a partially visible line
    /// must retain its original shaping and be clipped by the PDF graphics
    /// state rather than being discarded by layout. Keeping the scope in the
    /// original paint band preserves CSS 2.2 Appendix E ordering:
    /// <https://www.w3.org/TR/css-overflow-3/#overflow-clip-edge> and
    /// <https://www.w3.org/TR/CSS22/zindex.html>.
    pub(crate) fn push_line_clipped_in_band(
        &mut self,
        band: PaintBand,
        line: RenderedLine,
        clip: PaintClip,
    ) -> usize {
        let mut last_index = None;
        for line in split_rendered_line_at_font_run_boundaries(line) {
            let (index, operation) = self.record_line(line);
            self.paint_tree.root.bands.push_effect_scope_in_band(
                band,
                PaintEffectScope::new(
                    PaintEffects::transparent_overflow_scope(
                        super::contours::OverflowClipEffect::Rect(clip),
                    ),
                    Some(clip),
                    vec![PaintDisplayItem::Operation(operation)],
                ),
            );
            last_index = Some(index);
        }
        last_index.expect("at least one rendered line segment")
    }

    pub(in crate::document) fn try_merge_with_previous_line(
        &mut self,
        band: PaintBand,
        line: &RenderedLine,
    ) -> Option<usize> {
        let Some(PaintDisplayItem::Operation(PaintOperation::Line(index))) =
            self.paint_tree.root.bands.bands[band.index()].last()
        else {
            return None;
        };
        let previous = self.lines.get_mut(*index)?;
        if rendered_lines_can_merge_as_exact_paint_continuation(previous, line) {
            previous.append_same_line_continuation(line);
            return Some(*index);
        }
        if rendered_lines_can_merge_as_inline_continuation(previous, line) {
            previous.append_same_line_continuation(line);
            return Some(*index);
        }
        if rendered_lines_can_merge_as_tracking_continuation(previous, line) {
            previous.append_same_line_continuation(line);
            return Some(*index);
        }
        if !rendered_lines_can_merge_with_word_gap(previous, line) {
            return None;
        }
        previous.append_same_line_with_gap(line);
        Some(*index)
    }

    pub(crate) fn push_image_in_band(&mut self, band: PaintBand, image: RenderedImage) -> usize {
        let (index, operation) = self.record_image(image);
        self.push_paint_tree_operation_in_band(band, operation);
        index
    }

    pub(crate) fn push_opaque_text_coverage_in_band(
        &mut self,
        band: PaintBand,
        line: RenderedLine,
        paths: Vec<RenderedPath>,
    ) -> usize {
        debug_assert!(
            !paths.is_empty() && paths.iter().all(|path| path.opaque_coverage_rect.is_some())
        );
        let painted_glyph_count = line
            .runs
            .iter()
            .filter_map(|run| run.glyphs.as_ref())
            .flat_map(|glyphs| glyphs.iter())
            .filter(|glyph| glyph.painted_id().is_some())
            .count();
        debug_assert!(
            painted_glyph_count == 0 || painted_glyph_count == paths.len(),
            "an opaque text coverage record must own every paintable glyph in its line"
        );
        let (line_index, _) = self.record_line(line);
        let path_indices = paths
            .into_iter()
            .map(|path| self.record_path(path).0)
            .collect();
        let coverage_index = self.opaque_text_coverages.len();
        self.opaque_text_coverages.push(OpaqueTextCoverage {
            line_index,
            path_indices,
        });
        self.push_paint_tree_operation_in_band(
            band,
            PaintOperation::OpaqueTextCoverage(coverage_index),
        );
        line_index
    }

    /// Insert a fragment-replayed SVG text outline without discarding its
    /// retained SVG compositing subtree.
    pub(crate) fn push_svg_text_outline_scope_in_band(
        &mut self,
        band: PaintBand,
        content: crate::document::paint::effects::PaintEffectScope,
        actual_text: Rc<str>,
    ) -> usize {
        let (index, operation) = self.record_svg_text_outline_scope(content, actual_text);
        self.push_paint_tree_operation_in_band(band, operation);
        index
    }

    /// Record ordered PDF text-paint slices without changing the CSS line's
    /// layout or decoration ownership.
    pub(crate) fn push_text_paint_segments_in_band(
        &mut self,
        band: PaintBand,
        segments: Vec<RenderedTextPaintSegment>,
    ) {
        for segment in segments {
            match segment {
                RenderedTextPaintSegment::Text(line) => {
                    self.push_line_in_band(band, line);
                }
                RenderedTextPaintSegment::OpaqueCoverage { line, paths } => {
                    self.push_opaque_text_coverage_in_band(band, line, paths);
                }
            }
        }
    }

    pub(crate) fn push_image_pattern_in_band(
        &mut self,
        band: PaintBand,
        pattern: RenderedImagePattern,
    ) -> usize {
        let (index, operation) = self.record_image_pattern(pattern);
        self.push_paint_tree_operation_in_band(band, operation);
        index
    }

    pub(crate) fn push_gradient_pattern_in_band(
        &mut self,
        band: PaintBand,
        pattern: RenderedGradientPattern,
    ) -> usize {
        let (index, operation) = self.record_gradient_pattern(pattern);
        self.push_paint_tree_operation_in_band(band, operation);
        index
    }

    pub(crate) fn push_svg_pattern_in_band(
        &mut self,
        band: PaintBand,
        pattern: RenderedSvgPattern,
    ) -> usize {
        let (index, operation) = self.record_svg_pattern(pattern);
        self.push_paint_tree_operation_in_band(band, operation);
        index
    }

    pub(crate) fn push_link(&mut self, link: RenderedLink) {
        self.links.push(link.clone());
        self.paint_tree.push_link(PaintBand::Inline, link);
    }

    /// Return a flattened inspection view of the CSS paint tree.
    ///
    /// CSS 2.2 Appendix E defines painting as an ordered sequence, and PDF content
    /// streams preserve visible stacking by serializing drawing operators in that
    /// order. The retained tree is authoritative; this projection intentionally
    /// does not provide an alternative serialization path.
    pub(crate) fn paint_operations(&self) -> Cow<'_, [PaintOperation]> {
        Cow::Owned(self.paint_tree.flattened_operations())
    }

    pub(crate) fn validate_paint_operations(&self, page_index: usize) -> Result<()> {
        let mut rects_seen = vec![false; self.rects.len()];
        let mut rounded_rects_seen = vec![false; self.rounded_rects.len()];
        let mut paths_seen = vec![false; self.paths.len()];
        let mut strokes_seen = vec![false; self.strokes.len()];
        let mut images_seen = vec![false; self.images.len()];
        let mut image_patterns_seen = vec![false; self.image_patterns.len()];
        let mut gradient_patterns_seen = vec![false; self.gradient_patterns.len()];
        let mut svg_patterns_seen = vec![false; self.svg_patterns.len()];
        let mut lines_seen = vec![false; self.lines.len()];
        let mut opaque_text_coverages_seen = vec![false; self.opaque_text_coverages.len()];
        let mut svg_text_outlines_seen = vec![false; self.svg_text_outlines.len()];

        let mut operations = self.paint_operations().into_owned();
        let mut operation_index = 0;
        while let Some(&operation) = operations.get(operation_index) {
            match operation {
                PaintOperation::Rect(index) => mark_operation_index(
                    &mut rects_seen,
                    index,
                    self.rects.len(),
                    page_index,
                    operation_index,
                    "rect",
                )?,
                PaintOperation::Stroke(index) => mark_operation_index(
                    &mut strokes_seen,
                    index,
                    self.strokes.len(),
                    page_index,
                    operation_index,
                    "stroke",
                )?,
                PaintOperation::RoundedRect(index) => mark_operation_index(
                    &mut rounded_rects_seen,
                    index,
                    self.rounded_rects.len(),
                    page_index,
                    operation_index,
                    "rounded rect",
                )?,
                PaintOperation::Path(index) => mark_operation_index(
                    &mut paths_seen,
                    index,
                    self.paths.len(),
                    page_index,
                    operation_index,
                    "path",
                )?,
                PaintOperation::Image(index) => mark_operation_index(
                    &mut images_seen,
                    index,
                    self.images.len(),
                    page_index,
                    operation_index,
                    "image",
                )?,
                PaintOperation::ImagePattern(index) => mark_operation_index(
                    &mut image_patterns_seen,
                    index,
                    self.image_patterns.len(),
                    page_index,
                    operation_index,
                    "image pattern",
                )?,
                PaintOperation::GradientPattern(index) => mark_operation_index(
                    &mut gradient_patterns_seen,
                    index,
                    self.gradient_patterns.len(),
                    page_index,
                    operation_index,
                    "gradient pattern",
                )?,
                PaintOperation::SvgPattern(index) => mark_operation_index(
                    &mut svg_patterns_seen,
                    index,
                    self.svg_patterns.len(),
                    page_index,
                    operation_index,
                    "SVG pattern",
                )?,
                PaintOperation::Line(index) => mark_operation_index(
                    &mut lines_seen,
                    index,
                    self.lines.len(),
                    page_index,
                    operation_index,
                    "line",
                )?,
                PaintOperation::OpaqueTextCoverage(index) => {
                    mark_operation_index(
                        &mut opaque_text_coverages_seen,
                        index,
                        self.opaque_text_coverages.len(),
                        page_index,
                        operation_index,
                        "opaque text coverage",
                    )?;
                    let coverage = self.opaque_text_coverages.get(index).ok_or_else(|| {
                        Error::InvalidInput(format!(
                            "page {} paint operation {} references missing opaque text coverage {}",
                            page_index + 1,
                            operation_index,
                            index
                        ))
                    })?;
                    mark_operation_index(
                        &mut lines_seen,
                        coverage.line_index,
                        self.lines.len(),
                        page_index,
                        operation_index,
                        "line",
                    )?;
                    for path_index in &coverage.path_indices {
                        mark_operation_index(
                            &mut paths_seen,
                            *path_index,
                            self.paths.len(),
                            page_index,
                            operation_index,
                            "path",
                        )?;
                    }
                }
                PaintOperation::SvgTextOutline(index) => {
                    mark_operation_index(
                        &mut svg_text_outlines_seen,
                        index,
                        self.svg_text_outlines.len(),
                        page_index,
                        operation_index,
                        "SVG text outline",
                    )?;
                    let outline = self.svg_text_outlines.get(index).ok_or_else(|| {
                        Error::InvalidInput(format!(
                            "page {} paint operation {} references missing SVG text outline {}",
                            page_index + 1,
                            operation_index,
                            index
                        ))
                    })?;
                    outline.content.push_flattened_operations(&mut operations);
                }
            }
            operation_index += 1;
        }

        ensure_all_operations_referenced(&rects_seen, page_index, "rect")?;
        ensure_all_operations_referenced(&rounded_rects_seen, page_index, "rounded rect")?;
        ensure_all_operations_referenced(&paths_seen, page_index, "path")?;
        ensure_all_operations_referenced(&strokes_seen, page_index, "stroke")?;
        for (index, seen) in images_seen.into_iter().enumerate() {
            if !seen
                && !self
                    .svg_pattern_images
                    .iter()
                    .any(|resource| self.images.get(index) == Some(resource))
            {
                return Err(Error::InvalidInput(format!(
                    "page {} has unreferenced image {} while paint operations are present",
                    page_index + 1,
                    index
                )));
            }
        }
        ensure_all_operations_referenced(&image_patterns_seen, page_index, "image pattern")?;
        ensure_all_operations_referenced(&gradient_patterns_seen, page_index, "gradient pattern")?;
        ensure_all_operations_referenced(&svg_patterns_seen, page_index, "SVG pattern")?;
        ensure_all_operations_referenced(&lines_seen, page_index, "line")?;
        ensure_all_operations_referenced(
            &opaque_text_coverages_seen,
            page_index,
            "opaque text coverage",
        )?;
        ensure_all_operations_referenced(&svg_text_outlines_seen, page_index, "SVG text outline")?;
        Ok(())
    }

    pub(crate) fn paint_tree(&self) -> &PagePaintTree {
        &self.paint_tree
    }

    pub(crate) fn finalize_paint_tree_for_public_view(&mut self) {
        self.links = self.paint_tree.transformed_links(self);
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
        // SVG paint-server cells own image resources but do not emit their
        // images in page painting order. Register those sources with the page
        // resource inventory so PDF planning can deduplicate and bind them to
        // the cell Form without manufacturing a page-level draw operation.
        for paint in [path.fill_paint.as_ref(), path.stroke_paint.as_ref()]
            .into_iter()
            .flatten()
        {
            if let crate::document::paint::paths::RenderedPathPaint::SvgPattern(pattern) = paint {
                let mut images = Vec::new();
                pattern.scene.raster_images(&mut images);
                self.svg_pattern_images.extend(images.into_iter().cloned());
            }
        }
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

    /// Record SVG text lowered to outlines while preserving its authored text
    /// for PDF extraction with one `/ActualText` marked-content sequence.
    fn record_svg_text_outline(
        &mut self,
        outlined: crate::svg::SvgOutlinedText,
    ) -> (usize, PaintOperation) {
        let content = self.record_svg_group(outlined.content);
        self.record_svg_text_outline_scope(content, outlined.actual_text)
    }

    /// Record a captured SVG-text outline subtree without flattening its
    /// compositing boundaries. Paint fragments can replay this representation
    /// on a later page, where PDF emission still wraps the complete subtree in
    /// one `/ActualText` span.
    fn record_svg_text_outline_scope(
        &mut self,
        content: crate::document::paint::effects::PaintEffectScope,
        actual_text: Rc<str>,
    ) -> (usize, PaintOperation) {
        let content = content.into_recorded_nodes(self);
        let index = self.svg_text_outlines.len();
        self.svg_text_outlines.push(SvgTextOutline {
            content,
            actual_text,
        });
        (index, PaintOperation::SvgTextOutline(index))
    }

    pub(crate) fn record_image(&mut self, image: RenderedImage) -> (usize, PaintOperation) {
        let index = self.images.len();
        self.images.push(image);
        (index, PaintOperation::Image(index))
    }

    pub(crate) fn record_image_pattern(
        &mut self,
        pattern: RenderedImagePattern,
    ) -> (usize, PaintOperation) {
        let index = self.image_patterns.len();
        self.image_patterns.push(pattern);
        (index, PaintOperation::ImagePattern(index))
    }

    pub(crate) fn record_gradient_pattern(
        &mut self,
        pattern: RenderedGradientPattern,
    ) -> (usize, PaintOperation) {
        let index = self.gradient_patterns.len();
        self.gradient_patterns.push(pattern);
        (index, PaintOperation::GradientPattern(index))
    }

    pub(crate) fn record_svg_pattern(
        &mut self,
        pattern: RenderedSvgPattern,
    ) -> (usize, PaintOperation) {
        let index = self.svg_patterns.len();
        self.svg_patterns.push(pattern);
        (index, PaintOperation::SvgPattern(index))
    }

    pub(crate) fn paint_fragment(&self) -> PaintFragment {
        PaintFragment {
            display_list: PaintDisplayList {
                bands: self.paint_tree.root.bands.primitive_node_copy(self),
            },
            links: Vec::new(),
        }
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
            PaintPrimitive::ImagePattern(pattern) => self.record_image_pattern(pattern).1,
            PaintPrimitive::ProjectiveRaster(_) => {
                unreachable!("projective raster lowering runs after page primitive recording")
            }
            PaintPrimitive::GradientPattern(pattern) => self.record_gradient_pattern(pattern).1,
            PaintPrimitive::SvgPattern(pattern) => self.record_svg_pattern(pattern).1,
            PaintPrimitive::Line(line) => self.record_line(line).1,
            PaintPrimitive::OpaqueTextCoverage { line, paths } => {
                let (line_index, _) = self.record_line(line);
                let path_indices = paths
                    .into_iter()
                    .map(|path| self.record_path(path).0)
                    .collect();
                let index = self.opaque_text_coverages.len();
                self.opaque_text_coverages.push(OpaqueTextCoverage {
                    line_index,
                    path_indices,
                });
                PaintOperation::OpaqueTextCoverage(index)
            }
            PaintPrimitive::SvgTextOutline {
                content,
                actual_text,
            } => self.record_svg_text_outline_scope(*content, actual_text).1,
        }
    }

    pub(crate) fn record_paint_fragment(
        &mut self,
        fragment: &PaintFragment,
        offset: PaintTranslation,
    ) -> RecordedPaintFragment {
        self.record_paint_fragment_owned(fragment.clone(), offset)
    }

    pub(crate) fn record_paint_fragment_owned(
        &mut self,
        fragment: PaintFragment,
        offset: PaintTranslation,
    ) -> RecordedPaintFragment {
        let translated = fragment.translated(offset);
        let display_list = translated
            .display_list
            .into_recorded_nodes(self)
            .with_links(PaintBand::Inline, translated.links);
        let mut links = Vec::new();
        display_list
            .bands
            .push_transformed_links(PaintTransform::identity(), None, &mut links);
        self.links.extend(links);
        RecordedPaintFragment { display_list }
    }

    pub(crate) fn append_paint_fragment(
        &mut self,
        fragment: &PaintFragment,
        offset: PaintTranslation,
    ) {
        let recorded = self.record_paint_fragment(fragment, offset);
        self.append_recorded_paint_fragment(recorded);
    }

    pub(crate) fn append_paint_fragment_owned(
        &mut self,
        fragment: PaintFragment,
        offset: PaintTranslation,
    ) {
        let recorded = self.record_paint_fragment_owned(fragment, offset);
        self.append_recorded_paint_fragment(recorded);
    }

    /// Replay a fragment below all existing page paint in each CSS paint band.
    ///
    /// Negative-z generated page-margin boxes are discovered after normal
    /// document layout, but CSS Paged Media paints them below the page canvas.
    /// <https://www.w3.org/TR/css-page-3/#painting>
    pub(crate) fn prepend_paint_fragment_owned(
        &mut self,
        fragment: PaintFragment,
        offset: PaintTranslation,
    ) {
        let recorded = self.record_paint_fragment_owned(fragment, offset);
        self.paint_tree.prepend_display_list(recorded.display_list);
    }

    pub(crate) fn append_recorded_paint_fragment(&mut self, recorded: RecordedPaintFragment) {
        self.paint_tree.append_display_list(recorded.display_list);
    }

    pub(crate) fn sort_paint_tree_stacking_contexts(&mut self) {
        self.paint_tree.sort_stacking_contexts();
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
                .cloned()
                .map(PaintPrimitive::RoundedRect),
            PaintOperation::Path(index) => {
                self.paths.get(*index).cloned().map(PaintPrimitive::Path)
            }
            PaintOperation::Stroke(index) => self
                .strokes
                .get(*index)
                .cloned()
                .map(PaintPrimitive::Stroke),
            PaintOperation::Image(index) => {
                self.images.get(*index).cloned().map(PaintPrimitive::Image)
            }
            PaintOperation::ImagePattern(index) => self
                .image_patterns
                .get(*index)
                .cloned()
                .map(PaintPrimitive::ImagePattern),
            PaintOperation::GradientPattern(index) => self
                .gradient_patterns
                .get(*index)
                .cloned()
                .map(PaintPrimitive::GradientPattern),
            PaintOperation::SvgPattern(index) => self
                .svg_patterns
                .get(*index)
                .cloned()
                .map(PaintPrimitive::SvgPattern),
            PaintOperation::Line(index) => {
                self.lines.get(*index).cloned().map(PaintPrimitive::Line)
            }
            PaintOperation::OpaqueTextCoverage(index) => {
                let coverage = self.opaque_text_coverages.get(*index)?;
                let line = self.lines.get(coverage.line_index)?.clone();
                let paths = coverage
                    .path_indices
                    .iter()
                    .map(|index| self.paths.get(*index).cloned())
                    .collect::<Option<Vec<_>>>()?;
                Some(PaintPrimitive::OpaqueTextCoverage { line, paths })
            }
            PaintOperation::SvgTextOutline(index) => {
                self.svg_text_outlines.get(*index).cloned().map(|outline| {
                    PaintPrimitive::SvgTextOutline {
                        content: Box::new(outline.content.into_primitive_nodes(self)),
                        actual_text: outline.actual_text,
                    }
                })
            }
        }
    }

    pub(in crate::document) fn push_paint_tree_operation_in_band(
        &mut self,
        band: PaintBand,
        operation: PaintOperation,
    ) {
        self.paint_tree.push_operation(band, operation);
    }
}

/// A consumed, page-local cursor into a CSS paint-order band.
///
/// The fields stay private so layout cannot accidentally confuse primitive
/// array indexes with display-list positions. Consuming the value on insertion
/// also prevents reusing a cursor after the band has changed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PaintBandInsertionPoint {
    band: PaintBand,
    item_index: usize,
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
    ImagePattern(usize),
    GradientPattern(usize),
    SvgPattern(usize),
    Line(usize),
    OpaqueTextCoverage(usize),
    SvgTextOutline(usize),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct OpaqueTextCoverage {
    pub(crate) line_index: usize,
    pub(crate) path_indices: Vec<usize>,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct SvgTextOutline {
    pub(crate) content: crate::document::paint::effects::PaintEffectScope,
    pub(crate) actual_text: Rc<str>,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct PaintCheckpoint {
    pub(in crate::document) paint_tree: PagePaintTree,
    pub(in crate::document) rects: Vec<RenderedRect>,
    pub(in crate::document) rounded_rects: Vec<RenderedRoundedRect>,
    pub(in crate::document) paths: Vec<RenderedPath>,
    pub(in crate::document) strokes: Vec<RenderedStroke>,
    pub(in crate::document) images: Vec<RenderedImage>,
    pub(in crate::document) svg_pattern_images: Vec<RenderedImage>,
    pub(in crate::document) image_patterns: Vec<RenderedImagePattern>,
    pub(in crate::document) gradient_patterns: Vec<RenderedGradientPattern>,
    pub(in crate::document) svg_patterns: Vec<RenderedSvgPattern>,
    pub(in crate::document) lines: Vec<RenderedLine>,
    pub(in crate::document) opaque_text_coverages: Vec<OpaqueTextCoverage>,
    pub(in crate::document) svg_text_outlines: Vec<SvgTextOutline>,
    pub(in crate::document) links: Vec<RenderedLink>,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum PaintPrimitive {
    Rect(RenderedRect),
    RoundedRect(RenderedRoundedRect),
    Path(RenderedPath),
    Stroke(RenderedStroke),
    Image(RenderedImage),
    ImagePattern(RenderedImagePattern),
    /// Raster paint retained until the PDF backend can lower a projective CSS
    /// scene. PDF has no projective CTM, so the backend paints only the
    /// finite, viewer-visible polygon.
    ProjectiveRaster(ProjectiveRasterPrimitive),
    GradientPattern(RenderedGradientPattern),
    SvgPattern(RenderedSvgPattern),
    Line(RenderedLine),
    OpaqueTextCoverage {
        line: RenderedLine,
        paths: Vec<RenderedPath>,
    },
    SvgTextOutline {
        content: Box<crate::document::paint::effects::PaintEffectScope>,
        actual_text: Rc<str>,
    },
}

/// One source raster together with the projective plane that produced its
/// finite visible destination polygon.
///
/// CSS Transforms 2 requires clipping at the viewer plane before perspective
/// division. Keeping the source and polygon together prevents the PDF backend
/// from emitting the original unbounded operation after that clipping step.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ProjectiveRasterPrimitive {
    pub(crate) source: ProjectiveRasterSource,
    pub(crate) visible_polygon: Vec<PaintPoint>,
    pub(crate) source_transform: Projective3dPaintTransform,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum ProjectiveRasterSource {
    Image(RenderedImage),
    ImagePattern(RenderedImagePattern),
}

impl PaintPrimitive {
    pub(crate) fn translated(self, offset: PaintTranslation) -> Self {
        match self {
            Self::Rect(rect) => Self::Rect(rect.translated(offset)),
            Self::RoundedRect(rect) => Self::RoundedRect(rect.translated(offset)),
            Self::Path(path) => Self::Path(path.translated(offset)),
            Self::Stroke(stroke) => Self::Stroke(stroke.translated(offset)),
            Self::Image(image) => Self::Image(image.translated(offset)),
            Self::ImagePattern(pattern) => Self::ImagePattern(pattern.translated(offset)),
            Self::ProjectiveRaster(raster) => Self::ProjectiveRaster(raster.translated(offset)),
            Self::GradientPattern(pattern) => Self::GradientPattern(pattern.translated(offset)),
            Self::SvgPattern(pattern) => Self::SvgPattern(pattern.translated(offset)),
            Self::Line(line) => Self::Line(line.translated(offset)),
            Self::OpaqueTextCoverage { line, paths } => Self::OpaqueTextCoverage {
                line: line.translated(offset),
                paths: paths
                    .into_iter()
                    .map(|path| path.translated(offset))
                    .collect(),
            },
            Self::SvgTextOutline {
                content,
                actual_text,
            } => Self::SvgTextOutline {
                content: Box::new((*content).translated(offset)),
                actual_text,
            },
        }
    }

    pub(crate) fn clipped_to_rect(self, clip: PaintClip) -> Option<Self> {
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
            Self::ImagePattern(pattern) => rect_bounds(pattern.paint_rect())
                .and_then(|bounds| bounds.intersect(clip))
                .map(|intersection| {
                    Self::ImagePattern(pattern.with_intersected_clip(RenderedPathClip::new(
                        paint_rect_path_commands(intersection.paint_rect()),
                        RenderedPathFillRule::NonZero,
                        Vec::new(),
                    )))
                }),
            Self::ProjectiveRaster(raster) => raster
                .bounds()
                .and_then(|bounds| bounds.intersect(clip))
                .map(|_| Self::ProjectiveRaster(raster)),
            Self::GradientPattern(pattern) => rect_bounds(pattern.paint_bounds())
                .and_then(|bounds| bounds.intersect(clip))
                .map(|intersection| {
                    Self::GradientPattern(pattern.with_intersected_clip(RenderedPathClip::new(
                        paint_rect_path_commands(intersection.paint_rect()),
                        RenderedPathFillRule::NonZero,
                        Vec::new(),
                    )))
                }),
            Self::SvgPattern(pattern) => rect_bounds(pattern.paint_rect())
                .and_then(|bounds| bounds.intersect(clip))
                .map(|intersection| {
                    Self::SvgPattern(pattern.with_intersected_clip(RenderedPathClip::new(
                        paint_rect_path_commands(intersection.paint_rect()),
                        RenderedPathFillRule::NonZero,
                        Vec::new(),
                    )))
                }),
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
            Self::OpaqueTextCoverage { line, paths } => line
                .paint_bounds()
                .intersect(clip)
                .map(|_| Self::OpaqueTextCoverage { line, paths }),
            Self::SvgTextOutline {
                content,
                actual_text,
            } => content
                .bounds
                .is_none_or(|bounds| bounds.intersect(clip).is_some())
                .then_some(Self::SvgTextOutline {
                    content,
                    actual_text,
                }),
        }
    }

    pub(in crate::document) fn bounds(&self) -> Option<PaintClip> {
        match self {
            Self::Rect(rect) => rect_bounds(rect.paint_rect()),
            Self::RoundedRect(rect) => rect_bounds(rect.paint_rect()),
            Self::Image(image) => rect_bounds(image.paint_rect()),
            Self::ImagePattern(pattern) => rect_bounds(pattern.paint_rect()),
            Self::ProjectiveRaster(raster) => raster.bounds(),
            Self::GradientPattern(pattern) => rect_bounds(pattern.paint_bounds()),
            Self::SvgPattern(pattern) => rect_bounds(pattern.paint_rect()),
            Self::Stroke(stroke) => Some(stroke.paint_bounds()),
            Self::Line(line) => Some(line.paint_bounds()),
            Self::OpaqueTextCoverage { line, .. } => Some(line.paint_bounds()),
            Self::SvgTextOutline { content, .. } => content.bounds,
            Self::Path(path) => path_bounds(path),
        }
    }
}

impl ProjectiveRasterPrimitive {
    fn translated(mut self, offset: PaintTranslation) -> Self {
        self.visible_polygon = self
            .visible_polygon
            .into_iter()
            .map(|point| offset.transform_point(point))
            .collect();
        self.source = match self.source {
            ProjectiveRasterSource::Image(image) => {
                ProjectiveRasterSource::Image(image.translated(offset))
            }
            ProjectiveRasterSource::ImagePattern(pattern) => {
                ProjectiveRasterSource::ImagePattern(pattern.translated(offset))
            }
        };
        self
    }

    fn bounds(&self) -> Option<PaintClip> {
        let mut points = self.visible_polygon.iter().copied();
        let first = points.next()?;
        let (mut min_x, mut max_x, mut min_y, mut max_y) = (first.x, first.x, first.y, first.y);
        for point in points {
            min_x = min_x.min(point.x);
            max_x = max_x.max(point.x);
            min_y = min_y.min(point.y);
            max_y = max_y.max(point.y);
        }
        (min_x.is_finite() && max_x.is_finite() && min_y.is_finite() && max_y.is_finite())
            .then(|| PaintClip::new(min_x, min_y, max_x - min_x, max_y - min_y))
    }
}

#[cfg(test)]
mod tests {
    use std::rc::Rc;

    use super::{PaintOperation, PaintPrimitive};
    use crate::CssColor;
    use crate::document::Page;
    use crate::document::paint::display_list::PaintBand;
    use crate::document::paint::geometry::{
        PaintClip, PaintPoint, PaintRect, PaintSize, PaintStrokeWidth, PaintTransform,
        PaintTranslation,
    };
    use crate::document::paint::paths::{
        RenderedGradient, RenderedGradientKind, RenderedGradientStop, RenderedPath,
        RenderedPathFillRule, paint_rect_path_commands,
    };
    use crate::document::paint::patterns::{PaintPatternTiling, RenderedGradientPattern};
    use crate::document::paint::shapes::RenderedRect;
    use crate::document::paint::text::{
        RenderedGlyph, RenderedGlyphKind, RenderedLine, RenderedLineSource, RenderedTextMatrix,
        RenderedTextRun,
    };

    fn black_rect(x: f32) -> RenderedRect {
        RenderedRect::new(
            x,
            0.0,
            10.0,
            10.0,
            Some(CssColor::BLACK),
            None,
            PaintStrokeWidth::ZERO,
        )
    }

    fn text_line(text: &str, x: f32) -> RenderedLine {
        RenderedLine::new(
            text.to_string(),
            x,
            20.0,
            12.0,
            Some(0),
            CssColor::BLACK,
            vec![RenderedTextRun {
                text: Rc::from(text),
                actual_text: None,
                x_offset: 0.0,
                y_offset: 0.0,
                text_matrix: RenderedTextMatrix::IDENTITY,
                font_size: 12.0,
                font_id: Some(0),
                font_palette: crate::css::FontPalette::Normal,
                glyphs: Some(
                    text.chars()
                        .map(|character| RenderedGlyph {
                            kind: RenderedGlyphKind::Paint(1),
                            x_advance: 7.0,
                            nominal_x_advance: 7.0,
                            x_offset: 0.0,
                            y_offset: 0.0,
                            unicode: character.to_string(),
                        })
                        .collect::<Vec<_>>()
                        .into(),
                ),
                glyph_source_ranges: None,
            }],
        )
    }

    #[test]
    fn exact_text_paint_continuations_share_one_page_line() {
        let mut page = Page::new(100.0, 100.0);
        page.push_line(text_line("room", 10.0));
        let mut ellipsis = text_line("…", 38.0);
        ellipsis.source = RenderedLineSource::BlockEllipsis;
        page.push_line(ellipsis);

        assert_eq!(page.lines.len(), 1);
        assert_eq!(page.lines[0].text, "room…");
        assert_eq!(page.lines[0].runs.len(), 2);
    }

    #[test]
    fn paint_operation_between_text_records_prevents_continuation_coalescing() {
        let mut page = Page::new(100.0, 100.0);
        page.push_line(text_line("room", 10.0));
        page.push_rect_in_band(PaintBand::InFlowBlock, black_rect(38.0));
        page.push_line(text_line("…", 38.0));

        assert_eq!(page.lines.len(), 2);
    }

    #[test]
    fn clipped_text_record_prevents_continuation_coalescing() {
        let mut page = Page::new(100.0, 100.0);
        page.push_line(text_line("room", 10.0));
        page.push_line_clipped_in_band(
            PaintBand::InFlowBlock,
            text_line("…", 38.0),
            PaintClip::new(38.0, 10.0, 7.0, 20.0),
        );

        assert_eq!(page.lines.len(), 2);
    }

    #[test]
    fn validation_rejects_a_tree_operation_with_a_missing_primitive() {
        let mut page = Page::new(100.0, 100.0);
        page.rects.push(black_rect(0.0));
        page.paint_tree
            .push_operation(PaintBand::InFlowBlock, PaintOperation::Rect(1));

        let error = page.validate_paint_operations(0).unwrap_err().to_string();
        assert!(error.contains("paint operation 0 references missing rect 1"));
    }

    #[test]
    fn validation_rejects_unreferenced_primitive_storage() {
        let mut page = Page::new(100.0, 100.0);
        page.rects.extend([black_rect(0.0), black_rect(10.0)]);
        page.paint_tree
            .push_operation(PaintBand::InFlowBlock, PaintOperation::Rect(0));

        let error = page.validate_paint_operations(0).unwrap_err().to_string();
        assert!(error.contains("unreferenced rect 1"));
    }

    #[test]
    fn transformed_path_survives_destination_fragment_clip() {
        let path = RenderedPath::new(
            paint_rect_path_commands(PaintRect::new(
                PaintPoint::new(0.0, 0.0),
                PaintSize::new(10.0, 10.0),
            )),
            Some(CssColor::BLACK),
            RenderedPathFillRule::NonZero,
            None,
            PaintStrokeWidth::ZERO,
            None,
        )
        .with_transform(PaintTransform::new(1.0, 0.0, 0.0, 1.0, 100.0, 0.0));

        assert!(
            PaintPrimitive::Path(path)
                .clipped_to_rect(PaintClip::new(105.0, 0.0, 5.0, 10.0))
                .is_some()
        );
    }

    #[test]
    fn transformed_gradient_pattern_survives_destination_fragment_clip() {
        let rect = PaintRect::new(PaintPoint::new(0.0, 0.0), PaintSize::new(10.0, 10.0));
        let gradient = RenderedGradient {
            kind: RenderedGradientKind::Linear {
                start: PaintPoint::new(0.0, 0.0),
                end: PaintPoint::new(10.0, 10.0),
            },
            color_space: crate::css::CssColorSpace::Srgb,
            stops: vec![
                RenderedGradientStop {
                    offset: 0.0,
                    color: CssColor::BLACK,
                    interpolation_exponent: 1.0,
                },
                RenderedGradientStop {
                    offset: 1.0,
                    color: CssColor::WHITE,
                    interpolation_exponent: 1.0,
                },
            ],
            periodic: None,
            transform: PaintTransform::identity(),
        };
        let pattern = RenderedGradientPattern::new(
            rect,
            PaintPatternTiling::new(rect.size, rect.size, rect.origin),
            gradient,
            None,
        )
        .transformed(PaintTransform::new(1.0, 0.0, 0.0, 1.0, 100.0, 0.0));

        assert_eq!(pattern.tiling.origin, PaintPoint::new(0.0, 0.0));
        let projected = pattern
            .clone()
            .translated_geometry_preserving_tile_origin(PaintTranslation::new(0.0, 75.0));
        assert_eq!(projected.tiling.origin, PaintPoint::new(0.0, 0.0));
        assert_eq!(projected.paint_rect().origin, PaintPoint::new(0.0, 75.0));
        assert!(
            PaintPrimitive::GradientPattern(pattern)
                .clipped_to_rect(PaintClip::new(105.0, 0.0, 5.0, 10.0))
                .is_some()
        );
    }
}

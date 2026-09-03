use std::rc::Rc;

use super::counter_styles::{
    CounterStyleFallbackContext, CounterStyleRenderContext, complex_predefined_counter_style,
    counter_style_rule, counter_text_with_context, custom_counter_marker_text_with_context,
    custom_counter_marker_text_with_effective, predefined_named_counter_text_with_context,
};
use super::*;
use crate::layout::assets::DocumentPageIndex;
use crate::text::is_css_preserved_document_space;

impl<'a> LayoutBuilder<'a> {
    /// Run an off-page capture whose lines cannot become an ancestor list
    /// item's principal line.
    ///
    /// The closure receives a snapshot taken after the enclosing marker
    /// anchors have been detached. It may install its own scratch coordinate
    /// system and extract paint freely; this method always restores the outer
    /// builder and its marker anchors after the capture returns.
    /// <https://drafts.csswg.org/css-lists-3/#list-style-position-property>
    pub(in crate::layout) fn with_non_principal_line_capture<T>(
        &mut self,
        capture: impl FnOnce(&mut Self, &LayoutSnapshot) -> T,
    ) -> T {
        let pending_outside_marker_anchors = self.pending_outside_marker_anchors.suspend();
        let snapshot = self.snapshot();
        let result = capture(self, &snapshot);
        debug_assert!(
            self.pending_outside_marker_anchors.is_empty(),
            "a scratch layout must finalize its local outside-marker anchors"
        );
        self.restore(snapshot);
        self.pending_outside_marker_anchors
            .restore(pending_outside_marker_anchors);
        result
    }

    /// Whether this list item supplies a non-empty marker line to its
    /// principal inline flow.
    ///
    /// This is intentionally resolved from the same counter and generated
    /// content state as the eventual marker, rather than inferred from the
    /// specified `list-style-type`: an explicit `::marker { content: ... }`
    /// can suppress an automatic marker, and a counter representation can be
    /// empty.  The caller uses this before deciding whether the list item's
    /// block margins can collapse through it.
    ///
    /// CSS Lists Level 3 makes an inside marker part of the principal block
    /// box's first line; that line therefore prevents an otherwise empty
    /// list item from being self-collapsing.
    /// <https://drafts.csswg.org/css-lists-3/#list-style-position-property>
    /// <https://drafts.csswg.org/css-lists-3/#marker-pseudo>
    pub(in crate::layout) fn has_in_flow_marker_line(
        &mut self,
        element: &Element,
        style: &ComputedStyle,
    ) -> bool {
        let quote_depth = self.quote_depth;
        let marker = self.marker_for_list_item(element, style, self.containing_block_direction);
        self.quote_depth = quote_depth;
        marker.is_some_and(|marker| {
            marker.participates_in_first_line() && marker.has_in_flow_content()
        })
    }

    pub(in crate::layout) fn marker_for_list_item(
        &mut self,
        element: &Element,
        style: &ComputedStyle,
        parent_direction: Direction,
    ) -> Option<ListMarker> {
        if !style.display.is_list_item() {
            return None;
        }

        // CSS Lists 3 defines the marker box and `list-style-type` marker
        // string; HTML's `start`, `reversed`, and `value` attributes seed the
        // same ordinal counter for `ol`/`li`.
        // https://www.w3.org/TR/css-lists-3/#markers
        // https://html.spec.whatwg.org/multipage/grouping-content.html#the-ol-element
        let marker_style = style
            .marker_style
            .as_deref()
            .cloned()
            .unwrap_or_else(|| style.clone());
        let planned_stacks = match style.marker_counter_origin {
            css::MarkerCounterOrigin::Principal => self
                .counter_plan
                .values_at_origin
                .get(&CounterOriginKey::new(
                    element,
                    box_tree::CounterEventSource::Marker,
                ))
                .or_else(|| {
                    self.counter_plan
                        .values_at_origin
                        .get(&CounterOriginKey::new(
                            element,
                            box_tree::CounterEventSource::Principal,
                        ))
                }),
            css::MarkerCounterOrigin::Before => {
                self.counter_plan
                    .values_at_origin
                    .get(&CounterOriginKey::new(
                        element,
                        box_tree::CounterEventSource::Before,
                    ))
            }
            css::MarkerCounterOrigin::After => {
                self.counter_plan
                    .values_at_origin
                    .get(&CounterOriginKey::new(
                        element,
                        box_tree::CounterEventSource::After,
                    ))
            }
        };
        let ordinal = planned_stacks
            .and_then(|stacks| stacks.get(LIST_ITEM_COUNTER_NAME))
            .and_then(|values| values.last())
            .cloned()
            .or_else(|| self.counter_set.current(LIST_ITEM_COUNTER_NAME))
            .unwrap_or_default();
        // CSS Lists 3: for automatic markers, `list-style-image` is tried
        // before falling back to the textual `list-style-type`.
        // Explicit `::marker { content: ... }` bypasses automatic markers.
        let image = self
            .marker_image_for_style(style)
            .filter(|_| marker_style.marker_content == MarkerContent::Auto);
        let runtime_stacks;
        let counter_stacks = if let Some(planned_stacks) = planned_stacks {
            planned_stacks
        } else {
            runtime_stacks = self.counter_set.stacks();
            &runtime_stacks
        };
        let marker = if image.is_some() {
            // `list-style-image` replaces the counter representation, not
            // the marker's generated separator. An inside marker therefore
            // still contributes the normal following U+0020 to the inline
            // stream, where it remains available for extraction.
            // <https://drafts.csswg.org/css-lists-3/#list-style-position-property>
            Some((String::new(), true))
        } else if let Some(parts) = marker_style.content.generated_parts() {
            let text = evaluate_generated_content_text(
                element,
                parts,
                counter_stacks,
                &self.counter_styles,
                CounterStyleRenderContext::for_style(&marker_style),
            );
            (!text.is_empty()).then_some((text, false))
        } else if marker_style.marker_content == MarkerContent::Auto {
            // The originating list item's list-style properties select the
            // automatic marker representation. `::marker` can style that
            // representation, but cannot substitute its inherited
            // `list-style-type` or `list-style-image` values.
            // <https://drafts.csswg.org/css-lists-3/#marker-content>
            automatic_marker_text_with_context(
                style.list_style_type.clone(),
                ordinal,
                &self.counter_styles,
                CounterStyleRenderContext::for_style(&marker_style),
            )
        } else {
            marker_text(
                &marker_style,
                ordinal,
                &self.counter_styles,
                counter_stacks,
                &mut self.quote_depth,
                CounterStyleRenderContext::for_style(&marker_style),
            )
        };
        let (text, suffix_space) = marker?;
        Some(ListMarker {
            source_element: Some(element.id),
            text,
            image,
            style: marker_style,
            // A non-atomic inline list item has no block marker gutter, so an
            // `outside` marker participates as `inside`. An inline flow-root
            // is atomic and retains its own outside marker box.
            // <https://drafts.csswg.org/css-lists-3/#list-style-position-property>
            position: if style.display.is_inline_level() && !style.display.is_atomic_inline() {
                ListStylePosition::Inside
            } else {
                style.list_style_position
            },
            positioning_direction: match style.marker_side {
                MarkerSide::MatchSelf => style.direction,
                MarkerSide::MatchParent => parent_direction,
            },
            suffix_space,
        })
    }

    pub(in crate::layout) fn paint_outside_marker(
        &mut self,
        marker: &ListMarker,
        style: &ComputedStyle,
        anchor: OutsideMarkerAnchor,
    ) {
        if !marker.paints_outside()
            || style.visibility != Visibility::Visible
            || (marker.text.is_empty() && marker.image.is_none())
        {
            return;
        }
        if let Some(image) = &marker.image {
            let gap = if marker.suffix_space {
                self.marker_gap_width(&marker.style).points()
            } else {
                0.0
            };
            let x = match marker.positioning_direction {
                Direction::Ltr => anchor.principal_line_inline_span.left_x() - image.width - gap,
                Direction::Rtl => anchor.principal_line_inline_span.right_x() + gap,
            };
            let rect = PageTopRect::new(
                x,
                anchor.formatted_line_block_start.points(),
                image.width,
                image.height,
            )
            .paint_rect();
            if let Some(asset) = &image.svg {
                for path in asset.paint_paths(rect) {
                    self.push_path_in_band(PaintBand::Inline, path);
                }
            } else {
                self.push_image(
                    RenderedImage::from_paint_rect(
                        rect,
                        false,
                        image.decoded.pixel_size.width,
                        image.decoded.pixel_size.height,
                        image.decoded.source_rect,
                        crate::layout::assets::raster_image_sampling(&marker.style),
                        image.decoded.rgb.shared(),
                        image.decoded.alpha.clone(),
                        None,
                    )
                    .with_raster_color_space(image.decoded.color_space.clone())
                    .with_image_id(image.decoded.image_id),
                );
            }
            return;
        }
        let mut items = Vec::new();
        self.push_inside_marker_items(marker, style, None, &mut items);
        let measurement =
            self.intrinsic_inline_measurement_for_items(items.clone(), &marker.style, f32::MAX);
        let marker_width = measurement.contribution.max_content.points();
        let sequence = if marker
            .text
            .chars()
            .last()
            .is_some_and(is_css_preserved_document_space)
        {
            measurement.sequence
        } else {
            self.collect_inline_line_sequence_with_text_box_trim(
                items,
                &marker.style,
                marker_width,
                0.0,
                0.0,
            )
        };
        let marker_left = match marker.positioning_direction {
            Direction::Ltr => anchor.principal_line_inline_span.left_x() - marker_width,
            Direction::Rtl => anchor.principal_line_inline_span.right_x(),
        };
        let marker_baseline_offset = layout_pt(sequence.first_line_baseline_offset(
            self.inline_box_text_line_layout_baseline_offset(&marker.style),
        ));
        let marker_block_start = anchor
            .alphabetic_baseline
            .toward_block_start(marker_baseline_offset);
        self.paint_inline_box_sequence_with_float_policy(
            &sequence,
            &marker.style,
            marker_left,
            marker_width,
            marker_block_start.points(),
            NestedInlinePaintFloatPolicy::PreserveResolvedGeometry,
        );
    }

    /// Begin deferring an outside marker until an accepted in-flow line
    /// supplies its interoperable anchor.  This deliberately scopes the
    /// capture to horizontal writing: physical vertical-marker placement has
    /// separate unresolved behavior and retains its established fallback.
    pub(in crate::layout) fn begin_outside_marker_anchor(
        &mut self,
        marker: Option<&ListMarker>,
        list_item_style: &ComputedStyle,
        content_inline_span: PageInlineSpan,
    ) -> bool {
        let Some(marker) = marker.filter(|marker| marker.paints_outside()) else {
            return false;
        };
        if list_item_style.writing_mode != WritingMode::HorizontalTb {
            return false;
        }
        let fallback_alphabetic_baseline = PageTopBlockPosition::new(self.cursor_y)
            .toward_block_end(layout_pt(
                self.inline_box_text_line_layout_baseline_offset(list_item_style),
            ));
        self.pending_outside_marker_anchors
            .push(PendingOutsideMarkerAnchor {
                marker: marker.clone(),
                list_item_style: list_item_style.clone(),
                fallback: OutsideMarkerFallbackCandidate {
                    containing_inline_span: content_inline_span,
                    fallback_line_block_span: PageBlockSpan::new(
                        self.cursor_y,
                        list_item_style.line_height,
                    ),
                    alphabetic_baseline: fallback_alphabetic_baseline,
                },
                paint: DeferredOutsideMarkerPaintState::AwaitingAnchor,
            });
        true
    }

    /// Finish a list item's marker capture, retaining the old block-start
    /// fallback only for an item that establishes no eligible in-flow line.
    pub(in crate::layout) fn finish_outside_marker_anchor(&mut self) {
        let Some(pending) = self.pending_outside_marker_anchors.pop() else {
            return;
        };
        let paint = match pending.paint {
            DeferredOutsideMarkerPaintState::Resolved(paint) => *paint,
            DeferredOutsideMarkerPaintState::PaintedInPlace => return,
            DeferredOutsideMarkerPaintState::AwaitingAnchor => {
                let anchor = self.resolve_float_adjacent_outside_marker_fallback(pending.fallback);
                let paint = self.capture_outside_marker_paint(
                    &pending.marker,
                    &pending.list_item_style,
                    anchor,
                );
                self.commit_deferred_outside_marker_paint(paint);
                return;
            }
            DeferredOutsideMarkerPaintState::Capturing => {
                debug_assert!(
                    false,
                    "outside marker capture must complete before finalization"
                );
                return;
            }
        };
        self.commit_deferred_outside_marker_paint(paint);
    }

    /// Paint an outside marker into an isolated fragment at the anchor line,
    /// then restore the active descendant paint tree.
    ///
    /// An outside marker is the list item's first generated child. A nested
    /// relatively positioned block may expose the first principal line, but
    /// must not capture the marker in its own auto-level pseudo stacking
    /// context. The owner commits this fragment only after descendant layout
    /// completes.
    /// <https://drafts.csswg.org/css-lists-3/#markers>
    /// <https://www.w3.org/TR/CSS22/zindex.html>
    fn capture_outside_marker_paint(
        &mut self,
        marker: &ListMarker,
        style: &ComputedStyle,
        anchor: OutsideMarkerAnchor,
    ) -> ResolvedOutsideMarkerPaint {
        let page = DocumentPageIndex::new(self.pages.len());
        let checkpoint = self.current_page.paint_checkpoint();
        self.paint_outside_marker(marker, style, anchor);
        let fragment = self.current_page.take_paint_fragment_since(checkpoint);
        ResolvedOutsideMarkerPaint {
            marker: marker.clone(),
            list_item_style: style.clone(),
            anchor,
            page,
            fragment,
        }
    }

    /// Attach a marker fragment to the list item's page-level paint owner.
    ///
    /// The target can be a page already closed by descendant fragmentation;
    /// that page still contains the enclosing list item's unfinalized paint
    /// suffix, so this remains part of the owner's later fragment capture.
    /// CSS Appendix E's paint bands then put owner inline paint below the
    /// relatively positioned descendant's auto/zero stacking context.
    /// <https://www.w3.org/TR/CSS22/zindex.html>
    fn commit_deferred_outside_marker_paint(&mut self, paint: ResolvedOutsideMarkerPaint) {
        let page_index = paint.page.get();
        if page_index < self.pages.len() {
            self.pages[page_index]
                .append_paint_fragment_owned(paint.fragment, PaintTranslation::identity());
        } else {
            debug_assert_eq!(
                page_index,
                self.pages.len(),
                "a deferred marker must target a materialized document page"
            );
            self.current_page
                .append_paint_fragment_owned(paint.fragment, PaintTranslation::identity());
        }
    }

    pub(in crate::layout) fn outside_marker_anchor_is_pending(&self, marker: &ListMarker) -> bool {
        self.pending_outside_marker_anchors.iter().any(|pending| {
            match (pending.marker.source_element, marker.source_element) {
                (Some(pending_source), Some(marker_source)) => pending_source == marker_source,
                _ => pending.marker == *marker,
            }
        })
    }

    pub(in crate::layout) fn outside_marker_fallback_anchor(
        &mut self,
        style: &ComputedStyle,
        content_inline_span: PageInlineSpan,
    ) -> OutsideMarkerAnchor {
        let formatted_line_block_start = PageTopBlockPosition::new(self.cursor_y);
        let fallback_baseline_offset = self.inline_box_text_line_layout_baseline_offset(style);
        let fallback = OutsideMarkerFallbackCandidate {
            containing_inline_span: content_inline_span,
            fallback_line_block_span: PageBlockSpan::new(self.cursor_y, style.line_height),
            alphabetic_baseline: formatted_line_block_start
                .toward_block_end(layout_pt(fallback_baseline_offset)),
        };
        if style.writing_mode != WritingMode::HorizontalTb {
            return OutsideMarkerAnchor {
                principal_line_inline_span: fallback.containing_inline_span,
                formatted_line_block_start,
                alphabetic_baseline: fallback.alphabetic_baseline,
            };
        }
        self.resolve_float_adjacent_outside_marker_fallback(fallback)
    }

    /// Resolve Spindrift's compatibility placement for an outside marker that
    /// lacks a principal line. CSS Lists leaves float-adjacent placement
    /// undefined; using the fallback line's float band keeps the marker on
    /// the inline-start side of the principal box without moving that box.
    /// <https://drafts.csswg.org/css-lists-3/#list-style-position-property>
    fn resolve_float_adjacent_outside_marker_fallback(
        &self,
        candidate: OutsideMarkerFallbackCandidate,
    ) -> OutsideMarkerAnchor {
        let principal_line_inline_span = self
            .float_band_in_span(
                candidate.fallback_line_block_span,
                candidate.containing_inline_span,
            )
            .span;
        OutsideMarkerAnchor {
            principal_line_inline_span,
            formatted_line_block_start: PageTopBlockPosition::new(
                candidate.fallback_line_block_span.top_y(),
            ),
            alphabetic_baseline: candidate.alphabetic_baseline,
        }
    }

    pub(in crate::layout) fn anchor_pending_outside_markers_to_in_flow_line(
        &mut self,
        formatted_line_block_start: PageTopBlockPosition,
        baseline_offset: LayoutLength,
        paint_owner: OutsideMarkerPaintOwner,
    ) {
        let alphabetic_baseline = formatted_line_block_start.toward_block_end(baseline_offset);
        let anchors = self
            .pending_outside_marker_anchors
            .iter()
            .enumerate()
            .filter(|(_, pending)| {
                matches!(
                    pending.paint,
                    DeferredOutsideMarkerPaintState::AwaitingAnchor
                )
            })
            .map(|(index, pending)| {
                (
                    index,
                    pending.marker.clone(),
                    pending.list_item_style.clone(),
                    OutsideMarkerAnchor {
                        principal_line_inline_span: pending.fallback.containing_inline_span,
                        formatted_line_block_start,
                        alphabetic_baseline,
                    },
                )
            })
            .collect::<Vec<_>>();
        for (index, marker, list_item_style, anchor) in anchors {
            // Mark this before marker-line layout re-enters the shared line
            // painter. The marker's own generated line is not the list
            // item's principal line and must not recursively re-anchor it.
            if paint_owner == OutsideMarkerPaintOwner::ListItem {
                self.pending_outside_marker_anchors
                    .begin_paint_capture(index);
                let paint = self.capture_outside_marker_paint(&marker, &list_item_style, anchor);
                self.pending_outside_marker_anchors
                    .finish_paint_capture(index, paint);
            } else {
                self.pending_outside_marker_anchors
                    .mark_painted_in_place(index);
                self.paint_outside_marker(&marker, &list_item_style, anchor);
            }
        }
    }

    pub(in crate::layout) fn marker_gap_width(&mut self, style: &ComputedStyle) -> LayoutLength {
        // The automatic textual marker suffix ends in U+0020.  Its advance is
        // therefore the selected font's space advance, not a synthesized
        // half-em gutter.
        // <https://drafts.csswg.org/css-counter-styles-3/#generate-a-counter>
        self.inline_space_width(style)
    }

    pub(in crate::layout) fn push_inside_marker_items(
        &mut self,
        marker: &ListMarker,
        _block_style: &ComputedStyle,
        link_target: Option<String>,
        items: &mut Vec<InlineItem>,
    ) {
        let marker_scope_style = marker_inline_scope_style(&marker.style);
        let marker_ends_in_preserved_space = marker.suffix_space
            || marker
                .text
                .chars()
                .last()
                .is_some_and(is_css_preserved_document_space);
        self.push_inline_scope_start_items(
            &marker_scope_style,
            link_target.clone(),
            0.0,
            InlineVisualOffset::zero(),
            None,
            // A marker ending in preserved document space keeps its existing
            // whitespace-collection path. A punctuation-suffixed marker has
            // no separator and shares the zero-size inline scope shape of an
            // authored isolate.
            !marker_ends_in_preserved_space,
            items,
        );
        if let Some(image) = &marker.image {
            items.push(InlineItem::Atom(Box::new(InlineAtom::new(
                image
                    .svg
                    .clone()
                    .map(|asset| InlineAtomContent::Svg { asset: Some(asset) })
                    .unwrap_or_else(|| InlineAtomContent::Image(image.decoded.clone())),
                marker.style.clone(),
                None,
                // The atom's content box is exactly the marker image. Line
                // layout accounts for baseline descent separately; including
                // it here would stretch the image's painted height.
                InlineSize::new(image.width, image.height),
                image.height,
                0.0,
                link_target.clone(),
                None,
            ))));
        } else if !marker.text.is_empty() {
            items.push(InlineItem::Word(Box::new(InlineWord {
                text: marker.text.clone(),
                style: inline_style(&marker.style),
                baseline_shift: 0.0,
                visual_offset: InlineVisualOffset::zero(),
                link_target: link_target.clone().map(Rc::from),
                mergeable: true,
                source: InlineTextSource::Marker,
                hanging_edges: InlineHangingEdges::default(),
                excluded_positioning_geometry_source: None,
                ancestor_inline_decorations: Vec::new().into(),
            })));
        }
        if marker.suffix_space {
            items.push(InlineItem::Word(Box::new(InlineWord {
                text: " ".to_string(),
                style: inline_style(&marker.style),
                baseline_shift: 0.0,
                visual_offset: InlineVisualOffset::zero(),
                link_target: link_target.clone().map(Rc::from),
                mergeable: true,
                source: InlineTextSource::Normal,
                hanging_edges: InlineHangingEdges::default(),
                excluded_positioning_geometry_source: None,
                ancestor_inline_decorations: Vec::new().into(),
            })));
        }
        self.push_inline_scope_end_items(
            &marker_scope_style,
            link_target,
            0.0,
            InlineVisualOffset::zero(),
            None,
            !marker_ends_in_preserved_space,
            items,
        );
    }

    pub(in crate::layout) fn marker_image_for_style(
        &self,
        style: &ComputedStyle,
    ) -> Option<MarkerImage> {
        let image = style.list_style_image.as_image()?;
        let asset = match image.selected_image() {
            css::BackgroundImage::Url(_) | css::BackgroundImage::ImageFunction(_) => {
                match resolve_css_image_source(
                    image.selected_image(),
                    ImageResolutionContext {
                        base_url: self.base_url,
                        root_url: None,
                        current_color: style.color,
                        orientation: crate::layout::asset_helpers::raster_orientation_policy(
                            style.image_orientation,
                        ),
                        svg_context: crate::svg::SvgImageContext::from_used_color_scheme(
                            style.used_color_scheme,
                        ),
                        resource_cache: self.resource_cache,
                    },
                ) {
                    ResolvedCssImage::External(asset) => asset,
                    ResolvedCssImage::SolidColor(color) => {
                        ResolvedImageAsset::Raster(solid_color_marker_image(color))
                    }
                    ResolvedCssImage::Invalid => return None,
                }
            }
            // Existing gradient marker support is deliberately unchanged.
            _ => return None,
        };
        // Candidate density selects SVG options but does not rescale their
        // vector natural dimensions.
        // <https://drafts.csswg.org/css-images-4/#image-set-notation>
        let intrinsic_resolution = match &asset {
            ResolvedImageAsset::Raster(_) => image.intrinsic_resolution(),
            ResolvedImageAsset::Svg(_) => 1.0,
        }
        .max(f32::MIN_POSITIVE);
        let intrinsic_size = asset.intrinsic_size();
        let width = intrinsic_size.width / intrinsic_resolution;
        let height = intrinsic_size.height / intrinsic_resolution;
        if width <= 0.0 || height <= 0.0 {
            return None;
        }
        let (decoded, svg) = match asset {
            ResolvedImageAsset::Raster(decoded) => (decoded, None),
            ResolvedImageAsset::Svg(svg) => (
                DecodedPngImage::new(1, 1, vec![0, 0, 0], Some(vec![0])),
                Some(svg),
            ),
        };
        Some(MarkerImage {
            decoded,
            svg,
            width,
            height,
        })
    }
}

fn solid_color_marker_image(color: CssColor) -> DecodedPngImage {
    let color = crate::css::color_to_predefined_rgb(color, crate::css::CssColorSpace::Srgb)
        .expect("sRGB is a predefined CSS RGB space");
    DecodedPngImage::new(
        1,
        1,
        vec![
            (color.components()[0] * 255.0).round().clamp(0.0, 255.0) as u8,
            (color.components()[1] * 255.0).round().clamp(0.0, 255.0) as u8,
            (color.components()[2] * 255.0).round().clamp(0.0, 255.0) as u8,
        ],
        (color.alpha() < 1.0)
            .then_some(vec![(color.alpha() * 255.0).round().clamp(0.0, 255.0) as u8]),
    )
}

pub(in crate::layout) fn marker_inline_scope_style(style: &ComputedStyle) -> ComputedStyle {
    let mut style = style.clone();
    style.display = Display::INLINE;
    style
}

pub(in crate::layout) fn marker_text(
    style: &ComputedStyle,
    ordinal: i32,
    counter_styles: &HashMap<String, CounterStyleRule>,
    counter_stack: &HashMap<String, Vec<i32>>,
    quote_depth: &mut usize,
    render_context: CounterStyleRenderContext,
) -> Option<(String, bool)> {
    match &style.marker_content {
        MarkerContent::Auto => automatic_marker_text_with_context(
            style.list_style_type.clone(),
            ordinal,
            counter_styles,
            render_context,
        ),
        MarkerContent::None => None,
        MarkerContent::Parts(parts) => {
            let mut text = String::new();
            for part in parts {
                match part {
                    MarkerContentPart::Text(part) => text.push_str(part),
                    MarkerContentPart::Quote(quote) => match quote {
                        GeneratedQuote::Open => {
                            text.push_str(
                                &crate::layout::inline_collect::quote_pair(style, *quote_depth).0,
                            );
                            *quote_depth += 1;
                        }
                        GeneratedQuote::Close => {
                            *quote_depth = quote_depth.saturating_sub(1);
                            text.push_str(
                                &crate::layout::inline_collect::quote_pair(style, *quote_depth).1,
                            );
                        }
                        GeneratedQuote::NoOpen => *quote_depth += 1,
                        GeneratedQuote::NoClose => {
                            *quote_depth = quote_depth.saturating_sub(1);
                        }
                    },
                    MarkerContentPart::Counter {
                        name,
                        style: counter_style,
                    } => {
                        let value = if name.as_str() == LIST_ITEM_COUNTER_NAME {
                            ordinal
                        } else {
                            counter_stack
                                .get(name)
                                .and_then(|values| values.last().cloned())
                                .unwrap_or(0)
                        };
                        if let Some(counter) = counter_text_with_context(
                            counter_style.clone().unwrap_or(ListStyleType::Decimal),
                            value,
                            counter_styles,
                            render_context,
                        ) {
                            text.push_str(&counter);
                        }
                    }
                    MarkerContentPart::Counters {
                        name,
                        separator,
                        style: counter_style,
                    } => {
                        let values = counter_stack.get(name).cloned().unwrap_or_else(|| vec![0]);
                        let style = counter_style.clone().unwrap_or(ListStyleType::Decimal);
                        let counters = values
                            .into_iter()
                            .filter_map(|value| {
                                counter_text_with_context(
                                    style.clone(),
                                    value,
                                    counter_styles,
                                    render_context,
                                )
                            })
                            .collect::<Vec<_>>();
                        if !counters.is_empty() {
                            text.push_str(&counters.join(separator));
                        }
                    }
                }
            }
            (!text.is_empty()).then_some((text, false))
        }
    }
}

#[cfg(test)]
pub(in crate::layout) fn automatic_marker_text(
    list_style_type: ListStyleType,
    ordinal: i32,
    counter_styles: &HashMap<String, CounterStyleRule>,
) -> Option<(String, bool)> {
    automatic_marker_text_with_context(
        list_style_type,
        ordinal,
        counter_styles,
        CounterStyleRenderContext::default_context(),
    )
}

pub(in crate::layout) fn automatic_marker_text_with_context(
    list_style_type: ListStyleType,
    ordinal: i32,
    counter_styles: &HashMap<String, CounterStyleRule>,
    render_context: CounterStyleRenderContext,
) -> Option<(String, bool)> {
    if let ListStyleType::Named(name) = &list_style_type
        && let Some(effective) = complex_predefined_counter_style(name)
    {
        let mut fallback_context = CounterStyleFallbackContext::default();
        fallback_context.visit(name);
        return custom_counter_marker_text_with_effective(
            &effective,
            ordinal,
            counter_styles,
            render_context,
            &mut fallback_context,
        );
    }
    if let ListStyleType::Named(name) = &list_style_type
        && let Some(rule) = counter_style_rule(name, counter_styles)
    {
        return custom_counter_marker_text_with_context(
            rule,
            ordinal,
            counter_styles,
            render_context,
        );
    }
    if let ListStyleType::Named(name) = &list_style_type
        && let Some((representation, suffix)) =
            predefined_named_counter_text_with_context(name, ordinal, render_context)
    {
        return Some((format!("{representation}{suffix}"), suffix == " "));
    }
    if let ListStyleType::Anonymous(rule) = &list_style_type {
        return custom_counter_marker_text_with_context(
            rule,
            ordinal,
            counter_styles,
            render_context,
        );
    }
    let representation = counter_text_with_context(
        list_style_type.clone(),
        ordinal,
        counter_styles,
        render_context,
    )?;
    match list_style_type {
        ListStyleType::Disc
        | ListStyleType::Circle
        | ListStyleType::Square
        | ListStyleType::DisclosureOpen
        | ListStyleType::DisclosureClosed
        | ListStyleType::Anonymous(_) => Some((representation, true)),
        ListStyleType::String(_) => Some((representation, false)),
        ListStyleType::Decimal | ListStyleType::Named(_) => {
            Some((format!("{representation}."), true))
        }
        ListStyleType::None => None,
    }
}

#[cfg(test)]
mod tests;

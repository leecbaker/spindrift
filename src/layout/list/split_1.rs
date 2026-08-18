use super::*;
use crate::css::CounterStyleRangeInterval;
use crate::text::is_css_preserved_document_space;
use icu_segmenter::GraphemeClusterSegmenter;
use std::collections::HashSet;
use std::rc::Rc;

/// The writing context needed by predefined styles whose representation
/// depends on the element's inline and block directions.
///
/// CSS Counter Styles defines the disclosure styles in terms of these
/// directions, so the same context must reach direct use, generated content,
/// and a custom style that `extends` a disclosure style.
/// <https://drafts.csswg.org/css-counter-styles-3/#disclosure-open>
/// <https://drafts.csswg.org/css-counter-styles-3/#disclosure-closed>
#[derive(Debug, Clone, Copy)]
pub(in crate::layout) struct CounterStyleRenderContext {
    direction: Direction,
    writing_mode: WritingMode,
}

impl CounterStyleRenderContext {
    pub(in crate::layout) fn for_style(style: &ComputedStyle) -> Self {
        Self {
            direction: style.direction,
            writing_mode: style.writing_mode,
        }
    }

    fn default_context() -> Self {
        Self {
            direction: Direction::Ltr,
            writing_mode: WritingMode::HorizontalTb,
        }
    }
}

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
        let planned_stacks = self
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
            });
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
                painted: false,
            });
        true
    }

    /// Finish a list item's marker capture, retaining the old block-start
    /// fallback only for an item that establishes no eligible in-flow line.
    pub(in crate::layout) fn finish_outside_marker_anchor(&mut self) {
        let Some(pending) = self.pending_outside_marker_anchors.pop() else {
            return;
        };
        if pending.painted {
            return;
        }
        let anchor = self.resolve_float_adjacent_outside_marker_fallback(pending.fallback);
        self.paint_outside_marker(&pending.marker, &pending.list_item_style, anchor);
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

    /// Resolve Quire's compatibility placement for an outside marker that
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
    ) {
        let alphabetic_baseline = formatted_line_block_start.toward_block_end(baseline_offset);
        let anchors = self
            .pending_outside_marker_anchors
            .iter()
            .enumerate()
            .filter(|(_, pending)| !pending.painted)
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
            self.pending_outside_marker_anchors.mark_painted(index);
            self.paint_outside_marker(&marker, &list_item_style, anchor);
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

pub(in crate::layout) fn counter_text(
    list_style_type: ListStyleType,
    ordinal: i32,
    counter_styles: &HashMap<String, CounterStyleRule>,
) -> Option<String> {
    counter_text_with_context(
        list_style_type,
        ordinal,
        counter_styles,
        CounterStyleRenderContext::default_context(),
    )
}

pub(in crate::layout) fn counter_text_with_context(
    list_style_type: ListStyleType,
    ordinal: i32,
    counter_styles: &HashMap<String, CounterStyleRule>,
    render_context: CounterStyleRenderContext,
) -> Option<String> {
    match list_style_type {
        ListStyleType::Disc => Some("\u{2022}".to_string()),
        ListStyleType::Circle => Some("\u{25e6}".to_string()),
        ListStyleType::Square => Some("\u{25aa}".to_string()),
        ListStyleType::DisclosureOpen => Some(disclosure_symbol(true, render_context).to_string()),
        ListStyleType::DisclosureClosed => {
            Some(disclosure_symbol(false, render_context).to_string())
        }
        ListStyleType::Decimal => Some(ordinal.to_string()),
        ListStyleType::String(text) => Some(text),
        ListStyleType::Anonymous(rule) => {
            custom_counter_text_with_context(&rule, ordinal, counter_styles, render_context)
        }
        ListStyleType::Named(name) => counter_style_rule(&name, counter_styles)
            .and_then(|rule| {
                custom_counter_text_with_context(rule, ordinal, counter_styles, render_context)
            })
            .or_else(|| {
                complex_predefined_counter_style(&name).and_then(|effective| {
                    let mut fallback_context = CounterStyleFallbackContext::default();
                    fallback_context.visit(&name);
                    custom_counter_text_with_effective(
                        &effective,
                        ordinal,
                        counter_styles,
                        render_context,
                        &mut fallback_context,
                    )
                })
            })
            .or_else(|| {
                predefined_named_counter_text_with_context(&name, ordinal, render_context)
                    .map(|(text, _)| text)
            })
            .or_else(|| Some(ordinal.to_string())),
        ListStyleType::None => None,
    }
}

#[cfg(test)]
pub(in crate::layout) fn custom_counter_marker_text(
    rule: &CounterStyleRule,
    ordinal: i32,
    counter_styles: &HashMap<String, CounterStyleRule>,
) -> Option<(String, bool)> {
    custom_counter_marker_text_with_context(
        rule,
        ordinal,
        counter_styles,
        CounterStyleRenderContext::default_context(),
    )
}

pub(in crate::layout) fn custom_counter_marker_text_with_context(
    rule: &CounterStyleRule,
    ordinal: i32,
    counter_styles: &HashMap<String, CounterStyleRule>,
    render_context: CounterStyleRenderContext,
) -> Option<(String, bool)> {
    let effective = resolve_counter_style(rule, counter_styles, 0);
    let mut fallback_context = CounterStyleFallbackContext::for_rule(rule);
    custom_counter_marker_text_with_effective(
        &effective,
        ordinal,
        counter_styles,
        render_context,
        &mut fallback_context,
    )
}

fn custom_counter_marker_text_with_effective(
    effective: &EffectiveCounterStyle,
    ordinal: i32,
    counter_styles: &HashMap<String, CounterStyleRule>,
    render_context: CounterStyleRenderContext,
    fallback_context: &mut CounterStyleFallbackContext,
) -> Option<(String, bool)> {
    custom_counter_text_with_effective(
        effective,
        ordinal,
        counter_styles,
        render_context,
        fallback_context,
    )
    .map(|text| {
        (
            format!("{}{}{}", effective.prefix, text, effective.suffix),
            false,
        )
    })
}

#[cfg(test)]
pub(in crate::layout) fn custom_counter_text(
    rule: &CounterStyleRule,
    ordinal: i32,
    counter_styles: &HashMap<String, CounterStyleRule>,
) -> Option<String> {
    custom_counter_text_with_context(
        rule,
        ordinal,
        counter_styles,
        CounterStyleRenderContext::default_context(),
    )
}

pub(in crate::layout) fn custom_counter_text_with_context(
    rule: &CounterStyleRule,
    ordinal: i32,
    counter_styles: &HashMap<String, CounterStyleRule>,
    render_context: CounterStyleRenderContext,
) -> Option<String> {
    let effective = resolve_counter_style(rule, counter_styles, 0);
    let mut fallback_context = CounterStyleFallbackContext::for_rule(rule);
    custom_counter_text_with_effective(
        &effective,
        ordinal,
        counter_styles,
        render_context,
        &mut fallback_context,
    )
}

/// State held while producing one counter representation.
///
/// CSS Counter Styles falls back to decimal when a fallback chain repeats a
/// counter style. Tracking the normalized names makes that rule independent
/// of arbitrary nesting depth while preserving case-sensitive custom names.
/// <https://drafts.csswg.org/css-counter-styles-3/#counter-style-fallback>
#[derive(Default)]
struct CounterStyleFallbackContext {
    visited: HashSet<String>,
}

impl CounterStyleFallbackContext {
    fn for_rule(rule: &CounterStyleRule) -> Self {
        let mut context = Self::default();
        if !rule.name.is_empty() {
            context.visit(&rule.name);
        }
        context
    }

    fn visit(&mut self, name: &str) -> bool {
        let name = crate::css::canonical_predefined_counter_style_name(name).unwrap_or(name);
        self.visited.insert(name.to_string())
    }
}

fn custom_counter_text_with_effective(
    style: &EffectiveCounterStyle,
    ordinal: i32,
    counter_styles: &HashMap<String, CounterStyleRule>,
    render_context: CounterStyleRenderContext,
    fallback_context: &mut CounterStyleFallbackContext,
) -> Option<String> {
    if !counter_style_range_contains(&style.range, &style.system, ordinal) {
        return Some(fallback_counter_text(
            &style.fallback,
            ordinal,
            counter_styles,
            render_context,
            fallback_context,
        ));
    }

    let absolute_ordinal = if ordinal < 0 {
        i32::try_from(i64::from(ordinal).abs()).ok()?
    } else {
        ordinal
    };
    let is_complex_predefined = style.predefined.is_some();
    // Fixed and cyclic systems select their symbols directly from the signed
    // ordinal. A negative ordinal is therefore not a magnitude requiring the
    // `negative` affix.
    // <https://drafts.csswg.org/css-counter-styles-3/#fixed-system>
    // <https://drafts.csswg.org/css-counter-styles-3/#cyclic-system>
    let uses_negative_affix = ordinal < 0
        && !is_complex_predefined
        && !matches!(
            style.system,
            CounterStyleSystem::Cyclic | CounterStyleSystem::Fixed(_)
        );
    let Some(mut text) = style
        .predefined
        .and_then(|name| {
            predefined_named_counter_text_with_context(name, ordinal, render_context)
                .map(|(text, _)| text)
        })
        .or_else(|| match style.system {
            CounterStyleSystem::Cyclic => cyclic_counter_text(ordinal, &style.symbols),
            CounterStyleSystem::Numeric => numeric_counter_text(absolute_ordinal, &style.symbols),
            CounterStyleSystem::Alphabetic => {
                alphabetic_counter_text(absolute_ordinal, &style.symbols)
            }
            CounterStyleSystem::Symbolic => symbolic_counter_text(absolute_ordinal, &style.symbols),
            CounterStyleSystem::Fixed(first) => fixed_counter_text(ordinal, first, &style.symbols),
            CounterStyleSystem::Additive => {
                additive_counter_text(absolute_ordinal, &style.additive_symbols)
            }
            CounterStyleSystem::Extends(_) => None,
        })
    else {
        // A fallback is a complete new representation. It does not inherit
        // the failed style's pad or negative descriptors; the originally
        // requested marker keeps its prefix and suffix at the call site.
        // <https://drafts.csswg.org/css-counter-styles-3/#counter-style-fallback>
        return Some(fallback_counter_text(
            &style.fallback,
            ordinal,
            counter_styles,
            render_context,
            fallback_context,
        ));
    };
    if let Some((width, symbol)) = &style.pad {
        // `pad` measures the representation after a negative affix has been
        // accounted for, in extended grapheme clusters rather than Unicode
        // scalar values. The affix is still appended after padding below.
        // <https://drafts.csswg.org/css-counter-styles-3/#counter-style-pad>
        let negative_length = if uses_negative_affix {
            counter_representation_grapheme_length(&style.negative.0)
                + counter_representation_grapheme_length(&style.negative.1)
        } else {
            0
        };
        let text_len = counter_representation_grapheme_length(&text) + negative_length;
        if text_len < *width {
            text = format!("{}{}", symbol.repeat(*width - text_len), text);
        }
    }
    if uses_negative_affix {
        text = format!("{}{}{}", style.negative.0, text, style.negative.1);
    }
    Some(text)
}

fn counter_representation_grapheme_length(text: &str) -> usize {
    GraphemeClusterSegmenter::new()
        .segment_str(text)
        .count()
        .saturating_sub(1)
}

fn fallback_counter_text(
    fallback: &str,
    ordinal: i32,
    counter_styles: &HashMap<String, CounterStyleRule>,
    render_context: CounterStyleRenderContext,
    fallback_context: &mut CounterStyleFallbackContext,
) -> String {
    if !fallback_context.visit(fallback) {
        return ordinal.to_string();
    }
    if let Some(rule) = counter_style_rule(fallback, counter_styles) {
        let effective = resolve_counter_style(rule, counter_styles, 0);
        return custom_counter_text_with_effective(
            &effective,
            ordinal,
            counter_styles,
            render_context,
            fallback_context,
        )
        .unwrap_or_else(|| ordinal.to_string());
    }
    let style = css::parse_list_style_type(fallback).unwrap_or(ListStyleType::Decimal);
    match style {
        ListStyleType::Named(name) if name == fallback => ordinal.to_string(),
        other => counter_text_with_context(other, ordinal, counter_styles, render_context)
            .unwrap_or_else(|| ordinal.to_string()),
    }
}

/// Resolve a counter-style reference without erasing the case distinction for
/// author-defined names.  Only the predefined names are ASCII-case-insensitive.
/// <https://drafts.csswg.org/css-counter-styles-3/#counter-style-name>
pub(in crate::layout) fn counter_style_rule<'a>(
    name: &str,
    counter_styles: &'a HashMap<String, CounterStyleRule>,
) -> Option<&'a CounterStyleRule> {
    counter_styles.get(name).or_else(|| {
        crate::css::canonical_predefined_counter_style_name(name)
            .and_then(|canonical| counter_styles.get(canonical))
    })
}

#[derive(Debug, Clone)]
pub(in crate::layout) struct EffectiveCounterStyle {
    pub(in crate::layout) system: CounterStyleSystem,
    pub(in crate::layout) symbols: Vec<String>,
    pub(in crate::layout) additive_symbols: Vec<(i32, String)>,
    pub(in crate::layout) prefix: String,
    pub(in crate::layout) suffix: String,
    pub(in crate::layout) negative: (String, String),
    pub(in crate::layout) pad: Option<(usize, String)>,
    pub(in crate::layout) range: CounterStyleRange,
    pub(in crate::layout) fallback: String,
    /// Complex predefined styles have algorithms that cannot be expressed by
    /// the simple `system` descriptor, but remain valid `extends` targets.
    pub(in crate::layout) predefined: Option<&'static str>,
}

fn default_effective_counter_style() -> EffectiveCounterStyle {
    EffectiveCounterStyle {
        system: CounterStyleSystem::Numeric,
        symbols: decimal_counter_symbols(),
        additive_symbols: Vec::new(),
        prefix: String::new(),
        suffix: ". ".to_string(),
        negative: ("-".to_string(), String::new()),
        pad: None,
        range: CounterStyleRange::Auto,
        fallback: "decimal".to_string(),
        predefined: None,
    }
}

#[derive(Debug, Clone)]
struct CounterStyleResolution {
    effective: EffectiveCounterStyle,
    /// Names in the current `extends` cycle. A style in this set replaces its
    /// inherited base with decimal, while a non-participating caller may still
    /// inherit the repaired style and its descriptors.
    /// <https://drafts.csswg.org/css-counter-styles-3/#extends-system>
    cyclic_names: Vec<String>,
}

pub(in crate::layout) fn resolve_counter_style(
    rule: &CounterStyleRule,
    counter_styles: &HashMap<String, CounterStyleRule>,
    _depth: usize,
) -> EffectiveCounterStyle {
    let mut visiting = Vec::new();
    if !rule.name.is_empty() {
        visiting.push(rule.name.clone());
    }
    resolve_counter_style_inner(rule, counter_styles, &mut visiting).effective
}

fn resolve_counter_style_inner(
    rule: &CounterStyleRule,
    counter_styles: &HashMap<String, CounterStyleRule>,
    visiting: &mut Vec<String>,
) -> CounterStyleResolution {
    let inherited = if let CounterStyleSystem::Extends(name) = &rule.system {
        if let Some(cycle_start) = visiting.iter().position(|visited| visited == name) {
            CounterStyleResolution {
                effective: default_effective_counter_style(),
                cyclic_names: visiting[cycle_start..].to_vec(),
            }
        } else if let Some(effective) = complex_predefined_counter_style(name) {
            CounterStyleResolution {
                effective,
                cyclic_names: Vec::new(),
            }
        } else if let Some(target) = counter_style_rule(name, counter_styles) {
            visiting.push(name.clone());
            let resolved = resolve_counter_style_inner(target, counter_styles, visiting);
            visiting.pop();
            resolved
        } else {
            CounterStyleResolution {
                effective: default_effective_counter_style(),
                cyclic_names: Vec::new(),
            }
        }
    } else {
        CounterStyleResolution {
            effective: default_effective_counter_style(),
            cyclic_names: Vec::new(),
        }
    };
    let mut effective = if inherited.cyclic_names.iter().any(|name| name == &rule.name) {
        default_effective_counter_style()
    } else {
        inherited.effective
    };
    if !matches!(rule.system, CounterStyleSystem::Extends(_)) {
        effective.system = rule.system.clone();
        effective.symbols = rule.symbols.clone();
        effective.additive_symbols = rule.additive_symbols.clone();
        effective.predefined = None;
    }
    if let Some(prefix) = &rule.prefix {
        effective.prefix = prefix.clone();
    }
    if let Some(suffix) = &rule.suffix {
        effective.suffix = suffix.clone();
    }
    if let Some(negative) = &rule.negative {
        effective.negative = negative.clone();
    }
    if let Some(pad) = &rule.pad {
        effective.pad = Some(pad.clone());
    }
    if let Some(range) = &rule.range {
        effective.range = range.clone();
    }
    if let Some(fallback) = &rule.fallback {
        effective.fallback = fallback.clone();
    }
    CounterStyleResolution {
        effective,
        cyclic_names: inherited.cyclic_names,
    }
}

/// Construct the effective descriptor set for the complex styles which the
/// spec defines algorithmically rather than through the normative UA sheet.
/// They must nevertheless be valid `extends` targets.
/// <https://drafts.csswg.org/css-counter-styles-3/#complex-counters>
fn complex_predefined_counter_style(name: &str) -> Option<EffectiveCounterStyle> {
    let canonical = crate::css::canonical_predefined_counter_style_name(name)?;
    let (suffix, range, fallback) = match canonical {
        "disclosure-open" | "disclosure-closed" => (" ", CounterStyleRange::Auto, "decimal"),
        "simp-chinese-informal"
        | "simp-chinese-formal"
        | "trad-chinese-informal"
        | "trad-chinese-formal"
        | "cjk-ideographic" => (
            "、",
            CounterStyleRange::Intervals(vec![CounterStyleRangeInterval {
                start: -9_999,
                end: 9_999,
            }]),
            "cjk-decimal",
        ),
        "ethiopic-numeric" => ("/ ", CounterStyleRange::Auto, "decimal"),
        _ => return None,
    };
    Some(EffectiveCounterStyle {
        suffix: suffix.to_string(),
        range,
        fallback: fallback.to_string(),
        predefined: Some(canonical),
        ..default_effective_counter_style()
    })
}

pub(in crate::layout) fn counter_style_range_contains(
    range: &CounterStyleRange,
    system: &CounterStyleSystem,
    ordinal: i32,
) -> bool {
    let value = i64::from(ordinal);
    match range {
        CounterStyleRange::Auto => match system {
            CounterStyleSystem::Alphabetic | CounterStyleSystem::Symbolic => ordinal >= 1,
            CounterStyleSystem::Additive => ordinal >= 0,
            _ => true,
        },
        CounterStyleRange::Intervals(intervals) => intervals
            .iter()
            .any(|interval| value >= interval.start && value <= interval.end),
    }
}

pub(in crate::layout) fn decimal_counter_symbols() -> Vec<String> {
    (0..=9).map(|digit| digit.to_string()).collect()
}

pub(in crate::layout) fn cyclic_counter_text(index: i32, symbols: &[String]) -> Option<String> {
    let count = i32::try_from(symbols.len()).ok()?;
    if count == 0 {
        return None;
    }
    let position = (index - 1).rem_euclid(count);
    symbols.get(position as usize).cloned()
}

pub(in crate::layout) fn fixed_counter_text(
    index: i32,
    first: i32,
    symbols: &[String],
) -> Option<String> {
    let offset = index.checked_sub(first)?;
    let offset = usize::try_from(offset).ok()?;
    symbols.get(offset).cloned()
}

pub(in crate::layout) fn symbolic_counter_text(index: i32, symbols: &[String]) -> Option<String> {
    if index <= 0 || symbols.is_empty() {
        return None;
    }
    let count = i32::try_from(symbols.len()).ok()?;
    let symbol = symbols.get(((index - 1) % count) as usize)?;
    let repetitions = ((index + count - 1) / count) as usize;
    Some(symbol.repeat(repetitions))
}

pub(in crate::layout) fn alphabetic_counter_text(index: i32, symbols: &[String]) -> Option<String> {
    if index <= 0 || symbols.len() < 2 {
        return None;
    }
    let base = symbols.len();
    let mut value = index as usize;
    let mut output = Vec::new();
    while value > 0 {
        value -= 1;
        output.push(symbols[value % base].as_str());
        value /= base;
    }
    Some(output.iter().rev().cloned().collect::<String>())
}

pub(in crate::layout) fn numeric_counter_text(index: i32, symbols: &[String]) -> Option<String> {
    if symbols.len() < 2 {
        return None;
    }
    let base = i64::try_from(symbols.len()).ok()?;
    let sign = if index < 0 { "-" } else { "" };
    let mut value = i64::from(index).abs();
    if value == 0 {
        return symbols.first().map(|zero| format!("{sign}{zero}"));
    }
    let mut output = Vec::new();
    while value > 0 {
        let digit = usize::try_from(value % base).ok()?;
        output.push(symbols.get(digit)?.as_str());
        value /= base;
    }
    Some(format!(
        "{sign}{}",
        output.iter().rev().cloned().collect::<String>()
    ))
}

pub(in crate::layout) fn additive_counter_text(
    index: i32,
    symbols: &[(i32, String)],
) -> Option<String> {
    if index == 0 {
        return symbols
            .iter()
            .find_map(|(weight, symbol)| (*weight == 0).then(|| symbol.clone()));
    }
    if index < 0 {
        return None;
    }
    let mut value = index;
    let mut output = String::new();
    for (weight, symbol) in symbols {
        if *weight <= 0 {
            continue;
        }
        while value >= *weight {
            output.push_str(symbol);
            value -= *weight;
        }
    }
    (value == 0).then_some(output)
}

pub(in crate::layout) fn predefined_named_counter_text_with_context(
    name: &str,
    ordinal: i32,
    render_context: CounterStyleRenderContext,
) -> Option<(String, &'static str)> {
    match name {
        "disclosure-open" => Some((disclosure_symbol(true, render_context).to_string(), " ")),
        "disclosure-closed" => Some((disclosure_symbol(false, render_context).to_string(), " ")),
        "simp-chinese-informal" => {
            chinese_longhand_marker(ordinal, ChineseLonghandStyle::SimplifiedInformal)
                .map(|text| (text, "、"))
        }
        "simp-chinese-formal" => {
            chinese_longhand_marker(ordinal, ChineseLonghandStyle::SimplifiedFormal)
                .map(|text| (text, "、"))
        }
        "trad-chinese-informal" | "cjk-ideographic" => {
            chinese_longhand_marker(ordinal, ChineseLonghandStyle::TraditionalInformal)
                .map(|text| (text, "、"))
        }
        "trad-chinese-formal" => {
            chinese_longhand_marker(ordinal, ChineseLonghandStyle::TraditionalFormal)
                .map(|text| (text, "、"))
        }
        "ethiopic-numeric" => ethiopic_numeric_marker(ordinal).map(|text| (text, "/ ")),
        _ => None,
    }
}

/// Return the disclosure triangle selected by the element's writing context.
///
/// The closed marker points toward the block's inline-end side and the open
/// marker toward its block-end side, except that vertical writing swaps the
/// role expected by the disclosure widget's expansion axis.
/// <https://drafts.csswg.org/css-counter-styles-3/#disclosure-open>
/// <https://drafts.csswg.org/css-counter-styles-3/#disclosure-closed>
fn disclosure_symbol(open: bool, context: CounterStyleRenderContext) -> char {
    match (context.writing_mode, context.direction, open) {
        (WritingMode::HorizontalTb, _, true) => '\u{25be}',
        (WritingMode::HorizontalTb, Direction::Ltr, false) => '\u{25b8}',
        (WritingMode::HorizontalTb, Direction::Rtl, false) => '\u{25c2}',
        (WritingMode::VerticalLr, Direction::Ltr, true) => '\u{25b8}',
        (WritingMode::VerticalLr, Direction::Rtl, true) => '\u{25b8}',
        (WritingMode::VerticalLr, Direction::Ltr, false) => '\u{25be}',
        (WritingMode::VerticalLr, Direction::Rtl, false) => '\u{25b4}',
        (WritingMode::VerticalRl, Direction::Ltr, true) => '\u{25c2}',
        (WritingMode::VerticalRl, Direction::Rtl, true) => '\u{25c2}',
        (WritingMode::VerticalRl, Direction::Ltr, false) => '\u{25be}',
        (WritingMode::VerticalRl, Direction::Rtl, false) => '\u{25b4}',
        // Sideways modes have horizontal typographic orientation but a
        // vertical block flow. They use the matching vertical geometry.
        (WritingMode::SidewaysLr, Direction::Ltr, true) => '\u{25b8}',
        (WritingMode::SidewaysLr, Direction::Rtl, true) => '\u{25b8}',
        (WritingMode::SidewaysLr, Direction::Ltr, false) => '\u{25be}',
        (WritingMode::SidewaysLr, Direction::Rtl, false) => '\u{25b4}',
        (WritingMode::SidewaysRl, Direction::Ltr, true) => '\u{25c2}',
        (WritingMode::SidewaysRl, Direction::Rtl, true) => '\u{25c2}',
        (WritingMode::SidewaysRl, Direction::Ltr, false) => '\u{25be}',
        (WritingMode::SidewaysRl, Direction::Rtl, false) => '\u{25b4}',
    }
}

#[derive(Debug, Clone, Copy)]
pub(in crate::layout) enum ChineseLonghandStyle {
    SimplifiedInformal,
    SimplifiedFormal,
    TraditionalInformal,
    TraditionalFormal,
}

impl ChineseLonghandStyle {
    pub(in crate::layout) fn digits(self) -> &'static [&'static str; 10] {
        match self {
            Self::SimplifiedInformal | Self::TraditionalInformal => {
                &["零", "一", "二", "三", "四", "五", "六", "七", "八", "九"]
            }
            Self::SimplifiedFormal => &["零", "壹", "贰", "叁", "肆", "伍", "陆", "柒", "捌", "玖"],
            Self::TraditionalFormal => {
                &["零", "壹", "貳", "參", "肆", "伍", "陸", "柒", "捌", "玖"]
            }
        }
    }

    pub(in crate::layout) fn markers(self) -> &'static [&'static str; 4] {
        match self {
            Self::SimplifiedInformal | Self::TraditionalInformal => &["", "十", "百", "千"],
            Self::SimplifiedFormal => &["", "拾", "佰", "仟"],
            Self::TraditionalFormal => &["", "拾", "佰", "仟"],
        }
    }

    pub(in crate::layout) fn negative(self) -> &'static str {
        match self {
            Self::SimplifiedInformal | Self::SimplifiedFormal => "负",
            Self::TraditionalInformal | Self::TraditionalFormal => "負",
        }
    }

    pub(in crate::layout) fn is_informal(self) -> bool {
        matches!(self, Self::SimplifiedInformal | Self::TraditionalInformal)
    }
}

/// Render CSS Counter Styles Level 3 Chinese longhand predefined styles.
///
/// The spec defines these styles as special algorithms rather than ordinary
/// `@counter-style` rules:
/// <https://www.w3.org/TR/css-counter-styles-3/#limited-chinese>.
pub(in crate::layout) fn chinese_longhand_marker(
    ordinal: i32,
    style: ChineseLonghandStyle,
) -> Option<String> {
    if !(-9999..=9999).contains(&ordinal) {
        return Some(numeric_marker_i32(ordinal, CJK_DECIMAL_DIGITS));
    }
    if ordinal == 0 {
        return Some(style.digits()[0].to_string());
    }

    let mut places = std::iter::successors(Some(ordinal.abs()), |value| Some(value / 10))
        .take(4)
        .enumerate()
        .map(|(place, value)| (value % 10, place))
        .collect::<Vec<_>>();
    while matches!(places.last(), Some((0, _))) {
        places.pop();
    }

    let digits = style.digits();
    let markers = style.markers();
    let mut output = String::new();
    let mut pending_zero = false;
    for &(digit, place) in places.iter().rev() {
        if digit == 0 {
            pending_zero = true;
            continue;
        }
        if pending_zero && !output.is_empty() {
            output.push_str(digits[0]);
        }
        pending_zero = false;
        if !(style.is_informal() && ordinal.abs() < 20 && place == 1 && digit == 1) {
            output.push_str(digits[digit as usize]);
        }
        output.push_str(markers[place]);
    }

    if ordinal < 0 {
        output = format!("{}{output}", style.negative());
    }
    Some(output)
}

/// Render CSS Counter Styles Level 3 `ethiopic-numeric`.
///
/// <https://www.w3.org/TR/css-counter-styles-3/#ethiopic-numeric-counter-style>
pub(in crate::layout) fn ethiopic_numeric_marker(ordinal: i32) -> Option<String> {
    if ordinal <= 0 {
        return Some(ordinal.to_string());
    }
    if ordinal == 1 {
        return Some("፩".to_string());
    }

    let mut groups = Vec::new();
    let mut value = ordinal;
    while value > 0 {
        groups.push(value % 100);
        value /= 100;
    }

    let mut output = String::new();
    for index in (0..groups.len()).rev() {
        let group = groups[index];
        let odd_index = index % 2 == 1;
        let most_significant = index + 1 == groups.len();
        if group != 0 && !(most_significant && group == 1) && !(odd_index && group == 1) {
            output.push_str(&ethiopic_group_text(group));
        }
        if odd_index && group != 0 {
            output.push('፻');
        } else if index != 0 && !odd_index {
            output.push('፼');
        }
    }
    Some(output)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ua_counter_styles() -> HashMap<String, CounterStyleRule> {
        crate::css::html5_user_agent_stylesheet()
            .counter_styles
            .iter()
            .cloned()
            .map(|style| (style.name.clone(), style))
            .collect()
    }

    fn rule(name: &str, system: CounterStyleSystem) -> CounterStyleRule {
        CounterStyleRule {
            name: name.to_string(),
            system,
            symbols: Vec::new(),
            additive_symbols: Vec::new(),
            prefix: None,
            suffix: None,
            negative: None,
            pad: None,
            range: None,
            fallback: None,
            speak_as: None,
        }
    }

    fn fixed_rule(name: &str, first: i32, symbols: &[&str]) -> CounterStyleRule {
        let mut rule = rule(name, CounterStyleSystem::Fixed(first));
        rule.symbols = symbols.iter().map(|symbol| (*symbol).to_string()).collect();
        rule
    }

    #[test]
    fn fallback_chains_are_unbounded_but_cycles_use_decimal() {
        let mut styles = HashMap::new();
        for index in 0..12 {
            let name = format!("style-{index}");
            let mut rule = fixed_rule(&name, 1, &["x"]);
            rule.range = Some(CounterStyleRange::Intervals(vec![
                CounterStyleRangeInterval { start: 1, end: 1 },
            ]));
            rule.fallback = Some(format!("style-{}", index + 1));
            styles.insert(name, rule);
        }
        let last = fixed_rule("style-12", 13, &["z"]);
        styles.insert(last.name.clone(), last);

        assert_eq!(
            custom_counter_text(styles.get("style-0").unwrap(), 13, &styles),
            Some("z".to_string())
        );

        let mut a = fixed_rule("a", 1, &["a"]);
        a.range = Some(CounterStyleRange::Intervals(vec![
            CounterStyleRangeInterval { start: 1, end: 1 },
        ]));
        a.fallback = Some("b".to_string());
        let mut b = fixed_rule("b", 1, &["b"]);
        b.range = Some(CounterStyleRange::Intervals(vec![
            CounterStyleRangeInterval { start: 1, end: 1 },
        ]));
        b.fallback = Some("a".to_string());
        let cycles = HashMap::from([(a.name.clone(), a.clone()), (b.name.clone(), b)]);
        assert_eq!(custom_counter_text(&a, 2, &cycles), Some("2".to_string()));
    }

    #[test]
    fn fallback_representation_keeps_the_requested_marker_affixes() {
        let mut requested = fixed_rule("requested", 1, &["a"]);
        requested.range = Some(CounterStyleRange::Intervals(vec![
            CounterStyleRangeInterval { start: 1, end: 1 },
        ]));
        requested.fallback = Some("fallback".to_string());
        requested.prefix = Some("[".to_string());
        requested.suffix = Some("]".to_string());
        let fallback = fixed_rule("fallback", 2, &["b"]);
        let styles = HashMap::from([
            (requested.name.clone(), requested.clone()),
            (fallback.name.clone(), fallback),
        ]);

        assert_eq!(
            custom_counter_marker_text(&requested, 2, &styles),
            Some(("[b]".to_string(), false))
        );
    }

    #[test]
    fn pad_uses_grapheme_clusters_and_includes_negative_affixes() {
        let mut combining = fixed_rule("combining", 1, &["a\u{0304}"]);
        combining.pad = Some((2, "o".to_string()));
        assert_eq!(
            custom_counter_text(&combining, 1, &HashMap::new()),
            Some("oa\u{0304}".to_string())
        );

        let mut emoji = fixed_rule("emoji", 1, &["👩‍💻"]);
        emoji.pad = Some((2, "o".to_string()));
        assert_eq!(
            custom_counter_text(&emoji, 1, &HashMap::new()),
            Some("o👩‍💻".to_string())
        );

        let mut negative = rule("negative", CounterStyleSystem::Numeric);
        negative.symbols = decimal_counter_symbols();
        negative.pad = Some((4, "0".to_string()));
        negative.negative = Some(("(".to_string(), ")".to_string()));
        assert_eq!(
            custom_counter_text(&negative, -2, &HashMap::new()),
            Some("(02)".to_string())
        );

        let fixed = fixed_rule("negative-fixed", -1, &["a"]);
        assert_eq!(
            custom_counter_text(&fixed, -1, &HashMap::new()),
            Some("a".to_string())
        );

        let cyclic = rule("negative-cyclic", CounterStyleSystem::Cyclic);
        let mut cyclic = cyclic;
        cyclic.symbols = vec!["a".into(), "b".into()];
        assert_eq!(
            custom_counter_text(&cyclic, -2, &HashMap::new()),
            Some("b".to_string())
        );
    }

    #[test]
    fn disclosure_styles_follow_writing_context_and_remain_extendable() {
        let mut extended = rule(
            "custom-disclosure",
            CounterStyleSystem::Extends("disclosure-closed".into()),
        );
        extended.prefix = Some("[".into());
        extended.suffix = Some("]".into());
        let styles = HashMap::from([(extended.name.clone(), extended.clone())]);
        let cases = [
            (ComputedStyle::initial(), "\u{25b8}", "\u{25be}"),
            (
                {
                    let mut style = ComputedStyle::initial();
                    style.direction = Direction::Rtl;
                    style
                },
                "\u{25c2}",
                "\u{25be}",
            ),
            (
                {
                    let mut style = ComputedStyle::initial();
                    style.writing_mode = WritingMode::VerticalLr;
                    style
                },
                "\u{25be}",
                "\u{25b8}",
            ),
            (
                {
                    let mut style = ComputedStyle::initial();
                    style.writing_mode = WritingMode::VerticalRl;
                    style.direction = Direction::Rtl;
                    style
                },
                "\u{25b4}",
                "\u{25c2}",
            ),
        ];

        for (style, closed, open) in cases {
            let context = CounterStyleRenderContext::for_style(&style);
            assert_eq!(
                counter_text_with_context(ListStyleType::DisclosureClosed, 1, &styles, context,),
                Some(closed.to_string())
            );
            assert_eq!(
                counter_text_with_context(ListStyleType::DisclosureOpen, 1, &styles, context),
                Some(open.to_string())
            );
            assert_eq!(
                custom_counter_marker_text_with_context(&extended, 1, &styles, context),
                Some((format!("[{closed}]"), false))
            );
        }
    }

    #[test]
    fn extends_cycles_repair_only_the_cycle_members_with_decimal_bases() {
        let mut a = rule("a", CounterStyleSystem::Extends("b".into()));
        a.prefix = Some("a".into());
        let mut b = rule("b", CounterStyleSystem::Extends("c".into()));
        b.suffix = Some("b".into());
        let mut c = rule("c", CounterStyleSystem::Extends("b".into()));
        c.pad = Some((2, "c".into()));
        let styles = HashMap::from([
            (a.name.clone(), a.clone()),
            (b.name.clone(), b),
            (c.name.clone(), c),
        ]);

        let a = resolve_counter_style(&a, &styles, 0);
        assert_eq!(a.prefix, "a");
        assert_eq!(a.suffix, "b");
        assert_eq!(a.pad, None);

        let b = resolve_counter_style(styles.get("b").unwrap(), &styles, 0);
        assert_eq!(b.prefix, "");
        assert_eq!(b.suffix, "b");
        assert_eq!(b.pad, None);

        let c = resolve_counter_style(styles.get("c").unwrap(), &styles, 0);
        assert_eq!(c.prefix, "");
        assert_eq!(c.suffix, ". ");
        assert_eq!(c.pad, Some((2, "c".into())));
    }

    #[test]
    fn complex_predefined_styles_are_extendable() {
        let custom = rule(
            "chapter",
            CounterStyleSystem::Extends("simp-chinese-informal".into()),
        );
        let styles = HashMap::from([(custom.name.clone(), custom.clone())]);
        let effective = resolve_counter_style(&custom, &styles, 0);

        assert_eq!(effective.predefined, Some("simp-chinese-informal"));
        assert_eq!(effective.suffix, "、");
        assert_eq!(
            custom_counter_text(&custom, 1_000, &styles),
            Some("一千".into())
        );
    }

    #[test]
    fn lookup_preserves_custom_case_and_normalizes_predefined_names() {
        let custom = rule("custom", CounterStyleSystem::Numeric);
        let predefined = rule("decimal-leading-zero", CounterStyleSystem::Numeric);
        let styles = HashMap::from([
            (custom.name.clone(), custom),
            (predefined.name.clone(), predefined),
        ]);
        assert!(counter_style_rule("Custom", &styles).is_none());
        assert!(counter_style_rule("custom", &styles).is_some());
        assert!(counter_style_rule("Decimal-Leading-Zero", &styles).is_some());
    }

    #[test]
    fn cjk_decimal_honors_its_ua_range_and_fallback() {
        let counter_styles = ua_counter_styles();
        let style = crate::css::parse_list_style_type("cjk-decimal").expect("valid style");

        assert_eq!(style, ListStyleType::Named("cjk-decimal".to_string()));
        assert_eq!(
            counter_text(style.clone(), 12_345, &counter_styles),
            Some("一二三四五".to_string())
        );
        assert_eq!(
            counter_text(style, -1, &counter_styles),
            Some("-1".to_string())
        );
    }

    #[test]
    fn list_style_none_suppresses_only_the_automatic_marker() {
        let counter_styles = HashMap::new();
        assert_eq!(
            automatic_marker_text(ListStyleType::None, 2, &counter_styles),
            None
        );

        let mut marker_style = ComputedStyle::initial();
        marker_style.marker_content = MarkerContent::Parts(vec![
            MarkerContentPart::Counter {
                name: LIST_ITEM_COUNTER_NAME.to_string(),
                style: Some(ListStyleType::Decimal),
            },
            MarkerContentPart::Text(". ".to_string()),
        ]);
        let stacks = HashMap::from([(LIST_ITEM_COUNTER_NAME.to_string(), vec![2])]);
        let mut quote_depth = 0;
        assert_eq!(
            marker_text(
                &marker_style,
                2,
                &counter_styles,
                &stacks,
                &mut quote_depth,
                CounterStyleRenderContext::for_style(&marker_style),
            ),
            Some(("2. ".to_string(), false))
        );
    }

    #[test]
    fn outside_anchor_preserves_line_start_and_baseline_as_distinct_positions() {
        let line_start = PageTopBlockPosition::new(100.0);
        let anchor = OutsideMarkerAnchor {
            principal_line_inline_span: PageInlineSpan::from_edges(20.0, 80.0),
            formatted_line_block_start: line_start,
            alphabetic_baseline: line_start.toward_block_end(layout_pt(12.0)),
        };

        assert_eq!(anchor.principal_line_inline_span.left_x(), 20.0);
        assert_eq!(anchor.principal_line_inline_span.right_x(), 80.0);
        assert_eq!(anchor.formatted_line_block_start.points(), 100.0);
        assert_eq!(anchor.alphabetic_baseline.points(), 88.0);
    }
}

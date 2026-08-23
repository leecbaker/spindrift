use super::super::super::*;

/// Tracks a legacy immediate float row view for callers that need the first
/// line's already-placed exclusions.
///
/// CSS 2.2 floats are shifted to the line's left or right edge and subsequent
/// floats are placed beside previous floats when space permits:
/// <https://www.w3.org/TR/CSS22/visuren.html#floats>.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(in crate::layout) struct FloatRunState {
    /// Full physical row span before same-row floats shorten it.
    ///
    /// CSS 2.2 places consecutive floats beside earlier floats when possible.
    /// This span is page physical `x` in the current block formatting context:
    /// <https://www.w3.org/TR/CSS22/visuren.html#floats>.
    pub(in crate::layout) row_span: PageInlineSpan,
    /// Remaining physical row span after same-row floats have been included.
    ///
    /// This is the immediate line-box availability for legacy float placement
    /// callers; durable later exclusions are stored as [`FloatShape`] entries
    /// in [`FloatContext`].
    pub(in crate::layout) available_span: PageInlineSpan,
    /// Physical block interval occupied by same-row floats.
    ///
    /// The span uses Quire's page top-edge convention: `top_y` is the row top
    /// and `bottom_y` moves downward as floats are added. CSS floats shorten
    /// later line boxes until the lowest same-row float bottom:
    /// <https://www.w3.org/TR/CSS22/visuren.html#floats>.
    pub(in crate::layout) occupied_block_span: PageBlockSpan,
    pub(in crate::layout) active: bool,
}

/// Durable float exclusion list for one block formatting context.
///
/// CSS 2.2 keeps floated margin boxes out of normal flow but shortens later
/// line boxes and formatting contexts around them in the same block formatting
/// context:
/// <https://www.w3.org/TR/CSS22/visuren.html#floats>.
#[derive(Debug, Clone, PartialEq)]
pub(in crate::layout) struct FloatContext {
    pub(in crate::layout) shapes: Vec<FloatShape>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(in crate::layout) struct FloatId(pub(in crate::layout) usize);

/// A page-local float placement expressed in the containing block's logical
/// axes and projected once into paint geometry.
///
/// CSS floats use physical `left` and `right`, while the containing block can
/// have a vertical or sideways writing mode. Retaining both representations at
/// this boundary prevents replay, exclusion queries, and fragmentation from
/// independently rebuilding an x/y rectangle with incompatible axes.
/// <https://www.w3.org/TR/CSS22/visuren.html#floats>
/// <https://www.w3.org/TR/css-writing-modes-4/#abstract-box>
#[derive(Debug, Clone, Copy, PartialEq)]
pub(in crate::layout) struct LogicalFloatPlacement {
    pub(in crate::layout) page_index: usize,
    pub(in crate::layout) writing_mode: WritingMode,
    pub(in crate::layout) direction: Direction,
    pub(in crate::layout) side: UsedFloatSide,
    /// Physical containing-block bounds used for the logical projection.
    pub(in crate::layout) containing: PageTopRect,
    /// Offset and extent from the containing block's logical inline-start.
    pub(in crate::layout) inline_span: LogicalInlineSpan,
    /// Offset and extent from the containing block's logical block-start.
    pub(in crate::layout) block_start: f32,
    pub(in crate::layout) block_size: f32,
    /// The single physical projection used by paint and physical collision
    /// adapters.
    pub(in crate::layout) margin_box: PageTopRect,
}

impl LogicalFloatPlacement {
    #[allow(clippy::too_many_arguments)]
    pub(in crate::layout) fn new(
        page_index: usize,
        writing_mode: WritingMode,
        direction: Direction,
        side: UsedFloatSide,
        containing: PageTopRect,
        inline_span: LogicalInlineSpan,
        block_start: f32,
        block_size: f32,
    ) -> Self {
        let margin_box = project_logical_float_margin_box(
            containing,
            writing_mode,
            direction,
            inline_span,
            block_start,
            block_size,
        );
        Self {
            page_index,
            writing_mode,
            direction,
            side,
            containing,
            inline_span,
            block_start: block_start.max(0.0),
            block_size: block_size.max(0.0),
            margin_box,
        }
    }

    /// Adapt an existing physical CSS 2.2 placement into the shared logical
    /// representation. This is the sole bridge for legacy float searches;
    /// callers must use [`Self::margin_box`] after construction.
    pub(in crate::layout) fn from_physical_margin_box(
        page_index: usize,
        writing_mode: WritingMode,
        direction: Direction,
        side: UsedFloatSide,
        containing: PageTopRect,
        margin_box: PageTopRect,
    ) -> Self {
        let axes = WritingModeAxes::new(writing_mode, direction);
        let inline_start = logical_axis_start_from_physical_rect(
            containing,
            margin_box,
            axes.physical_side(LogicalSide::InlineStart),
        );
        let block_start = logical_axis_start_from_physical_rect(
            containing,
            margin_box,
            axes.physical_side(LogicalSide::BlockStart),
        );
        let inline_size = if axes.swaps_physical_axes() {
            margin_box.height()
        } else {
            margin_box.width()
        };
        let block_size = if axes.swaps_physical_axes() {
            margin_box.width()
        } else {
            margin_box.height()
        };
        Self::new(
            page_index,
            writing_mode,
            direction,
            side,
            containing,
            LogicalInlineSpan::new(inline_start, inline_size),
            block_start,
            block_size,
        )
    }

    pub(in crate::layout) fn with_margin_box(
        self,
        containing: PageTopRect,
        margin_box: PageTopRect,
    ) -> Self {
        Self::from_physical_margin_box(
            self.page_index,
            self.writing_mode,
            self.direction,
            self.side,
            containing,
            margin_box,
        )
    }

    /// Rebind a fragmented float slice to the page whose paint and exclusion
    /// geometry it now owns. Its logical axes and containing-block projection
    /// remain the same used layout until the fragmentainer supplies a new
    /// containing rectangle.
    pub(in crate::layout) fn on_page(self, page_index: usize) -> Self {
        Self { page_index, ..self }
    }
}

fn project_logical_float_margin_box(
    containing: PageTopRect,
    writing_mode: WritingMode,
    direction: Direction,
    inline_span: LogicalInlineSpan,
    block_start: f32,
    block_size: f32,
) -> PageTopRect {
    let axes = WritingModeAxes::new(writing_mode, direction);
    let inline_side = axes.physical_side(LogicalSide::InlineStart);
    let block_side = axes.physical_side(LogicalSide::BlockStart);
    let (x, width) = match (inline_side, block_side) {
        (PhysicalSide::Left | PhysicalSide::Right, PhysicalSide::Top | PhysicalSide::Bottom) => (
            physical_axis_origin(
                containing,
                inline_side,
                inline_span.start(),
                inline_span.size(),
            ),
            inline_span.size(),
        ),
        (PhysicalSide::Top | PhysicalSide::Bottom, PhysicalSide::Left | PhysicalSide::Right) => (
            physical_axis_origin(containing, block_side, block_start, block_size),
            block_size,
        ),
        _ => unreachable!("logical axes must be perpendicular"),
    };
    let (top_y, height) = match (inline_side, block_side) {
        (PhysicalSide::Top | PhysicalSide::Bottom, PhysicalSide::Left | PhysicalSide::Right) => (
            physical_page_top_edge(
                containing,
                inline_side,
                inline_span.start(),
                inline_span.size(),
            ),
            inline_span.size(),
        ),
        (PhysicalSide::Left | PhysicalSide::Right, PhysicalSide::Top | PhysicalSide::Bottom) => (
            physical_page_top_edge(containing, block_side, block_start, block_size),
            block_size,
        ),
        _ => unreachable!("logical axes must be perpendicular"),
    };
    PageTopRect::new(x, top_y, width, height)
}

fn logical_axis_start_from_physical_rect(
    containing: PageTopRect,
    rect: PageTopRect,
    side: PhysicalSide,
) -> f32 {
    match side {
        PhysicalSide::Left => rect.x() - containing.x(),
        PhysicalSide::Right => containing.x() + containing.width() - rect.x() - rect.width(),
        PhysicalSide::Top => containing.top_y() - rect.top_y(),
        PhysicalSide::Bottom => {
            rect.top_y() - rect.height() - (containing.top_y() - containing.height())
        }
    }
    .max(0.0)
}

fn physical_axis_origin(containing: PageTopRect, side: PhysicalSide, start: f32, size: f32) -> f32 {
    match side {
        PhysicalSide::Left => containing.x() + start,
        PhysicalSide::Right => containing.x() + containing.width() - start - size,
        PhysicalSide::Top | PhysicalSide::Bottom => unreachable!("expected horizontal side"),
    }
}

fn physical_page_top_edge(
    containing: PageTopRect,
    side: PhysicalSide,
    start: f32,
    size: f32,
) -> f32 {
    match side {
        PhysicalSide::Top => containing.top_y() - start,
        PhysicalSide::Bottom => containing.top_y() - containing.height() + start + size,
        PhysicalSide::Left | PhysicalSide::Right => unreachable!("expected vertical side"),
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(in crate::layout) struct FloatShape {
    pub(in crate::layout) id: FloatId,
    /// Whether this is a CSS float or another in-flow participant in the
    /// shared exclusion query. Initial letters wrap later inline content, but
    /// are not floats: `clear`, float placement, and float containment must
    /// not observe them.
    pub(in crate::layout) kind: FlowExclusionKind,
    pub(in crate::layout) specified_side: Float,
    pub(in crate::layout) side: UsedFloatSide,
    pub(in crate::layout) source_order: usize,
    pub(in crate::layout) fragment_index: usize,
    pub(in crate::layout) starts_on_previous_page: bool,
    pub(in crate::layout) continues_on_next_page: bool,
    pub(in crate::layout) page_index: usize,
    pub(in crate::layout) rect: PageTopRect,
    /// Signed CSS margin-box extent on the physical inline axis.
    ///
    /// `rect` intentionally remains a non-negative geometry rectangle for
    /// painting and float exclusion. This extent preserves the CSS2 outer
    /// edge relationship when negative margins make the margin edges cross.
    pub(in crate::layout) outer_inline_extent: MarginBoxLength,
    /// The logical placement selected for a CSS float before it was projected
    /// into this page-local fragment. Initial-letter exclusions retain their
    /// own used layout record instead: they share wrap queries with floats,
    /// but are not CSS floats.
    pub(in crate::layout) placement: Option<LogicalFloatPlacement>,
    /// CSS Shapes float area used only for wrapping later content. Float
    /// placement itself continues to use `rect`, the CSS 2.2 margin box.
    pub(in crate::layout) area: FloatArea,
    /// Used initial-letter geometry when this is an in-flow initial-letter
    /// exclusion. CSS floats leave this empty.
    pub(in crate::layout) initial_letter: Option<InitialLetterLayout>,
}

/// Semantic role of a page-local flow exclusion.
///
/// CSS Inline Level 3 makes an initial letter a participant in the containing
/// block's line-wrap exclusion geometry without giving it CSS 2.2 float
/// behavior: <https://drafts.csswg.org/css-inline-3/#initial-letter-layout>.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::layout) enum FlowExclusionKind {
    Float,
    InitialLetter,
}

/// Stable used-layout identity and geometry for one initial letter on one
/// page-local fragment.
///
/// The initial letter's wrapped line span can include root-strut leading,
/// while its margin box is used for subsequent-initial clearance. Keeping
/// both representations together prevents line selection, replay, and
/// clearance from conflating those distinct CSS Inline concepts.
/// <https://drafts.csswg.org/css-inline-3/#initial-letter-position>
#[derive(Debug, Clone, PartialEq)]
pub(in crate::layout) struct InitialLetterLayout {
    pub(in crate::layout) source_order: usize,
    pub(in crate::layout) page_index: usize,
    pub(in crate::layout) writing_mode: WritingMode,
    pub(in crate::layout) direction: Direction,
    pub(in crate::layout) used_font_size: f32,
    /// Whether this geometry belongs to a graph-selection probe rather than
    /// the committed initial-letter fragment. Probe cleanup must never remove
    /// a real earlier initial merely because a short following block begins
    /// near its line-stack cursor.
    pub(in crate::layout) provisional: bool,
    /// Distance from the containing block's strut edge to the initial
    /// letter's margin-box block start. This is retained separately from the
    /// wrapping box because the latter covers whole line slabs.
    pub(in crate::layout) block_start_alignment_inset: f32,
    pub(in crate::layout) margin_box: PageTopRect,
    pub(in crate::layout) wrapping_box: PageTopRect,
    /// Logical line slots intersected by the margin-box exclusion. The range
    /// is source-local; its page identity above makes it safe to replay after
    /// page/column fragmentation.
    pub(in crate::layout) impacted_line_range: std::ops::Range<usize>,
    /// The contour used by `initial-letter-wrap`. Rectangular modes use
    /// `Rect`; a later glyph-outline adapter can populate a real contour
    /// without changing the shared exclusion representation.
    pub(in crate::layout) contour: FloatContour,
}

/// A resolved, page-local CSS Shapes float area.
#[derive(Debug, Clone, PartialEq)]
pub(in crate::layout) struct FloatArea {
    pub(in crate::layout) contour: FloatContour,
    /// Used `shape-margin` in page points. The offset is applied before the
    /// CSS Shapes margin-box clip.
    pub(in crate::layout) shape_margin: f32,
    /// Replaced image floats can have a provisional CSS 2.2 placement
    /// rectangle before their independently used image content box is known.
    /// Image contours retain that resolved margin clip for wrapping only.
    pub(in crate::layout) margin_clip: Option<PageTopRect>,
}

impl FloatArea {
    pub(in crate::layout) const RECT: Self = Self {
        contour: FloatContour::Rect,
        shape_margin: 0.0,
        margin_clip: None,
    };

    pub(in crate::layout) fn new(contour: FloatContour, shape_margin: f32) -> Self {
        Self {
            contour,
            shape_margin: shape_margin.max(0.0),
            margin_clip: None,
        }
    }

    pub(in crate::layout) fn with_margin_clip(mut self, margin_clip: PageTopRect) -> Self {
        self.margin_clip = Some(margin_clip);
        self
    }

    /// Horizontal outer edges occupied anywhere in a line slab.
    pub(in crate::layout) fn horizontal_edges(
        &self,
        margin_rect: PageTopRect,
        slab: PageBlockSpan,
    ) -> Option<PageInlineSpan> {
        let margin_rect = self.margin_clip.unwrap_or(margin_rect);
        let top = margin_rect.top_y().min(slab.top_y());
        let bottom = margin_rect.bottom_y().max(slab.bottom_y());
        if top <= bottom + super::exclusions::FLOAT_EPSILON {
            return None;
        }
        self.contour
            .horizontal_edges_with_margin(margin_rect, bottom, top, self.shape_margin)
            .map(|(left, right)| PageInlineSpan::from_edges(left, right))
    }

    /// Vertical outer edges occupied anywhere in a vertical-writing slab.
    pub(in crate::layout) fn vertical_edges(
        &self,
        margin_rect: PageTopRect,
        slab: PageInlineSpan,
    ) -> Option<PageBlockSpan> {
        let margin_rect = self.margin_clip.unwrap_or(margin_rect);
        let left = margin_rect.x().max(slab.left_x());
        let right = (margin_rect.x() + margin_rect.width()).min(slab.right_x());
        if right <= left + super::exclusions::FLOAT_EPSILON {
            return None;
        }
        self.contour
            .vertical_edges_with_margin(margin_rect, left, right, self.shape_margin)
            .map(|(bottom, top)| PageBlockSpan::from_edges(top, bottom))
    }

    pub(in crate::layout) fn has_discontinuous_horizontal_boundary_at(
        &self,
        clip: PageTopRect,
        top: f32,
    ) -> bool {
        self.shape_margin <= super::exclusions::FLOAT_EPSILON
            && self
                .contour
                .has_discontinuous_horizontal_boundary_at(clip, top)
    }

    pub(in crate::layout) fn horizontal_transition_tops(
        &self,
        clip: PageTopRect,
        slab_height: f32,
        output: &mut Vec<f32>,
    ) {
        self.contour
            .horizontal_transition_tops(clip, slab_height, output);
        if self.shape_margin <= super::exclusions::FLOAT_EPSILON {
            return;
        }
        match &self.contour {
            FloatContour::Rect => {}
            FloatContour::Circle {
                center_y, radius, ..
            } => output.extend([
                center_y + radius + self.shape_margin,
                center_y - radius - self.shape_margin,
            ]),
            FloatContour::Ellipse {
                center_y, radius_y, ..
            } => output.extend([
                center_y + radius_y + self.shape_margin,
                center_y - radius_y - self.shape_margin,
            ]),
            FloatContour::RoundedRect(rect) => {
                rect.outset(self.shape_margin)
                    .horizontal_transition_tops(slab_height, output);
            }
            FloatContour::Polygon { vertices, .. } => {
                for vertex in vertices {
                    output.extend([
                        vertex.top_y() + self.shape_margin,
                        vertex.top_y() - self.shape_margin,
                    ]);
                }
            }
            FloatContour::RasterAlpha { .. } => {}
        }
    }
}

/// Contours implemented by CSS Shapes milestone 1. Coordinates are page-local
/// and the enclosing margin rectangle clips every contour.
#[derive(Debug, Clone, PartialEq)]
pub(in crate::layout) enum FloatContour {
    Rect,
    RoundedRect(UsedRoundedRect),
    Circle {
        center_x: f32,
        center_y: f32,
        radius: f32,
    },
    Ellipse {
        center_x: f32,
        center_y: f32,
        radius_x: f32,
        radius_y: f32,
    },
    Polygon {
        vertices: Vec<PageTopPoint>,
        fill_rule: css::ShapeFillRule,
    },
    RasterAlpha {
        rect: PageTopRect,
        pixel_width: u32,
        pixel_height: u32,
        alpha: Vec<u8>,
        threshold: u8,
    },
}

#[derive(Debug, Clone, Copy)]
struct UsedEllipse {
    center_x: f32,
    center_y: f32,
    radius_x: f32,
    radius_y: f32,
}

/// Borrowed raster contour data shared by horizontal and vertical slab
/// queries. Keeping the decoded alpha plane together prevents the query API
/// from accepting mismatched image dimensions, thresholds, and pixels.
#[derive(Clone, Copy)]
struct RasterAlphaContour<'a> {
    rect: PageTopRect,
    pixel_width: u32,
    pixel_height: u32,
    alpha: &'a [u8],
    threshold: u8,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(in crate::layout) struct UsedRoundedRect {
    pub(in crate::layout) left: f32,
    pub(in crate::layout) right: f32,
    pub(in crate::layout) top: f32,
    pub(in crate::layout) bottom: f32,
    pub(in crate::layout) top_left: (f32, f32),
    pub(in crate::layout) top_right: (f32, f32),
    pub(in crate::layout) bottom_right: (f32, f32),
    pub(in crate::layout) bottom_left: (f32, f32),
}

impl FloatContour {
    /// Whether the contour's horizontal exclusion changes discontinuously at
    /// this clip boundary. Rectangular float areas must release the next line
    /// exactly at their block-end edge; unlike a curved contour, there is no
    /// intervening monotonic interval to bisect.
    pub(in crate::layout) fn has_discontinuous_horizontal_boundary_at(
        &self,
        clip: PageTopRect,
        top: f32,
    ) -> bool {
        if (top - clip.top_y()).abs() > f32::EPSILON && (top - clip.bottom_y()).abs() > f32::EPSILON
        {
            return false;
        }
        match self {
            Self::Rect => true,
            Self::RoundedRect(rect) => [
                rect.top_left,
                rect.top_right,
                rect.bottom_right,
                rect.bottom_left,
            ]
            .into_iter()
            .all(|(horizontal, vertical)| horizontal <= f32::EPSILON && vertical <= f32::EPSILON),
            Self::Circle { .. }
            | Self::Ellipse { .. }
            | Self::Polygon { .. }
            | Self::RasterAlpha { .. } => false,
        }
    }

    /// Add block-start positions where the horizontal slab intersection can
    /// change monotonic direction.
    ///
    /// A float retry may need to place a line between ordinary line-height
    /// rows.  These positions partition each Milestone 1 contour into ranges
    /// that can be searched for the first fitting line slab.
    /// <https://drafts.csswg.org/css-shapes-1/#shape-outside-property>
    pub(in crate::layout) fn horizontal_transition_tops(
        &self,
        clip: PageTopRect,
        slab_height: f32,
        output: &mut Vec<f32>,
    ) {
        output.extend([clip.top_y(), clip.bottom_y()]);
        match self {
            Self::Rect => {}
            Self::Circle { center_y, .. } | Self::Ellipse { center_y, .. } => {
                // The nearest point of the slab to the ellipse centre changes
                // when the centre enters or leaves the slab.
                output.extend([*center_y, *center_y + slab_height]);
            }
            Self::RoundedRect(rect) => rect.horizontal_transition_tops(slab_height, output),
            Self::Polygon { vertices, .. } => {
                for vertex in vertices {
                    output.extend([vertex.top_y(), vertex.top_y() + slab_height]);
                }
            }
            Self::RasterAlpha { rect, .. } => output.extend([rect.top_y(), rect.bottom_y()]),
        }
    }

    fn horizontal_edges(&self, clip: PageTopRect, bottom: f32, top: f32) -> Option<(f32, f32)> {
        match self {
            Self::Rect => Some((clip.x(), clip.x() + clip.width())),
            Self::Circle {
                center_x,
                center_y,
                radius,
            } => {
                ellipse_horizontal_edges(clip, *center_x, *center_y, *radius, *radius, bottom, top)
            }
            Self::Ellipse {
                center_x,
                center_y,
                radius_x,
                radius_y,
            } => ellipse_horizontal_edges(
                clip, *center_x, *center_y, *radius_x, *radius_y, bottom, top,
            ),
            Self::RoundedRect(rect) => rect.horizontal_edges(clip, bottom, top),
            Self::Polygon {
                vertices,
                fill_rule,
            } => polygon_horizontal_edges(clip, vertices, *fill_rule, bottom, top),
            Self::RasterAlpha {
                rect,
                pixel_width,
                pixel_height,
                alpha,
                threshold,
            } => raster_alpha_horizontal_edges(
                clip,
                RasterAlphaContour {
                    rect: *rect,
                    pixel_width: *pixel_width,
                    pixel_height: *pixel_height,
                    alpha,
                    threshold: *threshold,
                },
                bottom,
                top,
            ),
        }
    }

    fn horizontal_edges_with_margin(
        &self,
        clip: PageTopRect,
        bottom: f32,
        top: f32,
        margin: f32,
    ) -> Option<(f32, f32)> {
        if margin <= super::exclusions::FLOAT_EPSILON {
            return self.horizontal_edges(clip, bottom, top);
        }
        match self {
            Self::Rect => self.horizontal_edges(clip, bottom, top),
            Self::Circle {
                center_x,
                center_y,
                radius,
            } => ellipse_horizontal_edges(
                clip,
                *center_x,
                *center_y,
                radius + margin,
                radius + margin,
                bottom,
                top,
            ),
            Self::Ellipse {
                center_x,
                center_y,
                radius_x,
                radius_y,
            } => offset_ellipse_horizontal_edges(
                clip,
                UsedEllipse {
                    center_x: *center_x,
                    center_y: *center_y,
                    radius_x: *radius_x,
                    radius_y: *radius_y,
                },
                bottom,
                top,
                margin,
            ),
            Self::RoundedRect(rect) => rect.outset(margin).horizontal_edges(clip, bottom, top),
            Self::Polygon { vertices, .. } => {
                offset_polygon_horizontal_edges(clip, vertices, bottom, top, margin)
            }
            Self::RasterAlpha {
                rect,
                pixel_width,
                pixel_height,
                alpha,
                threshold,
            } => raster_alpha_horizontal_edges_with_margin(
                clip,
                RasterAlphaContour {
                    rect: *rect,
                    pixel_width: *pixel_width,
                    pixel_height: *pixel_height,
                    alpha,
                    threshold: *threshold,
                },
                bottom,
                top,
                margin,
            ),
        }
    }

    fn vertical_edges(&self, clip: PageTopRect, left: f32, right: f32) -> Option<(f32, f32)> {
        match self {
            Self::Rect => Some((clip.bottom_y(), clip.top_y())),
            Self::Circle {
                center_x,
                center_y,
                radius,
            } => ellipse_vertical_edges(clip, *center_x, *center_y, *radius, *radius, left, right),
            Self::Ellipse {
                center_x,
                center_y,
                radius_x,
                radius_y,
            } => ellipse_vertical_edges(
                clip, *center_x, *center_y, *radius_x, *radius_y, left, right,
            ),
            Self::RoundedRect(rect) => rect.vertical_edges(clip, left, right),
            Self::Polygon {
                vertices,
                fill_rule,
            } => polygon_vertical_edges(clip, vertices, *fill_rule, left, right),
            Self::RasterAlpha {
                rect,
                pixel_width,
                pixel_height,
                alpha,
                threshold,
            } => raster_alpha_vertical_edges(
                clip,
                RasterAlphaContour {
                    rect: *rect,
                    pixel_width: *pixel_width,
                    pixel_height: *pixel_height,
                    alpha,
                    threshold: *threshold,
                },
                left,
                right,
            ),
        }
    }

    fn vertical_edges_with_margin(
        &self,
        clip: PageTopRect,
        left: f32,
        right: f32,
        margin: f32,
    ) -> Option<(f32, f32)> {
        if margin <= super::exclusions::FLOAT_EPSILON {
            return self.vertical_edges(clip, left, right);
        }
        match self {
            Self::Rect => self.vertical_edges(clip, left, right),
            Self::Circle {
                center_x,
                center_y,
                radius,
            } => ellipse_vertical_edges(
                clip,
                *center_x,
                *center_y,
                radius + margin,
                radius + margin,
                left,
                right,
            ),
            Self::Ellipse {
                center_x,
                center_y,
                radius_x,
                radius_y,
            } => offset_ellipse_vertical_edges(
                clip,
                UsedEllipse {
                    center_x: *center_x,
                    center_y: *center_y,
                    radius_x: *radius_x,
                    radius_y: *radius_y,
                },
                left,
                right,
                margin,
            ),
            Self::RoundedRect(rect) => rect.outset(margin).vertical_edges(clip, left, right),
            Self::Polygon { vertices, .. } => {
                offset_polygon_vertical_edges(clip, vertices, left, right, margin)
            }
            Self::RasterAlpha {
                rect,
                pixel_width,
                pixel_height,
                alpha,
                threshold,
            } => raster_alpha_vertical_edges_with_margin(
                clip,
                RasterAlphaContour {
                    rect: *rect,
                    pixel_width: *pixel_width,
                    pixel_height: *pixel_height,
                    alpha,
                    threshold: *threshold,
                },
                left,
                right,
                margin,
            ),
        }
    }
}

fn ellipse_horizontal_edges(
    clip: PageTopRect,
    cx: f32,
    cy: f32,
    rx: f32,
    ry: f32,
    bottom: f32,
    top: f32,
) -> Option<(f32, f32)> {
    if rx <= 0.0 || ry <= 0.0 {
        return None;
    }
    let y = cy.clamp(bottom, top);
    if (y - cy).abs() > ry {
        return None;
    }
    let dx = rx * (1.0 - ((y - cy) / ry).powi(2)).max(0.0).sqrt();
    let left = (cx - dx).max(clip.x());
    let right = (cx + dx).min(clip.x() + clip.width());
    (right >= left).then_some((left, right))
}

fn raster_alpha_horizontal_edges(
    clip: PageTopRect,
    raster: RasterAlphaContour<'_>,
    bottom: f32,
    top: f32,
) -> Option<(f32, f32)> {
    let rows = raster_axis_indices(
        raster.rect.bottom_y(),
        raster.rect.top_y(),
        raster.pixel_height,
        bottom,
        top,
    )?;
    let mut left = f32::INFINITY;
    let mut right = f32::NEG_INFINITY;
    for row in rows {
        let start = row * raster.pixel_width as usize;
        let source = raster
            .alpha
            .get(start..start + raster.pixel_width as usize)?;
        let Some(first) = source.iter().position(|value| *value > raster.threshold) else {
            continue;
        };
        let Some(last) = source.iter().rposition(|value| *value > raster.threshold) else {
            continue;
        };
        left = left
            .min(raster.rect.x() + first as f32 / raster.pixel_width as f32 * raster.rect.width());
        right = right.max(
            raster.rect.x() + (last + 1) as f32 / raster.pixel_width as f32 * raster.rect.width(),
        );
    }
    let left = left.max(clip.x());
    let right = right.min(clip.x() + clip.width());
    (right >= left).then_some((left, right))
}

/// Return horizontal edges of a thresholded raster image after applying the
/// circular CSS `shape-margin` offset. Each opaque source pixel represents a
/// closed rectangular cell in the used image rectangle; the union of their
/// circular offsets is the raster image shape area.
fn raster_alpha_horizontal_edges_with_margin(
    clip: PageTopRect,
    raster: RasterAlphaContour<'_>,
    bottom: f32,
    top: f32,
    margin: f32,
) -> Option<(f32, f32)> {
    if raster.pixel_width == 0
        || raster.pixel_height == 0
        || raster.rect.width() <= 0.0
        || raster.rect.height() <= 0.0
    {
        return None;
    }
    let mut outer_left = f32::INFINITY;
    let mut outer_right = f32::NEG_INFINITY;
    let pixel_width_pt = raster.rect.width() / raster.pixel_width as f32;
    let pixel_height_pt = raster.rect.height() / raster.pixel_height as f32;
    for row in 0..raster.pixel_height as usize {
        let pixel_top = raster.rect.top_y() - row as f32 * pixel_height_pt;
        let pixel_bottom = pixel_top - pixel_height_pt;
        let vertical_distance = if pixel_top < bottom {
            bottom - pixel_top
        } else if pixel_bottom > top {
            pixel_bottom - top
        } else {
            0.0
        };
        if vertical_distance > margin {
            continue;
        }
        let horizontal_outset = (margin.mul_add(margin, -vertical_distance * vertical_distance))
            .max(0.0)
            .sqrt();
        let source = raster
            .alpha
            .get(row * raster.pixel_width as usize..(row + 1) * raster.pixel_width as usize)?;
        for (column, value) in source.iter().enumerate() {
            if *value <= raster.threshold {
                continue;
            }
            let pixel_left = raster.rect.x() + column as f32 * pixel_width_pt;
            outer_left = outer_left.min(pixel_left - horizontal_outset);
            outer_right = outer_right.max(pixel_left + pixel_width_pt + horizontal_outset);
        }
    }
    let left = outer_left.max(clip.x());
    let right = outer_right.min(clip.x() + clip.width());
    (right >= left).then_some((left, right))
}

fn raster_alpha_vertical_edges(
    clip: PageTopRect,
    raster: RasterAlphaContour<'_>,
    left: f32,
    right: f32,
) -> Option<(f32, f32)> {
    let columns = raster_axis_indices(
        raster.rect.x(),
        raster.rect.x() + raster.rect.width(),
        raster.pixel_width,
        left,
        right,
    )?;
    let mut bottom = f32::INFINITY;
    let mut top = f32::NEG_INFINITY;
    for column in columns {
        let mut first = None;
        let mut last = None;
        for row in 0..raster.pixel_height as usize {
            if raster
                .alpha
                .get(row * raster.pixel_width as usize + column)?
                > &raster.threshold
            {
                first.get_or_insert(row);
                last = Some(row);
            }
        }
        let Some(first) = first else {
            continue;
        };
        let Some(last) = last else {
            continue;
        };
        top = top.max(
            raster.rect.top_y() - first as f32 / raster.pixel_height as f32 * raster.rect.height(),
        );
        bottom = bottom.min(
            raster.rect.top_y()
                - (last + 1) as f32 / raster.pixel_height as f32 * raster.rect.height(),
        );
    }
    let bottom = bottom.max(clip.bottom_y());
    let top = top.min(clip.top_y());
    (top >= bottom).then_some((bottom, top))
}

/// Vertical-writing counterpart to [`raster_alpha_horizontal_edges_with_margin`].
fn raster_alpha_vertical_edges_with_margin(
    clip: PageTopRect,
    raster: RasterAlphaContour<'_>,
    left: f32,
    right: f32,
    margin: f32,
) -> Option<(f32, f32)> {
    if raster.pixel_width == 0
        || raster.pixel_height == 0
        || raster.rect.width() <= 0.0
        || raster.rect.height() <= 0.0
    {
        return None;
    }
    let mut outer_bottom = f32::INFINITY;
    let mut outer_top = f32::NEG_INFINITY;
    let pixel_width_pt = raster.rect.width() / raster.pixel_width as f32;
    let pixel_height_pt = raster.rect.height() / raster.pixel_height as f32;
    for column in 0..raster.pixel_width as usize {
        let pixel_left = raster.rect.x() + column as f32 * pixel_width_pt;
        let pixel_right = pixel_left + pixel_width_pt;
        let horizontal_distance = if pixel_right < left {
            left - pixel_right
        } else if pixel_left > right {
            pixel_left - right
        } else {
            0.0
        };
        if horizontal_distance > margin {
            continue;
        }
        let vertical_outset = (margin.mul_add(margin, -horizontal_distance * horizontal_distance))
            .max(0.0)
            .sqrt();
        for row in 0..raster.pixel_height as usize {
            if raster
                .alpha
                .get(row * raster.pixel_width as usize + column)?
                <= &raster.threshold
            {
                continue;
            }
            let pixel_top = raster.rect.top_y() - row as f32 * pixel_height_pt;
            let pixel_bottom = pixel_top - pixel_height_pt;
            outer_bottom = outer_bottom.min(pixel_bottom - vertical_outset);
            outer_top = outer_top.max(pixel_top + vertical_outset);
        }
    }
    let bottom = outer_bottom.max(clip.bottom_y());
    let top = outer_top.min(clip.top_y());
    (top >= bottom).then_some((bottom, top))
}

fn raster_axis_indices(
    source_low: f32,
    source_high: f32,
    pixel_count: u32,
    slab_low: f32,
    slab_high: f32,
) -> Option<std::ops::Range<usize>> {
    if pixel_count == 0 || source_high <= source_low {
        return None;
    }
    let low = source_low.max(slab_low);
    let high = source_high.min(slab_high);
    if high <= low {
        return None;
    }
    let scale = pixel_count as f32 / (source_high - source_low);
    let start = ((low - source_low) * scale).floor().max(0.0) as usize;
    let end = ((high - source_low) * scale).ceil().min(pixel_count as f32) as usize;
    (start < end).then_some(start..end)
}

fn ellipse_vertical_edges(
    clip: PageTopRect,
    cx: f32,
    cy: f32,
    rx: f32,
    ry: f32,
    left: f32,
    right: f32,
) -> Option<(f32, f32)> {
    if rx <= 0.0 || ry <= 0.0 {
        return None;
    }
    let x = cx.clamp(left, right);
    if (x - cx).abs() > rx {
        return None;
    }
    let dy = ry * (1.0 - ((x - cx) / rx).powi(2)).max(0.0).sqrt();
    let bottom = (cy - dy).max(clip.bottom_y());
    let top = (cy + dy).min(clip.top_y());
    (top >= bottom).then_some((bottom, top))
}

/// The horizontal extrema of the shape-margin offset of an ellipse.
///
/// At a source point outside the queried line slab, the circle of radius
/// `margin` contributes `sqrt(margin² - distance_to_slab²)` in the inline
/// direction. Both summands are concave along one half of the ellipse, so a
/// bounded ternary solve finds their unique maximum without tessellating the
/// contour.
fn offset_ellipse_horizontal_edges(
    clip: PageTopRect,
    ellipse: UsedEllipse,
    bottom: f32,
    top: f32,
    margin: f32,
) -> Option<(f32, f32)> {
    let extent = offset_ellipse_axis_extent(
        ellipse.center_y,
        ellipse.radius_y,
        ellipse.radius_x,
        bottom,
        top,
        margin,
    )?;
    let left = (ellipse.center_x - extent).max(clip.x());
    let right = (ellipse.center_x + extent).min(clip.x() + clip.width());
    (right >= left).then_some((left, right))
}

fn offset_ellipse_vertical_edges(
    clip: PageTopRect,
    ellipse: UsedEllipse,
    left: f32,
    right: f32,
    margin: f32,
) -> Option<(f32, f32)> {
    let extent = offset_ellipse_axis_extent(
        ellipse.center_x,
        ellipse.radius_x,
        ellipse.radius_y,
        left,
        right,
        margin,
    )?;
    let bottom = (ellipse.center_y - extent).max(clip.bottom_y());
    let top = (ellipse.center_y + extent).min(clip.top_y());
    (top >= bottom).then_some((bottom, top))
}

/// Return the maximal perpendicular extent of an ellipse after a circular
/// offset, over an axis-aligned slab. `source_axis`/`source_radius` are along
/// the slab axis and `perpendicular_radius` is the ellipse radius being
/// queried.
fn offset_ellipse_axis_extent(
    source_axis: f32,
    source_radius: f32,
    perpendicular_radius: f32,
    slab_low: f32,
    slab_high: f32,
    margin: f32,
) -> Option<f32> {
    if source_radius <= 0.0 || perpendicular_radius <= 0.0 {
        return None;
    }
    let source_low = source_axis - source_radius;
    let source_high = source_axis + source_radius;
    if slab_high < source_low - margin || slab_low > source_high + margin {
        return None;
    }

    let profile = |coordinate: f32| {
        let normalized = ((coordinate - source_axis) / source_radius).clamp(-1.0, 1.0);
        perpendicular_radius * (1.0 - normalized.powi(2)).max(0.0).sqrt()
    };
    let mut maximum = f32::NEG_INFINITY;
    let overlap_low = source_low.max(slab_low);
    let overlap_high = source_high.min(slab_high);
    if overlap_low <= overlap_high {
        maximum = profile(source_axis.clamp(overlap_low, overlap_high)) + margin;
    }

    for boundary in [slab_low, slab_high] {
        let interval_low = source_low.max(boundary - margin);
        let interval_high = source_high.min(boundary + margin);
        let (mut low, mut high) = if boundary == slab_low {
            (interval_low, interval_high.min(boundary))
        } else {
            (interval_low.max(boundary), interval_high)
        };
        if low > high {
            continue;
        }
        // The sum is concave, so this converges to the single stationary
        // maximum (or one of the interval endpoints) to sub-pixel precision.
        for _ in 0..40 {
            let first = (2.0 * low + high) / 3.0;
            let second = (low + 2.0 * high) / 3.0;
            let value = |coordinate: f32| {
                let distance = (coordinate - boundary).abs();
                profile(coordinate) + (margin.powi(2) - distance.powi(2)).max(0.0).sqrt()
            };
            if value(first) < value(second) {
                low = first;
            } else {
                high = second;
            }
        }
        for coordinate in [interval_low, interval_high, low, high] {
            let distance = (coordinate - boundary).abs();
            if distance <= margin + f32::EPSILON {
                maximum = maximum
                    .max(profile(coordinate) + (margin.powi(2) - distance.powi(2)).max(0.0).sqrt());
            }
        }
    }
    maximum.is_finite().then_some(maximum)
}

fn offset_polygon_horizontal_edges(
    clip: PageTopRect,
    vertices: &[PageTopPoint],
    bottom: f32,
    top: f32,
    margin: f32,
) -> Option<(f32, f32)> {
    let (left, right) = offset_polygon_axis_edges(
        vertices
            .iter()
            .map(|point| (point.top_y(), point.x()))
            .collect::<Vec<_>>()
            .as_slice(),
        bottom,
        top,
        margin,
    )?;
    let left = left.max(clip.x());
    let right = right.min(clip.x() + clip.width());
    (right >= left).then_some((left, right))
}

fn offset_polygon_vertical_edges(
    clip: PageTopRect,
    vertices: &[PageTopPoint],
    left: f32,
    right: f32,
    margin: f32,
) -> Option<(f32, f32)> {
    let (bottom, top) = offset_polygon_axis_edges(
        vertices
            .iter()
            .map(|point| (point.x(), point.top_y()))
            .collect::<Vec<_>>()
            .as_slice(),
        left,
        right,
        margin,
    )?;
    let bottom = bottom.max(clip.bottom_y());
    let top = top.min(clip.top_y());
    (top >= bottom).then_some((bottom, top))
}

/// Return the outer perpendicular edges of a closed polygon offset by a
/// circular shape-margin. The first coordinate is parallel to the queried
/// slab and the second is perpendicular.
fn offset_polygon_axis_edges(
    vertices: &[(f32, f32)],
    slab_low: f32,
    slab_high: f32,
    margin: f32,
) -> Option<(f32, f32)> {
    if vertices.is_empty() {
        return None;
    }
    let mut minimum = f32::INFINITY;
    let mut maximum = f32::NEG_INFINITY;
    for index in 0..vertices.len() {
        let start = vertices[index];
        let end = vertices[(index + 1) % vertices.len()];
        minimum = minimum.min(offset_segment_perpendicular_extreme(
            start, end, slab_low, slab_high, margin, false,
        ));
        maximum = maximum.max(offset_segment_perpendicular_extreme(
            start, end, slab_low, slab_high, margin, true,
        ));
    }
    (maximum >= minimum).then_some((minimum, maximum))
}

fn offset_segment_perpendicular_extreme(
    start: (f32, f32),
    end: (f32, f32),
    slab_low: f32,
    slab_high: f32,
    margin: f32,
    maximum: bool,
) -> f32 {
    let (axis_delta, perpendicular_delta) = (end.0 - start.0, end.1 - start.1);
    let direction = if maximum { 1.0 } else { -1.0 };
    let mut result = if maximum {
        f32::NEG_INFINITY
    } else {
        f32::INFINITY
    };
    let consider = |axis: f32, perpendicular: f32, boundary: f32| {
        let distance = (axis - boundary).abs();
        if distance <= margin + f32::EPSILON {
            Some(perpendicular + direction * (margin.powi(2) - distance.powi(2)).max(0.0).sqrt())
        } else {
            None
        }
    };

    // Source points already in the slab have the full perpendicular offset.
    let axis_low = start.0.min(end.0).max(slab_low);
    let axis_high = start.0.max(end.0).min(slab_high);
    if axis_low <= axis_high {
        if axis_delta.abs() <= f32::EPSILON {
            if let Some(value) = consider(
                start.0,
                if maximum {
                    start.1.max(end.1)
                } else {
                    start.1.min(end.1)
                },
                start.0,
            ) {
                result = if maximum {
                    result.max(value)
                } else {
                    result.min(value)
                };
            }
        } else {
            for axis in [axis_low, axis_high] {
                let t = (axis - start.0) / axis_delta;
                let perpendicular = start.1 + perpendicular_delta * t;
                let value = perpendicular + direction * margin;
                if maximum {
                    result = result.max(value);
                } else {
                    result = result.min(value);
                }
            }
        }
    }

    // On either side of the slab the perpendicular circle contribution and
    // the linear edge form a concave (or its negation, convex) function. Its
    // stationary point has a closed form.
    if axis_delta.abs() <= f32::EPSILON {
        for boundary in [slab_low, slab_high] {
            if let Some(value) = consider(
                start.0,
                if maximum {
                    start.1.max(end.1)
                } else {
                    start.1.min(end.1)
                },
                boundary,
            ) {
                result = if maximum {
                    result.max(value)
                } else {
                    result.min(value)
                };
            }
        }
        return result;
    }
    let slope = perpendicular_delta / axis_delta;
    for (boundary, upper_side) in [(slab_low, false), (slab_high, true)] {
        let (mut low, mut high) = (start.0.min(end.0), start.0.max(end.0));
        if upper_side {
            low = low.max(boundary);
            high = high.min(boundary + margin);
        } else {
            low = low.max(boundary - margin);
            high = high.min(boundary);
        }
        if low > high {
            continue;
        }
        let adjusted_slope = direction * slope;
        let stationary = boundary + adjusted_slope * margin / (1.0 + adjusted_slope.powi(2)).sqrt();
        for axis in [low, high, stationary.clamp(low, high)] {
            let t = (axis - start.0) / axis_delta;
            if let Some(value) = consider(axis, start.1 + perpendicular_delta * t, boundary) {
                result = if maximum {
                    result.max(value)
                } else {
                    result.min(value)
                };
            }
        }
    }
    result
}

fn polygon_horizontal_edges(
    clip: PageTopRect,
    vertices: &[PageTopPoint],
    fill_rule: css::ShapeFillRule,
    bottom: f32,
    top: f32,
) -> Option<(f32, f32)> {
    let boundary_epsilon = polygon_slab_interior_epsilon(bottom, top);
    let mut sample_y = vec![bottom + boundary_epsilon, top - boundary_epsilon];
    sample_y.extend(
        vertices
            .iter()
            .map(|vertex| vertex.top_y())
            .filter(|y| *y > bottom + boundary_epsilon && *y < top - boundary_epsilon),
    );
    let mut left = f32::INFINITY;
    let mut right = f32::NEG_INFINITY;
    for y in sample_y {
        let Some((sample_left, sample_right)) = polygon_scan_horizontal(vertices, fill_rule, y)
        else {
            continue;
        };
        left = left.min(sample_left);
        right = right.max(sample_right);
    }
    let left = left.max(clip.x());
    let right = right.min(clip.x() + clip.width());
    (right >= left).then_some((left, right))
}

fn polygon_vertical_edges(
    clip: PageTopRect,
    vertices: &[PageTopPoint],
    fill_rule: css::ShapeFillRule,
    left: f32,
    right: f32,
) -> Option<(f32, f32)> {
    let boundary_epsilon = polygon_slab_interior_epsilon(left, right);
    let mut sample_x = vec![left + boundary_epsilon, right - boundary_epsilon];
    sample_x.extend(
        vertices
            .iter()
            .map(|vertex| vertex.x())
            .filter(|x| *x > left + boundary_epsilon && *x < right - boundary_epsilon),
    );
    let mut bottom = f32::INFINITY;
    let mut top = f32::NEG_INFINITY;
    for x in sample_x {
        let Some((sample_bottom, sample_top)) = polygon_scan_vertical(vertices, fill_rule, x)
        else {
            continue;
        };
        bottom = bottom.min(sample_bottom);
        top = top.max(sample_top);
    }
    let bottom = bottom.max(clip.bottom_y());
    let top = top.min(clip.top_y());
    (top >= bottom).then_some((bottom, top))
}

fn polygon_slab_interior_epsilon(start: f32, end: f32) -> f32 {
    ((end - start).abs() * 1e-4).min(1e-3)
}

fn polygon_scan_horizontal(
    vertices: &[PageTopPoint],
    fill_rule: css::ShapeFillRule,
    y: f32,
) -> Option<(f32, f32)> {
    polygon_scan_intersections(
        vertices,
        fill_rule,
        y,
        |point| point.top_y(),
        |point| point.x(),
    )
}

fn polygon_scan_vertical(
    vertices: &[PageTopPoint],
    fill_rule: css::ShapeFillRule,
    x: f32,
) -> Option<(f32, f32)> {
    polygon_scan_intersections(
        vertices,
        fill_rule,
        x,
        |point| point.x(),
        |point| point.top_y(),
    )
}

fn polygon_scan_intersections(
    vertices: &[PageTopPoint],
    fill_rule: css::ShapeFillRule,
    coordinate: f32,
    primary: impl Fn(PageTopPoint) -> f32,
    secondary: impl Fn(PageTopPoint) -> f32,
) -> Option<(f32, f32)> {
    let mut crossings = Vec::new();
    for (start, end) in vertices
        .iter()
        .copied()
        .zip(vertices.iter().copied().cycle().skip(1))
        .take(vertices.len())
    {
        let start_primary = primary(start);
        let end_primary = primary(end);
        if !((start_primary <= coordinate && coordinate < end_primary)
            || (end_primary <= coordinate && coordinate < start_primary))
        {
            continue;
        }
        let ratio = (coordinate - start_primary) / (end_primary - start_primary);
        let secondary_coordinate = secondary(start) + (secondary(end) - secondary(start)) * ratio;
        let winding_delta = i32::from(end_primary > start_primary) * 2 - 1;
        crossings.push((secondary_coordinate, winding_delta));
    }
    crossings.sort_by(|left, right| left.0.total_cmp(&right.0));

    let mut index = 0;
    let mut filled = false;
    let mut winding = 0;
    let mut previous = None;
    let mut minimum = f32::INFINITY;
    let mut maximum = f32::NEG_INFINITY;
    while index < crossings.len() {
        let coordinate = crossings[index].0;
        if filled && let Some(previous) = previous {
            minimum = minimum.min(previous);
            maximum = maximum.max(coordinate);
        }
        let mut delta = 0;
        let mut count = 0;
        while index < crossings.len() && (crossings[index].0 - coordinate).abs() <= f32::EPSILON {
            delta += crossings[index].1;
            count += 1;
            index += 1;
        }
        match fill_rule {
            css::ShapeFillRule::EvenOdd if count % 2 == 1 => filled = !filled,
            css::ShapeFillRule::EvenOdd => {}
            css::ShapeFillRule::NonZero => {
                winding += delta;
                filled = winding != 0;
            }
        }
        previous = Some(coordinate);
    }
    (maximum >= minimum).then_some((minimum, maximum))
}

impl UsedRoundedRect {
    /// Offset a rounded rectangle by the circular `shape-margin` contour.
    /// Straight sides move outward and every quarter-ellipse radius grows by
    /// the same distance, which is the exact parallel curve of this contour.
    fn outset(self, margin: f32) -> Self {
        Self {
            left: self.left - margin,
            right: self.right + margin,
            top: self.top + margin,
            bottom: self.bottom - margin,
            top_left: (self.top_left.0 + margin, self.top_left.1 + margin),
            top_right: (self.top_right.0 + margin, self.top_right.1 + margin),
            bottom_right: (self.bottom_right.0 + margin, self.bottom_right.1 + margin),
            bottom_left: (self.bottom_left.0 + margin, self.bottom_left.1 + margin),
        }
    }

    fn horizontal_transition_tops(self, slab_height: f32, output: &mut Vec<f32>) {
        let corner_boundaries = [
            self.top - self.top_left.1,
            self.top - self.top_right.1,
            self.bottom + self.bottom_left.1,
            self.bottom + self.bottom_right.1,
        ];
        for boundary in corner_boundaries {
            output.extend([boundary, boundary + slab_height]);
        }
    }

    fn horizontal_edges(self, clip: PageTopRect, bottom: f32, top: f32) -> Option<(f32, f32)> {
        let bottom = bottom.max(self.bottom);
        let top = top.min(self.top);
        if top < bottom {
            return None;
        }
        let mut candidates = vec![bottom, top];
        candidates.extend([
            self.top - self.top_left.1,
            self.top - self.top_right.1,
            self.bottom + self.bottom_left.1,
            self.bottom + self.bottom_right.1,
        ]);
        let mut left = f32::INFINITY;
        let mut right = f32::NEG_INFINITY;
        for y in candidates.into_iter().filter(|y| *y >= bottom && *y <= top) {
            let (candidate_left, candidate_right) = self.horizontal_edges_at(y);
            left = left.min(candidate_left);
            right = right.max(candidate_right);
        }
        let left = left.max(clip.x());
        let right = right.min(clip.x() + clip.width());
        (right >= left).then_some((left, right))
    }

    fn horizontal_edges_at(self, y: f32) -> (f32, f32) {
        debug_assert!(y >= self.bottom && y <= self.top);
        let (mut left, mut right) = (self.left, self.right);
        if y > self.top - self.top_left.1.max(self.top_right.1) {
            if self.top_left.1 > 0.0 && y > self.top - self.top_left.1 {
                let dy = (y - (self.top - self.top_left.1)) / self.top_left.1;
                left = self.left + self.top_left.0 * (1.0 - (1.0 - dy.powi(2)).sqrt());
            }
            if self.top_right.1 > 0.0 && y > self.top - self.top_right.1 {
                let dy = (y - (self.top - self.top_right.1)) / self.top_right.1;
                right = self.right - self.top_right.0 * (1.0 - (1.0 - dy.powi(2)).sqrt());
            }
        } else if y < self.bottom + self.bottom_left.1.max(self.bottom_right.1) {
            if self.bottom_left.1 > 0.0 && y < self.bottom + self.bottom_left.1 {
                let dy = ((self.bottom + self.bottom_left.1) - y) / self.bottom_left.1;
                left = self.left + self.bottom_left.0 * (1.0 - (1.0 - dy.powi(2)).sqrt());
            }
            if self.bottom_right.1 > 0.0 && y < self.bottom + self.bottom_right.1 {
                let dy = ((self.bottom + self.bottom_right.1) - y) / self.bottom_right.1;
                right = self.right - self.bottom_right.0 * (1.0 - (1.0 - dy.powi(2)).sqrt());
            }
        }
        (left, right)
    }

    fn vertical_edges(self, clip: PageTopRect, left: f32, right: f32) -> Option<(f32, f32)> {
        let left = left.max(self.left);
        let right = right.min(self.right);
        if right < left {
            return None;
        }
        let mut candidates = vec![left, right];
        candidates.extend([
            self.left + self.top_left.0,
            self.left + self.bottom_left.0,
            self.right - self.top_right.0,
            self.right - self.bottom_right.0,
        ]);
        let mut bottom = f32::INFINITY;
        let mut top = f32::NEG_INFINITY;
        for x in candidates.into_iter().filter(|x| *x >= left && *x <= right) {
            let (candidate_bottom, candidate_top) = self.vertical_edges_at(x);
            bottom = bottom.min(candidate_bottom);
            top = top.max(candidate_top);
        }
        let bottom = bottom.max(clip.bottom_y());
        let top = top.min(clip.top_y());
        (top >= bottom).then_some((bottom, top))
    }

    fn vertical_edges_at(self, x: f32) -> (f32, f32) {
        debug_assert!(x >= self.left && x <= self.right);
        let (mut bottom, mut top) = (self.bottom, self.top);
        if x < self.left + self.top_left.0.max(self.bottom_left.0) {
            if self.top_left.0 > 0.0 && x < self.left + self.top_left.0 {
                let dx = ((self.left + self.top_left.0) - x) / self.top_left.0;
                top = self.top - self.top_left.1 * (1.0 - (1.0 - dx.powi(2)).sqrt());
            }
            if self.bottom_left.0 > 0.0 && x < self.left + self.bottom_left.0 {
                let dx = ((self.left + self.bottom_left.0) - x) / self.bottom_left.0;
                bottom = self.bottom + self.bottom_left.1 * (1.0 - (1.0 - dx.powi(2)).sqrt());
            }
        } else if x > self.right - self.top_right.0.max(self.bottom_right.0) {
            if self.top_right.0 > 0.0 && x > self.right - self.top_right.0 {
                let dx = (x - (self.right - self.top_right.0)) / self.top_right.0;
                top = self.top - self.top_right.1 * (1.0 - (1.0 - dx.powi(2)).sqrt());
            }
            if self.bottom_right.0 > 0.0 && x > self.right - self.bottom_right.0 {
                let dx = (x - (self.right - self.bottom_right.0)) / self.bottom_right.0;
                bottom = self.bottom + self.bottom_right.1 * (1.0 - (1.0 - dx.powi(2)).sqrt());
            }
        }
        (bottom, top)
    }
}

/// The two physical outer inline edges of a CSS float's margin box.
///
/// Unlike a line-wrap exclusion, a CSS margin box can have a negative used
/// inline extent when a negative margin exceeds the border-box width. CSS 2.2
/// float placement still aligns and fits that signed outer geometry; only its
/// positive-area portion can shorten a line box. Keeping those concerns
/// separate prevents a zero-width exclusion span from erasing the outer edge
/// that source-order placement must inspect:
/// <https://www.w3.org/TR/CSS22/visuren.html#float-position>.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(in crate::layout) struct FloatOuterInlineEdges {
    start_x: f32,
    end_x: f32,
}

impl FloatOuterInlineEdges {
    fn from_margin_box(margin_box: PageTopRect, signed_extent: MarginBoxLength) -> Self {
        Self {
            start_x: margin_box.x(),
            end_x: margin_box.x() + signed_extent.points(),
        }
    }

    pub(in crate::layout) fn signed_extent(self) -> MarginBoxLength {
        margin_box_pt(self.end_x - self.start_x)
    }

    /// Return the non-negative interval available to float-exclusion queries.
    ///
    /// A reversed pair of outer edges has no positive-area interval, but its
    /// `start_x` remains the legacy zero-width anchor used by left/right band
    /// intersection code.
    pub(in crate::layout) fn exclusion_span(self) -> PageInlineSpan {
        PageInlineSpan::new(self.start_x, self.signed_extent().points())
    }

    /// Whether a float has the CSS 2.2 placement required to share `band`.
    ///
    /// This deliberately compares the signed outer extent with the available
    /// width before checking the physical outer edge. A negative end margin on
    /// a right float can therefore put its painted border box beyond `band`
    /// while its right *margin* edge remains correctly aligned inside it.
    pub(in crate::layout) fn fits_at_used_side_in_band(
        self,
        side: UsedFloatSide,
        band: PageInlineSpan,
        epsilon: f32,
    ) -> bool {
        if self.signed_extent().points() > band.width() + epsilon {
            return false;
        }
        match side {
            UsedFloatSide::Left => (self.start_x - band.left_x()).abs() <= epsilon,
            UsedFloatSide::Right => (self.end_x - band.right_x()).abs() <= epsilon,
            UsedFloatSide::Top | UsedFloatSide::Bottom => false,
        }
    }
}

impl FloatShape {
    pub(in crate::layout) fn from_rect(
        id: FloatId,
        specified_side: Float,
        side: UsedFloatSide,
        source_order: usize,
        page_index: usize,
        rect: PageTopRect,
    ) -> Self {
        Self {
            id,
            kind: FlowExclusionKind::Float,
            specified_side,
            side,
            source_order,
            fragment_index: 0,
            starts_on_previous_page: false,
            continues_on_next_page: false,
            page_index,
            outer_inline_extent: margin_box_pt(rect.width()),
            rect,
            placement: None,
            area: FloatArea::RECT,
            initial_letter: None,
        }
    }

    pub(in crate::layout) fn from_fragment(fragment: &FloatPaintFragment) -> Self {
        Self {
            id: fragment.id,
            kind: FlowExclusionKind::Float,
            specified_side: fragment.specified_side,
            side: fragment.side,
            source_order: fragment.source_order,
            fragment_index: fragment.fragment_index,
            starts_on_previous_page: fragment.starts_on_previous_page,
            continues_on_next_page: fragment.continues_on_next_page,
            page_index: fragment.page_index,
            outer_inline_extent: fragment.outer_inline_extent,
            rect: fragment.rect,
            placement: Some(fragment.placement),
            area: fragment.area.clone(),
            initial_letter: None,
        }
    }

    #[cfg(test)]
    #[allow(clippy::too_many_arguments)]
    pub(in crate::layout) fn from_edges(
        id: FloatId,
        specified_side: Float,
        side: UsedFloatSide,
        source_order: usize,
        fragment_index: usize,
        starts_on_previous_page: bool,
        continues_on_next_page: bool,
        page_index: usize,
        left: f32,
        right: f32,
        top: f32,
        bottom: f32,
    ) -> Self {
        Self {
            id,
            kind: FlowExclusionKind::Float,
            specified_side,
            side,
            source_order,
            fragment_index,
            starts_on_previous_page,
            continues_on_next_page,
            page_index,
            outer_inline_extent: margin_box_pt((right - left).max(0.0)),
            rect: PageTopRect::new(left, top, (right - left).max(0.0), (top - bottom).max(0.0)),
            placement: None,
            area: FloatArea::RECT,
            initial_letter: None,
        }
    }

    /// Construct the page-local exclusion created by an in-flow initial
    /// letter. It shares contour querying with CSS floats but is excluded from
    /// float placement, `clear`, and float containment.
    /// <https://drafts.csswg.org/css-inline-3/#initial-letter-wrap>
    pub(in crate::layout) fn initial_letter_rect(
        id: FloatId,
        side: UsedFloatSide,
        layout: InitialLetterLayout,
    ) -> Self {
        // Initial-letter wrapping has the same slab-query contract as CSS
        // Shapes. Keep the used contour attached to the shared flow
        // exclusion rather than silently falling back to a rectangular float
        // area; this is what lets `initial-letter-wrap` evolve independently
        // of CSS 2.2 float placement.
        // <https://drafts.csswg.org/css-inline-3/#initial-letter-wrap>
        let area = FloatArea::new(layout.contour.clone(), 0.0);
        Self {
            id,
            kind: FlowExclusionKind::InitialLetter,
            // Only CSS floats have a specified float side. This value makes
            // shared diagnostics deterministic without granting float behavior.
            specified_side: Float::Left,
            side,
            source_order: layout.source_order,
            fragment_index: 0,
            starts_on_previous_page: false,
            continues_on_next_page: false,
            page_index: layout.page_index,
            outer_inline_extent: margin_box_pt(layout.margin_box.width()),
            rect: layout.wrapping_box,
            placement: None,
            area,
            initial_letter: Some(layout),
        }
    }

    pub(in crate::layout) fn is_css_float(&self) -> bool {
        self.kind == FlowExclusionKind::Float
    }

    /// Physical used margin box.
    ///
    /// An initial letter uses `rect` as its wrapping-box slab so the shared
    /// CSS Shapes query can include root-strut leading.  Its physical
    /// collision and clearance box is deliberately smaller when that leading
    /// is present. CSS floats have no separate initial-letter layout and use
    /// their normal margin rectangle directly.
    /// <https://drafts.csswg.org/css-inline-3/#initial-letter-position>
    pub(in crate::layout) fn physical_margin_box(&self) -> PageTopRect {
        self.initial_letter
            .as_ref()
            .map_or(self.rect, |layout| layout.margin_box)
    }

    /// The physical horizontal span occupied by this float's margin box.
    pub(in crate::layout) fn margin_box_inline_span(&self) -> PageInlineSpan {
        self.outer_inline_edges().exclusion_span()
    }

    /// Physical CSS 2.2 outer inline edges, including a signed negative
    /// margin-box extent when the two margin edges cross.
    pub(in crate::layout) fn outer_inline_edges(&self) -> FloatOuterInlineEdges {
        FloatOuterInlineEdges::from_margin_box(self.physical_margin_box(), self.outer_inline_extent)
    }

    /// The physical vertical span occupied by this float's margin box.
    pub(in crate::layout) fn margin_box_block_span(&self) -> PageBlockSpan {
        let margin_box = self.physical_margin_box();
        PageBlockSpan::new(margin_box.top_y(), margin_box.height())
    }

    pub(in crate::layout) fn translated_block(self, delta: LayoutLength) -> Self {
        let rect = PageTopRect::new(
            self.rect.x(),
            self.rect.top_y() + delta.points(),
            self.rect.width(),
            self.rect.height(),
        );
        Self {
            rect,
            outer_inline_extent: self.outer_inline_extent,
            placement: self
                .placement
                .map(|placement| placement.with_margin_box(placement.containing, rect)),
            ..self
        }
    }
}

/// Durable page-local representation of one floated box fragment.
///
/// CSS 2.2 floats exclude later content using their margin boxes, while CSS
/// Fragmentation can split a floated box across page fragmentainers. Each
/// visible fragment therefore needs both a paint-tree context and a page-local
/// exclusion shape:
/// <https://www.w3.org/TR/CSS22/visuren.html#floats> and
/// <https://www.w3.org/TR/css-break-3/#fragmentation-model>.
#[derive(Debug, Clone, PartialEq)]
pub(in crate::layout) struct FloatPaintFragment {
    pub(in crate::layout) id: FloatId,
    pub(in crate::layout) specified_side: Float,
    pub(in crate::layout) page_index: usize,
    pub(in crate::layout) side: UsedFloatSide,
    pub(in crate::layout) rect: PageTopRect,
    /// Signed outer inline extent retained separately from `rect` so a
    /// negative margin cannot be lost when the paint/exclusion rectangle is
    /// normalized to a non-negative size.
    pub(in crate::layout) outer_inline_extent: MarginBoxLength,
    pub(in crate::layout) placement: LogicalFloatPlacement,
    pub(in crate::layout) area: FloatArea,
    pub(in crate::layout) source_order: usize,
    pub(in crate::layout) fragment_index: usize,
    pub(in crate::layout) starts_on_previous_page: bool,
    pub(in crate::layout) continues_on_next_page: bool,
    pub(in crate::layout) context: PaintStackingContext,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::layout) enum UsedFloatSide {
    Left,
    Right,
    Top,
    Bottom,
}

/// The containing flow axes used to resolve line-relative float and clear
/// sides.
///
/// CSS Writing Modes resolves legacy `left` and `right` for `float` and
/// `clear` as line-relative sides of the containing block. Keeping this as a
/// distinct type prevents an orthogonal floated child from supplying its own
/// writing mode during placement:
/// <https://www.w3.org/TR/css-writing-modes-4/#line-mappings> and
/// <https://www.w3.org/TR/css-writing-modes-4/#orthogonal-flows>.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::layout) struct FloatPlacementAxes(FlowAxes);

impl FloatPlacementAxes {
    pub(in crate::layout) const fn new(
        containing_writing_mode: WritingMode,
        containing_direction: Direction,
    ) -> Self {
        Self(FlowAxes::new(containing_writing_mode, containing_direction))
    }

    pub(in crate::layout) fn for_style(containing_style: &ComputedStyle) -> Self {
        Self(FlowAxes::for_style(containing_style))
    }

    pub(in crate::layout) fn writing_mode(self) -> WritingMode {
        self.0.writing_mode()
    }

    pub(in crate::layout) fn direction(self) -> Direction {
        self.0.direction()
    }

    pub(in crate::layout) fn inline_start_side(self) -> PhysicalSide {
        self.0.inline_start_side()
    }

    fn line_left_side(self) -> PhysicalSide {
        self.0.line_left_side()
    }

    fn line_right_side(self) -> PhysicalSide {
        self.0.line_right_side()
    }

    fn inline_end_side(self) -> PhysicalSide {
        self.0.inline_end_side()
    }
}

/// Signed paint-space origin adjustment used while replaying a physically
/// placed float in its own formatting axes.
///
/// A bottom-origin vertical containing block expresses its inline coordinates
/// relative to the containing content bottom. Isolated replay uses the page's
/// top-based block cursor instead, so crossing that boundary requires one
/// explicit paint translation. Keeping the adjustment typed prevents it from
/// escaping into the containing flow's committed layout geometry.
/// <https://www.w3.org/TR/css-writing-modes-4/#logical-to-physical>
/// <https://www.w3.org/TR/css-writing-modes-4/#orthogonal-flows>
#[derive(Debug, Clone, Copy, PartialEq)]
pub(in crate::layout) struct FloatReplayBlockOriginAdjustment(LayoutLength);

impl FloatReplayBlockOriginAdjustment {
    pub(in crate::layout) fn for_containing_inline_axis(
        axes: FloatPlacementAxes,
        containing_physical_top: PageTopBlockPosition,
        containing_logical_inline_size: LogicalInlineContentSize,
    ) -> Self {
        let adjustment = if axes.inline_start_side() == PhysicalSide::Bottom {
            containing_physical_top.points() - containing_logical_inline_size.points()
        } else {
            0.0
        };
        Self(layout_pt(adjustment))
    }

    pub(in crate::layout) fn paint_translation(self) -> PaintTranslation {
        PaintTranslation::new(0.0, self.0.points())
    }
}

impl UsedFloatSide {
    pub(in crate::layout) fn from_float(
        float: Float,
        placement_axes: FloatPlacementAxes,
    ) -> Option<Self> {
        match float {
            Float::None => None,
            // GCPM footnotes use the `float` property as an extraction trigger,
            // not as an ordinary float-exclusion side. The footnote layout
            // phase consumes them before they can enter this model.
            Float::Footnote => None,
            Float::Left => Some(Self::from_physical_side(placement_axes.line_left_side())),
            Float::Right => Some(Self::from_physical_side(placement_axes.line_right_side())),
            Float::InlineStart => {
                Some(Self::from_physical_side(placement_axes.inline_start_side()))
            }
            Float::InlineEnd => Some(Self::from_physical_side(placement_axes.inline_end_side())),
        }
    }

    pub(in crate::layout) fn from_physical_side(side: PhysicalSide) -> Self {
        match side {
            PhysicalSide::Left => Self::Left,
            PhysicalSide::Right => Self::Right,
            PhysicalSide::Top => Self::Top,
            PhysicalSide::Bottom => Self::Bottom,
        }
    }

    pub(in crate::layout) fn matches_clear(
        self,
        clear: Clear,
        placement_axes: FloatPlacementAxes,
    ) -> bool {
        let clear_side = match clear {
            Clear::None => return false,
            Clear::Both => return true,
            Clear::Left => Self::from_physical_side(placement_axes.line_left_side()),
            Clear::Right => Self::from_physical_side(placement_axes.line_right_side()),
            Clear::InlineStart => Self::from_physical_side(placement_axes.inline_start_side()),
            Clear::InlineEnd => Self::from_physical_side(placement_axes.inline_end_side()),
        };
        self == clear_side
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(in crate::layout) struct FloatBand {
    /// The remaining physical line-box span in page coordinates after active
    /// CSS floats have shortened the row.
    ///
    /// CSS 2.2 defines floats as shortening line boxes in the same block
    /// formatting context. The span is physical page `x`, not logical inline
    /// coordinates; vertical writing modes must use [`LogicalFloatBand`]:
    /// <https://www.w3.org/TR/CSS22/visuren.html#floats>.
    pub(in crate::layout) span: PageInlineSpan,
}

impl FloatBand {
    pub(in crate::layout) fn from_span(span: PageInlineSpan) -> Self {
        Self { span }
    }

    pub(in crate::layout) fn from_edges(left: f32, right: f32) -> Self {
        Self::from_span(PageInlineSpan::from_edges(left, right))
    }

    pub(in crate::layout) fn left(self) -> f32 {
        self.span.left_x()
    }

    pub(in crate::layout) fn right(self) -> f32 {
        self.span.right_x()
    }

    pub(in crate::layout) fn width(self) -> f32 {
        self.span.width()
    }
}

/// Physical slabs queried by the float exclusion algorithm.
///
/// Callers normalize writing-mode-specific logical geometry into these two
/// page-local intervals before asking for a logical float band.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(in crate::layout) struct FloatBandQuery {
    pub(in crate::layout) horizontal_slab: PageInlineSpan,
    pub(in crate::layout) vertical_slab: PageBlockSpan,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(in crate::layout) struct LogicalFloatBand {
    /// Available logical inline interval after float exclusions.
    ///
    /// CSS Writing Modes defines inline coordinates independently from the
    /// physical page axis. This span is logical inline progress inside the
    /// queried line/slab, after active CSS floats have shortened it:
    /// <https://www.w3.org/TR/css-writing-modes-4/#abstract-box>.
    pub(in crate::layout) inline_span: LogicalInlineSpan,
    /// Physical page-y interval that corresponds to the available inline slab.
    ///
    /// Vertical writing modes can shorten the physical top or bottom of the
    /// slab while still reporting a logical inline span to inline layout.
    pub(in crate::layout) block_span: PageBlockSpan,
}

impl LogicalFloatBand {
    pub(in crate::layout) fn new(
        inline_span: LogicalInlineSpan,
        block_span: PageBlockSpan,
    ) -> Self {
        Self {
            inline_span,
            block_span,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(in crate::layout) struct FloatBandPlacement {
    /// Page-top origin of the float-avoidance band.
    ///
    /// CSS 2.2 places a float as far left or right as possible while its top
    /// edge is at or below the current line, after `clear` and active float
    /// exclusions are applied. The top-edge convention matches block layout's
    /// downward cursor before paint conversion:
    /// <https://www.w3.org/TR/CSS22/visuren.html#floats>.
    pub(in crate::layout) origin: PageTopPoint,
    /// Physical line-box span available at this float's block position.
    ///
    /// CSS floats shorten later line boxes in the same block formatting
    /// context. This span is the page-local horizontal band that accepted the
    /// float, not a CSS logical inline interval; vertical-writing float
    /// avoidance maps its logical inline availability into this typed result.
    /// <https://www.w3.org/TR/CSS22/visuren.html#floats>.
    pub(in crate::layout) available_span: PageInlineSpan,
}

/// Whether one edge of a normal-flow BFC root must remain in its containing
/// inline span during float avoidance.
///
/// CSS 2.2's normal block-width equation can place a border box outside its
/// containing block when the corresponding physical margin is negative. This
/// is a constraint on the *border box*, not on the residual float band:
/// <https://www.w3.org/TR/CSS22/visuren.html#floats>.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::layout) enum FloatAvoidanceInlineContainment {
    Required,
    PermittedNegativeMarginOverflow,
}

/// The normal-flow border box measured for one float-avoidance candidate.
///
/// Float margin boxes establish exclusions, while a BFC root is tested using
/// this resolved border box. Keeping the collision rectangle and containment
/// policy together prevents callers from accidentally treating the residual
/// float band (or a margin box) as the root's geometry:
/// <https://www.w3.org/TR/CSS22/visuren.html#floats>.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(in crate::layout) struct FloatAvoidanceCandidate {
    /// Physical inline span of the normal-flow border box before relative
    /// positioning is applied.
    pub(in crate::layout) normal_flow_border_box_inline_span: PageInlineSpan,
    pub(in crate::layout) normal_flow_border_box_block_size: BorderBoxLength,
    pub(in crate::layout) inline_start_containment: FloatAvoidanceInlineContainment,
    pub(in crate::layout) inline_end_containment: FloatAvoidanceInlineContainment,
}

impl FloatAvoidanceCandidate {
    pub(in crate::layout) fn permits_inline_start_overflow(self) -> bool {
        matches!(
            self.inline_start_containment,
            FloatAvoidanceInlineContainment::PermittedNegativeMarginOverflow
        )
    }

    pub(in crate::layout) fn permits_inline_end_overflow(self) -> bool {
        matches!(
            self.inline_end_containment,
            FloatAvoidanceInlineContainment::PermittedNegativeMarginOverflow
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(in crate::layout) struct FloatAvoidingBfcPlacement {
    /// Physical border-box inline-start selected by the fixed-point
    /// measurement. This differs from `left`, the residual float-band edge,
    /// whenever the normal block-width equation gives the BFC root a used
    /// start margin.
    pub(in crate::layout) placement: FloatBandPlacement,
    pub(in crate::layout) candidate: FloatAvoidanceCandidate,
}

/// A normal-flow margin-box placement selected after float avoidance.
///
/// The origin is the actual margin-box position after direction-sensitive
/// placement, while `available_span` preserves the residual exclusion band
/// that selected it:
/// <https://www.w3.org/TR/CSS22/visuren.html#floats>.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(in crate::layout) struct FloatPlacement {
    pub(in crate::layout) origin: PageTopPoint,
    pub(in crate::layout) available_span: PageInlineSpan,
}

impl FloatBandPlacement {
    pub(in crate::layout) fn new(band: FloatBand, top: PageTopBlockPosition) -> Self {
        Self {
            origin: PageTopPoint::new(band.span.left_x(), top.points()),
            available_span: band.span,
        }
    }

    /// Return the physical margin-box left edge for a horizontal CSS float.
    ///
    /// CSS 2.2 aligns a float's outer edge with the available band edge. A
    /// right float whose signed outer extent exceeds that band therefore
    /// overflows to the left; clamping its start would align the wrong edge.
    /// <https://www.w3.org/TR/CSS22/visuren.html#float-position>
    pub(in crate::layout) fn inline_float_margin_box_left(
        self,
        side: UsedFloatSide,
        outer_inline_extent: MarginBoxLength,
    ) -> f32 {
        match side {
            UsedFloatSide::Left => self.available_span.left_x(),
            UsedFloatSide::Right => self.available_span.right_x() - outer_inline_extent.points(),
            UsedFloatSide::Top | UsedFloatSide::Bottom => {
                unreachable!("horizontal float placement requires a left or right float")
            }
        }
    }
}

impl FloatPlacement {
    pub(in crate::layout) fn new(origin: PageTopPoint, available_span: PageInlineSpan) -> Self {
        Self {
            origin,
            available_span,
        }
    }
}

/// Matching float geometry for a single hypothetical block border edge.
///
/// This is deliberately only a float-context query. CSS 2.2 first establishes
/// the no-`clear` hypothetical border edge; block flow alone then decides
/// whether a matching target introduces *clearance* and re-resolves adjoining
/// margins. Keeping that margin-collapse decision outside [`FloatContext`]
/// prevents float placement from accidentally manufacturing a normal-flow
/// clearance boundary.
/// <https://www.w3.org/TR/CSS22/visuren.html#flow-control>
#[derive(Debug, Clone, Copy, PartialEq)]
pub(in crate::layout) struct FloatClearanceTarget {
    /// The lowest relevant float outer block-end in this fragmentainer.
    pub(in crate::layout) lowest_matching_outer_block_end:
        Option<super::exclusions::ClearedFloatOuterBlockEnd>,
    /// A matching float whose next fragment must be cleared before normal flow
    /// can resume.
    pub(in crate::layout) continued_float: Option<FloatId>,
}

/// Whether one block's own block-start margin remained adjoining to its
/// parent after its used clearance was resolved.
/// <https://www.w3.org/TR/CSS22/box.html#collapsing-margins>
/// <https://www.w3.org/TR/CSS22/visuren.html#flow-control>
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(in crate::layout) enum BlockMarginCollapseBoundary {
    #[default]
    Adjoining,
    SeparatedByClearance,
}

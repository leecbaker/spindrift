use std::collections::HashSet;

use super::*;
use crate::layout::assets::{DocumentPageIndex, PendingPositionedFragmentation};
use crate::layout::block::{DirectBlockLayoutConstraint, FloatReplayClearanceBoundary};

/// The writing-mode and direction that establish the document's initial
/// containing block.
///
/// For an HTML document, CSS Writing Modes takes these values from the first
/// eligible `body` child when the root does not have property containment.
/// The root's own style remains the inheritance source for generated page
/// margin boxes, so this deliberately models the principal-flow used values
/// separately from [`document_root_style`].
/// <https://www.w3.org/TR/css-writing-modes-4/#principal-flow>
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::layout) struct DocumentPrincipalFlow {
    pub(in crate::layout) writing_mode: WritingMode,
    pub(in crate::layout) direction: Direction,
    pub(in crate::layout) text_orientation: TextOrientation,
    pub(in crate::layout) source: PrincipalFlowSource,
}

/// The element that supplies the document's used principal flow.
///
/// A propagated body remains a document-canvas box; this identity lets layout
/// distinguish it from an ordinary root block sibling without changing the
/// root's cascaded style.
/// <https://www.w3.org/TR/css-writing-modes-4/#principal-flow>
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::layout) enum PrincipalFlowSource {
    Root,
    Body(ElementId),
}

/// State retained while the propagated HTML body is laid out as the document
/// canvas.
///
/// The body remains a real layout box, but its automatic canvas span is not a
/// root-sibling contribution. Its inline-end inset and trailing child margin
/// are instead retained until the root's following inline content has been
/// placed in the principal flow.
/// <https://www.w3.org/TR/css-writing-modes-4/#principal-flow>
#[derive(Debug, Clone, Copy, PartialEq)]
pub(in crate::layout) struct ActiveDocumentCanvas {
    pub(in crate::layout) body: Option<ElementId>,
    pub(in crate::layout) inline_end_inset: LayoutLength,
    /// The physical page-inline origin used by the canvas's first child.
    pub(in crate::layout) inline_origin: PageTopBlockPosition,
    /// Logical block track occupied by the document canvas itself. This is
    /// not derived from descendant ink or child box spans: an automatic body
    /// canvas occupies the resolved initial containing-block track.
    pub(in crate::layout) block_track_occupancy: LayoutLength,
    pub(in crate::layout) trailing_child_block_margin: LayoutLength,
}

/// The observable continuation left by a propagated HTML body after its
/// canvas layout has completed.
///
/// Keep the source facts separate rather than pre-combining them into a
/// writing-mode-specific scalar. The root principal-flow traversal projects
/// this record through the resolved principal-flow axes before it enters the
/// following inline sequence.
/// <https://www.w3.org/TR/css-writing-modes-4/#principal-flow>
#[derive(Debug, Clone, Copy, PartialEq)]
pub(in crate::layout) struct CompletedDocumentCanvas {
    pub(in crate::layout) body: Option<ElementId>,
    pub(in crate::layout) source_page: DocumentPageIndex,
    pub(in crate::layout) source_block_track: PageInlineSpan,
    pub(in crate::layout) inline_origin: PageTopBlockPosition,
    /// The canvas inset beyond its inline end, retained so bottom-origin
    /// flows can establish a fresh fragmentainer origin without paint replay.
    pub(in crate::layout) inline_end_inset: LayoutLength,
    pub(in crate::layout) block_end_inset: LayoutLength,
    pub(in crate::layout) block_track_occupancy: LayoutLength,
    pub(in crate::layout) trailing_child_block_margin: LayoutLength,
}

/// Root-owned placement selected before one source-ordered root inline
/// sequence consumes a completed propagated document canvas.
///
/// This is deliberately layout state, not a paint translation. It gives line
/// construction the fragmentainer and logical track that the canvas leaves
/// behind.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(in crate::layout) enum RootInlineCanvasPlacement {
    RemainingTrack {
        block_track: PageInlineSpan,
        inline_origin: PageTopBlockPosition,
    },
    NextPage {
        inline_origin: PageTopBlockPosition,
    },
}

/// Shared state between a propagated document body and root inline content.
///
/// The body stays a real canvas box, but its automatic canvas width must not
/// replace the root's logical block cursor. Instead the body exports one
/// completed logical-flow contribution for following root generated content.
/// <https://www.w3.org/TR/css-writing-modes-4/#principal-flow>
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub(in crate::layout) struct RootPrincipalFlowContext {
    pub(in crate::layout) active_canvas: Option<ActiveDocumentCanvas>,
    pub(in crate::layout) completed_canvas: Option<CompletedDocumentCanvas>,
    /// A completion currently being consumed by one source-ordered root
    /// inline sequence. Keeping it until that sequence returns avoids
    /// treating a page transition as a paint-time side effect.
    pub(in crate::layout) active_root_inline_canvas: Option<CompletedDocumentCanvas>,
}

/// Placement adjustment for a generated root pseudo-element whose computed
/// writing mode remains distinct from the principal flow used by the initial
/// containing block.
///
/// The element's computed style is never changed. This record only supplies
/// the physical block-start edge and the propagated body's canvas inset used
/// while projecting that one root pseudo box into the page area.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(in crate::layout) struct RootPseudoBlockProjection {
    pub(in crate::layout) element: ElementId,
    pub(in crate::layout) block_start: PhysicalSide,
    pub(in crate::layout) block_end_inset: LayoutLength,
}

impl DocumentPrincipalFlow {
    pub(in crate::layout) fn from_style(style: &ComputedStyle) -> Self {
        Self {
            writing_mode: style.writing_mode,
            // Keep the computed direction separately from the used direction:
            // text-orientation can force the latter in vertical flow without
            // changing the root's computed direction.
            // <https://drafts.csswg.org/css-writing-modes-4/#text-orientation>
            direction: style.direction,
            text_orientation: style.text_orientation,
            source: PrincipalFlowSource::Root,
        }
    }

    /// Returns whether `element` supplies the used principal flow.
    pub(in crate::layout) fn is_source_body(self, element: &Element) -> bool {
        self.source == PrincipalFlowSource::Body(element.id)
    }

    /// Whether the used principal flow is supplied by an eligible propagated
    /// body, rather than by the HTML root itself.
    pub(in crate::layout) fn has_propagated_body(self) -> bool {
        matches!(self.source, PrincipalFlowSource::Body(_))
    }

    /// Returns the used inline base direction of the resolved principal flow.
    pub(in crate::layout) fn used_direction(self) -> Direction {
        if self.writing_mode.has_vertical_lines()
            && self.text_orientation == TextOrientation::Upright
        {
            Direction::Ltr
        } else {
            self.direction
        }
    }

    /// Resolve paged-media progression from the document's principal writing
    /// mode, rather than from its inline base direction alone.
    ///
    /// In particular, vertical and sideways modes have fixed page
    /// progression regardless of `direction`.
    /// <https://drafts.csswg.org/css-writing-modes-4/#page-progression>
    pub(in crate::layout) fn page_progression_direction(self) -> Direction {
        match self.writing_mode {
            WritingMode::HorizontalTb => self.used_direction(),
            WritingMode::VerticalRl | WritingMode::SidewaysRl => Direction::Rtl,
            WritingMode::VerticalLr | WritingMode::SidewaysLr => Direction::Ltr,
        }
    }

    /// Produces the root's layout-only flow style.
    ///
    /// The propagated values establish the initial containing block and the
    /// root formatting context, but they do not alter the root's computed
    /// style or the inheritance parent used to build root pseudo-elements.
    /// <https://www.w3.org/TR/css-writing-modes-4/#principal-flow>
    pub(in crate::layout) fn root_layout_style(self, root_style: &ComputedStyle) -> ComputedStyle {
        let mut used = root_style.clone();
        used.writing_mode = self.writing_mode;
        used.direction = self.direction;
        used.text_orientation = self.text_orientation;
        used
    }
}

impl CompletedDocumentCanvas {
    /// Resolves the completed canvas into the root principal flow's logical
    /// block-track advance.
    ///
    /// This is intentionally the only writing-mode projection of the canvas
    /// completion. Callers either reserve this much of the current root track
    /// or begin a new fragmentainer before laying out the next inline run.
    /// <https://www.w3.org/TR/css-writing-modes-4/#principal-flow>
    pub(in crate::layout) fn root_inline_block_track_advance(
        self,
        axes: WritingModeAxes,
    ) -> LayoutLength {
        match axes.writing_mode() {
            WritingMode::SidewaysRl | WritingMode::SidewaysLr => {
                self.block_track_occupancy + self.block_end_inset + self.trailing_child_block_margin
            }
            WritingMode::VerticalRl => {
                self.block_track_occupancy + self.trailing_child_block_margin
            }
            WritingMode::VerticalLr => self.block_track_occupancy + self.block_end_inset,
            WritingMode::HorizontalTb => layout_pt(0.0),
        }
    }

    pub(in crate::layout) fn exhausts_root_block_track(
        self,
        axes: WritingModeAxes,
        available_block_track: f32,
    ) -> bool {
        self.root_inline_block_track_advance(axes).points() >= available_block_track - 0.01
    }

    /// Selects the fragmentainer-local placement for the following root
    /// inline sequence without altering paint already committed by the body.
    /// <https://www.w3.org/TR/css-writing-modes-4/#principal-flow>
    pub(in crate::layout) fn resolve_root_inline_placement(
        self,
        axes: WritingModeAxes,
        available_block_track: PageInlineSpan,
    ) -> RootInlineCanvasPlacement {
        if self.exhausts_root_block_track(axes, available_block_track.width()) {
            RootInlineCanvasPlacement::NextPage {
                inline_origin: self.inline_origin,
            }
        } else {
            let advance = self.root_inline_block_track_advance(axes).points();
            let block_track = match axes.physical_side(LogicalSide::BlockStart) {
                PhysicalSide::Left => PageInlineSpan::from_edges(
                    available_block_track.left_x() + advance,
                    available_block_track.right_x(),
                ),
                PhysicalSide::Right => PageInlineSpan::from_edges(
                    available_block_track.left_x(),
                    available_block_track.right_x() - advance,
                ),
                PhysicalSide::Top | PhysicalSide::Bottom => {
                    unreachable!("a vertical principal flow has a horizontal block track")
                }
            };
            RootInlineCanvasPlacement::RemainingTrack {
                block_track,
                inline_origin: self.inline_origin,
            }
        }
    }
}

/// The document-root font-metric state during layout.
///
/// Root-relative font units must use the root element's selected font, even
/// when the root itself has no local metric-relative value. Keeping the
/// bootstrap state distinct prevents an ordinary style's fallback metrics from
/// becoming the document-wide root basis.
/// <https://www.w3.org/TR/css-values-4/#font-relative-lengths>
#[derive(Debug, Clone, Copy)]
pub(in crate::layout) enum RootMetricState {
    Bootstrapping,
    Resolved(ResolvedRootFontMetrics),
}

impl RootMetricState {
    pub(in crate::layout) const fn font_size_basis(self) -> Option<css::RootFontMetricLengthBasis> {
        match self {
            Self::Bootstrapping => None,
            Self::Resolved(metrics) => Some(metrics.basis()),
        }
    }

    pub(in crate::layout) fn resolved(self) -> ResolvedRootFontMetrics {
        match self {
            Self::Bootstrapping => {
                unreachable!("the document root must establish font metrics before descendants")
            }
            Self::Resolved(metrics) => metrics,
        }
    }

    pub(in crate::layout) fn establish(&mut self, metrics: ResolvedRootFontMetrics) {
        debug_assert!(matches!(self, Self::Bootstrapping));
        *self = Self::Resolved(metrics);
    }
}

/// Root-font metrics measured after resolving the document root's used font.
///
/// The traversal APIs accept this wrapper rather than raw metrics, so a
/// descendant cannot accidentally receive a metric basis from an intervening
/// ancestor.
#[derive(Debug, Clone, Copy)]
pub(in crate::layout) struct ResolvedRootFontMetrics(css::RootFontMetricLengthBasis);

impl ResolvedRootFontMetrics {
    /// Records metrics measured from the document root's selected font.
    pub(in crate::layout) const fn measured_for_document_root(
        basis: css::RootFontMetricLengthBasis,
    ) -> Self {
        Self(basis)
    }

    pub(in crate::layout) const fn basis(self) -> css::RootFontMetricLengthBasis {
        self.0
    }
}

/// Selects the semantic purpose of an otherwise ordinary layout traversal.
///
/// An absolutely-positioned box resolves its geometry as though its
/// containing flow were continuous; only the subsequently resolved box may
/// fragment. Keeping that sizing traversal distinct from normal layout keeps
/// it from recursively materializing positioned descendants or pages.
/// <https://drafts.csswg.org/css-position-3/#fragmenting-absolutely-positioned-elements>
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::layout) enum LayoutPassKind {
    Normal,
    PositionedAutoSizeMeasurement,
}

/// Selects whether a traversal is allowed to contribute document output.
///
/// A speculative traversal may execute ordinary layout, including pagination,
/// floats, tables, and positioned descendants, but its pages and paint are
/// owned by [`builder::SpeculativeLayoutTransaction`]. This is independent
/// from [`LayoutPassKind`]: a normal layout algorithm can run in scratch
/// output while measuring intrinsic geometry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::layout) enum LayoutExecutionPurpose {
    Committed,
    Speculative,
}

/// Captures root counter resets that seed page-context counters.
///
/// CSS Paged Media page counters are independent page-associated counters, but
/// document counters can initialize them before page-context rules increment
/// or reset values for each generated page:
/// <https://www.w3.org/TR/css-page-3/#page-based-counters> and
/// <https://www.w3.org/TR/css-lists-3/#auto-numbering>.
pub(in crate::layout) struct LayoutBuilder<'a> {
    pub(in crate::layout) options: &'a RenderOptions,
    pub(in crate::layout) stylesheets: Stylesheets<'a>,
    pub(in crate::layout) base_url: Option<&'a url::Url>,
    pub(in crate::layout) root_url: Option<&'a url::Url>,
    pub(in crate::layout) resource_cache: &'a ResourceCache,
    pub(in crate::layout) iframe_documents: &'a HashMap<crate::dom::ElementId, Document>,
    pub(in crate::layout) iframe_viewport: Option<IframeEmbeddingContext>,
    pub(in crate::layout) pages: Vec<Page>,
    pub(in crate::layout) page_names: Vec<Option<String>>,
    pub(in crate::layout) page_blanks: Vec<bool>,
    pub(in crate::layout) page_name_scope_suppression: usize,
    pub(in crate::layout) page_name_element_scope_suppression: usize,
    /// Lexical used `page` values for active element scopes.
    ///
    /// This is intentionally separate from `current_page_name`: a descendant
    /// can temporarily select a named destination page without changing the
    /// nearest non-`auto` ancestor used to resolve a later sibling's `auto`.
    pub(in crate::layout) page_value_scope_stack: Vec<Option<String>>,
    pub(in crate::layout) page_named_strings: Vec<HashMap<String, Vec<NamedStringAssignment>>>,
    pub(in crate::layout) page_running_elements: Vec<HashMap<String, Vec<NamedStringAssignment>>>,
    /// Non-painting `string-set` sources keyed by the next/previous visible
    /// source boundary that owns their page position.
    pub(in crate::layout) suppressed_named_strings_before:
        HashMap<ElementId, Vec<box_tree::SuppressedNamedStringEvent>>,
    pub(in crate::layout) suppressed_named_strings_after:
        HashMap<ElementId, Vec<box_tree::SuppressedNamedStringEvent>>,
    pub(in crate::layout) page_anchors: HashMap<String, usize>,
    /// Continuous source positions for anchors captured during speculative
    /// replay. The public target map intentionally remains page-only; replay
    /// artifacts use this companion data to select the fragment that owns the
    /// anchor's source start.
    pub(in crate::layout) page_anchor_source_positions: HashMap<String, PaintPoint>,
    pub(in crate::layout) page_anchor_text: HashMap<String, AnchorText>,
    pub(in crate::layout) page_anchor_counters: HashMap<String, HashMap<String, Vec<i32>>>,
    pub(in crate::layout) target_references: TargetReferenceSnapshot,
    pub(in crate::layout) has_normal_flow_target_references: bool,
    pub(in crate::layout) document_canvas_background: Option<DocumentCanvasBackground>,
    /// The resolved static scroll translation for an embedded document root.
    /// A child document's propagated canvas paint is materialized after root
    /// layout, so it must retain the same viewport-relative translation as
    /// the captured root contents.
    /// <https://www.w3.org/TR/css-scroll-snap-1/#scroll-snap-model>
    pub(in crate::layout) document_canvas_scroll_translation: PaintTranslation,
    /// The root box area used to size and position a propagated canvas image.
    ///
    /// This belongs to the document canvas rather than its selected paint
    /// source: an eligible body background is treated as root-specified.
    pub(in crate::layout) document_canvas_root_positioning_area: Option<PaintBackgroundArea>,
    pub(in crate::layout) document_canvas_overflow: DocumentCanvasResolution,
    /// Insets introduced by active document-canvas (`html`/`body`) boxes.
    ///
    /// Their inline components re-enter every page fragment, while their
    /// block-start components belong only to the first fragment. A
    /// destination page recomputes them from its own page area.
    pub(in crate::layout) document_canvas_fragment_insets: Vec<FragmentOffsets>,
    /// Whether the document root generates its principal box.
    ///
    /// A `display: none` root suppresses the document formatting structure,
    /// including page-context painting and generated margin boxes.
    /// <https://drafts.csswg.org/css-display-3/#valdef-display-none>
    pub(in crate::layout) document_root_generates_box: bool,
    pub(in crate::layout) current_page: Page,
    pub(in crate::layout) current_page_has_flow_content: bool,
    /// Whether the current fragment has in-flow content eligible to form a
    /// CSS Fragmentation class-A boundary for named-page selection.
    pub(in crate::layout) current_page_has_named_page_flow_content: bool,
    /// The named page type directly selected by a Class-A source on the
    /// current page.
    ///
    /// This is separate from normal-flow occupancy: a Class-A box can select
    /// a named page whose only document paint is out-of-flow. Empty
    /// continuation pages deliberately clear this marker, even when their
    /// provisional context inherits that page type, so a succeeding Class-A
    /// boundary can replace the provisional context rather than emit an extra
    /// named blank page.
    pub(in crate::layout) current_page_selected_name: Option<String>,
    pub(in crate::layout) last_block_layout_outcome: BlockLayoutOutcome,
    /// Exact used geometry reported by the most recently completed principal
    /// formatting context for its transform effect. This survives paint-tree
    /// capture so transforms never infer their reference box from ink.
    pub(in crate::layout) last_principal_transform_box: Option<assets::TransformReferenceBox>,
    /// Number of active real-element `preserve-3d` rendering contexts during
    /// layout. Ordinary DOM descendants need retained `flat` boundaries even
    /// when they have no independent paint effect.
    pub(in crate::layout) preserve_3d_context_depth: usize,
    pub(in crate::layout) current_page_name: Option<String>,
    pub(in crate::layout) current_page_context: PageContext,
    /// Immutable initial containing block used by document viewport-relative
    /// lengths. Individual page contexts may change later through `@page`.
    pub(in crate::layout) initial_viewport_context: PageContext,
    /// Immutable renderer-provided page box used by viewport-relative page
    /// descriptors. Unlike the document initial containing block, this does
    /// not change when the first actual page is a differently sized named
    /// page.
    /// <https://www.w3.org/TR/css-values-4/#viewport-relative-lengths>
    pub(in crate::layout) page_descriptor_viewport_size: PageSize,
    pub(in crate::layout) fragmentainer_override: Option<FragmentainerOverride>,
    pub(in crate::layout) footnote_bodies: HashMap<ElementId, box_tree::FootnoteBox<'a>>,
    /// Whether each immutable source subtree contains a ruby formatting
    /// context. This structural fact survives fragmentainer retries.
    pub(in crate::layout) ruby_formatting_descendants: HashMap<ElementId, bool>,
    /// Named-page boundary summaries are a pure property of an immutable
    /// source subtree and its inherited page name. Replaying a rejected
    /// fragmentainer must not re-cascade that subtree.
    pub(in crate::layout) dom_page_boundary_summaries:
        HashMap<(ElementId, Option<String>), (ResolvedPageBoundaryValues, PageBoundaryValues)>,
    /// Table wrapper height estimates reused by speculative pagination
    /// probes. The key includes the source table, containing inline span, and
    /// resolved block-size constraints so an intrinsic flex probe cannot
    /// reuse a specified or stretched wrapper measurement.
    pub(in crate::layout) speculative_table_height_estimates:
        HashMap<TableHeightEstimateCacheKey, f32>,
    /// Row-height plans produced while probing an avoid-constrained table.
    /// They are reusable by the accepted table layout only when its source
    /// rows, resolved track width, wrapper block-size constraint, and
    /// intrinsic-or-definite row-distribution target agree. A flex/grid
    /// stretch replay supplies the definite target after an intrinsic probe
    /// has already measured the same rows.
    pub(in crate::layout) speculative_table_height_plans:
        HashMap<TableHeightPlanCacheKey, table::TableHeightPlan>,
    pub(in crate::layout) footnote_measurements: Vec<FootnoteMeasurement>,
    /// Measurements captured from the paint-producing pagination pass. They
    /// validate that the committed call-to-page assignment matches the page
    /// reservation that selected this pass.
    pub(in crate::layout) rendered_footnote_measurements: Vec<FootnoteMeasurement>,
    /// Calls committed during the current measurement pass. This prevents a
    /// replayed selected line from reserving the same detached body twice.
    pub(in crate::layout) measured_footnotes: HashSet<ElementId>,
    /// Source-order inline floats already committed by inline line selection.
    ///
    /// The ordinary block-child traversal later reaches the same DOM element.
    /// It consumes this record instead of laying out and painting the float a
    /// second time.  The record retains the marker, selected source row, and
    /// the durable page-local exclusion that owns the captured paint subtree.
    pub(in crate::layout) committed_inline_floats: HashMap<InlineFloatId, CommittedInlineFloat>,
    pub(in crate::layout) footnote_reservations: HashMap<usize, f32>,
    /// Lower bounds retained by footnote convergence once a reserved source
    /// page has pushed a call into a later fragmentainer. Replaying with only
    /// the destination's reservation must not move that call back earlier and
    /// recreate the same unsatisfied page constraint.
    /// <https://www.w3.org/TR/css-gcpm-3/#footnotes>
    pub(in crate::layout) footnote_call_minimum_page_indices: HashMap<ElementId, usize>,
    pub(in crate::layout) footnote_layout_mode: FootnoteLayoutMode,
    pub(in crate::layout) footnote_measurement_depth: usize,
    pub(in crate::layout) rendered_footnotes: HashSet<ElementId>,
    pub(in crate::layout) pending_page_footnotes: Vec<ElementId>,
    /// Descendants of a monolithic box lay out in an effectively unbounded
    /// fragmentainer so their overflow cannot split the containing box.
    pub(in crate::layout) fragmentation_suppression_depth: usize,
    /// Promoted `column-span:all` boxes fragment in the multicol ancestor's
    /// outer fragmentainer rather than prebreaking as ordinary siblings.
    pub(in crate::layout) multicol_spanner_fragmentation_depth: usize,
    /// Prevent recursive entry while a fixed spanner's overflowing
    /// descendants are laid out speculatively for deferred paint capture.
    pub(in crate::layout) multicol_spanner_speculation_depth: usize,
    /// Nested multicol containers encountered by a balance probe use their
    /// bounded estimate instead of recursively launching another probe tree.
    pub(in crate::layout) multicol_balance_probe_depth: usize,
    /// Pure auto-height float measurements reused by isolated layout replays.
    /// This cache deliberately survives layout snapshots: replays discard
    /// rendering state but not the used size of an unchanged float formatting
    /// context with the same used style and generated-content state.
    pub(in crate::layout) speculative_auto_float_margin_box_heights:
        HashMap<AutoFloatMeasurementKey, MarginBoxLength>,
    /// Floats currently undergoing their isolated auto-height measurement.
    ///
    /// This is deliberately not part of [`LayoutSnapshot`]. A measurement
    /// restores its snapshot before returning, but a nested replay must still
    /// be able to observe the in-progress outer measurement and break the
    /// cycle instead of starting another isolated replay.
    pub(in crate::layout) active_auto_float_measurements: Vec<ElementId>,
    /// Keys for the bounded estimator used when an auto-height float re-enters
    /// its own isolated measurement. This second stack prevents the estimator
    /// from recursively invoking itself through an intervening floated child.
    pub(in crate::layout) active_auto_float_measurement_fallbacks: Vec<ElementId>,
    /// Element-owned adjoining block-start margin scopes handed from a parent
    /// traversal to the child currently being laid out, including the
    /// clear:none parent-start edge needed for CSS2 clearance.
    pub(in crate::layout) inherited_adjoining_start_margins: InheritedAdjoiningStartMarginScopes,
    /// Lexically scoped resolved clearance edges inherited by descendants in
    /// the same float formatting context while adjoining-float placement is
    /// being captured for a possible replay.
    pub(in crate::layout) float_replay_clearance_scopes: Vec<Option<FloatReplayClearanceBoundary>>,
    pub(in crate::layout) cursor_y: f32,
    pub(in crate::layout) content_left: f32,
    pub(in crate::layout) content_right: f32,
    /// Final coordinate contexts of active table-cell content scopes.
    pub(in crate::layout) table_cell_content_coordinate_contexts:
        Vec<table::TableCellContentCoordinateContext>,
    /// Physical inset at the bottom inline-start edge of a `sideways-lr`
    /// principal flow, supplied by the propagated body canvas.
    pub(in crate::layout) principal_inline_end_inset: f32,
    /// Physical block-end canvas inset contributed by the propagated body.
    pub(in crate::layout) principal_body_block_end_inset: LayoutLength,
    /// Scoped root-pseudo placement projection. Its presence never changes
    /// the pseudo-element's computed style.
    pub(in crate::layout) root_pseudo_block_projection: Option<RootPseudoBlockProjection>,
    /// Scoped direct-child geometry supplied by a vertical document-canvas
    /// flow. This is deliberately not a descendant containing block: it is
    /// consumed only by the selected immediate child.
    pub(in crate::layout) direct_block_layout_constraint: Option<DirectBlockLayoutConstraint>,
    /// Root-only logical inline measure for an isolated bottom-origin float
    /// replay. Element dispatch consumes this before descendants recurse.
    pub(in crate::layout) replayed_float_logical_inline_size: Option<LogicalInlineContentSize>,
    /// Translation used only while querying float exclusions for an in-flow
    /// block fragment generated by block-in-inline splitting.
    ///
    /// Relative positioning paints the fragment at this translated position,
    /// but it must not alter its parent block's normal-flow cursor or its
    /// containing block geometry. The float query is the one layout operation
    /// that needs the visual coordinate space.
    pub(in crate::layout) inline_split_float_exclusion_query_offset: RelativeOffset,
    pub(in crate::layout) content_logical_inline_size_stack: Vec<f32>,
    /// Active query containers, scoped to their descendants' used styles.
    pub(in crate::layout) container_unit_contexts: Vec<ContainerUnitContext>,
    /// Definite content-box inline sizes of active anonymous multicolumns.
    ///
    /// This is distinct from an element's own logical inline-size stack: a
    /// nested multicol principal can be measured in a temporary fragmentainer
    /// whose physical span is wider than its containing outer column.
    pub(in crate::layout) multicol_column_containing_blocks: Vec<MulticolColumnContainingBlock>,
    pub(in crate::layout) intrinsic_inline_percentage_basis_stack:
        Vec<IntrinsicInlinePercentageBasis>,
    pub(in crate::layout) inline_static_position: Option<StaticPositionCapture>,
    pub(in crate::layout) text_box_line_trim_stack: Vec<TextBoxLineTrim>,
    /// Per-block capture stack for line slots selected by inline layout.
    ///
    /// A nested block owns a nested capture and exports its result through
    /// `BlockLayoutOutcome`, preventing ancestors from double-counting it.
    pub(in crate::layout) clamp_line_slot_captures: Vec<ClampLineSlotCapture>,
    /// Suppress positioned-layer creation while a formatting context collects
    /// line items before a dedicated positioned-descendant pass.
    pub(in crate::layout) positioned_inline_layout_suppression_depth: usize,
    /// Last prepared in-flow line baseline in the active layout coordinate space.
    ///
    /// CSS 2.2 defines an inline-block baseline from its last in-flow line box,
    /// not from whether that line emitted visible glyph paint. Keeping this as
    /// layout state lets transparent text and other non-painting lines still
    /// export the correct atomic inline baseline:
    /// <https://www.w3.org/TR/CSS22/visudet.html#inlineblock-width>.
    pub(in crate::layout) last_in_flow_line_baseline_y: Option<f32>,
    /// Outside list markers awaiting their first accepted in-flow line.
    pub(in crate::layout) pending_outside_marker_anchors: PendingOutsideMarkerAnchors,
    pub(in crate::layout) block_static_position_y_offset: Option<f32>,
    pub(in crate::layout) absolute_static_position: Option<AbsoluteStaticPosition>,
    /// Final-geometry grid scopes used by positioned descendants that retain
    /// a grid container as their absolute containing block.
    pub(in crate::layout) grid_positioning_scopes: Vec<grid::GridPositioningScope>,
    /// One-shot resolved parent-track contexts for directly replayed subgrids.
    /// A context is consumed by the subgrid's own grid formatting context so
    /// unrelated descendant grids cannot accidentally inherit it.
    pub(in crate::layout) pending_subgrid_contexts: Vec<Option<grid::ResolvedSubgridContext>>,
    pub(in crate::layout) escaped_atom_positioning_depth: usize,
    pub(in crate::layout) active_atomic_inline_coordinate_spaces:
        Vec<AtomicInlineCoordinateSpaceId>,
    /// Monotonic identity source; speculative snapshots do not rewind it.
    pub(in crate::layout) next_atomic_inline_coordinate_space_id: u64,
    /// The outer absolute-position containing block and scratch-local static
    /// rectangle of the active escaped atomic inline, when applicable.
    pub(in crate::layout) escaped_atom_positioning_context: Option<EscapedAtomPositioningContext>,
    pub(in crate::layout) containing_block_direction: Direction,
    pub(in crate::layout) containing_block_writing_mode: WritingMode,
    /// Principal-flow axes of the initial containing block.
    ///
    /// CSS Writing Modes allows an eligible HTML body to supply these axes
    /// without changing the root element's computed style.
    /// <https://www.w3.org/TR/css-writing-modes-4/#principal-flow>
    pub(in crate::layout) initial_containing_block_writing_mode: WritingMode,
    /// Used flow axes of the initial containing block. This is intentionally
    /// separate from the HTML root's cascaded style: an eligible body can
    /// establish the principal flow without changing root computed values.
    /// <https://www.w3.org/TR/css-writing-modes-4/#principal-flow>
    pub(in crate::layout) principal_flow: DocumentPrincipalFlow,
    /// Cursor contribution shared between the propagated body canvas and
    /// anonymous root inline content.
    pub(in crate::layout) root_principal_flow_context: RootPrincipalFlowContext,
    /// Scoped observers of generic fragmentainer transitions. They are used
    /// by table-caption layout to retain actual destination fragmentainers
    /// without teaching ordinary block layout about tables.
    pub(in crate::layout) fragmentainer_transition_recorders: Vec<FragmentainerTransitionRecorder>,
    pub(in crate::layout) fragment_top_offsets: Vec<FragmentTopOffset>,
    pub(in crate::layout) child_available_space_stack: Vec<ChildAvailableSpace>,
    /// Normal-flow containing blocks provided while replaying flex/grid item
    /// contents. This is intentionally distinct from positioned containing
    /// blocks: relative positioning does not itself establish one.
    pub(in crate::layout) normal_flow_relative_containing_blocks:
        Vec<NormalFlowRelativeContainingBlock>,
    /// Static-position containing blocks active while their descendants are
    /// collected and laid out.
    pub(in crate::layout) static_position_containing_blocks: Vec<StaticPositionContainingBlock>,
    /// Canonical block-axis percentage context for nested formatting contexts.
    pub(in crate::layout) block_percentage_context_stack: BlockPercentageContextStack,
    /// One-shot descendant percentage bases for replayed flex items.
    ///
    /// Flex replay materializes a final used height in a temporary style, but
    /// CSS Flexbox does not always make that post-flexing size definite for
    /// descendant percentage resolution. The pending entry is consumed by the
    /// replayed item's root formatting context, before its descendants begin
    /// their ordinary layout:
    /// <https://drafts.csswg.org/css-flexbox/#definite-sizes>.
    pub(in crate::layout) replayed_flex_item_percentage_height_bases:
        Vec<Option<BlockSizePercentageBasis>>,
    /// One-shot wrapper block sizes passed from flex/grid item placement to a
    /// root table formatting context.
    pub(in crate::layout) table_wrapper_block_size_overrides: Vec<Option<BorderBoxLength>>,
    /// One-shot containing-block sizing contracts passed to absolutely
    /// positioned table roots.
    pub(in crate::layout) positioned_table_sizing: Vec<Option<PositionedTableSizing>>,
    pub(in crate::layout) truncate_page_start_margins: bool,
    pub(in crate::layout) avoid_inside_retry_depth: usize,
    pub(in crate::layout) out_of_flow_prebreak_suppression_depth: usize,
    pub(in crate::layout) layout_pass_kind: LayoutPassKind,
    pub(in crate::layout) execution_purpose: LayoutExecutionPurpose,
    pub(in crate::layout) element_side_effect_suppression_depth: usize,
    /// Generated box currently entering generic positioned layout, including
    /// its originating element so descendants cannot inherit its box role.
    pub(in crate::layout) positioned_generated_source: Option<InlineStaticPositionSourceId>,
    pub(in crate::layout) containing_blocks: Vec<PositionedContainingBlockContext>,
    /// Ancestor containing blocks that capture fixed-position descendants.
    /// Relative positioning captures absolute descendants only; transforms
    /// and layout/paint containment also capture fixed descendants.
    pub(in crate::layout) fixed_containing_blocks: Vec<PositionedContainingBlockContext>,
    /// One-shot direct-child endpoints selected by a multicol planner for
    /// per-fragmentainer `text-box-trim: trim-end`.
    pub(in crate::layout) multicol_text_box_trim_end_child_indices: Option<Vec<usize>>,
    pub(in crate::layout) counter_set: CounterSet,
    pub(in crate::layout) counter_plan: CounterPlan,
    pub(in crate::layout) quote_depth: usize,
    pub(in crate::layout) current_page_named_strings: HashMap<String, Vec<NamedStringAssignment>>,
    pub(in crate::layout) current_page_running_elements:
        HashMap<String, Vec<NamedStringAssignment>>,
    pub(in crate::layout) next_assignment_id: usize,
    pub(in crate::layout) assignment_capture_stack: Vec<Vec<AssignmentId>>,
    pub(in crate::layout) ancestors: Vec<ElementSignature>,
    pub(in crate::layout) page_counter_initial_values: HashMap<String, i32>,
    pub(in crate::layout) page_rules: Vec<PageRule>,
    pub(in crate::layout) page_progression_direction: Direction,
    /// Root inherited style used only to initialize generated page-margin boxes.
    ///
    /// Page-margin boxes inherit typography and writing-mode from the page
    /// context, while physical page geometry remains resolved from `@page`
    /// declarations and render options.
    pub(in crate::layout) page_margin_inherited_style: ComputedStyle,
    pub(in crate::layout) page_declarations: Declarations,
    pub(in crate::layout) counter_styles: HashMap<String, CounterStyleRule>,
    pub(in crate::layout) first_page_declarations: Declarations,
    /// The single document-root snapshot used by eager and lazily built boxes.
    pub(in crate::layout) root_metric_state: RootMetricState,
    /// Whether any eagerly built style consumes root-relative selected-font
    /// metrics, which determines whether the root must intern a font.
    pub(in crate::layout) root_metrics_require_selected_font: bool,
    pub(in crate::layout) font_system: Box<FontSystem>,
    /// Reused output storage for CSS Text automatic-spacing preprocessing.
    ///
    /// Each pass swaps this buffer with its input stream after draining that
    /// stream, so repeated inline formatting contexts do not allocate a new
    /// `Vec<InlineItem>` solely to insert autospace edges.
    pub(in crate::layout) autospace_items_scratch: Vec<InlineItem>,
    pub(in crate::layout) bookmarks: Vec<Bookmark>,
    pub(in crate::layout) positioned_layers: Vec<PositionedPaintLayer>,
    /// Logical positioned principals already committed to a final page.
    /// This is a debug-time backstop for speculative retries, which ownership
    /// alone cannot relate across separate executions.
    pub(in crate::layout) committed_positioned_paint_identities:
        HashSet<(DocumentPageIndex, PositionedPaintCommitKey)>,
    /// Nested positioned scratch layout owns its pending page-local layers
    /// until its transaction restores the enclosing page sequence. A page
    /// break reached during that scratch pass must not flush a provisional
    /// layer into the real document.
    pub(in crate::layout) positioned_paint_transaction_depth: usize,
    /// Exclusive page count for the scratch layout of a positioned subtree
    /// whose ancestor `overflow: clip` chain proves later fragments cannot
    /// affect static PDF output. `None` preserves normal, potentially-visible
    /// fragmentation semantics.
    pub(in crate::layout) positioned_scratch_page_limit: Option<usize>,
    /// Document page represented by scratch page zero during positioned
    /// layout, so scratch continuations select their real destination context.
    /// <https://drafts.csswg.org/css-position-3/#fragmenting-abspos>
    pub(in crate::layout) positioned_scratch_page_origin: Option<DocumentPageIndex>,
    pub(in crate::layout) fixed_layers: Vec<FixedPaintLayer>,
    /// Positioned flex descendants captured during temporary multicolumn
    /// layout, awaiting replay against the committed containing block.
    pub(in crate::layout) deferred_multicol_positioned_children:
        Vec<DeferredMulticolPositionedChild>,
    pub(in crate::layout) multicol_positioned_containing_block_spans:
        Vec<MulticolPositionedContainingBlockSpan>,
    pub(in crate::layout) next_multicol_positioned_containing_block_span_id: u64,
    /// Open positioned-containing-block spans while temporary multicolumn
    /// layout is capturing descendants. This mirrors `containing_blocks` so a
    /// deferred descendant can retain the stable span identity of its owner.
    pub(in crate::layout) active_multicol_positioned_containing_block_spans: Vec<u64>,
    /// Nesting depth of temporary multicolumn layout that captures positioned
    /// flex descendants. Only the outermost owner may replay the queue.
    pub(in crate::layout) multicol_positioned_replay_capture_depth: usize,
    /// Furthest page reached by a committed absolutely positioned margin box.
    ///
    /// This is deliberately separate from `pending_positioned_fragmentation`:
    /// transparent abspos geometry normally does not create blank pages, but a
    /// viewport-fixed layer must replay across its complete final page span.
    pub(in crate::layout) absolute_positioned_page_span_target: Option<usize>,
    pub(in crate::layout) pending_positioned_fragmentation: PendingPositionedFragmentation,
    pub(in crate::layout) next_paint_source_order: usize,
    pub(in crate::layout) overflow_clips: Vec<OverflowClip>,
    pub(in crate::layout) active_scroll_snap_scopes: Vec<scroll_snap::ActiveScrollSnapScope>,
    pub(in crate::layout) next_float_id: usize,
    pub(in crate::layout) float_contexts: Vec<FloatContext>,
    /// Parent containing-block spans for floated boxes currently capturing an
    /// isolated paint subtree. A fragmented float must re-enter this parent
    /// span on each destination page rather than carry the source page's
    /// side-by-side float placement.
    ///
    /// <https://www.w3.org/TR/css-break-3/#box-splitting> and
    /// <https://www.w3.org/TR/CSS22/visuren.html#floats>.
    pub(in crate::layout) float_fragment_parent_inline_spans: Vec<PageInlineSpan>,
    pub(in crate::layout) adjoining_float_origin_y: Option<f32>,
    pub(in crate::layout) pending_paint_fragments: Vec<PendingPaintFragment>,
    pub(in crate::layout) pending_page_side_effects: Vec<PendingPageSideEffects>,
    /// Nested floated principal boxes defer replaced-element paint scoping so
    /// the float can capture the complete paint subtree in its Float band.
    pub(in crate::layout) float_paint_capture_depth: usize,
    pub(in crate::layout) preserve_scoped_paint_public_order: bool,
    pub(in crate::layout) defer_next_block_decoration_promotion: bool,
}

pub(in crate::layout) struct LayoutBuilderConfig<'a> {
    pub(in crate::layout) options: &'a RenderOptions,
    pub(in crate::layout) stylesheets: Stylesheets<'a>,
    pub(in crate::layout) base_url: Option<&'a url::Url>,
    pub(in crate::layout) root_url: Option<&'a url::Url>,
    pub(in crate::layout) resource_cache: &'a ResourceCache,
    pub(in crate::layout) iframe_documents: &'a HashMap<crate::dom::ElementId, Document>,
    pub(in crate::layout) iframe_viewport: Option<IframeEmbeddingContext>,
    pub(in crate::layout) page_progression_direction: Direction,
    pub(in crate::layout) page_counter_initial_values: HashMap<String, i32>,
    pub(in crate::layout) target_references: TargetReferenceSnapshot,
    pub(in crate::layout) font_system: FontSystem,
}

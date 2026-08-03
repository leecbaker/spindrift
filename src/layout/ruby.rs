//! Normalization of CSS Ruby's layout-internal box roles.
//!
//! CSS Ruby does not expose `rb`, `rt`, `rbc`, and `rtc` as ordinary nested
//! inline formatting contexts.  This module turns the box-tree roles into a
//! typed base/annotation view before inline collection decides how to
//! materialize a ruby fragment.  Keeping this representation separate from
//! the generic CSS Display box tree prevents annotation text from accidentally
//! becoming parent-line text while preserving the original boxes for painting,
//! links, counters, floats, and positioned descendants.
//!
//! <https://drafts.csswg.org/css-ruby-1/#anon-gen-ruby>
//! <https://drafts.csswg.org/css-ruby-1/#ruby-annotation-pairing>

use super::*;
use std::rc::Rc;

/// Non-negative logical inline span of one ruby level.
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd, Default)]
pub(in crate::layout) struct RubyInlineSpan(f32);

impl RubyInlineSpan {
    pub(in crate::layout) fn new(points: f32) -> Self {
        debug_assert!(points >= 0.0);
        Self(points.max(0.0))
    }

    pub(in crate::layout) fn points(self) -> f32 {
        self.0
    }
}

/// Non-negative logical block extent of a ruby level.
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd, Default)]
pub(in crate::layout) struct RubyBlockExtent(f32);

impl RubyBlockExtent {
    pub(in crate::layout) fn new(points: f32) -> Self {
        debug_assert!(points >= 0.0);
        Self(points.max(0.0))
    }

    pub(in crate::layout) fn points(self) -> f32 {
        self.0
    }
}

/// Baseline offset from a ruby atom's logical block start.
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd, Default)]
pub(in crate::layout) struct RubyBaselineOffset(f32);

impl RubyBaselineOffset {
    pub(in crate::layout) fn new(points: f32) -> Self {
        debug_assert!(points >= 0.0);
        Self(points.max(0.0))
    }

    pub(in crate::layout) fn points(self) -> f32 {
        self.0
    }
}

/// Used metrics of one ruby base or annotation level.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub(in crate::layout) struct RubyLevelMetrics {
    pub(in crate::layout) before_baseline: RubyBlockExtent,
    pub(in crate::layout) after_baseline: RubyBlockExtent,
    pub(in crate::layout) baseline: RubyBaselineOffset,
}

impl RubyLevelMetrics {
    pub(in crate::layout) fn block_extent(self) -> RubyBlockExtent {
        RubyBlockExtent::new(self.before_baseline.points() + self.after_baseline.points())
    }
}

/// Shared metric stack exported by all columns of a normalized ruby group.
#[derive(Debug, Clone, PartialEq, Default)]
pub(in crate::layout) struct RubyColumnGroupMetrics {
    pub(in crate::layout) base: RubyLevelMetrics,
    pub(in crate::layout) annotation_levels: Vec<RubyLevelMetrics>,
    pub(in crate::layout) exported_baseline: RubyBaselineOffset,
}

/// Local paint origin prepared for a ruby atom. It is deliberately distinct
/// from the parent line origin so nested ruby replay cannot reapply float
/// displacement that was already included in the atom's prepared rectangle.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(in crate::layout) struct RubyPaintOrigin {
    inline: f32,
    block: f32,
}

impl RubyPaintOrigin {
    pub(in crate::layout) fn new(inline: f32, block: f32) -> Self {
        Self { inline, block }
    }

    pub(in crate::layout) fn inline(self) -> f32 {
        self.inline
    }

    pub(in crate::layout) fn block_offset(self, offset: RubyBlockExtent) -> f32 {
        self.block + offset.points()
    }
}

/// A normalized ruby container, expressed as paired base and annotation
/// columns.  A column may have an empty base or annotation segment; this is
/// required by CSS Ruby's anonymous counterpart generation.
#[derive(Debug)]
pub(in crate::layout) struct NormalizedRuby<'a> {
    pub(in crate::layout) columns: Vec<RubyColumn<'a>>,
    pub(in crate::layout) annotation_level_count: usize,
}

/// One base segment and its annotation segments, ordered from the first
/// annotation level to the last.
#[derive(Debug)]
pub(in crate::layout) struct RubyColumn<'a> {
    pub(in crate::layout) base: RubySegment<'a>,
    pub(in crate::layout) annotations: Vec<RubyAnnotation<'a>>,
}

/// CSS Ruby role assigned before base/annotation pairing.
///
/// Keeping source whitespace separate from generated anonymous content makes
/// the counterpart-generation rules impossible to apply to generated text.
/// <https://drafts.csswg.org/css-ruby-1/#anon-gen-ruby>
#[derive(Debug, Clone)]
pub(in crate::layout) enum RubyLevelItem<'a> {
    Base(RubySegment<'a>),
    Annotation(RubySegment<'a>),
    IntraLevelWhitespace(RubySegment<'a>),
}

impl<'a> RubyLevelItem<'a> {
    fn into_segment(self) -> RubySegment<'a> {
        match self {
            Self::Base(segment)
            | Self::Annotation(segment)
            | Self::IntraLevelWhitespace(segment) => segment,
        }
    }
}

/// The Ruby whitespace boundary that required an empty counterpart.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(clippy::enum_variant_names)] // CSS Ruby names these three inter-level boundaries.
pub(in crate::layout) enum RubyWhitespaceKind {
    InterBase,
    InterAnnotation,
    InterSegment,
}

/// Provenance of a ruby segment after anonymous-box normalization.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(in crate::layout) enum RubySegmentProvenance {
    /// An authored or generated ruby role. Generated pseudo content remains
    /// here: it is ordinary role content, never source whitespace.
    ExplicitRole,
    /// Content wrapped by CSS Ruby anonymous-box generation.
    #[default]
    AnonymousGenerated,
    /// Collapsible source whitespace between same-level role boxes.
    IntraLevelWhitespace(RubyWhitespaceKind),
}

/// An in-flow sequence owned by one anonymous or explicit ruby role box.
///
/// The segment intentionally keeps box references rather than copying text.
/// Later line collection can therefore preserve generated content, nested
/// inline styles, links, and directional scope.
#[derive(Debug, Clone, Default)]
pub(in crate::layout) struct RubySegment<'a> {
    pub(in crate::layout) boxes: Vec<box_tree::FormattingBox<'a>>,
    /// The originating explicit ruby role, when this segment has one.
    /// Anonymous segments inherit their enclosing ruby-level metrics instead.
    pub(in crate::layout) style: Option<Rc<ComputedStyle>>,
    /// The source/anonymous classification used by pairing. This replaces the
    /// former boolean so a whitespace counterpart cannot be mistaken for a
    /// spanning anonymous annotation.
    pub(in crate::layout) provenance: RubySegmentProvenance,
}

impl RubySegment<'_> {
    pub(in crate::layout) fn is_empty(&self) -> bool {
        self.boxes.is_empty()
    }

    fn is_anonymous(&self) -> bool {
        self.provenance == RubySegmentProvenance::AnonymousGenerated
    }

    fn is_intra_level_whitespace(&self) -> bool {
        matches!(
            self.provenance,
            RubySegmentProvenance::IntraLevelWhitespace(_)
        )
    }

    /// Flatten only the base-side source characters needed to evaluate the
    /// CSS Text boundary between adjacent ruby bases. This does not make the
    /// ruby annotation part of the parent text run.
    pub(in crate::layout) fn boundary_text(&self) -> String {
        let mut text = String::new();
        for box_ in &self.boxes {
            formatting_box_text_content(box_, &mut text);
        }
        text
    }
}

fn formatting_box_text_content(box_: &box_tree::FormattingBox<'_>, output: &mut String) {
    match box_ {
        box_tree::FormattingBox::Text(text) => output.push_str(&text.text),
        box_tree::FormattingBox::Replaced(_) | box_tree::FormattingBox::AtomicInline(_) => {
            output.push(crate::text::OBJECT_REPLACEMENT_CHARACTER)
        }
        box_tree::FormattingBox::Inline(box_) => {
            for child in &box_.core.children {
                formatting_box_text_content(child, output);
            }
        }
        box_tree::FormattingBox::Block(box_) => {
            for child in &box_.core.children {
                formatting_box_text_content(child, output);
            }
        }
        box_tree::FormattingBox::InlineSplitBlockContext(box_) => {
            for child in &box_.core.children {
                formatting_box_text_content(child, output);
            }
        }
        box_tree::FormattingBox::Table(box_) => {
            for child in &box_.core.children {
                formatting_box_text_content(child, output);
            }
        }
        box_tree::FormattingBox::Flex(box_) => {
            for child in &box_.core.children {
                formatting_box_text_content(child, output);
            }
        }
        box_tree::FormattingBox::AnonymousBlock(box_) => {
            for child in &box_.children {
                formatting_box_text_content(child, output);
            }
        }
    }
}

/// One annotation segment paired to a base column.  A positive span is
/// retained explicitly so later column sizing can distribute an annotation
/// across its covered bases instead of pretending it belongs to only one.
#[derive(Debug, Clone)]
pub(in crate::layout) struct RubyAnnotation<'a> {
    pub(in crate::layout) segment: RubySegment<'a>,
    pub(in crate::layout) span: usize,
    pub(in crate::layout) starts_span: bool,
}

impl<'a> NormalizedRuby<'a> {
    /// Normalize the children of a `display: ruby` box.
    ///
    /// Direct base content becomes anonymous base segments. `rbc` provides
    /// explicit base segments, while direct `rt` and `rtc` provide annotation
    /// levels. Out-of-flow boxes and floats are deliberately excluded from
    /// anonymous segment creation; normal positioned/float handling retains
    /// them on their original box-tree path.
    pub(in crate::layout) fn from_children(children: &'a [box_tree::FormattingBox<'a>]) -> Self {
        let mut bases: Vec<RubySegment<'a>> = Vec::new();
        let mut pending_anonymous_base = RubySegment {
            provenance: RubySegmentProvenance::AnonymousGenerated,
            ..RubySegment::default()
        };
        let mut levels: Vec<Vec<RubySegment<'a>>> = Vec::new();
        let mut pending_direct_annotations = Vec::new();

        let in_flow = in_flow_children(children).collect::<Vec<_>>();
        for (index, child) in in_flow.iter().enumerate() {
            let Some((_, _, style, child_children)) = child.element_parts() else {
                if is_intra_ruby_whitespace(child)
                    && pending_direct_annotations
                        .last()
                        .is_some_and(|annotation: &RubySegment<'_>| !annotation.is_anonymous())
                    && in_flow[index + 1..].iter().any(|next| {
                        next.element_parts().is_some_and(|(_, _, style, _)| {
                            style.display.inner == DisplayInner::RubyText
                        })
                    })
                {
                    pending_direct_annotations.push(intra_level_whitespace_segment(
                        child,
                        RubyWhitespaceKind::InterAnnotation,
                        None,
                    ));
                    continue;
                }
                push_base_child(&mut pending_anonymous_base, child);
                continue;
            };
            match style.display.inner {
                DisplayInner::RubyBaseContainer => {
                    flush_segment(&mut bases, &mut pending_anonymous_base);
                    for base in
                        explicit_role_segments(child_children, DisplayInner::RubyBase, Some(style))
                    {
                        bases.push(base.into_segment());
                    }
                }
                DisplayInner::RubyBase => {
                    flush_segment(&mut bases, &mut pending_anonymous_base);
                    bases.push(segment_for_role(child));
                }
                DisplayInner::RubyTextContainer => {
                    flush_annotation_level(&mut levels, &mut pending_direct_annotations);
                    levels.push(
                        explicit_role_segments(child_children, DisplayInner::RubyText, Some(style))
                            .into_iter()
                            .map(RubyLevelItem::into_segment)
                            .collect(),
                    );
                }
                DisplayInner::RubyText => {
                    flush_segment(&mut bases, &mut pending_anonymous_base);
                    pending_direct_annotations.push(segment_for_role(child));
                }
                _ => push_base_child(&mut pending_anonymous_base, child),
            }
        }
        flush_segment(&mut bases, &mut pending_anonymous_base);
        flush_annotation_level(&mut levels, &mut pending_direct_annotations);

        // Empty annotation containers have no annotation box and therefore
        // cannot add an interlinear level or displace a later direct `rt`.
        // Out-of-flow descendants were excluded before this normalization.
        // <https://drafts.csswg.org/css-ruby-1/#anon-gen-ruby>
        levels.retain(|level| level.iter().any(|segment| !segment.is_empty()));

        let annotation_level_count = levels.len();
        let mut columns = bases
            .into_iter()
            .map(|base| RubyColumn {
                base,
                annotations: Vec::with_capacity(annotation_level_count),
            })
            .collect::<Vec<_>>();

        for level in levels {
            pair_annotation_level(&mut columns, level);
        }

        Self {
            columns,
            annotation_level_count,
        }
    }
}

fn in_flow_children<'a>(
    children: &'a [box_tree::FormattingBox<'a>],
) -> impl Iterator<Item = &'a box_tree::FormattingBox<'a>> {
    children.iter().filter(|child| {
        !box_tree::is_out_of_flow_box(child)
            && !child
                .element_parts()
                .is_some_and(|(_, _, style, _)| style.float != Float::None)
    })
}

fn explicit_role_segments<'a>(
    children: &'a [box_tree::FormattingBox<'a>],
    role: DisplayInner,
    anonymous_style: Option<&ComputedStyle>,
) -> Vec<RubyLevelItem<'a>> {
    let mut segments: Vec<RubySegment<'a>> = Vec::new();
    let mut anonymous = RubySegment {
        boxes: Vec::new(),
        style: anonymous_style.map(|style| Rc::new(style.clone())),
        provenance: RubySegmentProvenance::AnonymousGenerated,
    };
    let children = in_flow_children(children).collect::<Vec<_>>();
    let mut previous_role_was_generated = false;
    for (index, child) in children.iter().enumerate() {
        if child
            .element_parts()
            .is_some_and(|(_, _, style, _)| style.display.inner == role)
        {
            flush_segment(&mut segments, &mut anonymous);
            segments.push(segment_for_role(child));
            previous_role_was_generated = ruby_role_is_generated(child);
        } else if is_intra_ruby_whitespace(child)
            && segments
                .last()
                .is_some_and(|segment| !segment.is_anonymous())
            && !previous_role_was_generated
            && children[index + 1..].iter().any(|next| {
                next.element_parts().is_some_and(|(_, _, style, _)| {
                    style.display.inner == role && !ruby_role_is_generated(next)
                })
            })
        {
            flush_segment(&mut segments, &mut anonymous);
            let kind = match role {
                DisplayInner::RubyBase => RubyWhitespaceKind::InterBase,
                DisplayInner::RubyText => RubyWhitespaceKind::InterAnnotation,
                _ => RubyWhitespaceKind::InterSegment,
            };
            let segment = RubySegment {
                boxes: vec![inlinify_ruby_child(child)],
                style: anonymous_style.map(|style| Rc::new(style.clone())),
                provenance: RubySegmentProvenance::IntraLevelWhitespace(kind),
            };
            segments.push(segment);
        } else {
            push_base_child(&mut anonymous, child);
        }
    }
    flush_segment(&mut segments, &mut anonymous);
    segments
        .into_iter()
        .map(|segment| {
            if segment.is_intra_level_whitespace() {
                RubyLevelItem::IntraLevelWhitespace(segment)
            } else {
                role_item(role, segment)
            }
        })
        .collect()
}

fn ruby_role_is_generated(child: &box_tree::FormattingBox<'_>) -> bool {
    child
        .element_core()
        .is_some_and(|core| matches!(core.source, box_tree::BoxSource::GeneratedPseudo(_)))
}

fn role_item<'a>(role: DisplayInner, segment: RubySegment<'a>) -> RubyLevelItem<'a> {
    match role {
        DisplayInner::RubyBase => RubyLevelItem::Base(segment),
        DisplayInner::RubyText => RubyLevelItem::Annotation(segment),
        _ => RubyLevelItem::IntraLevelWhitespace(segment),
    }
}

fn intra_level_whitespace_segment<'a>(
    child: &'a box_tree::FormattingBox<'a>,
    kind: RubyWhitespaceKind,
    style: Option<&ComputedStyle>,
) -> RubySegment<'a> {
    RubySegment {
        boxes: vec![inlinify_ruby_child(child)],
        style: style.map(|style| Rc::new(style.clone())),
        provenance: RubySegmentProvenance::IntraLevelWhitespace(kind),
    }
}

fn segment_for_role<'a>(child: &'a box_tree::FormattingBox<'a>) -> RubySegment<'a> {
    let boxes = child
        .element_parts()
        .map(|(_, _, _, children)| {
            in_flow_children(children)
                .map(inlinify_ruby_child)
                .collect()
        })
        .unwrap_or_else(|| vec![inlinify_ruby_child(child)]);
    RubySegment {
        boxes,
        style: child
            .element_parts()
            .map(|(_, _, style, _)| Rc::new(style.clone())),
        provenance: RubySegmentProvenance::ExplicitRole,
    }
}

fn push_base_child<'a>(segment: &mut RubySegment<'a>, child: &'a box_tree::FormattingBox<'a>) {
    // CSS Ruby discards intra-ruby whitespace at anonymous role boundaries.
    // Keep non-whitespace text intact; CSS Text whitespace processing later
    // owns the detail within a base/annotation segment.
    if !is_intra_ruby_whitespace(child) {
        segment.boxes.push(inlinify_ruby_child(child));
    }
}

fn is_intra_ruby_whitespace(child: &box_tree::FormattingBox<'_>) -> bool {
    matches!(child, box_tree::FormattingBox::Text(text) if text.text.chars().all(is_css_collapsible_whitespace))
}

/// CSS Ruby inlinifies a direct in-flow block child of a ruby formatting
/// context. Preserve its independent block formatting context by converting
/// it to an inline flow-root atom.
/// <https://drafts.csswg.org/css-ruby-1/#anon-gen-inlinize>
fn inlinify_ruby_child<'a>(child: &'a box_tree::FormattingBox<'a>) -> box_tree::FormattingBox<'a> {
    let box_tree::FormattingBox::Block(block) = child else {
        return child.clone();
    };
    let mut core = block.core.clone();
    Rc::make_mut(&mut core.style).display =
        Display::new(DisplayOuter::Inline, DisplayInner::FlowRoot);
    box_tree::FormattingBox::AtomicInline(box_tree::AtomicInlineBoxWith {
        core,
        marker: block.marker.clone(),
        table_fragment: None,
    })
}

/// Clone the direct in-flow children of a ruby role after CSS Ruby's
/// inlinification transformation. Out-of-flow descendants remain untouched.
pub(in crate::layout) fn inlinified_direct_children<'a>(
    children: &'a [box_tree::FormattingBox<'a>],
) -> Vec<box_tree::FormattingBox<'a>> {
    children
        .iter()
        .map(|child| {
            if box_tree::is_out_of_flow_box(child)
                || child
                    .element_parts()
                    .is_some_and(|(_, _, style, _)| style.float != Float::None)
            {
                child.clone()
            } else {
                inlinify_ruby_child(child)
            }
        })
        .collect()
}

fn flush_segment<'a>(segments: &mut Vec<RubySegment<'a>>, pending: &mut RubySegment<'a>) {
    if !pending.is_empty() {
        let provenance = pending.provenance;
        segments.push(std::mem::replace(
            pending,
            RubySegment {
                provenance,
                ..RubySegment::default()
            },
        ));
    }
}

fn flush_annotation_level<'a>(
    levels: &mut Vec<Vec<RubySegment<'a>>>,
    pending: &mut Vec<RubySegment<'a>>,
) {
    if !pending.is_empty() {
        levels.push(std::mem::take(pending));
    }
}

fn pair_annotation_level<'a>(columns: &mut Vec<RubyColumn<'a>>, annotations: Vec<RubySegment<'a>>) {
    if annotations.len() == 1 && annotations[0].is_anonymous() && columns.len() > 1 {
        let span = columns.len();
        for (index, column) in columns.iter_mut().enumerate() {
            column.annotations.push(RubyAnnotation {
                segment: if index == 0 {
                    annotations[0].clone()
                } else {
                    RubySegment::default()
                },
                span,
                starts_span: index == 0,
            });
        }
        return;
    }

    // Align source whitespace before ordinal pairing. Every retained
    // intra-level whitespace item creates a real ruby column and gets an
    // explicit empty counterpart on the opposite level. This is the part of
    // CSS Ruby anonymous generation that cannot be represented by simply
    // padding the shorter vector at its end.
    // <https://drafts.csswg.org/css-ruby-1/#anon-gen-ruby>
    let completed_levels = columns
        .first()
        .map(|column| column.annotations.len())
        .unwrap_or_default();
    let mut column_index = 0;
    let mut annotation_index = 0;
    while column_index < columns.len() || annotation_index < annotations.len() {
        let annotation_is_whitespace = annotations
            .get(annotation_index)
            .is_some_and(RubySegment::is_intra_level_whitespace);
        if column_index == columns.len() || annotation_is_whitespace {
            let mut column = RubyColumn {
                base: RubySegment::default(),
                annotations: (0..completed_levels)
                    .map(|_| empty_ruby_annotation())
                    .collect(),
            };
            column.annotations.push(RubyAnnotation {
                segment: annotations[annotation_index].clone(),
                span: 1,
                starts_span: true,
            });
            columns.insert(column_index, column);
            column_index += 1;
            annotation_index += 1;
            continue;
        }

        let base_is_whitespace = columns[column_index].base.is_intra_level_whitespace();
        let segment = if base_is_whitespace && !annotation_is_whitespace {
            RubySegment::default()
        } else {
            let segment = annotations
                .get(annotation_index)
                .cloned()
                .unwrap_or_default();
            annotation_index += usize::from(annotation_index < annotations.len());
            segment
        };
        columns[column_index].annotations.push(RubyAnnotation {
            segment,
            span: 1,
            starts_span: true,
        });
        column_index += 1;
    }
}

fn empty_ruby_annotation<'a>() -> RubyAnnotation<'a> {
    RubyAnnotation {
        segment: RubySegment::default(),
        span: 1,
        starts_span: true,
    }
}

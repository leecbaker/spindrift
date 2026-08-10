use super::*;
use crate::layout::inline_collect::{InlineBoxEdge, inline_box_edge_has_nonzero_component};

/// Return whether source provenance may keep adjacent text in one layout
/// shaping group.
///
/// Marker provenance identifies generated list-label semantics at paint and
/// extraction time, but an inside marker is an inline child. It must not by
/// itself introduce a CSS Text shaping boundary with following inline text.
/// Footnote calls, run-ins, and UAX #9 control runs retain their dedicated
/// layout boundaries.
/// <https://drafts.csswg.org/css-lists-3/#marker-content> and
/// <https://www.w3.org/TR/css-text-3/#boundary-shaping>
pub(in crate::layout) fn inline_text_sources_are_layout_compatible(
    left: InlineTextSource,
    right: InlineTextSource,
) -> bool {
    match (left, right) {
        (InlineTextSource::FootnoteCall(left), InlineTextSource::FootnoteCall(right)) => {
            left == right
        }
        (InlineTextSource::FootnoteCall(_), _) | (_, InlineTextSource::FootnoteCall(_)) => false,
        (InlineTextSource::BidiControl, InlineTextSource::BidiControl) => true,
        (InlineTextSource::BidiControl, _) | (_, InlineTextSource::BidiControl) => false,
        (InlineTextSource::BlockEllipsis, InlineTextSource::BlockEllipsis) => true,
        (InlineTextSource::BlockEllipsis, _) | (_, InlineTextSource::BlockEllipsis) => false,
        (InlineTextSource::WordSpaceTransform(_), InlineTextSource::WordSpaceTransform(_)) => true,
        (InlineTextSource::WordSpaceTransform(_), _)
        | (_, InlineTextSource::WordSpaceTransform(_)) => false,
        (InlineTextSource::RunIn, InlineTextSource::RunIn) => true,
        (InlineTextSource::RunIn, _) | (_, InlineTextSource::RunIn) => false,
        _ => true,
    }
}

/// Return whether fragments can stay in one pending paint-time shaping group.
///
/// Visible text with different paint state must still flush into separate PDF
/// text runs, but transparent inline boundaries do not interrupt cursive
/// shaping. Keeping a joining fragment next to its visible neighbors preserves
/// Text boundary shaping across styled inline boxes without merging visible
/// paint state:
/// <https://www.w3.org/TR/css-text-3/#boundary-shaping> and
/// <https://www.w3.org/TR/alreq/#h_disjoining_enforcement>.
pub(in crate::layout) fn can_queue_inline_fragments_for_shaping(
    left: &(impl InlineFragmentAccess + ?Sized),
    right: &(impl InlineFragmentAccess + ?Sized),
) -> bool {
    if !inline_text_sources_are_layout_compatible(left.source(), right.source()) {
        return false;
    }
    // A prepared text group owns exactly one PDF link annotation. Keep a
    // hyperlink boundary as a group boundary even when its inline box is
    // otherwise transparent to CSS Text shaping; otherwise visual bidi
    // reordering can place an unlinked fragment first and discard the link
    // target from the group.
    // <https://www.w3.org/TR/css-text-3/#boundary-shaping> and
    // <https://www.w3.org/TR/css-ui-4/#links>
    if left.link_target() != right.link_target() {
        return false;
    }
    can_paint_inline_fragments_together(left, right)
        || ((inline_fragment_is_join_control_only(left)
            || inline_fragment_is_join_control_only(right))
            && can_shape_inline_fragments_together(left, right))
        || ((inline_fragment_is_arabic_tatweel_only(left)
            || inline_fragment_is_arabic_tatweel_only(right))
            && can_shape_inline_fragments_together(left, right))
        || ((inline_fragment_contains_joining_context(left)
            || inline_fragment_contains_joining_context(right))
            && can_shape_inline_fragments_together(left, right))
}

/// Return whether adjacent inline fragments can be shaped as one text run.
///
/// CSS Text shaping operates over typographic runs after inline box tree
/// construction. Font/style changes can split the resulting font runs, but
/// they must not by themselves remove adjacent context for cursive-script
/// shaping; CSS Text only requires an inline-boundary break for separating
/// margin/border/padding, non-baseline alignment, or bidi isolation:
/// <https://www.w3.org/TR/css-text-3/#boundary-shaping>.
pub(in crate::layout) fn can_shape_inline_fragments_together(
    left: &(impl InlineFragmentAccess + ?Sized),
    right: &(impl InlineFragmentAccess + ?Sized),
) -> bool {
    if inline_fragment_is_join_control_only(left) {
        return !inline_box_edge_breaks_shaping(right.style())
            && !inline_bidi_isolation_boundary_breaks_shaping(left, right);
    }
    if inline_fragment_is_join_control_only(right) {
        return !inline_box_edge_breaks_shaping(left.style())
            && !inline_bidi_isolation_boundary_breaks_shaping(left, right);
    }
    if left.visual_offset() != right.visual_offset() {
        return false;
    }
    left.style().vertical_align == right.style().vertical_align
        && left.style().writing_mode == right.style().writing_mode
        && left.style().language == right.style().language
        // Tracking is positioned after bidi reordering. A different used
        // value at this boundary therefore needs its own shaped/positioned
        // run rather than borrowing cursive context from the neighbor.
        // <https://www.w3.org/TR/css-text-3/#letter-spacing-property>
        && left.style().letter_spacing == right.style().letter_spacing
        && !inline_box_edge_breaks_shaping(left.style())
        && !inline_box_edge_breaks_shaping(right.style())
        && !inline_bidi_isolation_boundary_breaks_shaping(left, right)
}

/// Return whether an actual `unicode-bidi` isolate boundary separates two
/// fragments for shaping.
///
/// A UAX #9 visual split can produce several fragments from text inside one
/// isolate. That split remains within the isolate and must retain contextual
/// shaping; only fragments with different lexical inline scopes cross the
/// CSS isolation boundary:
/// <https://www.w3.org/TR/css-text-3/#boundary-shaping> and
/// <https://www.w3.org/TR/css-writing-modes-4/#unicode-bidi>.
pub(in crate::layout) fn inline_bidi_isolation_boundary_breaks_shaping(
    left: &(impl InlineFragmentAccess + ?Sized),
    right: &(impl InlineFragmentAccess + ?Sized),
) -> bool {
    (inline_box_bidi_isolation_breaks_shaping(left.style())
        || inline_box_bidi_isolation_breaks_shaping(right.style()))
        && !matches!(
            (left.tracking_scope(), right.tracking_scope()),
            (Some(left), Some(right)) if Rc::ptr_eq(left, right)
        )
}

/// Return whether two computed styles have the same text-shaping inputs after
/// CSS Text's pre-shaping transformations have been applied.
///
/// `display` establishes the otherwise transparent inline-box boundary, while
/// `text-transform` has already produced the source scalars passed to the
/// shaper. Neither value changes the OpenType shaping result for those
/// scalars, so a single styled shaping span may preserve cursive joining over
/// such a boundary:
/// <https://www.w3.org/TR/css-text-3/#boundary-shaping> and
/// <https://www.w3.org/TR/css-text-3/#order-operations>.
pub(in crate::layout) fn styles_have_equivalent_text_shaping_inputs(
    left: &ComputedStyle,
    right: &ComputedStyle,
) -> bool {
    left.font_family == right.font_family
        && left.font_size == right.font_size
        && left.font_size_adjust == right.font_size_adjust
        && left.line_height == right.line_height
        && left.font_weight == right.font_weight
        && left.font_style == right.font_style
        && left.font_width == right.font_width
        && left.font_synthesis == right.font_synthesis
        && left.font_feature_settings == right.font_feature_settings
        && left.text_spacing_trim == right.text_spacing_trim
        && left.font_variation_settings == right.font_variation_settings
        && left.font_kerning == right.font_kerning
        && left.font_variant_ligatures == right.font_variant_ligatures
        && left.font_variant_position == right.font_variant_position
        && left.font_variant_caps == right.font_variant_caps
        && left.font_variant_numeric == right.font_variant_numeric
        && left.font_variant_alternates == right.font_variant_alternates
        && left.font_variant_east_asian == right.font_variant_east_asian
        && left.font_variant_emoji == right.font_variant_emoji
        // This predicate also authorizes reusing one source-shaped paint
        // artifact, so a paint-only palette boundary must remain explicit.
        // <https://drafts.csswg.org/css-fonts-4/#font-palette-prop>
        && left.font_palette == right.font_palette
        && left.overflow_wrap == right.overflow_wrap
        && left.text_wrap_mode == right.text_wrap_mode
        && left.word_spacing == right.word_spacing
        && left.letter_spacing == right.letter_spacing
        && left.language == right.language
        && left.writing_mode == right.writing_mode
        && left.text_orientation == right.text_orientation
}

pub(in crate::layout) fn inline_fragment_is_join_control_only(
    fragment: &(impl InlineFragmentAccess + ?Sized),
) -> bool {
    !fragment.text().is_empty() && fragment.text().chars().all(character_is_join_control)
}

pub(in crate::layout) fn inline_fragment_is_arabic_tatweel_only(
    fragment: &(impl InlineFragmentAccess + ?Sized),
) -> bool {
    !fragment.text().is_empty() && fragment.text().chars().all(character_is_arabic_tatweel)
}

pub(in crate::layout) fn inline_fragment_contains_joining_context(
    fragment: &(impl InlineFragmentAccess + ?Sized),
) -> bool {
    fragment.text().chars().any(|character| {
        character_is_join_control(character)
            || character_is_arabic_tatweel(character)
            || character_has_joining_behavior(character)
    })
}

/// Return whether a style's bidi scope should affect inline line ordering.
///
/// HTML's UA stylesheet sets `unicode-bidi: isolate` on many block containers,
/// but a block formatting context already separates the block from surrounding
/// inline content. Inline-level scopes, block overrides, and plaintext still
/// need UAX #9 controls during line ordering:
/// <https://html.spec.whatwg.org/multipage/rendering.html#bidi-rendering> and
/// <https://www.w3.org/TR/css-writing-modes-4/#unicode-bidi>.
pub(in crate::layout) fn inline_bidi_scope_affects_line_ordering(style: &ComputedStyle) -> bool {
    bidi_control_scope_for_style(style).is_some()
        && !(style.display.is_block_level() && style.unicode_bidi == UnicodeBidi::Isolate)
}

/// Return whether an inline box boundary must interrupt text shaping.
///
/// CSS Text boundary shaping allows shaping across inline boundaries unless
/// the boundary has nonzero margin, border, or padding, which creates a real
/// visual separation:
/// <https://www.w3.org/TR/css-text-3/#boundary-shaping>.
pub(in crate::layout) fn inline_box_edge_breaks_shaping(style: &ComputedStyle) -> bool {
    style.display.is_inline_level()
        && (inline_box_edge_has_nonzero_component(style, InlineBoxEdge::Start)
            || inline_box_edge_has_nonzero_component(style, InlineBoxEdge::End))
}

/// Return whether an inline bidi-isolation boundary interrupts shaping.
///
/// CSS Text boundary shaping treats bidi isolation boundaries as shaping
/// boundaries because isolated text is reordered as an independent bidi scope:
/// <https://www.w3.org/TR/css-text-3/#boundary-shaping> and
/// <https://www.w3.org/TR/css-writing-modes-4/#unicode-bidi>.
pub(in crate::layout) fn inline_box_bidi_isolation_breaks_shaping(style: &ComputedStyle) -> bool {
    style.display.is_inline_level()
        && matches!(
            style.unicode_bidi,
            UnicodeBidi::Isolate | UnicodeBidi::IsolateOverride | UnicodeBidi::Plaintext
        )
}

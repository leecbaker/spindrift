use crate::css::{ComputedStyle, LineBreak, OverflowWrap, WordBreak};
use crate::text::{FontSystem, is_css_collapsible_whitespace, measured_break_opportunities};
use icu_segmenter::GraphemeClusterSegmenter;

pub(super) fn guarded_max_content_width(width: f32, style: &ComputedStyle) -> f32 {
    width + intrinsic_text_width_epsilon(style)
}

/// Rounds intrinsic text widths up enough to survive line-breaker metric drift.
///
/// CSS Sizing defines max-content as the inline size with no soft wraps:
/// <https://www.w3.org/TR/css-sizing-3/#max-content>. The PDF text emitter and
/// line breaker use shaped font metrics through different APIs, so intrinsic
/// widths keep a sub-glyph guard band to avoid wrapping at their own measured
/// max-content boundary.
fn intrinsic_text_width_epsilon(style: &ComputedStyle) -> f32 {
    (style.font_size * 0.05).clamp(0.25, 1.5)
}

pub(super) fn transformed_min_content_segment_widths(
    font_system: &mut FontSystem,
    text: &str,
    style: &ComputedStyle,
) -> Vec<f32> {
    transformed_min_content_segments(text, style)
        .into_iter()
        .map(|segment| font_system.measure_line_text(segment, style))
        .collect()
}

/// Return the advance of one punctuation glyph that CSS Text has made hangable.
///
/// The `hanging-punctuation` policy decides which glyph may hang before this
/// helper is called. Measuring the selected glyph separately keeps `last`,
/// `force-end`, and `allow-end` from re-checking each other's keywords:
/// <https://www.w3.org/TR/css-text-3/#hanging-punctuation-property>.
pub(super) fn hanging_punctuation_character_width(
    font_system: &mut FontSystem,
    character: char,
    style: &ComputedStyle,
) -> f32 {
    font_system.measure_text(&character.to_string(), style)
}

fn transformed_min_content_segments<'a>(
    transformed: &'a str,
    style: &ComputedStyle,
) -> Vec<&'a str> {
    if transformed.is_empty() {
        return Vec::new();
    }
    if matches!(style.overflow_wrap, OverflowWrap::Anywhere)
        || matches!(style.word_break, WordBreak::BreakAll)
        || matches!(style.line_break, LineBreak::Anywhere)
    {
        let boundaries = GraphemeClusterSegmenter::new()
            .segment_str(transformed)
            .collect::<Vec<_>>();
        return boundaries
            .windows(2)
            .filter_map(|window| {
                let segment = &transformed[window[0]..window[1]];
                (!segment.is_empty()).then_some(segment)
            })
            .collect();
    }
    let breaks = measured_break_opportunities(transformed, style);
    let mut start = 0usize;
    let mut segments = Vec::new();
    for end in breaks {
        if end <= start || end > transformed.len() {
            continue;
        }
        let segment = trim_css_intrinsic_segment(&transformed[start..end], style);
        if !segment.is_empty() {
            segments.push(segment);
        }
        start = end;
    }
    if start < transformed.len() {
        let segment = trim_css_intrinsic_segment(&transformed[start..], style);
        if !segment.is_empty() {
            segments.push(segment);
        }
    }
    segments
}

/// Trim segment-edge document whitespace for intrinsic text measurement.
///
/// CSS Text excludes collapsible document white space at soft wrap boundaries
/// from the measured line box, while preserved white-space modes keep their
/// authored spacing:
/// <https://www.w3.org/TR/css-text-3/#white-space-processing> and
/// <https://www.w3.org/TR/css-sizing-3/#min-content>.
fn trim_css_intrinsic_segment<'a>(segment: &'a str, style: &ComputedStyle) -> &'a str {
    if style.white_space.preserves_space_edges() {
        segment
    } else {
        segment.trim_matches(is_css_collapsible_whitespace)
    }
}

// CSS 2.2 shrink-to-fit width: min(max(preferred-min, available), preferred).
pub(super) fn shrink_to_fit_width(preferred_min: f32, preferred: f32, available: f32) -> f32 {
    preferred_min.max(available).min(preferred)
}

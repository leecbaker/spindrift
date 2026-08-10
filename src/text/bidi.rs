use super::*;
use icu_segmenter::GraphemeClusterSegmenter;
use std::ops::Range;
use unicode_bidi::{BidiInfo, Level};

/// Return the Unicode controls that model an inline CSS `unicode-bidi` scope.
///
/// CSS Writing Modes defines `unicode-bidi` in terms of extra Unicode
/// Bidirectional Algorithm embeddings, overrides, isolates, and plaintext
/// paragraph direction. Inline scopes map directly to UAX #9 formatting
/// controls for text shaping and visual ordering; block-level `isolate` is a
/// formatting-context boundary in this layout engine and does not inject text
/// controls into the block's own anonymous inline content:
/// <https://www.w3.org/TR/css-writing-modes-4/#unicode-bidi>,
/// <https://www.w3.org/TR/css-display-3/#formatting-context> and
/// <https://www.unicode.org/reports/tr9/#Directional_Formatting_Characters>.
pub(crate) fn bidi_control_scope_for_style(
    style: &ComputedStyle,
) -> Option<(&'static str, &'static str)> {
    match (style.unicode_bidi, style.used_direction()) {
        // In a vertical typographic mode, `text-orientation: upright` makes
        // every typographic character unit a strong LTR unit for bidi. Model
        // that inline boundary with an LRO/PDF pair: an LTR embedding would
        // still retain Hebrew and Arabic characters' intrinsic RTL classes,
        // whereas the specified strong-LTR treatment overrides each unit.
        // This is independent of the computed `direction`, which remains
        // available for inheritance into descendants outside this override.
        // <https://drafts.csswg.org/css-writing-modes-4/#text-orientation>
        (UnicodeBidi::Normal, Direction::Ltr) if style.direction != style.used_direction() => {
            Some(("\u{202d}", "\u{202c}"))
        }
        (UnicodeBidi::Normal, _) => None,
        (UnicodeBidi::Isolate, _) if style.display.is_block_level() => None,
        (UnicodeBidi::Embed, Direction::Ltr) => Some(("\u{202a}", "\u{202c}")),
        (UnicodeBidi::Embed, Direction::Rtl) => Some(("\u{202b}", "\u{202c}")),
        (UnicodeBidi::Isolate, Direction::Ltr) => Some(("\u{2066}", "\u{2069}")),
        (UnicodeBidi::Isolate, Direction::Rtl) => Some(("\u{2067}", "\u{2069}")),
        (UnicodeBidi::BidiOverride, Direction::Ltr) => Some(("\u{202d}", "\u{202c}")),
        (UnicodeBidi::BidiOverride, Direction::Rtl) => Some(("\u{202e}", "\u{202c}")),
        (UnicodeBidi::IsolateOverride, Direction::Ltr) => {
            Some(("\u{2068}\u{202d}", "\u{202c}\u{2069}"))
        }
        (UnicodeBidi::IsolateOverride, Direction::Rtl) => {
            Some(("\u{2068}\u{202e}", "\u{202c}\u{2069}"))
        }
        (UnicodeBidi::Plaintext, _) => Some(("\u{2068}", "\u{2069}")),
    }
}

/// Remove Unicode bidi formatting controls from text intended for painting.
///
/// CSS `unicode-bidi` controls influence ordering during the Unicode
/// Bidirectional Algorithm but do not create visible glyphs:
/// <https://www.w3.org/TR/css-writing-modes-4/#unicode-bidi>.
pub(crate) fn text_without_bidi_format_controls(text: &str) -> Cow<'_, str> {
    if !text.chars().any(character_is_bidi_format_control) {
        return Cow::Borrowed(text);
    }
    Cow::Owned(
        text.chars()
            .filter(|character| !character_is_bidi_format_control(*character))
            .collect(),
    )
}

/// Resolve a selected logical line into grapheme-cluster ranges in UAX #9
/// visual order using CSS's already-resolved paragraph direction.
///
/// CSS `direction` supplies the paragraph embedding level unless
/// `unicode-bidi: plaintext` selected a first-strong direction for this line.
/// CSS-generated embeddings, overrides, and isolates have already been
/// inserted into `text` by inline collection, so they remain direct UAX #9
/// input instead of being approximated with an LRM/RLM prefix.
/// <https://www.w3.org/TR/css-writing-modes-4/#bidi-algo>
/// <https://www.unicode.org/reports/tr9/#P2>
/// <https://www.unicode.org/reports/tr9/#L2>
pub(crate) fn resolve_bidi_visual_ranges(
    text: &str,
    base_direction: Direction,
) -> Vec<BidiVisualRange> {
    if text.is_empty() {
        return Vec::new();
    }

    let base_level = match base_direction {
        Direction::Ltr => Level::ltr(),
        Direction::Rtl => Level::rtl(),
    };
    let bidi_info = BidiInfo::new(text, Some(base_level));
    let grapheme_boundaries = GraphemeClusterSegmenter::new()
        .segment_str(text)
        .collect::<Vec<_>>();
    let mut ranges = Vec::new();

    for paragraph in &bidi_info.paragraphs {
        let (levels, visual_runs) = bidi_info.visual_runs(paragraph, paragraph.range.clone());
        for run in visual_runs {
            let level = levels[run.start];
            let direction = if level.is_rtl() {
                ResolvedBidiDirection::Rtl
            } else {
                ResolvedBidiDirection::Ltr
            };
            let mut clusters = grapheme_boundaries
                .windows(2)
                .map(|boundaries| boundaries[0]..boundaries[1])
                .filter(|cluster| cluster.start >= run.start && cluster.end <= run.end)
                .filter(|cluster| {
                    text[cluster.clone()]
                        .chars()
                        .any(|character| !character_is_bidi_format_control(character))
                })
                .collect::<Vec<_>>();
            if direction == ResolvedBidiDirection::Rtl {
                clusters.reverse();
            }
            for range in clusters {
                push_visual_range(&mut ranges, range, direction);
            }
        }
    }

    if ranges.is_empty() {
        ranges.push(BidiVisualRange {
            range: 0..text.len(),
            direction: match base_direction {
                Direction::Ltr => ResolvedBidiDirection::Ltr,
                Direction::Rtl => ResolvedBidiDirection::Rtl,
            },
        });
    }
    ranges
}

/// Append one visual range, preserving the complete logical slice for
/// adjacent LTR grapheme clusters in the same resolved level.
///
/// A UAX #9 run is not inherently a paint-fragment boundary. Coalescing its
/// source-contiguous clusters retains one inline box fragment for an ordinary
/// LTR span embedded in RTL text, so its background and link annotation own
/// the span rather than each individual grapheme.
/// <https://www.unicode.org/reports/tr9/#L2> and
/// <https://www.w3.org/TR/css-writing-modes-4/#bidi-linebox>
fn push_visual_range(
    ranges: &mut Vec<BidiVisualRange>,
    range: Range<usize>,
    direction: ResolvedBidiDirection,
) {
    if range.is_empty() {
        return;
    }
    if let Some(previous) = ranges.last_mut()
        && previous.direction == direction
        && previous.range.end == range.start
    {
        previous.range.end = range.end;
        return;
    }
    ranges.push(BidiVisualRange { range, direction });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn visual_text(text: &str, direction: Direction) -> String {
        resolve_bidi_visual_ranges(text, direction)
            .into_iter()
            .map(|range| text_without_bidi_format_controls(&text[range.range]).into_owned())
            .collect()
    }

    #[test]
    fn css_paragraph_base_level_changes_neutral_and_rtl_visual_order() {
        let text = "> a > ב > c >";
        let ltr = visual_text(text, Direction::Ltr);
        let rtl = visual_text(text, Direction::Rtl);

        assert_ne!(ltr, rtl);
        assert_eq!(ltr.chars().filter(|character| *character == 'ב').count(), 1);
        assert_eq!(rtl.chars().filter(|character| *character == 'ב').count(), 1);
    }

    #[test]
    fn ltr_paragraph_keeps_neutral_punctuation_outside_rtl_text_at_ltr_level() {
        let text = "> \u{5d0} > a >";
        let ranges = resolve_bidi_visual_ranges(text, Direction::Ltr);

        assert!(
            ranges
                .iter()
                .filter(|range| &text[range.range.clone()] == ">")
                .all(|range| range.direction == ResolvedBidiDirection::Ltr)
        );
        assert!(ranges.iter().any(|range| {
            &text[range.range.clone()] == "\u{5d0}" && range.direction == ResolvedBidiDirection::Rtl
        }));
    }

    #[test]
    fn explicit_override_controls_remain_uba_input() {
        let text = "a \u{202e}bc\u{202c} d";

        assert_eq!(visual_text(text, Direction::Ltr), "a cb d");
    }

    #[test]
    fn resolved_plaintext_fallback_direction_controls_neutral_line_level() {
        let text = "--";
        let ltr = resolve_bidi_visual_ranges(text, Direction::Ltr);
        let rtl = resolve_bidi_visual_ranges(text, Direction::Rtl);

        assert!(
            ltr.iter()
                .all(|range| range.direction == ResolvedBidiDirection::Ltr)
        );
        assert!(
            rtl.iter()
                .all(|range| range.direction == ResolvedBidiDirection::Rtl)
        );
    }

    #[test]
    fn astral_bidi_ranges_remain_utf8_character_boundaries() {
        let text = "a \u{1e900} b";
        let ranges = resolve_bidi_visual_ranges(text, Direction::Ltr);

        assert!(ranges.iter().all(|range| {
            text.is_char_boundary(range.range.start) && text.is_char_boundary(range.range.end)
        }));
        assert_eq!(
            ranges
                .iter()
                .map(|range| &text[range.range.clone()])
                .collect::<String>()
                .chars()
                .filter(|character| *character == '\u{1e900}')
                .count(),
            1
        );
    }

    #[test]
    fn strong_astral_adlam_run_is_visual_rtl_in_an_ltr_paragraph() {
        let text = "\u{1e900}\u{1e901}\u{1e902}\u{1e901}\u{1e904}";
        let ranges = resolve_bidi_visual_ranges(text, Direction::Ltr);

        assert!(ranges.iter().all(|range| {
            range.direction == ResolvedBidiDirection::Rtl
                && text.is_char_boundary(range.range.start)
                && text.is_char_boundary(range.range.end)
        }));
        assert_eq!(
            ranges
                .iter()
                .map(|range| &text[range.range.clone()])
                .collect::<String>(),
            "\u{1e904}\u{1e901}\u{1e902}\u{1e901}\u{1e900}"
        );
    }
}

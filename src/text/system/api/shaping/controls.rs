use super::super::*;
use std::borrow::Cow;

pub(crate) fn span_boundary_needs_join_control(left: &str, right: &str) -> bool {
    if left
        .chars()
        .next_back()
        .is_some_and(character_is_join_control)
        || right.chars().next().is_some_and(character_is_join_control)
    {
        return false;
    }
    let Some(left) = left
        .chars()
        .rev()
        .find(|character| !character_is_join_control(*character))
    else {
        return false;
    };
    let Some(right) = right
        .chars()
        .find(|character| !character_is_join_control(*character))
    else {
        return false;
    };
    character_can_join_following(left) && character_can_join_preceding(right)
}

/// Append a shaping-only ZWJ and remember its byte range for output cleanup.
///
/// CSS Text shaping may need join controls that are not present in the DOM
/// text. Tracking synthetic controls separately preserves the original text
/// for PDF extraction while still giving OpenType shaping the required
/// joining context:
/// <https://www.w3.org/TR/css-text-3/#boundary-shaping>.
pub(in crate::text) fn push_synthetic_join_control(
    text: &mut String,
    synthetic_ranges: &mut Vec<Range<usize>>,
) {
    let start = text.len();
    text.push('\u{200d}');
    synthetic_ranges.push(start..text.len());
}

pub(in crate::text) fn text_needs_edge_join_context(text: &str) -> bool {
    text_needs_leading_join_context(text) || text_needs_trailing_join_context(text)
}

pub(in crate::text) fn text_needs_leading_join_context(text: &str) -> bool {
    let mut characters = text.chars();
    if characters.next() != Some('\u{200d}') {
        return false;
    }
    characters
        .find(|character| !character_is_join_control(*character))
        .is_some_and(character_can_join_preceding)
}

pub(in crate::text) fn text_needs_trailing_join_context(text: &str) -> bool {
    let mut characters = text.chars().rev();
    if characters.next() != Some('\u{200d}') {
        return false;
    }
    characters
        .find(|character| !character_is_join_control(*character))
        .is_some_and(character_can_join_following)
}

/// Add shaping-only tatweel at run edges requested by explicit ZWJ.
///
/// U+200D at the start or end of an isolated shaping run asks the shaper to
/// form a connection to a neighboring joining context. Some shaping backends
/// do not apply that edge context without a concrete joining neighbor, so the
/// renderer supplies U+0640 ARABIC TATWEEL as shaping-only context and removes
/// it from emitted glyph text:
/// <https://www.w3.org/TR/css-text-3/#text-encoding> and
/// <https://www.w3.org/TR/alreq/#h_joining_enforcement>.
pub(in crate::text) fn push_edge_join_context(
    text: &mut String,
    ranges: &mut Vec<(Range<usize>, &ComputedStyle)>,
    synthetic_ranges: &mut Vec<Range<usize>>,
) {
    let mut insertions = Vec::new();
    for (range, _) in ranges.iter() {
        let Some(slice) = text.get(range.clone()) else {
            continue;
        };
        if let Some(index) = leading_join_context_insertion_index(slice) {
            insertions.push(range.start + index);
        } else if text[..range.start].ends_with('\u{200d}')
            && slice
                .chars()
                .next()
                .is_some_and(character_can_join_preceding)
        {
            // A joiner can be owned by a separately styled span (including a
            // fallback font face selected solely for U+200D).  The adjacent
            // Arabic span still needs a concrete joining neighbor; retain the
            // authored joiner and add only the shaping-only tatweel context at
            // the styled boundary.
            // <https://www.w3.org/TR/css-text-3/#boundary-shaping> and
            // <https://www.w3.org/TR/alreq/#h_joining-enforcement>
            insertions.push(range.start);
        }
        if let Some(index) = trailing_join_context_insertion_index(slice) {
            insertions.push(range.start + index);
        } else if text[range.end..].starts_with('\u{200d}')
            && slice
                .chars()
                .next_back()
                .is_some_and(character_can_join_following)
        {
            insertions.push(range.end);
        }
    }
    insertions.sort_unstable();
    insertions.dedup();
    for index in insertions.into_iter().rev() {
        insert_synthetic_join_context(text, ranges, synthetic_ranges, index);
    }
}

pub(in crate::text) fn range_is_synthetic_only(
    range: Range<usize>,
    synthetic_ranges: &[Range<usize>],
) -> bool {
    range.start < range.end
        && synthetic_ranges
            .iter()
            .any(|synthetic| synthetic.start <= range.start && range.end <= synthetic.end)
}

pub(in crate::text) fn leading_join_context_insertion_index(text: &str) -> Option<usize> {
    if !text.starts_with('\u{200d}') {
        return None;
    }
    text.char_indices()
        .find(|(_, character)| !character_is_join_control(*character))
        .and_then(|(index, character)| character_can_join_preceding(character).then_some(index))
}

pub(in crate::text) fn trailing_join_context_insertion_index(text: &str) -> Option<usize> {
    if !text.ends_with('\u{200d}') {
        return None;
    }
    text.char_indices()
        .rev()
        .find(|(_, character)| !character_is_join_control(*character))
        .and_then(|(index, character)| {
            character_can_join_following(character).then_some(index + character.len_utf8())
        })
}

pub(in crate::text) fn insert_synthetic_join_context(
    text: &mut String,
    ranges: &mut [(Range<usize>, &ComputedStyle)],
    synthetic_ranges: &mut Vec<Range<usize>>,
    index: usize,
) {
    let context_len = '\u{0640}'.len_utf8();
    text.insert(index, '\u{0640}');
    for (range, _) in ranges.iter_mut() {
        if range.start >= index {
            range.start += context_len;
            range.end += context_len;
        } else if range.end >= index {
            range.end += context_len;
        }
    }
    for range in synthetic_ranges.iter_mut() {
        if range.start >= index {
            range.start += context_len;
            range.end += context_len;
        } else if range.end >= index {
            range.end += context_len;
        }
    }
    synthetic_ranges.push(index..index + context_len);
}

/// Remove shaping-only join controls from emitted text content.
///
/// PDF ToUnicode data should reflect the document text, not internal shaping
/// controls inserted to satisfy CSS Text boundary shaping:
/// <https://www.w3.org/TR/css-text-3/#boundary-shaping> and ISO 32000-2
/// section 9.10.3.
pub(in crate::text) fn text_without_synthetic_join_controls(
    text: &str,
    range: Range<usize>,
    synthetic_ranges: &[Range<usize>],
) -> String {
    let Some(slice) = text.get(range.clone()) else {
        return String::new();
    };
    slice
        .char_indices()
        .filter_map(|(offset, character)| {
            let index = range.start + offset;
            (!synthetic_ranges
                .iter()
                .any(|synthetic| synthetic.contains(&index)))
            .then_some(character)
        })
        .collect()
}

/// Remove shaping-only join-control glyph records from fallback-shaped output.
///
/// The fallback shaper maps one input character to one glyph, so synthetic ZWJ
/// code points can be dropped without changing visible glyph advances:
/// <https://www.w3.org/TR/css-text-3/#boundary-shaping>.
pub(in crate::text) fn glyphs_without_synthetic_join_controls(
    glyphs: Vec<RenderedGlyph>,
    raw_text: &str,
    run_start: usize,
    synthetic_ranges: &[Range<usize>],
) -> Vec<RenderedGlyph> {
    let mut glyphs = glyphs.into_iter();
    let mut output = raw_text
        .char_indices()
        .filter_map(|(offset, character)| {
            let mut glyph = glyphs.next()?;
            let index = run_start + offset;
            if synthetic_ranges
                .iter()
                .any(|synthetic| synthetic.contains(&index))
                || character_is_default_ignorable_code_point(character)
            {
                None
            } else {
                glyph.unicode = character.to_string();
                Some(glyph)
            }
        })
        .collect::<Vec<_>>();
    output.extend(glyphs);
    output
}

/// A default-ignorable-only run that participated in shaping but must not
/// contribute visible fallback geometry.
///
/// Font fallback may select a face solely to map a ZWJ, ZWNJ, or another
/// default-ignorable control. The control remains part of the shaping stream,
/// but CSS Text does not give it visual advance. Parley has already included
/// that fallback run's advance in the following runs' visual offsets, so
/// conversion removes the advance from those offsets when the run is omitted.
/// <https://www.w3.org/TR/css-text-3/#text-encoding>
#[derive(Debug, Clone, PartialEq)]
pub(in crate::text) struct DroppedDefaultIgnorableRun {
    pub(in crate::text) x_offset: f32,
    pub(in crate::text) advance: f32,
    /// Authored controls remain in extraction text even though this fallback
    /// run produces no paintable glyphs.
    pub(in crate::text) text: Rc<str>,
}

/// Return whether a complete shaping run consists only of default-ignorable
/// controls.
pub(in crate::text) fn text_is_default_ignorable_only(text: &str) -> bool {
    !text.is_empty() && text.chars().all(character_is_default_ignorable_code_point)
}

/// How a fallback run containing a default-ignorable shaping control converts
/// to PDF glyph payload.
///
/// Default-ignorable controls remain in Parley's input. Conversion may omit a
/// control-only fallback run, or safely re-home one simple visible scalar to
/// its selected CSS face. All other clusters retain Parley's glyph result so
/// complex-script substitutions and positioning cannot be weakened.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::text) enum ControlFallbackCluster {
    DropControlOnly,
    RehomeSimpleVisibleFragment { character: char },
    PreserveParleyGlyphs,
}

/// Inputs needed to re-home one safe control fallback fragment.
#[derive(Debug, Clone)]
pub(in crate::text) struct ControlFallbackRehomeRequest {
    pub(in crate::text) character: char,
    pub(in crate::text) fallback_font_id: usize,
    pub(in crate::text) text: Rc<str>,
    pub(in crate::text) font_size: f32,
    pub(in crate::text) x_offset: f32,
    pub(in crate::text) parley_advance: f32,
    pub(in crate::text) source_range: Option<Range<usize>>,
}

/// Classify a Parley fallback run that may contain ZWNJ/ZWJ.
///
/// Re-homing is intentionally proof-based: one visible source scalar, at
/// least one join control, an unpositioned Parley glyph sequence, and a script
/// whose shaping does not use cursive or Indic-style contextual forms. Any
/// ambiguous cluster keeps its original Parley glyph payload.
pub(in crate::text) fn classify_control_fallback_cluster(
    text: &str,
    has_positioned_glyph: bool,
) -> ControlFallbackCluster {
    if text_is_default_ignorable_only(text) {
        return ControlFallbackCluster::DropControlOnly;
    }
    if has_positioned_glyph || !text.chars().any(character_is_join_control) {
        return ControlFallbackCluster::PreserveParleyGlyphs;
    }
    let mut visible = text
        .chars()
        .filter(|character| !character_is_default_ignorable_code_point(*character));
    let Some(character) = visible.next() else {
        return ControlFallbackCluster::PreserveParleyGlyphs;
    };
    if visible.next().is_none() && character_has_simple_shaping(character) {
        ControlFallbackCluster::RehomeSimpleVisibleFragment { character }
    } else {
        ControlFallbackCluster::PreserveParleyGlyphs
    }
}

fn character_has_simple_shaping(character: char) -> bool {
    if character_has_joining_behavior(character) {
        return false;
    }
    let script = CodePointMapData::<IcuScript>::new().get(character);
    let Some(script) = PropertyNamesShort::<IcuScript>::new().get_locale_script(script) else {
        return false;
    };
    let tag = script.into_raw();
    tag == *b"Latn" || tag == *b"Grek" || tag == *b"Cyrl" || tag == *b"Armn" || tag == *b"Geor"
}

/// Remove omitted default-ignorable fallback advances from a visual run
/// origin.
///
/// Parley exposes run origins in physical visual order. Removing a positive
/// advance therefore shifts only origins physically after the omitted run,
/// independent of the run's logical bidi direction.
pub(in crate::text) fn corrected_visual_run_x_offset(
    x_offset: f32,
    dropped_runs: &[DroppedDefaultIgnorableRun],
) -> f32 {
    x_offset
        - dropped_runs
            .iter()
            .filter(|dropped| dropped.x_offset < x_offset)
            .map(|dropped| dropped.advance)
            .sum::<f32>()
}

/// Stitch a re-homed control fallback fragment to its immediately following
/// compatible visual run.
///
/// The control remains in the combined source text, while the glyph stream is
/// the selected-face stream that would have been produced without the fallback
/// control glyph. Stitching is limited to increasing physical origins; RTL and
/// any non-adjacent visual arrangement retain independent runs.
pub(in crate::text) fn stitch_rehomed_control_fallback_run(
    runs: &mut Vec<ShapedGlyphRun>,
    index: usize,
) -> bool {
    let Some(next_index) = index.checked_add(1).filter(|index| *index < runs.len()) else {
        return false;
    };
    let previous = &runs[index];
    let next = &runs[next_index];
    if next.x_offset < previous.x_offset
        || previous.font_id != next.font_id
        || previous.font_size != next.font_size
        || previous.font_palette != next.font_palette
    {
        return false;
    }
    let next = runs.remove(next_index);
    let previous = &mut runs[index];
    previous.text = format!("{}{}", previous.text, next.text).into();
    previous.glyphs.extend(next.glyphs);
    previous
        .glyph_source_ranges
        .extend(next.glyph_source_ranges);
    true
}

/// Retain an omitted authored join-control run in adjacent visual text.
///
/// A default-ignorable fallback run contributes neither glyphs nor advance,
/// but dropping its source text would change copy/paste and accessibility
/// text. When its neighboring visual runs use the same text presentation,
/// merge all three into one run; otherwise attach the control to the prior
/// run so extraction still preserves logical text without changing paint.
pub(in crate::text) fn stitch_dropped_join_control_runs(
    runs: &mut Vec<ShapedGlyphRun>,
    dropped_runs: &[DroppedDefaultIgnorableRun],
) {
    for dropped in dropped_runs {
        if !dropped.text.chars().any(character_is_join_control) {
            continue;
        }
        let Some(previous_index) = runs
            .iter()
            .enumerate()
            .filter(|(_, run)| run.x_offset <= dropped.x_offset)
            .map(|(index, _)| index)
            .next_back()
        else {
            continue;
        };
        let next_index = (previous_index + 1..runs.len())
            .find(|&index| runs[index].x_offset >= dropped.x_offset);
        let mut combined_text = String::from(runs[previous_index].text.as_ref());
        combined_text.push_str(&dropped.text);
        let can_merge_next = next_index.is_some_and(|index| {
            let previous = &runs[previous_index];
            let next = &runs[index];
            previous.font_id == next.font_id
                && previous.font_size == next.font_size
                && previous.font_palette == next.font_palette
                && previous.y_offset == next.y_offset
                && previous.text_matrix == next.text_matrix
        });
        if can_merge_next {
            let next = runs.remove(next_index.expect("checked above"));
            combined_text.push_str(&next.text);
            let previous = &mut runs[previous_index];
            previous.text = combined_text.into();
            previous.glyphs.extend(next.glyphs);
            previous
                .glyph_source_ranges
                .extend(next.glyph_source_ranges);
        } else {
            runs[previous_index].text = combined_text.into();
        }
    }
}

pub(in crate::text) fn apply_synthetic_position_fallback(
    glyphs: &mut [RenderedGlyph],
    font_size: &mut f32,
    style: &ComputedStyle,
    face: &ttf_parser::Face<'_>,
    text: &str,
) {
    if !style.font_synthesis.position {
        return;
    }
    let (scale, shift) = match style.font_variant_position {
        FontVariantPosition::Sub => (0.65, -*font_size * 0.2),
        FontVariantPosition::Super => (0.65, *font_size * 0.35),
        FontVariantPosition::Normal => return,
    };
    if opentype_position_feature_substituted(glyphs, face, text) {
        return;
    }
    *font_size *= scale;
    for glyph in glyphs {
        glyph.x_advance *= scale;
        glyph.nominal_x_advance *= scale;
        glyph.x_offset *= scale;
        glyph.y_offset = glyph.y_offset * scale + shift;
    }
}

pub(in crate::text) fn opentype_position_feature_substituted(
    glyphs: &[RenderedGlyph],
    face: &ttf_parser::Face<'_>,
    text: &str,
) -> bool {
    let visible_glyphs = glyphs
        .iter()
        .filter(|glyph| !glyph.unicode.is_empty())
        .filter(|glyph| {
            glyph
                .unicode
                .chars()
                .any(|character| !character_is_default_ignorable_code_point(character))
        })
        .collect::<Vec<_>>();
    let visible_characters = text
        .chars()
        .filter(|character| !character_is_default_ignorable_code_point(*character))
        .collect::<Vec<_>>();

    // CSS Fonts requires an all-or-nothing choice for a super/subscript run:
    // if the requested OpenType feature cannot provide alternates for every
    // character, synthesize the whole run instead.  In particular, treating a
    // single substituted digit as sufficient would mix Lato's `sups` glyphs
    // with ordinary spaces and punctuation.
    // <https://drafts.csswg.org/css-fonts-4/#font-variant-position-prop>
    visible_characters.len() == visible_glyphs.len()
        && !visible_characters.is_empty()
        && visible_characters
            .into_iter()
            .zip(visible_glyphs)
            .all(|(character, glyph)| {
                face.glyph_index(character)
                    .is_some_and(|nominal| Some(nominal.0) != glyph.painted_id())
            })
}

/// Return whether a shaped glyph cluster represents only default-ignorable code points.
///
/// CSS text shaping must preserve controls such as ZWJ/ZWNJ and variation
/// selectors in shaping input, while PDF painting must not emit visible
/// fallback glyphs for clusters made only from Unicode default-ignorable code
/// points:
/// <https://www.w3.org/TR/css-text-3/#text-encoding>,
/// <https://www.unicode.org/reports/tr44/#Default_Ignorable_Code_Point>, and
/// ISO 32000-2 section 9.10.3.
pub(in crate::text) fn cluster_is_default_ignorable_only(
    raw_text: &str,
    emitted_text: &str,
) -> bool {
    !raw_text.is_empty()
        && raw_text
            .chars()
            .all(character_is_default_ignorable_code_point)
        && (emitted_text.is_empty()
            || emitted_text
                .chars()
                .all(character_is_default_ignorable_code_point))
}

/// Return whether an empty glyph is a non-painting shaping artifact.
///
/// Join controls can become a font-internal space glyph: its used advance is
/// zero, but its nominal font advance is non-zero. It must not enter the PDF
/// stream because it has no ToUnicode value. A genuine positioned mark also
/// has zero used advance, but its nominal advance is zero, so it remains
/// paintable. This identifies the artifact without treating all
/// default-ignorable source ranges as disposable.
///
/// <https://www.w3.org/TR/css-text-3/#text-encoding> and ISO 32000-2:2020,
/// 9.10.3.
pub(in crate::text) fn glyph_is_non_painting_shaping_artifact(
    face: &ttf_parser::Face<'_>,
    glyph_id: u16,
    used_advance: f32,
    unicode: &str,
) -> bool {
    unicode.is_empty()
        && used_advance == 0.0
        && face
            .glyph_hor_advance(ttf_parser::GlyphId(glyph_id))
            .is_some_and(|advance| advance != 0)
}

pub(in crate::text) fn default_ignorable_cluster_has_shaping_glyph(
    face: &ttf_parser::Face<'_>,
    run_text: &str,
    emitted_cluster_text: &str,
    glyphs: impl IntoIterator<Item = (u16, f32)>,
) -> bool {
    run_text
        .chars()
        .any(|character| !character_is_default_ignorable_code_point(character))
        && glyphs.into_iter().any(|(glyph_id, advance)| {
            advance != 0.0
                && !emitted_cluster_text.chars().any(|character| {
                    face.glyph_index(character)
                        .is_some_and(|nominal| nominal.0 == glyph_id)
                })
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::paint::text::RenderedTextMatrix;
    #[test]
    fn default_ignorable_only_text_is_recognized_as_shaping_control() {
        assert!(text_is_default_ignorable_only("\u{200c}\u{200d}"));
        assert!(!text_is_default_ignorable_only("f\u{200c}i"));
        assert!(!text_is_default_ignorable_only(""));
    }

    #[test]
    fn control_fallback_cluster_classification_preserves_ambiguous_shaping() {
        assert_eq!(
            classify_control_fallback_cluster("\u{200c}\u{200d}", false),
            ControlFallbackCluster::DropControlOnly,
        );
        assert_eq!(
            classify_control_fallback_cluster("f\u{200c}", false),
            ControlFallbackCluster::RehomeSimpleVisibleFragment { character: 'f' },
        );
        assert_eq!(
            classify_control_fallback_cluster("f\u{200c}", true),
            ControlFallbackCluster::PreserveParleyGlyphs,
        );
        assert_eq!(
            classify_control_fallback_cluster("fi\u{200c}", false),
            ControlFallbackCluster::PreserveParleyGlyphs,
        );
        assert_eq!(
            classify_control_fallback_cluster("\u{0627}\u{200d}", false),
            ControlFallbackCluster::PreserveParleyGlyphs,
        );
    }

    #[test]
    fn dropped_default_ignorable_run_shifts_later_visual_origins() {
        let dropped_runs = [DroppedDefaultIgnorableRun {
            x_offset: 10.0,
            advance: 3.0,
            text: Rc::from(""),
        }];

        assert_eq!(corrected_visual_run_x_offset(5.0, &dropped_runs), 5.0);
        assert_eq!(corrected_visual_run_x_offset(10.0, &dropped_runs), 10.0);
        assert_eq!(corrected_visual_run_x_offset(16.0, &dropped_runs), 13.0);
    }

    #[test]
    fn dropped_default_ignorable_correction_is_independent_of_logical_run_order() {
        // RTL runs are emitted in visual order, which can be the reverse of
        // logical text order. The correction therefore depends only on the
        // physical origins Parley provided.
        let dropped_runs = [
            DroppedDefaultIgnorableRun {
                x_offset: 18.0,
                advance: 2.0,
                text: Rc::from(""),
            },
            DroppedDefaultIgnorableRun {
                x_offset: 6.0,
                advance: 1.5,
                text: Rc::from(""),
            },
        ];

        assert_eq!(corrected_visual_run_x_offset(12.0, &dropped_runs), 10.5);
        assert_eq!(corrected_visual_run_x_offset(24.0, &dropped_runs), 20.5);
    }

    #[test]
    fn rehomed_control_fragment_stitches_only_following_compatible_visual_runs() {
        let glyph = |unicode: &str| RenderedGlyph {
            kind: RenderedGlyphKind::Paint(1),
            x_advance: 1.0,
            nominal_x_advance: 1.0,
            x_offset: 0.0,
            y_offset: 0.0,
            unicode: unicode.to_string(),
        };
        let run = |text: &str, x_offset: f32| ShapedGlyphRun {
            text: text.into(),
            x_offset,
            y_offset: 0.0,
            text_matrix: RenderedTextMatrix::IDENTITY,
            font_size: 12.0,
            font_id: Some(1),
            font_palette: FontPalette::Normal,
            glyphs: vec![glyph(text)],
            glyph_source_ranges: vec![None],
        };
        let mut runs = vec![run("f\u{200c}", 2.0), run("i", 3.0)];
        assert!(stitch_rehomed_control_fallback_run(&mut runs, 0));
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0].text.as_ref(), "f\u{200c}i");

        let mut reversed_runs = vec![run("f\u{200c}", 3.0), run("i", 2.0)];
        assert!(!stitch_rehomed_control_fallback_run(&mut reversed_runs, 0));
    }
}

use super::*;
use crate::css::FontPalette;
use crate::document::{PaintSpace, PaintStrokeWidth};
use std::borrow::Cow;
use std::rc::Rc;

/// OpenType glyph-outline coordinates in font design units.
///
/// A glyph outline is not page-local paint geometry. Its conversion to
/// [`PaintPoint`] happens through [`GlyphOutlineToPaint`] exactly once at the
/// font-outline paint boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GlyphOutlineSpace {}

type GlyphOutlinePoint = euclid::Point2D<f32, GlyphOutlineSpace>;
type GlyphOutlineToPaint = euclid::ScaleOffset2D<f32, GlyphOutlineSpace, PaintSpace>;

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

/// Remove default-ignorable controls that must not affect font fallback.
///
/// CSS Text line breaking still operates on the original text. This shaping
/// cleanup only removes default-ignorable controls that are neutral for glyph
/// selection and bidi ordering, preventing controls such as CGJ from making a
/// visible Ahem glyph fall back to another font:
/// <https://www.w3.org/TR/css-text-3/#text-processing-order> and
/// <https://www.w3.org/TR/css-text-3/#line-break-details>.
pub(in crate::text) fn text_without_font_neutral_default_ignorables(text: &str) -> Cow<'_, str> {
    if !text
        .chars()
        .any(character_is_font_neutral_default_ignorable)
    {
        return Cow::Borrowed(text);
    }
    Cow::Owned(
        text.chars()
            .filter(|character| !character_is_font_neutral_default_ignorable(*character))
            .collect(),
    )
}

/// Apply Unicode compatibility substitutions that belong to glyph selection,
/// while retaining the authored text for CSS Text processing and PDF
/// extraction.
///
/// U+2011 NON-BREAKING HYPHEN has the compatibility decomposition U+2010
/// HYPHEN. Shapers apply that decomposition before choosing glyphs, allowing a
/// face with a hyphen glyph (such as Ahem) to render the non-breaking form
/// without falling back to an unrelated face. Its original line-break class
/// and the PDF ToUnicode value must nevertheless remain U+2011, so callers
/// use this only for the transient Parley shaping input. The substitution is
/// byte-length preserving, which keeps shaped ranges aligned with source
/// ranges.
///
/// <https://www.unicode.org/reports/tr15/#Compatibility_Formatting_Characters>
/// and <https://www.w3.org/TR/css-text-3/#text-processing-order>.
pub(in crate::text) fn text_with_shaping_compatibility_normalization(text: &str) -> Cow<'_, str> {
    if !text.contains('\u{2011}') {
        return Cow::Borrowed(text);
    }
    Cow::Owned(text.replace('\u{2011}', "\u{2010}"))
}

pub(in crate::text) fn text_with_font_variant_emoji<'a>(
    text: &'a str,
    style: &ComputedStyle,
) -> Cow<'a, str> {
    if matches!(
        style.font_variant_emoji,
        FontVariantEmoji::Normal | FontVariantEmoji::Unicode
    ) {
        return Cow::Borrowed(text);
    }
    let mut output = String::with_capacity(text.len());
    push_text_with_font_variant_emoji(&mut output, text, style);
    if output == text {
        Cow::Borrowed(text)
    } else {
        Cow::Owned(output)
    }
}

pub(in crate::text) fn push_text_with_font_variant_emoji(
    output: &mut String,
    text: &str,
    style: &ComputedStyle,
) {
    let selector = match style.font_variant_emoji {
        FontVariantEmoji::Text => '\u{fe0e}',
        FontVariantEmoji::Emoji => '\u{fe0f}',
        FontVariantEmoji::Normal | FontVariantEmoji::Unicode => {
            output.push_str(text);
            return;
        }
    };
    let mut chars = text.chars().peekable();
    while let Some(character) = chars.next() {
        output.push(character);
        if emoji_presentation_participating_code_point(character)
            && !chars
                .peek()
                .is_some_and(|next| matches!(*next, '\u{fe0e}' | '\u{fe0f}'))
        {
            output.push(selector);
        }
    }
}

pub(in crate::text) fn text_without_variation_selectors(text: &str) -> Cow<'_, str> {
    if !text
        .chars()
        .any(|character| matches!(character, '\u{fe00}'..='\u{fe0f}' | '\u{e0100}'..='\u{e01ef}'))
    {
        return Cow::Borrowed(text);
    }
    Cow::Owned(
        text.chars()
            .filter(|character| {
                !matches!(character, '\u{fe00}'..='\u{fe0f}' | '\u{e0100}'..='\u{e01ef}')
            })
            .collect(),
    )
}

pub(in crate::text) fn text_without_glyph_output_controls(text: &str) -> Cow<'_, str> {
    if !text.chars().any(|character| {
        character_is_join_control(character)
            || matches!(
                character,
                '\u{fe00}'..='\u{fe0f}' | '\u{e0100}'..='\u{e01ef}'
            )
    }) {
        return Cow::Borrowed(text);
    }
    Cow::Owned(
        text.chars()
            .filter(|character| {
                !character_is_join_control(*character)
                    && !matches!(
                        character,
                        '\u{fe00}'..='\u{fe0f}' | '\u{e0100}'..='\u{e01ef}'
                    )
            })
            .collect(),
    )
}

/// Return whether a code point has standardized text and emoji variation
/// sequences.
///
/// CSS Fonts calls these Emoji Presentation Participating Code Points and
/// defines `font-variant-emoji` in terms of Unicode's registered emoji
/// variation sequences. The range table below was generated from Unicode
/// Emoji 15.1's `emoji-variation-sequences.txt`; it has 371 bases in 183
/// inclusive ranges. It must not be replaced by either the `Emoji` or
/// `Emoji_Presentation` properties: the former is broader, while the latter
/// excludes text-default bases such as the keycap digits.
///
/// <https://www.w3.org/TR/css-fonts-4/#font-variant-emoji-prop>
/// <https://www.unicode.org/Public/15.1.0/ucd/emoji/emoji-variation-sequences.txt>
pub(in crate::text) fn emoji_presentation_participating_code_point(character: char) -> bool {
    const EMOJI_VARIATION_SEQUENCE_BASE_RANGES: &[(u32, u32)] = &[
        (0x0023, 0x0023),
        (0x002a, 0x002a),
        (0x0030, 0x0039),
        (0x00a9, 0x00a9),
        (0x00ae, 0x00ae),
        (0x203c, 0x203c),
        (0x2049, 0x2049),
        (0x2122, 0x2122),
        (0x2139, 0x2139),
        (0x2194, 0x2199),
        (0x21a9, 0x21aa),
        (0x231a, 0x231b),
        (0x2328, 0x2328),
        (0x23cf, 0x23cf),
        (0x23e9, 0x23f3),
        (0x23f8, 0x23fa),
        (0x24c2, 0x24c2),
        (0x25aa, 0x25ab),
        (0x25b6, 0x25b6),
        (0x25c0, 0x25c0),
        (0x25fb, 0x25fe),
        (0x2600, 0x2604),
        (0x260e, 0x260e),
        (0x2611, 0x2611),
        (0x2614, 0x2615),
        (0x2618, 0x2618),
        (0x261d, 0x261d),
        (0x2620, 0x2620),
        (0x2622, 0x2623),
        (0x2626, 0x2626),
        (0x262a, 0x262a),
        (0x262e, 0x262f),
        (0x2638, 0x263a),
        (0x2640, 0x2640),
        (0x2642, 0x2642),
        (0x2648, 0x2653),
        (0x265f, 0x2660),
        (0x2663, 0x2663),
        (0x2665, 0x2666),
        (0x2668, 0x2668),
        (0x267b, 0x267b),
        (0x267e, 0x267f),
        (0x2692, 0x2697),
        (0x2699, 0x2699),
        (0x269b, 0x269c),
        (0x26a0, 0x26a1),
        (0x26a7, 0x26a7),
        (0x26aa, 0x26ab),
        (0x26b0, 0x26b1),
        (0x26bd, 0x26be),
        (0x26c4, 0x26c5),
        (0x26c8, 0x26c8),
        (0x26ce, 0x26cf),
        (0x26d1, 0x26d1),
        (0x26d3, 0x26d4),
        (0x26e9, 0x26ea),
        (0x26f0, 0x26f5),
        (0x26f7, 0x26fa),
        (0x26fd, 0x26fd),
        (0x2702, 0x2702),
        (0x2705, 0x2705),
        (0x2708, 0x270d),
        (0x270f, 0x270f),
        (0x2712, 0x2712),
        (0x2714, 0x2714),
        (0x2716, 0x2716),
        (0x271d, 0x271d),
        (0x2721, 0x2721),
        (0x2728, 0x2728),
        (0x2733, 0x2734),
        (0x2744, 0x2744),
        (0x2747, 0x2747),
        (0x274c, 0x274c),
        (0x274e, 0x274e),
        (0x2753, 0x2755),
        (0x2757, 0x2757),
        (0x2763, 0x2764),
        (0x2795, 0x2797),
        (0x27a1, 0x27a1),
        (0x27b0, 0x27b0),
        (0x27bf, 0x27bf),
        (0x2934, 0x2935),
        (0x2b05, 0x2b07),
        (0x2b1b, 0x2b1c),
        (0x2b50, 0x2b50),
        (0x2b55, 0x2b55),
        (0x3030, 0x3030),
        (0x303d, 0x303d),
        (0x3297, 0x3297),
        (0x3299, 0x3299),
        (0x1f004, 0x1f004),
        (0x1f170, 0x1f171),
        (0x1f17e, 0x1f17f),
        (0x1f202, 0x1f202),
        (0x1f21a, 0x1f21a),
        (0x1f22f, 0x1f22f),
        (0x1f237, 0x1f237),
        (0x1f30d, 0x1f30f),
        (0x1f315, 0x1f315),
        (0x1f31c, 0x1f31c),
        (0x1f321, 0x1f321),
        (0x1f324, 0x1f32c),
        (0x1f336, 0x1f336),
        (0x1f378, 0x1f378),
        (0x1f37d, 0x1f37d),
        (0x1f393, 0x1f393),
        (0x1f396, 0x1f397),
        (0x1f399, 0x1f39b),
        (0x1f39e, 0x1f39f),
        (0x1f3a7, 0x1f3a7),
        (0x1f3ac, 0x1f3ae),
        (0x1f3c2, 0x1f3c2),
        (0x1f3c4, 0x1f3c4),
        (0x1f3c6, 0x1f3c6),
        (0x1f3ca, 0x1f3ce),
        (0x1f3d4, 0x1f3e0),
        (0x1f3ed, 0x1f3ed),
        (0x1f3f3, 0x1f3f3),
        (0x1f3f5, 0x1f3f5),
        (0x1f3f7, 0x1f3f7),
        (0x1f408, 0x1f408),
        (0x1f415, 0x1f415),
        (0x1f41f, 0x1f41f),
        (0x1f426, 0x1f426),
        (0x1f43f, 0x1f43f),
        (0x1f441, 0x1f442),
        (0x1f446, 0x1f449),
        (0x1f44d, 0x1f44e),
        (0x1f453, 0x1f453),
        (0x1f46a, 0x1f46a),
        (0x1f47d, 0x1f47d),
        (0x1f4a3, 0x1f4a3),
        (0x1f4b0, 0x1f4b0),
        (0x1f4b3, 0x1f4b3),
        (0x1f4bb, 0x1f4bb),
        (0x1f4bf, 0x1f4bf),
        (0x1f4cb, 0x1f4cb),
        (0x1f4da, 0x1f4da),
        (0x1f4df, 0x1f4df),
        (0x1f4e4, 0x1f4e6),
        (0x1f4ea, 0x1f4ed),
        (0x1f4f7, 0x1f4f7),
        (0x1f4f9, 0x1f4fb),
        (0x1f4fd, 0x1f4fd),
        (0x1f508, 0x1f508),
        (0x1f50d, 0x1f50d),
        (0x1f512, 0x1f513),
        (0x1f549, 0x1f54a),
        (0x1f550, 0x1f567),
        (0x1f56f, 0x1f570),
        (0x1f573, 0x1f579),
        (0x1f587, 0x1f587),
        (0x1f58a, 0x1f58d),
        (0x1f590, 0x1f590),
        (0x1f5a5, 0x1f5a5),
        (0x1f5a8, 0x1f5a8),
        (0x1f5b1, 0x1f5b2),
        (0x1f5bc, 0x1f5bc),
        (0x1f5c2, 0x1f5c4),
        (0x1f5d1, 0x1f5d3),
        (0x1f5dc, 0x1f5de),
        (0x1f5e1, 0x1f5e1),
        (0x1f5e3, 0x1f5e3),
        (0x1f5e8, 0x1f5e8),
        (0x1f5ef, 0x1f5ef),
        (0x1f5f3, 0x1f5f3),
        (0x1f5fa, 0x1f5fa),
        (0x1f610, 0x1f610),
        (0x1f687, 0x1f687),
        (0x1f68d, 0x1f68d),
        (0x1f691, 0x1f691),
        (0x1f694, 0x1f694),
        (0x1f698, 0x1f698),
        (0x1f6ad, 0x1f6ad),
        (0x1f6b2, 0x1f6b2),
        (0x1f6b9, 0x1f6ba),
        (0x1f6bc, 0x1f6bc),
        (0x1f6cb, 0x1f6cb),
        (0x1f6cd, 0x1f6cf),
        (0x1f6e0, 0x1f6e5),
        (0x1f6e9, 0x1f6e9),
        (0x1f6f0, 0x1f6f0),
        (0x1f6f3, 0x1f6f3),
    ];

    let code_point = character as u32;
    let candidate_index =
        EMOJI_VARIATION_SEQUENCE_BASE_RANGES.partition_point(|(_, end)| *end < code_point);
    EMOJI_VARIATION_SEQUENCE_BASE_RANGES
        .get(candidate_index)
        .is_some_and(|(start, _)| code_point >= *start)
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

impl FontSystem {
    /// Return vector coverage for glyphs whose outlines are exactly one
    /// full-em opaque rectangle, while retaining their normal PDF text runs.
    ///
    /// A full-em glyph (such as the square test glyph used by CSS conformance
    /// suites) has identical CSS ink and advance geometry.  PDF text
    /// rasterizers may nevertheless hint its outline on a different pixel
    /// boundary from an adjacent vector background.  Repainting only this
    /// provably equivalent outline through the vector path boundary keeps the
    /// text object for selection and ToUnicode while making its coverage share
    /// the page-space geometry used by backgrounds and borders.
    ///
    /// CSS Writing Modes defines glyph orientation independently from the
    /// logical inline and block axes, so the retained run matrix is applied to
    /// the outline exactly once here:
    /// <https://www.w3.org/TR/css-writing-modes-4/#text-orientation>.
    pub(crate) fn full_em_rect_glyph_coverage_paths(
        &self,
        origin: PaintPoint,
        runs: &[RenderedTextRun],
        color: CssColor,
    ) -> Vec<RenderedPath> {
        if !color.is_opaque() {
            return Vec::new();
        }

        let mut paths = Vec::new();
        for run in runs {
            let Some(font_id) = run.font_id else {
                continue;
            };
            let Some(font) = self.document_fonts.get(font_id) else {
                continue;
            };
            let Ok(face) = ttf_parser::Face::parse(&font.data, font.face_index) else {
                continue;
            };
            let Some(glyphs) = run.glyphs.as_ref() else {
                continue;
            };
            let units_per_em = font.units_per_em.max(1) as f32;
            let scale = run.font_size / units_per_em;
            let [a, b, c, d] = run.text_matrix.pdf_components();
            // Mirror PDF text emission exactly: `x_offset`/`y_offset` select
            // the run text matrix origin, while each glyph starts from the
            // run-local pen.  Folding the run offset into the pen would put
            // a rotated outline on the wrong physical axis.
            let transform =
                PaintTransform::new(a, b, c, d, origin.x + run.x_offset, origin.y + run.y_offset);
            let mut cursor = 0.0;
            for glyph in glyphs.iter() {
                let Some(glyph_id) = glyph.painted_id().map(ttf_parser::GlyphId) else {
                    cursor += glyph.x_advance;
                    continue;
                };
                if !full_em_rectangle_outline(&face, glyph_id, units_per_em) {
                    cursor += glyph.x_advance;
                    continue;
                }
                let mut builder = GlyphPathBuilder::new(GlyphOutlineToPaint::new(
                    scale,
                    scale,
                    cursor + glyph.x_offset,
                    run.y_offset + glyph.y_offset,
                ));
                if face.outline_glyph(glyph_id, &mut builder).is_some()
                    && !builder.commands.is_empty()
                {
                    let path = RenderedPath::new(
                        builder.commands,
                        Some(color),
                        RenderedPathFillRule::NonZero,
                        None,
                        PaintStrokeWidth::ZERO,
                        None,
                    )
                    .with_transform(transform);
                    // `full_em_rectangle_outline` established that this
                    // transformed path covers its bounding rectangle without
                    // holes, curves, or transparency.
                    let coverage = path
                        .bounds()
                        .expect("full-em rectangle outline has finite bounds");
                    paths.push(path.with_opaque_coverage_rect(coverage));
                }
                cursor += glyph.x_advance;
            }
        }
        paths
    }

    /// Convert bitmap OpenType glyphs into raster paint operations.
    ///
    /// PDF Type 0 fonts can embed outline programs, but `sbix`, `CBDT`,
    /// `EBDT`, and `bdat` glyph data is raster artwork.  Keep shaping and
    /// layout advances in the text run while replacing only the paintable
    /// bitmap glyphs with image XObjects.
    ///
    /// OpenType raster glyph metrics are expressed in strike pixels; their
    /// placement is converted at this boundary into page-local paint points.
    /// <https://learn.microsoft.com/en-us/typography/opentype/spec/sbix>
    /// <https://learn.microsoft.com/en-us/typography/opentype/spec/cbdt>
    pub(crate) fn take_raster_glyph_images(
        &self,
        origin: PaintPoint,
        runs: &mut [RenderedTextRun],
    ) -> Vec<RenderedImage> {
        let mut images = Vec::new();
        for run in runs {
            let Some(font_id) = run.font_id else {
                continue;
            };
            let Some(font) = self.document_fonts.get(font_id) else {
                continue;
            };
            let Ok(face) = ttf_parser::Face::parse(&font.data, font.face_index) else {
                continue;
            };
            let Some(glyphs) = run.glyphs.as_ref() else {
                continue;
            };

            // CSS absolute units use 96 px per inch while Quire paint space
            // uses points, so one CSS px is 0.75 paint points.
            let requested_ppem = (run.font_size * (96.0 / 72.0))
                .round()
                .clamp(1.0, u16::MAX as f32) as u16;
            let mut cursor = run.x_offset;
            let mut retained = Vec::with_capacity(glyphs.len());
            for glyph in glyphs.iter() {
                let Some(glyph_id) = glyph.painted_id().map(ttf_parser::GlyphId) else {
                    retained.push(glyph.clone());
                    cursor += glyph.x_advance;
                    continue;
                };
                let Some(raster) = face.glyph_raster_image(glyph_id, requested_ppem) else {
                    retained.push(glyph.clone());
                    cursor += glyph.x_advance;
                    continue;
                };
                let Some(decoded) = decode_raster_glyph_image(raster) else {
                    log::warn!(
                        "unable to decode bitmap glyph {} from font {}; retaining it for the PDF font path",
                        glyph_id.0,
                        font.post_script_name
                    );
                    retained.push(glyph.clone());
                    cursor += glyph.x_advance;
                    continue;
                };
                if raster.pixels_per_em == 0 || decoded.width == 0 || decoded.height == 0 {
                    log::warn!(
                        "bitmap glyph {} from font {} has unusable strike metrics; retaining it for the PDF font path",
                        glyph_id.0,
                        font.post_script_name
                    );
                    retained.push(glyph.clone());
                    cursor += glyph.x_advance;
                    continue;
                }
                let pixel_scale = run.font_size / raster.pixels_per_em as f32;
                let local_origin = PaintPoint::new(
                    cursor + glyph.x_offset + raster.x as f32 * pixel_scale,
                    run.y_offset + glyph.y_offset + raster.y as f32 * pixel_scale,
                );
                let rect = if run.text_matrix.is_identity() {
                    PaintRect::new(
                        PaintPoint::new(origin.x + local_origin.x, origin.y + local_origin.y),
                        PaintSize::new(
                            decoded.width as f32 * pixel_scale,
                            decoded.height as f32 * pixel_scale,
                        ),
                    )
                } else {
                    PaintRect::new(
                        local_origin,
                        PaintSize::new(
                            decoded.width as f32 * pixel_scale,
                            decoded.height as f32 * pixel_scale,
                        ),
                    )
                };
                let mut image = RenderedImage::from_paint_rect(
                    rect,
                    false,
                    decoded.width,
                    decoded.height,
                    None,
                    false,
                    decoded.rgb,
                    decoded.alpha,
                    None,
                );
                if !glyph.unicode.is_empty() {
                    image = image.with_actual_text(Rc::from(glyph.unicode.as_str()));
                }
                if !run.text_matrix.is_identity() {
                    let [a, b, c, d] = run.text_matrix.pdf_components();
                    image =
                        image.with_transform(PaintTransform::new(a, b, c, d, origin.x, origin.y));
                }
                images.push(image);
                cursor += glyph.x_advance;
            }
            run.glyphs = (!retained.is_empty()).then(|| retained.into());
        }
        images
    }

    /// Convert COLR v0 glyph layers into PDF path paint operations.
    ///
    /// PDF text objects cannot select CPAL palettes. Painting the layer outlines
    /// explicitly keeps CSS Fonts palette choice visible in PDF output while
    /// retaining normal shaping for advances and line layout.
    /// <https://www.w3.org/TR/css-fonts-4/#font-palette-prop>
    /// <https://learn.microsoft.com/en-us/typography/opentype/spec/colr>
    pub(crate) fn take_color_glyph_paths(
        &self,
        origin: PaintPoint,
        runs: &mut [RenderedTextRun],
        palettes: &[FontPalette],
        style: &ComputedStyle,
    ) -> Vec<RenderedPath> {
        let mut paths = Vec::new();
        for (index, run) in runs.iter_mut().enumerate() {
            if !run.text_matrix.is_identity() {
                continue;
            }
            let Some(font_id) = run.font_id else {
                continue;
            };
            let Some(font) = self.document_fonts.get(font_id) else {
                continue;
            };
            let Ok(face) = ttf_parser::Face::parse(&font.data, font.face_index) else {
                continue;
            };
            let selection = palettes.get(index).unwrap_or(&style.font_palette);
            let (palette, overrides) =
                self.color_palette_selection(&face, selection, font_id, style);
            let Some(glyphs) = run.glyphs.as_ref() else {
                continue;
            };
            let mut cursor = run.x_offset;
            let mut retained = Vec::with_capacity(glyphs.len());
            for glyph in glyphs.iter() {
                let Some(glyph_id) = glyph.painted_id().map(ttf_parser::GlyphId) else {
                    retained.push(glyph.clone());
                    cursor += glyph.x_advance;
                    continue;
                };
                if !face.is_color_glyph(glyph_id) {
                    retained.push(glyph.clone());
                    cursor += glyph.x_advance;
                    continue;
                }
                let scale = run.font_size / font.units_per_em.max(1) as f32;
                let mut painter = ColrPathPainter {
                    face: &face,
                    outline_to_paint: GlyphOutlineToPaint::new(
                        scale,
                        scale,
                        origin.x + cursor + glyph.x_offset,
                        origin.y + run.y_offset + glyph.y_offset,
                    ),
                    paths: Vec::new(),
                    outlines: Vec::new(),
                };
                // COLR v0 palettes are rasterized as sRGB by ttf-parser.
                let color = crate::css::color_to_predefined_rgb(
                    style.color,
                    crate::css::CssColorSpace::Srgb,
                )
                .expect("sRGB is a predefined CSS RGB space");
                let foreground = ttf_parser::RgbaColor {
                    red: (color.components()[0] * 255.0).round() as u8,
                    green: (color.components()[1] * 255.0).round() as u8,
                    blue: (color.components()[2] * 255.0).round() as u8,
                    alpha: (color.alpha() * 255.0).round() as u8,
                };
                let painted = match overrides {
                    Some(overrides) if !overrides.is_empty() => {
                        paint_colr_v0_glyph(glyph_id, palette, foreground, overrides, &mut painter)
                    }
                    _ => false,
                };
                if painted
                    || face
                        .paint_color_glyph(glyph_id, palette, foreground, &mut painter)
                        .is_some()
                {
                    paths.append(&mut painter.paths);
                } else {
                    retained.push(glyph.clone());
                }
                cursor += glyph.x_advance;
            }
            run.glyphs = (!retained.is_empty()).then(|| retained.into());
        }
        paths
    }
}

struct DecodedRasterGlyph {
    width: u32,
    height: u32,
    rgb: Rc<[u8]>,
    alpha: Option<Rc<[u8]>>,
}

fn decode_raster_glyph_image(
    image: ttf_parser::RasterGlyphImage<'_>,
) -> Option<DecodedRasterGlyph> {
    let width = u32::from(image.width);
    let height = u32::from(image.height);
    let pixel_count = usize::try_from(width.checked_mul(height)?).ok()?;
    if pixel_count == 0 {
        return None;
    }
    let (rgb, alpha) = match image.format {
        ttf_parser::RasterImageFormat::PNG => decode_png_raster_glyph(image.data, pixel_count)?,
        ttf_parser::RasterImageFormat::BitmapPremulBgra32 => {
            decode_premultiplied_bgra(image.data, pixel_count)?
        }
        format => decode_grayscale_raster(image.data, width, height, format)?,
    };
    Some(DecodedRasterGlyph {
        width,
        height,
        rgb: Rc::from(rgb.into_boxed_slice()),
        alpha: alpha.map(|alpha| Rc::from(alpha.into_boxed_slice())),
    })
}

fn decode_png_raster_glyph(data: &[u8], pixel_count: usize) -> Option<(Vec<u8>, Option<Vec<u8>>)> {
    let decoded = image::load_from_memory_with_format(data, image::ImageFormat::Png)
        .ok()?
        .to_rgba8();
    if decoded.len() / 4 != pixel_count {
        return None;
    }
    rgba_to_rgb_alpha(decoded.as_raw())
}

fn decode_premultiplied_bgra(
    data: &[u8],
    pixel_count: usize,
) -> Option<(Vec<u8>, Option<Vec<u8>>)> {
    if data.len() != pixel_count.checked_mul(4)? {
        return None;
    }
    let mut rgb = Vec::with_capacity(pixel_count * 3);
    let mut alpha = Vec::with_capacity(pixel_count);
    let mut has_alpha = false;
    for pixel in data.chunks_exact(4) {
        let [blue, green, red, opacity] = pixel.try_into().ok()?;
        let unpremultiply = |component: u8| {
            if opacity == 0 {
                0
            } else {
                ((u16::from(component) * 255 + u16::from(opacity) / 2) / u16::from(opacity))
                    .min(255) as u8
            }
        };
        rgb.extend_from_slice(&[
            unpremultiply(red),
            unpremultiply(green),
            unpremultiply(blue),
        ]);
        alpha.push(opacity);
        has_alpha |= opacity < 255;
    }
    Some((rgb, has_alpha.then_some(alpha)))
}

fn decode_grayscale_raster(
    data: &[u8],
    width: u32,
    height: u32,
    format: ttf_parser::RasterImageFormat,
) -> Option<(Vec<u8>, Option<Vec<u8>>)> {
    let (bits_per_pixel, packed) = match format {
        ttf_parser::RasterImageFormat::BitmapMono => (1, false),
        ttf_parser::RasterImageFormat::BitmapMonoPacked => (1, true),
        ttf_parser::RasterImageFormat::BitmapGray2 => (2, false),
        ttf_parser::RasterImageFormat::BitmapGray2Packed => (2, true),
        ttf_parser::RasterImageFormat::BitmapGray4 => (4, false),
        ttf_parser::RasterImageFormat::BitmapGray4Packed => (4, true),
        ttf_parser::RasterImageFormat::BitmapGray8 => (8, false),
        _ => return None,
    };
    let pixel_count = usize::try_from(width.checked_mul(height)?).ok()?;
    let row_bits = usize::try_from(width).ok()?.checked_mul(bits_per_pixel)?;
    let padded_row_bytes = row_bits.checked_add(7)? / 8;
    let required_bytes = if packed {
        pixel_count.checked_mul(bits_per_pixel)?.checked_add(7)? / 8
    } else {
        padded_row_bytes.checked_mul(usize::try_from(height).ok()?)?
    };
    if data.len() < required_bytes {
        return None;
    }
    let mut rgb = Vec::with_capacity(pixel_count * 3);
    let mut alpha = Vec::with_capacity(pixel_count);
    let max_value = (1u16 << bits_per_pixel) - 1;
    for pixel_index in 0..pixel_count {
        let bit_offset = if packed {
            pixel_index.checked_mul(bits_per_pixel)?
        } else {
            let row = pixel_index / usize::try_from(width).ok()?;
            let column = pixel_index % usize::try_from(width).ok()?;
            row.checked_mul(padded_row_bytes)?
                .checked_mul(8)?
                .checked_add(column.checked_mul(bits_per_pixel)?)?
        };
        let byte = *data.get(bit_offset / 8)?;
        let shift = 8usize
            .checked_sub(bits_per_pixel)?
            .checked_sub(bit_offset % 8)?;
        let value = (byte >> shift) & ((1u8 << bits_per_pixel) - 1);
        let opacity = ((u16::from(value) * 255 + max_value / 2) / max_value) as u8;
        rgb.extend_from_slice(&[0, 0, 0]);
        alpha.push(opacity);
    }
    Some((rgb, Some(alpha)))
}

fn rgba_to_rgb_alpha(rgba: &[u8]) -> Option<(Vec<u8>, Option<Vec<u8>>)> {
    if !rgba.len().is_multiple_of(4) {
        return None;
    }
    let mut rgb = Vec::with_capacity(rgba.len() / 4 * 3);
    let mut alpha = Vec::with_capacity(rgba.len() / 4);
    let mut has_alpha = false;
    for pixel in rgba.chunks_exact(4) {
        rgb.extend_from_slice(&pixel[..3]);
        alpha.push(pixel[3]);
        has_alpha |= pixel[3] < 255;
    }
    Some((rgb, has_alpha.then_some(alpha)))
}

impl FontSystem {
    fn color_palette_selection<'a>(
        &'a self,
        face: &ttf_parser::Face<'_>,
        selection: &FontPalette,
        font_id: usize,
        style: &ComputedStyle,
    ) -> (u16, Option<&'a HashMap<u16, CssColor>>) {
        // A `@font-palette-values` rule matches the CSS family of the
        // selected `@font-face`, which can deliberately be the empty string.
        // The embedded OpenType family name is not an equivalent identifier.
        // <https://drafts.csswg.org/css-fonts-4/#font-palette-values>
        let family = self
            .document_fonts
            .selected_face_features(font_id)
            .map(|(family, _)| family)
            .or_else(|| font_feature_family(&style.font_family))
            .or_else(|| {
                self.document_fonts
                    .get(font_id)
                    .map(|font| font.family.clone())
            });
        match selection {
            FontPalette::Named(name) => self
                .font_palette_values
                .get(name)
                .and_then(|definitions| {
                    definitions.iter().rev().find(|definition| {
                        definition.families.is_empty()
                            || definition.families.iter().any(|defined_family| {
                                family.as_ref().is_some_and(|family| {
                                    defined_family.trim().eq_ignore_ascii_case(family.trim())
                                })
                            })
                    })
                })
                .map_or((0, None), |definition| {
                    (
                        color_palette_index(face, &definition.base),
                        Some(&definition.overrides),
                    )
                }),
            selection => (color_palette_index(face, selection), None),
        }
    }
}

fn color_palette_index(face: &ttf_parser::Face<'_>, selection: &FontPalette) -> u16 {
    let count = face.color_palettes().map_or(0, |count| count.get());
    if count == 0 {
        return 0;
    }
    match selection {
        FontPalette::Normal | FontPalette::Named(_) => 0,
        FontPalette::Index(index) => (*index).min(count - 1),
        FontPalette::Light => cpal_palette_with_type(face, 1).unwrap_or(0),
        FontPalette::Dark => cpal_palette_with_type(face, 2).unwrap_or(0),
    }
}

/// CPAL version 1 stores a u32 palette-type flag for every palette. These
/// flags select `font-palette: light` and `dark` without exposing platform
/// color preferences through the CSS cascade.
/// <https://learn.microsoft.com/en-us/typography/opentype/spec/cpal>
fn cpal_palette_with_type(face: &ttf_parser::Face<'_>, wanted: u32) -> Option<u16> {
    let data = face
        .raw_face()
        .table(ttf_parser::Tag::from_bytes(b"CPAL"))?;
    let version = read_u16(data, 0)?;
    if version < 1 {
        return None;
    }
    let palette_count = read_u16(data, 4)? as usize;
    let types_offset = 12usize.checked_add(palette_count.checked_mul(2)?)?;
    let types_offset = read_u32(data, types_offset)? as usize;
    (0..palette_count).find_map(|index| {
        (read_u32(data, types_offset.checked_add(index.checked_mul(4)?)?)? & wanted != 0)
            .then_some(index as u16)
    })
}

fn read_u16(data: &[u8], offset: usize) -> Option<u16> {
    Some(u16::from_be_bytes(
        data.get(offset..offset.checked_add(2)?)?.try_into().ok()?,
    ))
}

fn read_u32(data: &[u8], offset: usize) -> Option<u32> {
    Some(u32::from_be_bytes(
        data.get(offset..offset.checked_add(4)?)?.try_into().ok()?,
    ))
}

/// Paint a COLR version 0 base-glyph record directly when CSS overrides CPAL
/// entries. The public OpenType painter correctly resolves colors, but its
/// `Paint::Solid` callback intentionally exposes only the resolved RGBA value,
/// whereas CSS `override-colors` applies by CPAL entry index.
/// <https://learn.microsoft.com/en-us/typography/opentype/spec/colr>
/// <https://www.w3.org/TR/css-fonts-4/#font-palette-values-override-colors>
fn paint_colr_v0_glyph(
    glyph_id: ttf_parser::GlyphId,
    palette: u16,
    foreground: ttf_parser::RgbaColor,
    overrides: &HashMap<u16, CssColor>,
    painter: &mut ColrPathPainter<'_>,
) -> bool {
    let face = painter.face;
    let Some(data) = face.raw_face().table(ttf_parser::Tag::from_bytes(b"COLR")) else {
        return false;
    };
    if read_u16(data, 0) != Some(0) {
        return false;
    }
    let Some(base_count) = read_u16(data, 2) else {
        return false;
    };
    let Some(base_offset) = read_u32(data, 4).map(|offset| offset as usize) else {
        return false;
    };
    let Some(layer_offset) = read_u32(data, 8).map(|offset| offset as usize) else {
        return false;
    };
    let Some(layer_count) = read_u16(data, 12) else {
        return false;
    };
    let base = (0..base_count as usize).find_map(|index| {
        let offset = base_offset.checked_add(index.checked_mul(6)?)?;
        if read_u16(data, offset)? == glyph_id.0 {
            Some((read_u16(data, offset + 2)?, read_u16(data, offset + 4)?))
        } else {
            None
        }
    });
    let Some((first_layer, layers)) = base else {
        return false;
    };
    let foreground = CssColor::rgba(
        foreground.red,
        foreground.green,
        foreground.blue,
        foreground.alpha as f32 / 255.0,
    );
    for layer_index in first_layer as usize..first_layer as usize + layers as usize {
        if layer_index >= layer_count as usize {
            return false;
        }
        let Some(offset) = layer_offset.checked_add(layer_index * 4) else {
            return false;
        };
        let (Some(layer_glyph), Some(color_index)) =
            (read_u16(data, offset), read_u16(data, offset + 2))
        else {
            return false;
        };
        let color = if color_index == u16::MAX {
            foreground
        } else if let Some(color) = overrides.get(&color_index) {
            *color
        } else if let Some(color) = cpal_color(face, palette, color_index) {
            color
        } else {
            return false;
        };
        append_colr_outline(
            face,
            ttf_parser::GlyphId(layer_glyph),
            painter.outline_to_paint,
            color,
            &mut painter.paths,
        );
    }
    true
}

fn cpal_color(face: &ttf_parser::Face<'_>, palette: u16, color_index: u16) -> Option<CssColor> {
    let data = face
        .raw_face()
        .table(ttf_parser::Tag::from_bytes(b"CPAL"))?;
    let entries_per_palette = read_u16(data, 2)? as usize;
    let palette_count = read_u16(data, 4)? as usize;
    let color_record_count = read_u16(data, 6)? as usize;
    let color_records_offset = read_u32(data, 8)? as usize;
    let palette_index_offset = 12usize.checked_add((palette as usize).checked_mul(2)?)?;
    if palette as usize >= palette_count || color_index as usize >= entries_per_palette {
        return None;
    }
    let record_index = read_u16(data, palette_index_offset)? as usize + color_index as usize;
    if record_index >= color_record_count {
        return None;
    }
    let offset = color_records_offset.checked_add(record_index.checked_mul(4)?)?;
    let [blue, green, red, alpha] = data.get(offset..offset.checked_add(4)?)?.try_into().ok()?;
    Some(CssColor::rgba(red, green, blue, alpha as f32 / 255.0))
}

fn append_colr_outline(
    face: &ttf_parser::Face<'_>,
    glyph_id: ttf_parser::GlyphId,
    outline_to_paint: GlyphOutlineToPaint,
    color: CssColor,
    paths: &mut Vec<RenderedPath>,
) {
    let mut builder = GlyphPathBuilder::new(outline_to_paint);
    if face.outline_glyph(glyph_id, &mut builder).is_some() && !builder.commands.is_empty() {
        paths.push(RenderedPath::new(
            builder.commands,
            Some(color),
            RenderedPathFillRule::NonZero,
            None,
            PaintStrokeWidth::ZERO,
            None,
        ));
    }
}

/// Return whether `glyph_id` is an unadorned rectangle spanning one em in
/// both dimensions and one em of horizontal advance.
///
/// The check deliberately rejects curves, multiple contours, holes, and
/// partial-em rectangles.  Its only purpose is to recognize glyphs for which
/// an additional vector fill is semantically identical to normal text ink.
fn full_em_rectangle_outline(
    face: &ttf_parser::Face<'_>,
    glyph_id: ttf_parser::GlyphId,
    units_per_em: f32,
) -> bool {
    let Some(bounds) = face.glyph_bounding_box(glyph_id) else {
        return false;
    };
    let Some(advance) = face.glyph_hor_advance(glyph_id) else {
        return false;
    };
    if bounds.x_min != 0
        || bounds.x_max as f32 != units_per_em
        || (bounds.y_max - bounds.y_min) as f32 != units_per_em
        || advance as f32 != units_per_em
    {
        return false;
    }

    let mut probe = FullEmRectangleOutlineProbe::default();
    if face.outline_glyph(glyph_id, &mut probe).is_none() {
        return false;
    }
    probe.is_rectangle(bounds)
}

#[derive(Default)]
struct FullEmRectangleOutlineProbe {
    points: Vec<(i16, i16)>,
    close_count: usize,
    invalid: bool,
}

impl FullEmRectangleOutlineProbe {
    fn coordinate(value: f32) -> Option<i16> {
        (value.is_finite() && value.fract() == 0.0)
            .then_some(value as i16)
            .filter(|coordinate| *coordinate as f32 == value)
    }

    fn push_point(&mut self, x: f32, y: f32) {
        let Some(x) = Self::coordinate(x) else {
            self.invalid = true;
            return;
        };
        let Some(y) = Self::coordinate(y) else {
            self.invalid = true;
            return;
        };
        self.points.push((x, y));
    }

    fn is_rectangle(&self, bounds: ttf_parser::Rect) -> bool {
        if self.invalid || self.close_count != 1 {
            return false;
        }
        let points = match self.points.as_slice() {
            [a, b, c, d] => [*a, *b, *c, *d],
            [a, b, c, d, e] if a == e => [*a, *b, *c, *d],
            _ => return false,
        };
        let corners = [
            (bounds.x_min, bounds.y_min),
            (bounds.x_min, bounds.y_max),
            (bounds.x_max, bounds.y_min),
            (bounds.x_max, bounds.y_max),
        ];
        if !corners.iter().all(|corner| points.contains(corner)) {
            return false;
        }
        points
            .iter()
            .zip(points.iter().cycle().skip(1))
            .take(4)
            .all(|((x0, y0), (x1, y1))| (x0 == x1) != (y0 == y1))
    }
}

impl ttf_parser::OutlineBuilder for FullEmRectangleOutlineProbe {
    fn move_to(&mut self, x: f32, y: f32) {
        if !self.points.is_empty() {
            self.invalid = true;
        }
        self.push_point(x, y);
    }

    fn line_to(&mut self, x: f32, y: f32) {
        self.push_point(x, y);
    }

    fn quad_to(&mut self, _: f32, _: f32, _: f32, _: f32) {
        self.invalid = true;
    }

    fn curve_to(&mut self, _: f32, _: f32, _: f32, _: f32, _: f32, _: f32) {
        self.invalid = true;
    }

    fn close(&mut self) {
        self.close_count += 1;
    }
}

struct ColrPathPainter<'a> {
    face: &'a ttf_parser::Face<'a>,
    outline_to_paint: GlyphOutlineToPaint,
    paths: Vec<RenderedPath>,
    outlines: Vec<ttf_parser::GlyphId>,
}

impl ttf_parser::colr::Painter<'_> for ColrPathPainter<'_> {
    fn outline_glyph(&mut self, glyph_id: ttf_parser::GlyphId) {
        self.outlines.push(glyph_id);
    }

    fn paint(&mut self, paint: ttf_parser::colr::Paint<'_>) {
        let ttf_parser::colr::Paint::Solid(color) = paint else {
            return;
        };
        let color = CssColor::rgba(
            color.red,
            color.green,
            color.blue,
            color.alpha as f32 / 255.0,
        );
        for glyph_id in self.outlines.drain(..) {
            append_colr_outline(
                self.face,
                glyph_id,
                self.outline_to_paint,
                color,
                &mut self.paths,
            );
        }
    }

    fn push_clip(&mut self) {}
    fn push_clip_box(&mut self, _: ttf_parser::colr::ClipBox) {}
    fn pop_clip(&mut self) {}
    fn push_layer(&mut self, _: ttf_parser::colr::CompositeMode) {}
    fn pop_layer(&mut self) {}
    fn push_transform(&mut self, _: ttf_parser::Transform) {}
    fn pop_transform(&mut self) {}
}

struct GlyphPathBuilder {
    outline_to_paint: GlyphOutlineToPaint,
    current: Option<GlyphOutlinePoint>,
    commands: Vec<RenderedPathCommand>,
}

impl GlyphPathBuilder {
    fn new(outline_to_paint: GlyphOutlineToPaint) -> Self {
        Self {
            outline_to_paint,
            current: None,
            commands: Vec::new(),
        }
    }
}

impl ttf_parser::OutlineBuilder for GlyphPathBuilder {
    fn move_to(&mut self, x: f32, y: f32) {
        let point = GlyphOutlinePoint::new(x, y);
        self.commands.push(RenderedPathCommand::move_to(
            self.outline_to_paint.transform_point(point),
        ));
        self.current = Some(point);
    }

    fn line_to(&mut self, x: f32, y: f32) {
        let point = GlyphOutlinePoint::new(x, y);
        self.commands.push(RenderedPathCommand::line_to(
            self.outline_to_paint.transform_point(point),
        ));
        self.current = Some(point);
    }

    fn quad_to(&mut self, x1: f32, y1: f32, x: f32, y: f32) {
        let Some(start) = self.current else {
            return;
        };
        let control = GlyphOutlinePoint::new(x1, y1);
        let end = GlyphOutlinePoint::new(x, y);
        let control_1 = start + (control - start) * (2.0 / 3.0);
        let control_2 = end + (control - end) * (2.0 / 3.0);
        self.commands.push(RenderedPathCommand::curve_to(
            self.outline_to_paint.transform_point(control_1),
            self.outline_to_paint.transform_point(control_2),
            self.outline_to_paint.transform_point(end),
        ));
        self.current = Some(end);
    }

    fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
        let control_1 = GlyphOutlinePoint::new(x1, y1);
        let control_2 = GlyphOutlinePoint::new(x2, y2);
        let end = GlyphOutlinePoint::new(x, y);
        self.commands.push(RenderedPathCommand::curve_to(
            self.outline_to_paint.transform_point(control_1),
            self.outline_to_paint.transform_point(control_2),
            self.outline_to_paint.transform_point(end),
        ));
        self.current = Some(end);
    }

    fn close(&mut self) {
        self.commands.push(RenderedPathCommand::Close);
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
    use std::io::Cursor;

    #[test]
    fn emoji_presentation_participation_uses_unicode_variation_sequence_bases() {
        assert!(emoji_presentation_participating_code_point('#'));
        assert!(emoji_presentation_participating_code_point('*'));
        assert!(('0'..='9').all(emoji_presentation_participating_code_point));
        assert!(emoji_presentation_participating_code_point('©'));
        assert!(!emoji_presentation_participating_code_point('A'));
    }

    #[test]
    fn font_variant_emoji_inserts_selectors_for_keycap_bases() {
        let keycap = "1\u{20e3}";
        let mut style = ComputedStyle::initial();

        style.font_variant_emoji = FontVariantEmoji::Text;
        assert_eq!(
            text_with_font_variant_emoji(keycap, &style),
            "1\u{fe0e}\u{20e3}"
        );

        style.font_variant_emoji = FontVariantEmoji::Emoji;
        assert_eq!(
            text_with_font_variant_emoji(keycap, &style),
            "1\u{fe0f}\u{20e3}"
        );
    }

    #[test]
    fn font_variant_emoji_respects_authored_selectors_and_unchanged_values() {
        let keycap_with_text_selector = "1\u{fe0e}\u{20e3}";
        let keycap_with_emoji_selector = "1\u{fe0f}\u{20e3}";
        let mut style = ComputedStyle::initial();

        style.font_variant_emoji = FontVariantEmoji::Emoji;
        assert_eq!(
            text_with_font_variant_emoji(keycap_with_text_selector, &style),
            keycap_with_text_selector
        );
        style.font_variant_emoji = FontVariantEmoji::Text;
        assert_eq!(
            text_with_font_variant_emoji(keycap_with_emoji_selector, &style),
            keycap_with_emoji_selector
        );
        style.font_variant_emoji = FontVariantEmoji::Normal;
        assert_eq!(text_with_font_variant_emoji("1", &style), "1");
        style.font_variant_emoji = FontVariantEmoji::Unicode;
        assert_eq!(text_with_font_variant_emoji("1", &style), "1");
    }

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
            text_matrix: crate::RenderedTextMatrix::IDENTITY,
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

    #[test]
    fn glyph_outline_transform_preserves_source_to_paint_mapping() {
        let mut builder = GlyphPathBuilder::new(GlyphOutlineToPaint::new(2.0, 3.0, 10.0, 20.0));

        ttf_parser::OutlineBuilder::move_to(&mut builder, 1.0, 2.0);
        ttf_parser::OutlineBuilder::quad_to(&mut builder, 4.0, 5.0, 7.0, 8.0);

        assert_eq!(
            builder.commands,
            vec![
                RenderedPathCommand::move_to(PaintPoint::new(12.0, 26.0)),
                RenderedPathCommand::curve_to(
                    PaintPoint::new(16.0, 32.0),
                    PaintPoint::new(20.0, 38.0),
                    PaintPoint::new(24.0, 44.0),
                ),
            ]
        );
    }

    #[test]
    fn decodes_padded_monochrome_and_grayscale_bitmap_glyphs_as_alpha_masks() {
        let monochrome = decode_raster_glyph_image(ttf_parser::RasterGlyphImage {
            x: 0,
            y: 0,
            width: 3,
            height: 1,
            pixels_per_em: 16,
            format: ttf_parser::RasterImageFormat::BitmapMono,
            data: &[0b1010_0000],
        })
        .unwrap();
        assert_eq!(monochrome.rgb.as_ref(), &[0; 9]);
        assert_eq!(monochrome.alpha.as_deref().unwrap(), &[255, 0, 255]);

        let grayscale = decode_raster_glyph_image(ttf_parser::RasterGlyphImage {
            x: 0,
            y: 0,
            width: 2,
            height: 1,
            pixels_per_em: 16,
            format: ttf_parser::RasterImageFormat::BitmapGray2,
            data: &[0b0110_0000],
        })
        .unwrap();
        assert_eq!(grayscale.alpha.as_deref().unwrap(), &[85, 170]);
    }

    #[test]
    fn decodes_premultiplied_bgra_bitmap_glyphs() {
        let decoded = decode_raster_glyph_image(ttf_parser::RasterGlyphImage {
            x: 0,
            y: 0,
            width: 1,
            height: 1,
            pixels_per_em: 16,
            format: ttf_parser::RasterImageFormat::BitmapPremulBgra32,
            data: &[64, 32, 16, 128],
        })
        .unwrap();
        assert_eq!(decoded.rgb.as_ref(), &[32, 64, 128]);
        assert_eq!(decoded.alpha.as_deref().unwrap(), &[128]);
    }

    #[test]
    fn decodes_png_bitmap_glyphs() {
        let source = image::RgbaImage::from_raw(1, 1, vec![10, 20, 30, 128]).unwrap();
        let mut encoded = Cursor::new(Vec::new());
        image::DynamicImage::ImageRgba8(source)
            .write_to(&mut encoded, image::ImageFormat::Png)
            .unwrap();
        let decoded = decode_raster_glyph_image(ttf_parser::RasterGlyphImage {
            x: 0,
            y: 0,
            width: 1,
            height: 1,
            pixels_per_em: 16,
            format: ttf_parser::RasterImageFormat::PNG,
            data: encoded.get_ref(),
        })
        .unwrap();
        assert_eq!(decoded.rgb.as_ref(), &[10, 20, 30]);
        assert_eq!(decoded.alpha.as_deref().unwrap(), &[128]);
    }
}

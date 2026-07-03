use super::*;
use std::borrow::Cow;

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
    let mut output = String::new();
    for (offset, character) in slice.char_indices() {
        let index = range.start + offset;
        if !synthetic_ranges
            .iter()
            .any(|synthetic| synthetic.contains(&index))
        {
            output.push(character);
        }
    }
    output
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
    let mut output = Vec::with_capacity(glyphs.len());
    let mut glyphs = glyphs.into_iter();
    for (offset, character) in raw_text.char_indices() {
        let Some(mut glyph) = glyphs.next() else {
            break;
        };
        let index = run_start + offset;
        if synthetic_ranges
            .iter()
            .any(|synthetic| synthetic.contains(&index))
            || character_is_default_ignorable_code_point(character)
        {
            continue;
        } else {
            glyph.unicode = character.to_string();
            output.push(glyph);
        }
    }
    output.extend(glyphs);
    output
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

pub(in crate::text) fn emoji_presentation_participating_code_point(character: char) -> bool {
    matches!(
        character as u32,
        0x00a9
            | 0x00ae
            | 0x203c
            | 0x2049
            | 0x2122
            | 0x2139
            | 0x2194..=0x21aa
            | 0x231a..=0x231b
            | 0x2328
            | 0x23cf
            | 0x23e9..=0x23f3
            | 0x23f8..=0x23fa
            | 0x24c2
            | 0x25aa..=0x25ab
            | 0x25b6
            | 0x25c0
            | 0x25fb..=0x25fe
            | 0x2600..=0x27bf
            | 0x2934..=0x2935
            | 0x2b05..=0x2b55
            | 0x3030
            | 0x303d
            | 0x3297
            | 0x3299
            | 0x1f000..=0x1faff
    )
}

pub(in crate::text) fn apply_synthetic_position_fallback(
    glyphs: &mut [RenderedGlyph],
    font_size: &mut f32,
    style: &ComputedStyle,
    face: &ttf_parser::Face<'_>,
    text: &str,
) {
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
    let mut visible_glyphs = glyphs
        .iter()
        .filter(|glyph| !glyph.unicode.is_empty())
        .filter(|glyph| {
            glyph
                .unicode
                .chars()
                .any(|character| !character_is_default_ignorable_code_point(character))
        });
    text.chars()
        .filter(|character| !character_is_default_ignorable_code_point(*character))
        .zip(&mut visible_glyphs)
        .any(|(character, glyph)| {
            face.glyph_index(character)
                .is_some_and(|nominal| nominal.0 != glyph.id)
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

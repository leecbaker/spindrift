use super::super::*;
use std::borrow::Cow;

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

#[cfg(test)]
mod tests {
    use super::*;
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
}

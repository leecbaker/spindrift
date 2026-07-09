use super::system::span_boundary_needs_join_control;

#[test]
fn arabic_visual_ranges_are_emitted_in_reverse_cluster_order() {
    let mut style = ComputedStyle::initial();
    style.direction = Direction::Ltr;
    let mut system = FontSystem::new();
    let ranges = system.visual_ranges_for_unwrapped_text("السلامعليكم", &style);
    assert_eq!(
        ranges
            .iter()
            .map(|range| range.range.clone())
            .collect::<Vec<_>>(),
        vec![
            20..22,
            18..20,
            16..18,
            14..16,
            12..14,
            10..12,
            8..10,
            6..8,
            4..6,
            2..4,
            0..2
        ]
    );
    assert!(
        ranges
            .iter()
            .all(|range| range.direction == ResolvedBidiDirection::Rtl)
    );
}
use super::*;
use crate::css::{
    ComputedLengthPercentage, Css, FontFeatureSetting, FontFeatureSettings, FontSizeAdjust,
    FontSizeAdjustMetric, FontSizeAdjustValue, WhiteSpace, parse_stylesheet,
};
use crate::units::{LayoutLength, SemanticLengthExt, layout_pt};

async fn feature_probe_font_system() -> (FontSystem, ComputedStyle) {
    let stylesheet = parse_stylesheet(
        &Css::from_string(
            r#"@font-face {
                font-family: FeatureProbe;
                src: url("WeasyPrint/tests/resources/weasyprint.otf");
            }"#,
        )
        .with_base_path(".")
        .expect("current directory should be a valid file URL"),
    );
    let mut style = ComputedStyle::initial();
    style.font_family = FontFamily::Names(vec!["FeatureProbe".to_string()]);
    style.font_size = 16.0;
    style.line_height = 16.0;
    let system = FontSystem::start_loading()
        .load_stylesheet_fonts(&[stylesheet])
        .finish()
        .await;
    (system, style)
}

#[test]
fn used_line_height_preserves_layout_length_type() {
    let mut system = FontSystem::new();
    let mut style = ComputedStyle::initial();
    style.line_height_is_normal = false;
    style.line_height = 18.0;

    let line_height: LayoutLength = system.used_line_height(&style);

    assert_eq!(line_height, layout_pt(18.0));
}

#[test]
fn ic_advance_preserves_layout_length_type() {
    let mut system = FontSystem::new();
    let mut style = ComputedStyle::initial();
    style.font_size = 18.0;

    let ic_advance: LayoutLength = system.ic_advance_for_style(&style);

    assert_eq!(ic_advance, layout_pt(18.0));
}

#[test]
fn ch_advance_preserves_layout_length_type() {
    let mut system = FontSystem::new();
    let mut style = ComputedStyle::initial();
    style.font_size = 18.0;

    let ch_advance: LayoutLength = system.ch_advance(&style);

    assert!(ch_advance > layout_pt(0.0));
}

#[test]
fn installed_font_fallback_skips_private_use_characters() {
    let mut system = FontSystem::new();
    let style = ComputedStyle::initial();

    assert_eq!(
        system.resolve_system_fallback_for_character(
            '\u{e000}',
            style.font_weight,
            style.font_style,
            style.font_width,
        ),
        None,
        "CSS Fonts requires a missing-glyph representation instead of installed-font fallback"
    );
}

#[test]
fn platform_fallback_matches_direct_and_parley_selection() {
    let mut system = FontSystem::new();
    let mut style = ComputedStyle::initial();
    style.font_family = FontFamily::Names(vec!["Quire Test Missing Family".to_string()]);

    for character in ['A', '\u{0634}', '\u{1f600}'] {
        let direct = system
            .resolve_system_fallback_for_character(
                character,
                style.font_weight,
                style.font_style,
                style.font_width,
            )
            .unwrap_or_else(|| {
                panic!(
                    "platform fallback should provide a font for U+{:04X}",
                    character as u32
                )
            });
        assert_eq!(system.font_for_character(&style, character), Some(direct));

        let text = character.to_string();
        let runs = system.shape_text_runs_with_parley(&text, &style);
        assert_eq!(
            runs.len(),
            1,
            "expected one run for U+{:04X}",
            character as u32
        );
        assert_eq!(runs[0].font_id, Some(direct));
    }
}

fn shaped_glyph_ids(system: &mut FontSystem, text: &str, style: &ComputedStyle) -> Vec<u16> {
    system
        .shape_text_runs_with_parley(text, style)
        .into_iter()
        .flat_map(|run| run.glyphs)
        .map(|glyph| glyph.id)
        .collect()
}

#[tokio::test]
async fn font_neutral_cgj_does_not_change_visible_glyph_selection() {
    let (mut system, style) = feature_probe_font_system().await;

    let plain = shaped_glyph_ids(&mut system, "A", &style);
    let with_cgj = shaped_glyph_ids(&mut system, "A\u{034f}", &style);
    let styled_plain = system
        .shape_styled_text_runs_with_parley(&[StyledTextSpan {
            text: "A",
            style: &style,
        }])
        .into_iter()
        .flat_map(|run| run.glyphs)
        .map(|glyph| glyph.id)
        .collect::<Vec<_>>();
    let styled_with_cgj = system
        .shape_styled_text_runs_with_parley(&[StyledTextSpan {
            text: "A\u{034f}",
            style: &style,
        }])
        .into_iter()
        .flat_map(|run| run.glyphs)
        .map(|glyph| glyph.id)
        .collect::<Vec<_>>();

    // CGJ affects UAX #14 boundaries but is font-neutral: it must not make a
    // visible glyph fall back to a different face.
    // <https://www.w3.org/TR/css-text-3/#line-break-details>
    // <https://www.unicode.org/reports/tr44/#Default_Ignorable_Code_Point>
    assert_eq!(with_cgj, plain);
    assert_eq!(styled_with_cgj, styled_plain);
}

#[test]
fn visual_ordered_shape_keeps_neutral_punctuation_in_resolved_order() {
    let mut system = FontSystem::new();
    let mut style = ComputedStyle::initial();
    style.font_family = FontFamily::SansSerif;
    style.font_size = 12.0;
    style.line_height = 14.4;
    let text = ";א56;234;1";

    let shaped = system
        .shape_visual_ordered_line(text, &style, style.line_height, ResolvedBidiDirection::Ltr)
        .expect("visual text should shape");
    let visible_text = shaped
        .runs
        .iter()
        .flat_map(|run| &run.glyphs)
        .map(|glyph| glyph.source_text())
        .collect::<String>();

    assert_eq!(visible_text, text);
    assert!(
        visible_text
            .chars()
            .all(|character| !character_is_bidi_format_control(character))
    );
}

#[test]
fn rtl_visual_slice_mirrors_glyph_without_rewriting_source_text() {
    let mut system = FontSystem::new();
    let mut style = ComputedStyle::initial();
    style.font_family = FontFamily::SansSerif;
    style.font_size = 12.0;
    style.line_height = 14.4;

    let rtl = system
        .shape_visual_ordered_line(">", &style, style.line_height, ResolvedBidiDirection::Rtl)
        .expect("RTL visual slice should shape");
    let ltr_mirror = system
        .shape_visual_ordered_line("<", &style, style.line_height, ResolvedBidiDirection::Ltr)
        .expect("mirrored LTR punctuation should shape");
    let ltr = system
        .shape_visual_ordered_line(">", &style, style.line_height, ResolvedBidiDirection::Ltr)
        .expect("LTR visual slice should shape");
    let rtl_glyph = rtl
        .runs
        .iter()
        .flat_map(|run| run.glyphs.iter())
        .find(|glyph| glyph.source_text() == ">")
        .expect("RTL source punctuation should emit a glyph");
    let mirrored_glyph = ltr_mirror
        .runs
        .iter()
        .flat_map(|run| run.glyphs.iter())
        .find(|glyph| glyph.source_text() == "<")
        .expect("LTR mirrored punctuation should emit a glyph");
    let ltr_glyph = ltr
        .runs
        .iter()
        .flat_map(|run| run.glyphs.iter())
        .find(|glyph| glyph.source_text() == ">")
        .expect("LTR source punctuation should emit a glyph");

    assert_eq!(rtl_glyph.rendered.id, mirrored_glyph.rendered.id);
    assert_ne!(rtl_glyph.rendered.id, ltr_glyph.rendered.id);
    assert_eq!(rtl_glyph.source_text(), ">");
}

#[test]
fn rtl_base_direction_marks_neutral_punctuation_as_an_rtl_visual_slice() {
    let mut system = FontSystem::new();
    let mut style = ComputedStyle::initial();
    style.direction = Direction::Rtl;
    let text = "> a > ב > c >";

    let ranges = system.visual_ranges_for_unwrapped_text(text, &style);

    assert!(ranges.iter().any(|visual_range| {
        text.get(visual_range.range.clone()) == Some(">")
            && visual_range.direction == ResolvedBidiDirection::Rtl
    }));
}

#[tokio::test]
async fn parley_font_run_mapping_reuses_document_font_id() {
    let mut system = FontSystem::new();
    let mut style = ComputedStyle::initial();
    style.font_family = FontFamily::SansSerif;
    style.font_size = 12.0;
    style.line_height = 14.4;

    let first_font_id = system
        .shape_text_runs_with_parley("Repeated", &style)
        .into_iter()
        .find_map(|run| run.font_id)
        .unwrap();
    let font_count = system.document_fonts.fonts.len();
    let second_font_id = system
        .shape_text_runs_with_parley("Repeated", &style)
        .into_iter()
        .find_map(|run| run.font_id)
        .unwrap();

    assert_eq!(first_font_id, second_font_id);
    assert_eq!(system.document_fonts.fonts.len(), font_count);
}

#[test]
fn reusable_parley_layout_does_not_invalidate_prior_shaping() {
    let mut system = FontSystem::new();
    let mut style = ComputedStyle::initial();
    style.font_family = FontFamily::SansSerif;
    style.font_size = 12.0;
    style.line_height = 14.4;

    let first = system.shape_text_runs_with_parley("first shaped run", &style);
    assert!(!first.is_empty());
    let first_snapshot = first.clone();

    let styled = system.shape_styled_text_runs_with_parley(&[
        StyledTextSpan {
            text: "styled ",
            style: &style,
        },
        StyledTextSpan {
            text: "shaping",
            style: &style,
        },
    ]);
    assert!(!styled.is_empty());

    let visual_ranges = system.visual_ranges_for_unwrapped_text("abc אבג", &style);
    assert!(!visual_ranges.is_empty());
    let later = system.shape_text_runs_with_parley("later shaped run", &style);

    assert!(!later.is_empty());
    assert_eq!(first, first_snapshot);
}

#[tokio::test]
async fn opentype_font_feature_controls_affect_shaping() {
    let (mut system, style) = feature_probe_font_system().await;

    let kerned = system.measure_text("kk", &style);
    let mut no_kern = style.clone();
    no_kern.font_kerning = FontKerning::None;
    assert!(system.measure_text("kk", &no_kern) > kerned);

    let common_ligature = system.measure_text("liga", &style);
    let mut no_common_ligature = style.clone();
    no_common_ligature.font_variant_ligatures = FontVariantLigatures::Values {
        common: Some(false),
        discretionary: None,
        historical: None,
        contextual: None,
    };
    assert!(system.measure_text("liga", &no_common_ligature) > common_ligature);

    let mut subscript = style.clone();
    subscript.font_variant_position = FontVariantPosition::Sub;
    assert!(system.measure_text("subs", &subscript) < system.measure_text("subs", &style));

    let mut discretionary = style.clone();
    discretionary.font_variant_ligatures = FontVariantLigatures::Values {
        common: None,
        discretionary: Some(true),
        historical: None,
        contextual: None,
    };
    assert!(system.measure_text("dlig", &discretionary) < system.measure_text("dlig", &style));

    let mut numeric = style.clone();
    numeric.font_variant_numeric = FontVariantNumeric::Values(vec![
        FontVariantNumericValue::OldstyleNums,
        FontVariantNumericValue::SlashedZero,
    ]);
    assert!(system.measure_text("onum", &numeric) < system.measure_text("onum", &style));
    assert!(system.measure_text("zero", &numeric) < system.measure_text("zero", &style));

    let normal_caps = shaped_glyph_ids(&mut system, "Pp", &style);
    let mut small_caps = style.clone();
    small_caps.font_variant_caps = FontVariantCaps::SmallCaps;
    let small_caps_glyphs = shaped_glyph_ids(&mut system, "Pp", &small_caps);
    assert_ne!(small_caps_glyphs, normal_caps);
}

#[tokio::test]
async fn low_level_font_feature_settings_override_variant_features() {
    let (mut system, mut style) = feature_probe_font_system().await;
    style.font_variant_ligatures = FontVariantLigatures::Values {
        common: None,
        discretionary: Some(false),
        historical: None,
        contextual: None,
    };
    let disabled = system.measure_text("dlig", &style);

    style.font_feature_settings = FontFeatureSettings(vec![FontFeatureSetting::new(*b"dlig", 1)]);
    let enabled_by_low_level_setting = system.measure_text("dlig", &style);

    assert!(enabled_by_low_level_setting < disabled);
}

#[tokio::test]
async fn font_face_feature_descriptors_participate_in_shaping_precedence() {
    let stylesheet = parse_stylesheet(
        &Css::from_string(
            r#"@font-face {
                font-family: FeatureFaceDefaults;
                src: url("WeasyPrint/tests/resources/weasyprint.otf");
                font-variant-ligatures: discretionary-ligatures;
            }"#,
        )
        .with_base_path(".")
        .expect("current directory should be a valid file URL"),
    );
    let mut style = ComputedStyle::initial();
    style.font_family = FontFamily::Names(vec!["FeatureFaceDefaults".to_string()]);
    style.font_size = 16.0;
    style.line_height = 16.0;
    let mut system = FontSystem::start_loading()
        .load_stylesheet_fonts(&[stylesheet])
        .finish()
        .await;

    let enabled_by_face_default = system.measure_text("dlig", &style);
    let mut disabled_by_element = style.clone();
    disabled_by_element.font_variant_ligatures = FontVariantLigatures::Values {
        common: None,
        discretionary: Some(false),
        historical: None,
        contextual: None,
    };
    let disabled = system.measure_text("dlig", &disabled_by_element);
    assert!(enabled_by_face_default < disabled);

    disabled_by_element.font_feature_settings =
        FontFeatureSettings(vec![FontFeatureSetting::new(*b"dlig", 1)]);
    let reenabled_by_low_level = system.measure_text("dlig", &disabled_by_element);
    assert!(reenabled_by_low_level < disabled);
}

#[tokio::test]
async fn font_variant_emoji_selectors_do_not_leak_to_emitted_text() {
    let mut system = FontSystem::new();
    let mut style = ComputedStyle::initial();
    style.font_variant_emoji = FontVariantEmoji::Emoji;

    let emitted = system
        .shape_text_runs_with_parley("©", &style)
        .into_iter()
        .map(|run| run.text.to_string())
        .collect::<String>();

    assert_eq!(emitted, "©");
    assert!(!emitted.contains('\u{fe0f}'));
}

#[tokio::test]
async fn numeric_font_size_adjust_changes_used_shaping_size() {
    let (mut system, style) = feature_probe_font_system().await;
    let normal_width = system.measure_text("xxxx", &style);
    let mut adjusted = style.clone();
    adjusted.font_size_adjust = FontSizeAdjust::Value {
        metric: FontSizeAdjustMetric::ExHeight,
        value: FontSizeAdjustValue::Number(0.9),
    };

    let adjusted_width = system.measure_text("xxxx", &adjusted);
    let adjusted_run = system
        .shape_text_runs_with_parley("xxxx", &adjusted)
        .into_iter()
        .next()
        .expect("adjusted run");

    assert!((adjusted_width - normal_width).abs() > 0.1);
    assert!((adjusted_run.font_size - style.font_size).abs() > 0.1);
}

#[tokio::test]
async fn font_size_adjust_from_font_uses_first_available_font_ratio() {
    let (mut system, style) = feature_probe_font_system().await;
    let normal_width = system.measure_text("xxxx", &style);
    let mut adjusted = style.clone();
    adjusted.font_size_adjust = FontSizeAdjust::Value {
        metric: FontSizeAdjustMetric::ExHeight,
        value: FontSizeAdjustValue::FromFont,
    };

    let adjusted_width = system.measure_text("xxxx", &adjusted);
    let adjusted_run = system
        .shape_text_runs_with_parley("xxxx", &adjusted)
        .into_iter()
        .next()
        .expect("adjusted run");

    assert!((adjusted_width - normal_width).abs() < 0.01);
    assert!((adjusted_run.font_size - style.font_size).abs() < 0.01);
}

#[tokio::test]
async fn font_size_adjust_keeps_explicit_line_height_computed_size() {
    let (mut system, mut style) = feature_probe_font_system().await;
    style.line_height = 20.0;
    style.line_height_is_normal = false;
    style.font_size_adjust = FontSizeAdjust::Value {
        metric: FontSizeAdjustMetric::ExHeight,
        value: FontSizeAdjustValue::Number(0.9),
    };

    let line = system
        .shape_unwrapped_line("xxxx", &style, style.line_height)
        .expect("shaped line");

    assert_eq!(line.line_height, 20.0);
}

#[tokio::test]
async fn same_face_bold_request_gets_synthesized_document_font_label() {
    let mut system = FontSystem::new();
    let mut normal = ComputedStyle::initial();
    normal.font_family = FontFamily::SansSerif;
    let mut bold = normal.clone();
    bold.font_weight = FontWeight::BOLD;
    let fonts = system.query_fonts(
        &[FontiqueQueryFamily::Generic(
            FontiqueGenericFamily::SansSerif,
        )],
        normal.font_weight,
        normal.font_style,
        normal.font_width,
    );
    let font = fonts
        .into_iter()
        .find(|font| !font.synthesis.any())
        .expect("system sans-serif font");
    let family_label = "SyntheticLabelProbe";

    let normal_id = system
        .document_font_from_query_font(
            font.clone(),
            Some(family_label),
            &FontRequest::single_name(
                family_label,
                normal.font_weight,
                normal.font_style,
                normal.font_width,
            ),
        )
        .unwrap();
    let bold_id = system
        .document_font_from_query_font(
            font,
            Some(family_label),
            &FontRequest::single_name(
                family_label,
                bold.font_weight,
                bold.font_style,
                bold.font_width,
            ),
        )
        .unwrap();

    let normal_font = system.document_fonts.fonts.get(normal_id).unwrap();
    let bold_font = system.document_fonts.fonts.get(bold_id).unwrap();
    assert_eq!(normal_font.family, family_label);
    assert_eq!(bold_font.family, family_label);
    assert_ne!(normal_id, bold_id);
    assert_ne!(normal_font.post_script_name, bold_font.post_script_name);
    assert!(bold_font.post_script_name.contains("Bold"));
}

#[tokio::test]
async fn inter_word_justification_mutates_shaped_separator_advances() {
    let mut system = FontSystem::new();
    let mut style = ComputedStyle::initial();
    style.font_family = FontFamily::SansSerif;
    style.font_size = 12.0;
    style.line_height = 14.4;

    let mut shaped = system
        .shape_unwrapped_line("one two", &style, style.line_height)
        .expect("line should shape");
    let original_width = shaped.advance_width();
    let original_run_count = shaped.runs.len();
    let original_font_ids = shaped
        .runs
        .iter()
        .map(|run| run.font_id)
        .collect::<Vec<_>>();
    let original_glyph_ids = shaped
        .runs
        .iter()
        .flat_map(|run| &run.glyphs)
        .map(|glyph| glyph.rendered.id)
        .collect::<Vec<_>>();
    let original_space_advance = shaped
        .runs
        .iter()
        .flat_map(|run| &run.glyphs)
        .find(|glyph| glyph.source_text() == " ")
        .map(|glyph| glyph.rendered.x_advance)
        .expect("space glyph");

    let added_width = shaped.apply_inter_word_justification(10.0, 1);

    let justified_space_advance = shaped
        .runs
        .iter()
        .flat_map(|run| &run.glyphs)
        .find(|glyph| glyph.source_text() == " ")
        .map(|glyph| glyph.rendered.x_advance)
        .expect("space glyph");
    assert!((added_width - 10.0).abs() < 0.01);
    assert!((shaped.advance_width() - original_width - 10.0).abs() < 0.01);
    assert!((justified_space_advance - original_space_advance - 10.0).abs() < 0.01);
    assert_eq!(shaped.runs.len(), original_run_count);
    assert_eq!(
        shaped
            .runs
            .iter()
            .map(|run| run.font_id)
            .collect::<Vec<_>>(),
        original_font_ids
    );
    assert_eq!(
        shaped
            .runs
            .iter()
            .flat_map(|run| &run.glyphs)
            .map(|glyph| glyph.rendered.id)
            .collect::<Vec<_>>(),
        original_glyph_ids
    );
}

#[tokio::test]
async fn inter_word_justification_preserves_bidi_visual_glyph_order() {
    let mut system = FontSystem::new();
    let mut style = ComputedStyle::initial();
    style.font_family = FontFamily::SansSerif;
    style.font_size = 12.0;
    style.line_height = 14.4;

    let mut shaped = system
        .shape_unwrapped_line("abc אבג def", &style, style.line_height)
        .expect("bidi line should shape");
    let original_sources = shaped
        .runs
        .iter()
        .flat_map(|run| &run.glyphs)
        .map(|glyph| glyph.source_text().to_string())
        .collect::<Vec<_>>();
    let original_run_font_ids = shaped
        .runs
        .iter()
        .map(|run| run.font_id)
        .collect::<Vec<_>>();

    let added_width = shaped.apply_inter_word_justification(6.0, 2);

    assert!((added_width - 12.0).abs() < 0.01);
    assert_eq!(
        shaped
            .runs
            .iter()
            .flat_map(|run| &run.glyphs)
            .map(|glyph| glyph.source_text().to_string())
            .collect::<Vec<_>>(),
        original_sources
    );
    assert_eq!(
        shaped
            .runs
            .iter()
            .map(|run| run.font_id)
            .collect::<Vec<_>>(),
        original_run_font_ids
    );
}

#[tokio::test]
async fn shape_measured_line_excludes_hanging_glyphs_from_css_measure() {
    let mut system = FontSystem::new();
    let mut style = ComputedStyle::initial();
    style.font_family = FontFamily::SansSerif;
    style.font_size = 12.0;
    style.line_height = 14.4;
    style.white_space = crate::css::WhiteSpace::Normal;

    let shaped = system
        .shape_measured_line("X\u{3000}", &style, style.line_height)
        .expect("line should shape");
    let visible_width = system.measure_text("X", &style);

    assert_eq!(shaped.text.as_ref(), "X\u{3000}");
    assert!(
        (shaped.width - visible_width).abs() < 0.01,
        "CSS line measure should exclude hanging ideographic space"
    );
    assert!(
        shaped.advance_width() > shaped.width,
        "shaped payload should keep the hanging glyph for painting"
    );
}

#[tokio::test]
async fn letter_spacing_uses_unicode_joining_properties() {
    assert_eq!(used_letter_spacing_for_text("abc", 10.0), 10.0);
    assert_eq!(used_letter_spacing_for_text("تفاحة", 10.0), 0.0);
    assert!(character_has_joining_behavior('ت'));
    assert!(!character_has_joining_behavior('a'));
    assert!(character_can_join_following('ب'));
    assert!(character_can_join_preceding('ا'));
    assert!(!character_can_join_following('ا'));
    assert!(character_is_arabic_tatweel('\u{0640}'));
    assert!(character_has_joining_behavior('\u{0640}'));
    assert!(character_is_join_control('\u{200c}'));
    assert!(character_is_join_control('\u{200d}'));
    assert!(!character_is_join_control('\u{200e}'));
}

#[tokio::test]
async fn line_measure_excludes_inline_end_letter_spacing() {
    let mut system = FontSystem::new();
    let mut style = ComputedStyle::initial();
    style.font_family = FontFamily::Monospace;
    style.font_size = 12.0;
    style.letter_spacing = ComputedLengthPercentage::from_points(10.0);

    let mut untracked = style.clone();
    untracked.letter_spacing = ComputedLengthPercentage::ZERO;
    let expected_line_width =
        system.measure_text("aa", &untracked) + style.used_letter_spacing().points();

    assert!(
        (line_end_letter_spacing_width("aa", &style).points()
            - style.used_letter_spacing().points())
        .abs()
            < 0.01
    );
    assert!((system.measure_line_text("aa", &style) - expected_line_width).abs() < 0.01);
    assert!(
        (system.measure_line_text("a", &style) - system.measure_text("a", &untracked)).abs() < 0.01
    );
}

#[test]
fn generic_parley_source_uses_quire_embeddable_face() {
    let mut system = FontSystem::new();
    let mut style = ComputedStyle::initial();
    style.font_family = FontFamily::Monospace;

    let resolved_font_id = system
        .resolve_style(&style)
        .expect("an embeddable system monospace font");
    let resolved_font = system
        .document_fonts
        .get(resolved_font_id)
        .expect("resolved document font");
    assert!(
        ttf_parser::Face::parse(&resolved_font.data, resolved_font.face_index)
            .is_ok_and(|face| face.is_outline_embedding_allowed()),
        "generic selection must not choose a font whose outlines cannot be embedded"
    );

    let expected_source =
        parley_font_family_source(&FontFamily::Names(vec![resolved_font.family.clone()]));
    assert_eq!(
        system.resolved_parley_font_family_source(&style),
        expected_source
    );

    let runs = system.shape_text_runs_with_parley("monospace", &style);
    assert!(runs.iter().all(|run| run.font_id == Some(resolved_font_id)));
}

#[test]
fn quoted_generic_name_stays_named_in_parley_source() {
    let mut system = FontSystem::new();
    let mut style = ComputedStyle::initial();
    style.font_family = FontFamily::List(vec![
        FontFamily::Names(vec!["fantasy".to_string()]),
        FontFamily::Monospace,
    ]);

    let source = system.resolved_parley_font_family_source(&style);
    assert!(source.starts_with("\"fantasy\", "));
    assert!(!source.ends_with("monospace"));
}

#[tokio::test]
async fn join_controls_stay_in_adjacent_font_runs() {
    let mut system = FontSystem::new();
    let mut style = ComputedStyle::initial();
    style.font_family = FontFamily::SansSerif;
    style.font_size = 12.0;

    let runs = system.shape_text_runs_with_parley("A\u{200c}B", &style);

    assert_eq!(runs.len(), 1);
    assert_eq!(runs[0].text.as_ref(), "A\u{200c}B");
}

#[tokio::test]
async fn join_controls_shape_but_do_not_emit_visible_glyphs() {
    let mut system = FontSystem::new();
    let mut style = ComputedStyle::initial();
    style.font_family = FontFamily::SansSerif;
    style.font_size = 12.0;

    let runs = system.shape_text_runs_with_parley("A\u{200c}B", &style);

    assert_eq!(runs.len(), 1);
    assert_eq!(runs[0].text.as_ref(), "A\u{200c}B");
    let glyphs = &runs[0].glyphs;
    assert!(
        glyphs
            .iter()
            .all(|glyph| !glyph.unicode.chars().any(character_is_join_control)),
        "{glyphs:?}"
    );
}

#[tokio::test]
async fn zwj_wrapped_arabic_letter_shapes_with_wpt_font() {
    let stylesheet = parse_stylesheet(
        &Css::from_string(
            r#"@font-face {
                font-family: AlreqNaskh;
                src: url("tests/resources/fonts/NotoNaskhArabic-regular.woff2");
            }"#,
        )
        .with_base_path(".")
        .expect("current directory should be a valid file URL"),
    );
    let mut style = ComputedStyle::initial();
    style.font_family = FontFamily::Names(vec!["AlreqNaskh".to_string()]);
    style.font_size = 20.0;
    style.line_height = 24.0;
    style.direction = Direction::Rtl;
    let mut system = FontSystem::start_loading()
        .load_stylesheet_fonts(&[stylesheet])
        .finish()
        .await;

    assert!(
        system
            .shape_unwrapped_line("\u{0627}", &style, style.line_height)
            .is_some(),
        "plain alef should shape with the WPT font"
    );
    let shaped = system
        .shape_unwrapped_line("\u{200d}\u{0627}\u{200d}", &style, style.line_height)
        .expect("ZWJ-wrapped alef should shape");

    assert!(shaped.advance_width() > 0.0, "{shaped:?}");
    assert!(
        shaped
            .runs
            .iter()
            .flat_map(|run| run.glyphs.iter())
            .all(|glyph| !glyph
                .rendered
                .unicode
                .chars()
                .any(character_is_join_control)),
        "{shaped:?}"
    );
}

#[tokio::test]
async fn failed_font_face_source_does_not_discard_a_sibling_face() {
    let stylesheet = parse_stylesheet(
        &Css::from_string(
            r#"@font-face {
                font-family: MissingFace;
                src: url("tests/resources/fonts/does-not-exist.woff2");
            }
            @font-face {
                font-family: RecoverableFace;
                src: url("tests/resources/fonts/NotoNaskhArabic-regular.woff2");
            }"#,
        )
        .with_base_path(".")
        .expect("current directory should be a valid file URL"),
    );
    let mut style = ComputedStyle::initial();
    style.font_family = FontFamily::Names(vec!["RecoverableFace".to_string()]);
    style.font_size = 20.0;
    style.line_height = 24.0;
    style.direction = Direction::Rtl;
    let mut system = FontSystem::start_loading()
        .load_stylesheet_fonts(&[stylesheet])
        .finish()
        .await;

    assert!(
        system
            .shape_unwrapped_line("\u{0627}", &style, style.line_height)
            .is_some()
    );
    assert!(
        system
            .document_fonts
            .fonts
            .iter()
            .any(|font| font.family == "RecoverableFace")
    );
}

#[tokio::test]
async fn shared_font_program_keeps_each_css_face_metadata() {
    let stylesheet = parse_stylesheet(
        &Css::from_string(
            r#"@font-face {
                font-family: SharedFaceA;
                src: url("tests/resources/fonts/NotoNaskhArabic-regular.woff2");
                size-adjust: 50%;
                unicode-range: U+0627;
            }
            @font-face {
                font-family: SharedFaceA;
                src: url("./tests/resources/fonts/NotoNaskhArabic-regular.woff2");
                font-style: oblique;
                size-adjust: 75%;
                unicode-range: U+0628;
            }"#,
        )
        .with_base_path(".")
        .expect("current directory should be a valid file URL"),
    );
    let mut system = FontSystem::start_loading()
        .load_stylesheet_fonts(&[stylesheet])
        .finish()
        .await;
    let style = ComputedStyle::initial();
    let face_a = system
        .resolve_font_family(
            &FontFamily::Names(vec!["SharedFaceA".to_string()]),
            style.font_weight,
            style.font_style,
            style.font_width,
        )
        .expect("first shared face resolves");
    let mut oblique = style;
    oblique.font_style = FontStyle::Oblique;
    let face_b = system
        .resolve_font_family(
            &FontFamily::Names(vec!["SharedFaceA".to_string()]),
            oblique.font_weight,
            oblique.font_style,
            oblique.font_width,
        )
        .expect("second shared face resolves");

    assert_eq!(
        system.document_fonts.get(face_a).unwrap().data.blob_id(),
        system.document_fonts.get(face_b).unwrap().data.blob_id(),
    );
    assert_eq!(system.document_fonts.font_size_adjust(face_a), Some(0.5));
    assert_eq!(system.document_fonts.font_size_adjust(face_b), Some(0.75));
    assert!(system.document_fonts.font_has_character(face_a, '\u{0627}'));
    assert!(!system.document_fonts.font_has_character(face_a, '\u{0628}'));
    assert!(system.document_fonts.font_has_character(face_b, '\u{0628}'));
    assert!(!system.document_fonts.font_has_character(face_b, '\u{0627}'));
}

#[tokio::test]
async fn styled_tatweel_fragment_shapes_adjacent_arabic_letter() {
    let stylesheet = parse_stylesheet(
        &Css::from_string(
            r#"@font-face {
                font-family: AlreqNaskh;
                src: url("tests/resources/fonts/NotoNaskhArabic-regular.woff2");
            }
            @font-face {
                font-family: AlreqTatweel;
                src: url("tests/resources/fonts/Scheherazade-Regular.woff");
            }"#,
        )
        .with_base_path(".")
        .expect("current directory should be a valid file URL"),
    );
    let mut arabic = ComputedStyle::initial();
    arabic.font_family = FontFamily::Names(vec!["AlreqNaskh".to_string()]);
    arabic.font_size = 20.0;
    arabic.line_height = 24.0;
    arabic.direction = Direction::Rtl;
    let mut tatweel = arabic.clone();
    tatweel.font_family = FontFamily::Names(vec!["AlreqTatweel".to_string()]);
    let mut system = FontSystem::start_loading()
        .load_stylesheet_fonts(&[stylesheet])
        .finish()
        .await;

    let isolated_beh = shaped_glyph_ids(&mut system, "\u{0628}", &arabic);
    let shaped = system.shape_styled_text_runs_with_parley(&[
        StyledTextSpan {
            text: "\u{0628}",
            style: &arabic,
        },
        StyledTextSpan {
            text: "\u{0640}",
            style: &tatweel,
        },
    ]);
    let glyph_ids = shaped
        .into_iter()
        .flat_map(|run| run.glyphs)
        .filter(|glyph| glyph.x_advance != 0.0)
        .map(|glyph| glyph.id)
        .collect::<Vec<_>>();

    assert_ne!(glyph_ids.first(), isolated_beh.first(), "{glyph_ids:?}");
}

#[tokio::test]
async fn explicit_join_control_suppresses_synthetic_boundary_joiner() {
    assert!(!span_boundary_needs_join_control("ـ\u{200c}", "ا"));
    assert!(!span_boundary_needs_join_control("ـ", "\u{200c}ا"));
    assert!(span_boundary_needs_join_control("ـ", "ا"));
    assert!(span_boundary_needs_join_control("ب", "ا"));
    assert!(!span_boundary_needs_join_control("ا", "ب"));
}

#[tokio::test]
async fn other_space_separators_emit_blank_glyphs_with_original_unicode() {
    let mut system = FontSystem::new();
    let mut style = ComputedStyle::initial();
    style.font_family = FontFamily::SansSerif;
    style.font_size = 12.0;

    let space_glyph = system
        .shape_text_runs_with_parley(" ", &style)
        .into_iter()
        .flat_map(|run| run.glyphs)
        .next()
        .unwrap();
    let en_space_glyph = system
        .shape_text_runs_with_parley("\u{2002}", &style)
        .into_iter()
        .flat_map(|run| run.glyphs)
        .next()
        .unwrap();

    assert_eq!(en_space_glyph.id, space_glyph.id);
    assert_eq!(en_space_glyph.unicode, "\u{2002}");
    assert!(en_space_glyph.x_advance > 0.0);
}

#[tokio::test]
async fn unicode_properties_cover_bidi_and_line_break_classes() {
    assert!(contains_bidi_text("abc אבג"));
    assert_eq!(
        plaintext_direction_for_text("\u{200f}TIN"),
        Some(Direction::Rtl)
    );
    assert_eq!(line_break_class('\u{298d}'), LineBreak::OpenPunctuation);
    assert_eq!(line_break_class('\u{207e}'), LineBreak::ClosePunctuation);
    assert_eq!(line_break_class('あ'), LineBreak::Ideographic);
    assert!(character_is_bidi_format_control('\u{2067}'));
    assert!(!character_is_bidi_format_control('א'));
}

#[tokio::test]
async fn unicode_general_categories_back_text_classification_helpers() {
    assert!(character_is_unicode_letter('A'));
    assert!(character_is_unicode_letter('é'));
    assert!(!character_is_unicode_letter('1'));
    assert!(character_is_unicode_alphanumeric('A'));
    assert!(character_is_unicode_alphanumeric('१'));
    assert!(!character_is_unicode_alphanumeric('\u{3000}'));
    assert!(character_is_text_decoration_spacer('\u{3000}'));
    assert!(!character_is_text_decoration_spacer('\u{202f}'));
    assert!(character_receives_text_emphasis_mark('字'));
    assert!(!character_receives_text_emphasis_mark('。'));
    assert!(character_is_last_hangable_punctuation('」'));
    assert!(character_is_last_hangable_punctuation('"'));
    assert!(!character_is_last_hangable_punctuation('A'));
    assert!(character_is_first_hangable_punctuation('「'));
    assert!(character_is_first_hangable_punctuation('\u{3000}'));
    assert!(!character_is_first_hangable_punctuation('A'));
    assert!(character_is_hangable_stop_or_comma('。'));
    assert!(character_is_hangable_stop_or_comma('\u{060c}'));
    assert!(!character_is_hangable_stop_or_comma(';'));
    assert!(character_is_unicode_control('\u{0007}'));
    assert!(!character_is_unicode_control('\u{200d}'));
    assert!(character_is_default_ignorable_code_point('\u{200d}'));
    assert!(character_is_default_ignorable_code_point('\u{034f}'));
    assert!(character_is_font_neutral_default_ignorable('\u{034f}'));
    assert!(!contains_bidi_text("A\u{034f}"));
    assert!(character_is_default_ignorable_code_point('\u{fe0f}'));
    assert!(!character_is_default_ignorable_code_point('字'));
    assert!(character_preserves_word_boundary_context('’'));
    assert!(!character_preserves_word_boundary_context(' '));
}

#[tokio::test]
async fn css_whitespace_collapse_preserves_other_space_separators() {
    assert_eq!(
        collapse_css_collapsible_whitespace("  A\t\u{3000}\n B  "),
        "A \u{3000} B"
    );
}

#[tokio::test]
async fn atomic_inline_boundaries_follow_css_text_line_breaking() {
    let style = ComputedStyle::initial();
    let object = OBJECT_REPLACEMENT_CHARACTER.to_string();

    assert!(inline_atomic_boundary_allows_soft_wrap(
        "A", &object, &style
    ));
    assert!(inline_atomic_boundary_allows_soft_wrap(
        "A ", &object, &style
    ));
    assert!(!inline_atomic_boundary_allows_soft_wrap(
        &object, ",", &style
    ));
    assert!(inline_atomic_boundary_allows_soft_wrap(
        ", ", &object, &style
    ));
    assert!(inline_atomic_boundary_allows_soft_wrap(
        &object, "A", &style
    ));
    assert!(inline_atomic_boundary_allows_soft_wrap(
        "A)", &object, &style
    ));
    assert!(!inline_atomic_boundary_allows_soft_wrap(
        "A\u{034f}",
        &object,
        &style
    ));
    assert!(!inline_atomic_boundary_allows_soft_wrap(
        "A\u{180e}",
        &object,
        &style
    ));
    assert!(!inline_atomic_boundary_allows_soft_wrap(
        "A\u{2007}",
        &object,
        &style
    ));
    assert!(inline_atomic_boundary_allows_soft_wrap(
        "A\u{00a0}",
        &object,
        &style
    ));
    assert!(!inline_atomic_boundary_allows_soft_wrap(
        "A\u{202f}",
        &object,
        &style
    ));
    assert!(!inline_atomic_boundary_allows_soft_wrap(
        &object,
        "\u{034f}B",
        &style
    ));
    assert!(!inline_atomic_boundary_allows_soft_wrap(
        &object,
        "\u{180e}B",
        &style
    ));
    assert!(!inline_atomic_boundary_allows_soft_wrap(
        &object,
        "\u{2007}B",
        &style
    ));
}

#[tokio::test]
async fn measured_line_breaks_do_not_leave_opening_punctuation_at_line_end() {
    let mut style = ComputedStyle::initial();
    style.font_family = FontFamily::SansSerif;
    let text = "中中\u{298d}文";
    let opening_punctuation = text.find('\u{298d}').unwrap();
    let after_opening_punctuation = opening_punctuation
        + text[opening_punctuation..]
            .chars()
            .next()
            .unwrap()
            .len_utf8();
    let breaks = measured_break_opportunities(text, &style);

    assert!(breaks.contains(&opening_punctuation));
    assert!(!breaks.contains(&after_opening_punctuation));
}

#[tokio::test]
async fn measured_line_breaks_do_not_leave_closing_punctuation_at_line_start() {
    let mut style = ComputedStyle::initial();
    style.font_family = FontFamily::SansSerif;
    let text = "中中中\u{207e}文";
    let closing_punctuation = text.find('\u{207e}').unwrap();
    let breaks = measured_break_opportunities(text, &style);

    assert!(!breaks.contains(&closing_punctuation));
}

#[tokio::test]
async fn measured_line_breaks_keep_ascii_closing_brace_with_previous_character() {
    let mut style = ComputedStyle::initial();
    style.font_family = FontFamily::SansSerif;
    let text = "中中中}文";
    let third_ideograph = text.match_indices('中').nth(2).unwrap().0;
    let after_second_ideograph = third_ideograph;
    let closing_punctuation = text.find('}').unwrap();
    let breaks = measured_break_opportunities(text, &style);

    assert!(breaks.contains(&after_second_ideograph));
    assert!(!breaks.contains(&closing_punctuation));
}

#[tokio::test]
async fn measured_keep_all_suppresses_alnum_and_ideograph_unit_breaks() {
    let mut style = ComputedStyle::initial();
    style.font_family = FontFamily::SansSerif;
    style.word_break = CssWordBreak::KeepAll;
    let text = "中文english中文，english 中文";
    let punctuation = text.find('，').unwrap();
    let after_punctuation = punctuation + '，'.len_utf8();
    let space = text.find(' ').unwrap();
    let after_space = space + ' '.len_utf8();
    let breaks = measured_break_opportunities(text, &style);

    assert!(breaks.contains(&after_punctuation), "{breaks:?}");
    assert!(breaks.contains(&after_space), "{breaks:?}");
    assert!(!breaks.contains(&"中".len()), "{breaks:?}");
    assert!(!breaks.contains(&"中文".len()), "{breaks:?}");
    assert!(!breaks.contains(&"中文english".len()), "{breaks:?}");
}

#[tokio::test]
async fn measured_keep_all_preserves_only_space_and_punctuation_boundaries() {
    let mut style = ComputedStyle::initial();
    style.font_family = FontFamily::SansSerif;
    style.word_break = CssWordBreak::KeepAll;
    let text = "中文english中文english 中文english中文，english中文english";
    let breaks = measured_break_opportunities(text, &style);

    assert_eq!(
        breaks,
        vec![
            "中文english中文english ".len(),
            "中文english中文english 中文english中文，".len(),
            text.len()
        ]
    );
}

#[tokio::test]
async fn measured_keep_all_retains_hyphen_boundaries() {
    let mut style = ComputedStyle::initial();
    style.word_break = CssWordBreak::KeepAll;
    let text = "AB-CD-EF";
    let breaks = measured_break_opportunities(text, &style);

    // `keep-all` suppresses CJK and word-unit opportunities, but must retain
    // the ordinary UAX #14 break after a hyphen.
    // <https://drafts.csswg.org/css-text-3/#valdef-word-break-keep-all>
    assert!(breaks.contains(&"AB-".len()), "{breaks:?}");
    assert!(breaks.contains(&"AB-CD-".len()), "{breaks:?}");
}

#[tokio::test]
async fn measured_manual_suppresses_thai_dictionary_breaks() {
    let mut style = ComputedStyle::initial();
    style.word_break = CssWordBreak::Manual;
    let text = "กรุงเทพคือสวยงาม";

    let breaks = measured_break_opportunities(text, &style);

    // `manual` retains only author-provided opportunities in this SA run;
    // there is none before the terminal graph boundary.
    // <https://drafts.csswg.org/css-text-4/#word-boundary-detection>
    assert_eq!(breaks, vec![text.len()]);
}

#[tokio::test]
async fn measured_pre_wrap_breaks_after_preserved_space_runs() {
    let mut style = ComputedStyle::initial();
    style.white_space = crate::css::WhiteSpace::PreWrap;
    let text = "xxxxxxxxxxxxxxxxxx x";

    let breaks = measured_break_opportunities(text, &style);

    assert!(breaks.contains(&"xxxxxxxxxxxxxxxxxx ".len()));
    assert!(!breaks.contains(&"xxxxxxxxxxxxxxxxxx".len()));
}

#[tokio::test]
async fn measured_break_spaces_breaks_after_each_preserved_space_and_tab() {
    let mut style = ComputedStyle::initial();
    style.white_space = crate::css::WhiteSpace::BreakSpaces;

    let text = "A  \tB";
    let breaks = measured_break_opportunities(text, &style);

    assert!(breaks.contains(&2));
    assert!(breaks.contains(&3));
    assert!(breaks.contains(&4));
}

#[tokio::test]
async fn measured_break_all_does_not_add_letter_breaks_beside_preserved_spaces() {
    let mut style = ComputedStyle::initial();
    style.word_break = CssWordBreak::BreakAll;
    style.white_space = crate::css::WhiteSpace::BreakSpaces;
    let text = "X XX X";
    let breaks = measured_break_opportunities(text, &style);

    assert!(
        breaks.contains(&3),
        "break-all should split the XX pair: {breaks:?}"
    );
    assert!(
        !breaks.contains(&1),
        "must not break before a space: {breaks:?}"
    );
    assert!(
        !breaks.contains(&4),
        "must not break before a space: {breaks:?}"
    );
}

#[tokio::test]
async fn measured_break_all_keeps_prefix_numeric_sequences_together() {
    let mut style = ComputedStyle::initial();
    style.word_break = CssWordBreak::BreakAll;
    let text = "XX XX\\\\\\";
    let before_prefix_numeric = "XX XX".len();
    let after_first_prefix_numeric = before_prefix_numeric + '\\'.len_utf8();

    let breaks = measured_break_opportunities(text, &style);

    // CSS Text's `break-all` only overrides letter-to-letter restrictions.
    // The UAX #14 prohibition after a PR class still applies, while the
    // preceding ordinary boundary remains available.
    // <https://drafts.csswg.org/css-text-3/#valdef-word-break-break-all>
    // <https://www.unicode.org/reports/tr14/#LB25>
    assert!(breaks.contains(&before_prefix_numeric), "{breaks:?}");
    assert!(!breaks.contains(&after_first_prefix_numeric), "{breaks:?}");
}

#[tokio::test]
async fn measured_anywhere_breaks_do_not_split_grapheme_clusters() {
    let mut style = ComputedStyle::initial();
    style.overflow_wrap = CssOverflowWrap::Anywhere;
    let text = "க்\u{0bc6}";

    let breaks = measured_break_opportunities(text, &style);

    assert_eq!(breaks, vec![text.len()]);
}

#[tokio::test]
async fn hyphenate_limit_chars_filters_automatic_hyphenation_breaks() {
    let hyphenator = hyphenator_for_language("en-us").expect("embedded en-us hyphenator");

    assert_eq!(
        text_with_auto_hyphenation("example", &hyphenator, HyphenateLimitChars::AUTO),
        "ex\u{00ad}am\u{00ad}ple"
    );
    assert_eq!(
        text_with_auto_hyphenation(
            "example",
            &hyphenator,
            HyphenateLimitChars {
                total: 8,
                before: 2,
                after: 2,
            },
        ),
        "example"
    );
    assert_eq!(
        text_with_auto_hyphenation(
            "example",
            &hyphenator,
            HyphenateLimitChars {
                total: HyphenateLimitChars::AUTO_TOTAL,
                before: 3,
                after: 2,
            },
        ),
        "exam\u{00ad}ple"
    );
    assert_eq!(
        text_with_auto_hyphenation(
            "example",
            &hyphenator,
            HyphenateLimitChars {
                total: HyphenateLimitChars::AUTO_TOTAL,
                before: 2,
                after: 4,
            },
        ),
        "ex\u{00ad}ample"
    );
    assert_eq!(
        text_with_auto_hyphenation(
            "example",
            &hyphenator,
            HyphenateLimitChars {
                total: HyphenateLimitChars::AUTO_TOTAL,
                before: 3,
                after: 4,
            },
        ),
        "example"
    );
}

#[test]
fn unicode_space_separators_hang_in_preserved_white_space_modes() {
    let mut pre = ComputedStyle::initial();
    pre.white_space = WhiteSpace::Pre;
    assert_eq!(
        trim_trailing_css_hanging_space_separators("x\u{3000}", &pre),
        "x"
    );

    let mut pre_wrap = pre.clone();
    pre_wrap.white_space = WhiteSpace::PreWrap;
    assert_eq!(
        trim_trailing_css_hanging_space_separators("x\u{2000}", &pre_wrap),
        "x"
    );

    let mut break_spaces = pre;
    break_spaces.white_space = WhiteSpace::BreakSpaces;
    assert_eq!(
        trim_trailing_css_hanging_space_separators("x\u{3000}", &break_spaces),
        "x\u{3000}"
    );
}

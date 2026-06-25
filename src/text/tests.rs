use super::system::span_boundary_needs_join_control;
use super::*;
use crate::css::{
    ComputedLengthPercentage, Css, FontFeatureSetting, FontFeatureSettings, FontSizeAdjust,
    FontSizeAdjustMetric, FontSizeAdjustValue, parse_stylesheet,
};
use std::path::PathBuf;

async fn feature_probe_font_system() -> (FontSystem, ComputedStyle) {
    let stylesheet = parse_stylesheet(
        &Css::from_string(
            r#"@font-face {
                font-family: FeatureProbe;
                src: url("WeasyPrint/tests/resources/weasyprint.otf");
            }"#,
        )
        .with_base_url(Some(PathBuf::from("."))),
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

fn shaped_glyph_ids(system: &mut FontSystem, text: &str, style: &ComputedStyle) -> Vec<u16> {
    system
        .shape_text_runs_with_parley(text, style)
        .into_iter()
        .flat_map(|run| run.glyphs.unwrap_or_default())
        .map(|glyph| glyph.id)
        .collect()
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
        .with_base_url(Some(PathBuf::from("."))),
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
        .map(|run| run.text)
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
async fn parley_wraps_text_with_shaped_widths() {
    let mut system = FontSystem::new();
    let mut style = ComputedStyle::initial();
    style.font_family = FontFamily::SansSerif;
    style.font_size = 12.0;
    style.line_height = 14.4;

    let available_width = system.measure_text("one two", &style) + 0.1;
    assert!(system.measure_text("one two three", &style) > available_width);

    let lines = system
        .break_text("one two three", &style, available_width)
        .into_iter()
        .map(|line| line.text)
        .collect::<Vec<_>>();

    assert_eq!(lines, vec!["one two", "three"]);
}

#[tokio::test]
async fn broken_lines_carry_shaped_font_and_glyph_data() {
    let mut system = FontSystem::new();
    let mut style = ComputedStyle::initial();
    style.font_family = FontFamily::SansSerif;
    style.font_size = 12.0;
    style.line_height = 14.4;

    let line = system
        .break_text("one two", &style, 500.0)
        .into_iter()
        .next()
        .unwrap();
    let shaped = line.shaped.as_ref().expect("line should be shaped once");
    let advance = shaped.advance_width();

    assert_eq!(shaped.text, line.text);
    assert!(shaped.first_font_id().is_some());
    assert!((advance - line.width).abs() < 0.01);
    assert!(shaped.runs.iter().all(|run| run.font_id.is_some()));
    assert!(
        shaped
            .runs
            .iter()
            .flat_map(|run| &run.glyphs)
            .any(|glyph| !glyph.source_text.is_empty())
    );
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
        .find(|glyph| glyph.source_text == " ")
        .map(|glyph| glyph.rendered.x_advance)
        .expect("space glyph");

    let added_width = shaped.apply_inter_word_justification(10.0, 1);

    let justified_space_advance = shaped
        .runs
        .iter()
        .flat_map(|run| &run.glyphs)
        .find(|glyph| glyph.source_text == " ")
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
        .map(|glyph| glyph.source_text.clone())
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
            .map(|glyph| glyph.source_text.clone())
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
async fn broken_line_width_uses_shaped_css_measure_without_losing_hanging_glyphs() {
    let mut system = FontSystem::new();
    let mut style = ComputedStyle::initial();
    style.font_family = FontFamily::SansSerif;
    style.font_size = 12.0;
    style.line_height = 14.4;
    style.white_space = crate::css::WhiteSpace::Normal;

    let line = system
        .break_text("X\u{3000}", &style, 500.0)
        .into_iter()
        .next()
        .unwrap();
    let shaped = line.shaped.as_ref().expect("line should be shaped once");
    let visible_width = system.measure_text("X", &style);

    assert_eq!(line.text, "X\u{3000}");
    assert!(
        (line.width - visible_width).abs() < 0.01,
        "CSS line measure should exclude hanging ideographic space"
    );
    assert!(
        shaped.advance_width() > line.width,
        "shaped payload should keep the hanging glyph for painting"
    );
}

#[tokio::test]
async fn shaped_lines_keep_controls_for_shaping_without_visible_glyphs() {
    let mut system = FontSystem::new();
    let mut style = ComputedStyle::initial();
    style.font_family = FontFamily::SansSerif;
    style.font_size = 12.0;
    style.line_height = 14.4;

    let line = system
        .break_text("A\u{200c}B", &style, 500.0)
        .into_iter()
        .next()
        .unwrap();
    let shaped = line.shaped.as_ref().expect("line should be shaped once");

    assert_eq!(shaped.text, "A\u{200c}B");
    assert!(
        shaped
            .runs
            .iter()
            .flat_map(|run| &run.glyphs)
            .all(|glyph| !glyph.source_text.chars().any(character_is_join_control)),
        "{shaped:?}"
    );
}

#[tokio::test]
async fn break_spaces_wraps_at_preserved_spaces() {
    let mut system = FontSystem::new();
    let mut style = ComputedStyle::initial();
    style.font_family = FontFamily::SansSerif;
    style.font_size = 12.0;
    style.line_height = 14.4;
    style.white_space = crate::css::WhiteSpace::BreakSpaces;

    let available_width = system.measure_text("A  ", &style) + 0.1;
    assert!(system.measure_text("A   ", &style) > available_width);

    let lines = system
        .break_text("A   B", &style, available_width)
        .into_iter()
        .map(|line| line.text)
        .collect::<Vec<_>>();

    assert_eq!(lines, vec!["A  ", " B"]);
}

#[tokio::test]
async fn break_spaces_preserves_trailing_space_advance() {
    let mut system = FontSystem::new();
    let mut style = ComputedStyle::initial();
    style.font_family = FontFamily::SansSerif;
    style.font_size = 12.0;
    style.line_height = 14.4;
    style.white_space = crate::css::WhiteSpace::BreakSpaces;

    let line = system
        .break_text("A  ", &style, 100.0)
        .into_iter()
        .next()
        .unwrap();

    assert_eq!(line.text, "A  ");
    assert!((line.width - system.measure_text("A  ", &style)).abs() < 0.01);
    let shaped = line
        .shaped
        .as_ref()
        .expect("break-spaces line should carry selected shaped payload");
    assert_eq!(shaped.text, line.text);
    assert!((shaped.advance_width() - line.width).abs() < 0.01);
    assert!(shaped.runs.iter().any(|run| !run.glyphs.is_empty()));
}

#[tokio::test]
async fn parley_lines_expose_bidi_visual_text_order() {
    let mut system = FontSystem::new();
    let mut style = ComputedStyle::initial();
    style.font_family = FontFamily::SansSerif;
    style.font_size = 12.0;
    style.line_height = 14.4;

    let line = system
        .break_text("abc אבג def", &style, 500.0)
        .into_iter()
        .next()
        .unwrap();

    assert_eq!(line.text, "abc גבא def");
}

#[tokio::test]
async fn css_ltr_direction_overrides_first_strong_rtl_paragraph_direction() {
    let mut system = FontSystem::new();
    let mut style = ComputedStyle::initial();
    style.font_family = FontFamily::SansSerif;
    style.font_size = 12.0;
    style.line_height = 14.4;
    style.direction = Direction::Ltr;

    let line = system
        .break_text("אבג abc", &style, 500.0)
        .into_iter()
        .next()
        .unwrap();

    assert_eq!(line.text, "גבא abc");
}

#[tokio::test]
async fn css_rtl_direction_overrides_first_strong_ltr_paragraph_direction() {
    let mut system = FontSystem::new();
    let mut style = ComputedStyle::initial();
    style.font_family = FontFamily::SansSerif;
    style.font_size = 12.0;
    style.line_height = 14.4;
    style.direction = Direction::Rtl;

    let line = system
        .break_text("abc אבג def", &style, 500.0)
        .into_iter()
        .next()
        .unwrap();

    assert_eq!(line.text, "def גבא abc");
}

#[tokio::test]
async fn css_direction_controls_neutral_only_bidi_paragraphs_without_painting_controls() {
    let mut system = FontSystem::new();
    let mut style = ComputedStyle::initial();
    style.font_family = FontFamily::SansSerif;
    style.font_size = 12.0;
    style.line_height = 14.4;
    style.direction = Direction::Rtl;

    let line = system
        .break_text("?!", &style, 500.0)
        .into_iter()
        .next()
        .unwrap();

    assert_eq!(line.text, "!?");
    assert!(!line.text.chars().any(character_is_bidi_format_control));
}

#[tokio::test]
async fn unicode_bidi_override_uses_uba_controls_without_painting_them() {
    let mut system = FontSystem::new();
    let mut style = ComputedStyle::initial();
    style.font_family = FontFamily::SansSerif;
    style.font_size = 12.0;
    style.line_height = 14.4;
    style.direction = Direction::Rtl;
    style.unicode_bidi = UnicodeBidi::BidiOverride;

    let line = system
        .break_text("abc def", &style, 500.0)
        .into_iter()
        .next()
        .unwrap();

    assert_eq!(line.text, "fed cba");
    assert!(!line.text.chars().any(character_is_bidi_format_control));
}

#[tokio::test]
async fn bidi_control_only_line_carries_shaped_payload_without_rewrite() {
    let mut system = FontSystem::new();
    let mut style = ComputedStyle::initial();
    style.font_family = FontFamily::SansSerif;
    style.font_size = 12.0;
    style.line_height = 14.4;

    let line = system
        .break_text("A\u{200e}B", &style, 500.0)
        .into_iter()
        .next()
        .unwrap();
    let shaped = line
        .shaped
        .as_ref()
        .expect("bidi-control line should carry shaped payload");

    assert_eq!(line.text, "AB");
    assert_eq!(shaped.text, line.text);
    assert!((shaped.width - line.width).abs() < 0.01);
    assert!(shaped.runs.iter().flat_map(|run| &run.glyphs).all(|glyph| {
        !glyph
            .source_text
            .chars()
            .any(character_is_bidi_format_control)
    }));
}

#[tokio::test]
async fn overflow_wrap_anywhere_breaks_long_words() {
    let mut system = FontSystem::new();
    let mut normal = ComputedStyle::initial();
    normal.font_family = FontFamily::SansSerif;
    normal.font_size = 12.0;
    normal.line_height = 14.4;
    normal.overflow_wrap = CssOverflowWrap::Normal;

    let mut anywhere = normal.clone();
    anywhere.overflow_wrap = CssOverflowWrap::Anywhere;

    let available_width = system.measure_text("abc", &normal) + 0.1;
    let normal_lines = system.break_text("abcdefgh", &normal, available_width);
    let anywhere_lines = system.break_text("abcdefgh", &anywhere, available_width);

    assert_eq!(normal_lines.len(), 1);
    assert!(anywhere_lines.len() > 1);
    assert_eq!(
        anywhere_lines
            .iter()
            .map(|line| line.text.as_str())
            .collect::<String>(),
        "abcdefgh"
    );
    assert!(
        anywhere_lines.iter().all(|line| line.shaped.is_some()),
        "emergency wrapped lines should keep shaped selected candidates"
    );
    for line in &anywhere_lines {
        let shaped = line.shaped.as_ref().unwrap();
        assert_eq!(shaped.text, line.text);
        assert!(
            (shaped.width - line.width).abs() < 0.01,
            "selected emergency candidate should supply measured width"
        );
    }
}

#[tokio::test]
async fn break_word_prefers_pre_wrap_space_opportunities() {
    let mut system = FontSystem::new();
    let mut style = ComputedStyle::initial();
    style.font_family = FontFamily::SansSerif;
    style.font_size = 12.0;
    style.line_height = 14.4;
    style.white_space = crate::css::WhiteSpace::PreWrap;
    style.overflow_wrap = CssOverflowWrap::BreakWord;

    let available_width = system
        .measure_text(" XX ", &style)
        .max(system.measure_text("XXX ", &style))
        + 0.1;
    let lines = system.break_text(" XX XXX ", &style, available_width);

    assert_eq!(
        lines
            .iter()
            .map(|line| line.text.as_str())
            .collect::<Vec<_>>(),
        vec![" XX", "XXX "]
    );
}

#[tokio::test]
async fn pre_wrap_soft_wrap_hangs_trailing_space_and_consumes_break_space() {
    let mut system = FontSystem::new();
    let mut style = ComputedStyle::initial();
    style.font_family = FontFamily::SansSerif;
    style.font_size = 12.0;
    style.line_height = 14.4;
    style.white_space = crate::css::WhiteSpace::PreWrap;

    let available_width = system.measure_text("one two three", &style) + 0.1;
    assert!(system.measure_text("one two three four", &style) > available_width);

    let lines = system.break_text("one two three four five", &style, available_width);

    assert_eq!(
        lines
            .iter()
            .map(|line| line.text.as_str())
            .collect::<Vec<_>>(),
        vec!["one two three", "four five"]
    );
    assert!(
        (lines[0].width - system.measure_text("one two three", &style)).abs() < 0.01,
        "soft-wrap trailing space must not contribute to line width"
    );
}

#[tokio::test]
async fn normal_white_space_wraps_after_leading_ideographic_space_sequence() {
    let mut system = FontSystem::new();
    let mut style = ComputedStyle::initial();
    style.font_family = FontFamily::SansSerif;
    style.font_size = 12.0;
    style.line_height = 14.4;
    style.white_space = crate::css::WhiteSpace::Normal;

    let leading_spaces = "\u{3000}\u{3000}";
    let available_width = system.measure_text(leading_spaces, &style) + 0.1;
    assert!(system.measure_text("\u{3000}\u{3000}XX", &style) > available_width);

    let lines = system.break_text("\u{3000}\u{3000}XX", &style, available_width);

    assert_eq!(
        lines
            .iter()
            .map(|line| line.text.as_str())
            .collect::<Vec<_>>(),
        vec![leading_spaces, "XX"]
    );
}

#[tokio::test]
async fn normal_white_space_hangs_trailing_ideographic_space_from_line_measure() {
    let mut system = FontSystem::new();
    let mut style = ComputedStyle::initial();
    style.font_family = FontFamily::SansSerif;
    style.font_size = 12.0;
    style.line_height = 14.4;
    style.white_space = crate::css::WhiteSpace::Normal;

    let available_width = system.measure_text("X", &style) + 0.1;
    let line = system
        .break_text("X\u{3000}", &style, available_width)
        .into_iter()
        .next()
        .unwrap();

    assert_eq!(line.text, "X\u{3000}");
    assert!(
        (line.width - system.measure_text("X", &style)).abs() < 0.01,
        "trailing ideographic space must hang from line measurement"
    );
}

#[tokio::test]
async fn forced_break_hangs_trailing_ideographic_space() {
    let mut system = FontSystem::new();
    let mut style = ComputedStyle::initial();
    style.font_family = FontFamily::SansSerif;
    style.font_size = 12.0;
    style.line_height = 14.4;
    style.white_space = crate::css::WhiteSpace::Normal;

    let available_width = system.measure_text("X", &style) + 0.1;
    let lines = system.break_text("X\u{3000}\nXX", &style, available_width);

    assert_eq!(lines[0].text, "X\u{3000}");
    assert!(
        (lines[0].width - system.measure_text("X", &style)).abs() < 0.01,
        "forced-break trailing ideographic space must not affect line width"
    );
}

#[tokio::test]
async fn plaintext_bidi_hangs_trailing_ideographic_space() {
    let mut system = FontSystem::new();
    let mut style = ComputedStyle::initial();
    style.font_family = FontFamily::SansSerif;
    style.font_size = 12.0;
    style.line_height = 14.4;
    style.white_space = crate::css::WhiteSpace::Normal;
    style.unicode_bidi = UnicodeBidi::Plaintext;

    let available_width = system.measure_text("X", &style) + 0.1;
    let line = system
        .break_text("X\u{3000}", &style, available_width)
        .into_iter()
        .next()
        .unwrap();

    assert_eq!(line.text, "X\u{3000}");
    assert!(
        (line.width - system.measure_text("X", &style)).abs() < 0.01,
        "unicode-bidi: plaintext must not disable CSS Text hanging"
    );
}

#[tokio::test]
async fn break_spaces_keeps_trailing_ideographic_space_advance() {
    let mut system = FontSystem::new();
    let mut style = ComputedStyle::initial();
    style.font_family = FontFamily::SansSerif;
    style.font_size = 12.0;
    style.line_height = 14.4;
    style.white_space = crate::css::WhiteSpace::BreakSpaces;

    let line = system
        .break_text("X\u{3000}", &style, 500.0)
        .into_iter()
        .next()
        .unwrap();

    assert_eq!(line.text, "X\u{3000}");
    assert!(
        (line.width - system.measure_text("X\u{3000}", &style)).abs() < 0.01,
        "break-spaces must keep separator advance"
    );
}

#[tokio::test]
async fn break_spaces_breaks_after_ba_space_separator_without_hanging() {
    let mut system = FontSystem::new();
    let mut style = ComputedStyle::initial();
    style.font_family = FontFamily::SansSerif;
    style.font_size = 12.0;
    style.line_height = 14.4;
    style.white_space = crate::css::WhiteSpace::BreakSpaces;

    assert_eq!(line_break_class('\u{2002}'), LineBreak::BreakAfter);
    let available_width = system.measure_text("xx", &style) + 0.1;
    let lines = system
        .break_text("xx\u{2002}A", &style, available_width)
        .into_iter()
        .map(|line| (line.text, line.width))
        .collect::<Vec<_>>();

    assert_eq!(lines[0].0, "xx\u{2002}");
    assert!(
        lines[0].1 > available_width,
        "break-spaces keeps the U+2002 advance instead of hanging it"
    );
    assert_eq!(lines[1].0, "A");
}

#[tokio::test]
async fn break_spaces_keeps_other_separator_uax14_breaks_between_ideographs() {
    let mut system = FontSystem::new();
    let mut style = ComputedStyle::initial();
    style.font_family = FontFamily::SansSerif;
    style.font_size = 12.0;
    style.line_height = 14.4;
    style.white_space = crate::css::WhiteSpace::BreakSpaces;

    let available_width = system.measure_text("xx", &style) + 0.1;
    let lines = system
        .break_text(
            "xx\u{2002}ああ\u{2002}ああ\u{2002}xx",
            &style,
            available_width,
        )
        .into_iter()
        .map(|line| line.text)
        .collect::<Vec<_>>();

    assert_eq!(
        lines,
        vec!["xx\u{2002}", "あ", "あ\u{2002}", "あ", "あ\u{2002}", "xx"]
    );
}

#[tokio::test]
async fn word_break_break_all_breaks_long_words() {
    let mut system = FontSystem::new();
    let mut style = ComputedStyle::initial();
    style.font_family = FontFamily::SansSerif;
    style.font_size = 12.0;
    style.line_height = 14.4;
    style.word_break = CssWordBreak::BreakAll;

    let available_width = system.measure_text("abc", &style) + 0.1;
    let lines = system.break_text("abcdefgh", &style, available_width);

    assert!(lines.len() > 1);
    assert_eq!(
        lines
            .iter()
            .map(|line| line.text.as_str())
            .collect::<String>(),
        "abcdefgh"
    );
}

#[tokio::test]
async fn unbroken_soft_hyphens_are_hidden() {
    let mut system = FontSystem::new();
    let mut style = ComputedStyle::initial();
    style.font_family = FontFamily::SansSerif;
    style.font_size = 12.0;
    style.line_height = 14.4;

    let lines = system.break_text("hyphen\u{00ad}ation", &style, 500.0);

    assert_eq!(lines.len(), 1);
    assert_eq!(lines[0].text, "hyphenation");
}

#[tokio::test]
async fn broken_soft_hyphens_are_visible() {
    let mut system = FontSystem::new();
    let mut style = ComputedStyle::initial();
    style.font_family = FontFamily::SansSerif;
    style.font_size = 12.0;
    style.line_height = 14.4;

    let available_width = system.measure_text("hyphen", &style) + 0.1;
    let lines = system.break_text("hyphen\u{00ad}ation", &style, available_width);

    assert!(lines.len() > 1);
    assert_eq!(lines[0].text, "hyphen-");
    assert_eq!(
        lines
            .iter()
            .map(|line| line.text.as_str())
            .collect::<String>(),
        "hyphen-ation"
    );
}

#[tokio::test]
async fn hyphens_none_suppresses_soft_hyphen_breaks() {
    let mut system = FontSystem::new();
    let mut style = ComputedStyle::initial();
    style.font_family = FontFamily::SansSerif;
    style.font_size = 12.0;
    style.line_height = 14.4;
    style.hyphens = Hyphens::None;

    let available_width = system.measure_text("hyphen", &style) + 0.1;
    let lines = system.break_text("hyphen\u{00ad}ation", &style, available_width);

    assert_eq!(lines.len(), 1);
    assert_eq!(lines[0].text, "hyphenation");
}

#[tokio::test]
async fn line_break_anywhere_adds_break_opportunities_without_visible_markers() {
    let mut system = FontSystem::new();
    let mut normal = ComputedStyle::initial();
    normal.font_family = FontFamily::SansSerif;
    normal.font_size = 12.0;
    normal.line_height = 14.4;

    let mut anywhere = normal.clone();
    anywhere.line_break = CssLineBreak::Anywhere;

    let available_width = system.measure_text("abc", &normal) + 0.1;
    let normal_lines = system.break_text("abcdefgh", &normal, available_width);
    let anywhere_lines = system.break_text("abcdefgh", &anywhere, available_width);

    assert_eq!(normal_lines.len(), 1);
    assert!(anywhere_lines.len() > 1);
    assert_eq!(
        anywhere_lines
            .iter()
            .map(|line| line.text.as_str())
            .collect::<String>(),
        "abcdefgh"
    );
    assert!(
        anywhere_lines
            .iter()
            .all(|line| !line.text.contains(ZERO_WIDTH_SPACE))
    );
}

#[tokio::test]
async fn line_break_anywhere_respects_white_space_pre_no_wrap_suppression() {
    let mut system = FontSystem::new();
    let mut style = ComputedStyle::initial();
    style.font_family = FontFamily::SansSerif;
    style.font_size = 12.0;
    style.line_height = 14.4;
    style.line_break = CssLineBreak::Anywhere;
    style.white_space = crate::css::WhiteSpace::Pre;

    let available_width = system.measure_text(" X", &style) + 0.1;
    let lines = system.break_text(" XXX", &style, available_width);

    assert_eq!(lines.len(), 1);
    assert_eq!(lines[0].text, " XXX");
}

#[tokio::test]
async fn line_break_anywhere_respects_white_space_nowrap_suppression() {
    let mut system = FontSystem::new();
    let mut style = ComputedStyle::initial();
    style.font_family = FontFamily::SansSerif;
    style.font_size = 12.0;
    style.line_height = 14.4;
    style.line_break = CssLineBreak::Anywhere;
    style.white_space = crate::css::WhiteSpace::NoWrap;

    let available_width = system.measure_text("XX", &style) + 0.1;
    let lines = system.break_text("XXXX XX", &style, available_width);

    assert_eq!(lines.len(), 1);
    assert_eq!(lines[0].text, "XXXX XX");
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
    style.letter_spacing = ComputedLengthPercentage::from_length(10.0);

    let mut untracked = style.clone();
    untracked.letter_spacing = ComputedLengthPercentage::ZERO;
    let expected_line_width = system.measure_text("aa", &untracked) + style.used_letter_spacing();

    assert!(
        (line_end_letter_spacing_width("aa", &style) - style.used_letter_spacing()).abs() < 0.01
    );
    assert!((system.measure_line_text("aa", &style) - expected_line_width).abs() < 0.01);
    assert!(
        (system.measure_line_text("a", &style) - system.measure_text("a", &untracked)).abs() < 0.01
    );
}

#[tokio::test]
async fn join_controls_stay_in_adjacent_font_runs() {
    let mut system = FontSystem::new();
    let mut style = ComputedStyle::initial();
    style.font_family = FontFamily::SansSerif;
    style.font_size = 12.0;

    let runs = system.shape_text_runs_with_parley("A\u{200c}B", &style);

    assert_eq!(runs.len(), 1);
    assert_eq!(runs[0].text, "A\u{200c}B");
}

#[tokio::test]
async fn join_controls_shape_but_do_not_emit_visible_glyphs() {
    let mut system = FontSystem::new();
    let mut style = ComputedStyle::initial();
    style.font_family = FontFamily::SansSerif;
    style.font_size = 12.0;

    let runs = system.shape_text_runs_with_parley("A\u{200c}B", &style);

    assert_eq!(runs.len(), 1);
    assert_eq!(runs[0].text, "A\u{200c}B");
    let glyphs = runs[0].glyphs.as_ref().expect("expected shaped glyphs");
    assert!(
        glyphs
            .iter()
            .all(|glyph| !glyph.unicode.chars().any(character_is_join_control)),
        "{glyphs:?}"
    );
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
        .flat_map(|run| run.glyphs.unwrap_or_default())
        .next()
        .unwrap();
    let en_space_glyph = system
        .shape_text_runs_with_parley("\u{2002}", &style)
        .into_iter()
        .flat_map(|run| run.glyphs.unwrap_or_default())
        .next()
        .unwrap();

    assert_eq!(en_space_glyph.id, space_glyph.id);
    assert_eq!(en_space_glyph.unicode, "\u{2002}");
    assert!(en_space_glyph.x_advance > 0.0);
}

#[tokio::test]
async fn unicode_properties_cover_bidi_and_line_break_classes() {
    assert!(contains_bidi_text("abc אבג"));
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
    assert!(character_is_default_ignorable_code_point('\u{fe0f}'));
    assert!(!character_is_default_ignorable_code_point('字'));
    assert!(character_preserves_word_boundary_context('’'));
    assert!(!character_preserves_word_boundary_context(' '));
}

#[tokio::test]
async fn hanging_punctuation_fit_width_uses_css_text_end_policy() {
    let mut system = FontSystem::new();
    let mut style = ComputedStyle::initial();
    style.font_family = FontFamily::SansSerif;
    style.font_size = 12.0;
    style.white_space = crate::css::WhiteSpace::Normal;

    let raw_width = system.measure_line_text("Hello。", &style);
    let stop_width = system.measure_text("。", &style);
    assert!(stop_width > 0.0);

    let none_width =
        system.hanging_punctuation_fit_width("Hello。", 0.."Hello。".len(), &style, raw_width);
    assert!((none_width - raw_width).abs() < 0.01);

    style.hanging_punctuation.force_end = true;
    let force_width =
        system.hanging_punctuation_fit_width("Hello。", 0.."Hello。".len(), &style, raw_width);
    assert!((force_width - (raw_width - stop_width)).abs() < 0.01);

    style.hanging_punctuation.force_end = false;
    style.hanging_punctuation.allow_end = true;
    let allow_width =
        system.hanging_punctuation_fit_width("Hello。", 0.."Hello。".len(), &style, raw_width);
    assert!((allow_width - (raw_width - stop_width)).abs() < 0.01);

    style.hanging_punctuation.allow_end = false;
    style.hanging_punctuation.last = true;
    let raw_close_width = system.measure_line_text("Hello」", &style);
    let last_width = system.hanging_punctuation_fit_width(
        "Hello」",
        0.."Hello」".len(),
        &style,
        raw_close_width,
    );
    let close_width = system.measure_text("」", &style);
    assert!((last_width - (raw_close_width - close_width)).abs() < 0.01);
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
async fn measured_anywhere_breaks_do_not_split_grapheme_clusters() {
    let mut style = ComputedStyle::initial();
    style.overflow_wrap = CssOverflowWrap::Anywhere;
    let text = "க்\u{0bc6}";

    let breaks = measured_break_opportunities(text, &style);

    assert_eq!(breaks, vec![text.len()]);
}

#[tokio::test]
async fn break_spaces_break_all_wraps_before_overflowing_character() {
    let mut system = FontSystem::new();
    let mut style = ComputedStyle::initial();
    style.white_space = crate::css::WhiteSpace::BreakSpaces;
    style.word_break = CssWordBreak::BreakAll;
    style.font_family = FontFamily::SansSerif;
    style.font_size = 12.0;

    let available_width = system.measure_text("  A", &style) + 0.1;
    let lines = system.break_text("  AB", &style, available_width);

    assert_eq!(lines[0].text, "  A");
    assert_eq!(lines[1].text, "B");
}

#[tokio::test]
async fn break_spaces_break_all_prefers_non_space_run_break_before_preserved_space() {
    let mut system = FontSystem::new();
    let mut style = ComputedStyle::initial();
    style.white_space = crate::css::WhiteSpace::BreakSpaces;
    style.word_break = CssWordBreak::BreakAll;
    style.font_family = FontFamily::SansSerif;
    style.font_size = 12.0;

    let available_width = system.measure_text("X XX", &style) + 0.1;
    let lines = system
        .break_text("X XX X", &style, available_width)
        .into_iter()
        .map(|line| line.text)
        .collect::<Vec<_>>();

    assert_eq!(lines, vec!["X X", "X X"]);
}

#[tokio::test]
async fn break_spaces_line_break_anywhere_can_break_before_preserved_space() {
    let mut system = FontSystem::new();
    let mut style = ComputedStyle::initial();
    style.white_space = crate::css::WhiteSpace::BreakSpaces;
    style.word_break = CssWordBreak::BreakAll;
    style.line_break = CssLineBreak::Anywhere;
    style.font_family = FontFamily::SansSerif;
    style.font_size = 12.0;

    let available_width = system.measure_text("X XX", &style) + 0.1;
    let lines = system
        .break_text("X XX X", &style, available_width)
        .into_iter()
        .map(|line| line.text)
        .collect::<Vec<_>>();

    assert_eq!(lines, vec!["X XX", " X"]);
}

#[tokio::test]
async fn hyphens_auto_uses_dictionary_for_known_language() {
    let mut system = FontSystem::new();
    let mut manual = ComputedStyle::initial();
    manual.font_family = FontFamily::SansSerif;
    manual.font_size = 12.0;
    manual.line_height = 14.4;

    let mut auto = manual.clone();
    auto.hyphens = Hyphens::Auto;
    auto.language = Some("en".to_string());

    let available_width = system.measure_text("ribo", &manual) + 0.1;
    let manual_lines = system.break_text("ribonuclease", &manual, available_width);
    let auto_lines = system.break_text("ribonuclease", &auto, available_width);

    assert_eq!(manual_lines.len(), 1);
    assert!(auto_lines.len() > 1);
    assert!(auto_lines.iter().any(|line| line.text.ends_with('-')));
    assert_eq!(
        auto_lines
            .iter()
            .map(|line| line.text.replace('-', ""))
            .collect::<String>(),
        "ribonuclease"
    );
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

#[tokio::test]
async fn hyphens_auto_requires_known_language() {
    let mut system = FontSystem::new();
    let mut style = ComputedStyle::initial();
    style.font_family = FontFamily::SansSerif;
    style.font_size = 12.0;
    style.line_height = 14.4;
    style.hyphens = Hyphens::Auto;

    let available_width = system.measure_text("ribo", &style) + 0.1;
    let lines = system.break_text("ribonuclease", &style, available_width);

    assert_eq!(lines.len(), 1);
    assert_eq!(lines[0].text, "ribonuclease");
}

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

    assert_eq!(shaped.text, "X\u{3000}");
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
async fn zwj_wrapped_arabic_letter_shapes_with_wpt_font() {
    let stylesheet = parse_stylesheet(
        &Css::from_string(
            r#"@font-face {
                font-family: AlreqNaskh;
                src: url("tests/resources/fonts/NotoNaskhArabic-regular.woff2");
            }"#,
        )
        .with_base_url(Some(PathBuf::from("."))),
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
        .with_base_url(Some(PathBuf::from("."))),
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
        .flat_map(|run| run.glyphs.unwrap_or_default())
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

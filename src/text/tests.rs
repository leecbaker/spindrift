use super::system::span_boundary_needs_join_control;
use crate::CssColor;
use crate::css::{ComputedLineHeight, ContentLanguage, WritingMode};
use crate::document::paint::geometry::PaintDisplacement;
use crate::document::paint::text::{RenderedGlyph, RenderedGlyphKind, RenderedLine};

#[test]
fn css_text_classifies_controls_before_whitespace_processing() {
    assert_eq!(
        classify_css_text_scalar('\u{000c}'),
        CssTextScalar::VisibleControl(VisibleControlCharacter('\u{000c}'))
    );
    assert_eq!(
        classify_css_text_scalar('\r'),
        CssTextScalar::CarriageReturn
    );
    assert_eq!(classify_css_text_scalar('\n'), CssTextScalar::SegmentBreak);
    assert_eq!(classify_css_text_scalar('\t'), CssTextScalar::Tab);
    assert_eq!(
        classify_css_text_scalar('\u{0080}'),
        CssTextScalar::VisibleControl(VisibleControlCharacter('\u{0080}'))
    );
    assert!(is_css_collapsible_whitespace('\r'));
    assert!(!is_css_collapsible_whitespace('\u{000c}'));
}

#[test]
fn css_text_materializes_controls_as_visible_common_symbols() {
    assert_eq!(
        css_text_rendering_text("A\u{000b}\u{000c}\u{007f}\u{009f}B"),
        "A\u{25a0}\u{25a0}\u{25a0}\u{25a0}B"
    );
    assert_eq!(css_text_rendering_text("A\rB\tC\nD"), "A B\tC\nD");
}

#[test]
fn rendered_line_alignment_reverses_the_stored_glyph_origin_adjustment() {
    let line = RenderedLine::new(
        "X".to_string(),
        10.0,
        20.0,
        12.0,
        // The legacy line-level font metadata deliberately does not identify
        // a loaded font. Baseline recovery must use the conversion recorded at
        // paint time instead of deriving a new one from this field.
        Some(usize::MAX),
        CssColor::BLACK,
        Vec::new(),
    )
    .with_glyph_origin_adjustment(PaintDisplacement::new(0.0, 3.5));

    assert!((FontSystem::new().rendered_line_alignment_y(&line).points() - 28.5).abs() < 0.01);
}

#[test]
fn arabic_visual_ranges_are_emitted_in_reverse_cluster_order() {
    let mut style = ComputedStyle::initial();
    style.direction = Direction::Ltr;
    let mut system = FontSystem::new();
    let ranges = system.visual_ranges_for_unwrapped_text("السلامعليكم", style.used_direction());
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

#[test]
fn rtl_flag_emoji_visual_range_precedes_the_hebrew_run() {
    let mut style = ComputedStyle::initial();
    style.direction = Direction::Rtl;
    style.font_size = 32.0;
    style.line_height = 38.4;
    let text = "לום🇱🇮";
    let mut system = FontSystem::new();

    let visual_text = system
        .visual_ranges_for_unwrapped_text(text, style.used_direction())
        .into_iter()
        .map(|range| &text[range.range])
        .collect::<String>();

    // The RTL run's individual source clusters are also in visual order, so
    // their source scalars appear reversed in this presentation sequence.
    assert_eq!(visual_text, "🇱🇮םול");
}
use super::*;
use crate::css::{
    ComputedLengthPercentage, Css, FontFeatureSetting, FontFeatureSettings, FontPalette,
    FontSizeAdjust, FontSizeAdjustMetric, FontSizeAdjustValue, FontVariationSetting, WhiteSpace,
    parse_stylesheet,
};
use crate::units::{LayoutLength, SemanticLengthExt, layout_pt};
use std::rc::Rc;

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
    style.line_height = 18.0;
    style.line_height_value = ComputedLineHeight::from_points(18.0);

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

#[tokio::test]
async fn glyph_metrics_use_the_css_font_size_without_size_adjust() {
    let stylesheet = parse_stylesheet(
        &Css::from_string(
            r#"@font-face {
                font-family: MetricFace;
                src: url("tests/fixtures/wpt/css/css-fonts/Ahem.ttf");
            }"#,
        )
        .with_base_path(".")
        .expect("current directory should be a valid file URL"),
    );
    let mut system = FontSystem::start_loading()
        .load_stylesheet_fonts(&[stylesheet])
        .finish()
        .await;
    let mut style = ComputedStyle::initial();
    style.font_family = FontFamily::Names(vec!["MetricFace".to_string()]);
    style.font_size = 40.0;

    let font_id = system
        .font_for_character(&style, '0')
        .expect("the loaded face should cover U+0030");
    assert_eq!(system.used_font_size_for_font(&style, font_id), Some(40.0));
}

#[tokio::test]
async fn upright_ch_uses_the_zero_glyph_face_and_its_size_adjustment() {
    let stylesheet = parse_stylesheet(
        &Css::from_string(
            r#"@font-face {
                font-family: MetricZero;
                src: url("tests/fixtures/wpt/css/css-fonts/Ahem.ttf");
                size-adjust: 50%;
                unicode-range: U+0030;
            }"#,
        )
        .with_base_path(".")
        .expect("current directory should be a valid file URL"),
    );
    let mut system = FontSystem::start_loading()
        .load_stylesheet_fonts(&[stylesheet])
        .finish()
        .await;
    let mut style = ComputedStyle::initial();
    style.font_family = FontFamily::Names(vec!["MetricZero".to_string()]);
    style.font_size = 40.0;
    style.line_height = 40.0;
    style.writing_mode = WritingMode::VerticalRl;
    style.text_orientation = TextOrientation::Upright;

    // Ahem lacks a vertical advance table, so CSS falls back to the matched
    // face's one-em advance. Its `size-adjust` makes that 20pt, rather than
    // the 40pt U+0020 line-metric fallback face.
    assert_eq!(system.ch_advance(&style), layout_pt(20.0));
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
        .map(|glyph| glyph.painted_id().expect("paintable glyph"))
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
        .map(|glyph| glyph.painted_id().expect("paintable glyph"))
        .collect::<Vec<_>>();
    let styled_with_cgj = system
        .shape_styled_text_runs_with_parley(&[StyledTextSpan {
            text: "A\u{034f}",
            style: &style,
        }])
        .into_iter()
        .flat_map(|run| run.glyphs)
        .map(|glyph| glyph.painted_id().expect("paintable glyph"))
        .collect::<Vec<_>>();

    // CGJ affects UAX #14 boundaries but is font-neutral: it must not make a
    // visible glyph fall back to a different face.
    // <https://www.w3.org/TR/css-text-3/#line-break-details>
    // <https://www.unicode.org/reports/tr44/#Default_Ignorable_Code_Point>
    assert_eq!(with_cgj, plain);
    assert_eq!(styled_with_cgj, styled_plain);
}

#[test]
fn styled_zero_width_space_between_different_styles_keeps_source_text() {
    let mut system = FontSystem::new();
    let mut pre = ComputedStyle::initial();
    pre.font_family = FontFamily::Monospace;
    pre.font_size = 12.0;
    pre.line_height = 14.4;
    pre.white_space = WhiteSpace::Pre;
    let mut normal = pre.clone();
    normal.white_space = WhiteSpace::Normal;

    let runs = system.shape_styled_text_runs_with_parley(&[
        StyledTextSpan {
            text: "X",
            style: &pre,
        },
        StyledTextSpan {
            text: "\u{200b}",
            style: &normal,
        },
        StyledTextSpan {
            text: "X",
            style: &pre,
        },
    ]);

    assert!(
        !runs.is_empty(),
        "a font-neutral U+200B must not create an empty Parley style range"
    );
    assert_eq!(
        runs.iter().map(|run| run.text.as_ref()).collect::<String>(),
        "XX",
        "U+200B stays outside Parley's styled line-breaking buffer"
    );
    assert!(
        runs.iter()
            .flat_map(|run| &run.glyph_source_ranges)
            .any(|range| range.as_ref().is_some_and(|range| range == &(0..1)))
    );
    assert!(
        runs.iter()
            .flat_map(|run| &run.glyph_source_ranges)
            .any(|range| range.as_ref().is_some_and(|range| range == &(4..5)))
    );
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

    assert_eq!(
        rtl_glyph.rendered.painted_id(),
        mirrored_glyph.rendered.painted_id()
    );
    assert_ne!(
        rtl_glyph.rendered.painted_id(),
        ltr_glyph.rendered.painted_id()
    );
    assert_eq!(rtl_glyph.source_text(), ">");
}

#[test]
fn ltr_visual_slice_does_not_mirror_punctuation_from_its_fragment_direction() {
    let mut system = FontSystem::new();
    let mut rtl_fragment_style = ComputedStyle::initial();
    rtl_fragment_style.font_family = FontFamily::SansSerif;
    rtl_fragment_style.font_size = 12.0;
    rtl_fragment_style.line_height = 14.4;
    rtl_fragment_style.direction = Direction::Rtl;

    let fragment = system
        .shape_visual_ordered_line(
            ">",
            &rtl_fragment_style,
            rtl_fragment_style.line_height,
            ResolvedBidiDirection::Ltr,
        )
        .expect("visual text should shape");
    let mut ltr_reference_style = rtl_fragment_style.clone();
    ltr_reference_style.direction = Direction::Ltr;
    let ltr_reference = system
        .shape_visual_ordered_line(
            ">",
            &ltr_reference_style,
            ltr_reference_style.line_height,
            ResolvedBidiDirection::Ltr,
        )
        .expect("LTR reference should shape");

    let glyph = fragment
        .runs
        .iter()
        .flat_map(|run| run.glyphs.iter())
        .find(|glyph| glyph.source_text() == ">")
        .expect("source punctuation should emit a glyph");
    let reference_glyph = ltr_reference
        .runs
        .iter()
        .flat_map(|run| run.glyphs.iter())
        .find(|glyph| glyph.source_text() == ">")
        .expect("reference punctuation should emit a glyph");

    assert_eq!(
        glyph.rendered.painted_id(),
        reference_glyph.rendered.painted_id()
    );
}

#[test]
fn isolate_override_uses_first_strong_isolate_controls() {
    let mut style = ComputedStyle::initial();
    style.unicode_bidi = UnicodeBidi::IsolateOverride;

    style.direction = Direction::Ltr;
    assert_eq!(
        bidi_control_scope_for_style(&style),
        Some(("\u{2068}\u{202d}", "\u{202c}\u{2069}"))
    );

    style.direction = Direction::Rtl;
    assert_eq!(
        bidi_control_scope_for_style(&style),
        Some(("\u{2068}\u{202e}", "\u{202c}\u{2069}"))
    );
}

#[test]
fn upright_vertical_inline_scope_forces_ltr_bidi() {
    let mut style = ComputedStyle::initial();
    style.writing_mode = WritingMode::VerticalRl;
    style.direction = Direction::Rtl;
    style.text_orientation = TextOrientation::Upright;

    assert_eq!(
        bidi_control_scope_for_style(&style),
        Some(("\u{202d}", "\u{202c}"))
    );
}

#[test]
fn isolate_override_resolves_the_full_line_before_visual_paint_shaping() {
    let mut system = FontSystem::new();
    let mut style = ComputedStyle::initial();
    style.direction = Direction::Ltr;
    let text = "> \u{2068}\u{202e}אבגד > abcd\u{202c}\u{2069} >";

    let visual_text = system
        .visual_ranges_for_unwrapped_text(text, style.used_direction())
        .into_iter()
        .map(|range| text_without_bidi_format_controls(&text[range.range]).into_owned())
        .collect::<String>();

    assert_eq!(visual_text, "> dcba > דגבא >");
}

#[test]
fn cached_rtl_visual_slice_mirrors_glyph_without_rewriting_source_text() {
    let mut system = FontSystem::new();
    let mut style = ComputedStyle::initial();
    style.font_family = FontFamily::SansSerif;
    style.font_size = 12.0;
    style.line_height = 14.4;

    let mut cached = system
        .shape_unwrapped_line(">", &style, style.line_height)
        .and_then(|shaped| shaped.source_slice(0..1))
        .expect("logical source slice should shape");
    system.apply_resolved_bidi_glyph_mirroring(&mut cached, ResolvedBidiDirection::Rtl);
    let mirrored = system
        .shape_visual_ordered_line("<", &style, style.line_height, ResolvedBidiDirection::Ltr)
        .expect("mirrored LTR punctuation should shape");

    let cached_glyph = cached
        .runs
        .iter()
        .flat_map(|run| run.glyphs.iter())
        .find(|glyph| glyph.source_text() == ">")
        .expect("cached RTL source punctuation should emit a glyph");
    let mirrored_glyph = mirrored
        .runs
        .iter()
        .flat_map(|run| run.glyphs.iter())
        .find(|glyph| glyph.source_text() == "<")
        .expect("mirrored LTR punctuation should emit a glyph");

    assert_eq!(
        cached_glyph.rendered.painted_id(),
        mirrored_glyph.rendered.painted_id()
    );
    assert_eq!(cached_glyph.source_text(), ">");
}

#[test]
fn rtl_base_direction_marks_neutral_punctuation_as_an_rtl_visual_slice() {
    let mut system = FontSystem::new();
    let mut style = ComputedStyle::initial();
    style.direction = Direction::Rtl;
    let text = "> a > ב > c >";

    let ranges = system.visual_ranges_for_unwrapped_text(text, style.used_direction());

    assert!(ranges.iter().any(|visual_range| {
        text.get(visual_range.range.clone()) == Some(">")
            && visual_range.direction == ResolvedBidiDirection::Rtl
    }));
}

#[test]
fn isolate_controls_keep_the_outer_bidi_sequence_neutral() {
    let mut system = FontSystem::new();
    let mut style = ComputedStyle::initial();
    style.direction = Direction::Rtl;
    let text = "a - \u{2066}[1]\u{2069}...";

    let visual_text = system
        .visual_ranges_for_unwrapped_text(text, style.used_direction())
        .into_iter()
        .map(|range| text_without_bidi_format_controls(&text[range.range]).into_owned())
        .collect::<String>();

    assert_eq!(visual_text, "...[1] - a");
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

    let visual_ranges = system.visual_ranges_for_unwrapped_text("abc אבג", style.used_direction());
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
async fn font_face_variation_descriptor_is_merged_into_the_selected_shaping_style() {
    let stylesheet = parse_stylesheet(
        &Css::from_string(
            r#"@font-face {
                font-family: VariationFaceDefaults;
                src: url("WeasyPrint/tests/resources/weasyprint.otf");
                font-variation-settings: "wdth" 125, "wght" 600.7;
            }"#,
        )
        .with_base_path(".")
        .expect("current directory should be a valid file URL"),
    );
    let mut style = ComputedStyle::initial();
    style.font_family = FontFamily::Names(vec!["VariationFaceDefaults".to_string()]);
    let mut system = FontSystem::start_loading()
        .load_stylesheet_fonts(&[stylesheet])
        .finish()
        .await;

    let resolved = system.style_with_selected_face_variations(&style);

    assert_eq!(
        resolved.font_variation_settings,
        FontVariationSettings(vec![
            FontVariationSetting {
                tag: *b"wdth",
                value: 125.0_f32.to_bits(),
            },
            FontVariationSetting {
                tag: *b"wght",
                value: 600.7_f32.to_bits(),
            },
        ])
    );
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
async fn emoji_selectors_choose_distinct_generic_serif_faces() {
    let mut system = FontSystem::new();
    let mut style = ComputedStyle::initial();
    style.font_family = FontFamily::Serif;
    style.font_size = 40.0;
    style.line_height = 80.0;
    let text_source = system.emoji_presentation_family_source("\u{263a}\u{fe0e}", &style);
    let emoji_source = system.emoji_presentation_family_source("\u{263a}\u{fe0f}", &style);

    let text_face = system
        .shape_text_runs_with_parley("\u{263a}\u{fe0e}", &style)
        .into_iter()
        .find_map(|run| run.font_id)
        .expect("text-presentation serif face");
    let emoji_face = system
        .shape_text_runs_with_parley("\u{263a}\u{fe0f}", &style)
        .into_iter()
        .find_map(|run| run.font_id)
        .expect("emoji-presentation fallback face");
    assert_ne!(
        text_face, emoji_face,
        "variation selectors must select distinct text and emoji presentation faces: text_source={text_source:?}, emoji_source={emoji_source:?}"
    );
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
async fn font_size_adjust_from_font_keeps_its_primary_metric_across_unicode_range_fallback() {
    let stylesheet = parse_stylesheet(
        &Css::from_string(
            r#"@font-face {
                font-family: SpaceOnlyAhem;
                src: url("tests/fixtures/wpt/css/css-fonts/Ahem.ttf");
                unicode-range: U+0020;
            }"#,
        )
        .with_base_path(".")
        .expect("current directory should be a valid file URL"),
    );
    let mut system = FontSystem::start_loading()
        .load_stylesheet_fonts(&[stylesheet])
        .finish()
        .await;
    let mut from_font = ComputedStyle::initial();
    from_font.font_size = 50.0;
    from_font.line_height = 50.0;
    from_font.font_family = FontFamily::List(vec![
        FontFamily::Names(vec!["SpaceOnlyAhem".to_string()]),
        FontFamily::Serif,
    ]);
    from_font.font_size_adjust = FontSizeAdjust::Value {
        metric: FontSizeAdjustMetric::ExHeight,
        value: FontSizeAdjustValue::FromFont,
    };
    let primary = system
        .resolve_metric_font_for_style(&from_font)
        .expect("space-only primary face");
    assert_eq!(
        system
            .document_fonts
            .get(primary)
            .expect("primary font")
            .family,
        "SpaceOnlyAhem"
    );
    let mut explicit = from_font.clone();
    explicit.font_size_adjust = FontSizeAdjust::Value {
        metric: FontSizeAdjustMetric::ExHeight,
        // Ahem's fallback x-height is 0.8em; use its known local metric as
        // the explicit control for the `from-font` primary-face path.
        value: FontSizeAdjustValue::Number(0.8),
    };

    let from_font_run = system
        .shape_text_runs_with_parley("foobar", &from_font)
        .into_iter()
        .next()
        .expect("range-fallback run");
    let explicit_run = system
        .shape_text_runs_with_parley("foobar", &explicit)
        .into_iter()
        .next()
        .expect("explicit-aspect run");
    assert!(
        (from_font_run.font_size - explicit_run.font_size).abs() < 0.01,
        "from-font must retain the primary face's aspect value after Unicode-range fallback: from-font={from_font_run:?}, explicit={explicit_run:?}"
    );
}

#[tokio::test]
async fn font_size_adjust_keeps_explicit_line_height_computed_size() {
    let (mut system, mut style) = feature_probe_font_system().await;
    style.line_height = 20.0;
    style.line_height_value = ComputedLineHeight::from_points(20.0);
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
async fn same_face_bold_request_does_not_infer_synthesis_without_fontique_match() {
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
    assert_eq!(normal_font.post_script_name, bold_font.post_script_name);
    assert!(!normal_font.synthesis.embolden);
    assert!(
        !bold_font.synthesis.embolden,
        "a bold request alone is not evidence that Fontique selected faux bold"
    );
}

#[test]
fn source_slice_preserves_metadata_and_leaves_the_source_runs_unchanged() {
    fn glyph(
        unicode: &str,
        advance: f32,
        source_range: std::ops::Range<usize>,
    ) -> ShapedInlineGlyph {
        ShapedInlineGlyph {
            rendered: RenderedGlyph {
                kind: RenderedGlyphKind::Paint(1),
                x_advance: advance,
                nominal_x_advance: advance,
                x_offset: 0.0,
                y_offset: 0.0,
                unicode: unicode.into(),
            },
            paints: true,
            source_range: Some(source_range),
        }
    }

    let source = ShapedInlineLine {
        text: Rc::from("abcd"),
        width: 99.0,
        offset: 7.0,
        aligned_by_parley: true,
        line_height: 18.0,
        baseline_adjustment: 3.0,
        runs: vec![
            ShapedInlineRun {
                text: Rc::from("ab"),
                x_offset: 1.0,
                font_size: 12.0,
                font_id: Some(4),
                font_palette: FontPalette::Normal,
                glyphs: vec![glyph("a", 2.0, 0..1), glyph("b", 3.0, 1..2)],
                paints: true,
            },
            ShapedInlineRun {
                text: Rc::from("cd"),
                x_offset: 6.0,
                font_size: 14.0,
                font_id: Some(5),
                font_palette: FontPalette::Named("accent".into()),
                glyphs: vec![glyph("c", 4.0, 2..3), glyph("d", 5.0, 3..4)],
                paints: true,
            },
        ],
    };
    let original = source.clone();

    let slice = source
        .source_slice(1..3)
        .expect("range covers complete clusters");
    assert_eq!(
        source.source_range_advance_width(1..3),
        Some(slice.advance_width()),
        "measurement-only source ranges must match durable source slices"
    );

    assert_eq!(source, original, "slicing must not mutate the source shape");
    assert_eq!(slice.text.as_ref(), "bc");
    assert_eq!(slice.offset, source.offset);
    assert_eq!(slice.aligned_by_parley, source.aligned_by_parley);
    assert_eq!(slice.line_height, source.line_height);
    assert_eq!(slice.baseline_adjustment, source.baseline_adjustment);
    assert_eq!(slice.width, slice.advance_width());
    assert_eq!(slice.runs.len(), 2);
    assert_eq!(slice.runs[0].text.as_ref(), "b");
    assert_eq!(slice.runs[1].text.as_ref(), "c");
    assert_eq!(slice.runs[0].glyphs[0].source_range, Some(0..1));
    assert_eq!(slice.runs[1].glyphs[0].source_range, Some(1..2));
    assert_eq!(slice.runs[0].font_id, Some(4));
    assert_eq!(slice.runs[1].font_id, Some(5));
    assert_eq!(
        slice.runs[1].font_palette,
        FontPalette::Named("accent".into())
    );
}

#[test]
fn rendered_runs_preserve_ligature_source_text_with_actual_text() {
    let glyph = |unicode: &str| ShapedInlineGlyph {
        rendered: RenderedGlyph {
            kind: RenderedGlyphKind::Paint(42),
            x_advance: 7.0,
            nominal_x_advance: 7.0,
            x_offset: 0.0,
            y_offset: 0.0,
            unicode: unicode.to_owned(),
        },
        paints: true,
        source_range: None,
    };
    let ligature = ShapedInlineRun {
        text: Rc::from("fi"),
        x_offset: 0.0,
        font_size: 12.0,
        font_id: Some(0),
        font_palette: FontPalette::Normal,
        // HarfBuzz can attach the fi glyph to its first source cluster.
        glyphs: vec![glyph("f")],
        paints: true,
    };
    let faithful = ShapedInlineRun {
        text: Rc::from("fit"),
        x_offset: 0.0,
        font_size: 12.0,
        font_id: Some(0),
        font_palette: FontPalette::Normal,
        glyphs: vec![glyph("f"), glyph("i"), glyph("t")],
        paints: true,
    };

    assert_eq!(ligature.rendered_run().actual_text.as_deref(), Some("fi"));
    assert_eq!(faithful.rendered_run().actual_text, None);
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
        .map(|glyph| glyph.rendered.painted_id().expect("paintable glyph"))
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
            .map(|glyph| glyph.rendered.painted_id().expect("paintable glyph"))
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
    oblique.font_style = FontStyle::DEFAULT_OBLIQUE;
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
    assert!(!system.document_fonts.font_has_character(face_a, ' '));
    assert!(system.document_fonts.font_has_character(face_b, '\u{0628}'));
    assert!(!system.document_fonts.font_has_character(face_b, '\u{0627}'));
}

#[tokio::test]
async fn unicode_range_excludes_a_face_from_first_available_font_metrics() {
    let stylesheet = parse_stylesheet(
        &Css::from_string(
            r#"@font-face {
                font-family: NoSpace;
                src: url("tests/resources/fonts/NotoNaskhArabic-regular.woff2");
                unicode-range: U+0627;
            }
            @font-face {
                font-family: WithSpace;
                src: url("tests/resources/fonts/NotoNaskhArabic-regular.woff2");
            }"#,
        )
        .with_base_path(".")
        .expect("current directory should be a valid file URL"),
    );
    let mut system = FontSystem::start_loading()
        .load_stylesheet_fonts(&[stylesheet])
        .finish()
        .await;
    let mut style = ComputedStyle::initial();
    style.font_family = FontFamily::List(vec![
        FontFamily::Names(vec!["NoSpace".to_string()]),
        FontFamily::Names(vec!["WithSpace".to_string()]),
    ]);

    let first_available = system
        .resolve_metric_font_for_style(&style)
        .expect("the second face provides U+0020");
    assert_eq!(
        system.document_fonts.get(first_available).unwrap().family,
        "WithSpace"
    );
}

#[tokio::test]
async fn missing_glyph_zero_does_not_cover_tab_space_metrics_or_paint_tabs() {
    let stylesheet = parse_stylesheet(
        &Css::from_string(
            r#"@font-face {
                font-family: NoSpace;
                src: url("tests/resources/fonts/CanvasTest-nospace.ttf");
            }
            @font-face {
                font-family: WithSpace;
                src: url("tests/resources/fonts/noto-sans-v8-latin-regular.woff");
            }"#,
        )
        .with_base_path(".")
        .expect("current directory should be a valid file URL"),
    );
    let mut system = FontSystem::start_loading()
        .load_stylesheet_fonts(&[stylesheet])
        .finish()
        .await;
    let mut style = ComputedStyle::initial();
    style.font_family = FontFamily::Names(vec!["NoSpace".to_string(), "WithSpace".to_string()]);
    style.white_space = WhiteSpace::Pre;

    let no_space = system
        .resolve_font_family(
            &FontFamily::Names(vec!["NoSpace".to_string()]),
            style.font_weight,
            style.font_style,
            style.font_width,
        )
        .expect("the no-space test face resolves");
    assert!(
        !system.document_fonts.font_has_character(no_space, ' '),
        "a cmap mapping to `.notdef` must not count as U+0020 coverage"
    );
    let matched = system
        .character_font_match(&style, ' ')
        .expect("the later stack face supplies U+0020");
    assert_eq!(
        system.document_fonts.get(matched.font_id).unwrap().family,
        "WithSpace"
    );
    assert_ne!(matched.glyph_id.raw().0, 0);

    let shaped = system
        .shape_unwrapped_line("\t", &style, style.line_height)
        .expect("a preserved tab shapes as a layout advance");
    let tab = shaped
        .runs
        .iter()
        .flat_map(|run| &run.glyphs)
        .find(|glyph| glyph.source_text() == "\t")
        .expect("tab source remains in the shaped line");
    assert!(tab.rendered.is_advance_only());
    assert!(tab.rendered.x_advance > 0.0);
    assert!(!tab.paints);
}

#[tokio::test]
async fn font_face_feature_defaults_retain_each_declared_tag() {
    let stylesheet = parse_stylesheet(
        &Css::from_string(
            r#"@font-face {
                font-family: FeatureDefaults;
                src: url("WeasyPrint/tests/resources/weasyprint.otf");
                font-feature-settings: "liga" on, "clig" on, "calt" on, "hlig" on,
                    "dlig" on, "onum" on, "smcp" on, "jp90" on;
            }"#,
        )
        .with_base_path(".")
        .expect("current directory should be a valid file URL"),
    );
    assert_eq!(stylesheet.font_faces[0].font_feature_settings.0.len(), 8);

    let mut system = FontSystem::start_loading()
        .load_stylesheet_fonts(&[stylesheet])
        .finish()
        .await;
    let mut style = ComputedStyle::initial();
    style.font_family = FontFamily::Names(vec!["FeatureDefaults".to_string()]);
    let context = system
        .font_feature_context_for_style(&style)
        .expect("@font-face descriptor creates a shaping context");
    assert_eq!(
        context
            .face_defaults
            .expect("selected face keeps its descriptor defaults")
            .font_feature_settings
            .0
            .len(),
        8
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
        .map(|glyph| glyph.painted_id().expect("paintable glyph"))
        .collect::<Vec<_>>();

    assert_ne!(glyph_ids.first(), isolated_beh.first(), "{glyph_ids:?}");
}

#[tokio::test]
async fn styled_zwnj_fragment_suppresses_arabic_joining_with_a_different_font() {
    let stylesheet = parse_stylesheet(
        &Css::from_string(
            r#"@font-face {
                font-family: AlreqNaskh;
                src: url("tests/resources/fonts/NotoNaskhArabic-regular.woff2");
            }
            @font-face {
                font-family: AlreqJoinControls;
                src: url("tests/resources/fonts/noto-sans-v8-latin-regular.woff");
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
    let mut join_control = arabic.clone();
    join_control.font_family = FontFamily::Names(vec!["AlreqJoinControls".to_string()]);
    join_control.line_height = 0.0;
    let mut system = FontSystem::start_loading()
        .load_stylesheet_fonts(&[stylesheet])
        .finish()
        .await;

    let expected = shaped_glyph_ids(&mut system, "\u{0640}\u{fe8d}", &arabic);
    let shaped = system.shape_styled_text_runs_with_parley(&[
        StyledTextSpan {
            text: "\u{0640}",
            style: &arabic,
        },
        StyledTextSpan {
            text: "\u{200c}",
            style: &join_control,
        },
        StyledTextSpan {
            text: "\u{0627}",
            style: &arabic,
        },
    ]);
    let actual = shaped
        .into_iter()
        .flat_map(|run| run.glyphs)
        .filter(|glyph| glyph.x_advance != 0.0)
        .map(|glyph| glyph.painted_id().expect("paintable glyph"))
        .collect::<Vec<_>>();

    assert_eq!(
        actual, expected,
        "explicit ZWNJ must preserve no-join shaping"
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
        .flat_map(|run| run.glyphs)
        .next()
        .unwrap();
    let en_space_glyph = system
        .shape_text_runs_with_parley("\u{2002}", &style)
        .into_iter()
        .flat_map(|run| run.glyphs)
        .next()
        .unwrap();

    assert_eq!(en_space_glyph.painted_id(), space_glyph.painted_id());
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
async fn measured_line_breaks_do_not_leave_nonstarters_at_line_start() {
    // The CSS Text i18n NS reftests exercise all of these scalars. The CJK
    // ideograph/word fallback must not reintroduce a boundary that UAX #14
    // forbids before the Nonstarter class.
    // <https://www.unicode.org/reports/tr14/#LB13>
    const NONSTARTERS: [char; 9] = [
        '\u{3005}', '\u{303b}', '\u{303c}', '\u{309d}', '\u{309e}', '\u{30fd}', '\u{30fe}',
        '\u{ff9e}', '\u{ff9f}',
    ];

    for nonstarter in NONSTARTERS {
        let text = format!("中中中{nonstarter}文");
        let prohibited_boundary = text.find(nonstarter).unwrap();
        let ordinary_boundary = "中中".len();

        for word_break in [CssWordBreak::Normal, CssWordBreak::BreakAll] {
            let mut style = ComputedStyle::initial();
            style.font_family = FontFamily::SansSerif;
            style.word_break = word_break;
            let breaks = measured_break_opportunities(&text, &style);

            assert!(
                breaks.contains(&ordinary_boundary),
                "{nonstarter:?}: {breaks:?}"
            );
            assert!(
                !breaks.contains(&prohibited_boundary),
                "{nonstarter:?}: {breaks:?}"
            );
        }
    }
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
async fn normal_thai_named_entities_remain_one_complex_context_unit() {
    let mut style = ComputedStyle::initial();
    style.language = ContentLanguage::from_html_attribute("th");
    let text = "กรุงเทพคือสวยงาม";

    assert_eq!(
        measured_break_opportunities(text, &style),
        vec!["กรุงเทพ".len(), "กรุงเทพคือ".len(), text.len()]
    );
}

#[tokio::test]
async fn auto_phrase_uses_icu_word_boundaries_for_known_languages() {
    let mut style = ComputedStyle::initial();
    style.word_break = CssWordBreak::AutoPhrase;
    style.language = ContentLanguage::from_html_attribute("ja");
    let text = "東京へ行きましょう。";

    let auto_phrase = measured_break_opportunities(text, &style);
    style.word_break = CssWordBreak::Normal;
    let normal = measured_break_opportunities(text, &style);
    let icu_word_boundaries = WordSegmenter::new_auto(WordBreakInvariantOptions::default())
        .segment_str(text)
        .collect::<Vec<_>>();

    assert!(
        auto_phrase.len() < normal.len(),
        "{auto_phrase:?} vs {normal:?}"
    );
    assert_eq!(auto_phrase, vec!["東京へ".len(), text.len()]);
    assert!(auto_phrase.iter().all(|position| {
        *position == text.len()
            || !keep_all_suppresses_break_between(
                text[..*position].chars().next_back().unwrap(),
                text[*position..].chars().next().unwrap(),
            )
            || icu_word_boundaries.binary_search(position).is_ok()
    }));
}

#[tokio::test]
async fn auto_phrase_falls_back_to_normal_without_a_known_language() {
    let text = "東京へ行きましょう。";
    let mut auto_phrase = ComputedStyle::initial();
    auto_phrase.word_break = CssWordBreak::AutoPhrase;
    let normal = ComputedStyle::initial();

    assert_eq!(
        measured_break_opportunities(text, &auto_phrase),
        measured_break_opportunities(text, &normal)
    );
}

#[tokio::test]
async fn auto_phrase_uses_kham_thai_named_entity_boundaries() {
    let text = "กรุงเทพคือสวยงาม";
    let mut style = ComputedStyle::initial();
    style.word_break = CssWordBreak::AutoPhrase;
    style.language = ContentLanguage::from_html_attribute("th");
    let breaks = measured_break_opportunities(text, &style);
    let mut expected = phrase_boundaries(text, AutoPhraseLanguage::Thai)
        .expect("declared Thai has Kham phrase analysis")
        .boundary_offsets()
        .to_vec();
    expected.push(text.len());
    assert_eq!(breaks, expected);
}

#[tokio::test]
async fn auto_phrase_keeps_gl_wj_and_zwj_boundaries_protected() {
    let mut style = ComputedStyle::initial();
    style.word_break = CssWordBreak::AutoPhrase;
    style.language = ContentLanguage::from_html_attribute("ja");

    for text in [
        "東京\u{00a0}へ\u{00a0}行きましょう。",
        "東京\u{2060}へ\u{2060}行きましょう。",
        "東京\u{200d}へ\u{200d}行きましょう。",
    ] {
        let breaks = measured_break_opportunities(text, &style);
        for (offset, character) in text.char_indices() {
            if matches!(character, '\u{00a0}' | '\u{2060}' | '\u{200d}') {
                assert!(!breaks.contains(&offset), "{text:?}: {breaks:?}");
                assert!(
                    !breaks.contains(&(offset + character.len_utf8())),
                    "{text:?}: {breaks:?}"
                );
            }
        }
    }
}

#[tokio::test]
async fn auto_phrase_suppresses_authored_soft_hyphens() {
    let mut style = ComputedStyle::initial();
    style.word_break = CssWordBreak::AutoPhrase;
    style.language = ContentLanguage::from_html_attribute("en");
    let text = "con\u{00ad}sid\u{00ad}eration";

    assert_eq!(
        text_with_hyphenation_controls(text, &style),
        "consideration"
    );
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
fn soft_hyphen_does_not_suppress_a_later_space_wrap() {
    let style = ComputedStyle::initial();
    let text = "Deoxy\u{00ad}ribo\u{00ad}nucleic acid";

    let opportunities = measured_break_opportunities(text, &style);

    assert!(
        opportunities
            .binary_search(&"Deoxy\u{00ad}ribo\u{00ad}nucleic ".len())
            .is_ok(),
        "a later ordinary space remains a line-break opportunity after a soft hyphen"
    );
}

#[test]
fn soft_hyphen_remains_a_break_before_line_start_prohibited_punctuation() {
    let style = ComputedStyle::initial();
    let text = "tú\u{00ad}’àn";

    assert!(
        measured_break_opportunities(text, &style)
            .binary_search(&"tú\u{00ad}".len())
            .is_ok()
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

#[test]
fn shaped_terminal_tracking_is_removed_from_the_glyph_advance() {
    let mut system = FontSystem::new();
    let mut style = ComputedStyle::initial();
    style.font_family = FontFamily::Monospace;
    style.font_size = 12.0;
    style.letter_spacing = ComputedLengthPercentage::from_points(10.0);
    let mut shaped = system
        .shape_unwrapped_line("ab", &style, style.line_height)
        .expect("text shapes");
    let tracked_width = shaped.advance_width();

    shaped.remove_terminal_letter_spacing(style.used_letter_spacing().points());

    assert!(
        (shaped.advance_width() - (tracked_width - 10.0)).abs() < 0.01,
        "terminal tracking must be removed from the durable glyph artifact"
    );
}

#[test]
fn untracked_inline_shaping_suppresses_backend_letter_spacing() {
    let mut system = FontSystem::new();
    let mut tracked = ComputedStyle::initial();
    tracked.font_family = FontFamily::Monospace;
    tracked.font_size = 12.0;
    tracked.letter_spacing = ComputedLengthPercentage::from_points(10.0);
    let mut untracked = tracked.clone();
    untracked.letter_spacing = ComputedLengthPercentage::ZERO;

    for text in ["ab", "\u{200b}\u{200c}\u{200d}\u{feff}\u{200e}\u{2066}xx"] {
        let actual = system
            .shape_untracked_inline_line(text, &tracked, tracked.line_height)
            .expect("untracked inline text shapes");
        let expected = system
            .shape_unwrapped_line(text, &untracked, untracked.line_height)
            .expect("zero-spacing text shapes");

        assert!(
            (actual.advance_width() - expected.advance_width()).abs() < 0.01,
            "untracked shaping must match a style whose used letter spacing is zero for {text:?}"
        );
    }
}

#[test]
fn zero_width_space_is_font_neutral_during_tracked_shaping() {
    let mut system = FontSystem::new();
    let mut tracked = ComputedStyle::initial();
    tracked.font_family = FontFamily::Monospace;
    tracked.font_size = 12.0;
    tracked.letter_spacing = ComputedLengthPercentage::from_points(10.0);
    let mut untracked = tracked.clone();
    untracked.letter_spacing = ComputedLengthPercentage::ZERO;

    let mut shaped = system
        .shape_unwrapped_line("2\u{200b}", &tracked, tracked.line_height)
        .expect("text shapes");
    shaped.remove_terminal_letter_spacing(tracked.used_letter_spacing().points());

    let base = system
        .shape_unwrapped_line("2", &untracked, untracked.line_height)
        .expect("base text shapes");
    assert!((shaped.advance_width() - base.advance_width()).abs() < 0.01);
}

#[test]
fn styled_palette_boundaries_remain_separate_paint_runs() {
    let mut system = FontSystem::new();
    let mut first = ComputedStyle::initial();
    first.font_family = FontFamily::Monospace;
    first.font_palette = FontPalette::Named("--first".to_string());
    let mut second = first.clone();
    second.font_palette = FontPalette::Named("--second".to_string());

    let runs = system.shape_styled_text_runs_with_parley(&[
        StyledTextSpan {
            text: "A",
            style: &first,
        },
        StyledTextSpan {
            text: "B",
            style: &second,
        },
    ]);

    assert_eq!(runs.len(), 2);
    assert_eq!(runs[0].font_palette, first.font_palette);
    assert_eq!(runs[1].font_palette, second.font_palette);
}

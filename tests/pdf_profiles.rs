//! Integration tests for supported PDF profiles and compression behavior.

use base64::Engine as _;
use moxcms::{ColorProfile, ToneReprCurve};
use quire::{Document, Html, PdfCompression, PdfOptions, PdfProfile, RenderOptions};

trait PdfBytesForTest {
    fn write_pdf_bytes(&self, options: &PdfOptions) -> quire::Result<Vec<u8>>;
}

impl PdfBytesForTest for Document {
    fn write_pdf_bytes(&self, options: &PdfOptions) -> quire::Result<Vec<u8>> {
        let mut bytes = Vec::new();
        self.write_pdf(&mut bytes, options)?;
        Ok(bytes)
    }
}

trait HtmlPdfBytesForTest {
    async fn write_pdf_bytes(
        &self,
        render_options: &RenderOptions,
        pdf_options: &PdfOptions,
    ) -> quire::Result<Vec<u8>>;
}

impl HtmlPdfBytesForTest for Html {
    async fn write_pdf_bytes(
        &self,
        render_options: &RenderOptions,
        pdf_options: &PdfOptions,
    ) -> quire::Result<Vec<u8>> {
        let mut bytes = Vec::new();
        self.write_pdf(&mut bytes, render_options, pdf_options)
            .await?;
        Ok(bytes)
    }
}

fn wide_gamut_profile() -> Vec<u8> {
    let mut profile = ColorProfile::new_display_p3();
    let curve = ToneReprCurve::Parametric(vec![2.0]);
    profile.red_trc = Some(curve.clone());
    profile.green_trc = Some(curve.clone());
    profile.blue_trc = Some(curve);
    profile.encode().unwrap()
}

fn tagged_png_data_url(profile: &[u8]) -> String {
    let mut image = Vec::new();
    let mut info = png::Info::with_size(1, 1);
    info.color_type = png::ColorType::Rgb;
    info.icc_profile = Some(profile.to_vec().into());
    let mut writer = png::Encoder::with_info(&mut image, info)
        .unwrap()
        .write_header()
        .unwrap();
    writer.write_image_data(&[230, 32, 16]).unwrap();
    drop(writer);
    format!(
        "data:image/png;base64,{}",
        base64::engine::general_purpose::STANDARD.encode(image)
    )
}

#[tokio::test]
async fn pdf_profiles_round_trip_and_select_expected_writer_output() {
    let profiles = [
        ("pdf", PdfProfile::Pdf, "%PDF-1.4", None),
        ("pdf/a-1b", PdfProfile::PdfA1B, "%PDF-1.4", Some(("1", "B"))),
        ("pdf/a-2b", PdfProfile::PdfA2B, "%PDF-1.7", Some(("2", "B"))),
        ("pdf/a-3b", PdfProfile::PdfA3B, "%PDF-1.7", Some(("3", "B"))),
        ("pdf/a-2u", PdfProfile::PdfA2U, "%PDF-1.7", Some(("2", "U"))),
        ("pdf/a-3u", PdfProfile::PdfA3U, "%PDF-1.7", Some(("3", "U"))),
    ];

    assert_eq!(PdfProfile::default(), PdfProfile::Pdf);
    for (name, expected_profile, pdf_header, pdfa_identification) in profiles {
        let profile = name.parse::<PdfProfile>().unwrap();
        assert_eq!(profile, expected_profile);
        assert_eq!(profile.to_string(), name);

        let options = PdfOptions {
            profile,
            compression: PdfCompression::Uncompressed,
            ..PdfOptions::default()
        };
        let pdf = Html::from_string("<p>PDF profile test</p>")
            .write_pdf_bytes(&RenderOptions::default(), &options)
            .await
            .unwrap();
        let pdf_text = String::from_utf8_lossy(&pdf);

        assert!(pdf_text.starts_with(pdf_header));
        match pdfa_identification {
            Some((part, conformance)) => {
                assert!(pdf_text.contains(&format!(r#"pdfaid:part="{part}""#)));
                assert!(pdf_text.contains(&format!(r#"pdfaid:conformance="{conformance}""#)));
            }
            None => assert!(!pdf_text.contains("pdfaid")),
        }
    }
}

#[tokio::test]
async fn rendered_document_can_be_serialized_with_distinct_pdf_policies() {
    let document = Html::from_string("<title>Reusable</title><p>PDF options</p>")
        .render(&RenderOptions::default())
        .await
        .unwrap();

    let default_pdf = document.write_pdf_bytes(&PdfOptions::default()).unwrap();
    let alternate_options = PdfOptions {
        profile: PdfProfile::Pdf,
        font_embedding: quire::FontEmbeddingMode::Full,
        compression: PdfCompression::Uncompressed,
        producer: "Quire integration test".to_string(),
    };
    let alternate_pdf = document.write_pdf_bytes(&alternate_options).unwrap();
    let alternate_text = String::from_utf8_lossy(&alternate_pdf);

    assert_eq!(document.metadata().title(), Some("Reusable"));
    assert!(default_pdf.starts_with(b"%PDF-1.4"));
    assert!(alternate_pdf.starts_with(b"%PDF-1.4"));
    assert!(alternate_text.contains("/Producer (Quire integration test)"));
    assert!(!alternate_text.contains("/FlateDecode"));
    assert_ne!(default_pdf, alternate_pdf);
}

#[tokio::test]
async fn ordinary_pdf_retains_display_p3_vector_paint_as_iccbased() {
    let options = PdfOptions {
        profile: PdfProfile::Pdf,
        compression: PdfCompression::Uncompressed,
        ..PdfOptions::default()
    };
    let pdf = Html::from_string(
        "<style>p { color: color(display-p3 1 .2 0); border: 2pt solid color(display-p3 0 1 .2); background: color(display-p3 .1 .2 1) }</style><p>P3</p>",
    )
    .write_pdf_bytes(&RenderOptions::default(), &options)
    .await
    .unwrap();
    let text = String::from_utf8_lossy(&pdf);

    assert!(text.contains("/CSDisplayP3"), "{text}");
    assert!(text.contains("/ICCBased"), "{text}");
    assert!(text.contains("/CSDisplayP3 cs"), "{text}");
    assert!(!text.contains(" rg\n"));
}

#[tokio::test]
async fn ordinary_pdf_plans_final_p3_resources_for_wide_oklab_paint() {
    let options = PdfOptions {
        profile: PdfProfile::Pdf,
        compression: PdfCompression::Uncompressed,
        ..PdfOptions::default()
    };
    let pdf = Html::from_string(
        "<style>@page { size: 100pt 100pt; margin: 0 } body { margin: 0 } \
         div { width: 100pt; height: 100pt; background: oklab(1 .5 .2) }</style><div></div>",
    )
    .write_pdf_bytes(&RenderOptions::default(), &options)
    .await
    .unwrap();
    let text = String::from_utf8_lossy(&pdf);

    assert!(text.contains("/CSDisplayP3 [/ICCBased"), "{text}");
    assert!(text.contains("/CSDisplayP3 cs"), "{text}");
    assert!(!text.contains("/CSXYZD50"), "{text}");
}

#[tokio::test]
async fn ordinary_pdf_declares_the_final_p3_resource_for_wide_xyz_direct_paint() {
    let options = PdfOptions {
        profile: PdfProfile::Pdf,
        compression: PdfCompression::Uncompressed,
        ..PdfOptions::default()
    };

    // CSS Color 4's `xyz` alias is D65. These are all the XYZ encodings of
    // Display-P3 green, which is outside sRGB but exactly representable by the
    // final ordinary-PDF Display-P3 paint condition.
    // <https://www.w3.org/TR/css-color-4/#predefined>
    for (space, components) in [
        ("xyz", "0.26567 0.69174 0.04511"),
        ("xyz-d50", "0.29194 0.692236 0.041884"),
        ("xyz-d65", "0.26567 0.69174 0.04511"),
    ] {
        let pdf = Html::from_string(format!(
            "<!DOCTYPE html><style>\
             .test {{ background-color: red; width: 12em; height: 12em; }}
             .test {{ background-color: color({space} {components}); }} /* wide P3 green */</style>\
             <body><p>XYZ direct paint</p><div class=\"test\"></div></body>"
        ))
        .write_pdf_bytes(&RenderOptions::default(), &options)
        .await
        .unwrap();
        let text = String::from_utf8_lossy(&pdf);

        assert!(
            text.contains("/CSDisplayP3 [/ICCBased"),
            "{space} must declare its final Display-P3 color-space resource: {text}"
        );
        assert!(
            text.contains("/CSDisplayP3 cs"),
            "{space} must select the declared Display-P3 resource for direct paint: {text}"
        );
        assert!(
            !text.contains("/CSXYZD50"),
            "{space} direct paint must not plan a D50 XYZ resource: {text}"
        );
    }
}

#[tokio::test]
async fn ordinary_pdf_keeps_in_gamut_oklab_paint_in_srgb() {
    let options = PdfOptions {
        profile: PdfProfile::Pdf,
        compression: PdfCompression::Uncompressed,
        ..PdfOptions::default()
    };
    let pdf = Html::from_string(
        "<style>@page { size: 100pt 100pt; margin: 0 } body { margin: 0 } \
         div { width: 100pt; height: 100pt; background: oklab(51.975% -.1403 .10768) }</style><div></div>",
    )
    .write_pdf_bytes(&RenderOptions::default(), &options)
    .await
    .unwrap();
    let text = String::from_utf8_lossy(&pdf);

    assert!(text.contains("/CSsRGB cs"), "{text}");
    assert!(!text.contains("/CSDisplayP3"), "{text}");
}

#[tokio::test]
async fn pdfa_vector_paint_uses_tagged_srgb_output_intent() {
    let options = PdfOptions {
        profile: PdfProfile::PdfA2B,
        compression: PdfCompression::Uncompressed,
        ..PdfOptions::default()
    };
    let pdf = Html::from_string(
        "<style>p { color: color(display-p3 1 .2 0); border: 2pt solid color(display-p3 0 1 .2); background: oklab(1 .5 .2) }</style><p>P3</p>",
    )
    .write_pdf_bytes(&RenderOptions::default(), &options)
    .await
    .unwrap();
    let text = String::from_utf8_lossy(&pdf);

    assert!(text.contains("/OutputIntents"));
    assert!(text.contains("/DestOutputProfile"));
    assert!(text.contains("/CSsRGB cs"), "{text}");
    assert!(!text.contains("/CSDisplayP3 cs"));
}

#[tokio::test]
async fn ordinary_pdf_gradients_use_managed_rgb_iccbased_spaces() {
    let options = PdfOptions {
        profile: PdfProfile::Pdf,
        compression: PdfCompression::Uncompressed,
        ..PdfOptions::default()
    };
    let pdf = Html::from_string(
        "<style>\
         .p3 { width: 120pt; height: 20pt; background: linear-gradient(to right, color(display-p3 1 .2 0), color(display-p3 0 .2 1)); }\
         .mixed { width: 120pt; height: 20pt; background: radial-gradient(color(display-p3 1 .2 0), color(rec2020 .1 .3 1)); }\
         .transparent { width: 120pt; height: 20pt; background: linear-gradient(color(display-p3 1 .2 0 / .2), color(display-p3 0 .2 1 / .8)); }\
         </style><div class=p3></div><div class=mixed></div><div class=transparent></div>",
    )
    .write_pdf_bytes(&RenderOptions::default(), &options)
    .await
    .unwrap();
    let text = String::from_utf8_lossy(&pdf);

    assert!(text.contains("/CSDisplayP3"), "{text}");
    assert!(text.contains("/ICCBased"), "{text}");
    assert!(text.contains("/SMask"), "{text}");
    assert!(!text.contains("/DeviceRGB"), "{text}");
}

#[tokio::test]
async fn ordinary_pdf_wide_pcs_gradient_uses_its_p3_profile_for_the_shading() {
    let options = PdfOptions {
        profile: PdfProfile::Pdf,
        compression: PdfCompression::Uncompressed,
        ..PdfOptions::default()
    };
    let pdf = Html::from_string(
        "<style>@page { size: 120pt 40pt; margin: 0 } body { margin: 0 } \
         div { width: 120pt; height: 40pt; background: linear-gradient(in xyz-d50, oklab(1 .5 .2), oklab(.9 .5 .2)) }</style><div></div>",
    )
    .write_pdf_bytes(&RenderOptions::default(), &options)
    .await
    .unwrap();
    let text = String::from_utf8_lossy(&pdf);
    let profile_reference = text
        .split("/CSDisplayP3 [/ICCBased ")
        .nth(1)
        .and_then(|suffix| suffix.split_once(" R"))
        .map(|(reference, _)| format!("{reference} R"))
        .expect("Display-P3 page resource");

    assert!(text.contains("/ShadingType 2"), "{text}");
    assert!(
        text.contains(&format!("/ColorSpace [/ICCBased {profile_reference}]")),
        "the shading must use the same ICC profile as its Display-P3 output: {text}"
    );
    assert!(!text.contains("/CSXYZD50"), "{text}");
}

#[tokio::test]
async fn pdfa_raster_gradient_uses_only_tagged_srgb() {
    let options = PdfOptions {
        profile: PdfProfile::PdfA2B,
        compression: PdfCompression::Uncompressed,
        ..PdfOptions::default()
    };
    let pdf = Html::from_string(
        "<style>div { width: 80pt; height: 80pt; } .native { background: linear-gradient(color(display-p3 1 .2 0), color(display-p3 0 1 .2)); } .raster { background: conic-gradient(color(display-p3 1 .2 0), color(display-p3 0 1 .2), color(display-p3 1 .2 0)); }</style><div class=native></div><div class=raster></div>",
    )
    .write_pdf_bytes(&RenderOptions::default(), &options)
    .await
    .unwrap();
    let text = String::from_utf8_lossy(&pdf);

    assert!(text.contains("/OutputIntents"), "{text}");
    assert!(text.contains("/CSsRGB"), "{text}");
    assert!(text.contains("/Subtype /Image"), "{text}");
    assert!(text.contains("/ColorSpace [/ICCBased"), "{text}");
    assert!(!text.contains("/DeviceRGB"), "{text}");
    assert!(!text.contains("CSDisplayP3"), "{text}");
}

#[tokio::test]
async fn ordinary_pdf_conic_gradient_uses_its_rgb_raster_output_tag() {
    let options = PdfOptions {
        profile: PdfProfile::Pdf,
        compression: PdfCompression::Uncompressed,
        ..PdfOptions::default()
    };
    let pdf = Html::from_string(
        "<style>div { width: 80pt; height: 80pt; background: conic-gradient(color(display-p3 1 .2 0), color(display-p3 0 1 .2), color(display-p3 1 .2 0)); }</style><div></div>",
    )
    .write_pdf_bytes(&RenderOptions::default(), &options)
    .await
    .unwrap();
    let text = String::from_utf8_lossy(&pdf);
    let raster_output_reference = text
        .split("/CSDisplayP3 [/ICCBased ")
        .nth(1)
        .and_then(|suffix| suffix.split_once(" R"))
        .map(|(reference, _)| format!("{reference} R"))
        .expect("raster output color-space page resource");

    assert!(text.contains("/Subtype /Image"), "{text}");
    assert!(
        text.contains(&format!(
            "/ColorSpace [/ICCBased {raster_output_reference}]"
        )),
        "the conic image must refer to its RGB output ICC profile: {text}"
    );
}

#[tokio::test]
async fn ordinary_pdf_generated_gradient_uses_its_rgb_raster_output_tag() {
    let options = PdfOptions {
        profile: PdfProfile::Pdf,
        compression: PdfCompression::Uncompressed,
        ..PdfOptions::default()
    };
    let pdf = Html::from_string(
        "<style>div { width: 80pt; height: 80pt; border: 12pt solid transparent; border-image: linear-gradient(color(display-p3 1 .2 0), color(display-p3 0 1 .2)) 1; }</style><div></div>",
    )
    .write_pdf_bytes(&RenderOptions::default(), &options)
    .await
    .unwrap();
    let text = String::from_utf8_lossy(&pdf);
    let raster_output_reference = text
        .split("/CSDisplayP3 [/ICCBased ")
        .nth(1)
        .and_then(|suffix| suffix.split_once(" R"))
        .map(|(reference, _)| format!("{reference} R"))
        .expect("raster output color-space page resource");

    assert!(text.contains("/Subtype /Image"), "{text}");
    assert!(
        text.contains(&format!(
            "/ColorSpace [/ICCBased {raster_output_reference}]"
        )),
        "the generated image must refer to its RGB output ICC profile: {text}"
    );
}

#[tokio::test]
async fn ordinary_pdf_promotes_an_embedded_uniform_raster_profile_once() {
    let profile = wide_gamut_profile();
    let image = tagged_png_data_url(&profile);
    let options = PdfOptions {
        profile: PdfProfile::Pdf,
        compression: PdfCompression::Uncompressed,
        ..PdfOptions::default()
    };
    let pdf = Html::from_string(format!(
        "<style>img {{ width: 20pt; height: 20pt; }}</style><img src=\"{image}\">"
    ))
    .write_pdf_bytes(&RenderOptions::default(), &options)
    .await
    .unwrap();
    let text = String::from_utf8_lossy(&pdf);

    assert!(!text.contains("/Subtype /Image"), "{text}");
    assert!(text.contains("/CSEmbeddedRgb1 [/ICCBased"), "{text}");
    assert!(text.contains("/CSEmbeddedRgb1 cs"), "{text}");
    assert_eq!(
        pdf.windows(profile.len())
            .filter(|window| *window == profile.as_slice())
            .count(),
        1,
        "the source ICC profile is embedded exactly once for the calibrated fill"
    );
}

#[tokio::test]
async fn pdfa_uniform_image_converts_embedded_profiles_to_tagged_srgb_fill() {
    let profile = wide_gamut_profile();
    let image = tagged_png_data_url(&profile);
    let options = PdfOptions {
        profile: PdfProfile::PdfA2B,
        compression: PdfCompression::Uncompressed,
        ..PdfOptions::default()
    };
    let pdf = Html::from_string(format!("<img src=\"{image}\">"))
        .write_pdf_bytes(&RenderOptions::default(), &options)
        .await
        .unwrap();
    let text = String::from_utf8_lossy(&pdf);

    assert!(text.contains("/OutputIntents"), "{text}");
    assert!(!text.contains("/Subtype /Image"), "{text}");
    assert!(text.contains("/CSsRGB cs"), "{text}");
    assert!(
        !pdf.windows(profile.len())
            .any(|window| window == profile.as_slice()),
        "PDF/A must not retain the source image profile"
    );
}

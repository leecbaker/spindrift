use super::*;
use crate::css::FontPalette;
use crate::document::PaintSpace;
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

/// A default-ignorable-only run that participated in shaping but must not
/// contribute visible fallback geometry.
///
/// Font fallback may select a face solely to map a ZWJ, ZWNJ, or another
/// default-ignorable control. The control remains part of the shaping stream,
/// but CSS Text does not give it visual advance. Parley has already included
/// that fallback run's advance in the following runs' visual offsets, so
/// conversion removes the advance from those offsets when the run is omitted.
/// <https://www.w3.org/TR/css-text-3/#text-encoding>
#[derive(Debug, Clone, Copy, PartialEq)]
pub(in crate::text) struct DroppedDefaultIgnorableRun {
    pub(in crate::text) x_offset: f32,
    pub(in crate::text) advance: f32,
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
                let glyph_id = ttf_parser::GlyphId(glyph.id);
                let Some(raster) = face.glyph_raster_image(glyph_id, requested_ppem) else {
                    retained.push(glyph.clone());
                    cursor += glyph.x_advance;
                    continue;
                };
                let Some(decoded) = decode_raster_glyph_image(raster) else {
                    log::warn!(
                        "unable to decode bitmap glyph {} from font {}; retaining it for the PDF font path",
                        glyph.id,
                        font.post_script_name
                    );
                    retained.push(glyph.clone());
                    cursor += glyph.x_advance;
                    continue;
                };
                if raster.pixels_per_em == 0 || decoded.width == 0 || decoded.height == 0 {
                    log::warn!(
                        "bitmap glyph {} from font {} has unusable strike metrics; retaining it for the PDF font path",
                        glyph.id,
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
            let (palette, overrides) = self.color_palette_selection(&face, selection, &font.family);
            let Some(glyphs) = run.glyphs.as_ref() else {
                continue;
            };
            let mut cursor = run.x_offset;
            let mut retained = Vec::with_capacity(glyphs.len());
            for glyph in glyphs.iter() {
                let glyph_id = ttf_parser::GlyphId(glyph.id);
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
                let color = crate::css::color_to_srgb(style.color);
                let foreground = ttf_parser::RgbaColor {
                    red: (color.r * 255.0).round() as u8,
                    green: (color.g * 255.0).round() as u8,
                    blue: (color.b * 255.0).round() as u8,
                    alpha: (color.a * 255.0).round() as u8,
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
        family: &str,
    ) -> (u16, Option<&'a HashMap<u16, Color>>) {
        match selection {
            FontPalette::Named(name) => self
                .font_palette_values
                .get(name)
                .and_then(|definitions| {
                    definitions.iter().rev().find(|definition| {
                        definition.families.is_empty()
                            || definition.families.iter().any(|defined_family| {
                                defined_family.trim().eq_ignore_ascii_case(family.trim())
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
    overrides: &HashMap<u16, Color>,
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
    let foreground = Color::rgba(
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

fn cpal_color(face: &ttf_parser::Face<'_>, palette: u16, color_index: u16) -> Option<Color> {
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
    Some(Color::rgba(red, green, blue, alpha as f32 / 255.0))
}

fn append_colr_outline(
    face: &ttf_parser::Face<'_>,
    glyph_id: ttf_parser::GlyphId,
    outline_to_paint: GlyphOutlineToPaint,
    color: Color,
    paths: &mut Vec<RenderedPath>,
) {
    let mut builder = GlyphPathBuilder::new(outline_to_paint);
    if face.outline_glyph(glyph_id, &mut builder).is_some() && !builder.commands.is_empty() {
        paths.push(RenderedPath::new(
            builder.commands,
            Some(color),
            RenderedPathFillRule::NonZero,
            None,
            0.0,
            None,
        ));
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
        let color = Color::rgba(
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
            },
            DroppedDefaultIgnorableRun {
                x_offset: 6.0,
                advance: 1.5,
            },
        ];

        assert_eq!(corrected_visual_run_x_offset(12.0, &dropped_runs), 10.5);
        assert_eq!(corrected_visual_run_x_offset(24.0, &dropped_runs), 20.5);
    }

    #[test]
    fn rehomed_control_fragment_stitches_only_following_compatible_visual_runs() {
        let glyph = |unicode: &str| RenderedGlyph {
            id: 1,
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

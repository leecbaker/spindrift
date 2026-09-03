use std::rc::Rc;

use super::super::*;
use crate::document::paint::geometry::{PaintRect, PaintSize, PaintTransform};
use crate::document::paint::images::RenderedImage;

impl FontSystem {
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
            if !face_has_raster_glyphs(&face) {
                continue;
            }
            let Some(glyphs) = run.glyphs.as_ref() else {
                continue;
            };

            // CSS absolute units use 96 px per inch while Spindrift paint space
            // uses points, so one CSS px is 0.75 paint points.
            let requested_ppem = (run.font_size * (96.0 / 72.0))
                .round()
                .clamp(1.0, u16::MAX as f32) as u16;
            let mut cursor = run.x_offset;
            let mut retained: Option<Vec<RenderedGlyph>> = None;
            for (glyph_index, glyph) in glyphs.iter().enumerate() {
                let Some(glyph_id) = glyph.painted_id().map(ttf_parser::GlyphId) else {
                    if let Some(retained) = &mut retained {
                        retained.push(glyph.clone());
                    }
                    cursor += glyph.x_advance;
                    continue;
                };
                let Some(raster) = face.glyph_raster_image(glyph_id, requested_ppem) else {
                    if let Some(retained) = &mut retained {
                        retained.push(glyph.clone());
                    }
                    cursor += glyph.x_advance;
                    continue;
                };
                let Some(decoded) = decode_raster_glyph_image(raster) else {
                    log::warn!(
                        "unable to decode bitmap glyph {} from font {}; retaining it for the PDF font path",
                        glyph_id.0,
                        font.post_script_name
                    );
                    if let Some(retained) = &mut retained {
                        retained.push(glyph.clone());
                    }
                    cursor += glyph.x_advance;
                    continue;
                };
                if raster.pixels_per_em == 0 || decoded.width == 0 || decoded.height == 0 {
                    log::warn!(
                        "bitmap glyph {} from font {} has unusable strike metrics; retaining it for the PDF font path",
                        glyph_id.0,
                        font.post_script_name
                    );
                    if let Some(retained) = &mut retained {
                        retained.push(glyph.clone());
                    }
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
                )
                .with_raster_sample_depth(decoded.sample_depth);
                if !glyph.unicode.is_empty() {
                    image = image.with_actual_text(Rc::from(glyph.unicode.as_str()));
                }
                if !run.text_matrix.is_identity() {
                    let [a, b, c, d] = run.text_matrix.pdf_components();
                    image =
                        image.with_transform(PaintTransform::new(a, b, c, d, origin.x, origin.y));
                }
                retained.get_or_insert_with(|| glyphs[..glyph_index].to_vec());
                images.push(image);
                cursor += glyph.x_advance;
            }
            if let Some(retained) = retained {
                run.glyphs = (!retained.is_empty()).then(|| retained.into());
            }
        }
        images
    }
}

fn face_has_raster_glyphs(face: &ttf_parser::Face<'_>) -> bool {
    let tables = face.tables();
    tables.sbix.is_some() || tables.bdat.is_some() || tables.ebdt.is_some() || tables.cbdt.is_some()
}

pub(super) struct DecodedRasterGlyph {
    pub(super) width: u32,
    pub(super) height: u32,
    pub(super) sample_depth: crate::image_store::RasterSampleDepth,
    pub(super) rgb: Rc<[u8]>,
    pub(super) alpha: Option<Rc<[u8]>>,
}

pub(super) fn decode_raster_glyph_image(
    image: ttf_parser::RasterGlyphImage<'_>,
) -> Option<DecodedRasterGlyph> {
    let width = u32::from(image.width);
    let height = u32::from(image.height);
    let pixel_count = usize::try_from(width.checked_mul(height)?).ok()?;
    if pixel_count == 0 {
        return None;
    }
    let (sample_depth, rgb, alpha) = match image.format {
        ttf_parser::RasterImageFormat::PNG => decode_png_raster_glyph(image.data, width, height)?,
        ttf_parser::RasterImageFormat::BitmapPremulBgra32 => {
            let (rgb, alpha) = decode_premultiplied_bgra(image.data, pixel_count)?;
            (crate::image_store::RasterSampleDepth::Eight, rgb, alpha)
        }
        format => {
            let (rgb, alpha) = decode_grayscale_raster(image.data, width, height, format)?;
            (crate::image_store::RasterSampleDepth::Eight, rgb, alpha)
        }
    };
    Some(DecodedRasterGlyph {
        width,
        height,
        sample_depth,
        rgb: Rc::from(rgb.into_boxed_slice()),
        alpha: alpha.map(|alpha| Rc::from(alpha.into_boxed_slice())),
    })
}

fn decode_png_raster_glyph(
    data: &[u8],
    expected_width: u32,
    expected_height: u32,
) -> Option<(
    crate::image_store::RasterSampleDepth,
    Vec<u8>,
    Option<Vec<u8>>,
)> {
    // PNG's largest source representation has four 16-bit components. Bound
    // decoder intermediates to the declared strike while retaining a small
    // metadata allowance for valid ICC and text chunks, and never exceed
    // Spindrift's document-image ceiling for malformed font data.
    const MAX_PNG_GLYPH_DECODER_BYTES: usize = 512 * 1024 * 1024;
    const PNG_GLYPH_METADATA_ALLOWANCE: usize = 1024 * 1024;
    let allocation_limit = usize::try_from(expected_width)
        .ok()?
        .checked_mul(usize::try_from(expected_height).ok()?)?
        .checked_mul(8)?
        .clamp(PNG_GLYPH_METADATA_ALLOWANCE, MAX_PNG_GLYPH_DECODER_BYTES);
    let decoded = crate::image_store::decode_png_samples(data, allocation_limit)?;
    if decoded.width != expected_width || decoded.height != expected_height {
        return None;
    }
    Some((decoded.sample_depth, decoded.rgb, decoded.alpha))
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
    for pixel in data.as_chunks::<4>().0 {
        let &[blue, green, red, opacity] = pixel;
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

#[cfg(test)]
mod tests {
    use super::*;

    fn rgba_png(width: u32, height: u32, pixels: &[u8]) -> Vec<u8> {
        let mut encoded = Vec::new();
        let mut encoder = png::Encoder::new(&mut encoded, width, height);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        encoder
            .write_header()
            .unwrap()
            .write_image_data(pixels)
            .unwrap();
        encoded
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
        let encoded = rgba_png(1, 1, &[10, 20, 30, 128]);
        let decoded = decode_raster_glyph_image(ttf_parser::RasterGlyphImage {
            x: 0,
            y: 0,
            width: 1,
            height: 1,
            pixels_per_em: 16,
            format: ttf_parser::RasterImageFormat::PNG,
            data: &encoded,
        })
        .unwrap();
        assert_eq!(decoded.rgb.as_ref(), &[10, 20, 30]);
        assert_eq!(decoded.alpha.as_deref().unwrap(), &[128]);
    }

    #[test]
    fn rejects_png_bitmap_glyphs_with_transposed_dimensions() {
        let encoded = rgba_png(2, 1, &[10, 20, 30, 255, 40, 50, 60, 255]);

        // Both the PNG and strike record contain two pixels, but accepting
        // only the pixel count would paint them at the wrong aspect ratio.
        let decoded = decode_raster_glyph_image(ttf_parser::RasterGlyphImage {
            x: 0,
            y: 0,
            width: 1,
            height: 2,
            pixels_per_em: 16,
            format: ttf_parser::RasterImageFormat::PNG,
            data: &encoded,
        });

        assert!(decoded.is_none());
    }

    #[test]
    fn rejects_malformed_png_bitmap_glyphs() {
        let decoded = decode_raster_glyph_image(ttf_parser::RasterGlyphImage {
            x: 0,
            y: 0,
            width: 1,
            height: 1,
            pixels_per_em: 16,
            format: ttf_parser::RasterImageFormat::PNG,
            data: b"\x89PNG\r\n\x1a\n",
        });
        assert!(decoded.is_none());
    }
}

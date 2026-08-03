use super::super::*;
use crate::document::paint::images::RenderedImage;
use std::rc::Rc;

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
}

pub(super) struct DecodedRasterGlyph {
    pub(super) width: u32,
    pub(super) height: u32,
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

fn rgba_to_rgb_alpha(rgba: &[u8]) -> Option<(Vec<u8>, Option<Vec<u8>>)> {
    if !rgba.len().is_multiple_of(4) {
        return None;
    }
    let mut rgb = Vec::with_capacity(rgba.len() / 4 * 3);
    let mut alpha = Vec::with_capacity(rgba.len() / 4);
    let mut has_alpha = false;
    for pixel in rgba.as_chunks::<4>().0 {
        rgb.extend_from_slice(&pixel[..3]);
        alpha.push(pixel[3]);
        has_alpha |= pixel[3] < 255;
    }
    Some((rgb, has_alpha.then_some(alpha)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;
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

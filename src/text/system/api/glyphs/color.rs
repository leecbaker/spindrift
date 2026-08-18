use super::super::*;
use super::outline::{GlyphOutlineToPaint, append_colr_outline};
use crate::CssColor;
use crate::document::paint::geometry::PaintPoint;
use crate::document::paint::paths::RenderedPath;

impl FontSystem {
    pub(crate) fn take_color_glyph_paths(
        &self,
        origin: PaintPoint,
        runs: &mut [RenderedTextRun],
        style: &ComputedStyle,
    ) -> Vec<RenderedPath> {
        let mut paths = Vec::new();
        for run in runs {
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
            if !face_has_colr_glyphs(&face) {
                continue;
            }
            let (palette, overrides) =
                self.color_palette_selection(&face, &run.font_palette, font_id, style);
            let Some(glyphs) = run.glyphs.as_ref() else {
                continue;
            };
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
                if !face.is_color_glyph(glyph_id) {
                    if let Some(retained) = &mut retained {
                        retained.push(glyph.clone());
                    }
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
                let color = crate::css::color_to_predefined_rgb(
                    style.color,
                    crate::css::CssColorSpace::Srgb,
                )
                .expect("sRGB is a predefined CSS RGB space");
                let foreground = ttf_parser::RgbaColor {
                    red: (color.components()[0] * 255.0).round() as u8,
                    green: (color.components()[1] * 255.0).round() as u8,
                    blue: (color.components()[2] * 255.0).round() as u8,
                    alpha: (color.alpha() * 255.0).round() as u8,
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
                    retained.get_or_insert_with(|| glyphs[..glyph_index].to_vec());
                    paths.append(&mut painter.paths);
                } else {
                    if let Some(retained) = &mut retained {
                        retained.push(glyph.clone());
                    }
                }
                cursor += glyph.x_advance;
            }
            if let Some(retained) = retained {
                run.glyphs = (!retained.is_empty()).then(|| retained.into());
            }
        }
        paths
    }
}

fn face_has_colr_glyphs(face: &ttf_parser::Face<'_>) -> bool {
    face.tables().colr.is_some()
}

impl FontSystem {
    fn color_palette_selection<'a>(
        &'a self,
        face: &ttf_parser::Face<'_>,
        selection: &FontPalette,
        font_id: usize,
        style: &ComputedStyle,
    ) -> (u16, Option<&'a HashMap<u16, CssColor>>) {
        // A `@font-palette-values` rule matches the CSS family of the
        // selected `@font-face`, which can deliberately be the empty string.
        // The embedded OpenType family name is not an equivalent identifier.
        // <https://drafts.csswg.org/css-fonts-4/#font-palette-values>
        let family = self
            .document_fonts
            .selected_face_features(font_id)
            .map(|(family, _)| family)
            .or_else(|| font_feature_family(&style.font_family))
            .or_else(|| {
                self.document_fonts
                    .get(font_id)
                    .map(|font| font.family.clone())
            });
        match selection {
            FontPalette::Named(name) => self
                .font_palette_values
                .get(name)
                .and_then(|definitions| {
                    definitions.iter().rev().find(|definition| {
                        definition.families.is_empty()
                            || definition.families.iter().any(|defined_family| {
                                family.as_ref().is_some_and(|family| {
                                    defined_family.trim().eq_ignore_ascii_case(family.trim())
                                })
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
    overrides: &HashMap<u16, CssColor>,
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
    let foreground = CssColor::rgba(
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

fn cpal_color(face: &ttf_parser::Face<'_>, palette: u16, color_index: u16) -> Option<CssColor> {
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
    Some(CssColor::rgba(red, green, blue, alpha as f32 / 255.0))
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
        let color = CssColor::rgba(
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

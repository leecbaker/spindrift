use super::*;

pub(super) fn image_key(image: &RenderedImage) -> ImageKey {
    let data = image_resource_data(image);
    ImageKey {
        pixel_width: data.pixel_width,
        pixel_height: data.pixel_height,
        interpolate: image.interpolate,
        rgb: data.rgb,
        alpha: data.alpha,
    }
}

pub(super) fn image_resource_data(image: &RenderedImage) -> ImageResourceData {
    let Some(source_rect) = image.source_rect else {
        return ImageResourceData {
            pixel_width: image.pixel_width,
            pixel_height: image.pixel_height,
            rgb: image.rgb.clone(),
            alpha: image.alpha.clone(),
        };
    };
    let x0 = source_rect.x.min(image.pixel_width);
    let y0 = source_rect.y.min(image.pixel_height);
    let x1 = x0.saturating_add(source_rect.width).min(image.pixel_width);
    let y1 = y0
        .saturating_add(source_rect.height)
        .min(image.pixel_height);
    let cropped_width = x1.saturating_sub(x0);
    let cropped_height = y1.saturating_sub(y0);
    if cropped_width == 0 || cropped_height == 0 {
        return ImageResourceData {
            pixel_width: 1,
            pixel_height: 1,
            rgb: vec![0, 0, 0],
            alpha: Some(vec![0]),
        };
    }

    let mut rgb = Vec::with_capacity(cropped_width as usize * cropped_height as usize * 3);
    let mut alpha = image
        .alpha
        .as_ref()
        .map(|_| Vec::with_capacity(cropped_width as usize * cropped_height as usize));
    for source_y in y0..y1 {
        let row_start = (source_y as usize * image.pixel_width as usize + x0 as usize) * 3;
        let row_end = row_start + cropped_width as usize * 3;
        rgb.extend_from_slice(&image.rgb[row_start..row_end]);
        if let (Some(source_alpha), Some(cropped_alpha)) = (&image.alpha, &mut alpha) {
            let alpha_row_start = source_y as usize * image.pixel_width as usize + x0 as usize;
            let alpha_row_end = alpha_row_start + cropped_width as usize;
            cropped_alpha.extend_from_slice(&source_alpha[alpha_row_start..alpha_row_end]);
        }
    }
    ImageResourceData {
        pixel_width: cropped_width,
        pixel_height: cropped_height,
        rgb,
        alpha,
    }
}

pub(super) struct ImageResourceData {
    pub(super) pixel_width: u32,
    pub(super) pixel_height: u32,
    pub(super) rgb: Vec<u8>,
    pub(super) alpha: Option<Vec<u8>>,
}

/// Return the PDF graphics-state resource name for a semi-transparent color.
///
/// PDF 1.4 transparency uses ExtGState dictionaries with stroking (`CA`) and
/// nonstroking (`ca`) alpha constants:
/// ISO 32000-1:2008, 11.7.4.3 "Constant Shape and Opacity".
pub(super) fn paint_alpha_resource_name(color: Color) -> Option<String> {
    alpha_key(color).map(|key| format!("GSalpha{key:03}"))
}

/// Plan a page-local `/ExtGState` resource for alpha paints.
///
/// PDF page resource dictionaries name ExtGState resources, and content streams
/// activate them with the `gs` operator:
/// ISO 32000-1:2008, 7.8.3 "Resource Dictionaries" and 8.4.5 "Graphics State
/// Parameter Dictionaries".
#[derive(Debug, Clone, PartialEq)]
pub(super) enum ExtGStateResource {
    Alpha {
        name: String,
        alpha: f32,
    },
    Blend {
        name: String,
        mode: crate::document::PaintBlendMode,
    },
}

impl ExtGStateResource {
    pub(super) fn name(&self) -> &str {
        match self {
            Self::Alpha { name, .. } | Self::Blend { name, .. } => name,
        }
    }
}

/// Collect page-local `/ExtGState` resource entries for alpha and blend modes.
///
/// PDF 1.4 transparency uses ExtGState dictionaries with stroking (`CA`) and
/// nonstroking (`ca`) alpha constants, and blend modes are selected with the
/// `/BM` graphics-state parameter:
/// ISO 32000-1:2008, 8.4.5 "Graphics State Parameter Dictionaries" and
/// 11.3.5 "Blend Mode".
pub(super) fn page_ext_gstate_resources(page: &Page) -> Vec<ExtGStateResource> {
    let mut alpha_keys = BTreeMap::new();
    let mut blend_modes = BTreeMap::new();
    for rect in &page.rects {
        if let Some(fill) = rect.fill {
            collect_alpha_key(&mut alpha_keys, fill);
        }
        if let Some(stroke) = rect.stroke {
            collect_alpha_key(&mut alpha_keys, stroke);
        }
    }
    for rect in &page.rounded_rects {
        if let Some(fill) = rect.fill {
            collect_alpha_key(&mut alpha_keys, fill);
        }
        if let Some(stroke) = rect.stroke {
            collect_alpha_key(&mut alpha_keys, stroke);
        }
    }
    for stroke in &page.strokes {
        collect_alpha_key(&mut alpha_keys, stroke.color);
    }
    for line in &page.lines {
        collect_alpha_key(&mut alpha_keys, line.color);
    }
    if let Some(tree) = page.paint_tree() {
        collect_paint_tree_ext_gstates(&mut alpha_keys, &mut blend_modes, &tree.root);
    }
    if alpha_keys.is_empty() && blend_modes.is_empty() {
        return Vec::new();
    }
    let mut entries = alpha_keys
        .into_keys()
        .map(|key| {
            let alpha = key as f32 / 1000.0;
            ExtGStateResource::Alpha {
                name: format!("GSalpha{key:03}"),
                alpha,
            }
        })
        .collect::<Vec<_>>();
    entries.extend(blend_modes.into_keys().filter_map(|mode| {
        Some(ExtGStateResource::Blend {
            name: mode.resource_name()?,
            mode,
        })
    }));
    entries
}

fn collect_alpha_key(alpha_keys: &mut BTreeMap<u16, ()>, color: Color) {
    if let Some(key) = alpha_key(color) {
        alpha_keys.insert(key, ());
    }
}

fn collect_opacity_key(alpha_keys: &mut BTreeMap<u16, ()>, opacity: f32) {
    collect_alpha_key(
        alpha_keys,
        Color {
            r: 0.0,
            g: 0.0,
            b: 0.0,
            a: opacity,
        },
    );
}

fn collect_paint_tree_ext_gstates(
    alpha_keys: &mut BTreeMap<u16, ()>,
    blend_modes: &mut BTreeMap<crate::document::PaintBlendMode, ()>,
    context: &crate::document::PaintStackingContext,
) {
    collect_opacity_key(alpha_keys, context.effects.opacity);
    if context.effects.blend_mode != crate::document::PaintBlendMode::Normal {
        blend_modes.insert(context.effects.blend_mode, ());
    }
    for band in crate::document::PaintBand::ORDER {
        for item in &context.bands.bands[band.index()] {
            match item {
                crate::document::PaintDisplayItem::StackingContext(child) => {
                    collect_paint_tree_ext_gstates(alpha_keys, blend_modes, child);
                }
                crate::document::PaintDisplayItem::EffectScope(scope) => {
                    collect_effect_scope_ext_gstates(alpha_keys, blend_modes, scope);
                }
                crate::document::PaintDisplayItem::Operation(_)
                | crate::document::PaintDisplayItem::Primitive(_)
                | crate::document::PaintDisplayItem::Link(_) => {}
            }
        }
    }
}

fn collect_effect_scope_ext_gstates(
    alpha_keys: &mut BTreeMap<u16, ()>,
    blend_modes: &mut BTreeMap<crate::document::PaintBlendMode, ()>,
    scope: &crate::document::PaintEffectScope,
) {
    collect_opacity_key(alpha_keys, scope.effects.opacity);
    if scope.effects.blend_mode != crate::document::PaintBlendMode::Normal {
        blend_modes.insert(scope.effects.blend_mode, ());
    }
    for item in &scope.items {
        match item {
            crate::document::PaintDisplayItem::StackingContext(child) => {
                collect_paint_tree_ext_gstates(alpha_keys, blend_modes, child);
            }
            crate::document::PaintDisplayItem::EffectScope(child) => {
                collect_effect_scope_ext_gstates(alpha_keys, blend_modes, child);
            }
            crate::document::PaintDisplayItem::Operation(_)
            | crate::document::PaintDisplayItem::Primitive(_)
            | crate::document::PaintDisplayItem::Link(_) => {}
        }
    }
}

fn alpha_key(color: Color) -> Option<u16> {
    if color.is_visible() && !color.is_opaque() {
        Some((color.a * 1000.0).round().clamp(1.0, 999.0) as u16)
    } else {
        None
    }
}

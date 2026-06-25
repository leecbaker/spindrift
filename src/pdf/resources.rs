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

pub(super) fn image_object_dictionary(
    pixel_width: u32,
    pixel_height: u32,
    interpolate: bool,
    rgb: &[u8],
    alpha_mask_id: Option<usize>,
) -> Vec<u8> {
    let soft_mask = alpha_mask_id
        .map(|id| format!(" /SMask {id} 0 R"))
        .unwrap_or_default();
    let mut object = format!(
        "<< /Type /XObject /Subtype /Image /Width {pixel_width} /Height {pixel_height} /ColorSpace /DeviceRGB /BitsPerComponent 8 /Interpolate {interpolate}{soft_mask} /Length {} >>\nstream\n",
        rgb.len()
    )
    .into_bytes();
    object.extend_from_slice(rgb);
    object.extend_from_slice(b"\nendstream\n");
    object
}

pub(super) fn image_alpha_mask_object(
    pixel_width: u32,
    pixel_height: u32,
    interpolate: bool,
    alpha: &[u8],
) -> Vec<u8> {
    let mut object = format!(
        "<< /Type /XObject /Subtype /Image /Width {pixel_width} /Height {pixel_height} /ColorSpace /DeviceGray /BitsPerComponent 8 /Interpolate {interpolate} /Length {} >>\nstream\n",
        alpha.len()
    )
    .into_bytes();
    object.extend_from_slice(alpha);
    object.extend_from_slice(b"\nendstream\n");
    object
}

/// Return the PDF graphics-state resource name for a semi-transparent color.
///
/// PDF 1.4 transparency uses ExtGState dictionaries with stroking (`CA`) and
/// nonstroking (`ca`) alpha constants:
/// ISO 32000-1:2008, 11.7.4.3 "Constant Shape and Opacity".
pub(super) fn paint_alpha_resource_name(color: Color) -> Option<String> {
    alpha_key(color).map(|key| format!("GSalpha{key:03}"))
}

/// Build a page-local `/ExtGState` resource dictionary for alpha paints.
///
/// PDF page resources can contain direct ExtGState dictionaries, and content
/// streams activate them with the `gs` operator:
/// ISO 32000-1:2008, 7.8.3 "Resource Dictionaries" and 8.4.5 "Graphics State
/// Parameter Dictionaries".
pub(super) fn page_ext_gstate_resource_dictionary(page: &Page) -> String {
    let mut alpha_keys = BTreeMap::new();
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
        collect_paint_tree_alpha_keys(&mut alpha_keys, &tree.root);
    }
    if alpha_keys.is_empty() {
        return String::new();
    }
    let entries = alpha_keys
        .into_keys()
        .map(|key| {
            let alpha = key as f32 / 1000.0;
            format!("/GSalpha{key:03} << /Type /ExtGState /ca {alpha:.3} /CA {alpha:.3} >>")
        })
        .collect::<Vec<_>>()
        .join(" ");
    format!(" /ExtGState << {entries} >>")
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

fn collect_paint_tree_alpha_keys(
    alpha_keys: &mut BTreeMap<u16, ()>,
    context: &crate::document::PaintStackingContext,
) {
    collect_opacity_key(alpha_keys, context.effects.opacity);
    for band in crate::document::PaintBand::ORDER {
        for item in &context.bands.bands[band.index()] {
            if let crate::document::PaintDisplayItem::StackingContext(child) = item {
                collect_paint_tree_alpha_keys(alpha_keys, child);
            }
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

pub(super) fn annotation_dictionary(link: &RenderedLink) -> String {
    format!(
        "<< /Type /Annot /Subtype /Link /Rect [{:.3} {:.3} {:.3} {:.3}] /Border [0 0 0] /A << /S /URI /URI ({}) >> >>\n",
        link.x,
        link.y,
        link.x + link.width,
        link.y + link.height,
        escape_pdf_string(&link.target)
    )
}

pub(super) fn info_dictionary(document: &Document) -> String {
    let mut info = format!(
        "<< /Producer ({})",
        escape_pdf_string(&document.metadata.producer)
    );
    if let Some(title) = &document.metadata.title {
        info.push_str(&format!(" /Title ({})", escape_pdf_string(title)));
    }
    if let Some(author) = &document.metadata.author {
        info.push_str(&format!(" /Author ({})", escape_pdf_string(author)));
    }
    if let Some(creator) = &document.metadata.creator {
        info.push_str(&format!(" /Creator ({})", escape_pdf_string(creator)));
    }
    info.push_str(" >>\n");
    info
}

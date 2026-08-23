//! Font-object serialization for already prepared embedded subsets.

use pdf_writer::types::{CidFontType, FontFlags, SystemInfo};
use pdf_writer::{Filter, Name, Pdf, Rect, Str};

use super::super::*;
use super::primitives::{i32_from_usize, pdf_name, pdf_ref};
use super::stream::encode_pdf_stream;
use crate::document::FontProgramKind;
use crate::timing::DebugTimer;

pub(crate) fn write_embedded_fonts(
    pdf: &mut Pdf,
    embedded_fonts: &[EmbeddedFontPlan<'_>],
    compression: crate::PdfCompression,
) {
    let _timer = DebugTimer::start(format!(
        "building {} embedded font object set(s)",
        embedded_fonts.len()
    ));
    for font in embedded_fonts {
        pdf.type0_font(pdf_ref(font.type0_id))
            .base_font(pdf_name(&font.base_name))
            .encoding_predefined(Name(b"Identity-H"))
            .descendant_font(pdf_ref(font.cid_font_id))
            .to_unicode(pdf_ref(font.to_unicode_id));
        let subtype = match font.font_program_kind {
            FontProgramKind::TrueType => CidFontType::Type2,
            FontProgramKind::OpenTypeCff => CidFontType::Type0,
        };
        {
            let mut cid_font = pdf.cid_font(pdf_ref(font.cid_font_id));
            cid_font
                .subtype(subtype)
                .base_font(pdf_name(&font.base_name))
                .system_info(SystemInfo {
                    registry: Str(b"Adobe"),
                    ordering: Str(b"Identity"),
                    supplement: 0,
                })
                .font_descriptor(pdf_ref(font.descriptor_id))
                .default_width(font.default_width);
            // ISO 32000-2:2020, 9.7.4.3: CIDToGIDMap applies only to CIDFontType2.
            if font.font_program_kind == FontProgramKind::TrueType {
                cid_font.cid_to_gid_map_predefined(Name(b"Identity"));
            }
            let mut widths = cid_font.widths();
            for (glyph_id, width) in cid_width_entries(font) {
                widths.consecutive(glyph_id, [width]);
            }
        }
        let metrics = &font.descriptor_metrics;
        {
            let mut descriptor = pdf.font_descriptor(pdf_ref(font.descriptor_id));
            descriptor
                .name(pdf_name(&font.base_name))
                .flags(FontFlags::from_bits_retain(metrics.flags))
                .bbox(Rect::new(
                    metrics.bbox[0] as f32,
                    metrics.bbox[1] as f32,
                    metrics.bbox[2] as f32,
                    metrics.bbox[3] as f32,
                ))
                .italic_angle(metrics.italic_angle)
                .ascent(metrics.ascent)
                .descent(metrics.descent)
                .cap_height(metrics.cap_height)
                .stem_v(metrics.stem_v);
            if let Some(x_height) = metrics.x_height {
                descriptor.x_height(x_height);
            }
            if let Some(avg_width) = metrics.avg_width {
                descriptor.avg_width(avg_width);
            }
            if let Some(max_width) = metrics.max_width {
                descriptor.max_width(max_width);
            }
            if let Some(missing_width) = metrics.missing_width {
                descriptor.missing_width(missing_width);
            }
            if let Some(cid_set_id) = font.cid_set_id {
                descriptor.cid_set(pdf_ref(cid_set_id));
            }
            match font.font_program_kind {
                FontProgramKind::TrueType => {
                    descriptor.font_file2(pdf_ref(font.file_id));
                }
                FontProgramKind::OpenTypeCff => {
                    descriptor.font_file3(pdf_ref(font.file_id));
                }
            }
        }
        log_embedded_font_file(font);
        let data = font.font_file_data.as_slice();
        let stream = encode_pdf_stream(compression, data);
        {
            let mut font_file = pdf.stream(pdf_ref(font.file_id), stream.bytes());
            if stream.uses_flate() {
                font_file.filter(Filter::FlateDecode);
            }
            match font.font_program_kind {
                FontProgramKind::TrueType => {
                    font_file.pair(Name(b"Length1"), i32_from_usize(data.len()));
                }
                FontProgramKind::OpenTypeCff => {
                    font_file.pair(Name(b"Subtype"), Name(b"CIDFontType0C"));
                }
            }
        }
        let cmap = to_unicode_cmap(font);
        let cmap_stream = encode_pdf_stream(compression, &cmap);
        {
            let mut cmap_writer = pdf.cmap(pdf_ref(font.to_unicode_id), cmap_stream.bytes());
            if cmap_stream.uses_flate() {
                cmap_writer.filter(Filter::FlateDecode);
            }
        }
        if let (Some(cid_set_id), Some(cid_set_data)) =
            (font.cid_set_id, font.cid_set_data.as_ref())
        {
            let cid_set_stream = encode_pdf_stream(compression, cid_set_data);
            let mut cid_set_writer = pdf.stream(pdf_ref(cid_set_id), cid_set_stream.bytes());
            if cid_set_stream.uses_flate() {
                cid_set_writer.filter(Filter::FlateDecode);
            }
        }
    }
}

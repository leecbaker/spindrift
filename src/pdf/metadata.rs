use crate::{DocumentMetadata, PdfCompression, PdfProfile};
use pdf_writer::{Filter, Pdf, Ref, TextStr};

/// Writes synchronized PDF document information and XMP metadata.
///
/// PDF 1.4 metadata streams are defined by ISO 32000-1:2008, 14.3.2. PDF/A-1
/// additionally requires document information dictionary entries and their
/// analogous XMP properties to be equivalent when both are present.
pub(super) fn write_document_info(
    pdf: &mut Pdf,
    info_id: Ref,
    metadata: &DocumentMetadata,
    producer: &str,
) {
    let metadata = PdfDocumentMetadata::new(metadata, producer);
    let mut info = pdf.document_info(info_id);
    info.producer(TextStr(metadata.producer));
    if let Some(title) = metadata.title {
        info.title(TextStr(title));
    }
    if let Some(author) = metadata.author {
        info.author(TextStr(author));
    }
    if let Some(creator) = metadata.creator {
        info.creator(TextStr(creator));
    }
}

/// Writes the catalog metadata stream mirrored from the PDF information fields.
///
/// PDF/A identification properties are defined by ISO 19005's PDF/A extension
/// schema. When a PDF/A profile is selected, the stream includes the required
/// `pdfaid:*` identification fields but does not claim that unrelated archival
/// requirements such as output intents or color restrictions are complete.
pub(super) fn write_document_xmp_metadata(
    pdf: &mut Pdf,
    metadata_id: Ref,
    metadata: &DocumentMetadata,
    profile: PdfProfile,
    compression: PdfCompression,
    producer: &str,
) {
    let packet = PdfDocumentMetadata::new(metadata, producer).xmp_packet(profile);
    let stream = super::encode_pdf_stream(compression, packet.as_bytes());
    let mut metadata_writer = pdf.metadata(metadata_id, stream.bytes());
    if stream.uses_flate() {
        metadata_writer.filter(Filter::FlateDecode);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct PdfDocumentMetadata<'a> {
    title: Option<&'a str>,
    author: Option<&'a str>,
    creator: Option<&'a str>,
    producer: &'a str,
}

impl<'a> PdfDocumentMetadata<'a> {
    fn new(metadata: &'a DocumentMetadata, producer: &'a str) -> Self {
        Self {
            title: metadata.title.as_deref(),
            author: metadata.author.as_deref(),
            creator: metadata.creator.as_deref(),
            producer,
        }
    }
    fn xmp_packet(self, profile: PdfProfile) -> String {
        let mut packet = String::new();
        packet.push_str(r#"<?xpacket begin="" id="W5M0MpCehiHzreSzNTczkc9d"?>"#);
        packet.push('\n');
        packet.push_str(r#"<x:xmpmeta xmlns:x="adobe:ns:meta/">"#);
        packet.push('\n');
        packet.push_str(r#"<rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#" xmlns:dc="http://purl.org/dc/elements/1.1/" xmlns:xmp="http://ns.adobe.com/xap/1.0/" xmlns:pdf="http://ns.adobe.com/pdf/1.3/""#);
        if profile.is_pdfa() {
            packet.push_str(r#" xmlns:pdfaid="http://www.aiim.org/pdfa/ns/id/""#);
        }
        packet.push('>');
        packet.push('\n');
        if let Some(identification) = profile.pdfa_identification() {
            packet.push_str(r#"<rdf:Description rdf:about="" pdfaid:part=""#);
            packet.push_str(&identification.part.to_string());
            packet.push_str(r#"" pdfaid:conformance=""#);
            packet.push_str(identification.conformance);
            packet.push_str(r#""></rdf:Description>"#);
            packet.push('\n');
        }
        self.push_producer(&mut packet);
        if let Some(title) = self.title {
            Self::push_lang_alt(&mut packet, "dc:title", title);
        }
        if let Some(author) = self.author {
            Self::push_creator(&mut packet, author);
        }
        if let Some(creator) = self.creator {
            Self::push_simple_element(&mut packet, "xmp:CreatorTool", creator);
        }
        packet.push_str("</rdf:RDF>\n");
        packet.push_str("</x:xmpmeta>\n");
        packet.push_str(r#"<?xpacket end="r"?>"#);
        packet
    }

    fn push_producer(self, packet: &mut String) {
        Self::push_simple_element(packet, "pdf:Producer", self.producer);
    }

    fn push_simple_element(packet: &mut String, element: &str, value: &str) {
        packet.push_str(r#"<rdf:Description rdf:about="">"#);
        packet.push('<');
        packet.push_str(element);
        packet.push('>');
        push_xml_escaped(packet, value);
        packet.push_str("</");
        packet.push_str(element);
        packet.push_str("></rdf:Description>\n");
    }

    fn push_lang_alt(packet: &mut String, element: &str, value: &str) {
        packet.push_str(r#"<rdf:Description rdf:about=""><"#);
        packet.push_str(element);
        packet.push_str(r#"><rdf:Alt><rdf:li xml:lang="x-default">"#);
        push_xml_escaped(packet, value);
        packet.push_str("</rdf:li></rdf:Alt></");
        packet.push_str(element);
        packet.push_str("></rdf:Description>\n");
    }

    fn push_creator(packet: &mut String, value: &str) {
        packet.push_str(r#"<rdf:Description rdf:about=""><dc:creator><rdf:Seq><rdf:li>"#);
        push_xml_escaped(packet, value);
        packet.push_str("</rdf:li></rdf:Seq></dc:creator></rdf:Description>\n");
    }
}

fn push_xml_escaped(output: &mut String, value: &str) {
    for character in value.chars() {
        match character {
            '&' => output.push_str("&amp;"),
            '<' => output.push_str("&lt;"),
            '>' => output.push_str("&gt;"),
            '"' => output.push_str("&quot;"),
            '\'' => output.push_str("&#39;"),
            _ => output.push(character),
        }
    }
}

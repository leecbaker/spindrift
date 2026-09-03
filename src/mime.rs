//! MIME type parsing for HTML and CSS resource declarations.
//!
//! HTML source selection uses the MIME Sniffing Standard's recoverable
//! `parse a MIME type` algorithm, whereas CSS `image-set()` requires a valid
//! MIME type string. Keeping those operations separate prevents a malformed
//! CSS descriptor from becoming valid merely because HTML source selection can
//! recover its essence.

/// A lowercased `type/subtype` MIME essence.
///
/// Parameters are intentionally omitted: Spindrift's current image decoders
/// select solely on the MIME essence.
/// <https://mimesniff.spec.whatwg.org/#mime-type-essence>
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct MimeEssence(String);

impl MimeEssence {
    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }
}

/// Parse a MIME type and retain its normalized essence.
///
/// Parameter parsing is deliberately recoverable: invalid or incomplete
/// parameter data does not alter a successfully parsed type/subtype.
/// <https://mimesniff.spec.whatwg.org/#parse-a-mime-type>
pub(crate) fn parse_mime_type_essence(input: &str) -> Option<MimeEssence> {
    let input = trim_http_whitespace(input);
    let (type_, remainder) = input.split_once('/')?;
    let (subtype, _) = remainder.split_once(';').unwrap_or((remainder, ""));
    let subtype = trim_http_whitespace(subtype);
    mime_essence(type_, subtype)
}

/// Parse a valid MIME type string and retain its normalized essence.
///
/// Unlike [`parse_mime_type_essence`], this rejects malformed parameter data.
/// CSS Images uses this stricter production for `image-set()` `type()`
/// descriptors.
/// <https://mimesniff.spec.whatwg.org/#valid-mime-type-string>
pub(crate) fn parse_valid_mime_type_essence(input: &str) -> Option<MimeEssence> {
    let input = trim_http_whitespace(input);
    let (type_, remainder) = input.split_once('/')?;
    let (subtype, mut parameters) = remainder
        .split_once(';')
        .map_or((remainder, None), |(subtype, parameters)| {
            (subtype, Some(parameters))
        });
    let essence = mime_essence(type_, trim_http_whitespace(subtype))?;

    while let Some(parameters_remaining) = parameters {
        let parameters_remaining = trim_http_whitespace(parameters_remaining);
        if parameters_remaining.is_empty() {
            return None;
        }
        let name_end = parameters_remaining
            .bytes()
            .position(|byte| !is_mime_token_character(byte))?;
        if name_end == 0 || parameters_remaining.as_bytes().get(name_end) != Some(&b'=') {
            return None;
        }
        let value = &parameters_remaining[name_end + 1..];
        let next = parse_mime_parameter_value(value)?;
        parameters = next
            .strip_prefix(';')
            .map(Some)
            .or_else(|| next.is_empty().then_some(None))?;
    }

    Some(essence)
}

fn mime_essence(type_: &str, subtype: &str) -> Option<MimeEssence> {
    if type_.is_empty()
        || subtype.is_empty()
        || subtype.contains('/')
        || !type_.bytes().all(is_mime_token_character)
        || !subtype.bytes().all(is_mime_token_character)
    {
        return None;
    }
    Some(MimeEssence(format!(
        "{}/{}",
        type_.to_ascii_lowercase(),
        subtype.to_ascii_lowercase()
    )))
}

/// Consume a strict MIME parameter value and return the unconsumed suffix.
fn parse_mime_parameter_value(value: &str) -> Option<&str> {
    if let Some(value) = value.strip_prefix('"') {
        let mut escaped = false;
        for (index, byte) in value.bytes().enumerate() {
            if escaped {
                escaped = false;
            } else if byte == b'\\' {
                escaped = true;
            } else if byte == b'"' {
                return Some(trim_http_whitespace(&value[index + 1..]));
            } else if byte.is_ascii_control() && byte != b'\t' {
                return None;
            }
        }
        return None;
    }

    let value_end = value
        .bytes()
        .position(|byte| !is_mime_token_character(byte))
        .unwrap_or(value.len());
    if value_end == 0 {
        return None;
    }
    Some(trim_http_whitespace(&value[value_end..]))
}

fn trim_http_whitespace(value: &str) -> &str {
    value.trim_matches(|character| matches!(character, '\t' | '\n' | '\r' | ' '))
}

fn is_mime_token_character(byte: u8) -> bool {
    byte.is_ascii_alphanumeric()
        || matches!(
            byte,
            b'!' | b'#'
                | b'$'
                | b'%'
                | b'&'
                | b'\''
                | b'*'
                | b'+'
                | b'-'
                | b'.'
                | b'^'
                | b'_'
                | b'`'
                | b'|'
                | b'~'
        )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn html_mime_parsing_recovers_the_essence_from_invalid_parameters() {
        for value in [
            "image/gif;",
            "image/gif;encodings",
            "image/gif;encodings=",
            "image/gif;encodings=foobar",
            " Image/GIF; charset=\"unterminated",
        ] {
            assert_eq!(
                parse_mime_type_essence(value)
                    .as_ref()
                    .map(MimeEssence::as_str),
                Some("image/gif"),
                "{value}"
            );
        }
    }

    #[test]
    fn mime_parsing_rejects_invalid_essences() {
        for value in [
            "image\\gif",
            "gif",
            "image/gif, image/png",
            "image/gif image/png",
        ] {
            assert!(parse_mime_type_essence(value).is_none(), "{value}");
        }
    }

    #[test]
    fn valid_mime_type_strings_require_complete_parameters() {
        for value in [
            "image/png; charset",
            "image/png; =utf-8",
            "image/png; charset=\"unterminated",
            "image/png; charset=bad value",
            "image/png;",
        ] {
            assert!(parse_valid_mime_type_essence(value).is_none(), "{value}");
        }
        assert_eq!(
            parse_valid_mime_type_essence("image/png; profile=\"display;p3\"")
                .as_ref()
                .map(MimeEssence::as_str),
            Some("image/png")
        );
    }
}

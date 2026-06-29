use super::*;

const HTML_TABLE_MAX_COLSPAN: usize = 1000;
const HTML_TABLE_MAX_ROWSPAN: usize = 65534;

/// Return the used HTML `colspan` value for a `td`/`th` element.
///
/// HTML table construction parses the leading non-negative integer,
/// ignores trailing non-digits, defaults invalid/zero `colspan` to 1, and
/// clamps the column span to 1000:
/// <https://html.spec.whatwg.org/multipage/tables.html#forming-a-table>.
pub(in crate::layout) fn html_table_colspan(cell: &Element) -> usize {
    cell.attrs
        .get("colspan")
        .and_then(|value| parse_html_table_non_negative_integer(value))
        .filter(|value| *value > 0)
        .map(|value| value.min(HTML_TABLE_MAX_COLSPAN))
        .unwrap_or(1)
}

/// Return the used HTML `rowspan` value for a `td`/`th` element.
///
/// `rowspan=0` spans to the end of the current row group; positive row spans
/// are clamped to both the HTML maximum and the remaining rows in the row
/// group:
/// <https://html.spec.whatwg.org/multipage/tables.html#attr-tdth-rowspan>.
pub(in crate::layout) fn html_table_rowspan(
    cell: &Element,
    row_index: usize,
    row_group_end: usize,
) -> usize {
    let remaining_rows = row_group_end.saturating_sub(row_index).max(1);
    match cell
        .attrs
        .get("rowspan")
        .and_then(|value| parse_html_table_non_negative_integer(value))
    {
        Some(0) => remaining_rows,
        Some(value) => value.clamp(1, HTML_TABLE_MAX_ROWSPAN).min(remaining_rows),
        None => 1,
    }
}

/// Return the used HTML `span` value for `col` and `colgroup`.
///
/// HTML uses the same leading-integer parsing and 1000-column upper bound for
/// column spans as it does for cell column spans:
/// <https://html.spec.whatwg.org/multipage/tables.html#forming-a-table>.
pub(in crate::layout) fn html_table_column_span(element: &Element) -> usize {
    element
        .attrs
        .get("span")
        .and_then(|value| parse_html_table_non_negative_integer(value))
        .filter(|value| *value > 0)
        .map(|value| value.min(HTML_TABLE_MAX_COLSPAN))
        .unwrap_or(1)
}

fn parse_html_table_non_negative_integer(value: &str) -> Option<usize> {
    let mut parsed = None;
    for character in value.trim_start().chars() {
        if !character.is_ascii_digit() {
            break;
        };
        let digit = character as usize - '0' as usize;
        parsed = Some(
            parsed
                .unwrap_or(0usize)
                .saturating_mul(10)
                .saturating_add(digit),
        );
    }
    parsed
}

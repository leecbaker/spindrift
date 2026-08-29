use super::*;

pub(in crate::css) fn remove_vertical_align_baseline_source(
    value: &str,
) -> Option<(BaselineSource, &str)> {
    let trimmed = value.trim();
    let lower = trimmed.to_ascii_lowercase();
    for (keyword, source) in [
        ("first", BaselineSource::First),
        ("last", BaselineSource::Last),
    ] {
        if lower == keyword {
            return Some((source, ""));
        }
        if let Some(lower_rest) = lower.strip_prefix(keyword)
            && lower_rest.starts_with(char::is_whitespace)
        {
            let rest = &trimmed[keyword.len()..];
            return Some((source, rest.trim()));
        }
        if let Some(lower_rest) = lower.strip_suffix(keyword)
            && lower_rest.ends_with(char::is_whitespace)
        {
            let rest = &trimmed[..trimmed.len() - keyword.len()];
            return Some((source, rest.trim()));
        }
    }
    Some((BaselineSource::Auto, trimmed))
}

/// Parses CSS Inline Layout `dominant-baseline`.
///
/// `dominant-baseline` is the inherited baseline-table selection used when
/// `alignment-baseline: baseline` resolves against the parent:
/// <https://drafts.csswg.org/css-inline-3/#dominant-baseline-property>.
pub(crate) fn parse_dominant_baseline(value: &str) -> Option<DominantBaseline> {
    Some(match parse_baseline_metric(value)? {
        BaselineMetricParseResult::Auto => DominantBaseline::Auto,
        BaselineMetricParseResult::Metric(metric) => DominantBaseline::Metric(metric),
        BaselineMetricParseResult::Baseline => return None,
    })
}

/// Parses CSS Inline Layout `alignment-baseline`.
///
/// The `baseline` keyword resolves to the parent's dominant baseline during
/// layout:
/// <https://drafts.csswg.org/css-inline-3/#alignment-baseline-property>.
pub(crate) fn parse_alignment_baseline(value: &str) -> Option<AlignmentBaseline> {
    Some(match parse_baseline_metric(value)? {
        BaselineMetricParseResult::Baseline => AlignmentBaseline::Baseline,
        BaselineMetricParseResult::Metric(metric) => AlignmentBaseline::Metric(metric),
        BaselineMetricParseResult::Auto => return None,
    })
}

pub(crate) fn parse_baseline_source(value: &str) -> Option<BaselineSource> {
    match trim_css_value(value).to_ascii_lowercase().as_str() {
        "auto" => Some(BaselineSource::Auto),
        "first" => Some(BaselineSource::First),
        "last" => Some(BaselineSource::Last),
        _ => None,
    }
}

/// Parses CSS Inline Layout `baseline-shift`.
///
/// The computed value keeps mixed length-percentages typed until layout can
/// resolve percentages against the aligned element's own line-height:
/// <https://drafts.csswg.org/css-inline-3/#baseline-shift-property>.
pub(crate) fn parse_baseline_shift(value: &str, font_size: f32) -> Option<BaselineShift> {
    match trim_css_value(value).to_ascii_lowercase().as_str() {
        "baseline" => Some(BaselineShift::ZERO),
        "sub" => Some(BaselineShift::Sub),
        "super" => Some(BaselineShift::Super),
        "top" => Some(BaselineShift::Top),
        "center" => Some(BaselineShift::Center),
        "bottom" => Some(BaselineShift::Bottom),
        _ => {
            parse_computed_length_percentage(value, font_size).map(BaselineShift::LengthPercentage)
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::css) enum BaselineMetricParseResult {
    Auto,
    Baseline,
    Metric(BaselineMetric),
}

pub(in crate::css) fn parse_baseline_metric(value: &str) -> Option<BaselineMetricParseResult> {
    Some(match trim_css_value(value).to_ascii_lowercase().as_str() {
        "auto" => BaselineMetricParseResult::Auto,
        "baseline" => BaselineMetricParseResult::Baseline,
        "text-bottom" => BaselineMetricParseResult::Metric(BaselineMetric::TextBottom),
        "alphabetic" => BaselineMetricParseResult::Metric(BaselineMetric::Alphabetic),
        "ideographic" => BaselineMetricParseResult::Metric(BaselineMetric::Ideographic),
        "middle" => BaselineMetricParseResult::Metric(BaselineMetric::Middle),
        "central" => BaselineMetricParseResult::Metric(BaselineMetric::Central),
        "mathematical" => BaselineMetricParseResult::Metric(BaselineMetric::Mathematical),
        "hanging" => BaselineMetricParseResult::Metric(BaselineMetric::Hanging),
        "text-top" => BaselineMetricParseResult::Metric(BaselineMetric::TextTop),
        _ => return None,
    })
}

/// Parses CSS 2.2 `vertical-align` compatibility values as a CSS Inline
/// shorthand over `alignment-baseline`, `baseline-source`, and
/// `baseline-shift`.
///
/// Length and percentage values are computed as typed length-percentages;
/// layout resolves percentages against the element's own line-height:
/// <https://www.w3.org/TR/CSS22/visudet.html#propdef-vertical-align>.
pub(crate) fn parse_vertical_align(value: &str, font_size: f32) -> Option<VerticalAlign> {
    let trimmed = trim_css_value(value);
    let (baseline_source, remaining) = remove_vertical_align_baseline_source(trimmed)?;
    let vertical_align = VerticalAlign::BASELINE.with_baseline_source(baseline_source);
    if remaining.is_empty() {
        return Some(vertical_align);
    }
    let lower = remaining.to_ascii_lowercase();
    match lower.as_str() {
        "baseline" => Some(vertical_align),
        "sub" => Some(vertical_align.with_baseline_shift(BaselineShift::Sub)),
        "super" => Some(vertical_align.with_baseline_shift(BaselineShift::Super)),
        "top" => Some(
            vertical_align
                .with_baseline_shift(BaselineShift::Top)
                .with_table_cell_align(TableCellVerticalAlignKeyword::Top),
        ),
        "center" => Some(vertical_align.with_baseline_shift(BaselineShift::Center)),
        "middle" => Some(
            vertical_align
                .with_alignment_baseline(AlignmentBaseline::Metric(BaselineMetric::Middle))
                .with_table_cell_align(TableCellVerticalAlignKeyword::Middle),
        ),
        "bottom" => Some(
            vertical_align
                .with_baseline_shift(BaselineShift::Bottom)
                .with_table_cell_align(TableCellVerticalAlignKeyword::Bottom),
        ),
        "text-top" => Some(
            vertical_align
                .with_alignment_baseline(AlignmentBaseline::Metric(BaselineMetric::TextTop)),
        ),
        "text-bottom" => Some(
            vertical_align
                .with_alignment_baseline(AlignmentBaseline::Metric(BaselineMetric::TextBottom)),
        ),
        _ => {
            if let Some(alignment_baseline) = parse_alignment_baseline(remaining) {
                return Some(vertical_align.with_alignment_baseline(alignment_baseline));
            }
            parse_baseline_shift(remaining, font_size)
                .map(|baseline_shift| vertical_align.with_baseline_shift(baseline_shift))
        }
    }
}

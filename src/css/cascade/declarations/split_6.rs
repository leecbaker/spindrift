use super::*;

/// Parses one CSS margin side into its typed computed value.
///
/// CSS Box Model defines margin side properties, including `auto`, and CSS
/// Values defines length-percentage values:
/// <https://www.w3.org/TR/CSS22/box.html#margin-properties> and
/// <https://www.w3.org/TR/css-values-4/#mixed-percentages>.
pub(in crate::css) fn set_margin_side(
    value: &str,
    font_size: f32,
    set: impl FnOnce(ComputedLengthPercentageOrAuto),
) {
    if let Some(value) = parse_computed_length_percentage_auto(value, font_size) {
        set(value);
    }
}

/// Applies a logical margin axis shorthand to computed physical margin edges.
///
/// CSS Logical Properties maps `margin-block` and `margin-inline` through the
/// computed writing mode and direction, and CSS Box Model permits `auto`
/// margins:
/// <https://www.w3.org/TR/css-logical-1/#margin-properties> and
/// <https://www.w3.org/TR/CSS22/box.html#margin-properties>.
pub(in crate::css) fn apply_logical_margin_axis(
    value: &str,
    style: &mut ComputedStyle,
    name: &str,
    origin: StylesheetOrigin,
) {
    let Some([start, end]) = logical_box_axis_side_names(name) else {
        return;
    };
    let parts = split_css_component_values(trim_css_value(value));
    let [start_value, end_value] = match parts.as_slice() {
        [all] => [*all, *all],
        [start, end] => [*start, *end],
        _ => return,
    };
    apply_logical_margin_side(start_value, style, start, origin);
    apply_logical_margin_side(end_value, style, end, origin);
}

/// Applies one logical margin longhand to its resolved physical side.
///
/// CSS Logical Properties defines flow-relative margin longhands as aliases
/// for physical margin sides:
/// <https://www.w3.org/TR/css-logical-1/#margin-properties>.
pub(in crate::css) fn apply_logical_margin_side(
    value: &str,
    style: &mut ComputedStyle,
    name: &str,
    origin: StylesheetOrigin,
) {
    let Some(side) = logical_box_side(name, style.direction, style.writing_mode) else {
        return;
    };
    set_margin_side(value, style.font_size, |typed| {
        set_margin_box_side(style, side, typed);
        set_ua_margin_em_side(
            style,
            side,
            (origin == StylesheetOrigin::UserAgent)
                .then(|| parse_em_length_factor(value))
                .flatten(),
        );
    });
}

pub(in crate::css) fn set_margin_box_side(
    style: &mut ComputedStyle,
    side: BoxSide,
    typed: ComputedLengthPercentageOrAuto,
) {
    let length = typed.length_if_no_percent().unwrap_or(0.0);
    match side {
        BoxSide::Top => {
            style.box_values.margin.top = typed;
            style.margin.top = length;
        }
        BoxSide::Right => {
            style.box_values.margin.right = typed;
            style.margin.right = length;
        }
        BoxSide::Bottom => {
            style.box_values.margin.bottom = typed;
            style.margin.bottom = length;
        }
        BoxSide::Left => {
            style.box_values.margin.left = typed;
            style.margin.left = length;
        }
    }
}

pub(in crate::css) fn set_ua_margin_em_side(
    style: &mut ComputedStyle,
    side: BoxSide,
    factor: Option<f32>,
) {
    match side {
        BoxSide::Top => style.ua_margin_em.top = factor,
        BoxSide::Right => style.ua_margin_em.right = factor,
        BoxSide::Bottom => style.ua_margin_em.bottom = factor,
        BoxSide::Left => style.ua_margin_em.left = factor,
    }
}

/// Parses one CSS length-percentage declaration into its typed computed value.
///
/// CSS Values and Units defines `<length-percentage>` and CSS Cascade defines
/// the computed-value stage:
/// <https://www.w3.org/TR/css-values-4/#mixed-percentages> and
/// <https://www.w3.org/TR/css-cascade-5/#computed>.
pub(in crate::css) fn set_computed_length_percentage(
    value: &str,
    font_size: f32,
    set: impl FnOnce(ComputedLengthPercentage),
) {
    if let Some(value) = parse_computed_length_percentage(value, font_size) {
        set(value);
    }
}

/// Applies a logical padding axis shorthand to computed physical padding edges.
///
/// CSS Logical Properties maps `padding-block` and `padding-inline` through
/// the computed writing mode and direction:
/// <https://www.w3.org/TR/css-logical-1/#padding-properties>.
pub(in crate::css) fn apply_logical_padding_axis(
    value: &str,
    style: &mut ComputedStyle,
    name: &str,
) {
    let Some([start, end]) = logical_box_axis_side_names(name) else {
        return;
    };
    let parts = split_css_component_values(trim_css_value(value));
    let [start_value, end_value] = match parts.as_slice() {
        [all] => [*all, *all],
        [start, end] => [*start, *end],
        _ => return,
    };
    apply_logical_padding_side(start_value, style, start);
    apply_logical_padding_side(end_value, style, end);
}

/// Applies one logical padding longhand to its resolved physical side.
///
/// CSS Logical Properties defines flow-relative padding longhands as aliases
/// for physical padding sides:
/// <https://www.w3.org/TR/css-logical-1/#padding-properties>.
pub(in crate::css) fn apply_logical_padding_side(
    value: &str,
    style: &mut ComputedStyle,
    name: &str,
) {
    let Some(side) = logical_box_side(name, style.direction, style.writing_mode) else {
        return;
    };
    set_computed_length_percentage(value, style.font_size, |typed| {
        set_padding_box_side(style, side, typed);
    });
}

pub(in crate::css) fn set_padding_box_side(
    style: &mut ComputedStyle,
    side: BoxSide,
    typed: ComputedLengthPercentage,
) {
    let length = typed.length_if_no_percent();
    match side {
        BoxSide::Top => {
            style.box_values.padding.top = typed;
            if let Some(length) = length {
                style.padding.top = length;
            }
        }
        BoxSide::Right => {
            style.box_values.padding.right = typed;
            if let Some(length) = length {
                style.padding.right = length;
            }
        }
        BoxSide::Bottom => {
            style.box_values.padding.bottom = typed;
            if let Some(length) = length {
                style.padding.bottom = length;
            }
        }
        BoxSide::Left => {
            style.box_values.padding.left = typed;
            if let Some(length) = length {
                style.padding.left = length;
            }
        }
    }
}

/// Projects typed computed padding edges into the current length-only renderer cache.
///
/// CSS Cascade defines computed values, while CSS Box Model defines padding
/// edge properties:
/// <https://www.w3.org/TR/css-cascade-5/#computed> and
/// <https://www.w3.org/TR/CSS22/box.html#padding-properties>.
pub(in crate::css) fn legacy_edge_lengths(
    values: CssEdges<ComputedLengthPercentage>,
) -> Option<Edges> {
    Some(Edges {
        top: values.top.length_if_no_percent()?,
        right: values.right.length_if_no_percent()?,
        bottom: values.bottom.length_if_no_percent()?,
        left: values.left.length_if_no_percent()?,
    })
}

/// Projects typed computed margin edges into the current length-only renderer cache.
///
/// CSS Cascade defines computed values, while CSS Box Model defines margin
/// edge properties and `auto` margins:
/// <https://www.w3.org/TR/css-cascade-5/#computed> and
/// <https://www.w3.org/TR/CSS22/box.html#margin-properties>.
pub(in crate::css) fn legacy_margin_edges(
    values: CssEdges<ComputedLengthPercentageOrAuto>,
) -> Edges {
    Edges {
        top: values.top.length_if_no_percent().unwrap_or(0.0),
        right: values.right.length_if_no_percent().unwrap_or(0.0),
        bottom: values.bottom.length_if_no_percent().unwrap_or(0.0),
        left: values.left.length_if_no_percent().unwrap_or(0.0),
    }
}

/// Parses UA stylesheet `em` margins for delayed font-size-relative resolution.
///
/// CSS Values defines `em` units as font-relative lengths, and CSS Cascade
/// defines the computed-value stage where font-relative values are resolved:
/// <https://www.w3.org/TR/css-values-4/#font-relative-lengths> and
/// <https://www.w3.org/TR/css-cascade-5/#computed>.
pub(in crate::css) fn parse_margin_em_edges(value: &str) -> OptionalEdges<f32> {
    let parts = value.split_whitespace().collect::<Vec<_>>();
    let [top, right, bottom, left] = match parts.as_slice() {
        [] => return OptionalEdges::NONE,
        [all] => [all, all, all, all],
        [vertical, horizontal] => [vertical, horizontal, vertical, horizontal],
        [top, horizontal, bottom] => [top, horizontal, bottom, horizontal],
        [top, right, bottom, left, ..] => [top, right, bottom, left],
    };
    OptionalEdges {
        top: parse_em_length_factor(top),
        right: parse_em_length_factor(right),
        bottom: parse_em_length_factor(bottom),
        left: parse_em_length_factor(left),
    }
}

pub(in crate::css) fn parse_em_length_factor(value: &str) -> Option<f32> {
    trim_css_value(value)
        .to_ascii_lowercase()
        .strip_suffix("em")
        .and_then(|factor| factor.trim().parse::<f32>().ok())
}

pub(in crate::css) fn parse_positive_integer(value: &str) -> Option<usize> {
    let value = value.trim();
    if value.starts_with('+') || value.starts_with('-') || value.contains('.') {
        return None;
    }
    value.parse::<usize>().ok().filter(|value| *value > 0)
}

/// Parse CSS `hyphenate-limit-chars`.
///
/// CSS Text defines the grammar as one to three values, each `auto` or a
/// positive integer: total word length, minimum characters before the break,
/// and minimum characters after the break:
/// <https://www.w3.org/TR/css-text-4/#hyphenate-limit-chars>.
pub(in crate::css) fn parse_hyphenate_limit_chars(value: &str) -> Option<HyphenateLimitChars> {
    let parts = value.split_whitespace().collect::<Vec<_>>();
    if parts.is_empty() || parts.len() > 3 {
        return None;
    }
    let total = parse_hyphenate_limit_component(parts[0], HyphenateLimitChars::AUTO_TOTAL)?;
    let before = parts
        .get(1)
        .map(|value| parse_hyphenate_limit_component(value, HyphenateLimitChars::AUTO_BEFORE))
        .unwrap_or(Some(HyphenateLimitChars::AUTO_BEFORE))?;
    let after = parts
        .get(2)
        .map(|value| parse_hyphenate_limit_component(value, HyphenateLimitChars::AUTO_AFTER))
        .unwrap_or(Some(HyphenateLimitChars::AUTO_AFTER))?;
    Some(HyphenateLimitChars {
        total,
        before,
        after,
    })
}

pub(in crate::css) fn parse_hyphenate_limit_component(value: &str, auto_value: u16) -> Option<u16> {
    if value.eq_ignore_ascii_case("auto") {
        return Some(auto_value);
    }
    u16::try_from(parse_positive_integer(value)?).ok()
}

pub(in crate::css) fn is_css_identifier(value: &str) -> bool {
    let mut chars = value.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    (first == '_' || first == '-' || first.is_ascii_alphabetic())
        && chars.all(|character| {
            character == '_' || character == '-' || character.is_ascii_alphanumeric()
        })
}

pub(in crate::css) fn parse_running_position(value: &str) -> Option<String> {
    let value = value.trim();
    let prefix = value.get(.."running".len())?;
    if !prefix.eq_ignore_ascii_case("running") {
        return None;
    }
    let argument = value["running".len()..]
        .trim_start()
        .strip_prefix('(')?
        .strip_suffix(')')?
        .trim();
    is_css_identifier(argument).then(|| argument.to_string())
}

pub(in crate::css) fn parse_page_break(value: &str) -> PageBreak {
    match value.trim().to_ascii_lowercase().as_str() {
        "avoid" | "avoid-page" => PageBreak::Avoid,
        "page" | "always" => PageBreak::Page,
        "left" => PageBreak::Left,
        "right" => PageBreak::Right,
        "recto" => PageBreak::Recto,
        "verso" => PageBreak::Verso,
        _ => PageBreak::Auto,
    }
}

/// Computes the writing context used to resolve logical properties.
///
/// CSS Logical Properties maps flow-relative properties through the computed
/// `direction` and `writing-mode` values. This prepass runs before shorthand
/// expansion and Cascade 5 rollback so logical and physical border longhands
/// compare in the right physical space:
/// <https://www.w3.org/TR/css-logical-1/#flow-relative> and
/// <https://www.w3.org/TR/css-cascade-5/#cascade>.
pub(in crate::css) fn logical_mapping_context(
    base_style: &ComputedStyle,
    declarations: &[CascadedDeclaration<'_>],
    inheritance_source: &ComputedStyle,
) -> (Direction, WritingMode) {
    let mut direction = base_style.direction;
    let mut writing_mode = base_style.writing_mode;
    for declaration in declarations {
        let name = declaration.name.as_ref();
        let value = trim_css_value(&declaration.value);
        if value.contains("var(")
            || declaration_is_revert(value)
            || declaration_is_revert_layer(value)
        {
            continue;
        }
        if name.eq_ignore_ascii_case("all") {
            if let Some(keyword) = CssWideDefaultKeyword::parse(value) {
                writing_mode = defaulted_writing_mode(keyword, inheritance_source);
            }
            continue;
        }
        if let Some(keyword) = CssWideDefaultKeyword::parse(value) {
            match name {
                "direction" => direction = defaulted_direction(keyword, inheritance_source),
                "writing-mode" => {
                    writing_mode = defaulted_writing_mode(keyword, inheritance_source);
                }
                _ => {}
            }
            continue;
        }
        match name {
            "direction" => {
                if let Some(parsed) = parse_direction(value) {
                    direction = parsed;
                }
            }
            "writing-mode" => {
                if let Some(parsed) = parse_writing_mode(value) {
                    writing_mode = parsed;
                }
            }
            _ => {}
        }
    }
    (direction, writing_mode)
}

pub(in crate::css) fn defaulted_direction(
    keyword: CssWideDefaultKeyword,
    inheritance_source: &ComputedStyle,
) -> Direction {
    match keyword {
        CssWideDefaultKeyword::Initial => ComputedStyle::initial().direction,
        CssWideDefaultKeyword::Inherit | CssWideDefaultKeyword::Unset => {
            inheritance_source.direction
        }
    }
}

pub(in crate::css) fn defaulted_writing_mode(
    keyword: CssWideDefaultKeyword,
    inheritance_source: &ComputedStyle,
) -> WritingMode {
    match keyword {
        CssWideDefaultKeyword::Initial => ComputedStyle::initial().writing_mode,
        CssWideDefaultKeyword::Inherit | CssWideDefaultKeyword::Unset => {
            inheritance_source.writing_mode
        }
    }
}

/// Parses CSS `direction`.
///
/// CSS Writing Modes defines `direction` keywords as `ltr` and `rtl`:
/// <https://www.w3.org/TR/css-writing-modes-4/#direction>.
pub(in crate::css) fn parse_direction(value: &str) -> Option<Direction> {
    match trim_css_value(value).to_ascii_lowercase().as_str() {
        "ltr" => Some(Direction::Ltr),
        "rtl" => Some(Direction::Rtl),
        _ => None,
    }
}

/// Parses CSS `writing-mode` values currently supported by layout.
///
/// CSS Writing Modes defines horizontal and vertical flow modes; sideways
/// modes are intentionally left unsupported until text layout supports them:
/// <https://www.w3.org/TR/css-writing-modes-4/#block-flow>.
pub(in crate::css) fn parse_writing_mode(value: &str) -> Option<WritingMode> {
    match trim_css_value(value).to_ascii_lowercase().as_str() {
        "horizontal-tb" => Some(WritingMode::HorizontalTb),
        "vertical-rl" => Some(WritingMode::VerticalRl),
        "vertical-lr" => Some(WritingMode::VerticalLr),
        _ => None,
    }
}

/// Parses CSS `text-orientation` values supported by vertical text placement.
///
/// CSS Writing Modes defines `mixed`, `upright`, and `sideways` as the modern
/// orientation keywords. Deprecated SVG aliases are intentionally left
/// unsupported until compatibility tests require them:
/// <https://www.w3.org/TR/css-writing-modes-4/#text-orientation>.
pub(in crate::css) fn parse_text_orientation(value: &str) -> Option<TextOrientation> {
    match trim_css_value(value).to_ascii_lowercase().as_str() {
        "mixed" => Some(TextOrientation::Mixed),
        "upright" => Some(TextOrientation::Upright),
        "sideways" => Some(TextOrientation::Sideways),
        _ => None,
    }
}

/// Parses CSS `opacity`.
///
/// CSS Color defines `opacity` as a number or percentage clamped to the
/// `[0, 1]` range:
/// <https://www.w3.org/TR/css-color-4/#transparency>.
pub(in crate::css) fn parse_opacity(value: &str) -> Option<f32> {
    let value = trim_css_value(value);
    if let Some(percent) = parse_percentage(value) {
        return Some(percent.clamp(0.0, 1.0));
    }
    value.parse::<f32>().ok().map(|value| value.clamp(0.0, 1.0))
}

/// Parses the supported CSS 2D `transform` function list.
///
/// CSS Transforms Level 1 defines the 2D functions accepted here. The
/// implementation intentionally rejects 3D functions until the layout and PDF
/// backends carry 3D matrices:
/// <https://www.w3.org/TR/css-transforms-1/#two-d-transform-functions>.
pub(in crate::css) fn parse_transform(value: &str, font_size: f32) -> Option<TransformList> {
    let value = trim_css_value(value);
    if value.eq_ignore_ascii_case("none") {
        return Some(Vec::new());
    }
    let functions = parse_transform_function_calls(value)?;
    let mut transform = Vec::new();
    for (name, args) in functions {
        let lower_name = name.to_ascii_lowercase();
        let comma_args = split_css_args(args, ',');
        let args = if comma_args.len() > 1 {
            comma_args
        } else {
            split_css_whitespace_args(args)
        };
        match lower_name.as_str() {
            "matrix" if args.len() == 6 => {
                transform.push(TransformFunction::Matrix(
                    parse_css_number(args[0])?,
                    parse_css_number(args[1])?,
                    parse_css_number(args[2])?,
                    parse_css_number(args[3])?,
                    parse_css_number(args[4])?,
                    parse_css_number(args[5])?,
                ));
            }
            "translate" if args.len() == 1 || args.len() == 2 => {
                let x = parse_computed_length_percentage(args[0], font_size)?;
                let y = args
                    .get(1)
                    .and_then(|arg| parse_computed_length_percentage(arg, font_size))
                    .unwrap_or(ComputedLengthPercentage::ZERO);
                transform.push(TransformFunction::Translate(x, y));
            }
            "translatex" if args.len() == 1 => {
                transform.push(TransformFunction::Translate(
                    parse_computed_length_percentage(args[0], font_size)?,
                    ComputedLengthPercentage::ZERO,
                ));
            }
            "translatey" if args.len() == 1 => {
                transform.push(TransformFunction::Translate(
                    ComputedLengthPercentage::ZERO,
                    parse_computed_length_percentage(args[0], font_size)?,
                ));
            }
            "scale" if args.len() == 1 || args.len() == 2 => {
                let x = parse_css_number(args[0])?;
                let y = args
                    .get(1)
                    .and_then(|arg| parse_css_number(arg))
                    .unwrap_or(x);
                transform.push(TransformFunction::Scale(x, y));
            }
            "scalex" if args.len() == 1 => {
                transform.push(TransformFunction::Scale(parse_css_number(args[0])?, 1.0));
            }
            "scaley" if args.len() == 1 => {
                transform.push(TransformFunction::Scale(1.0, parse_css_number(args[0])?));
            }
            "rotate" if args.len() == 1 => {
                transform.push(TransformFunction::Rotate(parse_css_angle_radians(args[0])?));
            }
            "skew" if args.len() == 1 || args.len() == 2 => {
                let x = parse_css_angle_radians(args[0])?;
                let y = args
                    .get(1)
                    .and_then(|arg| parse_css_angle_radians(arg))
                    .unwrap_or(0.0);
                transform.push(TransformFunction::Skew(x, y));
            }
            "skewx" if args.len() == 1 => {
                transform.push(TransformFunction::Skew(
                    parse_css_angle_radians(args[0])?,
                    0.0,
                ));
            }
            "skewy" if args.len() == 1 => {
                transform.push(TransformFunction::Skew(
                    0.0,
                    parse_css_angle_radians(args[0])?,
                ));
            }
            _ => return None,
        }
    }
    Some(transform)
}

/// Parses 2D `transform-origin`, ignoring a third z-origin component.
///
/// CSS Transforms resolves keyword origins to percentages over the border box:
/// <https://www.w3.org/TR/css-transforms-1/#transform-origin-property>.
pub(in crate::css) fn parse_transform_origin(
    value: &str,
    font_size: f32,
) -> Option<TransformOrigin> {
    let parts = split_css_whitespace_args(trim_css_value(value));
    if parts.is_empty() || parts.len() > 3 {
        return None;
    }
    let parts = &parts[..parts.len().min(2)];
    match parts {
        [single] if is_vertical_origin_keyword(single) => Some(TransformOrigin {
            x: ComputedLengthPercentage::from_percent(0.5),
            y: parse_origin_component(single, true, font_size)?,
        }),
        [single] => Some(TransformOrigin {
            x: parse_origin_component(single, false, font_size)?,
            y: ComputedLengthPercentage::from_percent(0.5),
        }),
        [first, second] if is_vertical_origin_keyword(first) => Some(TransformOrigin {
            x: parse_origin_component(second, false, font_size)?,
            y: parse_origin_component(first, true, font_size)?,
        }),
        [first, second] => Some(TransformOrigin {
            x: parse_origin_component(first, false, font_size)?,
            y: parse_origin_component(second, true, font_size)?,
        }),
        _ => None,
    }
}

pub(in crate::css) fn parse_origin_component(
    value: &str,
    vertical: bool,
    font_size: f32,
) -> Option<ComputedLengthPercentage> {
    match value.to_ascii_lowercase().as_str() {
        "left" if !vertical => Some(ComputedLengthPercentage::ZERO),
        "right" if !vertical => Some(ComputedLengthPercentage::from_percent(1.0)),
        "top" if vertical => Some(ComputedLengthPercentage::ZERO),
        "bottom" if vertical => Some(ComputedLengthPercentage::from_percent(1.0)),
        "center" => Some(ComputedLengthPercentage::from_percent(0.5)),
        _ => parse_computed_length_percentage(value, font_size),
    }
}

pub(in crate::css) fn is_vertical_origin_keyword(value: &str) -> bool {
    matches!(value.to_ascii_lowercase().as_str(), "top" | "bottom")
}

/// Parse CSS Compositing's `mix-blend-mode` keywords.
///
/// Non-`normal` values establish a stacking context:
/// <https://www.w3.org/TR/compositing-1/#mix-blend-mode>.
pub(in crate::css) fn parse_mix_blend_mode(value: &str) -> Option<MixBlendMode> {
    match trim_css_value(value).to_ascii_lowercase().as_str() {
        "normal" => Some(MixBlendMode::Normal),
        "multiply" => Some(MixBlendMode::Multiply),
        "screen" => Some(MixBlendMode::Screen),
        "overlay" => Some(MixBlendMode::Overlay),
        "darken" => Some(MixBlendMode::Darken),
        "lighten" => Some(MixBlendMode::Lighten),
        "color-dodge" => Some(MixBlendMode::ColorDodge),
        "color-burn" => Some(MixBlendMode::ColorBurn),
        "hard-light" => Some(MixBlendMode::HardLight),
        "soft-light" => Some(MixBlendMode::SoftLight),
        "difference" => Some(MixBlendMode::Difference),
        "exclusion" => Some(MixBlendMode::Exclusion),
        "hue" => Some(MixBlendMode::Hue),
        "saturation" => Some(MixBlendMode::Saturation),
        "color" => Some(MixBlendMode::Color),
        "luminosity" => Some(MixBlendMode::Luminosity),
        _ => None,
    }
}

/// Parse the `contain` keywords that affect paint isolation.
///
/// CSS Containment maps `strict` to size/layout/style/paint and `content` to
/// layout/style/paint containment:
/// <https://www.w3.org/TR/css-contain-2/#contain-property>.
pub(in crate::css) fn parse_contain(value: &str) -> Option<Contain> {
    let value = trim_css_value(value).to_ascii_lowercase();
    if value == "none" {
        return Some(Contain::NONE);
    }
    if value == "strict" {
        return Some(Contain {
            size: true,
            layout: true,
            style: true,
            paint: true,
        });
    }
    if value == "content" {
        return Some(Contain {
            size: false,
            layout: true,
            style: true,
            paint: true,
        });
    }

    let mut contain = Contain::NONE;
    for token in value.split_whitespace() {
        match token {
            "size" | "inline-size" => contain.size = true,
            "layout" => contain.layout = true,
            "style" => contain.style = true,
            "paint" => contain.paint = true,
            _ => return None,
        }
    }
    (contain.size || contain.layout || contain.style || contain.paint).then_some(contain)
}

/// Parse `clip-path` coarsely enough to identify stacking-context triggers.
///
/// The current paint layer records non-`none` values as isolation triggers; path
/// geometry is implemented later in paint effects:
/// <https://www.w3.org/TR/css-masking-1/#the-clip-path>.
pub(in crate::css) fn parse_clip_path(value: &str) -> Option<ClipPath> {
    let value = trim_css_value(value);
    if value.eq_ignore_ascii_case("none") {
        return Some(ClipPath::None);
    }
    let lower = value.to_ascii_lowercase();
    if lower.starts_with("url(") {
        Some(ClipPath::Url)
    } else if lower.starts_with("inset(") {
        Some(ClipPath::Inset)
    } else if lower.ends_with(')') {
        Some(ClipPath::Shape)
    } else {
        None
    }
}

/// Parse `will-change` features that may pre-create stacking contexts.
///
/// CSS Will Change lets authors request the same stacking behavior that the
/// named property would have at a non-initial value:
/// <https://www.w3.org/TR/css-will-change-1/#will-change>.
pub(in crate::css) fn parse_will_change(value: &str) -> Option<WillChange> {
    let value = trim_css_value(value);
    if value.eq_ignore_ascii_case("auto") {
        return Some(WillChange::default());
    }
    let mut will_change = WillChange::default();
    for token in value
        .split(',')
        .map(str::trim)
        .filter(|token| !token.is_empty())
    {
        match token.to_ascii_lowercase().as_str() {
            "contents" => will_change.contents = true,
            "scroll-position" => will_change.scroll_position = true,
            "opacity" => will_change.opacity = true,
            "transform" => will_change.transform = true,
            "filter" => will_change.filter = true,
            "clip-path" => will_change.clip_path = true,
            "mask" | "mask-image" => will_change.mask = true,
            "mix-blend-mode" => will_change.mix_blend_mode = true,
            "isolation" => will_change.isolation = true,
            "contain" => will_change.contain = true,
            _ => return None,
        }
    }
    Some(will_change)
}

pub(in crate::css) fn parse_transform_function_calls(value: &str) -> Option<Vec<(&str, &str)>> {
    let mut calls = Vec::new();
    let mut rest = trim_css_value(value);
    while !rest.is_empty() {
        let open = rest.find('(')?;
        let name = trim_css_value(&rest[..open]);
        if name.is_empty() {
            return None;
        }
        let close = find_matching_close_paren(rest, open)?;
        calls.push((name, &rest[open + 1..close]));
        rest = trim_css_value(&rest[close + 1..]);
    }
    Some(calls)
}

pub(in crate::css) fn find_matching_close_paren(value: &str, open: usize) -> Option<usize> {
    let mut depth = 0usize;
    for (index, byte) in value.bytes().enumerate().skip(open) {
        match byte {
            b'(' => depth += 1,
            b')' => {
                depth = depth.checked_sub(1)?;
                if depth == 0 {
                    return Some(index);
                }
            }
            _ => {}
        }
    }
    None
}

pub(in crate::css) fn split_css_args(value: &str, delimiter: char) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut start = 0usize;
    let mut depth = 0usize;
    for (index, character) in value.char_indices() {
        match character {
            '(' => depth += 1,
            ')' => depth = depth.saturating_sub(1),
            candidate if candidate == delimiter && depth == 0 => {
                parts.push(trim_css_value(&value[start..index]));
                start = index + candidate.len_utf8();
            }
            _ => {}
        }
    }
    parts.push(trim_css_value(&value[start..]));
    parts.into_iter().filter(|part| !part.is_empty()).collect()
}

pub(in crate::css) fn split_css_whitespace_args(value: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut start = None;
    let mut depth = 0usize;
    for (index, character) in value.char_indices() {
        match character {
            '(' => {
                if start.is_none() {
                    start = Some(index);
                }
                depth += 1;
            }
            ')' => depth = depth.saturating_sub(1),
            character if character.is_whitespace() && depth == 0 => {
                if let Some(part_start) = start.take() {
                    let part = trim_css_value(&value[part_start..index]);
                    if !part.is_empty() {
                        parts.push(part);
                    }
                }
            }
            _ if start.is_none() => start = Some(index),
            _ => {}
        }
    }
    if let Some(part_start) = start {
        let part = trim_css_value(&value[part_start..]);
        if !part.is_empty() {
            parts.push(part);
        }
    }
    parts
}

pub(in crate::css) fn parse_css_number(value: &str) -> Option<f32> {
    trim_css_value(value).parse::<f32>().ok()
}

pub(in crate::css) fn parse_css_angle_radians(value: &str) -> Option<f32> {
    let value = trim_css_value(value);
    let lower = value.to_ascii_lowercase();
    if let Some(number) = lower.strip_suffix("deg") {
        return parse_css_number(number).map(f32::to_radians);
    }
    if let Some(number) = lower.strip_suffix("grad") {
        return parse_css_number(number).map(|value| value * std::f32::consts::PI / 200.0);
    }
    if let Some(number) = lower.strip_suffix("turn") {
        return parse_css_number(number).map(|value| value * std::f32::consts::TAU);
    }
    lower
        .strip_suffix("rad")
        .and_then(parse_css_number)
        .or_else(|| parse_css_number(value).filter(|value| *value == 0.0))
}

/// Parses the `flex-flow` shorthand into `flex-direction` and `flex-wrap`.
///
/// CSS Flexible Box Layout defines `flex-flow` as
/// `<'flex-direction'> || <'flex-wrap'>`; omitted components reset to their
/// initial values (`row` and `nowrap`):
/// <https://www.w3.org/TR/css-flexbox-1/#flex-flow-property>.
pub(in crate::css) fn parse_flex_flow(value: &str) -> Option<(FlexDirection, FlexWrap)> {
    let mut direction = FlexDirection::Row;
    let mut wrap = FlexWrap::NoWrap;
    let mut saw_direction = false;
    let mut saw_wrap = false;
    for token in trim_css_value(value).split_whitespace() {
        match token.to_ascii_lowercase().as_str() {
            "row" if !saw_direction => {
                direction = FlexDirection::Row;
                saw_direction = true;
            }
            "row-reverse" if !saw_direction => {
                direction = FlexDirection::RowReverse;
                saw_direction = true;
            }
            "column" if !saw_direction => {
                direction = FlexDirection::Column;
                saw_direction = true;
            }
            "column-reverse" if !saw_direction => {
                direction = FlexDirection::ColumnReverse;
                saw_direction = true;
            }
            "nowrap" if !saw_wrap => {
                wrap = FlexWrap::NoWrap;
                saw_wrap = true;
            }
            "wrap" if !saw_wrap => {
                wrap = FlexWrap::Wrap;
                saw_wrap = true;
            }
            "wrap-reverse" if !saw_wrap => {
                wrap = FlexWrap::WrapReverse;
                saw_wrap = true;
            }
            _ => return None,
        }
    }
    (saw_direction || saw_wrap).then_some((direction, wrap))
}

/// Parses a single CSS Overflow keyword.
///
/// CSS Overflow defines the `overflow`, `overflow-x`, and `overflow-y`
/// properties as keyword values controlling visible, clipped, and scrollable
/// overflow:
/// <https://www.w3.org/TR/css-overflow-3/#overflow-properties>.
pub(in crate::css) fn parse_overflow_value(value: &str) -> Option<Overflow> {
    match trim_css_value(value).to_ascii_lowercase().as_str() {
        "visible" => Some(Overflow::Visible),
        "hidden" => Some(Overflow::Hidden),
        "clip" => Some(Overflow::Clip),
        "scroll" => Some(Overflow::Scroll),
        "auto" => Some(Overflow::Auto),
        _ => None,
    }
}

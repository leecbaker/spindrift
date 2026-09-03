use super::*;

pub(in crate::css) fn apply_cascaded_visual_declaration(
    style: &mut ComputedStyle,
    name: &str,
    value: &str,
    declaration: &CascadedDeclaration<'_>,
    inheritance_source: &ComputedStyle,
    parent_ch_advance: LayoutLength,
) -> bool {
    if apply_cascaded_sizing_declaration(
        style,
        name,
        value,
        declaration,
        inheritance_source,
        parent_ch_advance,
    ) || apply_cascaded_positioning_declaration(
        style,
        name,
        value,
        declaration,
        inheritance_source,
        parent_ch_advance,
    ) {
        return true;
    }
    if apply_cascaded_table_declaration(
        style,
        name,
        value,
        declaration,
        inheritance_source,
        parent_ch_advance,
    ) {
        return true;
    }
    match name {
        "outline-offset" => {
            if let Some(length) = parse_computed_length_percentage(value, style.font_size)
                && !length.contains_percentage()
            {
                style.outline_offset = length;
            }
        }
        "border-block-style" => {
            if let Some([start, end]) = parse_logical_border_styles(value)
                && let Some([start_side, end_side]) =
                    logical_axis_sides(name, style.direction, style.writing_mode)
            {
                set_border_side_style_value(style, start_side, start);
                set_border_side_style_value(style, end_side, end);
            }
        }
        "border-inline-style" => {
            if let Some([start, end]) = parse_logical_border_styles(value)
                && let Some([start_side, end_side]) =
                    logical_axis_sides(name, style.direction, style.writing_mode)
            {
                set_border_side_style_value(style, start_side, start);
                set_border_side_style_value(style, end_side, end);
            }
        }
        "border-radius" => {
            if let Some(radius) = parse_border_radius(value, style.font_size) {
                style.border_radius = radius;
            }
        }
        "border-shape" => {
            if let Some(shape) = parse_border_shape(value, style.font_size) {
                style.border_shape = shape;
            }
        }
        "shape-outside" => {
            if let Some(shape) = parse_shape_outside_with_urls(
                value,
                style.font_size,
                declaration.base_url,
                declaration.root_url,
            ) {
                style.shape_outside = shape;
            }
        }
        "shape-margin" => {
            if let Some(margin) = parse_computed_length_percentage(value, style.font_size)
                && !margin.is_definitely_negative()
            {
                style.shape_margin = margin;
            }
        }
        "shape-image-threshold" => {
            if let Some(threshold) =
                parse_css_number(value).and_then(ShapeImageThreshold::new_clamped)
            {
                style.shape_image_threshold = threshold;
            }
        }
        "corner" => {
            if let Some((radius, shapes)) = parse_corner_shorthand(value, style.font_size) {
                style.border_radius = radius;
                style.corner_shapes = shapes;
            }
        }
        "corner-shape" => {
            if let Some(shapes) = parse_corner_shapes(value) {
                style.corner_shapes = shapes;
            }
        }
        "border-top-left-radius" => {
            if let Some(radius) = parse_corner_radius(value, style.font_size) {
                style.border_radius.top_left = radius;
            }
        }
        "border-top-right-radius" => {
            if let Some(radius) = parse_corner_radius(value, style.font_size) {
                style.border_radius.top_right = radius;
            }
        }
        "border-bottom-right-radius" => {
            if let Some(radius) = parse_corner_radius(value, style.font_size) {
                style.border_radius.bottom_right = radius;
            }
        }
        "border-bottom-left-radius" => {
            if let Some(radius) = parse_corner_radius(value, style.font_size) {
                style.border_radius.bottom_left = radius;
            }
        }
        "corner-top-left-shape" => {
            if let Some(shape) = parse_corner_shape(value) {
                style.corner_shapes.top_left = shape;
            }
        }
        "corner-top-right-shape" => {
            if let Some(shape) = parse_corner_shape(value) {
                style.corner_shapes.top_right = shape;
            }
        }
        "corner-bottom-right-shape" => {
            if let Some(shape) = parse_corner_shape(value) {
                style.corner_shapes.bottom_right = shape;
            }
        }
        "corner-bottom-left-shape" => {
            if let Some(shape) = parse_corner_shape(value) {
                style.corner_shapes.bottom_left = shape;
            }
        }
        "border-start-start-radius"
        | "border-start-end-radius"
        | "border-end-start-radius"
        | "border-end-end-radius" => {
            if let Some(physical) =
                logical_corner_radius_longhand(name, style.direction, style.writing_mode)
                && let Some(radius) = parse_corner_radius(value, style.font_size)
            {
                match physical {
                    "border-top-left-radius" => style.border_radius.top_left = radius,
                    "border-top-right-radius" => style.border_radius.top_right = radius,
                    "border-bottom-right-radius" => style.border_radius.bottom_right = radius,
                    "border-bottom-left-radius" => style.border_radius.bottom_left = radius,
                    _ => {}
                }
            }
        }
        "border-top-style" => set_border_side_style(style, BorderSide::Top, value),
        "border-right-style" => set_border_side_style(style, BorderSide::Right, value),
        "border-bottom-style" => set_border_side_style(style, BorderSide::Bottom, value),
        "border-left-style" => set_border_side_style(style, BorderSide::Left, value),
        "border-block-start-style"
        | "border-block-end-style"
        | "border-inline-start-style"
        | "border-inline-end-style" => {
            if let Some(side) = logical_border_side(name, style.direction, style.writing_mode) {
                set_border_side_style(style, side, value);
            }
        }
        "background" => {
            apply_background_shorthand(style, value, declaration.base_url, declaration.root_url)
        }
        "background-color" => {
            style.background.background_color = if value.eq_ignore_ascii_case("currentcolor") {
                BackgroundColor::CurrentColor
            } else if parse_color_from_currentcolor_in_scheme(
                value,
                style.color,
                style.used_color_scheme,
            )
            .is_some()
            {
                BackgroundColor::RelativeCurrentColor {
                    expression: value.to_string(),
                    used_color_scheme: style.used_color_scheme,
                }
            } else if let Some(color) = parse_color(value) {
                BackgroundColor::Color(color)
            } else {
                style.background.background_color.clone()
            };
        }
        "background-image" => {
            apply_background_image_list(style, value, declaration.base_url, declaration.root_url);
        }
        "background-size" => {
            apply_background_size_list(style, value);
        }
        "object-fit" => {
            if let Some(object_fit) = parse_object_fit(value) {
                style.object_fit = object_fit;
            }
        }
        "object-view-box" => {
            if let Some(view_box) = parse_object_view_box(value, style.font_size) {
                style.object_view_box = view_box;
            }
        }
        "object-position" => {
            if let Some(object_position) = parse_background_position(value, style.font_size) {
                style.object_position = object_position;
            }
        }
        "image-orientation" => {
            if let Some(image_orientation) = parse_image_orientation(value) {
                style.image_orientation = image_orientation;
            }
        }
        "image-rendering" => {
            if let Some(image_rendering) = parse_image_rendering(value) {
                style.image_rendering = image_rendering;
            }
        }
        "background-position" => {
            apply_background_position_list(style, value);
        }
        "background-position-x" => {
            apply_background_position_axis_list(style, value, true);
        }
        "background-position-y" => {
            apply_background_position_axis_list(style, value, false);
        }
        "background-repeat" => {
            apply_background_repeat_list(style, value);
        }
        "background-attachment" => {
            apply_background_attachment_list(style, value);
        }
        "background-origin" => {
            apply_background_origin_list(style, value);
        }
        "background-clip" => {
            apply_background_clip_list(style, value);
        }
        "border-image" => {
            if let Some(mut border_image) = parse_border_image(value, style.font_size) {
                border_image.source_base_url = border_image
                    .source
                    .is_image()
                    .then(|| declaration.base_url.cloned())
                    .flatten();
                border_image.source_root_url = border_image
                    .source
                    .is_image()
                    .then(|| declaration.root_url.cloned())
                    .flatten();
                style.border_image = border_image;
            }
        }
        "border-image-source" => {
            if let Some(source) = parse_border_image_source(value) {
                style.border_image.source = source;
                style.border_image.source_base_url = style
                    .border_image
                    .source
                    .is_image()
                    .then(|| declaration.base_url.cloned())
                    .flatten();
                style.border_image.source_root_url = style
                    .border_image
                    .source
                    .is_image()
                    .then(|| declaration.root_url.cloned())
                    .flatten();
            }
        }
        "border-image-slice" => {
            if let Some(slice) = parse_border_image_slice(value) {
                style.border_image.slice = slice;
            }
        }
        "border-image-width" => {
            if let Some(width) = parse_border_image_width(value, style.font_size) {
                style.border_image.width = width;
            }
        }
        "border-image-outset" => {
            if let Some(outset) = parse_border_image_outset(value, style.font_size) {
                style.border_image.outset = outset;
            }
        }
        "border-image-repeat" => {
            if let Some(repeat) = parse_border_image_repeat(value) {
                style.border_image.repeat = repeat;
            }
        }
        "color" => {
            // The color prepass resolves this property before every dependent
            // `currentcolor` property. Reapplying the whole declaration list
            // here would let a lower-priority declaration overwrite its
            // winning computed value.
            // <https://drafts.csswg.org/css-color-4/#currentcolor-color>
        }
        "forced-color-adjust" => {
            style.forced_color_adjust = match value.to_ascii_lowercase().as_str() {
                "auto" => ForcedColorAdjust::Auto,
                "none" => ForcedColorAdjust::None,
                "preserve-parent-color" => ForcedColorAdjust::PreserveParentColor,
                _ => style.forced_color_adjust,
            };
        }
        "fill" => {
            if value.eq_ignore_ascii_case("none") {
                style.svg_fill = SvgPresentationPaint::host_css(SvgPaint::None);
            } else if let Some(color) =
                parse_color_from_currentcolor_in_scheme(value, style.color, style.used_color_scheme)
                    .or_else(|| parse_color(value))
            {
                let paint = if value.eq_ignore_ascii_case("currentcolor") {
                    SvgPaint::CurrentColor
                } else {
                    SvgPaint::Color(color)
                };
                style.svg_fill = SvgPresentationPaint::host_css(paint);
            }
        }
        "stroke" => {
            if value.eq_ignore_ascii_case("none") {
                style.svg_stroke = SvgPresentationPaint::host_css(SvgPaint::None);
            } else if let Some(color) =
                parse_color_from_currentcolor_in_scheme(value, style.color, style.used_color_scheme)
                    .or_else(|| parse_color(value))
            {
                let paint = if value.eq_ignore_ascii_case("currentcolor") {
                    SvgPaint::CurrentColor
                } else {
                    SvgPaint::Color(color)
                };
                style.svg_stroke = SvgPresentationPaint::host_css(paint);
            }
        }
        "stroke-width" => {
            if let Some(width) = parse_computed_length_percentage(value, style.font_size)
                && !width.contains_percentage()
                && !width.is_definitely_negative()
            {
                style.svg_stroke_width = SvgStrokeWidth::HostCss(width);
            }
        }
        "flood-color" => {
            if let Some(color) =
                parse_color_from_currentcolor_in_scheme(value, style.color, style.used_color_scheme)
                    .or_else(|| parse_color(value))
            {
                style.svg_flood_color = SvgFilterColor {
                    color,
                    current_color_dependent: crate::css::color_depends_on_currentcolor(value),
                };
            }
        }
        "lighting-color" => {
            if let Some(color) =
                parse_color_from_currentcolor_in_scheme(value, style.color, style.used_color_scheme)
                    .or_else(|| parse_color(value))
            {
                style.svg_lighting_color = SvgFilterColor {
                    color,
                    current_color_dependent: crate::css::color_depends_on_currentcolor(value),
                };
            }
        }
        "-webkit-text-fill-color" => {
            if value.eq_ignore_ascii_case("currentcolor") {
                style.text_fill_color = CssColorOrCurrentColor::CurrentColor;
            } else if let Some(color) =
                parse_color_from_currentcolor_in_scheme(value, style.color, style.used_color_scheme)
                    .or_else(|| parse_color(value))
            {
                style.text_fill_color = CssColorOrCurrentColor::Color(color);
            }
        }
        "zoom" => {
            if let Some(zoom) = CssZoom::parse(value) {
                style.zoom = zoom;
            }
        }
        "font-size" => {
            // Applied in a pre-pass so same-rule `em` lengths use the
            // element's computed font size instead of declaration order.
        }
        "line-height" => {
            // CSS Values resolves `lh` in the `line-height` property against
            // the inherited line height, not the value being established by
            // this declaration.
            // <https://www.w3.org/TR/css-values-4/#lh>
            if let Some(mut line_height) = parse_computed_line_height(value, style.font_size) {
                line_height.resolve_inherited_line_height_relative_lengths(layout_pt(
                    inheritance_source.line_height,
                ));
                style.line_height_value = line_height;
                project_line_height(style);
            }
        }
        "letter-spacing" => {
            if let Some(letter_spacing) = parse_letter_spacing(value, style.font_size) {
                style.letter_spacing = letter_spacing;
            }
        }
        "word-spacing" => {
            if let Some(word_spacing) = parse_word_spacing(value, style.font_size) {
                style.word_spacing = word_spacing;
            }
        }
        "opacity" => {
            if let Some(opacity) = parse_opacity(value) {
                style.opacity = opacity;
            }
        }
        "ruby-position" => {
            if let Some(position) = parse_ruby_position(value) {
                style.ruby_position = position;
            }
        }
        "ruby-align" => {
            if let Some(align) = parse_ruby_align(value) {
                style.ruby_align = align;
            }
        }
        "ruby-overhang" => {
            if let Some(overhang) = parse_ruby_overhang(value) {
                style.ruby_overhang = overhang;
            }
        }
        "transform" => {
            if let Some(transform) = parse_transform(value, style.font_size) {
                style.transform = transform;
            }
        }
        "translate" => {
            if let Some(translate) = parse_individual_translate(value, style.font_size) {
                style.individual_transforms.translate = translate;
            }
        }
        "rotate" => {
            if let Some(rotate) = parse_individual_rotate(value) {
                style.individual_transforms.rotate = rotate;
            }
        }
        "scale" => {
            if let Some(scale) = parse_individual_scale(value) {
                style.individual_transforms.scale = scale;
            }
        }
        "transform-origin" => {
            if let Some(origin) = parse_transform_origin(value, style.font_size) {
                style.transform_origin = origin;
            }
        }
        "perspective" => {
            if value.eq_ignore_ascii_case("none") {
                style.perspective = ComputedPerspective::NONE;
            } else if let Some(length) = parse_computed_length_percentage(value, style.font_size)
                && let Some(length) = NonNegativeComputedLength::new(length)
            {
                style.perspective = ComputedPerspective::Distance(length);
            }
        }
        "perspective-origin" => {
            if let Some(origin) = parse_perspective_origin(value, style.font_size) {
                style.perspective_origin = origin;
            }
        }
        "transform-box" => {
            if let Some(transform_box) = parse_transform_box(value) {
                style.transform_box = transform_box;
            }
        }
        "transform-style" => {
            if let Some(transform_style) = parse_transform_style(value) {
                style.transform_style = transform_style;
            }
        }
        "backface-visibility" => {
            style.backface_visibility = match value.to_ascii_lowercase().as_str() {
                "visible" => BackfaceVisibility::Visible,
                "hidden" => BackfaceVisibility::Hidden,
                _ => style.backface_visibility,
            };
        }
        "isolation" => {
            style.isolation = match value.to_ascii_lowercase().as_str() {
                "isolate" => Isolation::Isolate,
                "auto" => Isolation::Auto,
                _ => style.isolation,
            };
        }
        "mix-blend-mode" => {
            if let Some(mode) = parse_mix_blend_mode(value) {
                style.mix_blend_mode = mode;
            }
        }
        "filter" => {
            if let Some(filter) = parse_filter(value) {
                style.filter = filter;
            }
        }
        "clip" => {
            if let Some(clip) = parse_legacy_clip(value, style.font_size) {
                style.legacy_clip = clip;
            }
        }
        "clip-path" => {
            if let Some(clip_path) = parse_clip_path(value, style.font_size) {
                style.clip_path = clip_path;
            }
        }
        "mask" => {
            let value = trim_css_value(value);
            style.mask = if value.eq_ignore_ascii_case("none") {
                MaskValue::None
            } else {
                MaskValue::Image(value.to_string())
            };
            // `mask` resets the complete mask-border shorthand, including
            // its reset-only `mask-border-source` subproperty.
            // <https://drafts.csswg.org/css-masking/#propdef-mask>
            style.mask_border_source = ComputedImage::None;
        }
        "mask-image" => {
            let value = trim_css_value(value);
            style.mask = if value.eq_ignore_ascii_case("none") {
                MaskValue::None
            } else {
                MaskValue::Image(value.to_string())
            };
        }
        "mask-border-source" => {
            if let Some(source) = parse_border_image_source(value) {
                style.mask_border_source = source;
            }
        }
        "mask-border" => {
            if let Some(source) = parse_mask_border_source(value, style.font_size) {
                style.mask_border_source = source;
            }
        }
        "contain" => {
            if let Some(contain) = parse_contain(value) {
                style.contain = contain;
            }
        }
        "container-type" => {
            if let Some(container_type) = parse_container_type(value) {
                style.container_type = container_type;
            }
        }
        "container-name" => {
            if let Some(names) = parse_container_names(value) {
                style.container_names = names;
            }
        }
        "container" => {
            if let Some((names, container_type)) = parse_container_shorthand(value) {
                style.container_names = names;
                style.container_type = container_type;
            }
        }
        "content-visibility" => {
            style.content_visibility = match value.to_ascii_lowercase().as_str() {
                "visible" => ContentVisibility::Visible,
                "auto" => ContentVisibility::Auto,
                "hidden" => ContentVisibility::Hidden,
                _ => style.content_visibility,
            };
        }
        "will-change" => {
            if let Some(will_change) = parse_will_change(value) {
                style.will_change = will_change;
            }
        }
        "text-align" => {
            if value.eq_ignore_ascii_case("justify-all") {
                style.text_align = TextAlign::JustifyAll;
                style.text_align_last = TextAlignLast::Auto;
            } else if let Some(align) = parse_text_align_all(value, inheritance_source, true) {
                style.text_align = align;
                style.text_align_last = TextAlignLast::Auto;
            }
        }
        "text-align-all" => {
            if let Some(align) = parse_text_align_all(value, inheritance_source, false) {
                style.text_align = align;
            }
        }
        "text-align-last" => {
            if let Some(align) = parse_text_align_last(value, inheritance_source) {
                style.text_align_last = align;
            }
        }
        "text-justify" => {
            style.text_justify = match value.trim().to_ascii_lowercase().as_str() {
                "auto" => TextJustify::Auto,
                "inter-word" => TextJustify::InterWord,
                "inter-character" | "distribute" => TextJustify::InterCharacter,
                "none" => TextJustify::None,
                _ => style.text_justify,
            };
        }
        "text-autospace" => {
            if let Some(text_autospace) = parse_text_autospace(value) {
                style.text_autospace = text_autospace;
            }
        }
        "text-spacing-trim" => {
            if let Some(text_spacing_trim) = parse_text_spacing_trim(value) {
                style.text_spacing_trim = text_spacing_trim;
            }
        }
        "text-spacing" => {
            if let Some((text_spacing_trim, text_autospace)) = parse_text_spacing(value) {
                style.text_spacing_trim = text_spacing_trim;
                style.text_autospace = text_autospace;
            }
        }
        "word-space-transform" => {
            if let Some(word_space_transform) = parse_word_space_transform(value) {
                style.word_space_transform = word_space_transform;
            }
        }
        "text-indent" => {
            if let Some(text_indent) = parse_text_indent(value, style.font_size) {
                style.text_indent = text_indent;
            }
        }
        "hanging-punctuation" => {
            if let Some(hanging_punctuation) = parse_hanging_punctuation(value) {
                style.hanging_punctuation = hanging_punctuation;
            }
        }
        "vertical-align" => {
            if let Some(vertical_align) = parse_vertical_align(value, style.font_size) {
                style.vertical_align = vertical_align;
            }
        }
        "dominant-baseline" => {
            if let Some(dominant_baseline) = parse_dominant_baseline(value) {
                style.vertical_align.dominant_baseline = dominant_baseline;
            }
        }
        "alignment-baseline" => {
            if let Some(alignment_baseline) = parse_alignment_baseline(value) {
                style.vertical_align.alignment_baseline = alignment_baseline;
            }
        }
        "baseline-source" => {
            if let Some(baseline_source) = parse_baseline_source(value) {
                style.vertical_align.baseline_source = baseline_source;
            }
        }
        "baseline-shift" => {
            if let Some(baseline_shift) = parse_baseline_shift(value, style.font_size) {
                style.vertical_align.baseline_shift = baseline_shift;
            }
        }
        "font-weight" => {
            if let Some(weight) = parse_font_weight(value, style.font_weight) {
                style.font_weight = weight;
            }
        }
        "font-style" => {
            if let Some(font_style) = parse_font_style(value) {
                style.font_style = font_style;
            }
        }
        "font-width" | "font-stretch" => {
            if let Some(width) = parse_font_width(value) {
                style.font_width = width;
            }
        }
        _ => return false,
    }
    true
}

/// Applies one modeled longhand emitted by the `font` shorthand.
///
/// `font` is expanded for cascade purposes before inherited font metrics are
/// available. The component therefore retains the shorthand token stream and
/// resolves it here, where its parent-relative size and final line-height
/// basis are known. CSS Fonts requires omitted reset-only subproperties to
/// receive their initial values.
/// <https://www.w3.org/TR/css-fonts-4/#font-prop>
pub(in crate::css) fn apply_font_shorthand_component(
    style: &mut ComputedStyle,
    component: ModeledLonghand,
    font: &ParsedFontShorthand,
) {
    match component {
        ModeledLonghand::FontStyle => style.font_style = font.style,
        ModeledLonghand::FontWeight => style.font_weight = font.weight,
        ModeledLonghand::FontWidth => style.font_width = font.width,
        ModeledLonghand::FontFamily => style.font_family = font.family.clone(),
        // Font size is applied by the dependency prepass so all dependent
        // longhands see its winning computed value.
        ModeledLonghand::FontSize => {}
        ModeledLonghand::LineHeight => {
            style.line_height_value = font
                .line_height
                .clone()
                .unwrap_or(ComputedLineHeight::Normal);
            project_line_height(style);
        }
        ModeledLonghand::FontFeatureSettings => {
            style.font_feature_settings = FontFeatureSettings::NORMAL;
        }
        ModeledLonghand::FontKerning => style.font_kerning = FontKerning::Auto,
        ModeledLonghand::FontLanguageOverride => {
            style.font_language_override = FontLanguageOverride::Normal;
        }
        ModeledLonghand::FontSizeAdjust => style.font_size_adjust = FontSizeAdjust::None,
        ModeledLonghand::FontVariationSettings => {
            style.font_variation_settings = FontVariationSettings::NORMAL;
        }
        ModeledLonghand::FontVariantLigatures => {
            style.font_variant_ligatures = FontVariantLigatures::Normal;
        }
        ModeledLonghand::FontVariantPosition => {
            style.font_variant_position = FontVariantPosition::Normal;
        }
        ModeledLonghand::FontVariantCaps => style.font_variant_caps = font.variant_caps,
        ModeledLonghand::FontVariantNumeric => {
            style.font_variant_numeric = FontVariantNumeric::Normal;
        }
        ModeledLonghand::FontVariantAlternates => {
            style.font_variant_alternates = FontVariantAlternates::Normal;
        }
        ModeledLonghand::FontVariantEastAsian => {
            style.font_variant_east_asian = FontVariantEastAsian::Normal;
        }
        ModeledLonghand::FontVariantEmoji => {
            style.font_variant_emoji = FontVariantEmoji::Normal;
        }
        _ => unreachable!("font shorthand component must be a font longhand"),
    }
}

/// Parse CSS Ruby's `ruby-position` keywords. The distinct inter-character
/// layout is retained in the computed value so it cannot silently select a
/// supported interlinear side.
/// <https://drafts.csswg.org/css-ruby-1/#ruby-position>
fn parse_ruby_position(value: &str) -> Option<RubyPosition> {
    match value.trim().to_ascii_lowercase().as_str() {
        "alternate" => Some(RubyPosition::Alternate),
        "over" => Some(RubyPosition::Over),
        "under" => Some(RubyPosition::Under),
        "inter-character" => Some(RubyPosition::InterCharacter),
        _ => None,
    }
}

/// Parse CSS Ruby's inline-axis content-distribution keywords.
/// <https://drafts.csswg.org/css-ruby-1/#ruby-align-property>
fn parse_ruby_align(value: &str) -> Option<RubyAlign> {
    match value.trim().to_ascii_lowercase().as_str() {
        "start" => Some(RubyAlign::Start),
        "center" => Some(RubyAlign::Center),
        "space-between" => Some(RubyAlign::SpaceBetween),
        "space-around" => Some(RubyAlign::SpaceAround),
        _ => None,
    }
}

/// Parse CSS Ruby's annotation-overhang policy.
/// <https://drafts.csswg.org/css-ruby-1/#ruby-overhang>
fn parse_ruby_overhang(value: &str) -> Option<RubyOverhang> {
    match value.trim().to_ascii_lowercase().as_str() {
        "auto" => Some(RubyOverhang::Auto),
        "spaces" | "none" => Some(RubyOverhang::Spaces),
        _ => None,
    }
}

/// Parses a `z-index` integer, including CSS math expressions.
///
/// A literal `z-index` value must be an `<integer>`. CSS math functions may
/// instead compute a `<number>`; CSS Values then rounds it to the nearest
/// integer, with ties toward positive infinity:
/// <https://drafts.csswg.org/css-position-3/#propdef-z-index> and
/// <https://drafts.csswg.org/css-values-4/#combine-integers>.
pub(super) fn parse_z_index(value: &str) -> Option<i32> {
    if let Ok(value) = value.parse::<i32>() {
        return Some(value);
    }
    let lower = value.to_ascii_lowercase();
    if !["calc(", "min(", "max(", "clamp("]
        .iter()
        .any(|function| lower.starts_with(function))
    {
        return None;
    }
    let MathValue::Number(value) = parse_math_value(value, ROOT_FONT_SIZE_PT)? else {
        return None;
    };
    if !value.is_finite() || value < i32::MIN as f32 || value > i32::MAX as f32 {
        return None;
    }
    let lower_integer = value.floor();
    let rounded = if value - lower_integer < 0.5 {
        lower_integer
    } else {
        lower_integer + 1.0
    };
    Some(rounded as i32)
}

/// Parses CSS Sizing `aspect-ratio`.
///
/// The computed value preserves whether `auto` was supplied so replaced
/// elements can continue to use their natural ratio, while non-replaced boxes
/// can expose the authored preferred ratio:
/// <https://www.w3.org/TR/css-sizing-4/#aspect-ratio>.
pub(super) fn parse_contain_intrinsic_size(
    value: &str,
    font_size: f32,
) -> Option<ContainIntrinsicSize> {
    let value = trim_css_value(value);
    if value.eq_ignore_ascii_case("none") {
        return Some(ContainIntrinsicSize::NONE);
    }
    let values = value.split_ascii_whitespace().collect::<Vec<_>>();
    let (width, height) = match values.as_slice() {
        [size] => (*size, *size),
        [width, height] => (*width, *height),
        _ => return None,
    };
    Some(ContainIntrinsicSize {
        width: Some(parse_computed_length_percentage(width, font_size)?),
        height: Some(parse_computed_length_percentage(height, font_size)?),
    })
}

/// Parse one physical component of `contain-intrinsic-size`.
///
/// `none` removes the fallback on that axis; a length-percentage provides the
/// substituted intrinsic size.
/// <https://drafts.csswg.org/css-sizing-4/#intrinsic-size-override>.
pub(super) fn parse_contain_intrinsic_size_component(
    value: &str,
    font_size: f32,
) -> Option<Option<ComputedLengthPercentage>> {
    let value = trim_css_value(value);
    if value.eq_ignore_ascii_case("none") {
        return Some(None);
    }
    parse_computed_length_percentage(value, font_size).map(Some)
}

/// Parses the size-query subset of CSS `container-type`.
///
/// Style and scroll-state containers are intentionally not represented here:
/// they select a different condition grammar and are outside Spindrift's current
/// containment-query surface.
/// <https://www.w3.org/TR/css-contain-3/#container-type>
fn parse_container_type(value: &str) -> Option<ContainerType> {
    match trim_css_value(value).to_ascii_lowercase().as_str() {
        "normal" => Some(ContainerType::Normal),
        "inline-size" => Some(ContainerType::InlineSize),
        "size" => Some(ContainerType::Size),
        _ => None,
    }
}

/// Parses a container-name list, excluding CSS-wide and reserved keywords.
/// <https://www.w3.org/TR/css-contain-3/#container-name>
fn parse_container_names(value: &str) -> Option<ContainerNames> {
    let value = trim_css_value(value);
    if value.eq_ignore_ascii_case("none") {
        return Some(ContainerNames::default());
    }
    let mut input = cssparser::ParserInput::new(value);
    let mut parser = cssparser::Parser::new(&mut input);
    let mut names = Vec::new();
    while !parser.is_exhausted() {
        names.push(parser.expect_ident_cloned().ok()?.to_string());
    }
    (!names.is_empty()
        && names.iter().all(|name| {
            !matches!(
                name.to_ascii_lowercase().as_str(),
                "none" | "default" | "initial" | "inherit" | "unset" | "revert" | "revert-layer"
            )
        }))
    .then_some(ContainerNames(names))
}

/// Parses `container: <name>? [ / <type> ]?` without accepting an invalid
/// partial shorthand.
/// <https://www.w3.org/TR/css-contain-3/#container-shorthand>
fn parse_container_shorthand(value: &str) -> Option<(ContainerNames, ContainerType)> {
    let value = trim_css_value(value);
    let pieces = try_split_css_top_level_delimiter(value, '/')?;
    let ([names] | [names, _]) = pieces.as_slice() else {
        return None;
    };
    let names = names.trim();
    let type_part = pieces.get(1).copied().map(str::trim);
    match type_part {
        Some(type_part) if !type_part.is_empty() => Some((
            parse_container_names(names)?,
            parse_container_type(type_part)?,
        )),
        Some(_) => None,
        None => parse_container_type(names)
            .map(|container_type| (ContainerNames::default(), container_type))
            .or_else(|| parse_container_names(names).map(|names| (names, ContainerType::Normal))),
    }
}

pub(super) fn parse_aspect_ratio(value: &str) -> Option<AspectRatio> {
    let value = trim_css_value(value);
    if value.eq_ignore_ascii_case("auto") {
        return Some(AspectRatio::AUTO);
    }

    let slash_parts = try_split_css_top_level_delimiter(value, '/')?;
    if slash_parts.len() > 2 {
        return None;
    }
    let mut tokens = Vec::new();
    for (index, part) in slash_parts.iter().enumerate() {
        tokens.extend(try_split_css_component_values(part)?);
        if index + 1 < slash_parts.len() {
            tokens.push("/");
        }
    }
    let auto = if tokens
        .first()
        .is_some_and(|token| token.eq_ignore_ascii_case("auto"))
    {
        tokens.remove(0);
        true
    } else if tokens
        .last()
        .is_some_and(|token| token.eq_ignore_ascii_case("auto"))
    {
        tokens.pop();
        true
    } else {
        false
    };

    if tokens.is_empty()
        || tokens
            .iter()
            .any(|token| token.eq_ignore_ascii_case("auto"))
    {
        return None;
    }

    let ratio = match tokens.as_slice() {
        [width] => parse_positive_css_number(width)?,
        [width, "/", height] => {
            parse_positive_css_number(width)? / parse_positive_css_number(height)?
        }
        _ => return None,
    };

    if auto {
        AspectRatio::auto_with_ratio(ratio)
    } else {
        AspectRatio::from_ratio(ratio)
    }
}

fn parse_positive_css_number(value: &str) -> Option<f32> {
    let number = value.parse::<f32>().ok()?;
    (number.is_finite() && number > 0.0).then_some(number)
}

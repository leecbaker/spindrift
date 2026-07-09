use super::*;
use crate::units::LayoutSize;

/// The marker inserted at a CSS line-clamp point.
///
/// CSS Overflow Level 4 models this independently from the line limit: the
/// marker can be automatic, suppressed, or an authored string.  Keeping that
/// distinction in the computed value prevents layout from treating every
/// clamp as a hard-coded U+2026 insertion.
/// <https://drafts.csswg.org/css-overflow-4/#block-ellipsis>
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum BlockEllipsis {
    Auto,
    None,
    String(String),
}

/// Used non-auto line-clamp state.
///
/// `legacy_webkit` records the prefixed syntax without forcing legacy flex
/// layout; both spellings feed the same line-selection and fragmentation
/// implementation.
/// <https://drafts.csswg.org/css-overflow-4/#line-clamp>
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LineClamp {
    pub(crate) max_lines: usize,
    pub(crate) ellipsis: BlockEllipsis,
    pub(crate) legacy_webkit: bool,
}

/// Reference box and non-negative expansion for an `overflow:clip` edge.
///
/// The Level 3 shorthand is deliberately a single value. Future Level 4
/// physical and logical longhands can expand this into per-side values without
/// changing the layout-facing clip-edge abstraction.
/// <https://www.w3.org/TR/css-overflow-3/#overflow-clip-margin>
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct OverflowClipMargin {
    pub(crate) reference_box: OverflowClipMarginBox,
    pub(crate) length: f32,
}

impl OverflowClipMargin {
    pub(crate) const ZERO: Self = Self {
        reference_box: OverflowClipMarginBox::Padding,
        length: 0.0,
    };
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum OverflowClipMarginBox {
    Border,
    Padding,
    Content,
}

impl LineClamp {
    pub(crate) const fn new(max_lines: usize, legacy_webkit: bool) -> Self {
        Self {
            max_lines,
            ellipsis: BlockEllipsis::Auto,
            legacy_webkit,
        }
    }
}

/// Computed [`object-fit`](https://www.w3.org/TR/css-images-3/#the-object-fit)
/// value for replaced elements.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum ObjectFit {
    #[default]
    Fill,
    Contain,
    Cover,
    None,
    ScaleDown,
}

pub(crate) fn parse_object_fit(value: &str) -> Option<ObjectFit> {
    match value.trim().to_ascii_lowercase().as_str() {
        "fill" => Some(ObjectFit::Fill),
        "contain" => Some(ObjectFit::Contain),
        "cover" => Some(ObjectFit::Cover),
        "none" => Some(ObjectFit::None),
        "scale-down" => Some(ObjectFit::ScaleDown),
        _ => None,
    }
}

/// Computed CSS Images metadata-orientation policy for raster image sources.
/// <https://drafts.csswg.org/css-images-3/#propdef-image-orientation>
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum ImageOrientation {
    #[default]
    FromImage,
    None,
}

pub(crate) fn parse_image_orientation(value: &str) -> Option<ImageOrientation> {
    match value.trim().to_ascii_lowercase().as_str() {
        "from-image" => Some(ImageOrientation::FromImage),
        "none" => Some(ImageOrientation::None),
        _ => None,
    }
}

/// Sampling policy for raster CSS images.
/// <https://drafts.csswg.org/css-images-4/#propdef-image-rendering>
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum ImageRendering {
    #[default]
    Auto,
    Pixelated,
}

pub(crate) fn parse_image_rendering(value: &str) -> Option<ImageRendering> {
    match value.trim().to_ascii_lowercase().as_str() {
        "auto" | "smooth" | "high-quality" => Some(ImageRendering::Auto),
        "pixelated" | "crisp-edges" => Some(ImageRendering::Pixelated),
        _ => None,
    }
}

/// The computed value of CSS `zoom` on one element.
///
/// CSS Viewport defines `zoom` as a non-inherited number or percentage.  The
/// `0` compatibility value is retained at computed-value time and normalized
/// only when deriving the used scale:
/// <https://drafts.csswg.org/css-viewport/#zoom-property>.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct CssZoom(f32);

impl CssZoom {
    pub(crate) const NORMAL: Self = Self(1.0);

    pub(crate) fn parse(value: &str) -> Option<Self> {
        let value = value.trim();
        let factor = if let Some(percentage) = value.strip_suffix('%') {
            percentage.trim().parse::<f32>().ok()? / 100.0
        } else {
            value.parse::<f32>().ok()?
        };
        if !factor.is_finite() || factor < 0.0 {
            return None;
        }
        Some(Self(factor))
    }

    /// Exposes the unnormalized computed value to cascade tests. Layout uses
    /// [`Self::used_factor`] instead.
    #[cfg(test)]
    pub(crate) const fn factor(self) -> f32 {
        self.0
    }

    pub(crate) const fn used_factor(self) -> f32 {
        if self.0 == 0.0 { 1.0 } else { self.0 }
    }
}

/// The product of an element's `zoom` and all flat-tree ancestor zooms.
///
/// This is deliberately distinct from [`CssZoom`]: the former is a computed
/// property value, while this quantity is only valid at used-value boundaries.
/// <https://drafts.csswg.org/css-viewport/#zoom-property>.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct EffectiveZoom(f32);

impl EffectiveZoom {
    pub(crate) const NORMAL: Self = Self(1.0);

    pub(crate) const fn from_parent_and_local(parent: Self, local: CssZoom) -> Self {
        Self(parent.0 * local.used_factor())
    }

    pub(crate) const fn factor(self) -> f32 {
        self.0
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ComputedStyle {
    pub custom_properties: HashMap<String, String>,
    /// The element's own non-inherited CSS `zoom` computed value.
    pub zoom: CssZoom,
    /// Layout-only product of this element's and all ancestor zoom values.
    pub effective_zoom: EffectiveZoom,
    /// Whether this temporary layout style has passed through the used zoom
    /// boundary. Computed styles remain unscaled until layout resolves their
    /// viewport and font-relative terms.
    pub(crate) zoom_applied: bool,
    pub display: Display,
    /// Legacy `display: -webkit-box` uses the old single-line box model.
    ///
    /// It establishes a flex formatting context for compatibility, but does
    /// not opt into modern `flex-wrap: balance` behavior.
    pub legacy_webkit_box: bool,
    pub flex_direction: FlexDirection,
    pub justify_content: JustifyContent,
    pub justify_items: JustifyItems,
    pub justify_self: JustifySelf,
    pub align_content: AlignContent,
    pub align_items: AlignItems,
    pub align_self: AlignSelf,
    pub flex_grow: f32,
    pub flex_shrink: f32,
    pub flex_basis: ComputedFlexBasis,
    pub flex_wrap: FlexWrap,
    /// Author-requested minimum number of balanced flex lines.
    ///
    /// CSS Flexbox Level 2 only applies this property to balanced wrapping:
    /// <https://drafts.csswg.org/css-flexbox-2/#flex-line-count-property>.
    pub flex_line_count: Option<usize>,
    pub order: i32,
    pub row_gap: ComputedGap,
    pub row_rule: GapRuleAxis,
    pub grid_template_rows: GridTrackList,
    pub grid_template_columns: GridTrackList,
    pub grid_template_areas: GridTemplateAreas,
    pub grid_auto_rows: GridAutoTrackList,
    pub grid_auto_columns: GridAutoTrackList,
    pub grid_auto_flow: GridAutoFlow,
    pub grid_lanes_direction: GridLanesDirection,
    pub grid_lanes_flow_tolerance: GridLanesFlowTolerance,
    pub grid_row_start: GridPlacement,
    pub grid_row_end: GridPlacement,
    pub grid_column_start: GridPlacement,
    pub grid_column_end: GridPlacement,
    pub column_count: Option<usize>,
    pub column_width: ComputedColumnWidth,
    pub column_height: ComputedColumnHeight,
    pub column_wrap: ColumnWrap,
    pub column_fill: ColumnFill,
    pub column_span: ColumnSpan,
    pub column_gap: ComputedGap,
    pub column_rule: GapRuleAxis,
    pub rule_overlap: GapRuleOverlap,
    pub box_values: ComputedBoxValues,
    pub aspect_ratio: AspectRatio,
    pub contain_intrinsic_size: ContainIntrinsicSize,
    pub margin_trim: MarginTrim,
    pub margin: Edges,
    pub ua_margin_em: OptionalEdges<f32>,
    pub padding: Edges,
    pub border_width: f32,
    pub border_widths: Edges,
    pub border_width_values: CssEdges<ComputedLengthPercentage>,
    pub border_color: Color,
    pub border_colors: BorderColors,
    pub border_styles: BorderStyles,
    pub border_radius: BorderRadius,
    pub corner_shapes: CornerShapes,
    pub border_image: BorderImage,
    pub outline_width: f32,
    pub outline_width_value: ComputedLengthPercentage,
    pub outline_color: Color,
    pub outline_style: BorderStyle,
    pub outline_offset: ComputedLengthPercentage,
    pub border_collapse: BorderCollapse,
    pub caption_side: CaptionSide,
    pub table_layout: TableLayout,
    pub empty_cells: EmptyCells,
    pub border_spacing: BorderSpacing,
    pub border_spacing_explicit: bool,
    pub background_color: Option<Color>,
    /// Whether `background-color` is specified as `currentcolor` and must be
    /// resolved against this element's own computed `color` after inheritance.
    /// CSS Color 4 resolves `currentcolor` at used-value time:
    /// <https://www.w3.org/TR/css-color-4/#resolving-other-colors>.
    pub background_color_is_current_color: bool,
    /// The uncomputed relative-color expression when its origin is
    /// `currentcolor`, retained so inheritance resolves it on each element.
    pub background_color_current_color_expression: Option<String>,
    pub background_image: Option<BackgroundImage>,
    pub background_size: BackgroundSize,
    pub background_position: BackgroundPosition,
    pub background_repeat: BackgroundRepeat,
    pub background_attachment: BackgroundAttachment,
    pub background_origin: BackgroundBox,
    pub background_clip: BackgroundBox,
    pub background_layers: Vec<BackgroundLayer>,
    /// Number of background layers established by the cascaded
    /// `background-image` value. Other background longhand lists repeat or
    /// truncate to this count at used-value time.
    /// <https://www.w3.org/TR/css-backgrounds-3/#layering>
    pub background_image_layer_count: usize,
    pub object_fit: ObjectFit,
    pub object_view_box: ObjectViewBox,
    pub image_orientation: ImageOrientation,
    pub image_rendering: ImageRendering,
    /// CSS Images positions the concrete object inside a replaced element's
    /// content box.  The grammar and percentage basis are the same as one
    /// background-position layer, but its initial value is centered.
    /// <https://www.w3.org/TR/css-images-3/#the-object-position>
    pub object_position: BackgroundPosition,
    pub box_decoration_break: BoxDecorationBreak,
    pub box_shadow: Vec<BoxShadow>,
    pub color: Color,
    /// WebKit-compatible glyph fill color; `None` represents `currentColor`.
    pub text_fill_color: Option<Color>,
    pub font_size: f32,
    /// Used font size of the document root, the computed-value basis for `rem`.
    /// <https://www.w3.org/TR/css-values-4/#rem>
    pub root_font_size: f32,
    /// The pre-used-value form of `font-size`.
    ///
    /// Mutable formatting boxes retain this until the pre-freeze font
    /// resolution pass has selected the parent metric font.
    pub deferred_font_size: DeferredFontSize,
    pub font_size_adjust: FontSizeAdjust,
    pub line_height_value: ComputedLineHeight,
    pub line_height: f32,
    pub line_height_multiplier: Option<f32>,
    pub line_height_is_normal: bool,
    pub letter_spacing: ComputedLengthPercentage,
    pub word_spacing: ComputedLengthPercentage,
    pub box_sizing: BoxSizing,
    pub direction: Direction,
    pub unicode_bidi: UnicodeBidi,
    pub writing_mode: WritingMode,
    pub text_orientation: TextOrientation,
    pub text_combine_upright: TextCombineUpright,
    pub text_align: TextAlign,
    pub text_align_last: TextAlignLast,
    pub text_justify: TextJustify,
    pub text_autospace: TextAutospace,
    pub word_space_transform: WordSpaceTransform,
    pub line_fit_edge: LineFitEdge,
    pub text_box_trim: TextBoxTrim,
    pub text_box_edge: TextBoxEdge,
    pub initial_letter: InitialLetter,
    pub initial_letter_align: InitialLetterAlign,
    pub initial_letter_wrap: InitialLetterWrap,
    pub text_indent: ComputedTextIndent,
    pub hanging_punctuation: HangingPunctuation,
    pub vertical_align: VerticalAlign,
    pub font_weight: FontWeight,
    pub font_style: FontStyle,
    pub font_width: FontWidth,
    pub font_family: FontFamily,
    pub font_synthesis: FontSynthesis,
    pub font_feature_settings: FontFeatureSettings,
    pub font_kerning: FontKerning,
    pub font_variant_ligatures: FontVariantLigatures,
    pub font_variant_position: FontVariantPosition,
    pub font_variant_caps: FontVariantCaps,
    pub font_variant_numeric: FontVariantNumeric,
    pub font_variant_alternates: FontVariantAlternates,
    pub font_variant_east_asian: FontVariantEastAsian,
    pub font_variant_emoji: FontVariantEmoji,
    pub font_palette: FontPalette,
    pub language: Option<String>,
    pub text_transform: TextTransform,
    pub white_space: WhiteSpace,
    pub text_wrap_mode: TextWrapMode,
    pub text_wrap_style: TextWrapStyle,
    /// Maximum number of visible inline line boxes before truncation.
    ///
    /// CSS Overflow defines line clamping; CSS Text 4 requires clamp selection
    /// to precede `text-wrap: balance` selection.
    pub line_clamp: Option<LineClamp>,
    pub tab_size: TabSize,
    pub word_break: WordBreak,
    pub overflow: Overflow,
    pub overflow_x: Overflow,
    pub overflow_y: Overflow,
    pub scroll_snap_type: ScrollSnapType,
    pub scroll_snap_align: ScrollSnapAlign,
    pub scroll_snap_stop: ScrollSnapStop,
    pub scroll_padding: CssEdges<ScrollPadding>,
    pub scroll_margin: CssEdges<ComputedLengthPercentage>,
    pub overflow_clip_margin: OverflowClipMargin,
    pub overflow_wrap: OverflowWrap,
    pub line_break: LineBreak,
    pub hyphens: Hyphens,
    pub hyphenate_character: HyphenateCharacter,
    pub hyphenate_limit_chars: HyphenateLimitChars,
    pub visibility: Visibility,
    pub list_style_type: ListStyleType,
    pub list_style_position: ListStylePosition,
    pub list_style_image: Option<String>,
    pub list_style_image_base_url: Option<url::Url>,
    pub list_style_image_root_url: Option<url::Url>,
    pub marker_side: MarkerSide,
    pub marker_content: MarkerContent,
    pub marker_style: Option<Box<ComputedStyle>>,
    pub content: Content,
    pub before_style: Option<Box<ComputedStyle>>,
    pub after_style: Option<Box<ComputedStyle>>,
    pub first_line_style: Option<Box<ComputedStyle>>,
    pub first_letter_style: Option<Box<ComputedStyle>>,
    pub quotes: Quotes,
    pub counter_resets: Vec<CounterReset>,
    pub counter_increments: Vec<CounterChange>,
    pub counter_sets: Vec<CounterChange>,
    pub string_sets: Vec<NamedStringSet>,
    /// Computed CSS `page` value, where `None` represents `auto`.
    ///
    /// CSS Paged Media uses the computed `page` property to create named page
    /// groups, and an explicit `page: auto` can end an ancestor's named page
    /// group:
    /// <https://www.w3.org/TR/css-page-3/#using-named-pages>.
    pub page_name: Option<String>,
    /// Whether the CSS `page` property was explicitly specified on this box.
    ///
    /// This distinguishes omitted `page` declarations from explicit
    /// `page: auto`, which have the same computed value but different
    /// named-page propagation behavior:
    /// <https://www.w3.org/TR/css-page-3/#using-named-pages>.
    pub page_name_specified: bool,
    pub break_before: PageBreak,
    pub break_after: PageBreak,
    pub break_inside_avoid: bool,
    pub break_inside_avoid_column: bool,
    pub orphans: usize,
    pub widows: usize,
    pub text_decoration_layers: Vec<TextDecoration>,
    pub text_decoration: TextDecoration,
    pub text_shadow: Vec<TextShadow>,
    pub text_emphasis_style: TextEmphasisStyle,
    pub text_emphasis_color: Option<Color>,
    pub text_emphasis_position: TextEmphasisPosition,
    pub text_emphasis_skip: TextEmphasisSkip,
    pub position: Position,
    pub float: Float,
    pub clear: Clear,
    pub running_element_name: Option<String>,
    pub abspos_static_source_was_inline_level: bool,
    pub abspos_static_source_was_atomic_inline: bool,
    pub z_index: Option<i32>,
    pub opacity: f32,
    pub transform: TransformList,
    pub individual_transforms: IndividualTransforms,
    pub transform_origin: TransformOrigin,
    pub transform_box: TransformBox,
    pub backface_visibility: BackfaceVisibility,
    pub isolation: Isolation,
    pub mix_blend_mode: MixBlendMode,
    pub filter: FilterValue,
    pub clip_path: ClipPath,
    pub mask: MaskValue,
    pub contain: Contain,
    /// `container-type` / `container` query-container axis capability.
    /// <https://www.w3.org/TR/css-contain-3/#container-type>
    pub container_type: ContainerType,
    /// Names selected by named `@container` rules.
    /// <https://www.w3.org/TR/css-contain-3/#container-name>
    pub container_names: ContainerNames,
    pub content_visibility: ContentVisibility,
    pub will_change: WillChange,
    pub bookmark_level: Option<u32>,
    pub bookmark_label: BookmarkLabel,
    pub bookmark_state: CssBookmarkState,
}

impl ComputedStyle {
    /// Return the used `direction` after writing-mode text-orientation rules.
    ///
    /// In vertical typographic modes, `text-orientation: upright` makes the
    /// used value of `direction` be `ltr`.  The computed value remains useful
    /// for cascade and serialization, so consumers that resolve logical axes
    /// must take this used-value boundary explicitly instead of rewriting the
    /// stored property.
    /// <https://drafts.csswg.org/css-writing-modes-4/#text-orientation>
    pub(crate) const fn used_direction(&self) -> Direction {
        match self.text_layout_policy() {
            TextLayoutPolicy::Vertical(TextOrientation::Upright) => Direction::Ltr,
            TextLayoutPolicy::Horizontal
            | TextLayoutPolicy::Vertical(TextOrientation::Mixed | TextOrientation::Sideways)
            | TextLayoutPolicy::Sideways(_) => self.direction,
        }
    }

    /// Return the used text-layout policy derived from writing-mode and
    /// text-orientation.
    ///
    /// CSS Writing Modes applies `text-orientation` only to vertical
    /// typographic mode; sideways writing modes instead force horizontal runs
    /// into their specified rotation.
    /// <https://www.w3.org/TR/css-writing-modes-4/#text-orientation>.
    pub(crate) const fn text_layout_policy(&self) -> TextLayoutPolicy {
        self.writing_mode.text_layout_policy(self.text_orientation)
    }

    /// Whether this inline may contribute soft wrap opportunities.
    ///
    /// The legacy `white-space` shorthand determines the initial wrapping
    /// mode, while the CSS Text 4 `text-wrap-mode` longhand can override it.
    /// <https://drafts.csswg.org/css-text-4/#text-wrap-mode-property>
    pub(crate) const fn allows_soft_wrap(&self) -> bool {
        match self.text_wrap_mode {
            TextWrapMode::Legacy => {
                !matches!(self.white_space, WhiteSpace::NoWrap | WhiteSpace::Pre)
            }
            TextWrapMode::Wrap => true,
            TextWrapMode::NoWrap => false,
        }
    }

    /// Returns the clip edge for the color layer beneath every background
    /// image layer.
    ///
    /// CSS Backgrounds and Borders paints `background-color` below the image
    /// layers and clips it using the final (bottom-most) `background-clip`
    /// value after layer-list repetition:
    /// <https://www.w3.org/TR/css-backgrounds-3/#the-background-clip>.
    pub(crate) fn background_color_clip(&self) -> BackgroundBox {
        self.background_layers
            .last()
            .map(|layer| layer.clip)
            .unwrap_or(self.background_clip)
    }

    /// Applies the element's effective CSS zoom at the used-value boundary.
    ///
    /// `zoom` scales fixed used lengths, while percentage and `auto` values
    /// remain relative to the already scaled containing block.  Callers use a
    /// fresh layout-style clone, so this transformation is intentionally
    /// destructive and must be performed exactly once per clone.
    /// <https://drafts.csswg.org/css-viewport/#zoom-property>
    pub(crate) fn apply_effective_zoom(&mut self) {
        if self.zoom_applied {
            return;
        }
        let factor = self.effective_zoom.factor();
        if factor == 1.0 {
            self.zoom_applied = true;
            return;
        }
        self.box_values.scale_fixed_length_components(factor);
        self.flex_basis.scale_fixed_length_components(factor);
        self.row_gap.scale_fixed_length_components(factor);
        self.column_gap.scale_fixed_length_components(factor);
        self.column_width.scale_fixed_length_components(factor);
        self.column_height.scale_fixed_length_components(factor);
        self.column_rule.scale_fixed_length_components(factor);
        self.row_rule.scale_fixed_length_components(factor);
        self.grid_template_rows
            .scale_fixed_length_components(factor);
        self.grid_template_columns
            .scale_fixed_length_components(factor);
        self.grid_auto_rows.scale_fixed_length_components(factor);
        self.grid_auto_columns.scale_fixed_length_components(factor);
        self.border_spacing.scale_fixed_length_components(factor);
        self.font_size *= factor;
        self.root_font_size *= factor;
        self.line_height_value.scale_fixed_length_components(factor);
        self.line_height *= factor;
        self.margin = scaled_edges(self.margin, factor);
        self.padding = scaled_edges(self.padding, factor);
        self.border_width *= factor;
        self.border_widths = scaled_edges(self.border_widths, factor);
        for value in [
            &mut self.border_width_values.top,
            &mut self.border_width_values.right,
            &mut self.border_width_values.bottom,
            &mut self.border_width_values.left,
        ] {
            value.scale_fixed_length_components(factor);
        }
        self.outline_width *= factor;
        self.outline_width_value
            .scale_fixed_length_components(factor);
        self.letter_spacing.scale_fixed_length_components(factor);
        self.word_spacing.scale_fixed_length_components(factor);
        self.text_indent
            .amount
            .scale_fixed_length_components(factor);
        self.zoom_applied = true;
    }

    pub fn initial() -> Self {
        let font_size = 12.0;
        Self {
            custom_properties: HashMap::new(),
            zoom: CssZoom::NORMAL,
            effective_zoom: EffectiveZoom::NORMAL,
            zoom_applied: false,
            display: Display::INLINE,
            legacy_webkit_box: false,
            flex_direction: FlexDirection::Row,
            justify_content: JustifyContent::NORMAL,
            justify_items: JustifyItems::NORMAL,
            justify_self: JustifySelf::AUTO,
            align_content: AlignContent::NORMAL,
            align_items: AlignItems::NORMAL,
            align_self: AlignSelf::AUTO,
            flex_grow: 0.0,
            flex_shrink: 1.0,
            flex_basis: ComputedFlexBasis::AUTO,
            flex_wrap: FlexWrap::NoWrap,
            flex_line_count: None,
            order: 0,
            row_gap: ComputedGap::NORMAL,
            row_rule: GapRuleAxis::initial(),
            grid_template_rows: GridTrackList::NONE,
            grid_template_columns: GridTrackList::NONE,
            grid_template_areas: GridTemplateAreas::NONE,
            grid_auto_rows: GridAutoTrackList::initial(),
            grid_auto_columns: GridAutoTrackList::initial(),
            grid_auto_flow: GridAutoFlow::ROW,
            grid_lanes_direction: GridLanesDirection::NORMAL,
            grid_lanes_flow_tolerance: GridLanesFlowTolerance::NORMAL,
            grid_row_start: GridPlacement::AUTO,
            grid_row_end: GridPlacement::AUTO,
            grid_column_start: GridPlacement::AUTO,
            grid_column_end: GridPlacement::AUTO,
            column_count: None,
            column_width: ComputedColumnWidth::AUTO,
            column_height: ComputedColumnHeight::AUTO,
            column_wrap: ColumnWrap::Auto,
            column_fill: ColumnFill::Balance,
            column_span: ColumnSpan::None,
            column_gap: ComputedGap::NORMAL,
            column_rule: GapRuleAxis::initial(),
            rule_overlap: GapRuleOverlap::RowOverColumn,
            box_values: ComputedBoxValues::initial(),
            aspect_ratio: AspectRatio::AUTO,
            contain_intrinsic_size: ContainIntrinsicSize::NONE,
            margin_trim: MarginTrim::NONE,
            margin: Edges::ZERO,
            ua_margin_em: OptionalEdges::NONE,
            padding: Edges::ZERO,
            border_width: 0.0,
            border_widths: Edges::ZERO,
            border_width_values: CssEdges::all(ComputedLengthPercentage::ZERO),
            border_color: Color::BLACK,
            border_colors: BorderColors::BLACK,
            border_styles: BorderStyles::NONE,
            border_radius: BorderRadius::ZERO,
            corner_shapes: CornerShapes::ROUND,
            border_image: BorderImage::initial(),
            outline_width: 3.0 * CSS_PX_TO_PT,
            outline_width_value: ComputedLengthPercentage::from_points(3.0 * CSS_PX_TO_PT),
            outline_color: Color::BLACK,
            outline_style: BorderStyle::None,
            outline_offset: ComputedLengthPercentage::ZERO,
            border_collapse: BorderCollapse::Separate,
            caption_side: CaptionSide::Top,
            table_layout: TableLayout::Auto,
            empty_cells: EmptyCells::Show,
            border_spacing: BorderSpacing::ZERO,
            border_spacing_explicit: false,
            background_color: None,
            background_color_is_current_color: false,
            background_color_current_color_expression: None,
            background_image: None,
            background_size: BackgroundSize::AUTO,
            background_position: BackgroundPosition::INITIAL,
            background_repeat: BackgroundRepeat::Repeat,
            background_attachment: BackgroundAttachment::Scroll,
            background_origin: BackgroundBox::Padding,
            background_clip: BackgroundBox::Border,
            background_layers: Vec::new(),
            background_image_layer_count: 1,
            object_fit: ObjectFit::Fill,
            object_view_box: ObjectViewBox::NONE,
            image_orientation: ImageOrientation::FromImage,
            image_rendering: ImageRendering::Auto,
            object_position: BackgroundPosition {
                x: BackgroundPositionAxis {
                    origin: BackgroundPositionOrigin::Center,
                    offset: ComputedLengthPercentage::ZERO,
                },
                y: BackgroundPositionAxis {
                    origin: BackgroundPositionOrigin::Center,
                    offset: ComputedLengthPercentage::ZERO,
                },
            },
            box_decoration_break: BoxDecorationBreak::Slice,
            box_shadow: Vec::new(),
            color: Color::BLACK,
            text_fill_color: None,
            font_size,
            root_font_size: font_size,
            deferred_font_size: DeferredFontSize::INITIAL,
            font_size_adjust: FontSizeAdjust::None,
            line_height_value: ComputedLineHeight::NORMAL,
            line_height: font_size * 1.2,
            line_height_multiplier: Some(1.2),
            line_height_is_normal: true,
            letter_spacing: ComputedLengthPercentage::ZERO,
            word_spacing: ComputedLengthPercentage::ZERO,
            box_sizing: BoxSizing::ContentBox,
            direction: Direction::Ltr,
            unicode_bidi: UnicodeBidi::Normal,
            writing_mode: WritingMode::HorizontalTb,
            text_orientation: TextOrientation::Mixed,
            text_combine_upright: TextCombineUpright::None,
            text_align: TextAlign::Start,
            text_align_last: TextAlignLast::Auto,
            text_justify: TextJustify::Auto,
            text_autospace: TextAutospace::NORMAL,
            word_space_transform: WordSpaceTransform::NONE,
            line_fit_edge: LineFitEdge::Leading,
            text_box_trim: TextBoxTrim::None,
            text_box_edge: TextBoxEdge::Auto,
            initial_letter: InitialLetter::Normal,
            initial_letter_align: InitialLetterAlign::ALPHABETIC,
            initial_letter_wrap: InitialLetterWrap::None,
            text_indent: ComputedTextIndent::ZERO,
            hanging_punctuation: HangingPunctuation::NONE,
            vertical_align: VerticalAlign::BASELINE,
            font_weight: FontWeight::NORMAL,
            font_style: FontStyle::Normal,
            font_width: FontWidth::NORMAL,
            font_family: FontFamily::SansSerif,
            font_synthesis: FontSynthesis::ALL,
            font_feature_settings: FontFeatureSettings::NORMAL,
            font_kerning: FontKerning::Auto,
            font_variant_ligatures: FontVariantLigatures::Normal,
            font_variant_position: FontVariantPosition::Normal,
            font_variant_caps: FontVariantCaps::Normal,
            font_variant_numeric: FontVariantNumeric::Normal,
            font_variant_alternates: FontVariantAlternates::Normal,
            font_variant_east_asian: FontVariantEastAsian::Normal,
            font_variant_emoji: FontVariantEmoji::Normal,
            font_palette: FontPalette::Normal,
            language: None,
            text_transform: TextTransform::NONE,
            white_space: WhiteSpace::Normal,
            text_wrap_mode: TextWrapMode::Legacy,
            text_wrap_style: TextWrapStyle::Auto,
            line_clamp: None,
            tab_size: TabSize::INITIAL,
            word_break: WordBreak::Normal,
            overflow: Overflow::Visible,
            overflow_x: Overflow::Visible,
            overflow_y: Overflow::Visible,
            scroll_snap_type: ScrollSnapType::None,
            scroll_snap_align: ScrollSnapAlign::default(),
            scroll_snap_stop: ScrollSnapStop::Normal,
            scroll_padding: CssEdges::all(ScrollPadding::Auto),
            scroll_margin: CssEdges::all(ComputedLengthPercentage::ZERO),
            overflow_clip_margin: OverflowClipMargin::ZERO,
            overflow_wrap: OverflowWrap::Normal,
            line_break: LineBreak::Auto,
            hyphens: Hyphens::Manual,
            hyphenate_character: HyphenateCharacter::Auto,
            hyphenate_limit_chars: HyphenateLimitChars::AUTO,
            visibility: Visibility::Visible,
            list_style_type: ListStyleType::Disc,
            list_style_position: ListStylePosition::Outside,
            list_style_image: None,
            list_style_image_base_url: None,
            list_style_image_root_url: None,
            marker_side: MarkerSide::MatchSelf,
            marker_content: MarkerContent::Auto,
            marker_style: None,
            content: Content::Normal,
            before_style: None,
            after_style: None,
            first_line_style: None,
            first_letter_style: None,
            quotes: Quotes::auto(),
            counter_resets: Vec::new(),
            counter_increments: Vec::new(),
            counter_sets: Vec::new(),
            string_sets: Vec::new(),
            page_name: None,
            page_name_specified: false,
            break_before: PageBreak::Auto,
            break_after: PageBreak::Auto,
            break_inside_avoid: false,
            break_inside_avoid_column: false,
            orphans: 2,
            widows: 2,
            text_decoration_layers: Vec::new(),
            text_decoration: TextDecoration {
                underline: false,
                overline: false,
                line_through: false,
                blink: false,
                spelling_error: false,
                grammar_error: false,
                style: TextDecorationStyle::Solid,
                thickness: TextDecorationThickness::Auto,
                inset: TextDecorationInset::ZERO,
                skip_ink: TextDecorationSkipInk::Auto,
                skip_self: TextDecorationSkipSelf::Auto,
                skip_box: TextDecorationSkipBox::None,
                skip_spaces: TextDecorationSkipSpaces::START_END,
                underline_offset: TextUnderlineOffset::Auto,
                underline_position: TextUnderlinePosition::AUTO,
                color: None,
            },
            text_shadow: Vec::new(),
            text_emphasis_style: TextEmphasisStyle::None,
            text_emphasis_color: None,
            text_emphasis_position: TextEmphasisPosition::default(),
            text_emphasis_skip: TextEmphasisSkip::default(),
            position: Position::Static,
            float: Float::None,
            clear: Clear::None,
            running_element_name: None,
            abspos_static_source_was_inline_level: false,
            abspos_static_source_was_atomic_inline: false,
            z_index: None,
            opacity: 1.0,
            transform: Vec::new(),
            individual_transforms: IndividualTransforms::NONE,
            transform_origin: TransformOrigin::INITIAL,
            transform_box: TransformBox::INITIAL,
            backface_visibility: BackfaceVisibility::Visible,
            isolation: Isolation::Auto,
            mix_blend_mode: MixBlendMode::Normal,
            filter: FilterValue::None,
            clip_path: ClipPath::None,
            mask: MaskValue::None,
            contain: Contain::NONE,
            container_type: ContainerType::Normal,
            container_names: ContainerNames::default(),
            content_visibility: ContentVisibility::Visible,
            will_change: WillChange::default(),
            bookmark_level: None,
            bookmark_label: BookmarkLabel::content_text(),
            bookmark_state: CssBookmarkState::Open,
        }
    }

    /// Resolves the deferred `font-size` against the already-used parent
    /// font. This is deliberately separate from ordinary font-relative
    /// property resolution: CSS Fonts gives `font-size` parent-relative
    /// metric bases, whereas other `em`/`ch` lengths use this style's font.
    /// <https://www.w3.org/TR/css-fonts-4/#font-size-prop>
    pub(crate) fn resolve_deferred_font_size(&mut self, parent: FontRelativeLengthBasis) {
        self.font_size = clamp_used_layout_length(self.deferred_font_size.resolve(parent)).points();
        let (line_height, multiplier, is_normal) =
            self.line_height_value.clone().projected(self.font_size);
        self.line_height = line_height;
        self.line_height_multiplier = multiplier;
        self.line_height_is_normal = is_normal;
    }

    /// Resolves deferred `font-size` after the page viewport has become known.
    ///
    /// CSS viewport-relative font sizes establish the inherited `em` basis of
    /// descendants, so this must happen before ordinary font-metric lengths
    /// are projected for the formatting tree:
    /// <https://www.w3.org/TR/css-values-4/#viewport-relative-lengths>.
    pub(crate) fn resolve_deferred_font_size_with_viewport(
        &mut self,
        parent: FontRelativeLengthBasis,
        viewport: LayoutSize,
    ) {
        let basis = ViewportLengthBasis::for_writing_mode(viewport, self.writing_mode);
        self.font_size = clamp_used_layout_length(
            self.deferred_font_size
                .resolve_with_viewport(parent, Some(basis)),
        )
        .points();
        let (line_height, multiplier, is_normal) =
            self.line_height_value.clone().projected(self.font_size);
        self.line_height = line_height;
        self.line_height_multiplier = multiplier;
        self.line_height_is_normal = is_normal;
    }

    /// Returns whether this style has an unresolved value that depends on its
    /// own used `ch` advance.
    ///
    /// `ComputedStyle` intentionally keeps `ch` components through the
    /// computed-value phase. This read-only traversal covers every
    /// style-owned metric-bearing property, including deferred CSS math,
    /// without turning an otherwise font-free style into a font lookup:
    /// <https://www.w3.org/TR/css-values-4/#font-relative-lengths>.
    pub(crate) fn requires_ch_advance(&self) -> bool {
        self.line_height_value.requires_ch_advance()
            || self.row_gap.requires_ch_advance()
            || self.column_gap.requires_ch_advance()
            || self.row_rule.requires_ch_advance()
            || self.column_rule.requires_ch_advance()
            || self.grid_template_rows.requires_ch_advance()
            || self.grid_template_columns.requires_ch_advance()
            || self.grid_auto_rows.requires_ch_advance()
            || self.grid_auto_columns.requires_ch_advance()
            || self.column_width.requires_ch_advance()
            || self.column_height.requires_ch_advance()
            || self.letter_spacing.requires_ch_advance()
            || self.word_spacing.requires_ch_advance()
            || self.box_values.requires_ch_advance()
            || self.border_radius.requires_ch_advance()
            || self.border_width_values.top.requires_ch_advance()
            || self.border_width_values.right.requires_ch_advance()
            || self.border_width_values.bottom.requires_ch_advance()
            || self.border_width_values.left.requires_ch_advance()
            || self.outline_width_value.requires_ch_advance()
            || self.outline_offset.requires_ch_advance()
            || self.flex_basis.requires_ch_advance()
            || self.text_indent.amount.requires_ch_advance()
            || self.vertical_align.requires_ch_advance()
            || self.tab_size.requires_ch_advance()
            || self
                .background_image
                .as_ref()
                .is_some_and(BackgroundImage::requires_ch_advance)
            || self.background_size.requires_ch_advance()
            || self.background_position.requires_ch_advance()
            || self.object_position.requires_ch_advance()
            || self.object_view_box.requires_ch_advance()
            || self
                .background_layers
                .iter()
                .any(BackgroundLayer::requires_ch_advance)
            || self
                .transform
                .iter()
                .any(TransformFunction::requires_ch_advance)
            || self.individual_transforms.requires_ch_advance()
            || self.transform_origin.requires_ch_advance()
            || self.border_image.requires_ch_advance()
            || self.text_decoration.requires_ch_advance()
            || self.border_spacing.requires_ch_advance()
            || self.text_shadow.iter().any(TextShadow::requires_ch_advance)
            || self.box_shadow.iter().any(BoxShadow::requires_ch_advance)
    }

    /// Returns whether resolving this style needs metrics from its selected
    /// font, rather than only the font-size fallback values.
    ///
    /// `ch`, `ic`, `ex`, and `cap` all use a selected-font metric. The
    /// explicit `ch` traversal above remains useful to avoid cloning in the
    /// common case; the other metric phases are represented throughout the
    /// computed-value graph and are detected by resolving them on a clone.
    /// This keeps the lookup decision in one place as new metric-bearing
    /// properties are added:
    /// <https://www.w3.org/TR/css-values-4/#font-relative-lengths>.
    pub(crate) fn requires_selected_font_metrics(&self) -> bool {
        if self.requires_ch_advance() {
            return true;
        }
        let mut resolved = self.clone();
        resolved.resolve_ic_relative_lengths(layout_pt(0.0));
        resolved.resolve_ex_relative_lengths(0.0);
        resolved.resolve_cap_relative_lengths(0.0);
        self != &resolved
    }

    /// Returns whether a generated pseudo-style needs this style's `ch`
    /// advance to resolve its deferred `font-size`.
    pub(crate) fn pseudo_styles_require_parent_ch_advance(&self) -> bool {
        [
            self.marker_style.as_deref(),
            self.before_style.as_deref(),
            self.after_style.as_deref(),
            self.first_line_style.as_deref(),
            self.first_letter_style.as_deref(),
        ]
        .into_iter()
        .flatten()
        .any(|pseudo| {
            pseudo
                .deferred_font_size
                .requires_parent_ch_advance(self.font_size)
        })
    }

    /// Finalizes the ordinary font-relative components of box-model computed
    /// values once the cascade has selected this element's font sizes.
    ///
    /// CSS Values computes `em` and `rem` from font sizes, while units based
    /// on selected-font metrics remain unresolved until layout.
    /// <https://www.w3.org/TR/css-values-4/#font-relative-lengths>
    pub(crate) fn finalize_computed_font_relative_lengths(&mut self) {
        self.box_values
            .resolve_em_relative_lengths(layout_pt(self.font_size));
        self.box_values
            .resolve_root_font_relative_lengths(self.root_font_size);
        if let Some(image) = &mut self.background_image {
            image.resolve_em_relative_lengths(layout_pt(self.font_size));
            image.resolve_root_font_relative_lengths(self.root_font_size);
        }
        self.background_size
            .resolve_em_relative_lengths(layout_pt(self.font_size));
        self.background_size
            .resolve_root_font_relative_lengths(self.root_font_size);
        self.background_position
            .resolve_em_relative_lengths(layout_pt(self.font_size));
        self.background_position
            .resolve_root_font_relative_lengths(self.root_font_size);
        for layer in &mut self.background_layers {
            layer.resolve_em_relative_lengths(layout_pt(self.font_size));
            layer.resolve_root_font_relative_lengths(self.root_font_size);
        }
    }

    /// Resolves font-metric-relative computed lengths for this style.
    ///
    /// CSS Values defines `ch` relative to the used advance of the "0" glyph.
    /// The cascade stores that component separately, then layout resolves it
    /// with the selected font face before formatting contexts consume used
    /// lengths:
    /// <https://www.w3.org/TR/css-values-4/#font-relative-lengths> and
    /// <https://www.w3.org/TR/css-cascade-5/#used>.
    pub(crate) fn resolve_font_metric_lengths(&mut self, ch_advance: LayoutLength) {
        self.resolve_font_metric_lengths_with_box_axes(ch_advance, ch_advance, ch_advance);
    }

    /// Resolves selected-font `ch` metrics with distinct physical box axes.
    pub(crate) fn resolve_font_metric_lengths_with_box_axes(
        &mut self,
        ch_advance: LayoutLength,
        horizontal_box_advance: LayoutLength,
        vertical_box_advance: LayoutLength,
    ) {
        self.line_height_value
            .resolve_font_metric_lengths(ch_advance);
        let (line_height, multiplier, is_normal) =
            self.line_height_value.clone().projected(self.font_size);
        self.line_height = line_height;
        self.line_height_multiplier = multiplier;
        self.line_height_is_normal = is_normal;
        self.row_gap.resolve_font_metric_lengths(ch_advance);
        self.column_gap.resolve_font_metric_lengths(ch_advance);
        self.row_rule.resolve_font_metric_lengths(ch_advance);
        self.column_rule.resolve_font_metric_lengths(ch_advance);
        self.grid_template_rows
            .resolve_font_metric_lengths(ch_advance);
        self.grid_template_columns
            .resolve_font_metric_lengths(ch_advance);
        self.grid_auto_rows.resolve_font_metric_lengths(ch_advance);
        self.grid_auto_columns
            .resolve_font_metric_lengths(ch_advance);
        self.column_width.resolve_font_metric_lengths(ch_advance);
        self.column_height.resolve_font_metric_lengths(ch_advance);
        self.letter_spacing.resolve_font_metric_lengths(ch_advance);
        self.word_spacing.resolve_font_metric_lengths(ch_advance);
        self.box_values
            .resolve_font_metric_lengths_by_physical_axis(
                horizontal_box_advance,
                vertical_box_advance,
            );
        self.border_radius.resolve_font_metric_lengths(ch_advance);
        self.object_view_box.resolve_font_metric_lengths(ch_advance);
        self.border_width_values
            .top
            .resolve_font_metric_lengths(ch_advance);
        self.border_width_values
            .right
            .resolve_font_metric_lengths(ch_advance);
        self.border_width_values
            .bottom
            .resolve_font_metric_lengths(ch_advance);
        self.border_width_values
            .left
            .resolve_font_metric_lengths(ch_advance);
        self.border_widths = Edges {
            top: self.border_width_values.top.length_max_zero().points(),
            right: self.border_width_values.right.length_max_zero().points(),
            bottom: self.border_width_values.bottom.length_max_zero().points(),
            left: self.border_width_values.left.length_max_zero().points(),
        };
        self.border_width = self
            .border_widths
            .top
            .max(self.border_widths.right)
            .max(self.border_widths.bottom)
            .max(self.border_widths.left);
        self.outline_width_value
            .resolve_font_metric_lengths(ch_advance);
        self.outline_width = self.outline_width_value.length_max_zero().points();
        self.outline_offset.resolve_font_metric_lengths(ch_advance);
        self.flex_basis.resolve_font_metric_lengths(ch_advance);
        self.text_indent
            .amount
            .resolve_font_metric_lengths(ch_advance);
        self.vertical_align.resolve_font_metric_lengths(ch_advance);
        self.tab_size.resolve_font_metric_lengths(ch_advance);
        if let Some(image) = &mut self.background_image {
            image.resolve_font_metric_lengths(ch_advance);
        }
        self.background_size.resolve_font_metric_lengths(ch_advance);
        self.background_position
            .resolve_font_metric_lengths(ch_advance);
        self.object_position.resolve_font_metric_lengths(ch_advance);
        for layer in &mut self.background_layers {
            layer.resolve_font_metric_lengths(ch_advance);
        }
        for function in &mut self.transform {
            function.resolve_font_metric_lengths(ch_advance);
        }
        self.individual_transforms
            .resolve_font_metric_lengths(ch_advance);
        self.transform_origin
            .resolve_font_metric_lengths(ch_advance);
        self.border_image.resolve_font_metric_lengths(ch_advance);
        self.text_decoration.resolve_font_metric_lengths(ch_advance);
        self.border_spacing.resolve_font_metric_lengths(ch_advance);
        for shadow in &mut self.text_shadow {
            shadow.resolve_font_metric_lengths(ch_advance);
        }
        for shadow in &mut self.box_shadow {
            shadow.resolve_font_metric_lengths(ch_advance);
        }
    }

    /// Resolves `ic` box-model components against the selected font's WATER
    /// glyph advance.
    /// <https://www.w3.org/TR/css-values-4/#ic>
    pub(crate) fn resolve_ic_relative_lengths(&mut self, ic_advance: LayoutLength) {
        self.resolve_ic_relative_lengths_with_box_axes(ic_advance, ic_advance, ic_advance);
    }

    /// Resolves selected-font `ic` metrics with distinct physical box axes.
    pub(crate) fn resolve_ic_relative_lengths_with_box_axes(
        &mut self,
        ic_advance: LayoutLength,
        horizontal_box_advance: LayoutLength,
        vertical_box_advance: LayoutLength,
    ) {
        self.line_height_value
            .resolve_ic_relative_lengths(ic_advance);
        let (line_height, multiplier, is_normal) =
            self.line_height_value.clone().projected(self.font_size);
        self.line_height = line_height;
        self.line_height_multiplier = multiplier;
        self.line_height_is_normal = is_normal;
        self.box_values
            .resolve_ic_relative_lengths_by_physical_axis(
                horizontal_box_advance,
                vertical_box_advance,
            );
    }

    /// Resolves `ex` box-model components against the selected font's x-height.
    /// <https://www.w3.org/TR/css-values-4/#ex>
    pub(crate) fn resolve_ex_relative_lengths(&mut self, x_height: f32) {
        self.line_height_value.resolve_ex_relative_lengths(x_height);
        let (line_height, multiplier, is_normal) =
            self.line_height_value.clone().projected(self.font_size);
        self.line_height = line_height;
        self.line_height_multiplier = multiplier;
        self.line_height_is_normal = is_normal;
        self.box_values.resolve_ex_relative_lengths(x_height);
    }

    /// Resolves `cap` box-model components against the selected font's cap height.
    /// <https://www.w3.org/TR/css-values-4/#cap>
    pub(crate) fn resolve_cap_relative_lengths(&mut self, cap_height: f32) {
        self.line_height_value
            .resolve_cap_relative_lengths(cap_height);
        let (line_height, multiplier, is_normal) =
            self.line_height_value.clone().projected(self.font_size);
        self.line_height = line_height;
        self.line_height_multiplier = multiplier;
        self.line_height_is_normal = is_normal;
        self.box_values.resolve_cap_relative_lengths(cap_height);
    }

    /// Resolves ordinary `lh` components after this style's computed line
    /// height is available. `line-height` itself is intentionally excluded:
    /// CSS Values gives `lh` in that property the inherited line-height basis.
    /// <https://www.w3.org/TR/css-values-4/#lh>
    pub(crate) fn resolve_line_height_relative_lengths(&mut self) {
        let line_height = layout_pt(self.line_height);
        self.box_values
            .resolve_line_height_relative_lengths(line_height);
        self.background_size
            .resolve_line_height_relative_lengths(line_height);
        self.background_position
            .resolve_line_height_relative_lengths(line_height);
        for layer in &mut self.background_layers {
            layer.resolve_line_height_relative_lengths(line_height);
        }
    }

    /// Resolves root-font metric components against the one document-root
    /// metric snapshot shared by every element and pseudo-element.
    /// <https://www.w3.org/TR/css-values-4/#font-relative-lengths>
    pub(crate) fn resolve_root_font_metric_lengths(&mut self, basis: RootFontMetricLengthBasis) {
        self.line_height_value
            .resolve_root_font_metric_lengths(basis);
        let (line_height, multiplier, is_normal) =
            self.line_height_value.clone().projected(self.font_size);
        self.line_height = line_height;
        self.line_height_multiplier = multiplier;
        self.line_height_is_normal = is_normal;
        self.box_values.resolve_root_font_metric_lengths(basis);
    }

    /// Resolves font-metric-relative lengths while preserving physical
    /// block-size constraints for a later formatting-context-specific pass.
    ///
    /// CSS Tables consumes table-cell `height`/`min-height`/`max-height` as
    /// row-axis constraints, even when the cell's own writing mode is
    /// orthogonal. Keeping those values metric-aware lets table layout resolve
    /// their `ch` components against the row-axis writing context instead of
    /// the cell content writing context.
    pub(crate) fn resolve_font_metric_lengths_preserving_box_block_sizes(
        &mut self,
        ch_advance: LayoutLength,
    ) {
        let height = self.box_values.height.clone();
        let min_height = self.box_values.min_height.clone();
        let max_height = self.box_values.max_height.clone();
        self.resolve_font_metric_lengths(ch_advance);
        self.box_values.height = height;
        self.box_values.min_height = min_height;
        self.box_values.max_height = max_height;
    }

    /// Resolves viewport-relative computed lengths for paged layout.
    ///
    /// CSS Values defines viewport-percentage lengths against the initial
    /// containing block. CSS Paged Media uses the page area as the initial
    /// containing block for document layout, so layout projects these
    /// components to absolute lengths after the active page context is known:
    /// <https://www.w3.org/TR/css-values-4/#viewport-relative-lengths> and
    /// <https://www.w3.org/TR/css-page-3/#page-model>.
    pub(crate) fn resolve_viewport_lengths_for_viewport(&mut self, physical_viewport: LayoutSize) {
        let basis = ViewportLengthBasis::for_writing_mode(physical_viewport, self.writing_mode);
        ResolveViewportLengths::resolve_viewport_lengths(self, basis);
    }

    /// Returns whether either the legacy transform list or an independent
    /// transform property applies a non-initial transformation.
    pub(crate) fn has_transform(&self) -> bool {
        !self.transform.is_empty() || !self.individual_transforms.is_none()
    }

    /// Return the used `letter-spacing` length in layout units.
    ///
    /// CSS Text Level 4 accepts `normal | <length-percentage>` for
    /// `letter-spacing`; percentages resolve against the used font size and
    /// font-relative components such as `ch` resolve before layout consumes
    /// the used value:
    /// <https://drafts.csswg.org/css-text-4/#letter-spacing-property> and
    /// <https://www.w3.org/TR/css-values-4/#font-relative-lengths>.
    pub(crate) fn used_letter_spacing(&self) -> LayoutLength {
        self.letter_spacing
            .used_length_with_percentage_basis(PercentageBasis::definite(layout_pt(self.font_size)))
            .unwrap_or(self.letter_spacing.fixed_component())
    }

    /// Return the used `word-spacing` length in layout units.
    ///
    /// CSS Text Level 4 defines `word-spacing` as an inherited spacing
    /// adjustment applied between words. Percentages inherit intact and resolve
    /// against the current element's used font size before text shaping and
    /// line breaking consume the used value:
    /// <https://www.w3.org/TR/css-text-4/#word-spacing-property>.
    pub(crate) fn used_word_spacing(&self) -> LayoutLength {
        self.word_spacing
            .used_length_with_percentage_basis(PercentageBasis::definite(layout_pt(self.font_size)))
            .unwrap_or(self.word_spacing.fixed_component())
    }
}

fn scaled_edges(edges: Edges, factor: f32) -> Edges {
    Edges {
        top: edges.top * factor,
        right: edges.right * factor,
        bottom: edges.bottom * factor,
        left: edges.left * factor,
    }
}

impl ResolveViewportLengths for ComputedStyle {
    fn resolve_viewport_lengths(&mut self, basis: ViewportLengthBasis) {
        self.row_gap.resolve_viewport_lengths(basis);
        self.column_gap.resolve_viewport_lengths(basis);
        self.row_rule.resolve_viewport_lengths(basis);
        self.column_rule.resolve_viewport_lengths(basis);
        self.grid_template_rows.resolve_viewport_lengths(basis);
        self.grid_template_columns.resolve_viewport_lengths(basis);
        self.grid_auto_rows.resolve_viewport_lengths(basis);
        self.grid_auto_columns.resolve_viewport_lengths(basis);
        self.grid_lanes_flow_tolerance
            .resolve_viewport_lengths(basis);
        self.column_width.resolve_viewport_lengths(basis);
        self.column_height.resolve_viewport_lengths(basis);
        self.letter_spacing.resolve_viewport_lengths(basis);
        self.word_spacing.resolve_viewport_lengths(basis);
        self.box_values.resolve_viewport_lengths(basis);
        self.border_radius.resolve_viewport_lengths(basis);
        self.border_width_values.top.resolve_viewport_lengths(basis);
        self.border_width_values
            .right
            .resolve_viewport_lengths(basis);
        self.border_width_values
            .bottom
            .resolve_viewport_lengths(basis);
        self.border_width_values
            .left
            .resolve_viewport_lengths(basis);
        self.border_widths = Edges {
            top: self.border_width_values.top.length_max_zero().points(),
            right: self.border_width_values.right.length_max_zero().points(),
            bottom: self.border_width_values.bottom.length_max_zero().points(),
            left: self.border_width_values.left.length_max_zero().points(),
        };
        self.border_width = self
            .border_widths
            .top
            .max(self.border_widths.right)
            .max(self.border_widths.bottom)
            .max(self.border_widths.left);
        self.outline_width_value.resolve_viewport_lengths(basis);
        self.outline_width = self.outline_width_value.length_max_zero().points();
        self.outline_offset.resolve_viewport_lengths(basis);
        for edge in [
            &mut self.scroll_padding.top,
            &mut self.scroll_padding.right,
            &mut self.scroll_padding.bottom,
            &mut self.scroll_padding.left,
        ] {
            if let ScrollPadding::LengthPercentage(value) = edge {
                value.resolve_viewport_lengths(basis);
            }
        }
        for edge in [
            &mut self.scroll_margin.top,
            &mut self.scroll_margin.right,
            &mut self.scroll_margin.bottom,
            &mut self.scroll_margin.left,
        ] {
            edge.resolve_viewport_lengths(basis);
        }
        self.flex_basis.resolve_viewport_lengths(basis);
        self.text_indent.amount.resolve_viewport_lengths(basis);
        self.vertical_align.resolve_viewport_lengths(basis);
        self.tab_size.resolve_viewport_lengths(basis);
        if let Some(image) = &mut self.background_image {
            image.resolve_viewport_lengths(basis);
        }
        self.background_size.resolve_viewport_lengths(basis);
        self.background_position.resolve_viewport_lengths(basis);
        self.object_position.resolve_viewport_lengths(basis);
        for layer in &mut self.background_layers {
            layer.resolve_viewport_lengths(basis);
        }
        for function in &mut self.transform {
            function.resolve_viewport_lengths(basis);
        }
        self.individual_transforms.resolve_viewport_lengths(basis);
        self.transform_origin.resolve_viewport_lengths(basis);
        self.object_view_box.resolve_viewport_lengths(basis);
        self.border_image.resolve_viewport_lengths(basis);
        self.text_decoration.resolve_viewport_lengths(basis);
        self.border_spacing.resolve_viewport_lengths(basis);
        for shadow in &mut self.text_shadow {
            shadow.resolve_viewport_lengths(basis);
        }
        for shadow in &mut self.box_shadow {
            shadow.resolve_viewport_lengths(basis);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn upright_vertical_text_uses_ltr_direction_without_rewriting_computed_value() {
        let mut style = ComputedStyle::initial();
        style.writing_mode = WritingMode::VerticalLr;
        style.direction = Direction::Rtl;
        style.text_orientation = TextOrientation::Upright;

        assert_eq!(style.direction, Direction::Rtl);
        assert_eq!(style.used_direction(), Direction::Ltr);

        style.writing_mode = WritingMode::SidewaysLr;
        assert_eq!(style.used_direction(), Direction::Rtl);
    }

    #[test]
    fn zoom_keeps_computed_zero_but_normalizes_its_used_factor() {
        let zero = CssZoom::parse("0").unwrap();
        let percentage = CssZoom::parse("150%").unwrap();

        assert_eq!(zero.factor(), 0.0);
        assert_eq!(zero.used_factor(), 1.0);
        assert_eq!(percentage.used_factor(), 1.5);
        assert_eq!(
            EffectiveZoom::from_parent_and_local(
                EffectiveZoom::from_parent_and_local(EffectiveZoom::NORMAL, percentage),
                percentage,
            )
            .factor(),
            2.25
        );
        assert!(CssZoom::parse("-1").is_none());
    }

    #[test]
    fn effective_zoom_scales_fixed_lengths_without_scaling_percentages() {
        let mut style = ComputedStyle::initial();
        style.effective_zoom = EffectiveZoom(2.0);
        style.box_values.width = ComputedLengthPercentageOrAuto::LengthPercentage(
            ComputedLengthPercentage::from_affine(layout_pt(9.0), 0.5, true),
        );
        style.font_size = 10.0;
        style.line_height = 12.0;
        style.apply_effective_zoom();

        let ComputedLengthPercentageOrAuto::LengthPercentage(width) = style.box_values.width else {
            panic!("test width remains a length-percentage");
        };
        assert_eq!(
            width
                .used_length_with_percentage_basis(PercentageBasis::definite(layout_pt(100.0)))
                .unwrap(),
            layout_pt(68.0)
        );
        assert_eq!(style.font_size, 20.0);
        assert_eq!(style.line_height, 24.0);
    }

    #[test]
    fn effective_zoom_scales_fixed_position_insets_once() {
        let mut style = ComputedStyle::initial();
        style.effective_zoom = EffectiveZoom(2.0);
        style.box_values.inset_left = ComputedLengthPercentageOrAuto::LengthPercentage(
            ComputedLengthPercentage::from_affine(layout_pt(9.0), 0.25, true),
        );
        style.box_values.inset_top = ComputedLengthPercentageOrAuto::LengthPercentage(
            ComputedLengthPercentage::from_points(7.0),
        );
        style.box_values.inset_right = ComputedLengthPercentageOrAuto::AUTO;

        style.apply_effective_zoom();
        style.apply_effective_zoom();

        let ComputedLengthPercentageOrAuto::LengthPercentage(left) = style.box_values.inset_left
        else {
            panic!("left inset remains a length-percentage");
        };
        assert_eq!(left.length_points(), 18.0);
        assert_eq!(left.percentage_coefficient_or_zero(), 0.25);
        let ComputedLengthPercentageOrAuto::LengthPercentage(top) = style.box_values.inset_top
        else {
            panic!("top inset remains a length-percentage");
        };
        assert_eq!(top.length_points(), 14.0);
        assert!(style.box_values.inset_right.is_auto());
    }

    #[test]
    fn effective_zoom_scales_flex_lengths_without_scaling_percentages() {
        let mut style = ComputedStyle::initial();
        style.effective_zoom = EffectiveZoom(2.0);
        style.flex_basis = ComputedFlexBasis::LengthPercentage(ComputedFlexBasisLength::new(
            ComputedLengthPercentage::from_affine(layout_pt(9.0), 0.5, true),
        ));
        style.row_gap = ComputedGap::LengthPercentage(ComputedLengthPercentage::from_affine(
            layout_pt(3.0),
            0.25,
            true,
        ));
        style.column_gap =
            ComputedGap::LengthPercentage(ComputedLengthPercentage::from_points(7.0));

        style.apply_effective_zoom();

        let ComputedFlexBasis::LengthPercentage(basis) = style.flex_basis else {
            panic!("flex basis remains a length-percentage");
        };
        assert_eq!(
            basis
                .value
                .used_length_with_percentage_basis(PercentageBasis::definite(layout_pt(100.0)))
                .unwrap(),
            layout_pt(68.0)
        );
        let ComputedGap::LengthPercentage(row_gap) = style.row_gap else {
            panic!("row gap remains a length-percentage");
        };
        assert_eq!(
            row_gap
                .used_length_with_percentage_basis(PercentageBasis::definite(layout_pt(100.0)))
                .unwrap(),
            layout_pt(31.0)
        );
        let ComputedGap::LengthPercentage(column_gap) = style.column_gap else {
            panic!("column gap remains a length-percentage");
        };
        assert_eq!(column_gap.length_points(), 14.0);
    }

    #[test]
    fn effective_zoom_scales_multicol_dimensions_and_rule_geometry_once() {
        let mut style = ComputedStyle::initial();
        style.effective_zoom = EffectiveZoom(2.0);
        style.column_width = ComputedColumnWidth::Length(ComputedLengthPercentage::from_affine(
            layout_pt(9.0),
            0.0,
            false,
        ));
        style.column_height = ComputedColumnHeight::Length(ComputedLengthPercentage::from_affine(
            layout_pt(7.0),
            0.0,
            false,
        ));
        style.column_rule.widths = GapRuleList::from_parts(
            vec![GapRuleListComponent::Value(
                ComputedLengthPercentage::from_affine(layout_pt(3.0), 0.25, true),
            )],
            Some(vec![ComputedLengthPercentage::from_points(5.0)]),
            Vec::new(),
        );
        style.column_rule.inset_cap_start = GapRuleInsetValue::LengthPercentage(
            ComputedLengthPercentage::from_affine(layout_pt(2.0), 0.5, true),
        );

        style.apply_effective_zoom();
        style.apply_effective_zoom();

        let ComputedColumnWidth::Length(width) = style.column_width else {
            panic!("column width remains a length");
        };
        assert_eq!(width.length_points(), 18.0);
        let ComputedColumnHeight::Length(height) = style.column_height else {
            panic!("column height remains a length");
        };
        assert_eq!(height.length_points(), 14.0);
        let first_rule = style.column_rule.widths.value_for_index(0, 2).unwrap();
        assert_eq!(
            first_rule
                .used_length_with_percentage_basis(PercentageBasis::definite(layout_pt(100.0)))
                .unwrap(),
            layout_pt(31.0),
        );
        assert_eq!(
            style
                .column_rule
                .widths
                .value_for_index(1, 2)
                .unwrap()
                .length_points(),
            10.0,
        );
        let GapRuleInsetValue::LengthPercentage(inset) = style.column_rule.inset_cap_start else {
            panic!("fixed rule inset remains a length percentage");
        };
        assert_eq!(
            inset
                .used_length_with_percentage_basis(PercentageBasis::definite(layout_pt(100.0)))
                .unwrap(),
            layout_pt(54.0),
        );
    }

    #[test]
    fn effective_zoom_scales_fixed_border_spacing_once_without_scaling_percentages() {
        let mut style = ComputedStyle::initial();
        style.effective_zoom = EffectiveZoom(2.0);
        style.border_spacing = BorderSpacing {
            horizontal: ComputedLengthPercentage::from_affine(layout_pt(7.0), 0.25, true),
            vertical: ComputedLengthPercentage::from_points(11.0),
        };

        style.apply_effective_zoom();
        style.apply_effective_zoom();

        assert_eq!(
            style
                .border_spacing
                .horizontal
                .used_length_with_percentage_basis(PercentageBasis::definite(layout_pt(100.0)))
                .unwrap(),
            layout_pt(39.0),
        );
        assert_eq!(style.border_spacing.vertical.length_points(), 22.0);
    }

    #[test]
    fn effective_zoom_scales_fixed_grid_tracks_without_scaling_percentages_or_fr() {
        let fixed_and_percentage =
            ComputedLengthPercentage::from_affine(layout_pt(6.0), 0.25, true);
        let fixed_fit_content = ComputedLengthPercentage::from_points(8.0);
        let mut style = ComputedStyle::initial();
        style.effective_zoom = EffectiveZoom(2.0);
        style.grid_template_columns = GridTrackList::Tracks {
            components: vec![GridTrackListComponent::Repeat(
                Vec::new(),
                GridRepeat {
                    count: GridRepeatCount::Number(2),
                    tracks: vec![
                        GridTrackListComponent::Track(
                            Vec::new(),
                            GridTrackSize {
                                min: GridMinTrackBreadth::LengthPercentage(fixed_and_percentage),
                                max: GridMaxTrackBreadth::Flex(1.0),
                            },
                        ),
                        GridTrackListComponent::Track(
                            Vec::new(),
                            GridTrackSize {
                                min: GridMinTrackBreadth::Auto,
                                max: GridMaxTrackBreadth::FitContent(fixed_fit_content),
                            },
                        ),
                    ],
                    trailing_names: Vec::new(),
                },
            )],
            trailing_names: Vec::new(),
        };
        style.grid_auto_rows = GridAutoTrackList {
            tracks: vec![GridTrackSize {
                min: GridMinTrackBreadth::LengthPercentage(ComputedLengthPercentage::from_points(
                    9.0,
                )),
                max: GridMaxTrackBreadth::MaxContent,
            }],
        };

        style.apply_effective_zoom();

        let GridTrackList::Tracks { components, .. } = &style.grid_template_columns else {
            panic!("grid template remains an explicit track list");
        };
        let GridTrackListComponent::Repeat(_, repeat) = &components[0] else {
            panic!("nested repeat remains present");
        };
        let GridTrackListComponent::Track(_, fixed_track) = &repeat.tracks[0] else {
            panic!("fixed track remains present");
        };
        let GridMinTrackBreadth::LengthPercentage(value) = &fixed_track.min else {
            panic!("track minimum remains a length-percentage");
        };
        assert_eq!(
            value
                .used_length_with_percentage_basis(PercentageBasis::definite(layout_pt(100.0)))
                .unwrap(),
            layout_pt(37.0)
        );
        assert!(matches!(fixed_track.max, GridMaxTrackBreadth::Flex(1.0)));
        let GridTrackListComponent::Track(_, fit_track) = &repeat.tracks[1] else {
            panic!("fit-content track remains present");
        };
        let GridMaxTrackBreadth::FitContent(value) = &fit_track.max else {
            panic!("track maximum remains fit-content");
        };
        assert_eq!(value.length_points(), 16.0);
        let GridMinTrackBreadth::LengthPercentage(value) = &style.grid_auto_rows.tracks[0].min
        else {
            panic!("implicit track minimum remains a length-percentage");
        };
        assert_eq!(value.length_points(), 18.0);
    }

    #[test]
    fn cloned_styles_do_not_cross_contaminate_shared_length_expression_trees() {
        let mut original = ComputedStyle::initial();
        original.box_values.width =
            ComputedLengthPercentageOrAuto::LengthPercentage(ComputedLengthPercentage::sum(
                ComputedLengthPercentage::from_rem(1.0),
                ComputedLengthPercentage::from_em(1.0),
            ));

        let mut first = original.clone();
        first.font_size = 10.0;
        first.root_font_size = 20.0;
        first.finalize_computed_font_relative_lengths();

        let mut second = original.clone();
        second.font_size = 30.0;
        second.root_font_size = 40.0;
        second.finalize_computed_font_relative_lengths();

        assert_eq!(first.box_values.width.length_if_no_percent(), Some(30.0));
        assert_eq!(second.box_values.width.length_if_no_percent(), Some(70.0));
        assert_eq!(original.box_values.width.length_if_no_percent(), None);
    }
}

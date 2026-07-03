use super::*;

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ComputedStyle {
    pub custom_properties: HashMap<String, String>,
    pub display: Display,
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
    pub order: i32,
    pub row_gap: ComputedGap,
    pub row_rule: GapRuleAxis,
    pub grid_template_rows: GridTrackList,
    pub grid_template_columns: GridTrackList,
    pub grid_template_areas: GridTemplateAreas,
    pub grid_auto_rows: GridAutoTrackList,
    pub grid_auto_columns: GridAutoTrackList,
    pub grid_auto_flow: GridAutoFlow,
    pub grid_row_start: GridPlacement,
    pub grid_row_end: GridPlacement,
    pub grid_column_start: GridPlacement,
    pub grid_column_end: GridPlacement,
    pub column_count: Option<usize>,
    pub column_width: ComputedColumnWidth,
    pub column_gap: ComputedGap,
    pub column_rule: GapRuleAxis,
    pub rule_overlap: GapRuleOverlap,
    pub box_values: ComputedBoxValues,
    pub aspect_ratio: AspectRatio,
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
    pub background_image: Option<BackgroundImage>,
    pub background_size: BackgroundSize,
    pub background_position: BackgroundPosition,
    pub background_repeat: BackgroundRepeat,
    pub background_origin: BackgroundBox,
    pub background_clip: BackgroundBox,
    pub background_layers: Vec<BackgroundLayer>,
    pub box_decoration_break: BoxDecorationBreak,
    pub box_shadow: Vec<BoxShadow>,
    pub color: Color,
    pub font_size: f32,
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
    pub text_align: TextAlign,
    pub text_align_last: TextAlignLast,
    pub text_justify: TextJustify,
    pub text_autospace: TextAutospace,
    pub line_fit_edge: LineFitEdge,
    pub text_box_trim: TextBoxTrim,
    pub text_box_edge: TextBoxEdge,
    pub text_indent: ComputedTextIndent,
    pub hanging_punctuation: HangingPunctuation,
    pub vertical_align: VerticalAlign,
    pub font_weight: FontWeight,
    pub font_style: FontStyle,
    pub font_width: FontWidth,
    pub font_family: FontFamily,
    pub font_feature_settings: FontFeatureSettings,
    pub font_kerning: FontKerning,
    pub font_variant_ligatures: FontVariantLigatures,
    pub font_variant_position: FontVariantPosition,
    pub font_variant_caps: FontVariantCaps,
    pub font_variant_numeric: FontVariantNumeric,
    pub font_variant_alternates: FontVariantAlternates,
    pub font_variant_east_asian: FontVariantEastAsian,
    pub font_variant_emoji: FontVariantEmoji,
    pub language: Option<String>,
    pub text_transform: TextTransform,
    pub white_space: WhiteSpace,
    pub tab_size: TabSize,
    pub word_break: WordBreak,
    pub overflow: Overflow,
    pub overflow_x: Overflow,
    pub overflow_y: Overflow,
    pub overflow_wrap: OverflowWrap,
    pub line_break: LineBreak,
    pub hyphens: Hyphens,
    pub hyphenate_limit_chars: HyphenateLimitChars,
    pub visibility: Visibility,
    pub list_style_type: ListStyleType,
    pub list_style_position: ListStylePosition,
    pub list_style_image: Option<String>,
    pub list_style_image_base_url: Option<PathBuf>,
    pub list_style_image_root_url: Option<PathBuf>,
    pub marker_side: MarkerSide,
    pub marker_content: MarkerContent,
    pub marker_style: Option<Box<ComputedStyle>>,
    pub content: Content,
    pub before_style: Option<Box<ComputedStyle>>,
    pub after_style: Option<Box<ComputedStyle>>,
    pub first_line_style: Option<Box<ComputedStyle>>,
    pub first_letter_style: Option<Box<ComputedStyle>>,
    pub quotes: Quotes,
    pub counter_resets: Vec<(String, i32)>,
    pub counter_increments: Vec<(String, i32)>,
    pub counter_sets: Vec<(String, i32)>,
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
    pub transform_origin: TransformOrigin,
    pub isolation: Isolation,
    pub mix_blend_mode: MixBlendMode,
    pub filter: FilterValue,
    pub clip_path: ClipPath,
    pub mask: MaskValue,
    pub contain: Contain,
    pub content_visibility: ContentVisibility,
    pub will_change: WillChange,
    pub bookmark_level: Option<u32>,
    pub bookmark_label: BookmarkLabel,
    pub bookmark_state: CssBookmarkState,
}

impl ComputedStyle {
    pub fn initial() -> Self {
        let font_size = 12.0;
        Self {
            custom_properties: HashMap::new(),
            display: Display::INLINE,
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
            order: 0,
            row_gap: ComputedGap::NORMAL,
            row_rule: GapRuleAxis::initial(),
            grid_template_rows: GridTrackList::NONE,
            grid_template_columns: GridTrackList::NONE,
            grid_template_areas: GridTemplateAreas::NONE,
            grid_auto_rows: GridAutoTrackList::initial(),
            grid_auto_columns: GridAutoTrackList::initial(),
            grid_auto_flow: GridAutoFlow::ROW,
            grid_row_start: GridPlacement::AUTO,
            grid_row_end: GridPlacement::AUTO,
            grid_column_start: GridPlacement::AUTO,
            grid_column_end: GridPlacement::AUTO,
            column_count: None,
            column_width: ComputedColumnWidth::AUTO,
            column_gap: ComputedGap::NORMAL,
            column_rule: GapRuleAxis::initial(),
            rule_overlap: GapRuleOverlap::RowOverColumn,
            box_values: ComputedBoxValues::initial(),
            aspect_ratio: AspectRatio::AUTO,
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
            background_image: None,
            background_size: BackgroundSize::AUTO,
            background_position: BackgroundPosition::INITIAL,
            background_repeat: BackgroundRepeat::Repeat,
            background_origin: BackgroundBox::Padding,
            background_clip: BackgroundBox::Border,
            background_layers: Vec::new(),
            box_decoration_break: BoxDecorationBreak::Slice,
            box_shadow: Vec::new(),
            color: Color::BLACK,
            font_size,
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
            text_align: TextAlign::Start,
            text_align_last: TextAlignLast::Auto,
            text_justify: TextJustify::Auto,
            text_autospace: TextAutospace::NORMAL,
            line_fit_edge: LineFitEdge::Leading,
            text_box_trim: TextBoxTrim::None,
            text_box_edge: TextBoxEdge::Auto,
            text_indent: ComputedTextIndent::ZERO,
            hanging_punctuation: HangingPunctuation::NONE,
            vertical_align: VerticalAlign::BASELINE,
            font_weight: FontWeight::NORMAL,
            font_style: FontStyle::Normal,
            font_width: FontWidth::NORMAL,
            font_family: FontFamily::SansSerif,
            font_feature_settings: FontFeatureSettings::NORMAL,
            font_kerning: FontKerning::Auto,
            font_variant_ligatures: FontVariantLigatures::Normal,
            font_variant_position: FontVariantPosition::Normal,
            font_variant_caps: FontVariantCaps::Normal,
            font_variant_numeric: FontVariantNumeric::Normal,
            font_variant_alternates: FontVariantAlternates::Normal,
            font_variant_east_asian: FontVariantEastAsian::Normal,
            font_variant_emoji: FontVariantEmoji::Normal,
            language: None,
            text_transform: TextTransform::NONE,
            white_space: WhiteSpace::Normal,
            tab_size: TabSize::INITIAL,
            word_break: WordBreak::Normal,
            overflow: Overflow::Visible,
            overflow_x: Overflow::Visible,
            overflow_y: Overflow::Visible,
            overflow_wrap: OverflowWrap::Normal,
            line_break: LineBreak::Auto,
            hyphens: Hyphens::Manual,
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
            transform_origin: TransformOrigin::INITIAL,
            isolation: Isolation::Auto,
            mix_blend_mode: MixBlendMode::Normal,
            filter: FilterValue::None,
            clip_path: ClipPath::None,
            mask: MaskValue::None,
            contain: Contain::NONE,
            content_visibility: ContentVisibility::Visible,
            will_change: WillChange::default(),
            bookmark_level: None,
            bookmark_label: BookmarkLabel::content_text(),
            bookmark_state: CssBookmarkState::Open,
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
    pub(crate) fn resolve_font_metric_lengths(&mut self, ch_advance: f32) {
        self.line_height_value
            .resolve_font_metric_lengths(ch_advance);
        let (line_height, multiplier, is_normal) = self.line_height_value.projected(self.font_size);
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
        self.letter_spacing.resolve_font_metric_lengths(ch_advance);
        self.word_spacing.resolve_font_metric_lengths(ch_advance);
        self.box_values.resolve_font_metric_lengths(ch_advance);
        self.border_radius.resolve_font_metric_lengths(ch_advance);
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
            top: self.border_width_values.top.length_points_max_zero(),
            right: self.border_width_values.right.length_points_max_zero(),
            bottom: self.border_width_values.bottom.length_points_max_zero(),
            left: self.border_width_values.left.length_points_max_zero(),
        };
        self.border_width = self
            .border_widths
            .top
            .max(self.border_widths.right)
            .max(self.border_widths.bottom)
            .max(self.border_widths.left);
        self.outline_width_value
            .resolve_font_metric_lengths(ch_advance);
        self.outline_width = self.outline_width_value.length_points_max_zero();
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
        for layer in &mut self.background_layers {
            layer.resolve_font_metric_lengths(ch_advance);
        }
        for function in &mut self.transform {
            function.resolve_font_metric_lengths(ch_advance);
        }
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
        ch_advance: f32,
    ) {
        let height = self.box_values.height;
        let min_height = self.box_values.min_height;
        let max_height = self.box_values.max_height;
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
    pub(crate) fn resolve_viewport_lengths(&mut self, viewport_width: f32, viewport_height: f32) {
        let (viewport_inline, viewport_block) = match self.writing_mode {
            WritingMode::HorizontalTb => (viewport_width, viewport_height),
            WritingMode::VerticalRl | WritingMode::VerticalLr => (viewport_height, viewport_width),
        };
        self.row_gap.resolve_viewport_lengths(
            viewport_width,
            viewport_height,
            viewport_inline,
            viewport_block,
        );
        self.column_gap.resolve_viewport_lengths(
            viewport_width,
            viewport_height,
            viewport_inline,
            viewport_block,
        );
        self.row_rule.resolve_viewport_lengths(
            viewport_width,
            viewport_height,
            viewport_inline,
            viewport_block,
        );
        self.column_rule.resolve_viewport_lengths(
            viewport_width,
            viewport_height,
            viewport_inline,
            viewport_block,
        );
        self.grid_template_rows.resolve_viewport_lengths(
            viewport_width,
            viewport_height,
            viewport_inline,
            viewport_block,
        );
        self.grid_template_columns.resolve_viewport_lengths(
            viewport_width,
            viewport_height,
            viewport_inline,
            viewport_block,
        );
        self.grid_auto_rows.resolve_viewport_lengths(
            viewport_width,
            viewport_height,
            viewport_inline,
            viewport_block,
        );
        self.grid_auto_columns.resolve_viewport_lengths(
            viewport_width,
            viewport_height,
            viewport_inline,
            viewport_block,
        );
        self.column_width.resolve_viewport_lengths(
            viewport_width,
            viewport_height,
            viewport_inline,
            viewport_block,
        );
        self.letter_spacing.resolve_viewport_lengths(
            viewport_width,
            viewport_height,
            viewport_inline,
            viewport_block,
        );
        self.word_spacing.resolve_viewport_lengths(
            viewport_width,
            viewport_height,
            viewport_inline,
            viewport_block,
        );
        self.box_values.resolve_viewport_lengths(
            viewport_width,
            viewport_height,
            viewport_inline,
            viewport_block,
        );
        self.border_radius.resolve_viewport_lengths(
            viewport_width,
            viewport_height,
            viewport_inline,
            viewport_block,
        );
        self.border_width_values.top.resolve_viewport_lengths(
            viewport_width,
            viewport_height,
            viewport_inline,
            viewport_block,
        );
        self.border_width_values.right.resolve_viewport_lengths(
            viewport_width,
            viewport_height,
            viewport_inline,
            viewport_block,
        );
        self.border_width_values.bottom.resolve_viewport_lengths(
            viewport_width,
            viewport_height,
            viewport_inline,
            viewport_block,
        );
        self.border_width_values.left.resolve_viewport_lengths(
            viewport_width,
            viewport_height,
            viewport_inline,
            viewport_block,
        );
        self.border_widths = Edges {
            top: self.border_width_values.top.length_points_max_zero(),
            right: self.border_width_values.right.length_points_max_zero(),
            bottom: self.border_width_values.bottom.length_points_max_zero(),
            left: self.border_width_values.left.length_points_max_zero(),
        };
        self.border_width = self
            .border_widths
            .top
            .max(self.border_widths.right)
            .max(self.border_widths.bottom)
            .max(self.border_widths.left);
        self.outline_width_value.resolve_viewport_lengths(
            viewport_width,
            viewport_height,
            viewport_inline,
            viewport_block,
        );
        self.outline_width = self.outline_width_value.length_points_max_zero();
        self.outline_offset.resolve_viewport_lengths(
            viewport_width,
            viewport_height,
            viewport_inline,
            viewport_block,
        );
        self.flex_basis.resolve_viewport_lengths(
            viewport_width,
            viewport_height,
            viewport_inline,
            viewport_block,
        );
        self.text_indent.amount.resolve_viewport_lengths(
            viewport_width,
            viewport_height,
            viewport_inline,
            viewport_block,
        );
        self.vertical_align.resolve_viewport_lengths(
            viewport_width,
            viewport_height,
            viewport_inline,
            viewport_block,
        );
        self.tab_size.resolve_viewport_lengths(
            viewport_width,
            viewport_height,
            viewport_inline,
            viewport_block,
        );
        if let Some(image) = &mut self.background_image {
            image.resolve_viewport_lengths(
                viewport_width,
                viewport_height,
                viewport_inline,
                viewport_block,
            );
        }
        self.background_size.resolve_viewport_lengths(
            viewport_width,
            viewport_height,
            viewport_inline,
            viewport_block,
        );
        self.background_position.resolve_viewport_lengths(
            viewport_width,
            viewport_height,
            viewport_inline,
            viewport_block,
        );
        for layer in &mut self.background_layers {
            layer.resolve_viewport_lengths(
                viewport_width,
                viewport_height,
                viewport_inline,
                viewport_block,
            );
        }
        for function in &mut self.transform {
            function.resolve_viewport_lengths(
                viewport_width,
                viewport_height,
                viewport_inline,
                viewport_block,
            );
        }
        self.transform_origin.resolve_viewport_lengths(
            viewport_width,
            viewport_height,
            viewport_inline,
            viewport_block,
        );
        self.border_image.resolve_viewport_lengths(
            viewport_width,
            viewport_height,
            viewport_inline,
            viewport_block,
        );
        self.text_decoration.resolve_viewport_lengths(
            viewport_width,
            viewport_height,
            viewport_inline,
            viewport_block,
        );
        self.border_spacing.resolve_viewport_lengths(
            viewport_width,
            viewport_height,
            viewport_inline,
            viewport_block,
        );
        for shadow in &mut self.text_shadow {
            shadow.resolve_viewport_lengths(
                viewport_width,
                viewport_height,
                viewport_inline,
                viewport_block,
            );
        }
        for shadow in &mut self.box_shadow {
            shadow.resolve_viewport_lengths(
                viewport_width,
                viewport_height,
                viewport_inline,
                viewport_block,
            );
        }
    }

    /// Return the used `letter-spacing` length in layout units.
    ///
    /// CSS Text accepts `normal | <length>` for `letter-spacing`; font-relative
    /// components such as `ch` are resolved before layout consumes the used
    /// value:
    /// <https://www.w3.org/TR/css-text-3/#letter-spacing-property> and
    /// <https://www.w3.org/TR/css-values-4/#font-relative-lengths>.
    pub(crate) fn used_letter_spacing(&self) -> f32 {
        self.letter_spacing.length_points()
    }

    /// Return the used `word-spacing` length in layout units.
    ///
    /// CSS Text defines `word-spacing` as an inherited spacing adjustment
    /// applied between words. Font-relative lengths are resolved before text
    /// shaping and line breaking consume the used value:
    /// <https://www.w3.org/TR/css-text-3/#word-spacing-property>.
    pub(crate) fn used_word_spacing(&self) -> f32 {
        self.word_spacing.length_points()
    }
}

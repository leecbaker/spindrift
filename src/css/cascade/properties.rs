use std::sync::Arc;

use super::*;
use crate::css::cascade::declarations::affected_longhand_names;

/// Whether the cascade owns the named property or shorthand.
///
/// This is the property identity boundary shared by declaration application
/// and CSS Conditional feature queries.  It deliberately uses the cascade's
/// longhand and shorthand model instead of a second hand-maintained list.
pub(in crate::css) fn is_modeled_property_name(name: &str) -> bool {
    ModeledProperty::parse(name).is_some()
}

/// A canonical modeled property that owns one independently cascaded computed
/// value. Shorthands, legacy aliases, and logical spellings must be resolved
/// to these entries before CSS-wide defaulting or computed-value copying.
///
/// Keeping the static name private makes this the property-identity boundary
/// for the style copier. External cascade code continues to use CSS spelling
/// at its parsing boundary, but cannot accidentally copy an arbitrary string.
/// <https://www.w3.org/TR/css-cascade-5/#value-stages>
macro_rules! define_modeled_longhands {
    ($( $variant:ident => $name:literal, )*) => {
        /// A canonical modeled property that owns one independently cascaded
        /// computed value. Shorthands, legacy aliases, and logical spellings
        /// must be resolved to these entries before CSS-wide defaulting or
        /// computed-value copying.
        ///
        /// This closed enum is the cascade property-identity boundary. Adding a
        /// modeled longhand requires adding its identity to this registry, and
        /// the copy operation below is regression-tested against every variant.
        /// <https://www.w3.org/TR/css-cascade-5/#value-stages>
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
        #[repr(u16)]
        pub(crate) enum ModeledLonghand {
            $($variant,)*
        }

        impl ModeledLonghand {
            pub(in crate::css) fn parse(name: &str) -> Option<Self> {
                match name {
                    $($name => Some(Self::$variant),)*
                    _ => None,
                }
            }

            pub(crate) const fn css_name(self) -> &'static str {
                match self {
                    $(Self::$variant => $name,)*
                }
            }

            pub(crate) const fn index(self) -> usize {
                self as usize
            }

            /// Whether CSS Pseudo-Elements permits this canonical longhand on
            /// `::first-line`.
            ///
            /// Keeping applicability on the canonical identity makes aliases,
            /// logical properties, and shorthands converge on the same closed
            /// property set before layout materializes the pseudo box.
            /// <https://www.w3.org/TR/css-pseudo-4/#first-line-styling>
            pub(crate) fn is_first_line_allowed(self) -> bool {
                let name = self.css_name();
                name.starts_with("font")
                    || name.starts_with("background")
                    || matches!(
                        name,
                        "color"
                            | "letter-spacing"
                            | "line-height"
                            | "opacity"
                            | "tab-size"
                            | "text-decoration-line"
                            | "text-decoration-style"
                            | "text-decoration-color"
                            | "text-decoration-thickness"
                            | "text-decoration-inset"
                            | "text-decoration-skip"
                            | "text-decoration-skip-ink"
                            | "text-decoration-skip-self"
                            | "text-decoration-skip-box"
                            | "text-decoration-skip-spaces"
                            | "text-underline-offset"
                            | "text-underline-position"
                            | "text-emphasis-color"
                            | "text-emphasis-style"
                            | "text-emphasis-position"
                            | "text-emphasis-skip"
                            | "text-shadow"
                            | "text-transform"
                            | "vertical-align"
                            | "word-spacing"
                            | "ruby-position"
                            | "ruby-align"
                            | "ruby-overhang"
                    )
            }

            fn copy_from(self, style: &mut ComputedStyle, source: &ComputedStyle) {
                match self {
                    $(Self::$variant => copy_modeled_longhand_by_css_name(style, source, $name),)*
                }
            }
        }

        pub(in crate::css) const ALL_MODELED_LONGHANDS: &[ModeledLonghand] = &[
            $(ModeledLonghand::$variant,)*
        ];
    };
}

/// A compact set of canonical computed longhands.
///
/// The enum discriminant and this bitset are generated from the same registry,
/// so a newly modeled property cannot silently fall outside typed pseudo-style
/// propagation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ModeledLonghandSet {
    words: Arc<[u64]>,
}

impl ModeledLonghandSet {
    pub(crate) fn empty() -> Self {
        Self {
            words: Arc::default(),
        }
    }

    pub(crate) fn insert(&mut self, longhand: ModeledLonghand) {
        if self.words.len() != ALL_MODELED_LONGHANDS.len().div_ceil(64) {
            self.words = vec![0; ALL_MODELED_LONGHANDS.len().div_ceil(64)].into();
        }
        let index = longhand.index();
        Arc::make_mut(&mut self.words)[index / 64] |= 1_u64 << (index % 64);
    }

    #[cfg(test)]
    pub(crate) fn insert_css_name(&mut self, name: &str) {
        let longhand = ModeledLonghand::parse(name)
            .unwrap_or_else(|| panic!("test requested unknown modeled longhand `{name}`"));
        self.insert(longhand);
    }

    pub(crate) fn contains(&self, longhand: ModeledLonghand) -> bool {
        let index = longhand.index();
        self.words
            .get(index / 64)
            .is_some_and(|word| word & (1_u64 << (index % 64)) != 0)
    }

    pub(crate) fn iter(&self) -> impl Iterator<Item = ModeledLonghand> + '_ {
        ALL_MODELED_LONGHANDS
            .iter()
            .copied()
            .filter(|longhand| self.contains(*longhand))
    }
}

impl Default for ModeledLonghandSet {
    fn default() -> Self {
        Self::empty()
    }
}

define_modeled_longhands! {
    ColorScheme => "color-scheme",
    AnimationName => "animation-name",
    AnimationDuration => "animation-duration",
    AnimationDelay => "animation-delay",
    Zoom => "zoom",
    Display => "display",
    FlexDirection => "flex-direction",
    JustifyContent => "justify-content",
    JustifyItems => "justify-items",
    JustifySelf => "justify-self",
    AlignContent => "align-content",
    AlignItems => "align-items",
    AlignSelf => "align-self",
    FlexWrap => "flex-wrap",
    FlexLineCount => "flex-line-count",
    FlexGrow => "flex-grow",
    FlexShrink => "flex-shrink",
    FlexBasis => "flex-basis",
    Order => "order",
    RowGap => "row-gap",
    ColumnGap => "column-gap",
    RowRuleWidth => "row-rule-width",
    RowRuleStyle => "row-rule-style",
    RowRuleColor => "row-rule-color",
    RowRuleBreak => "row-rule-break",
    RowRuleVisibilityItems => "row-rule-visibility-items",
    RowRuleInsetCapStart => "row-rule-inset-cap-start",
    RowRuleInsetCapEnd => "row-rule-inset-cap-end",
    RowRuleInsetJunctionStart => "row-rule-inset-junction-start",
    RowRuleInsetJunctionEnd => "row-rule-inset-junction-end",
    ColumnRuleWidth => "column-rule-width",
    ColumnRuleStyle => "column-rule-style",
    ColumnRuleColor => "column-rule-color",
    ColumnRuleBreak => "column-rule-break",
    ColumnRuleVisibilityItems => "column-rule-visibility-items",
    ColumnRuleInsetCapStart => "column-rule-inset-cap-start",
    ColumnRuleInsetCapEnd => "column-rule-inset-cap-end",
    ColumnRuleInsetJunctionStart => "column-rule-inset-junction-start",
    ColumnRuleInsetJunctionEnd => "column-rule-inset-junction-end",
    RuleOverlap => "rule-overlap",
    GridTemplateRows => "grid-template-rows",
    GridTemplateColumns => "grid-template-columns",
    GridTemplateAreas => "grid-template-areas",
    GridAutoRows => "grid-auto-rows",
    GridAutoColumns => "grid-auto-columns",
    GridAutoFlow => "grid-auto-flow",
    GridLanesDirection => "grid-lanes-direction",
    FlowTolerance => "flow-tolerance",
    GridRowStart => "grid-row-start",
    GridRowEnd => "grid-row-end",
    GridColumnStart => "grid-column-start",
    GridColumnEnd => "grid-column-end",
    ColumnCount => "column-count",
    ColumnWidth => "column-width",
    ColumnHeight => "column-height",
    ColumnWrap => "column-wrap",
    ColumnFill => "column-fill",
    ColumnSpan => "column-span",
    MarginTrim => "margin-trim",
    MarginTop => "margin-top",
    MarginRight => "margin-right",
    MarginBottom => "margin-bottom",
    MarginLeft => "margin-left",
    PaddingTop => "padding-top",
    PaddingRight => "padding-right",
    PaddingBottom => "padding-bottom",
    PaddingLeft => "padding-left",
    BorderTopWidth => "border-top-width",
    BorderRightWidth => "border-right-width",
    BorderBottomWidth => "border-bottom-width",
    BorderLeftWidth => "border-left-width",
    BorderTopStyle => "border-top-style",
    BorderRightStyle => "border-right-style",
    BorderBottomStyle => "border-bottom-style",
    BorderLeftStyle => "border-left-style",
    BorderTopColor => "border-top-color",
    BorderRightColor => "border-right-color",
    BorderBottomColor => "border-bottom-color",
    BorderLeftColor => "border-left-color",
    BorderTopLeftRadius => "border-top-left-radius",
    BorderTopRightRadius => "border-top-right-radius",
    BorderBottomRightRadius => "border-bottom-right-radius",
    BorderBottomLeftRadius => "border-bottom-left-radius",
    CornerTopLeftShape => "corner-top-left-shape",
    CornerTopRightShape => "corner-top-right-shape",
    CornerBottomRightShape => "corner-bottom-right-shape",
    CornerBottomLeftShape => "corner-bottom-left-shape",
    ShapeOutside => "shape-outside",
    ShapeMargin => "shape-margin",
    ShapeImageThreshold => "shape-image-threshold",
    BorderShape => "border-shape",
    BorderImageSource => "border-image-source",
    BorderImageSlice => "border-image-slice",
    BorderImageWidth => "border-image-width",
    BorderImageOutset => "border-image-outset",
    BorderImageRepeat => "border-image-repeat",
    BorderCollapse => "border-collapse",
    CaptionSide => "caption-side",
    TableLayout => "table-layout",
    EmptyCells => "empty-cells",
    BorderSpacing => "border-spacing",
    BackgroundColor => "background-color",
    BackgroundImage => "background-image",
    BackgroundSize => "background-size",
    BackgroundPosition => "background-position",
    BackgroundPositionX => "background-position-x",
    BackgroundPositionY => "background-position-y",
    BackgroundRepeat => "background-repeat",
    BackgroundAttachment => "background-attachment",
    BackgroundOrigin => "background-origin",
    BackgroundClip => "background-clip",
    ObjectFit => "object-fit",
    ObjectViewBox => "object-view-box",
    ObjectPosition => "object-position",
    ImageRendering => "image-rendering",
    ImageOrientation => "image-orientation",
    BoxDecorationBreak => "box-decoration-break",
    OutlineOffset => "outline-offset",
    OutlineWidth => "outline-width",
    OutlineStyle => "outline-style",
    OutlineColor => "outline-color",
    BoxShadow => "box-shadow",
    Color => "color",
    ForcedColorAdjust => "forced-color-adjust",
    Fill => "fill",
    Stroke => "stroke",
    StrokeWidth => "stroke-width",
    FloodColor => "flood-color",
    LightingColor => "lighting-color",
    WebkitTextFillColor => "-webkit-text-fill-color",
    Direction => "direction",
    UnicodeBidi => "unicode-bidi",
    WritingMode => "writing-mode",
    TextOrientation => "text-orientation",
    TextCombineUpright => "text-combine-upright",
    TextFit => "text-fit",
    LineFitEdge => "line-fit-edge",
    TextBoxTrim => "text-box-trim",
    TextBoxEdge => "text-box-edge",
    InitialLetter => "initial-letter",
    InitialLetterAlign => "initial-letter-align",
    InitialLetterWrap => "initial-letter-wrap",
    FontSize => "font-size",
    FontSizeAdjust => "font-size-adjust",
    LineHeight => "line-height",
    LetterSpacing => "letter-spacing",
    WordSpacing => "word-spacing",
    Width => "width",
    Height => "height",
    AspectRatio => "aspect-ratio",
    ContainIntrinsicSize => "contain-intrinsic-size",
    ContainIntrinsicWidth => "contain-intrinsic-width",
    ContainIntrinsicHeight => "contain-intrinsic-height",
    MinWidth => "min-width",
    MaxWidth => "max-width",
    MinHeight => "min-height",
    MaxHeight => "max-height",
    BoxSizing => "box-sizing",
    Left => "left",
    Top => "top",
    Right => "right",
    Bottom => "bottom",
    Position => "position",
    Float => "float",
    FootnoteDisplay => "footnote-display",
    FootnotePolicy => "footnote-policy",
    Clear => "clear",
    ZIndex => "z-index",
    Opacity => "opacity",
    Transform => "transform",
    Translate => "translate",
    Rotate => "rotate",
    Scale => "scale",
    TransformOrigin => "transform-origin",
    Perspective => "perspective",
    PerspectiveOrigin => "perspective-origin",
    TransformBox => "transform-box",
    TransformStyle => "transform-style",
    BackfaceVisibility => "backface-visibility",
    Isolation => "isolation",
    MixBlendMode => "mix-blend-mode",
    Filter => "filter",
    Clip => "clip",
    ClipPath => "clip-path",
    MaskImage => "mask-image",
    MaskBorderSource => "mask-border-source",
    Contain => "contain",
    ContainerType => "container-type",
    ContainerName => "container-name",
    ContentVisibility => "content-visibility",
    WillChange => "will-change",
    TextAlignAll => "text-align-all",
    TextAlignLast => "text-align-last",
    TextJustify => "text-justify",
    TextAutospace => "text-autospace",
    TextSpacingTrim => "text-spacing-trim",
    WordSpaceTransform => "word-space-transform",
    TextIndent => "text-indent",
    HangingPunctuation => "hanging-punctuation",
    VerticalAlign => "vertical-align",
    DominantBaseline => "dominant-baseline",
    AlignmentBaseline => "alignment-baseline",
    BaselineSource => "baseline-source",
    BaselineShift => "baseline-shift",
    FontWeight => "font-weight",
    FontStyle => "font-style",
    FontWidth => "font-width",
    FontFamily => "font-family",
    FontLanguageOverride => "font-language-override",
    FontFeatureSettings => "font-feature-settings",
    FontVariationSettings => "font-variation-settings",
    FontPalette => "font-palette",
    FontSynthesis => "font-synthesis",
    FontSynthesisWeight => "font-synthesis-weight",
    FontSynthesisStyle => "font-synthesis-style",
    FontSynthesisSmallCaps => "font-synthesis-small-caps",
    FontSynthesisPosition => "font-synthesis-position",
    FontKerning => "font-kerning",
    FontVariantLigatures => "font-variant-ligatures",
    FontVariantPosition => "font-variant-position",
    FontVariantCaps => "font-variant-caps",
    FontVariantNumeric => "font-variant-numeric",
    FontVariantAlternates => "font-variant-alternates",
    FontVariantEastAsian => "font-variant-east-asian",
    FontVariantEmoji => "font-variant-emoji",
    BookmarkLevel => "bookmark-level",
    BookmarkLabel => "bookmark-label",
    BookmarkState => "bookmark-state",
    TextTransform => "text-transform",
    TabSize => "tab-size",
    Visibility => "visibility",
    ListStyleType => "list-style-type",
    ListStylePosition => "list-style-position",
    ListStyleImage => "list-style-image",
    MarkerSide => "marker-side",
    CounterReset => "counter-reset",
    CounterIncrement => "counter-increment",
    CounterSet => "counter-set",
    StringSet => "string-set",
    Page => "page",
    BreakBefore => "break-before",
    BreakAfter => "break-after",
    BreakInside => "break-inside",
    Orphans => "orphans",
    Widows => "widows",
    TextDecorationLine => "text-decoration-line",
    TextDecorationStyle => "text-decoration-style",
    TextDecorationColor => "text-decoration-color",
    TextDecorationThickness => "text-decoration-thickness",
    TextDecorationInset => "text-decoration-inset",
    TextDecorationSkipInk => "text-decoration-skip-ink",
    TextDecorationSkipSelf => "text-decoration-skip-self",
    TextDecorationSkipBox => "text-decoration-skip-box",
    TextDecorationSkipSpaces => "text-decoration-skip-spaces",
    TextUnderlineOffset => "text-underline-offset",
    TextUnderlinePosition => "text-underline-position",
    TextEmphasisStyle => "text-emphasis-style",
    TextEmphasisColor => "text-emphasis-color",
    TextEmphasisPosition => "text-emphasis-position",
    TextEmphasisSkip => "text-emphasis-skip",
    RubyPosition => "ruby-position",
    RubyAlign => "ruby-align",
    RubyOverhang => "ruby-overhang",
    TextShadow => "text-shadow",
    WhiteSpace => "white-space",
    TextWrap => "text-wrap",
    TextWrapMode => "text-wrap-mode",
    TextWrapStyle => "text-wrap-style",
    WrapInside => "wrap-inside",
    MaxLines => "max-lines",
    BlockEllipsis => "block-ellipsis",
    Continue => "continue",
    WebkitBoxOrient => "-webkit-box-orient",
    WordBreak => "word-break",
    OverflowX => "overflow-x",
    OverflowY => "overflow-y",
    ScrollbarGutter => "scrollbar-gutter",
    ScrollbarWidth => "scrollbar-width",
    ScrollSnapType => "scroll-snap-type",
    ScrollSnapAlign => "scroll-snap-align",
    ScrollSnapStop => "scroll-snap-stop",
    ScrollTargetGroup => "scroll-target-group",
    ScrollMarkerGroup => "scroll-marker-group",
    ScrollPaddingTop => "scroll-padding-top",
    ScrollPaddingRight => "scroll-padding-right",
    ScrollPaddingBottom => "scroll-padding-bottom",
    ScrollPaddingLeft => "scroll-padding-left",
    ScrollMarginTop => "scroll-margin-top",
    ScrollMarginRight => "scroll-margin-right",
    ScrollMarginBottom => "scroll-margin-bottom",
    ScrollMarginLeft => "scroll-margin-left",
    OverflowClipMargin => "overflow-clip-margin",
    OverflowWrap => "overflow-wrap",
    LineBreak => "line-break",
    Hyphens => "hyphens",
    HyphenateCharacter => "hyphenate-character",
    HyphenateLimitChars => "hyphenate-limit-chars",
    Content => "content",
    Quotes => "quotes",
}

/// A supported CSS property spelling before it has been resolved to the
/// canonical physical longhands that own computed values.
///
/// CSS Cascade applies the cascade to a shorthand's component longhands, and
/// CSS Logical Properties resolves flow-relative spellings in the element's
/// writing context. Keeping that syntax distinct from [`ModeledLonghand`]
/// makes it impossible for defaulting or computed-style copying to mistake a
/// shorthand for an independently owned computed value.
/// <https://www.w3.org/TR/css-cascade-5/#shorthand>
/// <https://www.w3.org/TR/css-logical-1/>
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::css) enum ModeledProperty {
    Longhand(ModeledLonghand),
    /// A canonical longhand emitted from a `font` shorthand. Its property
    /// identity is the longhand; the component marker retains just enough
    /// provenance to parse the shorthand value at computed-value time.
    FontComponent(ModeledLonghand),
    Shorthand(ModeledShorthand),
    Logical(LogicalProperty),
    Alias(LegacyPropertyAlias),
    All,
}

macro_rules! define_modeled_syntax {
    ($type:ident, $all:ident { $($variant:ident => $name:literal,)* }) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        pub(in crate::css) enum $type { $($variant,)* }

        impl $type {
            fn parse(name: &str) -> Option<Self> {
                match name { $($name => Some(Self::$variant),)* _ => None }
            }

            const fn css_name(self) -> &'static str {
                match self { $(Self::$variant => $name,)* }
            }
        }

        #[cfg(test)]
        const $all: &[$type] = &[$($type::$variant,)*];
    };
}

// Physical shorthands and aggregate spelling supported by Spindrift. Each spelling
// is a closed variant so cascade identity cannot retain an authored string.
define_modeled_syntax! {
    ModeledShorthand, ALL_MODELED_SHORTHANDS {
        Background => "background", Border => "border", BorderBottom => "border-bottom",
        BorderBottomRadius => "border-bottom-radius", BorderColor => "border-color",
        BorderImage => "border-image", BorderLeft => "border-left", BorderLeftRadius => "border-left-radius",
        BorderRadius => "border-radius", BorderRight => "border-right", BorderRightRadius => "border-right-radius",
        BorderStyle => "border-style", BorderTop => "border-top", BorderTopRadius => "border-top-radius",
        BorderWidth => "border-width", ColumnRule => "column-rule", ColumnRuleInset => "column-rule-inset",
        ColumnRuleInsetCap => "column-rule-inset-cap", ColumnRuleInsetEnd => "column-rule-inset-end",
        ColumnRuleInsetJunction => "column-rule-inset-junction", ColumnRuleInsetStart => "column-rule-inset-start",
        Columns => "columns", Container => "container", Corner => "corner", CornerShape => "corner-shape",
        Animation => "animation", Flex => "flex", FlexFlow => "flex-flow", Font => "font", FontVariant => "font-variant",
        Gap => "gap", GridColumnGap => "grid-column-gap", GridGap => "grid-gap", GridRowGap => "grid-row-gap",
        ListStyle => "list-style", Margin => "margin", Mask => "mask", MaskBorder => "mask-border", Outline => "outline",
        Overflow => "overflow", Padding => "padding", PlaceContent => "place-content", PlaceItems => "place-items",
        PlaceSelf => "place-self", RowRule => "row-rule", RowRuleInset => "row-rule-inset",
        RowRuleInsetCap => "row-rule-inset-cap", RowRuleInsetEnd => "row-rule-inset-end",
        RowRuleInsetJunction => "row-rule-inset-junction", RowRuleInsetStart => "row-rule-inset-start",
        Rule => "rule", RuleBreak => "rule-break", RuleColor => "rule-color", RuleInset => "rule-inset",
        RuleInsetCap => "rule-inset-cap", RuleInsetEnd => "rule-inset-end", RuleInsetJunction => "rule-inset-junction",
        RuleInsetStart => "rule-inset-start", RuleStyle => "rule-style", RuleVisibilityItems => "rule-visibility-items",
        RuleWidth => "rule-width", TextAlign => "text-align", TextBox => "text-box",
        TextDecoration => "text-decoration", TextDecorationSkip => "text-decoration-skip",
        TextEmphasis => "text-emphasis", TextSpacing => "text-spacing", LineClamp => "line-clamp",
        WebkitLineClamp => "-webkit-line-clamp", ScrollMargin => "scroll-margin", ScrollPadding => "scroll-padding",
        Inset => "inset", GridRow => "grid-row", GridColumn => "grid-column", Grid => "grid",
        GridTemplate => "grid-template", GridArea => "grid-area",
    }
}

// Flow-relative spellings resolve through writing mode and, for sides,
// direction.
define_modeled_syntax! {
    LogicalProperty, ALL_LOGICAL_PROPERTIES {
        InlineSize => "inline-size", BlockSize => "block-size", MinInlineSize => "min-inline-size",
        MaxInlineSize => "max-inline-size", MinBlockSize => "min-block-size", MaxBlockSize => "max-block-size",
        ContainIntrinsicInlineSize => "contain-intrinsic-inline-size", ContainIntrinsicBlockSize => "contain-intrinsic-block-size",
        MarginBlock => "margin-block", MarginInline => "margin-inline", PaddingBlock => "padding-block", PaddingInline => "padding-inline",
        ScrollPaddingBlock => "scroll-padding-block", ScrollPaddingInline => "scroll-padding-inline",
        ScrollMarginBlock => "scroll-margin-block", ScrollMarginInline => "scroll-margin-inline",
        InsetBlock => "inset-block", InsetInline => "inset-inline",
        MarginBlockStart => "margin-block-start", MarginBlockEnd => "margin-block-end", MarginInlineStart => "margin-inline-start", MarginInlineEnd => "margin-inline-end",
        PaddingBlockStart => "padding-block-start", PaddingBlockEnd => "padding-block-end", PaddingInlineStart => "padding-inline-start", PaddingInlineEnd => "padding-inline-end",
        ScrollPaddingBlockStart => "scroll-padding-block-start", ScrollPaddingBlockEnd => "scroll-padding-block-end", ScrollPaddingInlineStart => "scroll-padding-inline-start", ScrollPaddingInlineEnd => "scroll-padding-inline-end",
        ScrollMarginBlockStart => "scroll-margin-block-start", ScrollMarginBlockEnd => "scroll-margin-block-end", ScrollMarginInlineStart => "scroll-margin-inline-start", ScrollMarginInlineEnd => "scroll-margin-inline-end",
        InsetBlockStart => "inset-block-start", InsetBlockEnd => "inset-block-end", InsetInlineStart => "inset-inline-start", InsetInlineEnd => "inset-inline-end",
        BorderBlock => "border-block", BorderInline => "border-inline", BorderBlockStart => "border-block-start", BorderBlockEnd => "border-block-end", BorderInlineStart => "border-inline-start", BorderInlineEnd => "border-inline-end",
        BorderBlockWidth => "border-block-width", BorderInlineWidth => "border-inline-width", BorderBlockStyle => "border-block-style", BorderInlineStyle => "border-inline-style", BorderBlockColor => "border-block-color", BorderInlineColor => "border-inline-color",
        BorderBlockStartWidth => "border-block-start-width", BorderBlockEndWidth => "border-block-end-width", BorderInlineStartWidth => "border-inline-start-width", BorderInlineEndWidth => "border-inline-end-width",
        BorderBlockStartStyle => "border-block-start-style", BorderBlockEndStyle => "border-block-end-style", BorderInlineStartStyle => "border-inline-start-style", BorderInlineEndStyle => "border-inline-end-style",
        BorderBlockStartColor => "border-block-start-color", BorderBlockEndColor => "border-block-end-color", BorderInlineStartColor => "border-inline-start-color", BorderInlineEndColor => "border-inline-end-color",
        BorderBlockStartRadius => "border-block-start-radius", BorderBlockEndRadius => "border-block-end-radius", BorderInlineStartRadius => "border-inline-start-radius", BorderInlineEndRadius => "border-inline-end-radius",
        BorderStartStartRadius => "border-start-start-radius", BorderStartEndRadius => "border-start-end-radius", BorderEndStartRadius => "border-end-start-radius", BorderEndEndRadius => "border-end-end-radius",
    }
}

// Legacy spellings are syntax only; their cascade target is canonical.
define_modeled_syntax! {
    LegacyPropertyAlias, ALL_LEGACY_PROPERTY_ALIASES {
        WebkitFlexBasis => "-webkit-flex-basis", PageBreakBefore => "page-break-before",
        PageBreakAfter => "page-break-after", PageBreakInside => "page-break-inside",
        FontStretch => "font-stretch", WordWrap => "word-wrap",
    }
}

/// Canonical longhands addressed by one property spelling without a heap
/// allocation. Dynamic logical resolution has at most six targets; fixed
/// shorthand target sets remain borrowed from the declaration registry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::css) enum ResolvedLonghandTargets {
    Inline {
        targets: [Option<ModeledLonghand>; 6],
        len: u8,
    },
    StaticNames(&'static [&'static str]),
    All,
}

impl ResolvedLonghandTargets {
    pub(in crate::css) fn from_names<const N: usize>(names: [&'static str; N]) -> Self {
        assert!(
            N <= 6,
            "context-resolved target set exceeds inline capacity"
        );
        let mut targets = [None; 6];
        for (slot, name) in targets.iter_mut().zip(names) {
            *slot = Some(
                ModeledLonghand::parse(name)
                    .unwrap_or_else(|| panic!("unregistered modeled longhand target `{name}`")),
            );
        }
        Self::Inline {
            targets,
            len: N as u8,
        }
    }

    pub(in crate::css) const fn static_names(names: &'static [&'static str]) -> Self {
        Self::StaticNames(names)
    }
}

pub(in crate::css) enum ResolvedLonghandTargetIter {
    Inline {
        targets: [Option<ModeledLonghand>; 6],
        index: usize,
        len: usize,
    },
    StaticNames {
        names: &'static [&'static str],
        index: usize,
    },
    All {
        index: usize,
    },
}

impl Iterator for ResolvedLonghandTargetIter {
    type Item = ModeledLonghand;

    fn next(&mut self) -> Option<Self::Item> {
        match self {
            Self::Inline {
                targets,
                index,
                len,
            } => {
                if *index == *len {
                    None
                } else {
                    let target = targets[*index].expect("inline target length must be initialized");
                    *index += 1;
                    Some(target)
                }
            }
            Self::StaticNames { names, index } => {
                let name = *names.get(*index)?;
                *index += 1;
                Some(
                    ModeledLonghand::parse(name)
                        .unwrap_or_else(|| panic!("unregistered modeled longhand target `{name}`")),
                )
            }
            Self::All { index } => loop {
                let target = *ALL_MODELED_LONGHANDS.get(*index)?;
                *index += 1;
                if !matches!(
                    target,
                    ModeledLonghand::Direction | ModeledLonghand::UnicodeBidi
                ) {
                    return Some(target);
                }
            },
        }
    }
}

impl IntoIterator for ResolvedLonghandTargets {
    type Item = ModeledLonghand;
    type IntoIter = ResolvedLonghandTargetIter;

    fn into_iter(self) -> Self::IntoIter {
        match self {
            Self::Inline { targets, len } => ResolvedLonghandTargetIter::Inline {
                targets,
                index: 0,
                len: usize::from(len),
            },
            Self::StaticNames(names) => ResolvedLonghandTargetIter::StaticNames { names, index: 0 },
            Self::All => ResolvedLonghandTargetIter::All { index: 0 },
        }
    }
}

impl ModeledProperty {
    pub(in crate::css) fn parse(name: &str) -> Option<Self> {
        if let Some(longhand) = ModeledLonghand::parse(name) {
            return Some(Self::Longhand(longhand));
        }
        match name {
            "all" => Some(Self::All),
            _ => LegacyPropertyAlias::parse(name)
                .map(Self::Alias)
                .or_else(|| LogicalProperty::parse(name).map(Self::Logical))
                .or_else(|| ModeledShorthand::parse(name).map(Self::Shorthand)),
        }
    }

    pub(in crate::css) const fn css_name(&self) -> &'static str {
        match self {
            Self::Longhand(longhand) | Self::FontComponent(longhand) => longhand.css_name(),
            Self::Shorthand(name) => name.css_name(),
            Self::Logical(name) => name.css_name(),
            Self::Alias(name) => name.css_name(),
            Self::All => "all",
        }
    }

    /// Resolves this spelling to the physical longhands affected in `context`.
    pub(in crate::css) fn resolve_targets(
        &self,
        direction: Direction,
        writing_mode: WritingMode,
    ) -> ResolvedLonghandTargets {
        match self {
            Self::Longhand(longhand) | Self::FontComponent(longhand) => {
                ResolvedLonghandTargets::from_names([longhand.css_name()])
            }
            Self::All => ResolvedLonghandTargets::All,
            Self::Shorthand(name) => {
                modeled_targets_for_name(name.css_name(), direction, writing_mode)
            }
            Self::Logical(name) => {
                modeled_targets_for_name(name.css_name(), direction, writing_mode)
            }
            Self::Alias(alias) => modeled_targets_for_name(
                match alias {
                    LegacyPropertyAlias::WebkitFlexBasis => "flex-basis",
                    LegacyPropertyAlias::PageBreakBefore => "break-before",
                    LegacyPropertyAlias::PageBreakAfter => "break-after",
                    LegacyPropertyAlias::PageBreakInside => "break-inside",
                    LegacyPropertyAlias::FontStretch => "font-width",
                    LegacyPropertyAlias::WordWrap => "overflow-wrap",
                },
                direction,
                writing_mode,
            ),
        }
    }

    pub(in crate::css) const fn font_component(self) -> Option<ModeledLonghand> {
        match self {
            Self::FontComponent(longhand) => Some(longhand),
            _ => None,
        }
    }
}

fn modeled_targets_for_name(
    name: &str,
    direction: Direction,
    writing_mode: WritingMode,
) -> ResolvedLonghandTargets {
    if let Some(longhand) = ModeledLonghand::parse(name) {
        return ResolvedLonghandTargets::from_names([longhand.css_name()]);
    }
    affected_longhand_names(name, direction, writing_mode)
        .unwrap_or_else(|| panic!("modeled property `{name}` has no modeled longhand targets"))
}

pub(in crate::css) fn all_modeled_longhands() -> impl Iterator<Item = ModeledLonghand> {
    ALL_MODELED_LONGHANDS.iter().copied().filter(|longhand| {
        !matches!(
            longhand,
            ModeledLonghand::Direction | ModeledLonghand::UnicodeBidi
        )
    })
}

impl ModeledLonghand {
    pub(crate) fn is_inherited(self) -> bool {
        matches!(
            self.css_name(),
            "border-collapse"
                | "border-spacing"
                | "caption-side"
                | "color"
                | "color-scheme"
                | "forced-color-adjust"
                | "fill"
                | "stroke"
                | "stroke-width"
                | "flood-color"
                | "lighting-color"
                | "-webkit-text-fill-color"
                | "direction"
                | "dominant-baseline"
                | "empty-cells"
                | "font-family"
                | "font-language-override"
                | "font-feature-settings"
                | "font-variation-settings"
                | "font-palette"
                | "font-synthesis"
                | "font-synthesis-weight"
                | "font-synthesis-style"
                | "font-synthesis-small-caps"
                | "font-synthesis-position"
                | "font-kerning"
                | "font-size"
                | "font-size-adjust"
                | "font-style"
                | "font-variant-alternates"
                | "font-variant-caps"
                | "font-variant-east-asian"
                | "font-variant-emoji"
                | "font-variant-ligatures"
                | "font-variant-numeric"
                | "font-variant-position"
                | "font-width"
                | "font-weight"
                | "hyphenate-character"
                | "hyphenate-limit-chars"
                | "hyphens"
                | "image-rendering"
                | "image-orientation"
                | "letter-spacing"
                | "line-break"
                | "line-height"
                | "block-ellipsis"
                | "list-style-image"
                | "list-style-position"
                | "list-style-type"
                | "marker-side"
                | "orphans"
                | "overflow-wrap"
                | "quotes"
                | "text-align"
                | "text-combine-upright"
                | "text-align-all"
                | "text-align-last"
                | "text-justify"
                | "text-autospace"
                | "text-fit"
                | "text-spacing-trim"
                | "word-space-transform"
                | "initial-letter-align"
                | "initial-letter-wrap"
                | "line-fit-edge"
                | "text-box-edge"
                | "text-orientation"
                | "text-decoration-skip-ink"
                | "text-decoration-skip-self"
                | "text-decoration-skip-box"
                | "text-decoration-skip-spaces"
                | "text-underline-offset"
                | "text-underline-position"
                | "text-emphasis-color"
                | "text-emphasis-position"
                | "text-emphasis-skip"
                | "text-emphasis-style"
                | "ruby-position"
                | "ruby-align"
                | "ruby-overhang"
                | "text-shadow"
                | "text-indent"
                | "hanging-punctuation"
                | "text-transform"
                | "text-wrap"
                | "text-wrap-mode"
                | "text-wrap-style"
                | "tab-size"
                | "visibility"
                | "white-space"
                | "widows"
                | "word-break"
                | "word-spacing"
                | "word-wrap"
                | "writing-mode"
        )
    }
}

/// Builds a child style's pre-cascade inherited base from its parent.
///
/// CSS Cascade defines inheritance as each inherited property taking the
/// parent's computed value when no cascaded value applies. This helper keeps
/// that default inheritance path aligned with `inherit`/`unset` defaulting:
/// <https://www.w3.org/TR/css-cascade-5/#inheritance>.
pub(super) fn inherited_base_style(parent: &ComputedStyle) -> ComputedStyle {
    let mut style = ComputedStyle::initial();
    style.longhand_provenance.current_source =
        parent.longhand_provenance.current_source.saturating_add(1);
    style.longhand_provenance.sources = vec![0; ALL_MODELED_LONGHANDS.len()].into();
    // `zoom` itself is non-inherited, but layout carries its accumulated used
    // scale through each flat-tree descendant. Preserve that layout-only
    // context while the computed longhand remains its initial value.
    // <https://drafts.csswg.org/css-viewport/#zoom-property>
    style.effective_zoom = parent.effective_zoom;
    style.root_font_size = parent.root_font_size;
    style.custom_properties = parent.custom_properties.clone();
    style.used_color_scheme = parent.used_color_scheme;
    style.page_color_scheme = parent.page_color_scheme;
    style.language = parent.language.clone();
    for &longhand in ALL_MODELED_LONGHANDS {
        if longhand.is_inherited() {
            copy_modeled_longhand(&mut style, parent, longhand);
        }
    }
    style
}

/// Builds a pseudo-element style's pre-cascade inherited base.
///
/// Pseudo-elements inherit from their originating element, but generated
/// quote content must use the originating element's already computed quote
/// system rather than re-resolving `quotes: auto` against the originating
/// element's own language:
/// <https://www.w3.org/TR/css-pseudo-4/#generated-content> and
/// <https://www.w3.org/TR/css-content-3/#quotes-property>.
pub(super) fn pseudo_inherited_base_style(originating_style: &ComputedStyle) -> ComputedStyle {
    let mut style = inherited_base_style(originating_style);
    style.quotes = originating_style.quotes.clone();
    style
}

pub(crate) fn copy_modeled_longhand(
    style: &mut ComputedStyle,
    source: &ComputedStyle,
    longhand: ModeledLonghand,
) {
    longhand.copy_from(style, source);
    ensure_longhand_provenance(style);
    if let Some(source_owner) = source
        .longhand_provenance
        .sources
        .get(longhand.index())
        .copied()
    {
        Arc::make_mut(&mut style.longhand_provenance.sources)[longhand.index()] = source_owner;
    }
}

pub(crate) fn ensure_longhand_provenance(style: &mut ComputedStyle) {
    if style.longhand_provenance.sources.len() != ALL_MODELED_LONGHANDS.len() {
        style.longhand_provenance.sources = vec![0; ALL_MODELED_LONGHANDS.len()].into();
    }
    if style.longhand_provenance.current_source == 0 {
        style.longhand_provenance.current_source = 1;
    }
}

pub(crate) fn mark_modeled_longhand_specified(
    style: &mut ComputedStyle,
    longhand: ModeledLonghand,
) {
    ensure_longhand_provenance(style);
    Arc::make_mut(&mut style.longhand_provenance.sources)[longhand.index()] =
        style.longhand_provenance.current_source;
}

pub(crate) fn inherit_modeled_longhand_provenance(
    style: &mut ComputedStyle,
    inheritance_source: &ComputedStyle,
    longhand: ModeledLonghand,
) {
    ensure_longhand_provenance(style);
    Arc::make_mut(&mut style.longhand_provenance.sources)[longhand.index()] = inheritance_source
        .longhand_provenance
        .sources
        .get(longhand.index())
        .copied()
        .unwrap_or(0);
}

pub(crate) fn modeled_longhand_has_same_source(
    left: &ComputedStyle,
    right: &ComputedStyle,
    longhand: ModeledLonghand,
) -> bool {
    left.longhand_provenance
        .sources
        .get(longhand.index())
        .copied()
        .unwrap_or(0)
        == right
            .longhand_provenance
            .sources
            .get(longhand.index())
            .copied()
            .unwrap_or(0)
}

/// Implements the computed-state contract for each canonical longhand.
///
/// `ModeledLonghand::copy_from` is the typed, exhaustive dispatch boundary;
/// this legacy name-oriented body is kept temporarily to avoid duplicating the
/// large field-level copy table while individual value parsers remain keyed by
/// CSS spelling.
fn copy_modeled_longhand_by_css_name(
    style: &mut ComputedStyle,
    source: &ComputedStyle,
    name: &'static str,
) {
    match name {
        "color-scheme" => {
            style.color_scheme = source.color_scheme.clone();
            style.used_color_scheme = source.used_color_scheme;
            style.page_color_scheme = source.page_color_scheme;
        }
        "animation-name" => style.animation_snapshot.name = source.animation_snapshot.name.clone(),
        "animation-duration" => {
            style.animation_snapshot.duration_seconds = source.animation_snapshot.duration_seconds;
        }
        "animation-delay" => {
            style.animation_snapshot.delay_seconds = source.animation_snapshot.delay_seconds;
        }
        "zoom" => style.zoom = source.zoom,
        "display" => {
            style.display = source.display;
            style.legacy_webkit_box = source.legacy_webkit_box;
        }
        "-webkit-box-orient" => style.webkit_box_orient = source.webkit_box_orient,
        "flex-direction" => style.flex_direction = source.flex_direction,
        "justify-content" => style.justify_content = source.justify_content,
        "justify-items" => style.justify_items = source.justify_items,
        "justify-self" => style.justify_self = source.justify_self,
        "align-content" => style.align_content = source.align_content,
        "align-items" => style.align_items = source.align_items,
        "align-self" => style.align_self = source.align_self,
        "flex-wrap" => style.flex_wrap = source.flex_wrap,
        "flex-line-count" => style.flex_line_count = source.flex_line_count,
        "flex-grow" => style.flex_grow = source.flex_grow,
        "flex-shrink" => style.flex_shrink = source.flex_shrink,
        "flex-basis" => style.flex_basis = source.flex_basis.clone(),
        "order" => style.order = source.order,
        "row-gap" => style.row_gap = source.row_gap.clone(),
        "column-gap" => style.column_gap = source.column_gap.clone(),
        "row-rule-width" => style.row_rule.widths = source.row_rule.widths.clone(),
        "row-rule-style" => style.row_rule.styles = source.row_rule.styles.clone(),
        "row-rule-color" => style.row_rule.colors = source.row_rule.colors.clone(),
        "row-rule-break" => style.row_rule.rule_break = source.row_rule.rule_break,
        "row-rule-visibility-items" => {
            style.row_rule.visibility_items = source.row_rule.visibility_items;
        }
        "row-rule-inset-cap-start" => {
            style.row_rule.inset_cap_start = source.row_rule.inset_cap_start.clone();
        }
        "row-rule-inset-cap-end" => {
            style.row_rule.inset_cap_end = source.row_rule.inset_cap_end.clone();
        }
        "row-rule-inset-junction-start" => {
            style.row_rule.inset_junction_start = source.row_rule.inset_junction_start.clone();
        }
        "row-rule-inset-junction-end" => {
            style.row_rule.inset_junction_end = source.row_rule.inset_junction_end.clone();
        }
        "column-rule-width" => style.column_rule.widths = source.column_rule.widths.clone(),
        "column-rule-style" => style.column_rule.styles = source.column_rule.styles.clone(),
        "column-rule-color" => style.column_rule.colors = source.column_rule.colors.clone(),
        "column-rule-break" => style.column_rule.rule_break = source.column_rule.rule_break,
        "column-rule-visibility-items" => {
            style.column_rule.visibility_items = source.column_rule.visibility_items;
        }
        "column-rule-inset-cap-start" => {
            style.column_rule.inset_cap_start = source.column_rule.inset_cap_start.clone();
        }
        "column-rule-inset-cap-end" => {
            style.column_rule.inset_cap_end = source.column_rule.inset_cap_end.clone();
        }
        "column-rule-inset-junction-start" => {
            style.column_rule.inset_junction_start =
                source.column_rule.inset_junction_start.clone();
        }
        "column-rule-inset-junction-end" => {
            style.column_rule.inset_junction_end = source.column_rule.inset_junction_end.clone();
        }
        "rule-overlap" => style.rule_overlap = source.rule_overlap,
        "grid-template-rows" => style.grid_template_rows = source.grid_template_rows.clone(),
        "grid-template-columns" => {
            style.grid_template_columns = source.grid_template_columns.clone();
        }
        "grid-template-areas" => style.grid_template_areas = source.grid_template_areas.clone(),
        "grid-auto-rows" => style.grid_auto_rows = source.grid_auto_rows.clone(),
        "grid-auto-columns" => style.grid_auto_columns = source.grid_auto_columns.clone(),
        "grid-auto-flow" => style.grid_auto_flow = source.grid_auto_flow,
        "grid-lanes-direction" => style.grid_lanes_direction = source.grid_lanes_direction,
        "flow-tolerance" => {
            style.grid_lanes_flow_tolerance = source.grid_lanes_flow_tolerance.clone()
        }
        "grid-row-start" => style.grid_row_start = source.grid_row_start.clone(),
        "grid-row-end" => style.grid_row_end = source.grid_row_end.clone(),
        "grid-column-start" => style.grid_column_start = source.grid_column_start.clone(),
        "grid-column-end" => style.grid_column_end = source.grid_column_end.clone(),
        "column-count" => style.column_count = source.column_count,
        "column-width" => style.column_width = source.column_width.clone(),
        "column-height" => style.column_height = source.column_height.clone(),
        "column-wrap" => style.column_wrap = source.column_wrap,
        "column-fill" => style.column_fill = source.column_fill,
        "column-span" => style.column_span = source.column_span,
        "aspect-ratio" => style.aspect_ratio = source.aspect_ratio,
        "contain-intrinsic-size" => {
            style.contain_intrinsic_size = source.contain_intrinsic_size.clone();
        }
        "contain-intrinsic-width" => {
            style.contain_intrinsic_size.width = source.contain_intrinsic_size.width.clone();
        }
        "contain-intrinsic-height" => {
            style.contain_intrinsic_size.height = source.contain_intrinsic_size.height.clone();
        }
        "margin-trim" => style.margin_trim = source.margin_trim,
        "margin-top" => {
            style.box_values.margin.top = source.box_values.margin.top.clone();
            style.margin.top = source.margin.top;
            style.ua_margin_em.top = source.ua_margin_em.top;
        }
        "margin-right" => {
            style.box_values.margin.right = source.box_values.margin.right.clone();
            style.margin.right = source.margin.right;
            style.ua_margin_em.right = source.ua_margin_em.right;
        }
        "margin-bottom" => {
            style.box_values.margin.bottom = source.box_values.margin.bottom.clone();
            style.margin.bottom = source.margin.bottom;
            style.ua_margin_em.bottom = source.ua_margin_em.bottom;
        }
        "margin-left" => {
            style.box_values.margin.left = source.box_values.margin.left.clone();
            style.margin.left = source.margin.left;
            style.ua_margin_em.left = source.ua_margin_em.left;
        }
        "padding-top" => {
            style.box_values.padding.top = source.box_values.padding.top.clone();
            style.padding.top = source.padding.top;
        }
        "padding-right" => {
            style.box_values.padding.right = source.box_values.padding.right.clone();
            style.padding.right = source.padding.right;
        }
        "padding-bottom" => {
            style.box_values.padding.bottom = source.box_values.padding.bottom.clone();
            style.padding.bottom = source.padding.bottom;
        }
        "padding-left" => {
            style.box_values.padding.left = source.box_values.padding.left.clone();
            style.padding.left = source.padding.left;
        }
        "border-top-width" => set_border_side_width(
            style,
            BorderSide::Top,
            source.border_width_values.top.clone(),
        ),
        "border-right-width" => set_border_side_width(
            style,
            BorderSide::Right,
            source.border_width_values.right.clone(),
        ),
        "border-bottom-width" => set_border_side_width(
            style,
            BorderSide::Bottom,
            source.border_width_values.bottom.clone(),
        ),
        "border-left-width" => set_border_side_width(
            style,
            BorderSide::Left,
            source.border_width_values.left.clone(),
        ),
        "border-top-style" => style.border_styles.top = source.border_styles.top,
        "border-right-style" => style.border_styles.right = source.border_styles.right,
        "border-bottom-style" => style.border_styles.bottom = source.border_styles.bottom,
        "border-left-style" => style.border_styles.left = source.border_styles.left,
        "border-top-color" => {
            style.border_colors.top = source.border_colors.top;
            style.border_color = source.border_color;
        }
        "border-right-color" => style.border_colors.right = source.border_colors.right,
        "border-bottom-color" => style.border_colors.bottom = source.border_colors.bottom,
        "border-left-color" => style.border_colors.left = source.border_colors.left,
        "border-top-left-radius" => {
            style.border_radius.top_left = source.border_radius.top_left.clone()
        }
        "border-top-right-radius" => {
            style.border_radius.top_right = source.border_radius.top_right.clone();
        }
        "border-bottom-right-radius" => {
            style.border_radius.bottom_right = source.border_radius.bottom_right.clone();
        }
        "border-bottom-left-radius" => {
            style.border_radius.bottom_left = source.border_radius.bottom_left.clone();
        }
        "corner-top-left-shape" => style.corner_shapes.top_left = source.corner_shapes.top_left,
        "corner-top-right-shape" => style.corner_shapes.top_right = source.corner_shapes.top_right,
        "corner-bottom-right-shape" => {
            style.corner_shapes.bottom_right = source.corner_shapes.bottom_right;
        }
        "corner-bottom-left-shape" => {
            style.corner_shapes.bottom_left = source.corner_shapes.bottom_left;
        }
        "border-shape" => style.border_shape = source.border_shape.clone(),
        "shape-outside" => style.shape_outside = source.shape_outside.clone(),
        "shape-margin" => style.shape_margin = source.shape_margin.clone(),
        "shape-image-threshold" => style.shape_image_threshold = source.shape_image_threshold,
        "border-image-source" => {
            style.border_image.source = source.border_image.source.clone();
            style.border_image.source_base_url = source.border_image.source_base_url.clone();
            style.border_image.source_root_url = source.border_image.source_root_url.clone();
        }
        "border-image-slice" => style.border_image.slice = source.border_image.slice,
        "border-image-width" => style.border_image.width = source.border_image.width.clone(),
        "border-image-outset" => style.border_image.outset = source.border_image.outset.clone(),
        "border-image-repeat" => style.border_image.repeat = source.border_image.repeat,
        "border-collapse" => style.border_collapse = source.border_collapse,
        "caption-side" => style.caption_side = source.caption_side,
        "table-layout" => style.table_layout = source.table_layout,
        "empty-cells" => style.empty_cells = source.empty_cells,
        "border-spacing" => {
            style.border_spacing = source.border_spacing.clone();
        }
        "background-color" => {
            style.background.background_color = source.background.background_color.clone();
        }
        "background-image" => {
            style.background.background_image = source.background.background_image.clone();
            style.background.background_layers = source.background.background_layers.clone();
            style.background.background_image_layer_count =
                source.background.background_image_layer_count;
        }
        "background-size" => {
            style.background.background_size = source.background.background_size.clone();
            let source_layer_count = source.background.background_layers.len().max(1);
            for (index, layer) in style.background.background_layers.iter_mut().enumerate() {
                layer.size = source
                    .background
                    .background_layers
                    .get(index % source_layer_count)
                    .map(|layer| layer.size.clone())
                    .unwrap_or(source.background.background_size.clone());
            }
        }
        "object-fit" => style.object_fit = source.object_fit,
        "object-view-box" => style.object_view_box = source.object_view_box.clone(),
        "image-rendering" => style.image_rendering = source.image_rendering,
        "image-orientation" => style.image_orientation = source.image_orientation,
        "object-position" => style.object_position = source.object_position.clone(),
        "background-position" => {
            style.background.background_position = source.background.background_position.clone();
            let source_layer_count = source.background.background_layers.len().max(1);
            for (index, layer) in style.background.background_layers.iter_mut().enumerate() {
                layer.position = source
                    .background
                    .background_layers
                    .get(index % source_layer_count)
                    .map(|layer| layer.position.clone())
                    .unwrap_or(source.background.background_position.clone());
            }
        }
        "background-position-x" => {
            style.background.background_position.x =
                source.background.background_position.x.clone();
            let source_layer_count = source.background.background_layers.len().max(1);
            for (index, layer) in style.background.background_layers.iter_mut().enumerate() {
                layer.position.x = source
                    .background
                    .background_layers
                    .get(index % source_layer_count)
                    .map(|layer| layer.position.x.clone())
                    .unwrap_or(source.background.background_position.x.clone());
            }
        }
        "background-position-y" => {
            style.background.background_position.y =
                source.background.background_position.y.clone();
            let source_layer_count = source.background.background_layers.len().max(1);
            for (index, layer) in style.background.background_layers.iter_mut().enumerate() {
                layer.position.y = source
                    .background
                    .background_layers
                    .get(index % source_layer_count)
                    .map(|layer| layer.position.y.clone())
                    .unwrap_or(source.background.background_position.y.clone());
            }
        }
        "background-repeat" => {
            style.background.background_repeat = source.background.background_repeat;
            let source_layer_count = source.background.background_layers.len().max(1);
            for (index, layer) in style.background.background_layers.iter_mut().enumerate() {
                layer.repeat = source
                    .background
                    .background_layers
                    .get(index % source_layer_count)
                    .map(|layer| layer.repeat)
                    .unwrap_or(source.background.background_repeat);
            }
        }
        "background-attachment" => {
            style.background.background_attachment = source.background.background_attachment;
            let source_layer_count = source.background.background_layers.len().max(1);
            for (index, layer) in style.background.background_layers.iter_mut().enumerate() {
                layer.attachment = source
                    .background
                    .background_layers
                    .get(index % source_layer_count)
                    .map(|layer| layer.attachment)
                    .unwrap_or(source.background.background_attachment);
            }
        }
        "background-origin" => {
            style.background.background_origin = source.background.background_origin;
            let source_layer_count = source.background.background_layers.len().max(1);
            for (index, layer) in style.background.background_layers.iter_mut().enumerate() {
                layer.origin = source
                    .background
                    .background_layers
                    .get(index % source_layer_count)
                    .map(|layer| layer.origin)
                    .unwrap_or(source.background.background_origin);
            }
        }
        "background-clip" => {
            style.background.background_clip = source.background.background_clip;
            let source_layer_count = source.background.background_layers.len().max(1);
            for (index, layer) in style.background.background_layers.iter_mut().enumerate() {
                layer.clip = source
                    .background
                    .background_layers
                    .get(index % source_layer_count)
                    .map(|layer| layer.clip)
                    .unwrap_or(source.background.background_clip);
            }
        }
        "color" => style.color = source.color,
        "forced-color-adjust" => style.forced_color_adjust = source.forced_color_adjust,
        "fill" => {
            style.svg_fill = source.svg_fill;
        }
        "stroke" => {
            style.svg_stroke = source.svg_stroke;
        }
        "stroke-width" => {
            style.svg_stroke_width = source.svg_stroke_width.clone();
        }
        "flood-color" => style.svg_flood_color = source.svg_flood_color,
        "lighting-color" => style.svg_lighting_color = source.svg_lighting_color,
        "-webkit-text-fill-color" => style.text_fill_color = source.text_fill_color,
        "direction" => style.direction = source.direction,
        "unicode-bidi" => style.unicode_bidi = source.unicode_bidi,
        "writing-mode" => style.writing_mode = source.writing_mode,
        "text-orientation" => style.text_orientation = source.text_orientation,
        "text-combine-upright" => style.text_combine_upright = source.text_combine_upright,
        "font-size" => {
            style.font_size = source.font_size;
            style.deferred_font_size = DeferredFontSize::Inherit;
            project_line_height(style);
        }
        "font-size-adjust" => style.font_size_adjust = source.font_size_adjust,
        "font-synthesis" => style.font_synthesis = source.font_synthesis,
        "font-synthesis-weight" => style.font_synthesis.weight = source.font_synthesis.weight,
        "font-synthesis-style" => style.font_synthesis.style = source.font_synthesis.style,
        "font-synthesis-small-caps" => {
            style.font_synthesis.small_caps = source.font_synthesis.small_caps
        }
        "font-synthesis-position" => style.font_synthesis.position = source.font_synthesis.position,
        "line-height" => {
            style.line_height_value = source.line_height_value.clone();
            style.line_height = source.line_height;
        }
        "letter-spacing" => style.letter_spacing = source.letter_spacing.clone(),
        "word-spacing" => style.word_spacing = source.word_spacing.clone(),
        "width" => style.box_values.width = source.box_values.width.clone(),
        "height" => {
            style.box_values.height = source.box_values.height.clone();
        }
        "min-width" => style.box_values.min_width = source.box_values.min_width.clone(),
        "max-width" => style.box_values.max_width = source.box_values.max_width.clone(),
        "min-height" => style.box_values.min_height = source.box_values.min_height.clone(),
        "max-height" => style.box_values.max_height = source.box_values.max_height.clone(),
        "box-sizing" => style.box_sizing = source.box_sizing,
        "left" => style.box_values.inset_left = source.box_values.inset_left.clone(),
        "top" => style.box_values.inset_top = source.box_values.inset_top.clone(),
        "right" => style.box_values.inset_right = source.box_values.inset_right.clone(),
        "bottom" => style.box_values.inset_bottom = source.box_values.inset_bottom.clone(),
        "position" => style.position = source.position.clone(),
        "float" => style.float = source.float,
        "footnote-display" => style.footnote_display = source.footnote_display,
        "footnote-policy" => style.footnote_policy = source.footnote_policy,
        "clear" => style.clear = source.clear,
        "z-index" => style.z_index = source.z_index,
        "opacity" => style.opacity = source.opacity,
        "transform" => style.transform = source.transform.clone(),
        "translate" => {
            style.individual_transforms.translate = source.individual_transforms.translate.clone()
        }
        "rotate" => style.individual_transforms.rotate = source.individual_transforms.rotate,
        "scale" => style.individual_transforms.scale = source.individual_transforms.scale,
        "transform-origin" => style.transform_origin = source.transform_origin.clone(),
        "perspective" => style.perspective = source.perspective.clone(),
        "perspective-origin" => style.perspective_origin = source.perspective_origin.clone(),
        "transform-box" => style.transform_box = source.transform_box,
        "transform-style" => style.transform_style = source.transform_style,
        "backface-visibility" => style.backface_visibility = source.backface_visibility,
        "isolation" => style.isolation = source.isolation,
        "mix-blend-mode" => style.mix_blend_mode = source.mix_blend_mode,
        "filter" => style.filter = source.filter.clone(),
        "clip" => style.legacy_clip = source.legacy_clip.clone(),
        "clip-path" => style.clip_path = source.clip_path.clone(),
        "mask-image" => style.mask = source.mask.clone(),
        "mask-border-source" => style.mask_border_source = source.mask_border_source.clone(),
        "contain" => style.contain = source.contain,
        "container-type" => style.container_type = source.container_type,
        "container-name" => style.container_names = source.container_names.clone(),
        "content-visibility" => style.content_visibility = source.content_visibility,
        "will-change" => style.will_change = source.will_change,
        "text-align" | "text-align-all" => style.text_align = source.text_align,
        "text-align-last" => style.text_align_last = source.text_align_last,
        "text-justify" => style.text_justify = source.text_justify,
        "text-autospace" => style.text_autospace = source.text_autospace,
        "text-fit" => style.text_fit = source.text_fit,
        "text-spacing-trim" => style.text_spacing_trim = source.text_spacing_trim,
        "word-space-transform" => style.word_space_transform = source.word_space_transform,
        "initial-letter" => style.initial_letter = source.initial_letter,
        "initial-letter-align" => style.initial_letter_align = source.initial_letter_align,
        "initial-letter-wrap" => style.initial_letter_wrap = source.initial_letter_wrap.clone(),
        "line-fit-edge" => style.line_fit_edge = source.line_fit_edge,
        "text-box-trim" => style.text_box_trim = source.text_box_trim,
        "text-box-edge" => style.text_box_edge = source.text_box_edge,
        "box-decoration-break" => style.box_decoration_break = source.box_decoration_break,
        "outline-offset" => style.outline_offset = source.outline_offset.clone(),
        "outline-width" => {
            style.outline_width = source.outline_width;
            style.outline_width_value = source.outline_width_value.clone();
        }
        "outline-style" => style.outline_style = source.outline_style,
        "outline-color" => style.outline_color = source.outline_color,
        "text-indent" => style.text_indent = source.text_indent.clone(),
        "hanging-punctuation" => style.hanging_punctuation = source.hanging_punctuation,
        "vertical-align" => style.vertical_align = source.vertical_align.clone(),
        "dominant-baseline" => {
            style.vertical_align.dominant_baseline = source.vertical_align.dominant_baseline;
        }
        "alignment-baseline" => {
            style.vertical_align.alignment_baseline = source.vertical_align.alignment_baseline;
        }
        "baseline-source" => {
            style.vertical_align.baseline_source = source.vertical_align.baseline_source;
        }
        "baseline-shift" => {
            style.vertical_align.baseline_shift = source.vertical_align.baseline_shift.clone();
        }
        "font-weight" => style.font_weight = source.font_weight,
        "font-style" => style.font_style = source.font_style,
        "font-width" | "font-stretch" => style.font_width = source.font_width,
        "font-family" => style.font_family = source.font_family.clone(),
        "font-language-override" => {
            style.font_language_override = source.font_language_override;
        }
        "font-feature-settings" => {
            style.font_feature_settings = source.font_feature_settings.clone();
        }
        "font-variation-settings" => {
            style.font_variation_settings = source.font_variation_settings.clone();
        }
        "font-palette" => style.font_palette = source.font_palette.clone(),
        "font-kerning" => style.font_kerning = source.font_kerning,
        "font-variant" => {
            style.font_variant_ligatures = source.font_variant_ligatures;
            style.font_variant_position = source.font_variant_position;
            style.font_variant_caps = source.font_variant_caps;
            style.font_variant_numeric = source.font_variant_numeric.clone();
            style.font_variant_alternates = source.font_variant_alternates.clone();
            style.font_variant_east_asian = source.font_variant_east_asian.clone();
            style.font_variant_emoji = source.font_variant_emoji;
        }
        "font-variant-ligatures" => {
            style.font_variant_ligatures = source.font_variant_ligatures;
        }
        "font-variant-position" => style.font_variant_position = source.font_variant_position,
        "font-variant-caps" => style.font_variant_caps = source.font_variant_caps,
        "font-variant-numeric" => {
            style.font_variant_numeric = source.font_variant_numeric.clone();
        }
        "font-variant-alternates" => {
            style.font_variant_alternates = source.font_variant_alternates.clone();
        }
        "font-variant-east-asian" => {
            style.font_variant_east_asian = source.font_variant_east_asian.clone();
        }
        "font-variant-emoji" => style.font_variant_emoji = source.font_variant_emoji,
        "bookmark-level" => style.bookmark_level = source.bookmark_level,
        "bookmark-label" => style.bookmark_label = source.bookmark_label.clone(),
        "bookmark-state" => style.bookmark_state = source.bookmark_state,
        "text-transform" => style.text_transform = source.text_transform,
        "tab-size" => style.tab_size = source.tab_size.clone(),
        "visibility" => style.visibility = source.visibility,
        "list-style-type" => style.list_style_type = source.list_style_type.clone(),
        "list-style-position" => style.list_style_position = source.list_style_position,
        "list-style-image" => {
            style.list_style_image = source.list_style_image.clone();
        }
        "marker-side" => style.marker_side = source.marker_side,
        "counter-reset" => style.counter_resets = source.counter_resets.clone(),
        "counter-increment" => style.counter_increments = source.counter_increments.clone(),
        "counter-set" => style.counter_sets = source.counter_sets.clone(),
        "string-set" => style.string_sets = source.string_sets.clone(),
        "page" => {
            style.page = source.page.clone();
        }
        "break-before" | "page-break-before" => style.break_before = source.break_before,
        "break-after" | "page-break-after" => style.break_after = source.break_after,
        "break-inside" | "page-break-inside" => {
            style.break_inside = source.break_inside;
        }
        "orphans" => style.orphans = source.orphans,
        "widows" => style.widows = source.widows,
        "text-decoration-line" | "text-decoration" => {
            style.text_decoration.underline = source.text_decoration.underline;
            style.text_decoration.overline = source.text_decoration.overline;
            style.text_decoration.line_through = source.text_decoration.line_through;
            style.text_decoration.blink = source.text_decoration.blink;
            style.text_decoration.spelling_error = source.text_decoration.spelling_error;
            style.text_decoration.grammar_error = source.text_decoration.grammar_error;
        }
        "text-decoration-style" => style.text_decoration.style = source.text_decoration.style,
        "text-decoration-color" => style.text_decoration.color = source.text_decoration.color,
        "text-decoration-thickness" => {
            style.text_decoration.thickness = source.text_decoration.thickness.clone();
        }
        "text-decoration-inset" => {
            style.text_decoration.inset = source.text_decoration.inset.clone();
        }
        "text-decoration-skip-ink" => {
            style.text_decoration.skip_ink = source.text_decoration.skip_ink;
        }
        "text-decoration-skip-self" => {
            style.text_decoration.skip_self = source.text_decoration.skip_self;
        }
        "text-decoration-skip-box" => {
            style.text_decoration.skip_box = source.text_decoration.skip_box;
        }
        "text-decoration-skip-spaces" => {
            style.text_decoration.skip_spaces = source.text_decoration.skip_spaces;
        }
        "text-underline-offset" => {
            style.text_decoration.underline_offset =
                source.text_decoration.underline_offset.clone();
        }
        "text-underline-position" => {
            style.text_decoration.underline_position = source.text_decoration.underline_position;
        }
        "text-emphasis-style" => {
            style.text_emphasis_style = source.text_emphasis_style.clone();
        }
        "text-emphasis-color" => style.text_emphasis_color = source.text_emphasis_color,
        "text-emphasis-position" => {
            style.text_emphasis_position = source.text_emphasis_position;
        }
        "text-emphasis-skip" => style.text_emphasis_skip = source.text_emphasis_skip,
        "ruby-position" => style.ruby_position = source.ruby_position,
        "ruby-align" => style.ruby_align = source.ruby_align,
        "ruby-overhang" => style.ruby_overhang = source.ruby_overhang,
        "text-shadow" => style.text_shadow = source.text_shadow.clone(),
        "box-shadow" => style.box_shadow = source.box_shadow.clone(),
        "white-space" => {
            style.white_space = source.white_space;
            style.text_wrap_mode = source.text_wrap_mode;
        }
        "text-wrap" => {
            style.text_wrap_mode = source.text_wrap_mode;
            style.text_wrap_style = source.text_wrap_style;
        }
        "text-wrap-mode" => style.text_wrap_mode = source.text_wrap_mode,
        "text-wrap-style" => style.text_wrap_style = source.text_wrap_style,
        "wrap-inside" => style.wrap_inside = source.wrap_inside,
        "max-lines" => {
            style.max_lines = source.max_lines;
            style.line_limit_traversal = None;
        }
        "block-ellipsis" => {
            style.block_ellipsis = source.block_ellipsis.clone();
            style.line_limit_traversal = None;
        }
        "continue" => {
            style.continue_ = source.continue_;
            style.line_limit_traversal = None;
        }
        "word-break" => style.word_break = source.word_break,
        "overflow-x" => style.overflow_x = source.overflow_x,
        "overflow-y" => style.overflow_y = source.overflow_y,
        "scrollbar-gutter" => style.scrollbar_gutter = source.scrollbar_gutter,
        "scrollbar-width" => style.scrollbar_width = source.scrollbar_width,
        "scroll-snap-type" => style.scroll_snap_type = source.scroll_snap_type,
        "scroll-snap-align" => style.scroll_snap_align = source.scroll_snap_align,
        "scroll-snap-stop" => style.scroll_snap_stop = source.scroll_snap_stop,
        "scroll-target-group" => style.scroll_target_group = source.scroll_target_group,
        "scroll-marker-group" => style.scroll_marker_group = source.scroll_marker_group,
        "scroll-padding-top" => style.scroll_padding.top = source.scroll_padding.top.clone(),
        "scroll-padding-right" => style.scroll_padding.right = source.scroll_padding.right.clone(),
        "scroll-padding-bottom" => {
            style.scroll_padding.bottom = source.scroll_padding.bottom.clone()
        }
        "scroll-padding-left" => style.scroll_padding.left = source.scroll_padding.left.clone(),
        "scroll-margin-top" => style.scroll_margin.top = source.scroll_margin.top.clone(),
        "scroll-margin-right" => style.scroll_margin.right = source.scroll_margin.right.clone(),
        "scroll-margin-bottom" => style.scroll_margin.bottom = source.scroll_margin.bottom.clone(),
        "scroll-margin-left" => style.scroll_margin.left = source.scroll_margin.left.clone(),
        "overflow-clip-margin" => style.overflow_clip_margin = source.overflow_clip_margin,
        "overflow-wrap" | "word-wrap" => style.overflow_wrap = source.overflow_wrap,
        "line-break" => style.line_break = source.line_break,
        "hyphens" => style.hyphens = source.hyphens,
        "hyphenate-character" => style.hyphenate_character = source.hyphenate_character.clone(),
        "hyphenate-limit-chars" => {
            style.hyphenate_limit_chars = source.hyphenate_limit_chars;
        }
        "content" => {
            style.content = source.content.clone();
            style.marker_content = source.marker_content.clone();
        }
        "quotes" => style.quotes = source.quotes.clone().inherited(),
        name => unreachable!("modeled longhand `{name}` has no computed-style copy operation"),
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;

    #[test]
    fn modeled_longhand_registry_is_unique_and_copy_complete() {
        let mut names = HashSet::new();
        let source = ComputedStyle::initial();
        let mut destination = ComputedStyle::initial();

        for &longhand in ALL_MODELED_LONGHANDS {
            let name = longhand.css_name();
            assert!(names.insert(name), "duplicate modeled longhand `{name}`");
            let parsed = ModeledLonghand::parse(name)
                .expect("the modeled-longhand registry must parse its own entry");
            assert_eq!(parsed, longhand);
            copy_modeled_longhand(&mut destination, &source, longhand);
        }
    }

    #[test]
    fn every_first_line_longhand_uses_the_exhaustive_computed_style_copier() {
        let source = ComputedStyle::initial();
        let mut destination = ComputedStyle::initial();
        let mut allowed = ModeledLonghandSet::empty();

        for &longhand in ALL_MODELED_LONGHANDS {
            if longhand.is_first_line_allowed() {
                allowed.insert(longhand);
                copy_modeled_longhand(&mut destination, &source, longhand);
            }
        }

        for longhand in allowed.iter() {
            assert!(longhand.is_first_line_allowed(), "{}", longhand.css_name());
        }
        for name in [
            "font-synthesis",
            "font-synthesis-weight",
            "font-synthesis-style",
            "font-synthesis-small-caps",
            "font-synthesis-position",
        ] {
            assert!(
                allowed.contains(ModeledLonghand::parse(name).expect("modeled font longhand")),
                "{name}",
            );
        }
        assert!(!allowed.contains(ModeledLonghand::Display));
    }

    #[test]
    fn closed_property_syntax_round_trips_and_always_resolves_to_longhands() {
        let mut names = HashSet::new();
        for shorthand in ALL_MODELED_SHORTHANDS {
            let property = ModeledProperty::Shorthand(*shorthand);
            let name = property.css_name();
            assert!(names.insert(name), "duplicate modeled syntax `{name}`");
            assert_eq!(ModeledProperty::parse(name), Some(property));
            for writing_mode in [WritingMode::HorizontalTb, WritingMode::VerticalRl] {
                assert!(
                    property
                        .resolve_targets(Direction::Ltr, writing_mode)
                        .into_iter()
                        .next()
                        .is_some()
                );
            }
        }
        for logical in ALL_LOGICAL_PROPERTIES {
            let property = ModeledProperty::Logical(*logical);
            let name = property.css_name();
            assert!(names.insert(name), "duplicate modeled syntax `{name}`");
            assert_eq!(ModeledProperty::parse(name), Some(property));
            for writing_mode in [WritingMode::HorizontalTb, WritingMode::VerticalRl] {
                assert!(
                    property
                        .resolve_targets(Direction::Ltr, writing_mode)
                        .into_iter()
                        .next()
                        .is_some()
                );
            }
        }
        for alias in ALL_LEGACY_PROPERTY_ALIASES {
            let property = ModeledProperty::Alias(*alias);
            let name = property.css_name();
            assert!(names.insert(name), "duplicate modeled syntax `{name}`");
            assert_eq!(ModeledProperty::parse(name), Some(property));
            assert!(
                property
                    .resolve_targets(Direction::Ltr, WritingMode::HorizontalTb)
                    .into_iter()
                    .next()
                    .is_some()
            );
        }
    }

    #[test]
    fn typed_property_syntax_resolves_to_canonical_longhand_targets() {
        let width = ModeledProperty::parse("width").expect("modeled longhand");
        assert!(matches!(
            width,
            ModeledProperty::Longhand(ModeledLonghand::Width)
        ));
        assert_eq!(
            width
                .resolve_targets(Direction::Ltr, WritingMode::HorizontalTb)
                .into_iter()
                .collect::<Vec<_>>(),
            vec![ModeledLonghand::Width]
        );

        let margin = ModeledProperty::parse("margin").expect("modeled shorthand");
        assert!(matches!(margin, ModeledProperty::Shorthand(_)));
        assert_eq!(
            margin
                .resolve_targets(Direction::Ltr, WritingMode::HorizontalTb)
                .into_iter()
                .collect::<Vec<_>>(),
            vec![
                ModeledLonghand::MarginTop,
                ModeledLonghand::MarginRight,
                ModeledLonghand::MarginBottom,
                ModeledLonghand::MarginLeft,
            ]
        );

        let alias = ModeledProperty::parse("word-wrap").expect("legacy alias");
        assert!(matches!(alias, ModeledProperty::Alias(_)));
        assert_eq!(
            alias
                .resolve_targets(Direction::Ltr, WritingMode::HorizontalTb)
                .into_iter()
                .collect::<Vec<_>>(),
            vec![ModeledLonghand::OverflowWrap]
        );

        let font_stretch = ModeledProperty::parse("font-stretch").expect("font-width alias");
        assert!(matches!(font_stretch, ModeledProperty::Alias(_)));
        assert_eq!(
            font_stretch
                .resolve_targets(Direction::Ltr, WritingMode::HorizontalTb)
                .into_iter()
                .collect::<Vec<_>>(),
            vec![ModeledLonghand::FontWidth]
        );

        let font = ModeledProperty::parse("font").expect("font shorthand");
        assert!(matches!(
            font,
            ModeledProperty::Shorthand(ModeledShorthand::Font)
        ));
        let font_targets = font
            .resolve_targets(Direction::Ltr, WritingMode::HorizontalTb)
            .into_iter()
            .collect::<Vec<_>>();
        assert!(font_targets.contains(&ModeledLonghand::FontSize));
        assert!(font_targets.contains(&ModeledLonghand::FontFeatureSettings));
        assert!(font_targets.contains(&ModeledLonghand::FontVariationSettings));

        let logical = ModeledProperty::parse("block-size").expect("logical property");
        assert!(matches!(logical, ModeledProperty::Logical(_)));
        assert_eq!(
            logical
                .resolve_targets(Direction::Ltr, WritingMode::HorizontalTb)
                .into_iter()
                .collect::<Vec<_>>(),
            vec![ModeledLonghand::Height]
        );
        assert_eq!(
            logical
                .resolve_targets(Direction::Ltr, WritingMode::VerticalRl)
                .into_iter()
                .collect::<Vec<_>>(),
            vec![ModeledLonghand::Width]
        );

        let containment = ModeledProperty::parse("contain-intrinsic-inline-size")
            .expect("logical containment property");
        assert_eq!(
            containment
                .resolve_targets(Direction::Ltr, WritingMode::VerticalRl)
                .into_iter()
                .collect::<Vec<_>>(),
            vec![ModeledLonghand::ContainIntrinsicHeight]
        );
    }

    #[test]
    fn all_targets_every_modeled_longhand_except_direction_and_unicode_bidi() {
        let all = ModeledProperty::parse("all").expect("all shorthand");
        let targets = all
            .resolve_targets(Direction::Ltr, WritingMode::HorizontalTb)
            .into_iter()
            .collect::<Vec<_>>();
        assert!(targets.contains(&ModeledLonghand::AspectRatio));
        assert!(!targets.contains(&ModeledLonghand::Direction));
        assert!(!targets.contains(&ModeledLonghand::UnicodeBidi));
    }
}

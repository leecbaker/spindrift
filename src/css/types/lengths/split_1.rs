use super::*;

pub(crate) const ROOT_FONT_SIZE_PT: f32 = 12.0;

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum AbsoluteLengthUnit {
    Px,
    Pt,
    In,
    Cm,
    Mm,
    Q,
    Pc,
    NumberPt,
}

impl AbsoluteLengthUnit {
    pub(crate) fn length_for_value(self, value: f32) -> LayoutLength {
        let points_per_unit = match self {
            Self::Px => CSS_PX_TO_PT,
            Self::Pt | Self::NumberPt => 1.0,
            Self::In => 72.0,
            Self::Cm => 72.0 / 2.54,
            Self::Mm => 72.0 / 25.4,
            Self::Q => 72.0 / 25.4 / 4.0,
            Self::Pc => 12.0,
        };
        layout_pt(value * points_per_unit)
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum SpecifiedLength {
    Absolute {
        value: f32,
        unit: AbsoluteLengthUnit,
    },
    FontRelativeEm(f32),
    FontRelativeCh(f32),
    RootFontRelativeRem(f32),
}

impl SpecifiedLength {
    // CSS Cascade 5 value processing resolves font-relative specified lengths
    // when computed values are produced; layout later turns computed
    // length-percentages into used values against a containing block.
    // https://www.w3.org/TR/css-cascade-5/#computed
    // https://www.w3.org/TR/css-values-4/#font-relative-lengths
    pub(crate) fn to_computed(self, font_size: f32, root_font_size: f32) -> ComputedLength {
        let length = match self {
            Self::Absolute { value, unit } => unit.length_for_value(value),
            Self::FontRelativeEm(value) => layout_pt(value * font_size),
            Self::FontRelativeCh(value) => layout_pt(value * font_size),
            Self::RootFontRelativeRem(value) => layout_pt(value * root_font_size),
        };
        ComputedLength { length }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct ComputedLength {
    pub length: LayoutLength,
}

/// Computed CSS `<length-percentage>` value, preserving the percentage
/// component until a property-specific used-value basis is available.
///
/// CSS Values and Units Level 4 defines mixed `<length-percentage>` values and
/// their later percentage resolution:
/// <https://www.w3.org/TR/css-values-4/#mixed-percentages>.
#[derive(Debug, Clone, Copy)]
pub(crate) struct ComputedLengthPercentage {
    pub length: LayoutLength,
    pub percent: f32,
    pub has_percentage: bool,
    pub ch: f32,
    pub vw: f32,
    pub vh: f32,
    pub vmin: f32,
    pub vmax: f32,
    pub vi: f32,
    pub vb: f32,
    pub math: Option<DeferredLengthPercentageMathId>,
}

impl PartialEq for ComputedLengthPercentage {
    fn eq(&self, other: &Self) -> bool {
        self.length == other.length
            && self.percent == other.percent
            && self.ch == other.ch
            && self.vw == other.vw
            && self.vh == other.vh
            && self.vmin == other.vmin
            && self.vmax == other.vmax
            && self.vi == other.vi
            && self.vb == other.vb
            && self.deferred_math() == other.deferred_math()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct DeferredLengthPercentageMathId(NonZeroU32);

pub(in crate::css) fn deferred_math_store() -> &'static Mutex<Vec<DeferredLengthPercentageMath>> {
    static STORE: OnceLock<Mutex<Vec<DeferredLengthPercentageMath>>> = OnceLock::new();
    STORE.get_or_init(|| Mutex::new(Vec::new()))
}

pub(in crate::css) fn store_deferred_math(
    math: DeferredLengthPercentageMath,
) -> DeferredLengthPercentageMathId {
    let mut store = deferred_math_store()
        .lock()
        .expect("deferred length math store should not be poisoned");
    store.push(math);
    let id = u32::try_from(store.len()).expect("deferred length math store exhausted");
    DeferredLengthPercentageMathId(NonZeroU32::new(id).expect("store ids are one-based"))
}

pub(in crate::css) fn load_deferred_math(
    id: DeferredLengthPercentageMathId,
) -> Option<DeferredLengthPercentageMath> {
    let store = deferred_math_store().lock().ok()?;
    store
        .get(usize::try_from(id.0.get()).ok()?.checked_sub(1)?)
        .copied()
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum DeferredLengthPercentageMath {
    Sum(LengthPercentageExpression, LengthPercentageExpression),
    Product(LengthPercentageExpression, f32),
    Min(LengthPercentageExpression, LengthPercentageExpression),
    Max(LengthPercentageExpression, LengthPercentageExpression),
    Clamp {
        min: LengthPercentageExpression,
        center: LengthPercentageExpression,
        max: LengthPercentageExpression,
    },
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum LengthPercentageExpression {
    Components(LengthPercentageComponents),
    Math(DeferredLengthPercentageMathId),
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct LengthPercentageComponents {
    pub length: LayoutLength,
    pub percent: f32,
    pub has_percentage: bool,
    pub ch: f32,
    pub vw: f32,
    pub vh: f32,
    pub vmin: f32,
    pub vmax: f32,
    pub vi: f32,
    pub vb: f32,
}

impl ComputedLengthPercentage {
    pub(crate) const ZERO: Self = Self {
        length: layout_pt(0.0),
        percent: 0.0,
        has_percentage: false,
        ch: 0.0,
        vw: 0.0,
        vh: 0.0,
        vmin: 0.0,
        vmax: 0.0,
        vi: 0.0,
        vb: 0.0,
        math: None,
    };

    pub(crate) fn from_points(points: f32) -> Self {
        Self::from_layout_length(layout_pt(points))
    }

    pub(crate) fn from_layout_length(length: LayoutLength) -> Self {
        Self {
            length,
            percent: 0.0,
            has_percentage: false,
            ch: 0.0,
            vw: 0.0,
            vh: 0.0,
            vmin: 0.0,
            vmax: 0.0,
            vi: 0.0,
            vb: 0.0,
            math: None,
        }
    }

    pub(crate) fn from_percent(percent: f32) -> Self {
        Self {
            length: layout_pt(0.0),
            percent,
            has_percentage: true,
            ch: 0.0,
            vw: 0.0,
            vh: 0.0,
            vmin: 0.0,
            vmax: 0.0,
            vi: 0.0,
            vb: 0.0,
            math: None,
        }
    }

    pub(crate) fn from_ch(ch: f32) -> Self {
        Self {
            length: layout_pt(0.0),
            percent: 0.0,
            has_percentage: false,
            ch,
            vw: 0.0,
            vh: 0.0,
            vmin: 0.0,
            vmax: 0.0,
            vi: 0.0,
            vb: 0.0,
            math: None,
        }
    }

    pub(crate) fn from_vw(vw: f32) -> Self {
        Self {
            length: layout_pt(0.0),
            percent: 0.0,
            has_percentage: false,
            ch: 0.0,
            vw,
            vh: 0.0,
            vmin: 0.0,
            vmax: 0.0,
            vi: 0.0,
            vb: 0.0,
            math: None,
        }
    }

    pub(crate) fn from_vh(vh: f32) -> Self {
        Self {
            length: layout_pt(0.0),
            percent: 0.0,
            has_percentage: false,
            ch: 0.0,
            vw: 0.0,
            vh,
            vmin: 0.0,
            vmax: 0.0,
            vi: 0.0,
            vb: 0.0,
            math: None,
        }
    }

    pub(crate) fn from_vmin(vmin: f32) -> Self {
        Self {
            length: layout_pt(0.0),
            percent: 0.0,
            has_percentage: false,
            ch: 0.0,
            vw: 0.0,
            vh: 0.0,
            vmin,
            vmax: 0.0,
            vi: 0.0,
            vb: 0.0,
            math: None,
        }
    }

    pub(crate) fn from_vmax(vmax: f32) -> Self {
        Self {
            length: layout_pt(0.0),
            percent: 0.0,
            has_percentage: false,
            ch: 0.0,
            vw: 0.0,
            vh: 0.0,
            vmin: 0.0,
            vmax,
            vi: 0.0,
            vb: 0.0,
            math: None,
        }
    }

    pub(crate) fn from_vi(vi: f32) -> Self {
        Self {
            length: layout_pt(0.0),
            percent: 0.0,
            has_percentage: false,
            ch: 0.0,
            vw: 0.0,
            vh: 0.0,
            vmin: 0.0,
            vmax: 0.0,
            vi,
            vb: 0.0,
            math: None,
        }
    }

    pub(crate) fn from_vb(vb: f32) -> Self {
        Self {
            length: layout_pt(0.0),
            percent: 0.0,
            has_percentage: false,
            ch: 0.0,
            vw: 0.0,
            vh: 0.0,
            vmin: 0.0,
            vmax: 0.0,
            vi: 0.0,
            vb,
            math: None,
        }
    }

    pub(crate) fn from_deferred_math(math: DeferredLengthPercentageMath) -> Self {
        Self {
            math: Some(store_deferred_math(math)),
            ..Self::ZERO
        }
    }

    pub(crate) fn length_points(self) -> f32 {
        layout_points(self.length)
    }

    pub(crate) fn length_points_max_zero(self) -> f32 {
        self.length_points().max(0.0)
    }

    pub(crate) fn length_is_zero(self) -> bool {
        self.length_points() == 0.0
    }

    pub(crate) fn length_with_percentage_basis(self, basis: f32) -> f32 {
        self.length_points() + self.percent * basis
    }

    pub(crate) fn from_expression(expression: LengthPercentageExpression) -> Self {
        match expression {
            LengthPercentageExpression::Components(components) => Self::from_components(components),
            LengthPercentageExpression::Math(id) => Self {
                math: Some(id),
                ..Self::ZERO
            },
        }
    }

    pub(crate) fn components(self) -> Option<LengthPercentageComponents> {
        self.math.is_none().then_some(LengthPercentageComponents {
            length: self.length,
            percent: self.percent,
            has_percentage: self.has_percentage,
            ch: self.ch,
            vw: self.vw,
            vh: self.vh,
            vmin: self.vmin,
            vmax: self.vmax,
            vi: self.vi,
            vb: self.vb,
        })
    }

    pub(crate) fn expression(self) -> LengthPercentageExpression {
        self.math
            .map(LengthPercentageExpression::Math)
            .unwrap_or_else(|| {
                LengthPercentageExpression::Components(LengthPercentageComponents {
                    length: self.length,
                    percent: self.percent,
                    has_percentage: self.has_percentage,
                    ch: self.ch,
                    vw: self.vw,
                    vh: self.vh,
                    vmin: self.vmin,
                    vmax: self.vmax,
                    vi: self.vi,
                    vb: self.vb,
                })
            })
    }

    pub(in crate::css) fn deferred_math(self) -> Option<DeferredLengthPercentageMath> {
        self.math.and_then(load_deferred_math)
    }

    pub(in crate::css) fn from_components(components: LengthPercentageComponents) -> Self {
        Self {
            length: components.length,
            percent: components.percent,
            has_percentage: components.has_percentage,
            ch: components.ch,
            vw: components.vw,
            vh: components.vh,
            vmin: components.vmin,
            vmax: components.vmax,
            vi: components.vi,
            vb: components.vb,
            math: None,
        }
    }

    pub(crate) fn length_if_no_percent(self) -> Option<f32> {
        (self.math.is_none()
            && self.percent == 0.0
            && !self.has_percentage
            && self.ch == 0.0
            && self.vw == 0.0
            && self.vh == 0.0
            && self.vmin == 0.0
            && self.vmax == 0.0
            && self.vi == 0.0
            && self.vb == 0.0)
            .then_some(layout_points(self.length))
    }

    /// Resolves this computed `<length-percentage>` against a used percentage basis.
    ///
    /// CSS math comparisons that contain percentages cannot pick their branch
    /// at computed-value time. Once the property-specific percentage basis is
    /// known, the deferred expression can be evaluated to a used length:
    /// <https://www.w3.org/TR/css-values-4/#math>.
    pub(crate) fn used_length_with_percentage_basis(self, percentage_basis: f32) -> Option<f32> {
        if let Some(id) = self.math {
            return load_deferred_math(id)?.evaluate_used_length(percentage_basis);
        }
        LengthPercentageComponents {
            length: self.length,
            percent: self.percent,
            has_percentage: self.has_percentage,
            ch: self.ch,
            vw: self.vw,
            vh: self.vh,
            vmin: self.vmin,
            vmax: self.vmax,
            vi: self.vi,
            vb: self.vb,
        }
        .used_length_with_percentage_basis(percentage_basis)
    }

    /// Resolves font-metric-relative `ch` components into absolute lengths.
    ///
    /// CSS Values defines `ch` as the advance of the "0" glyph in the used
    /// font. Reasyprint keeps `ch` separate through computed values, then
    /// folds it into the absolute length component once layout has resolved the
    /// actual font face:
    /// <https://www.w3.org/TR/css-values-4/#font-relative-lengths>.
    pub(crate) fn resolve_font_metric_lengths(&mut self, ch_advance: f32) {
        if let Some(id) = self.math {
            let Some(mut math) = load_deferred_math(id) else {
                return;
            };
            math.resolve_font_metric_lengths(ch_advance);
            if let Some(value) = math.evaluate() {
                *self = Self::from_components(value);
            } else {
                self.math = Some(store_deferred_math(math));
            }
            return;
        }
        if self.ch != 0.0 {
            self.length += layout_pt(self.ch * ch_advance);
            self.ch = 0.0;
        }
    }

    /// Resolves viewport-percentage components against the page area.
    ///
    /// CSS Values defines `vw`, `vh`, `vmin`, and `vmax` as percentages of the
    /// initial containing block. In paged media, CSS Paged Media defines the
    /// document canvas/initial containing block from the page area, so layout
    /// resolves these units once the page context is known:
    /// <https://www.w3.org/TR/css-values-4/#viewport-relative-lengths> and
    /// <https://www.w3.org/TR/css-page-3/#page-model>.
    pub(crate) fn resolve_viewport_lengths(
        &mut self,
        viewport_width: f32,
        viewport_height: f32,
        viewport_inline: f32,
        viewport_block: f32,
    ) {
        if let Some(id) = self.math {
            let Some(mut math) = load_deferred_math(id) else {
                return;
            };
            math.resolve_viewport_lengths(
                viewport_width,
                viewport_height,
                viewport_inline,
                viewport_block,
            );
            if let Some(value) = math.evaluate() {
                *self = Self::from_components(value);
            } else {
                self.math = Some(store_deferred_math(math));
            }
            return;
        }
        let viewport_min = viewport_width.min(viewport_height);
        let viewport_max = viewport_width.max(viewport_height);
        self.length += layout_pt(
            self.vw * viewport_width / 100.0
                + self.vh * viewport_height / 100.0
                + self.vmin * viewport_min / 100.0
                + self.vmax * viewport_max / 100.0
                + self.vi * viewport_inline / 100.0
                + self.vb * viewport_block / 100.0,
        );
        self.vw = 0.0;
        self.vh = 0.0;
        self.vmin = 0.0;
        self.vmax = 0.0;
        self.vi = 0.0;
        self.vb = 0.0;
    }
}

impl DeferredLengthPercentageMath {
    pub(in crate::css) fn resolve_font_metric_lengths(&mut self, ch_advance: f32) {
        match self {
            Self::Sum(left, right) | Self::Min(left, right) | Self::Max(left, right) => {
                left.resolve_font_metric_lengths(ch_advance);
                right.resolve_font_metric_lengths(ch_advance);
            }
            Self::Product(value, _) => {
                value.resolve_font_metric_lengths(ch_advance);
            }
            Self::Clamp { min, center, max } => {
                min.resolve_font_metric_lengths(ch_advance);
                center.resolve_font_metric_lengths(ch_advance);
                max.resolve_font_metric_lengths(ch_advance);
            }
        }
    }

    pub(in crate::css) fn evaluate(self) -> Option<LengthPercentageComponents> {
        match self {
            Self::Sum(left, right) => Some(left.evaluate()?.add(right.evaluate()?)),
            Self::Product(value, factor) => Some(value.evaluate()?.mul(factor)),
            Self::Min(left, right) => {
                compare_length_percentage_components(&[left.evaluate()?, right.evaluate()?], false)
            }
            Self::Max(left, right) => {
                compare_length_percentage_components(&[left.evaluate()?, right.evaluate()?], true)
            }
            Self::Clamp { min, center, max } => {
                let below_max = compare_length_percentage_components(
                    &[center.evaluate()?, max.evaluate()?],
                    false,
                )?;
                compare_length_percentage_components(&[below_max, min.evaluate()?], true)
            }
        }
    }

    pub(in crate::css) fn resolve_viewport_lengths(
        &mut self,
        viewport_width: f32,
        viewport_height: f32,
        viewport_inline: f32,
        viewport_block: f32,
    ) {
        match self {
            Self::Sum(left, right) | Self::Min(left, right) | Self::Max(left, right) => {
                left.resolve_viewport_lengths(
                    viewport_width,
                    viewport_height,
                    viewport_inline,
                    viewport_block,
                );
                right.resolve_viewport_lengths(
                    viewport_width,
                    viewport_height,
                    viewport_inline,
                    viewport_block,
                );
            }
            Self::Product(value, _) => {
                value.resolve_viewport_lengths(
                    viewport_width,
                    viewport_height,
                    viewport_inline,
                    viewport_block,
                );
            }
            Self::Clamp { min, center, max } => {
                min.resolve_viewport_lengths(
                    viewport_width,
                    viewport_height,
                    viewport_inline,
                    viewport_block,
                );
                center.resolve_viewport_lengths(
                    viewport_width,
                    viewport_height,
                    viewport_inline,
                    viewport_block,
                );
                max.resolve_viewport_lengths(
                    viewport_width,
                    viewport_height,
                    viewport_inline,
                    viewport_block,
                );
            }
        }
    }

    pub(crate) fn depends_on_metric_or_percent(self) -> Option<bool> {
        match self {
            Self::Sum(left, right) | Self::Min(left, right) | Self::Max(left, right) => {
                Some(left.depends_on_metric_or_percent()? || right.depends_on_metric_or_percent()?)
            }
            Self::Product(value, _) => value.depends_on_metric_or_percent(),
            Self::Clamp { min, center, max } => Some(
                min.depends_on_metric_or_percent()?
                    || center.depends_on_metric_or_percent()?
                    || max.depends_on_metric_or_percent()?,
            ),
        }
    }

    pub(in crate::css) fn evaluate_used_length(self, percentage_basis: f32) -> Option<f32> {
        match self {
            Self::Sum(left, right) => Some(
                left.evaluate_used_length(percentage_basis)?
                    + right.evaluate_used_length(percentage_basis)?,
            ),
            Self::Product(value, factor) => {
                Some(value.evaluate_used_length(percentage_basis)? * factor)
            }
            Self::Min(left, right) => Some(
                left.evaluate_used_length(percentage_basis)?
                    .min(right.evaluate_used_length(percentage_basis)?),
            ),
            Self::Max(left, right) => Some(
                left.evaluate_used_length(percentage_basis)?
                    .max(right.evaluate_used_length(percentage_basis)?),
            ),
            Self::Clamp { min, center, max } => Some(
                center
                    .evaluate_used_length(percentage_basis)?
                    .min(max.evaluate_used_length(percentage_basis)?)
                    .max(min.evaluate_used_length(percentage_basis)?),
            ),
        }
    }
}

impl LengthPercentageExpression {
    pub(in crate::css) fn evaluate(self) -> Option<LengthPercentageComponents> {
        match self {
            Self::Components(value) => Some(value),
            Self::Math(id) => load_deferred_math(id)?.evaluate(),
        }
    }

    pub(in crate::css) fn resolve_font_metric_lengths(&mut self, ch_advance: f32) {
        match self {
            Self::Components(value) => value.resolve_font_metric_lengths(ch_advance),
            Self::Math(id) => {
                let Some(mut math) = load_deferred_math(*id) else {
                    return;
                };
                math.resolve_font_metric_lengths(ch_advance);
                if let Some(value) = math.evaluate() {
                    *self = Self::Components(value);
                } else {
                    *id = store_deferred_math(math);
                }
            }
        }
    }

    pub(in crate::css) fn resolve_viewport_lengths(
        &mut self,
        viewport_width: f32,
        viewport_height: f32,
        viewport_inline: f32,
        viewport_block: f32,
    ) {
        match self {
            Self::Components(value) => value.resolve_viewport_lengths(
                viewport_width,
                viewport_height,
                viewport_inline,
                viewport_block,
            ),
            Self::Math(id) => {
                let Some(mut math) = load_deferred_math(*id) else {
                    return;
                };
                math.resolve_viewport_lengths(
                    viewport_width,
                    viewport_height,
                    viewport_inline,
                    viewport_block,
                );
                if let Some(value) = math.evaluate() {
                    *self = Self::Components(value);
                } else {
                    *id = store_deferred_math(math);
                }
            }
        }
    }

    pub(crate) fn depends_on_metric_or_percent(self) -> Option<bool> {
        match self {
            Self::Components(value) => Some(value.depends_on_metric_or_percent()),
            Self::Math(id) => load_deferred_math(id)?.depends_on_metric_or_percent(),
        }
    }

    pub(in crate::css) fn evaluate_used_length(self, percentage_basis: f32) -> Option<f32> {
        match self {
            Self::Components(value) => value.used_length_with_percentage_basis(percentage_basis),
            Self::Math(id) => load_deferred_math(id)?.evaluate_used_length(percentage_basis),
        }
    }
}

impl LengthPercentageComponents {
    pub(in crate::css) fn add(self, other: Self) -> Self {
        Self {
            length: self.length + other.length,
            percent: self.percent + other.percent,
            has_percentage: self.has_percentage || other.has_percentage,
            ch: self.ch + other.ch,
            vw: self.vw + other.vw,
            vh: self.vh + other.vh,
            vmin: self.vmin + other.vmin,
            vmax: self.vmax + other.vmax,
            vi: self.vi + other.vi,
            vb: self.vb + other.vb,
        }
    }

    pub(in crate::css) fn mul(self, factor: f32) -> Self {
        Self {
            length: self.length * factor,
            percent: self.percent * factor,
            has_percentage: self.has_percentage,
            ch: self.ch * factor,
            vw: self.vw * factor,
            vh: self.vh * factor,
            vmin: self.vmin * factor,
            vmax: self.vmax * factor,
            vi: self.vi * factor,
            vb: self.vb * factor,
        }
    }

    pub(in crate::css) fn depends_on_metric_or_percent(self) -> bool {
        self.percent != 0.0
            || self.has_percentage
            || self.ch != 0.0
            || self.vw != 0.0
            || self.vh != 0.0
            || self.vmin != 0.0
            || self.vmax != 0.0
            || self.vi != 0.0
            || self.vb != 0.0
    }

    pub(in crate::css) fn used_length_with_percentage_basis(
        self,
        percentage_basis: f32,
    ) -> Option<f32> {
        (self.ch == 0.0
            && self.vw == 0.0
            && self.vh == 0.0
            && self.vmin == 0.0
            && self.vmax == 0.0
            && self.vi == 0.0
            && self.vb == 0.0)
            .then_some(layout_points(self.length) + self.percent * percentage_basis)
    }

    pub(in crate::css) fn resolve_font_metric_lengths(&mut self, ch_advance: f32) {
        if self.ch != 0.0 {
            self.length += layout_pt(self.ch * ch_advance);
            self.ch = 0.0;
        }
    }

    pub(in crate::css) fn resolve_viewport_lengths(
        &mut self,
        viewport_width: f32,
        viewport_height: f32,
        viewport_inline: f32,
        viewport_block: f32,
    ) {
        let viewport_min = viewport_width.min(viewport_height);
        let viewport_max = viewport_width.max(viewport_height);
        self.length += layout_pt(
            self.vw * viewport_width / 100.0
                + self.vh * viewport_height / 100.0
                + self.vmin * viewport_min / 100.0
                + self.vmax * viewport_max / 100.0
                + self.vi * viewport_inline / 100.0
                + self.vb * viewport_block / 100.0,
        );
        self.vw = 0.0;
        self.vh = 0.0;
        self.vmin = 0.0;
        self.vmax = 0.0;
        self.vi = 0.0;
        self.vb = 0.0;
    }
}

pub(crate) fn compare_length_percentage_components(
    values: &[LengthPercentageComponents],
    choose_max: bool,
) -> Option<LengthPercentageComponents> {
    let mut best = *values.first()?;
    for candidate in &values[1..] {
        let ordering = length_percentage_component_ordering(*candidate, best)?;
        if (choose_max && ordering.is_gt()) || (!choose_max && ordering.is_lt()) {
            best = *candidate;
        }
    }
    Some(best)
}

pub(crate) fn length_percentage_component_ordering(
    left: LengthPercentageComponents,
    right: LengthPercentageComponents,
) -> Option<std::cmp::Ordering> {
    comparable_component_difference(LengthPercentageComponents {
        length: left.length - right.length,
        percent: left.percent - right.percent,
        has_percentage: left.has_percentage || right.has_percentage,
        ch: left.ch - right.ch,
        vw: left.vw - right.vw,
        vh: left.vh - right.vh,
        vmin: left.vmin - right.vmin,
        vmax: left.vmax - right.vmax,
        vi: left.vi - right.vi,
        vb: left.vb - right.vb,
    })
}

pub(crate) fn comparable_component_difference(
    value: LengthPercentageComponents,
) -> Option<std::cmp::Ordering> {
    let mut component = None;
    for amount in [
        layout_points(value.length),
        value.percent,
        value.ch,
        value.vw,
        value.vh,
        value.vmin,
        value.vmax,
        value.vi,
        value.vb,
    ] {
        if amount == 0.0 {
            continue;
        }
        if component.replace(amount).is_some() {
            return None;
        }
    }
    Some(component.unwrap_or(0.0).total_cmp(&0.0))
}

/// Computed CSS box size value for box-model properties.
///
/// CSS Cascade defines computed values as the result of resolving specified
/// values before layout computes used values:
/// <https://www.w3.org/TR/css-cascade-5/#computed>.
/// CSS Values defines `<length-percentage>`, and CSS Sizing adds intrinsic
/// size keywords and stretch-fit sizing to width/height/min/max-size
/// properties:
/// <https://www.w3.org/TR/css-values-4/#mixed-percentages> and
/// <https://www.w3.org/TR/css-sizing-3/#sizing-values> and
/// <https://drafts.csswg.org/css-sizing-4/#stretch-fit-sizing>.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum ComputedLengthPercentageOrAuto {
    Auto,
    MinContent,
    MaxContent,
    FitContent(Option<ComputedLengthPercentage>),
    Stretch,
    LengthPercentage(ComputedLengthPercentage),
}

impl ComputedLengthPercentageOrAuto {
    pub(crate) const AUTO: Self = Self::Auto;
    pub(crate) const ZERO: Self = Self::LengthPercentage(ComputedLengthPercentage::ZERO);

    pub(crate) fn length_if_no_percent(self) -> Option<f32> {
        match self {
            Self::LengthPercentage(value) => value.length_if_no_percent(),
            Self::Auto
            | Self::MinContent
            | Self::MaxContent
            | Self::FitContent(_)
            | Self::Stretch => None,
        }
    }

    pub(crate) fn is_auto(self) -> bool {
        matches!(self, Self::Auto)
    }

    pub(crate) fn resolve_font_metric_lengths(&mut self, ch_advance: f32) {
        match self {
            Self::LengthPercentage(value) | Self::FitContent(Some(value)) => {
                value.resolve_font_metric_lengths(ch_advance);
            }
            Self::Auto
            | Self::MinContent
            | Self::MaxContent
            | Self::FitContent(None)
            | Self::Stretch => {}
        }
    }

    pub(crate) fn resolve_viewport_lengths(
        &mut self,
        viewport_width: f32,
        viewport_height: f32,
        viewport_inline: f32,
        viewport_block: f32,
    ) {
        match self {
            Self::LengthPercentage(value) | Self::FitContent(Some(value)) => {
                value.resolve_viewport_lengths(
                    viewport_width,
                    viewport_height,
                    viewport_inline,
                    viewport_block,
                );
            }
            Self::Auto
            | Self::MinContent
            | Self::MaxContent
            | Self::FitContent(None)
            | Self::Stretch => {}
        }
    }
}

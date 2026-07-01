use super::*;
use std::num::NonZeroU32;
use std::sync::{Mutex, OnceLock};

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
    pub(crate) fn points_per_unit(self) -> f32 {
        match self {
            Self::Px => CSS_PX_TO_PT,
            Self::Pt | Self::NumberPt => 1.0,
            Self::In => 72.0,
            Self::Cm => 72.0 / 2.54,
            Self::Mm => 72.0 / 25.4,
            Self::Q => 72.0 / 25.4 / 4.0,
            Self::Pc => 12.0,
        }
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
        let points = match self {
            Self::Absolute { value, unit } => value * unit.points_per_unit(),
            Self::FontRelativeEm(value) => value * font_size,
            Self::FontRelativeCh(value) => value * font_size,
            Self::RootFontRelativeRem(value) => value * root_font_size,
        };
        ComputedLength { points }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct ComputedLength {
    pub points: f32,
}

/// Computed CSS `<length-percentage>` value, preserving the percentage
/// component until a property-specific used-value basis is available.
///
/// CSS Values and Units Level 4 defines mixed `<length-percentage>` values and
/// their later percentage resolution:
/// <https://www.w3.org/TR/css-values-4/#mixed-percentages>.
#[derive(Debug, Clone, Copy)]
pub(crate) struct ComputedLengthPercentage {
    pub length: f32,
    pub percent: f32,
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

fn deferred_math_store() -> &'static Mutex<Vec<DeferredLengthPercentageMath>> {
    static STORE: OnceLock<Mutex<Vec<DeferredLengthPercentageMath>>> = OnceLock::new();
    STORE.get_or_init(|| Mutex::new(Vec::new()))
}

fn store_deferred_math(math: DeferredLengthPercentageMath) -> DeferredLengthPercentageMathId {
    let mut store = deferred_math_store()
        .lock()
        .expect("deferred length math store should not be poisoned");
    store.push(math);
    let id = u32::try_from(store.len()).expect("deferred length math store exhausted");
    DeferredLengthPercentageMathId(NonZeroU32::new(id).expect("store ids are one-based"))
}

fn load_deferred_math(id: DeferredLengthPercentageMathId) -> Option<DeferredLengthPercentageMath> {
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
    pub length: f32,
    pub percent: f32,
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
        length: 0.0,
        percent: 0.0,
        ch: 0.0,
        vw: 0.0,
        vh: 0.0,
        vmin: 0.0,
        vmax: 0.0,
        vi: 0.0,
        vb: 0.0,
        math: None,
    };

    pub(crate) fn from_length(length: f32) -> Self {
        Self {
            length,
            percent: 0.0,
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
            length: 0.0,
            percent,
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
            length: 0.0,
            percent: 0.0,
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
            length: 0.0,
            percent: 0.0,
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
            length: 0.0,
            percent: 0.0,
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
            length: 0.0,
            percent: 0.0,
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
            length: 0.0,
            percent: 0.0,
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
            length: 0.0,
            percent: 0.0,
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
            length: 0.0,
            percent: 0.0,
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

    fn deferred_math(self) -> Option<DeferredLengthPercentageMath> {
        self.math.and_then(load_deferred_math)
    }

    fn from_components(components: LengthPercentageComponents) -> Self {
        Self {
            length: components.length,
            percent: components.percent,
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
            && self.ch == 0.0
            && self.vw == 0.0
            && self.vh == 0.0
            && self.vmin == 0.0
            && self.vmax == 0.0
            && self.vi == 0.0
            && self.vb == 0.0)
            .then_some(self.length)
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
            self.length += self.ch * ch_advance;
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
        self.length += self.vw * viewport_width / 100.0
            + self.vh * viewport_height / 100.0
            + self.vmin * viewport_min / 100.0
            + self.vmax * viewport_max / 100.0
            + self.vi * viewport_inline / 100.0
            + self.vb * viewport_block / 100.0;
        self.vw = 0.0;
        self.vh = 0.0;
        self.vmin = 0.0;
        self.vmax = 0.0;
        self.vi = 0.0;
        self.vb = 0.0;
    }
}

impl DeferredLengthPercentageMath {
    fn resolve_font_metric_lengths(&mut self, ch_advance: f32) {
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

    fn evaluate(self) -> Option<LengthPercentageComponents> {
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

    fn resolve_viewport_lengths(
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

    fn evaluate_used_length(self, percentage_basis: f32) -> Option<f32> {
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
    fn evaluate(self) -> Option<LengthPercentageComponents> {
        match self {
            Self::Components(value) => Some(value),
            Self::Math(id) => load_deferred_math(id)?.evaluate(),
        }
    }

    fn resolve_font_metric_lengths(&mut self, ch_advance: f32) {
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

    fn resolve_viewport_lengths(
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

    fn evaluate_used_length(self, percentage_basis: f32) -> Option<f32> {
        match self {
            Self::Components(value) => value.used_length_with_percentage_basis(percentage_basis),
            Self::Math(id) => load_deferred_math(id)?.evaluate_used_length(percentage_basis),
        }
    }
}

impl LengthPercentageComponents {
    fn add(self, other: Self) -> Self {
        Self {
            length: self.length + other.length,
            percent: self.percent + other.percent,
            ch: self.ch + other.ch,
            vw: self.vw + other.vw,
            vh: self.vh + other.vh,
            vmin: self.vmin + other.vmin,
            vmax: self.vmax + other.vmax,
            vi: self.vi + other.vi,
            vb: self.vb + other.vb,
        }
    }

    fn mul(self, factor: f32) -> Self {
        Self {
            length: self.length * factor,
            percent: self.percent * factor,
            ch: self.ch * factor,
            vw: self.vw * factor,
            vh: self.vh * factor,
            vmin: self.vmin * factor,
            vmax: self.vmax * factor,
            vi: self.vi * factor,
            vb: self.vb * factor,
        }
    }

    fn depends_on_metric_or_percent(self) -> bool {
        self.percent != 0.0
            || self.ch != 0.0
            || self.vw != 0.0
            || self.vh != 0.0
            || self.vmin != 0.0
            || self.vmax != 0.0
            || self.vi != 0.0
            || self.vb != 0.0
    }

    fn used_length_with_percentage_basis(self, percentage_basis: f32) -> Option<f32> {
        (self.ch == 0.0
            && self.vw == 0.0
            && self.vh == 0.0
            && self.vmin == 0.0
            && self.vmax == 0.0
            && self.vi == 0.0
            && self.vb == 0.0)
            .then_some(self.length + self.percent * percentage_basis)
    }

    fn resolve_font_metric_lengths(&mut self, ch_advance: f32) {
        if self.ch != 0.0 {
            self.length += self.ch * ch_advance;
            self.ch = 0.0;
        }
    }

    fn resolve_viewport_lengths(
        &mut self,
        viewport_width: f32,
        viewport_height: f32,
        viewport_inline: f32,
        viewport_block: f32,
    ) {
        let viewport_min = viewport_width.min(viewport_height);
        let viewport_max = viewport_width.max(viewport_height);
        self.length += self.vw * viewport_width / 100.0
            + self.vh * viewport_height / 100.0
            + self.vmin * viewport_min / 100.0
            + self.vmax * viewport_max / 100.0
            + self.vi * viewport_inline / 100.0
            + self.vb * viewport_block / 100.0;
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
        value.length,
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
/// size keywords to width/height/min/max-size properties:
/// <https://www.w3.org/TR/css-values-4/#mixed-percentages> and
/// <https://www.w3.org/TR/css-sizing-3/#sizing-values>.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum ComputedLengthPercentageOrAuto {
    Auto,
    MinContent,
    MaxContent,
    FitContent(Option<ComputedLengthPercentage>),
    LengthPercentage(ComputedLengthPercentage),
}

impl ComputedLengthPercentageOrAuto {
    pub(crate) const AUTO: Self = Self::Auto;
    pub(crate) const ZERO: Self = Self::LengthPercentage(ComputedLengthPercentage::ZERO);

    pub(crate) fn length_if_no_percent(self) -> Option<f32> {
        match self {
            Self::LengthPercentage(value) => value.length_if_no_percent(),
            Self::Auto | Self::MinContent | Self::MaxContent | Self::FitContent(_) => None,
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
            Self::Auto | Self::MinContent | Self::MaxContent | Self::FitContent(None) => {}
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
            Self::Auto | Self::MinContent | Self::MaxContent | Self::FitContent(None) => {}
        }
    }
}

/// Computed `flex-basis` value.
///
/// CSS Flexbox defines `flex-basis` as `content | <width>`, where `<width>`
/// includes intrinsic sizing keywords, `<length-percentage>`, and `auto`. The
/// `content` keyword is not a generic box-size value: it forces content-based
/// flex base sizing instead of retrieving the main-size property like `auto`:
/// <https://www.w3.org/TR/css-flexbox-1/#flex-basis-property> and
/// <https://www.w3.org/TR/css-sizing-3/#intrinsic-sizes>.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum ComputedFlexBasis {
    Auto,
    Content,
    MinContent,
    MaxContent,
    FitContent(Option<ComputedLengthPercentage>),
    LengthPercentage(ComputedLengthPercentage),
}

impl ComputedFlexBasis {
    pub(crate) const AUTO: Self = Self::Auto;

    pub(crate) fn resolve_font_metric_lengths(&mut self, ch_advance: f32) {
        match self {
            Self::FitContent(Some(value)) | Self::LengthPercentage(value) => {
                value.resolve_font_metric_lengths(ch_advance);
            }
            Self::Auto
            | Self::Content
            | Self::MinContent
            | Self::MaxContent
            | Self::FitContent(None) => {}
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
            Self::FitContent(Some(value)) | Self::LengthPercentage(value) => {
                value.resolve_viewport_lengths(
                    viewport_width,
                    viewport_height,
                    viewport_inline,
                    viewport_block,
                );
            }
            Self::Auto
            | Self::Content
            | Self::MinContent
            | Self::MaxContent
            | Self::FitContent(None) => {}
        }
    }
}

/// Four physical CSS edges in top/right/bottom/left order.
///
/// CSS Box Model Level 3 defines physical margin, padding, and border edge
/// properties in this order:
/// <https://www.w3.org/TR/css-box-3/#the-margin-properties>.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct CssEdges<T> {
    pub top: T,
    pub right: T,
    pub bottom: T,
    pub left: T,
}

impl<T: Copy> CssEdges<T> {
    pub(crate) const fn all(value: T) -> Self {
        Self {
            top: value,
            right: value,
            bottom: value,
            left: value,
        }
    }
}

/// Typed computed box-model values retained until layout resolves used values.
///
/// CSS Cascade defines computed values:
/// <https://www.w3.org/TR/css-cascade-5/#computed>.
/// CSS 2.2 defines used widths, margins, padding, and positioned offsets:
/// <https://www.w3.org/TR/CSS22/visudet.html>,
/// <https://www.w3.org/TR/CSS22/box.html>, and
/// <https://www.w3.org/TR/CSS22/visuren.html#relative-positioning>.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ComputedBoxValues {
    pub margin: CssEdges<ComputedLengthPercentageOrAuto>,
    pub padding: CssEdges<ComputedLengthPercentage>,
    pub width: ComputedLengthPercentageOrAuto,
    pub height: ComputedLengthPercentageOrAuto,
    pub min_width: ComputedLengthPercentageOrAuto,
    pub max_width: ComputedLengthPercentageOrAuto,
    pub min_height: ComputedLengthPercentageOrAuto,
    pub max_height: ComputedLengthPercentageOrAuto,
    pub inset_left: ComputedLengthPercentageOrAuto,
    pub inset_top: ComputedLengthPercentageOrAuto,
    pub inset_right: ComputedLengthPercentageOrAuto,
    pub inset_bottom: ComputedLengthPercentageOrAuto,
}

impl ComputedBoxValues {
    pub(crate) const fn initial() -> Self {
        Self {
            margin: CssEdges::all(ComputedLengthPercentageOrAuto::ZERO),
            padding: CssEdges::all(ComputedLengthPercentage::ZERO),
            width: ComputedLengthPercentageOrAuto::AUTO,
            height: ComputedLengthPercentageOrAuto::AUTO,
            min_width: ComputedLengthPercentageOrAuto::AUTO,
            max_width: ComputedLengthPercentageOrAuto::AUTO,
            min_height: ComputedLengthPercentageOrAuto::AUTO,
            max_height: ComputedLengthPercentageOrAuto::AUTO,
            inset_left: ComputedLengthPercentageOrAuto::AUTO,
            inset_top: ComputedLengthPercentageOrAuto::AUTO,
            inset_right: ComputedLengthPercentageOrAuto::AUTO,
            inset_bottom: ComputedLengthPercentageOrAuto::AUTO,
        }
    }

    pub(crate) fn resolve_font_metric_lengths(&mut self, ch_advance: f32) {
        self.margin.top.resolve_font_metric_lengths(ch_advance);
        self.margin.right.resolve_font_metric_lengths(ch_advance);
        self.margin.bottom.resolve_font_metric_lengths(ch_advance);
        self.margin.left.resolve_font_metric_lengths(ch_advance);
        self.padding.top.resolve_font_metric_lengths(ch_advance);
        self.padding.right.resolve_font_metric_lengths(ch_advance);
        self.padding.bottom.resolve_font_metric_lengths(ch_advance);
        self.padding.left.resolve_font_metric_lengths(ch_advance);
        self.width.resolve_font_metric_lengths(ch_advance);
        self.height.resolve_font_metric_lengths(ch_advance);
        self.min_width.resolve_font_metric_lengths(ch_advance);
        self.max_width.resolve_font_metric_lengths(ch_advance);
        self.min_height.resolve_font_metric_lengths(ch_advance);
        self.max_height.resolve_font_metric_lengths(ch_advance);
        self.inset_left.resolve_font_metric_lengths(ch_advance);
        self.inset_top.resolve_font_metric_lengths(ch_advance);
        self.inset_right.resolve_font_metric_lengths(ch_advance);
        self.inset_bottom.resolve_font_metric_lengths(ch_advance);
    }

    pub(crate) fn resolve_viewport_lengths(
        &mut self,
        viewport_width: f32,
        viewport_height: f32,
        viewport_inline: f32,
        viewport_block: f32,
    ) {
        self.margin.top.resolve_viewport_lengths(
            viewport_width,
            viewport_height,
            viewport_inline,
            viewport_block,
        );
        self.margin.right.resolve_viewport_lengths(
            viewport_width,
            viewport_height,
            viewport_inline,
            viewport_block,
        );
        self.margin.bottom.resolve_viewport_lengths(
            viewport_width,
            viewport_height,
            viewport_inline,
            viewport_block,
        );
        self.margin.left.resolve_viewport_lengths(
            viewport_width,
            viewport_height,
            viewport_inline,
            viewport_block,
        );
        self.padding.top.resolve_viewport_lengths(
            viewport_width,
            viewport_height,
            viewport_inline,
            viewport_block,
        );
        self.padding.right.resolve_viewport_lengths(
            viewport_width,
            viewport_height,
            viewport_inline,
            viewport_block,
        );
        self.padding.bottom.resolve_viewport_lengths(
            viewport_width,
            viewport_height,
            viewport_inline,
            viewport_block,
        );
        self.padding.left.resolve_viewport_lengths(
            viewport_width,
            viewport_height,
            viewport_inline,
            viewport_block,
        );
        self.width.resolve_viewport_lengths(
            viewport_width,
            viewport_height,
            viewport_inline,
            viewport_block,
        );
        self.height.resolve_viewport_lengths(
            viewport_width,
            viewport_height,
            viewport_inline,
            viewport_block,
        );
        self.min_width.resolve_viewport_lengths(
            viewport_width,
            viewport_height,
            viewport_inline,
            viewport_block,
        );
        self.max_width.resolve_viewport_lengths(
            viewport_width,
            viewport_height,
            viewport_inline,
            viewport_block,
        );
        self.min_height.resolve_viewport_lengths(
            viewport_width,
            viewport_height,
            viewport_inline,
            viewport_block,
        );
        self.max_height.resolve_viewport_lengths(
            viewport_width,
            viewport_height,
            viewport_inline,
            viewport_block,
        );
        self.inset_left.resolve_viewport_lengths(
            viewport_width,
            viewport_height,
            viewport_inline,
            viewport_block,
        );
        self.inset_top.resolve_viewport_lengths(
            viewport_width,
            viewport_height,
            viewport_inline,
            viewport_block,
        );
        self.inset_right.resolve_viewport_lengths(
            viewport_width,
            viewport_height,
            viewport_inline,
            viewport_block,
        );
        self.inset_bottom.resolve_viewport_lengths(
            viewport_width,
            viewport_height,
            viewport_inline,
            viewport_block,
        );
    }
}

/// Computed `isolation`.
///
/// CSS Compositing and Blending defines `isolation:isolate` as creating an
/// isolated stacking context:
/// <https://www.w3.org/TR/compositing-1/#isolation>.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Isolation {
    Auto,
    Isolate,
}

/// Computed `mix-blend-mode` subset.
///
/// Non-`normal` blend modes establish stacking contexts. The renderer currently
/// records the trigger and preserves isolation ordering; PDF blend-mode output
/// can be expanded from these variants:
/// <https://www.w3.org/TR/compositing-1/#mix-blend-mode>.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MixBlendMode {
    Normal,
    Multiply,
    Screen,
    Overlay,
    Darken,
    Lighten,
    ColorDodge,
    ColorBurn,
    HardLight,
    SoftLight,
    Difference,
    Exclusion,
    Hue,
    Saturation,
    Color,
    Luminosity,
}

/// A finite CSS filter amount validated as non-negative.
///
/// CSS Filter Effects uses this grammar for functions such as `brightness()`
/// and `saturate()`. Individual functions decide whether values above one are
/// clamped, permitted, or require a raster backend.
/// <https://www.w3.org/TR/filter-effects-1/#filter-functions>
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct NonNegativeFilterAmount(f32);

impl NonNegativeFilterAmount {
    pub(crate) fn new(value: f32) -> Option<Self> {
        (value.is_finite() && value >= 0.0).then_some(Self(value))
    }

    pub(crate) const fn value(self) -> f32 {
        self.0
    }

    pub(crate) fn clamped_unit_interval(self) -> UnitFilterAmount {
        UnitFilterAmount(self.0.min(1.0))
    }
}

/// A CSS filter amount known to be in the closed unit interval.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct UnitFilterAmount(f32);

impl UnitFilterAmount {
    pub(crate) const ONE: Self = Self(1.0);

    pub(crate) const fn value(self) -> f32 {
        self.0
    }

    pub(crate) fn multiplied(self, other: Self) -> Self {
        Self((self.0 * other.0).clamp(0.0, 1.0))
    }
}

/// A bounded linear transform of encoded sRGB components.
///
/// Rows are non-negative and sum to at most one. This preserves the unit RGB
/// cube, black, and alpha, which makes the transform distributable across a
/// normal source-over paint tree without introducing a clamp-dependent color
/// change. The type intentionally cannot represent color matrices such as
/// `sepia()` or `hue-rotate()` that require a raster filter surface.
/// <https://www.w3.org/TR/filter-effects-1/#filter-functions>
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct BoundedSrgbColorTransform {
    rows: [[f32; 3]; 3],
}

impl BoundedSrgbColorTransform {
    const EPSILON: f32 = 1e-5;

    pub(crate) const IDENTITY: Self = Self {
        rows: [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
    };

    fn new(rows: [[f32; 3]; 3]) -> Option<Self> {
        rows.iter()
            .all(|row| {
                row.iter()
                    .all(|component| component.is_finite() && *component >= 0.0)
                    && row.iter().sum::<f32>() <= 1.0 + Self::EPSILON
            })
            .then_some(Self { rows })
    }

    /// Compose `next` after this transform in CSS filter-list order.
    pub(crate) fn then(self, next: Self) -> Self {
        let mut rows = [[0.0; 3]; 3];
        for (row_index, row) in rows.iter_mut().enumerate() {
            for (column_index, component) in row.iter_mut().enumerate() {
                *component = (0..3)
                    .map(|index| next.rows[row_index][index] * self.rows[index][column_index])
                    .sum();
            }
        }
        Self::new(rows).expect("bounded sRGB transforms are closed under composition")
    }

    pub(crate) fn apply(self, components: [f32; 3]) -> [f32; 3] {
        self.rows.map(|row| {
            (row[0] * components[0] + row[1] * components[1] + row[2] * components[2])
                .clamp(0.0, 1.0)
        })
    }

    pub(crate) fn grayscale(amount: UnitFilterAmount) -> Self {
        let amount = amount.value();
        let retained = 1.0 - amount;
        Self::new([
            [
                0.2126 + 0.7874 * retained,
                0.7152 - 0.7152 * retained,
                0.0722 - 0.0722 * retained,
            ],
            [
                0.2126 - 0.2126 * retained,
                0.7152 + 0.2848 * retained,
                0.0722 - 0.0722 * retained,
            ],
            [
                0.2126 - 0.2126 * retained,
                0.7152 - 0.7152 * retained,
                0.0722 + 0.9278 * retained,
            ],
        ])
        .expect("CSS grayscale matrix is bounded for a unit amount")
    }

    pub(crate) fn saturate(amount: UnitFilterAmount) -> Self {
        let amount = amount.value();
        Self::new([
            [
                0.213 + 0.787 * amount,
                0.715 - 0.715 * amount,
                0.072 - 0.072 * amount,
            ],
            [
                0.213 - 0.213 * amount,
                0.715 + 0.285 * amount,
                0.072 - 0.072 * amount,
            ],
            [
                0.213 - 0.213 * amount,
                0.715 - 0.715 * amount,
                0.072 + 0.928 * amount,
            ],
        ])
        .expect("CSS saturation matrix is bounded for a unit amount")
    }

    pub(crate) fn brightness(amount: UnitFilterAmount) -> Self {
        let amount = amount.value();
        Self::new([[amount, 0.0, 0.0], [0.0, amount, 0.0], [0.0, 0.0, amount]])
            .expect("CSS brightness matrix is bounded for a unit amount")
    }
}

/// The exact filter subset that can be lowered into ordinary source-over
/// paint without a raster filter surface.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct ExactFilterLowering {
    pub(crate) color: BoundedSrgbColorTransform,
    pub(crate) alpha: UnitFilterAmount,
}

impl ExactFilterLowering {
    /// Whether the lowering leaves source pixels visually unchanged.
    pub(crate) fn is_visual_identity(self) -> bool {
        self.color == BoundedSrgbColorTransform::IDENTITY && self.alpha == UnitFilterAmount::ONE
    }
}

/// Computed CSS filter functions retained for later execution selection.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum FilterFunction {
    Grayscale(UnitFilterAmount),
    Saturate(NonNegativeFilterAmount),
    Brightness(NonNegativeFilterAmount),
    Opacity(UnitFilterAmount),
    /// A syntactically present filter function proved to leave pixels unchanged.
    VisualIdentity,
    /// A valid filter function that this first lowering pass cannot render.
    RequiresRasterBackend(String),
}

/// Computed `filter` value retained for stacking and later effect emission.
///
/// Non-`none` filter function lists establish a stacking context:
/// <https://www.w3.org/TR/filter-effects-1/#FilterProperty>.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum FilterValue {
    None,
    Functions(Vec<FilterFunction>),
}

impl FilterValue {
    pub(crate) fn exact_lowering(&self) -> Option<ExactFilterLowering> {
        let Self::Functions(functions) = self else {
            return None;
        };
        let mut lowering = ExactFilterLowering {
            color: BoundedSrgbColorTransform::IDENTITY,
            alpha: UnitFilterAmount::ONE,
        };
        for function in functions {
            match *function {
                FilterFunction::Grayscale(amount) => {
                    lowering.color = lowering
                        .color
                        .then(BoundedSrgbColorTransform::grayscale(amount));
                }
                FilterFunction::Saturate(amount) if amount.value() <= 1.0 => {
                    lowering.color = lowering.color.then(BoundedSrgbColorTransform::saturate(
                        amount.clamped_unit_interval(),
                    ));
                }
                FilterFunction::Brightness(amount) if amount.value() <= 1.0 => {
                    lowering.color = lowering.color.then(BoundedSrgbColorTransform::brightness(
                        amount.clamped_unit_interval(),
                    ));
                }
                FilterFunction::Opacity(amount) => {
                    lowering.alpha = lowering.alpha.multiplied(amount)
                }
                FilterFunction::VisualIdentity => {}
                FilterFunction::Saturate(_)
                | FilterFunction::Brightness(_)
                | FilterFunction::RequiresRasterBackend(_) => return None,
            }
        }
        Some(lowering)
    }
}

/// Computed mask image/source value retained for stacking and later masking.
///
/// Non-`none` masks establish an isolated paint effect in CSS Masking:
/// <https://www.w3.org/TR/css-masking-1/#the-mask-image>.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum MaskValue {
    None,
    Image(String),
}

/// Computed `will-change` features relevant to stacking-context prediction.
///
/// CSS Will Change requires the element to act as if specified features already
/// had their non-initial values for stacking-context creation:
/// <https://www.w3.org/TR/css-will-change-1/#will-change>.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) struct WillChange {
    pub(crate) contents: bool,
    pub(crate) scroll_position: bool,
    pub(crate) opacity: bool,
    pub(crate) transform: bool,
    pub(crate) filter: bool,
    pub(crate) clip_path: bool,
    pub(crate) mask: bool,
    pub(crate) mix_blend_mode: bool,
    pub(crate) isolation: bool,
    pub(crate) contain: bool,
}

#[cfg(test)]
mod filter_tests {
    use super::*;

    #[test]
    fn grayscale_clamps_and_preserves_neutral_colors() {
        let amount = NonNegativeFilterAmount::new(2.0)
            .unwrap()
            .clamped_unit_interval();
        let transform = BoundedSrgbColorTransform::grayscale(amount);
        assert_eq!(transform.apply([0.25, 0.25, 0.25]), [0.25, 0.25, 0.25]);
        let [red, green, blue] = transform.apply([1.0, 0.0, 0.0]);
        assert!((red - 0.2126).abs() < 0.0001);
        assert!((green - 0.2126).abs() < 0.0001);
        assert!((blue - 0.2126).abs() < 0.0001);
    }

    #[test]
    fn exact_filter_lowering_rejects_expanding_matrices() {
        let filter = FilterValue::Functions(vec![FilterFunction::Brightness(
            NonNegativeFilterAmount::new(1.01).unwrap(),
        )]);
        assert_eq!(filter.exact_lowering(), None);
    }

    #[test]
    fn exact_filter_composes_authored_order_and_alpha() {
        let filter = FilterValue::Functions(vec![
            FilterFunction::Brightness(NonNegativeFilterAmount::new(0.5).unwrap()),
            FilterFunction::Opacity(UnitFilterAmount(0.5)),
            FilterFunction::Grayscale(UnitFilterAmount(1.0)),
        ]);
        let lowering = filter.exact_lowering().unwrap();
        assert_eq!(lowering.alpha.value(), 0.5);
        let [red, green, blue] = lowering.color.apply([1.0, 0.0, 0.0]);
        assert!((red - 0.1063).abs() < 0.0001);
        assert!((green - red).abs() < 0.0001);
        assert!((blue - red).abs() < 0.0001);
    }
}

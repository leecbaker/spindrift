use super::font_registry::FontSupportKind;
use super::*;
use crate::css::{
    BaselineShift, ComputedLengthPercentage, FontSizeAdjust, FontSizeAdjustMetric,
    FontSizeAdjustValue,
};
use crate::{RenderedLine, RenderedTextRun};

mod split_1;
pub(in crate::text) use self::split_1::*;
mod split_2;
pub(crate) use self::split_2::*;
mod split_3;
pub(in crate::text) use self::split_3::*;

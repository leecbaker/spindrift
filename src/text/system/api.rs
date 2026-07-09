use super::font_registry::FontSupportKind;
use super::*;
use crate::css::{BaselineShift, FontSizeAdjust, FontSizeAdjustMetric, FontSizeAdjustValue};
use crate::document::{PaintSize, PaintTransform};
use crate::{
    Color, PaintPoint, PaintRect, RenderedImage, RenderedLine, RenderedPath, RenderedPathCommand,
    RenderedPathFillRule, RenderedTextRun,
};

mod split_1;
pub(in crate::text) use self::split_1::*;
mod split_2;
pub(crate) use self::split_2::*;
mod split_3;
pub(in crate::text) use self::split_3::*;

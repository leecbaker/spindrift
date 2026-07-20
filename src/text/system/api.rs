use super::font_registry::FontSupportKind;
use super::*;
#[cfg(test)]
use crate::RenderedLine;
use crate::css::{BaselineShift, FontSizeAdjust, FontSizeAdjustMetric, FontSizeAdjustValue};
use crate::document::{PaintSize, PaintTransform};
use crate::{
    CssColor, PaintPoint, PaintRect, RenderedImage, RenderedPath, RenderedPathCommand,
    RenderedPathFillRule, RenderedTextRun,
};

mod split_1;
pub(in crate::text) use self::split_1::*;
mod split_2;
pub(crate) use self::split_2::*;
mod split_3;
pub(in crate::text) use self::split_3::*;

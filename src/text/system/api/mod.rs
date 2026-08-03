#![allow(unused_imports)]

//! Internal font-system operations grouped by the stage that owns their data.
//!
//! Font selection and CSS metrics live alongside the [`FontSystem`] API,
//! while Parley conversion and OpenType paint extraction are separated into
//! shaping and glyph-painting submodules.

use super::font_registry::FontSupportKind;
use super::*;
use crate::CssColor;
use crate::css::{BaselineShift, FontSizeAdjust, FontSizeAdjustMetric, FontSizeAdjustValue};
use crate::document::paint::geometry::{PaintPoint, PaintRect, PaintSize, PaintTransform};
use crate::document::paint::images::RenderedImage;
use crate::document::paint::paths::{RenderedPath, RenderedPathCommand, RenderedPathFillRule};
#[cfg(test)]
use crate::document::paint::text::RenderedLine;
use crate::document::paint::text::RenderedTextRun;

mod font;
mod glyphs;
mod metrics;
mod shaping;

use font::*;
use glyphs::*;
use metrics::*;
use shaping::*;

pub(crate) use shaping::span_boundary_needs_join_control;

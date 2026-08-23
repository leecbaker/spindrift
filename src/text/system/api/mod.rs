//! Internal font-system operations grouped by the stage that owns their data.
//!
//! Font selection and CSS metrics live alongside the [`FontSystem`] API,
//! while Parley conversion and OpenType paint extraction are separated into
//! shaping and glyph-painting submodules.

use super::font_registry::{EmojiPresentationCapability, FontSupportKind};
use super::*;
use crate::CssColor;
use crate::document::paint::geometry::PaintPoint;
use crate::document::paint::text::RenderedTextRun;

mod font;
mod glyphs;
mod metrics;
mod shaping;

pub(in crate::text) use font::*;
use metrics::*;
pub(crate) use shaping::span_boundary_needs_join_control;
use shaping::*;

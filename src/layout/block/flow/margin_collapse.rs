use super::*;

mod eligibility;
pub(in crate::layout) use self::eligibility::*;
mod state;
pub(in crate::layout) use self::state::*;
mod formatting_boxes;
pub(in crate::layout) use self::formatting_boxes::*;
#[path = "margin_collapse/dom.rs"]
mod dom_backed;
pub(in crate::layout) use self::dom_backed::*;

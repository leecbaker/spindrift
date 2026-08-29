//! Computed CSS `<length-percentage>` values.
//!
//! The compact affine representation and the deferred CSS Math tree share one
//! public API. The recursive implementation lives in [`expression`].

mod expression;

pub(crate) use self::expression::*;

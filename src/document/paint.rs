use super::Page;
use crate::{Color, Error, Result};
use std::borrow::Cow;

mod split_1;
pub(crate) use self::split_1::*;
mod split_2;
pub(crate) use self::split_2::*;
mod split_3;
pub(crate) use self::split_3::*;
mod split_4;
pub use self::split_4::LinkAnnotation;
pub(crate) use self::split_4::*;

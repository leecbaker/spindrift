use super::*;

mod split_1;
pub(crate) use self::split_1::*;
mod split_2;
pub(crate) use self::split_2::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BoxSizing {
    ContentBox,
    BorderBox,
}

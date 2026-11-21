//! Font data shared by the tests.

use std::sync::Arc;

use crate::fonts::{Font, Fonts};

/// M+ 1 Code, the default example font.
pub(crate) const MPLUS: &[u8] = include_bytes!("../examples/assets/fonts/Mplus1Code-Regular.ttf");
/// A 10 px pixel font with Japanese coverage.
pub(crate) const FUSION: &[u8] =
    include_bytes!("../examples/assets/fonts/fusion-pixel-10px-monospaced-ja.ttf");
/// A Noto Color Emoji subset in COLRv1.
pub(crate) const COLRV1: &[u8] = include_bytes!("backend/testdata/noto_colrv1_subset.ttf");
/// The same subset as PNG strikes (CBDT).
#[cfg(feature = "emoji")]
pub(crate) const CBDT: &[u8] = include_bytes!("backend/testdata/noto_cbdt_subset.ttf");

/// M+ at 16 px, ready to share.
pub(crate) fn mplus_16() -> Arc<Fonts> {
    Arc::new(Fonts::new(Font::new(MPLUS).expect("M+ parses"), 16))
}

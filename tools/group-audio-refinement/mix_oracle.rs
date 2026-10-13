//! Dependency-free executable of the exact production mixing module + independent oracle.
#![allow(dead_code)]
const FRAME: usize = 960;
#[path = "../../crates/audio/src/mixing.rs"]
mod mixing;
pub use mixing::{mix, mix_sums};
#[path = "../../crates/audio/src/mix_sum_tests.rs"]
mod tests;

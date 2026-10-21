//! Generated UI sounds. The synthesis (notes, palettes, rendering) lives in the core (`p2pcore::cues`) so Android plays the
//! same samples; the desktop uses it as is.

pub use p2pcore::cues::*;

#[cfg(test)]
#[path = "cues_tests.rs"]
mod tests;

//! Generated UI sounds. The synthesis (notes, palettes, rendering) lives in the core (`p2pcore::cues`) so Android plays the
//! same samples; the desktop uses it as is.

pub use p2pcore::cues::*;

#[cfg(test)]
mod tests {
    #[test]
    fn line_gap_matches_the_mark() {
        assert_eq!(super::LINE_GAP_S, crate::mark::LINE_GAP_S);
    }
}

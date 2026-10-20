use super::*;

const DAY: &str = "2026-10-07";
const NOON: i64 = 1_791_374_400_000; // 2026-10-07 12:00:00 UTC

// KIND: invariant
#[test]
fn days() {
    assert_eq!(day_of(NOON), DAY);
    assert_eq!(day_of(0), "1970-01-01");
    assert_eq!(day_start(DAY), Some(NOON - 12 * HOUR_MS));
    assert_eq!(day_start("2026-02-30"), None);
    assert_eq!(day_start("nope"), None);
}

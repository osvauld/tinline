use super::super::images::MAX_GROUP_IMAGE_BYTES;
use super::*;
use crate::store::plan_eviction;

// Option B (group files §4): hosts keep every file, so the largest one (sealed: 16 bytes of tag
// per 64 KiB chunk) must fit a group's budget on its own, or the budget pass would drop it the
// moment it arrived.
#[test]
fn the_largest_file_fits_a_hosts_group_budget() {
    let sealed = crypt::cipher_len(MAX_FILE_BYTES);
    assert!(sealed <= MAX_GROUP_IMAGE_BYTES, "{sealed} > {MAX_GROUP_IMAGE_BYTES}");
    assert!(plan_eviction(&[([1; 32], sealed, 10, true)], MAX_GROUP_IMAGE_BYTES).is_empty());
    // Anything next to it goes first, the oldest first, and the file stays.
    let held = [([2; 32], 1000, 5, true), ([1; 32], sealed, 10, true)];
    assert_eq!(plan_eviction(&held, MAX_GROUP_IMAGE_BYTES), vec![[2; 32]]);
    assert!(group_core::record::HOST_KEEP_FILE_MAX >= MAX_FILE_BYTES);
}

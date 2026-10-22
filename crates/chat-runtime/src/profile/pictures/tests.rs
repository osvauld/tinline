use super::*;
#[test]
fn invalid_and_oversized_pictures_are_rejected() {
    assert!(normalize(&[0; 100], false).is_err());
    assert!(normalize(&vec![0; 2 * MAX_PICTURE + 1], false).is_err());
}

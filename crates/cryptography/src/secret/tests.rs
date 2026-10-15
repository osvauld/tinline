use super::*;

#[test]
fn debug_hides_the_bytes_and_random_differs() {
    let s = Secret32::new([0xab; 32]);
    assert_eq!(format!("{s:?}"), "Secret32(..)");
    assert_eq!(s.expose(), &[0xab; 32]);
    assert_ne!(Secret32::random().expose(), Secret32::random().expose());
}

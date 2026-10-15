use super::*;

#[test]
fn draws_differ_and_are_not_zero() {
    let (a, b) = (bytes::<32>(), bytes::<32>());
    assert_ne!(a, [0; 32]);
    assert_ne!(a, b);
    assert_ne!(u64(), u64());
}

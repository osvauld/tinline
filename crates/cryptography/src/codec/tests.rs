use super::*;
use serde::Deserialize;

#[derive(Debug, PartialEq, Serialize, Deserialize)]
struct S {
    n: u64,
    s: String,
    b: Vec<u8>,
    o: Option<[u8; 2]>,
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
enum E {
    A,
    B(u32),
    C { x: i64 },
}

fn s() -> S {
    S { n: 300, s: "hé".into(), b: vec![1, 2, 3], o: Some([9, 8]) }
}

#[test]
fn round_trips() {
    let b = to_bytes(&s()).unwrap();
    assert_eq!(from_bytes::<S>(&b, b.len()).unwrap(), s());
    assert_eq!(from_bytes_canonical::<S>(&b, 1024).unwrap(), s());
}

#[test]
fn cap_is_checked_before_decoding() {
    let b = to_bytes(&s()).unwrap();
    assert_eq!(from_bytes::<S>(&b, b.len() - 1), Err(CodecError::TooLarge));
    assert_eq!(from_bytes_canonical::<S>(&b, b.len() - 1), Err(CodecError::TooLarge));
}

#[test]
fn trailing_and_truncated_bytes_are_rejected() {
    let mut b = to_bytes(&s()).unwrap();
    assert_eq!(from_bytes::<S>(&b[..b.len() - 1], 1024), Err(CodecError::Decode));
    b.push(0);
    assert_eq!(from_bytes::<S>(&b, 1024), Err(CodecError::Decode));
}

#[test]
fn overlong_varint_decodes_but_is_not_canonical() {
    // 1 as a minimal varint is [1]; [0x81, 0x00] is the same value, overlong.
    assert_eq!(from_bytes::<u64>(&[0x81, 0x00], 16), Ok(1));
    assert_eq!(from_bytes_canonical::<u64>(&[0x81, 0x00], 16), Err(CodecError::NonCanonical));
    assert_eq!(from_bytes_canonical::<u64>(&[1], 16), Ok(1));
}

#[test]
fn enum_tags_are_one_byte_varints_in_declaration_order() {
    assert_eq!(to_bytes(&E::A).unwrap(), [0]);
    assert_eq!(to_bytes(&E::B(5)).unwrap(), [1, 5]);
    assert_eq!(to_bytes(&E::C { x: -1 }).unwrap(), [2, 1]); // zigzag
    assert_eq!(from_bytes::<E>(&[3], 16), Err(CodecError::Decode));
}

#[test]
fn len_prefix_max_covers_a_u64() {
    assert_eq!(to_bytes(&u64::MAX).unwrap().len(), LEN_PREFIX_MAX);
}

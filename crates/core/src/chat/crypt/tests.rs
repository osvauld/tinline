use super::*;

fn data(n: usize) -> Vec<u8> {
    (0..n).map(|i| (i * 31 % 251) as u8).collect()
}

#[test]
fn round_trips_around_chunk_boundaries() {
    let key = new_key();
    for n in [0, 1, 100, CHUNK - 1, CHUNK, CHUNK + 1, 2 * CHUNK, 2 * CHUNK + 5, 5 * CHUNK + 17] {
        let plain = data(n);
        let ct = encrypt_bytes(&key, &plain);
        assert_eq!(ct.len() as u64, cipher_len(n as u64), "len for {n}");
        assert_eq!(decrypt_bytes(&key, &ct).unwrap(), plain, "round trip {n}");
    }
}

#[test]
fn wrong_key_flip_truncate_reorder_extend_all_fail() {
    let key = new_key();
    let plain = data(3 * CHUNK + 10);
    let ct = encrypt_bytes(&key, &plain);
    assert!(decrypt_bytes(&new_key(), &ct).is_err());
    let mut flipped = ct.clone();
    flipped[CHUNK + 20] ^= 1;
    assert!(decrypt_bytes(&key, &flipped).is_err());
    // Drop the final chunk: the new last chunk was sealed as "not last".
    let full = CHUNK + TAG;
    assert!(decrypt_bytes(&key, &ct[..3 * full]).is_err());
    // Cut mid-chunk.
    assert!(decrypt_bytes(&key, &ct[..ct.len() - 3]).is_err());
    // Swap chunk 0 and 1.
    let mut swapped = ct.clone();
    let (a, b) = (ct[..full].to_vec(), ct[full..2 * full].to_vec());
    swapped[..full].copy_from_slice(&b);
    swapped[full..2 * full].copy_from_slice(&a);
    assert!(decrypt_bytes(&key, &swapped).is_err());
    // Append the first chunk again.
    let mut extended = ct.clone();
    extended.extend_from_slice(&ct[..full]);
    assert!(decrypt_bytes(&key, &extended).is_err());
    assert!(decrypt_bytes(&key, &[]).is_err());
    assert!(decrypt_bytes(&key, &[0u8; 5]).is_err());
}

#[test]
fn exact_multiple_has_no_trailing_empty_chunk() {
    let key = new_key();
    let ct = encrypt_bytes(&key, &data(2 * CHUNK));
    assert_eq!(ct.len(), 2 * (CHUNK + TAG));
}

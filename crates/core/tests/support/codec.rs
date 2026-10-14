//! URL-safe no-pad base64 (the encoding of `SignedBlob`), so tests can tamper with payloads.

const B64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";

pub fn encode(bytes: &[u8]) -> String {
    let mut out = String::new();
    for chunk in bytes.chunks(3) {
        let n = chunk.iter().fold(0u32, |a, b| (a << 8) | *b as u32) << (8 * (3 - chunk.len()));
        for i in 0..=chunk.len() {
            out.push(B64[((n >> (18 - 6 * i)) & 63) as usize] as char);
        }
    }
    out
}

pub fn decode(text: &str) -> Vec<u8> {
    let vals: Vec<u32> = text.bytes().map(|c| B64.iter().position(|x| *x == c).unwrap() as u32).collect();
    let mut out = Vec::new();
    for chunk in vals.chunks(4) {
        let n = chunk.iter().fold(0u32, |a, b| (a << 6) | b) << (6 * (4 - chunk.len()));
        for i in 0..chunk.len() - 1 {
            out.push((n >> (16 - 8 * i)) as u8);
        }
    }
    out
}

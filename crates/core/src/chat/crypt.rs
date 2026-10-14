//! Chunked AEAD for blobs: one fresh random AES-256-GCM key per blob, the plaintext cut into
//! 64 KiB chunks so a 2 GB file streams (and can be read from any chunk).
//!
//! Layout of the ciphertext (no header; the key and the plaintext size travel in the message):
//!
//! ```text
//! chunk i = AES-256-GCM(key, nonce = 0u32 ‖ i as u64 BE, aad = [last as u8], plaintext_i)
//! ```
//!
//! Every chunk but the last holds exactly `CHUNK` plaintext bytes (so `CHUNK + 16` on the
//! wire); the last holds 0 < n <= `CHUNK` bytes (an empty file is one empty chunk, 16 bytes).
//! The nonce carries the index, so chunks cannot be reordered or duplicated; the `last` flag
//! in the AAD means a truncated blob (final chunk dropped) or an extended one fails to open.
//! The key is used for exactly one blob, so (key, nonce) pairs never repeat.
//! `cipher_len(size)` is therefore a function of the plaintext size alone.

use std::io::{self, Read, Write};

use aes_gcm::aead::{Aead, KeyInit, Payload};
use aes_gcm::{Aes256Gcm, Nonce};
use rand::RngCore;

pub const CHUNK: usize = 64 * 1024;
const TAG: usize = 16;
/// Largest file we accept, in bytes.
pub const MAX_FILE: u64 = 2 * 1024 * 1024 * 1024;

pub fn new_key() -> [u8; 32] {
    let mut k = [0u8; 32];
    rand::rngs::OsRng.fill_bytes(&mut k);
    k
}

/// Ciphertext length for `plain` bytes of plaintext.
pub fn cipher_len(plain: u64) -> u64 {
    plain + TAG as u64 * plain.div_ceil(CHUNK as u64).max(1)
}

fn nonce(index: u64) -> [u8; 12] {
    let mut n = [0u8; 12];
    n[4..].copy_from_slice(&index.to_be_bytes());
    n
}

fn bad(msg: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, msg.to_string())
}

/// Reads until `buf` is full or EOF; returns how many bytes it got.
fn fill(r: &mut impl Read, buf: &mut [u8]) -> io::Result<usize> {
    let mut n = 0;
    while n < buf.len() {
        match r.read(&mut buf[n..]) {
            Ok(0) => break,
            Ok(k) => n += k,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
            Err(e) => return Err(e),
        }
    }
    Ok(n)
}

/// Encrypts `r` into `w`; returns the plaintext length. `progress` gets plaintext bytes done.
pub fn encrypt_stream(
    key: &[u8; 32],
    mut r: impl Read,
    mut w: impl Write,
    mut progress: impl FnMut(u64),
) -> io::Result<u64> {
    let cipher = Aes256Gcm::new_from_slice(key).map_err(|_| bad("key"))?;
    let mut cur = vec![0u8; CHUNK];
    let mut next = vec![0u8; CHUNK];
    let mut cur_n = fill(&mut r, &mut cur)?;
    let mut total = 0u64;
    let mut index = 0u64;
    loop {
        let next_n = if cur_n == CHUNK { fill(&mut r, &mut next)? } else { 0 };
        let last = next_n == 0;
        let ct = cipher
            .encrypt(Nonce::from_slice(&nonce(index)), Payload { msg: &cur[..cur_n], aad: &[last as u8] })
            .map_err(|_| bad("encrypt"))?;
        w.write_all(&ct)?;
        total += cur_n as u64;
        if total > MAX_FILE {
            return Err(bad("file larger than 2 GB"));
        }
        progress(total);
        if last {
            break;
        }
        std::mem::swap(&mut cur, &mut next);
        cur_n = next_n;
        index += 1;
    }
    w.flush()?;
    Ok(total)
}

/// Decrypts `r` into `w`; returns the plaintext length. Fails on any tampering, reordering or
/// truncation. Plaintext is written chunk by chunk as it verifies, so on an error the output
/// holds a verified prefix at most (callers write to a temp file and rename on success).
pub fn decrypt_stream(
    key: &[u8; 32],
    mut r: impl Read,
    mut w: impl Write,
    mut progress: impl FnMut(u64),
) -> io::Result<u64> {
    let cipher = Aes256Gcm::new_from_slice(key).map_err(|_| bad("key"))?;
    let size = CHUNK + TAG;
    let mut cur = vec![0u8; size];
    let mut next = vec![0u8; size];
    let mut cur_n = fill(&mut r, &mut cur)?;
    if cur_n < TAG {
        return Err(bad("blob too short"));
    }
    let mut total = 0u64;
    let mut index = 0u64;
    loop {
        let next_n = if cur_n == size { fill(&mut r, &mut next)? } else { 0 };
        let last = next_n == 0;
        let pt = cipher
            .decrypt(Nonce::from_slice(&nonce(index)), Payload { msg: &cur[..cur_n], aad: &[last as u8] })
            .map_err(|_| bad("blob does not authenticate"))?;
        w.write_all(&pt)?;
        total += pt.len() as u64;
        progress(total);
        if last {
            break;
        }
        if next_n < TAG {
            return Err(bad("blob too short"));
        }
        std::mem::swap(&mut cur, &mut next);
        cur_n = next_n;
        index += 1;
    }
    w.flush()?;
    Ok(total)
}

pub fn encrypt_bytes(key: &[u8; 32], data: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(cipher_len(data.len() as u64) as usize);
    encrypt_stream(key, data, &mut out, |_| {}).expect("in-memory encryption cannot fail");
    out
}

pub fn decrypt_bytes(key: &[u8; 32], data: &[u8]) -> io::Result<Vec<u8>> {
    let mut out = Vec::with_capacity(data.len());
    decrypt_stream(key, data, &mut out, |_| {})?;
    Ok(out)
}

#[cfg(test)]
mod tests;

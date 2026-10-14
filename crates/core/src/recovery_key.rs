//! The recovery key: the same 32-byte secret as the 24 recovery words, written as one line to paste.
//!
//! The words are BIP39 over 32 bytes of entropy (`identity::generate`), and everything (the identity, the storage
//! keys) is derived from the words, so the key round-trips to the *identical* words and restores the same account.
//!
//! Format: `tlkey-` + base58( version `0x01` ‖ entropy (32) ‖ checksum (4) ), checksum = the first 4 bytes of
//! `BLAKE3::derive_key("tinline/recovery-key/v1", version ‖ entropy)`. The checksum catches a mistyped or missing
//! character before anything is restored. It is exactly as powerful as the words: same care, same warnings.
//!
//! Free functions (no `Node`): platforms convert before calling `restore_identity` / `did_of_phrase`, and convert
//! the words from `recovery_phrase` to show the key.

use zeroize::Zeroizing;

use crate::error::Error;

const PREFIX: &str = "tlkey-";
const VERSION: u8 = 1;
const CONTEXT: &str = "tinline/recovery-key/v1";

fn checksum(body: &[u8]) -> [u8; 4] {
    let h = blake3::derive_key(CONTEXT, body);
    [h[0], h[1], h[2], h[3]]
}

/// The recovery key for a recovery phrase (`BadPhrase` if the words are not a valid 24-word phrase).
#[uniffi::export]
pub fn recovery_key_from_phrase(phrase: String) -> Result<String, Error> {
    let phrase = Zeroizing::new(phrase);
    let m = identity::Mnemonic::parse(normalize_words(&phrase).as_str()).map_err(|_| Error::BadPhrase)?;
    let entropy = Zeroizing::new(m.to_entropy());
    if entropy.len() != 32 {
        return Err(Error::BadPhrase);
    }
    let mut body = Zeroizing::new(Vec::with_capacity(37));
    body.push(VERSION);
    body.extend_from_slice(&entropy);
    let sum = checksum(&body);
    body.extend_from_slice(&sum);
    Ok(format!("{PREFIX}{}", bs58::encode(body.as_slice()).into_string()))
}

/// Whether `input` is meant as a recovery key rather than words (it starts with `tlkey-`), so a platform can say
/// "check the key" or "check the words" when it doesn't work.
#[uniffi::export]
pub fn is_recovery_key(input: String) -> bool {
    input.trim().to_ascii_lowercase().starts_with(PREFIX)
}

/// The recovery phrase for a recovery key or for recovery words, normalised (lower case, single spaces), ready
/// for `restore_identity` / `did_of_phrase`. `BadPhrase` if it is neither (a key with a wrong character fails its
/// checksum here).
#[uniffi::export]
pub fn recovery_phrase_from_input(input: String) -> Result<String, Error> {
    let input = Zeroizing::new(input);
    let t = input.trim();
    if !is_recovery_key(t.to_string()) {
        let words = normalize_words(t);
        identity::Mnemonic::parse(words.as_str()).map_err(|_| Error::BadPhrase)?;
        return Ok(words.to_string());
    }
    // Base58 is case-sensitive: only the prefix may differ in case. Spaces or line breaks from copying are dropped.
    let raw: String = t[PREFIX.len()..].chars().filter(|c| !c.is_whitespace()).collect();
    let body = Zeroizing::new(bs58::decode(raw).into_vec().map_err(|_| Error::BadPhrase)?);
    if body.len() != 1 + 32 + 4 || body[0] != VERSION {
        return Err(Error::BadPhrase);
    }
    let (data, sum) = body.split_at(33);
    if checksum(data) != sum {
        return Err(Error::BadPhrase);
    }
    let m = identity::Mnemonic::from_entropy(&data[1..]).map_err(|_| Error::BadPhrase)?;
    Ok(m.to_string())
}

fn normalize_words(s: &str) -> Zeroizing<String> {
    Zeroizing::new(s.split_whitespace().map(str::to_lowercase).collect::<Vec<_>>().join(" "))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn phrase() -> String {
        identity::generate().1.to_string()
    }

    #[test]
    fn key_round_trips_to_the_identical_words() {
        for _ in 0..50 {
            let p = phrase();
            let k = recovery_key_from_phrase(p.clone()).unwrap();
            assert!(k.starts_with(PREFIX), "{k}");
            assert!(k.len() < 60, "short enough to paste on one line: {} chars", k.len());
            assert_eq!(recovery_phrase_from_input(k.clone()).unwrap(), p);
            // The same account: identical DID from the words and from the key.
            let a = identity::recover(&p).unwrap().did().to_string();
            let b = identity::recover(&recovery_phrase_from_input(k).unwrap()).unwrap().did().to_string();
            assert_eq!(a, b);
        }
    }

    #[test]
    fn words_pass_through_normalised() {
        let p = phrase();
        let messy = format!("  {}\n", p.to_uppercase().replace(' ', "   "));
        assert_eq!(recovery_phrase_from_input(messy).unwrap(), p);
        assert_eq!(recovery_key_from_phrase(p.to_uppercase()).unwrap(), recovery_key_from_phrase(p).unwrap());
    }

    #[test]
    fn a_wrong_or_missing_character_is_caught() {
        let k = recovery_key_from_phrase(phrase()).unwrap();
        let alphabet = "123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz";
        let body: Vec<char> = k[PREFIX.len()..].chars().collect();
        let mut caught = 0;
        let mut tried = 0;
        for i in 0..body.len() {
            for c in alphabet.chars().filter(|c| *c != body[i]).take(3) {
                let mut b = body.clone();
                b[i] = c;
                tried += 1;
                caught += usize::from(recovery_phrase_from_input(format!("{PREFIX}{}", b.iter().collect::<String>())).is_err());
            }
            let mut short = body.clone();
            short.remove(i);
            assert!(recovery_phrase_from_input(format!("{PREFIX}{}", short.iter().collect::<String>())).is_err(), "dropped char {i}");
        }
        assert_eq!(caught, tried, "every single-character change is refused");
    }

    #[test]
    fn copy_artefacts_are_tolerated_and_garbage_is_refused() {
        let p = phrase();
        let k = recovery_key_from_phrase(p.clone()).unwrap();
        let (head, tail) = k.split_at(30);
        assert_eq!(recovery_phrase_from_input(format!("  TLKEY-{}\n{} ", &head[PREFIX.len()..], tail)).unwrap(), p);
        assert!(is_recovery_key(" tlkey-abc".into()) && !is_recovery_key("abandon ability".into()));
        for bad in ["", "tlkey-", "tlkey-0OIl", "hello world", "tlkey-3mJr7AoUXx2Wqd"] {
            assert!(recovery_phrase_from_input(bad.into()).is_err(), "{bad:?}");
        }
        assert!(recovery_key_from_phrase("not twenty four words".into()).is_err());
    }
}

//! Small helpers about contacts and contact cards: display names, safety numbers, card finding.

use super::*;

impl From<&StoredContact> for Contact {
    fn from(c: &StoredContact) -> Self {
        Contact {
            did: c.did.clone(),
            name: c.name.clone(),
            device: c
                .devices
                .first()
                .and_then(|d| PublicKey::from_bytes(&d.device).ok())
                .map(|k| k.to_string())
                .unwrap_or_default(),
            added_at: c.added_at,
            alias: c.alias.clone(),
            verified: c.verified,
            lost_touch: false,
        }
    }
}

/// What the UI should call this contact: our alias if any, else their own name.
pub fn display_name(c: &StoredContact) -> String {
    c.alias.clone().unwrap_or_else(|| c.name.clone())
}

/// The safety number for the pair: each side's 30 digits come from its own key, and the two
/// halves are joined in sorted order, so both phones compute the same string.
pub fn safety_number(mine: &str, theirs: &str) -> Result<String, Error> {
    let half = |did: &str| -> Result<String, Error> {
        let key = identity::public_key_from_did(did).ok_or_else(|| Error::Protocol("not a valid DID".into()))?;
        let mut h = Sha512::new().chain_update(b"tinline-safety-v1").chain_update(key).finalize();
        // Iterated, as a (cheap) brake on grinding keys whose digits look alike.
        for _ in 0..5200 {
            h = Sha512::new().chain_update(h).chain_update(key).finalize();
        }
        Ok(h[..30]
            .as_chunks::<5>()
            .0
            .iter()
            .map(|c| format!("{:05}", c.iter().fold(0u64, |n, b| n << 8 | *b as u64) % 100_000))
            .collect())
    };
    let (a, b) = (half(mine)?, half(theirs)?);
    let all = if a <= b { a + &b } else { b + &a };
    Ok(all.as_bytes().chunks(5).map(|g| std::str::from_utf8(g).unwrap_or("")).collect::<Vec<_>>().join(" "))
}

/// The first word of `text` that looks like a contact card (`OSVC2:` or `osvc1.`), so a card
/// pasted with a greeting around it still works.
pub fn find_card(text: &str) -> Option<String> {
    text.split(|c: char| c.is_whitespace() || matches!(c, '<' | '>' | '"' | '\'' | '(' | ')'))
        .map(|w| w.trim_end_matches(['.', ',', ';', '!', '?']))
        .find(|w| {
            w.get(..6).is_some_and(|h| h.eq_ignore_ascii_case("OSVC2:") || h.eq_ignore_ascii_case("osvc1."))
                && w.len() > 20
        })
        .map(str::to_string)
}

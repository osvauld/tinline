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
    let body: Vec<char> = k.get(PREFIX.len()..).unwrap().chars().collect();
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
    assert_eq!(recovery_phrase_from_input(format!("  TLKEY-{}\n{} ", head.get(PREFIX.len()..).unwrap(), tail)).unwrap(), p);
    assert!(is_recovery_key(" tlkey-abc".into()) && !is_recovery_key("abandon ability".into()));
    for bad in ["", "tlkey-", "tlkey-0OIl", "hello world", "tlkey-3mJr7AoUXx2Wqd"] {
        assert!(recovery_phrase_from_input(bad.into()).is_err(), "{bad:?}");
    }
    assert!(recovery_key_from_phrase("not twenty four words".into()).is_err());
}

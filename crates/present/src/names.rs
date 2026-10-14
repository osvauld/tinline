//! Names: first word, initials, and the name shown for a contact.

/// First word of a name ("Ann Lee" -> "Ann"); the whole name if it has no words.
#[uniffi::export]
pub fn first_name(name: String) -> String {
    first_word(&name).to_string()
}

/// Borrowing form for Rust callers.
pub fn first_word(name: &str) -> &str {
    name.split_whitespace().next().unwrap_or(name)
}

/// Up to two characters for an avatar: the first letter or digit of the first and the last word,
/// so "Lena (work)" is "LW" and "Ann Marie Lee" is "AL". "?" when there is none.
#[uniffi::export]
pub fn initials(name: String) -> String {
    let w: Vec<char> = name
        .split_whitespace()
        .filter_map(|w| w.chars().find(|c| c.is_alphanumeric()))
        .collect();
    match (w.first(), w.last()) {
        (Some(a), Some(b)) if w.len() > 1 => format!("{}{}", a.to_uppercase(), b.to_uppercase()),
        (Some(a), _) => a.to_uppercase().to_string(),
        _ => "?".into(),
    }
}

/// The name to show for a contact: the alias we gave them if it has text, else what they call
/// themselves, else "Unnamed".
#[uniffi::export]
pub fn display_name(alias: Option<String>, name: String) -> String {
    match alias {
        Some(a) if !a.trim().is_empty() => a,
        _ if !name.trim().is_empty() => name,
        _ => "Unnamed".into(),
    }
}

#[cfg(test)]
mod tests;

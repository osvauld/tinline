//! Conservative diagnostic guard. A true result is a cached-source-byte candidate,
//! NOT proof of exact PCM/byte lineage (compression/fade tails require a future API).
pub fn candidate(
    audible: bool,
    idle: bool,
    level: u8,
    before_lost: u64,
    after_lost: u64,
    before_concealed: u64,
    after_concealed: u64,
) -> bool {
    audible
        && !idle
        && level <= 127
        && before_lost == after_lost
        && before_concealed == after_concealed
}

#[cfg(test)]
mod tests;

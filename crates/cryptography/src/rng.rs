//! The one place that reads the OS random source; everything else calls these.

/// Fills `buf` from the OS CSPRNG. Panics only if the OS has no entropy source at all.
pub fn fill(buf: &mut [u8]) {
    getrandom::fill(buf).expect("OS random source unavailable");
}

/// `N` fresh random bytes.
pub fn bytes<const N: usize>() -> [u8; N] {
    let mut b = [0u8; N];
    fill(&mut b);
    b
}

pub fn u64() -> u64 {
    u64::from_le_bytes(bytes())
}

#[cfg(test)]
mod tests;

//! Fixtures shared by the engine tests (`tests/world.rs`) and the ranking tests (`rank/tests.rs`).

use identity::Identity;

/// The tests' wall-clock origin (unix ms).
pub(crate) const T0: u64 = 1_800_000_000_000;

/// A deterministic synthetic account from a one-byte seed.
pub(crate) fn ident(seed: u8) -> Identity {
    let m = identity::Mnemonic::from_entropy(&[seed; 32]).unwrap();
    identity::recover(&m.to_string()).unwrap()
}

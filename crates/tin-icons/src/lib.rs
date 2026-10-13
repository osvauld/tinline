//! Tinline line icons, generated from the mark's geometry.
//!
//! One Rust source draws every icon (`icons`), using the mark's construction (`draw`, from MARK-MATH.md) and the
//! app's smoothed corners (`shape`, a port of the playground's shape.ts), on a checked kurbo path (`path`).
//! `emit` writes the same glyphs for iced (Rust), Compose (Kotlin) and the playground (TypeScript), plus a contact
//! sheet. Run `cargo run -p tin-icons --bin tin-icons-gen` after changing an icon; a test fails if the
//! generated files are stale.

pub mod draw;
pub mod emit;
pub mod icons;
pub mod path;
pub mod shape;

#[cfg(test)]
mod tests;

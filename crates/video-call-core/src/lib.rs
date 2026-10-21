//! The platform-free part of 1:1 video calls (design: `docs/video.md`, `docs/design/video-adaptation.md`).
//!
//! What a call's video decides and shows, without a connection, a thread or a codec library:
//! the stats record the apps read ([`stats`]), the session-only developer controls ([`tuning`]),
//! the receiver-side adaptation policy ([`adapt`]), the sharper-video rules ([`hd`]), the stats
//! for nerds text ([`nerds`]), payload counters ([`traffic`]), path classification ([`path`]) and
//! camera frame geometry ([`frames`]). p2pcore owns the call runtime (tasks, codec threads, the
//! iroh connection) and the UniFFI surface; it declares these records to UniFFI.

pub mod adapt;
pub mod frames;
pub mod hd;
pub mod nerds;
pub mod path;
pub mod stats;
pub mod traffic;
pub mod tuning;

pub use adapt::{VideoBudget, VideoSendCap};
pub use nerds::VideoStatRow;
pub use stats::{VideoLive, VideoStats};
pub use tuning::{LocalCodecs, VideoTuning};

/// Why a tuning was refused.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum Error {
    #[error("{0}")]
    Invalid(String),
}

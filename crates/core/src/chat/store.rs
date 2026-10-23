//! The chat records in the sealed vault file (`chat.redb`) are `chat_core::store`, re-exported
//! here; this declares `PictureDownload` to UniFFI.

pub use chat_core::store::*;

// The enum lives with the vault setting (`chat_core::store`); this declares it to UniFFI.
/// Settings > Storage "Download pictures": when attachments under the size limit download by themselves.
#[uniffi::remote(Enum)]
pub enum PictureDownload {
    /// On Wi-Fi and on mobile data.
    Always,
    /// Only while the connection is not metered; on mobile data they wait for a tap.
    Wifi,
    /// Nothing downloads until it is tapped.
    Tap,
}

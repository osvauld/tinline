//! Device linking: why a link failed, and whether scanned text is a device-link code.

/// The core's link failure tokens, as a semantic enum.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum LinkFailure {
    Expired,
    Cancelled,
    Rejected,
    BadProof,
    Mismatch,
    Timeout,
    Net,
    Locked,
    Busy,
    BadGrant,
    AccountExists,
    /// Any other token; the core's own detail text may say more.
    Other,
}

/// Which failure a core reason token means.
#[uniffi::export]
pub fn link_failure(reason: String) -> LinkFailure {
    match reason.as_str() {
        "expired" => LinkFailure::Expired,
        "cancelled" => LinkFailure::Cancelled,
        "rejected" => LinkFailure::Rejected,
        "bad_proof" => LinkFailure::BadProof,
        "mismatch" => LinkFailure::Mismatch,
        "timeout" => LinkFailure::Timeout,
        "net" => LinkFailure::Net,
        "locked" => LinkFailure::Locked,
        "busy" => LinkFailure::Busy,
        "bad_grant" => LinkFailure::BadGrant,
        "account_exists" => LinkFailure::AccountExists,
        _ => LinkFailure::Other,
    }
}

/// What happened and what to do. `this_device` is "phone" or "computer". For an unknown token the
/// core's `detail` is used when it has text.
#[uniffi::export]
pub fn link_fail_text(reason: String, detail: Option<String>, this_device: String) -> String {
    match link_failure(reason) {
        LinkFailure::Expired => "The code expired. Try again.".into(),
        LinkFailure::Cancelled => "Cancelled on the other device.".into(),
        LinkFailure::Rejected => "That QR code was already used or isn\u{2019}t valid. Make a fresh one and try again.".into(),
        LinkFailure::BadProof => "The other device didn\u{2019}t accept that QR code. Make a fresh one and try again.".into(),
        LinkFailure::Mismatch => "These two devices don\u{2019}t belong to the same account, so nothing was shared.".into(),
        LinkFailure::Timeout => "The other device didn\u{2019}t answer in time. Keep both open and try again.".into(),
        LinkFailure::Net => "Couldn\u{2019}t reach the other device. Check that both are online.".into(),
        LinkFailure::Locked => "The other device is locked. Unlock Tinline there and try again.".into(),
        LinkFailure::Busy => "Tinline was busy with something else. Try again in a moment.".into(),
        LinkFailure::BadGrant => "The other device sent something this app couldn\u{2019}t read. Update Tinline on both and try again.".into(),
        LinkFailure::AccountExists => format!("This account is already on this {this_device}."),
        LinkFailure::Other => detail.filter(|d| !d.trim().is_empty()).unwrap_or_else(|| "Couldn\u{2019}t link. Try again.".into()),
    }
}

/// What a scanned or pasted text turned out to be.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum QrKind {
    DeviceLink,
    /// A contact code (`OSVC...`), which belongs in Add contact.
    ContactCode,
    Unknown,
}

/// Classifies scanned text.
#[uniffi::export]
pub fn link_qr_kind(text: String) -> QrKind {
    let t = text.trim();
    if t.starts_with("OSVL1:") {
        QrKind::DeviceLink
    } else if t.starts_with("OSVC") {
        QrKind::ContactCode
    } else {
        QrKind::Unknown
    }
}

/// `None` if `text` is a device-link code; otherwise why not.
#[uniffi::export]
pub fn link_qr_problem(text: String) -> Option<String> {
    match link_qr_kind(text) {
        QrKind::DeviceLink => None,
        QrKind::ContactCode => Some(
            "That\u{2019}s a contact code, not a device-link code. Use Add contact for those."
                .into(),
        ),
        QrKind::Unknown => Some("That isn\u{2019}t a Tinline device-link code.".into()),
    }
}

#[cfg(test)]
mod tests;

package com.osvauld.p2p

/**
 * Entry points for design elements whose data does not exist in the core yet. The screens, sheets
 * and dialogs are built; flip a flag when the data behind it is wired.
 *
 * - [rename]: needs `Contact.alias` and a `renameContact(did, alias)` core call.
 * - [verify]: needs `safetyNumber(did) -> String` (12 groups of 5 digits) and `Contact.verified` +
 *   `setVerified(did, bool)`; also shows the "Verified" pill on the contact page.
 * - [availability]: needs `setAvailable(bool)` / `setAvailableUntil(epochMs)` and a state to read
 *   back; enables the tappable status pill, the sheet, the Settings toggle and the banner.
 * - [history]: needs a call log (`recentCalls() -> List<CallRecord>`); fills the Recent section on
 *   Home, the per-contact history, the sub-lines under contact names and missed-call red.
 */
object Features {
    const val rename = false
    const val verify = false
    const val availability = false
    const val history = false
}

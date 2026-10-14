//! Wire messages and the length-prefixed JSON frames.

use super::*;

// ---- messages ----------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "t", rename_all = "snake_case")]
pub enum Msg {
    ContactHello {
        attestation: SignedAttestation,
        name: String,
        invite: SignedBlob,
        binding: String,
        /// The message token the joiner gives the ticket owner.
        grant_for_you: SignedGrant,
        /// The call token, when the joiner lets the owner call (ticket default: yes).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        call_grant_for_you: Option<SignedGrant>,
        /// The joiner's relay right now: an unsigned routing hint so the ticket owner can call
        /// back without waiting on DNS discovery. A wrong one only costs a slower dial.
        #[serde(default)]
        relay: Option<String>,
    },
    ContactWelcome {
        attestation: SignedAttestation,
        name: String,
        /// The owner's message token for the joiner.
        grant_for_you: SignedGrant,
        /// The owner's call token for the joiner, if it may call.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        call_grant_for_you: Option<SignedGrant>,
    },
    Reject {
        reason: String,
    },
    CallHello {
        call_id: String,
        attestation: SignedAttestation,
        /// The caller's call token from the callee; a caller without one presents its message
        /// token and is refused with `CALLS_NOT_ALLOWED`.
        grant: SignedGrant,
        /// The caller's current relay, same kind of hint as in `ContactHello`.
        #[serde(default)]
        relay: Option<String>,
        /// The caller's signed `DeviceList` (see `sign_device_list`); absent from old peers.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        devices: Option<SignedBlob>,
        /// The caller can do video in this call; absent = audio only.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        video: Option<VideoOffer>,
    },
    Ringing,
    Accept {
        /// A fresh call token for the caller (renew on answer).
        renewed_grant: Option<SignedGrant>,
        /// The callee's signed `DeviceList`, sent when the caller's copy is older.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        devices: Option<SignedBlob>,
        /// Video for this call: only when the hello offered it and the callee takes it. Both
        /// present = both sides run the video streams (`docs/video.md`); else audio only.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        video: Option<VideoOffer>,
    },
    /// Caller to a sibling device of the callee: stop ringing (design: device-linking §7).
    Cancel {
        reason: CancelReason,
    },
    Decline {
        reason: String,
    },
    Busy,
    Hangup,
    /// A DID-signed block or unblock notice (`sign_block_notice`). Sent as the first message of
    /// a connection to deliver it (answered `NoticeTaken`), and as the answer to every attempt of
    /// a DID we block (call hello, contact hello, and mid-call when the block happens then).
    Notice {
        notice: SignedBlob,
    },
    /// The notice arrived and was checked; the sender stops retrying.
    NoticeTaken,
    /// Fresh tokens from the sender for us ("allow calls", a message-token renewal). Each is
    /// DID-signed by its issuer, so the connection proves nothing more; answered `TokensTaken`.
    Tokens {
        grants: Vec<SignedGrant>,
    },
    TokensTaken,
    /// Our DID-signed `DeviceList` (`sign_device_list`), pushed to a contact whenever our set of
    /// installs changes (U8). DID-signed, so the connection proves nothing more; the receiver keeps
    /// it only if it verifies for that DID, is newer than the one held and within
    /// `MAX_LIST_SEQ_AHEAD_MS` of its clock. Answered `DevicesTaken` either way (the sender stops
    /// retrying), or with the receiver's block notice.
    Devices {
        list: SignedBlob,
    },
    DevicesTaken,
    /// A group invitation or the answer to one, between two contacts over the private channel
    /// (G3). Authenticated like `CallHello`: the sender's attestation (= the connection's device)
    /// and a token the receiver issued to it; the receiver requires a message token (P6) and
    /// answers its block notice to a DID it blocks. `letter` is opaque here (the core's
    /// base64url bincode `group::invitations::Letter`). Answered `GroupLetterTaken`.
    GroupLetter {
        attestation: SignedAttestation,
        grant: SignedGrant,
        letter: String,
    },
    GroupLetterTaken,
    /// Mid-call, only once both sides offered video: the sender's camera turned on or off.
    /// Sent when the call goes active if the camera is on, then on every change.
    Video {
        camera: bool,
    },
}

/// What a device can take in a video call. Its presence means AV1 decoding (`video` crate).
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub struct VideoOffer {
    /// Largest picture (width x height) this device wants to receive; the peer's sender keeps
    /// its resolution ladder at or below it.
    pub max_pixels: u32,
}

/// The `Reject` reason a contact gets when it calls without being allowed to (no call token, or
/// one taken back): shown as is, the call never rings.
pub const CALLS_NOT_ALLOWED: &str = "calls not allowed";

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CancelReason {
    AnsweredElsewhere,
    DeclinedElsewhere,
    CallerHangup,
}

/// u32 BE length + JSON.
pub fn encode_frame(msg: &Msg) -> Vec<u8> {
    encode_json_frame(msg)
}

pub(crate) fn encode_json_frame<T: Serialize>(msg: &T) -> Vec<u8> {
    let json = serde_json::to_vec(msg).expect("msg serializes");
    let mut out = (json.len() as u32).to_be_bytes().to_vec();
    out.extend(json);
    out
}

/// `Ok(None)` = need more bytes. The length is checked before buffering the body so a peer
/// can't make us wait on (or allocate for) a huge frame.
pub fn decode_frame(buf: &[u8]) -> Result<Option<(Msg, usize)>> {
    decode_json_frame(buf)
}

pub(crate) fn decode_json_frame<T: DeserializeOwned>(buf: &[u8]) -> Result<Option<(T, usize)>> {
    decode_json_frame_max(buf, MAX_FRAME)
}

/// `decode_json_frame` with a smaller body limit (`max` <= `MAX_FRAME`).
pub(crate) fn decode_json_frame_max<T: DeserializeOwned>(
    buf: &[u8],
    max: usize,
) -> Result<Option<(T, usize)>> {
    let Some(head) = buf.first_chunk::<4>() else {
        return Ok(None);
    };
    let len = u32::from_be_bytes(*head) as usize;
    if len > max {
        return Err(Error::FrameTooLarge);
    }
    let Some(body) = buf.get(4..4 + len) else {
        return Ok(None);
    };
    let msg = serde_json::from_slice(body).map_err(|_| Error::Decode)?;
    Ok(Some((msg, 4 + len)))
}

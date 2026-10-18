//! `Node` API: placing, answering and ending the one call, and its audio (UniFFI-exported).

use super::*;

#[uniffi::export]
impl Node {
    /// Starts calling `did`; progress arrives through `on_call_state`.
    pub fn call(&self, did: String) -> Result<CallInfo, Error> {
        self.inner.clone().start_call(did, false)
    }

    /// `call`, as a video call: the callee rings as one and both logs say video, and our camera is
    /// on from the start (as `set_camera(true)` right away). Push frames as soon as the camera
    /// delivers them: they are dropped until video runs, so the camera is warm by then (docs/video.md
    /// "Starting the camera"). An app that cannot use its camera turns it off (`set_camera(false)`).
    /// With video off on this node (`set_video_enabled(false)`) it is a voice call.
    pub fn video_call(&self, did: String) -> Result<CallInfo, Error> {
        self.inner.clone().start_call(did, true)
    }

    pub fn answer(&self, call_id: String) -> Result<(), Error> {
        self.inner.command(&call_id, Cmd::Answer)
    }

    pub fn decline(&self, call_id: String) -> Result<(), Error> {
        self.inner.command(&call_id, Cmd::Decline)
    }

    pub fn hangup(&self, call_id: String) -> Result<(), Error> {
        self.inner.command(&call_id, Cmd::Hangup)
    }

    /// Ends the active call (a local hangup) and answers the waiting one `call_id`, which
    /// then becomes the current call.
    pub fn end_and_answer(&self, call_id: String) -> Result<(), Error> {
        let live = self.inner.live.lock();
        let w = live.waiting.as_ref().filter(|c| c.info.call_id == call_id).ok_or(Error::NotFound)?;
        let cur = live.call.as_ref().ok_or(Error::NotFound)?;
        w.answer_on_promote.store(true, Ordering::SeqCst);
        cur.cmd.send(Cmd::Hangup).map_err(|_| Error::NotFound)
    }

    /// The second incoming call ringing over the current call, if any. Declining it
    /// (`decline`) tells its caller `busy`; ignoring it ends it with `no_answer` after 30 s.
    pub fn waiting_call(&self) -> Option<CallInfo> {
        self.inner.live.lock().waiting.as_ref().map(|c| c.info.clone())
    }

    pub fn current_call(&self) -> Option<CallInfo> {
        self.inner.live.lock().call.as_ref().map(|c| c.info.clone())
    }

    /// Microphone PCM, 48 kHz mono, any length; sent in 20 ms frames while a call is active.
    pub fn push_mic(&self, pcm: Vec<i16>) {
        self.inner.push_mic(&pcm);
    }

    /// The next `audio::FRAME` samples to play; silence when no call is active.
    pub fn pull_speaker(&self) -> Vec<i16> {
        self.inner.pull_speaker()
    }

    /// `push_mic` as little-endian PCM16 bytes: a `ByteArray` on the Kotlin side rather than
    /// a boxed `List<Short>`, which would allocate 50 times a second.
    pub fn push_mic_pcm16(&self, pcm: Vec<u8>) {
        let samples: Vec<i16> = pcm.chunks_exact(2).map(|b| i16::from_le_bytes([b[0], b[1]])).collect();
        self.inner.push_mic(&samples);
    }

    /// `pull_speaker` as little-endian PCM16 bytes.
    pub fn pull_speaker_pcm16(&self) -> Vec<u8> {
        self.inner.pull_speaker().iter().flat_map(|s| s.to_le_bytes()).collect()
    }

    pub fn call_stats(&self) -> Option<CallStats> {
        self.inner.call_stats()
    }

    /// Replace the microphone with a sine of this frequency (or `None` to stop); a self-test
    /// that does not depend on the device having a real mic.
    pub fn set_test_tone(&self, hz: Option<f32>) {
        self.inner.live.lock().tone = hz;
    }
}

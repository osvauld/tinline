//! Test hooks over the chat runtime, for the integration tests (`dev-tools`).

use crate::node::Node;
use crate::Error;

impl Node {
    /// For tests: offers a hand-made batch for today's shard with `contact` to the validator
    /// that judges batches relayed by our other devices. `kind`: 0 = a message of the contact's
    /// signed by another account's DID key, 1 = a message of ours signed by another account's DID
    /// key, 2 = a stranger's message, validly signed by the stranger, 3 = a genuine message of
    /// ours (control). Each is written under the writer ID its author and device ID give, so only
    /// the signature (or the author) is wrong.
    #[doc(hidden)]
    pub fn forged_own_batch_for_test(&self, contact: String, kind: u8) -> Result<(), Error> {
        self.run(move |inner| async move { inner.chat.chat_forged_own_batch_for_test(contact, kind).await })
    }

    /// For tests: does the chat store hold any shard of the conversation between `me_did` and
    /// `peer_did` (`me_did == peer_did`: the "You" docs)?
    #[doc(hidden)]
    pub fn has_chat_docs_for_test(&self, me_did: &str, peer_did: &str) -> bool {
        self.open().chat.chat_has_docs_for_test(me_did, peer_did)
    }

    /// For tests (P9): our writer ID (`None` while the chat store is not open).
    #[doc(hidden)]
    pub fn writer_id_for_test(&self) -> Option<u64> {
        self.open().chat.chat_writer_id_for_test()
    }

    /// For tests (P9): every `(day, Loro peer id, op count)` in our copy of the conversation
    /// with `peer_did`, and per message `(id, peer id of the change that created it)`.
    #[doc(hidden)]
    #[allow(clippy::type_complexity)]
    pub fn chat_doc_peers_for_test(&self, peer_did: String) -> (Vec<(String, u64, i32)>, Vec<(String, u64)>) {
        let inner = self.open();
        self.block_on(async move { inner.chat.chat_doc_peers_for_test(peer_did).await }).unwrap_or_default()
    }

    /// For tests (P10): the next `n` chat writes (`chat_persist`) fail, as if the disk were full.
    #[doc(hidden)]
    pub fn fail_next_chat_writes_for_test(&self, n: u32) {
        self.open().chat.chat_fail_next_writes_for_test(n);
    }

    /// For tests (P10b): stores a message of ours to `peer_did` sent at `at_ms`, written under
    /// another writer ID (a fresh device ID), as if it had just arrived from a sibling device by
    /// own-device sync: pending for the contact, not pushed. Returns its id.
    #[doc(hidden)]
    pub fn write_as_sibling_for_test(&self, peer_did: String, text: String, at_ms: i64) -> Result<String, Error> {
        self.run(move |inner| async move { inner.chat.chat_write_as_sibling_for_test(&peer_did, text, at_ms).await })
    }

    /// For tests: like `write_as_sibling_for_test`, but filed in the shard of `day`
    /// (`YYYY-MM-DD`) whatever `at_ms` is.
    #[doc(hidden)]
    pub fn write_in_day_for_test(&self, peer_did: String, text: String, at_ms: i64, day: String) -> Result<String, Error> {
        self.run(move |inner| async move { inner.chat.chat_write_in_day_for_test(&peer_did, text, at_ms, &day).await })
    }

    /// For tests: messages written now are stamped `ms` ahead of the real clock (negative: a
    /// message written at that past time by a right clock; edits, hello days and compaction use
    /// it too). The live window and catch-up decision keep the real clock.
    #[doc(hidden)]
    pub fn chat_clock_offset_for_test(&self, ms: i64) {
        self.open().chat.chat_clock_offset_for_test(ms);
    }

    /// For tests: this device's clock is wrong by `ms`: as `chat_clock_offset_for_test`, and the
    /// live window and catch-up decision use it as well (0 sets it right again).
    #[doc(hidden)]
    pub fn chat_clock_skew_for_test(&self, ms: i64) {
        self.open().chat.chat_clock_skew_for_test(ms);
    }

    /// For tests: does the chat store hold any "You" shard?
    #[doc(hidden)]
    pub fn has_you_docs_for_test(&self) -> bool {
        let Ok(me) = self.open().me() else { return false };
        self.has_chat_docs_for_test(me.id.did(), me.id.did())
    }
}

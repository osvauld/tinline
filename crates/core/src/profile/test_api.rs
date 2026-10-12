//! Narrow Rust-only hooks for checking actual blob authorization and physical GC in tests.
use crate::{Error, Node};
impl Node {
    #[doc(hidden)]
    pub fn profile_blob_allowed_for_test(
        &self,
        device: String,
        hash: String,
    ) -> Result<bool, Error> {
        let device = proto::device_from_text(&device)?;
        let hash = hash
            .parse()
            .map_err(|_| Error::Protocol("invalid blob hash".into()))?;
        Ok(self.inner.profile_may_serve(&device, &hash))
    }
    #[doc(hidden)]
    pub fn profile_blob_present_for_test(&self, hash: String) -> Result<bool, Error> {
        let inner = self.inner.clone();
        self.block_on(async move {
            let hub = inner.chat_hub().await?;
            let hash = hash
                .parse()
                .map_err(|_| Error::Protocol("invalid blob hash".into()))?;
            Ok(hub.complete_size(&hash).await.is_some())
        })?
    }
}

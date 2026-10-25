//! Pushing our DID-signed device list to contacts: the runtime is `node_runtime::devlist`,
//! re-exported here; this module holds the test hook.

use proto::SignedDeviceList as SignedBlob;

pub use node_runtime::devlist::*;

use crate::node::Node;
use crate::Error;

impl Node {
    /// For tests: sends `list_json` (a `SignedBlob`, any content) to the contact `did` as a
    /// `Devices` push, once, bypassing the outbox. Whether one of their devices answered
    /// `DevicesTaken` (which it does whether or not it kept the list).
    #[doc(hidden)]
    pub fn push_device_list_for_test(&self, did: String, list_json: String) -> Result<bool, Error> {
        let blob: SignedBlob = serde_json::from_str(&list_json)?;
        let inner = self.open();
        self.block_on(async move {
            let ep = inner.endpoint()?;
            let devices = inner.shared.lock().state.contacts.iter().find(|c| c.did == did).map(|c| c.devices.clone()).unwrap_or_default();
            for d in devices {
                if let Ok(Outcome::Taken) = inner.ctx.deliver_list(&ep, &did, d.device, d.relay.as_deref(), &blob).await {
                    return Ok(true);
                }
            }
            Ok(false)
        })?
    }
}


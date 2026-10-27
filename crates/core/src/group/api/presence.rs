//! The online list of open groups (`docs/design/public-group.md` §3, `engine::presence`).

use super::*;

#[uniffi::export]
impl Node {
    /// Members online in an open group now: those some active host saw within the last minute
    /// and who did not hide (best effort, may lag by about 15 s). Empty for a private group.
    /// Re-read after `GroupEvents::on_group_presence`. Blocking.
    pub fn group_online(&self, group_id: String) -> Result<Vec<OnlineMember>, Error> {
        let slot = self.open();
        let core = slot.group.group_core()?;
        Ok(slot.group.group_online_members(&core, &group_id)?)
    }

    /// Show this account as online in open groups (default) or not. A local setting of this
    /// device, not a record: hiding leaves no history. Hosts learn it when we reconnect, which
    /// happens at once.
    pub fn group_set_presence_visible(&self, visible: bool) -> Result<(), Error> {
        let slot = self.open();
        let core = slot.group.group_core()?;
        Ok(slot.group.group_presence_set_visible(&core, visible)?)
    }

    /// Whether this account shows as online in open groups.
    pub fn group_presence_visible(&self) -> Result<bool, Error> {
        Ok(self.open().group.group_core()?.presence.visible())
    }
}

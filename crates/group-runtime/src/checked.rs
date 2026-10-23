//! The open-then-check pair that starts nearly every group write: the group core, and proof that
//! we may act on `gid` (a current member, or an admin).

use std::sync::Arc;

use crate::Error;
use crate::engine::{GroupCore, GroupRt};

impl GroupRt {
    /// The group core, once we are a current member of `gid`.
    pub fn member_core(self: &Arc<Self>, gid: &str) -> Result<Arc<GroupCore>, Error> {
        let core = self.group_core()?;
        self.group_member_check(&core, gid)?;
        Ok(core)
    }

    /// The group core, once we are an admin of `gid`.
    pub fn admin_core(self: &Arc<Self>, gid: &str) -> Result<Arc<GroupCore>, Error> {
        let core = self.group_core()?;
        self.group_admin_check(&core, gid)?;
        Ok(core)
    }
}

//! Host and call-host appointment: the admin permit and the device's own acceptance. Chat
//! hosting (U-series) and call hosting (U21) are separate policies with the same shape.

use super::*;

/// Which hosting a permit or acceptance is for.
#[derive(Clone, Copy)]
enum HostKind {
    Chat,
    Call,
}

impl HostKind {
    fn permit(self, host: String, host_dev: [u8; 32], active: bool) -> Body {
        match self {
            HostKind::Chat => Body::HostPermit { host, host_dev, active },
            HostKind::Call => Body::CallHostPermit { host, host_dev, active },
        }
    }

    fn accept(self, active: bool) -> Body {
        match self {
            HostKind::Chat => Body::HostAccept { active },
            HostKind::Call => Body::CallHostAccept { active },
        }
    }

    fn must_be_members(self) -> &'static str {
        match self {
            HostKind::Chat => "hosts must be members",
            HostKind::Call => "call hosts must be members",
        }
    }
}

impl Node {
    /// An admin permits (`active`) or withdraws the permit of device `device_id` of `did`.
    fn host_permit(&self, group_id: String, did: String, device_id: String, active: bool, kind: HostKind) -> Result<(), Error> {
        let dev = parse_dev(&device_id)?;
        let core = self.inner.group.admin_core(&group_id)?;
        if active && !core.groups.lock().get(&group_id).is_some_and(|l| l.state.is_member(&did)) {
            return Err(Error::Rejected(kind.must_be_members().into()));
        }
        Ok(self.inner.group.group_write(&core, &group_id, kind.permit(did, dev, active)).map(|_| ())?)
    }

    /// This device agrees to host (or stops).
    fn host_accept(&self, group_id: String, accept: bool, kind: HostKind) -> Result<(), Error> {
        self.run(|inner| async move {
            let core = inner.group.member_core(&group_id)?;
            inner.group.group_ensure(&core).await?;
            inner.group.group_write(&core, &group_id, kind.accept(accept)).map(|_| ())
        })
    }
}

#[uniffi::export]
impl Node {
    /// Admin: device `device_id` of member `did` may host. It hosts once it accepts.
    pub fn group_appoint_host(&self, group_id: String, did: String, device_id: String) -> Result<(), Error> {
        self.host_permit(group_id, did, device_id, true, HostKind::Chat)
    }

    /// Admin: device `device_id` of `did` stops hosting. No rotation follows (D11: rotation is
    /// periodic only, `docs/rotation.md`).
    pub fn group_revoke_host(&self, group_id: String, did: String, device_id: String) -> Result<(), Error> {
        self.host_permit(group_id, did, device_id, false, HostKind::Chat)
    }

    /// This device agrees to host (or stops hosting). Hosting needs this and an admin's permit.
    pub fn group_accept_host(&self, group_id: String, accept: bool) -> Result<(), Error> {
        self.host_accept(group_id, accept, HostKind::Chat)
    }

    /// Admin: device `device_id` of member `did` may host the group's CALLS (U21; separate
    /// from chat hosting). It hosts calls once it accepts (`group_accept_call_host`).
    pub fn group_appoint_call_host(&self, group_id: String, did: String, device_id: String) -> Result<(), Error> {
        self.host_permit(group_id, did, device_id, true, HostKind::Call)
    }

    /// Admin: device `device_id` of `did` stops hosting calls. A call it hosts moves to a
    /// backup (members redial, G6 §5).
    pub fn group_revoke_call_host(&self, group_id: String, did: String, device_id: String) -> Result<(), Error> {
        self.host_permit(group_id, did, device_id, false, HostKind::Call)
    }

    /// This device agrees to host the group's calls (or stops). Needs this and an admin's
    /// call-host permit. Hosting exposes this device's group endpoint to every caller.
    pub fn group_accept_call_host(&self, group_id: String, accept: bool) -> Result<(), Error> {
        self.host_accept(group_id, accept, HostKind::Call)
    }
}

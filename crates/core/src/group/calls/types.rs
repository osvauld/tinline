//! The group-call records, declared to UniFFI (the runtime's are plain), and the app's
//! callback.

use std::sync::Arc;

use group_runtime::calls::*;

#[uniffi::remote(Enum)]
pub enum GroupCallEventKind {
    /// A call began in the group (`group_calls` lists it; `group_call_join` joins).
    Announced,
    /// A call of the group ended.
    Ended,
    /// `group_call_start` found no call host that agreed (`detail`: why).
    StartFailed,
    /// This device is in the call (also after a move to a backup host).
    Joined,
    /// Who is in the call changed (`participants`: DIDs).
    RosterChanged,
    /// The call host was lost or is handing over; moving to the next one.
    Reconnecting,
    /// This device left the call (`detail`: why: left, ended, removed, host lost, ...).
    Left,
    /// This device hosts the call now.
    HostingStarted,
    /// This device stopped hosting it (`detail`: why).
    HostingEnded,
    /// Hosting limited by battery or heat (one speaker, no new callers), or back to normal.
    HostingLimited,
    HostingUnlimited,
    /// The hosting device moved to metered data.
    MeteredData,
}

#[uniffi::remote(Record)]
pub struct GroupCallEvent {
    pub kind: GroupCallEventKind,
    pub group_id: String,
    pub call_id: String,
    /// `RosterChanged`: the participants' DIDs.
    pub participants: Vec<String>,
    pub detail: String,
}

/// Registered with `Node::set_group_call_events`. Called on core threads: do not block.
#[uniffi::export(rust, foreign)]
pub trait GroupCallEvents: Send + Sync {
    fn on_group_call(&self, event: GroupCallEvent);
}

#[uniffi::remote(Record)]
pub struct GroupCall {
    pub group_id: String,
    pub call_id: String,
    pub host_did: String,
    /// Hex group device ID of the host, when it is a current call host.
    pub host_device_id: Option<String>,
    pub starter_did: String,
    pub started_at: u64,
    /// Increments when a backup takes the call over.
    pub host_gen: u32,
    pub hosted_here: bool,
    /// This device is in it.
    pub joined: bool,
}

/// What kind of device this is, for call hosting (§2.4).
#[uniffi::remote(Enum)]
pub enum GroupCallDeviceClass {
    Desktop,
    Headless,
    Phone,
}

/// Android `PowerManager.THERMAL_STATUS_*` order; `None` where the platform has no reading.
#[uniffi::remote(Enum)]
pub enum GroupCallThermal {
    None,
    Light,
    Moderate,
    Severe,
    Critical,
}

/// The platform's local conditions for call hosting (§2.4), `Node::group_call_set_conditions`.
#[uniffi::remote(Record)]
pub struct GroupCallConditions {
    pub device_class: GroupCallDeviceClass,
    /// On mains or charging.
    pub plugged: bool,
    /// 0..=100; 100 for a machine without a battery.
    pub battery_pct: u8,
    pub metered: bool,
    /// The user allowed hosting on metered data.
    pub metered_allowed: bool,
    pub thermal: GroupCallThermal,
}

/// The group call this device takes part in, for the apps' audio and service (GA-A1).
#[uniffi::remote(Record)]
pub struct GroupCallHere {
    pub group_id: String,
    pub call_id: String,
    /// This device's user is in it (joining, joined or reconnecting): run the mic and speaker.
    pub in_call: bool,
    /// This device hosts it (keep the process and network up).
    pub hosting: bool,
}

/// Hands the runtime's events to the app's `GroupCallEvents`.
pub(crate) struct GroupCallEventsAdapter(pub(crate) Arc<dyn GroupCallEvents>);

impl group_runtime::calls::GroupCallSink for GroupCallEventsAdapter {
    fn on_group_call(&self, event: GroupCallEvent) {
        self.0.on_group_call(event);
    }
}

//! Our call-host status and the reasons to decline a request.

use super::*;

pub(super) fn status_class(c: Class) -> DeviceClass {
    match c {
        Class::Desktop => DeviceClass::Desktop,
        Class::Headless => DeviceClass::Headless,
        Class::Phone => DeviceClass::Phone,
    }
}

pub(super) fn status_thermal(t: Thermal) -> ThermalLevel {
    match t {
        Thermal::None => ThermalLevel::None,
        Thermal::Light => ThermalLevel::Light,
        Thermal::Moderate => ThermalLevel::Moderate,
        Thermal::Severe => ThermalLevel::Severe,
        Thermal::Critical => ThermalLevel::Critical,
    }
}

pub(super) fn power_of(c: &Cond) -> PowerState {
    match (c.class, c.plugged) {
        (Class::Phone, true) => PowerState::Charging(c.battery.min(100)),
        (_, true) if c.battery >= 100 => PowerState::Mains,
        (_, true) => PowerState::Charging(c.battery.min(100)),
        (_, false) => PowerState::Battery(c.battery.min(100)),
    }
}

/// This device's `CallHostStatus` for `gid` (`issued_ms` 0), or None while the platform has not
/// reported its conditions (`fed`): an unknown device does not advertise (GA-A1).
pub(super) fn own_status(gid: &str, host_ref: HostRef, fed: Option<Cond>, busy: bool) -> Option<CallHostStatus> {
    let cond = fed?;
    Some(CallHostStatus {
        v: GCALL_VERSION,
        gid: gid.to_string(),
        host_ref,
        class: status_class(cond.class),
        power: power_of(&cond),
        network: if cond.metered { NetworkKind::Metered } else { NetworkKind::Unmetered },
        thermal: status_thermal(cond.thermal),
        busy: busy || phone_policy(&cond) != PhoneAction::Advertise,
        load: 0,
        max_participants: cond.cap().clamp(2, MAX_PARTICIPANTS) as u8,
        issued_ms: 0,
        ttl_ms: STATUS_TTL_MS,
    })
}

/// Why this device declines a `CallHostRequest`, if it does.
pub(super) fn decline_reason(paused: bool, busy: bool, fed: Option<Cond>) -> Option<&'static str> {
    if paused {
        return Some("call hosting paused");
    }
    if busy {
        return Some("busy");
    }
    let Some(cond) = fed else { return Some("conditions not known yet") };
    if cond.class == Class::Phone {
        // §2.4 / Q11: a phone hosts only while its own user is in the call.
        Some("a phone hosts only calls its user starts")
    } else if phone_policy(&cond) != PhoneAction::Advertise {
        Some("conditions")
    } else {
        None
    }
}

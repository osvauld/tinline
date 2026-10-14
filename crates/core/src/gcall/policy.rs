//! Host conditions and the phone degrade policy (§2.4), as pure functions. Values are the §8
//! recommendations (G6-D3).

/// Q1: participant caps.
pub const CAP_DESKTOP: usize = 12;
pub const CAP_PHONE_WIFI: usize = 8;
pub const CAP_PHONE_METERED: usize = 4;
/// Q2 (decided 2026-10-14): speakers forwarded. Every speaker: each member hears everyone who is
/// talking except themselves, so the host's set may hold all 12. Silent and muted members send
/// only the level byte, so this costs bandwidth only while several people talk at once.
pub const K_ALL: usize = proto::gcall::MAX_PARTICIPANTS;
/// Q2: a phone host forwards the four loudest, a safety net so hosting never takes over the
/// phone (it spends ~0.5 ms of CPU per forwarded packet; eight people all talking would be
/// ~2800 packets/s at K = 12).
pub const K_PHONE: usize = 4;
/// Q2: a host on metered data forwards only the two loudest; every forwarded speaker costs its
/// user N - 2 streams up.
pub const K_METERED: usize = 2;
/// §2.4 battery cutoffs (unplugged).
pub const BATTERY_ADVERTISE_PCT: u8 = 30;
pub const BATTERY_LIMIT_PCT: u8 = 15;
pub const BATTERY_END_PCT: u8 = 8;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Class {
    Desktop,
    Headless,
    Phone,
}

/// Android `PowerManager.THERMAL_STATUS_*` order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Thermal {
    None,
    Light,
    Moderate,
    Severe,
    Critical,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cond {
    pub class: Class,
    pub plugged: bool,
    pub battery: u8,
    pub metered: bool,
    /// The user allowed hosting on metered data.
    pub metered_override: bool,
    pub thermal: Thermal,
}

impl Cond {
    pub fn desktop() -> Self {
        Cond { class: Class::Desktop, plugged: true, battery: 100, metered: false, metered_override: false, thermal: Thermal::None }
    }
    pub fn phone(battery: u8, plugged: bool) -> Self {
        Cond { class: Class::Phone, plugged, battery, ..Self::desktop() }
    }
    /// Participant cap under these conditions (§2.4).
    pub fn cap(&self) -> usize {
        match (self.class, self.metered) {
            (Class::Phone, true) => CAP_PHONE_METERED,
            (Class::Phone, false) => CAP_PHONE_WIFI,
            _ => CAP_DESKTOP,
        }
    }
    pub fn k(&self) -> usize {
        if self.metered {
            K_METERED
        } else if self.class == Class::Phone {
            K_PHONE
        } else {
            K_ALL
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PhoneAction {
    Advertise,
    /// Stop advertising (`busy`); a running call continues.
    StopAdvertising,
    /// K = 1, refuse new Joins, show "hosting limited".
    Limited,
    /// `End{host_leaving, next_host: backups[0]}`.
    End,
}

/// The §2.4 cutoffs table. "User ends hosting" is a separate trigger.
pub fn phone_policy(c: &Cond) -> PhoneAction {
    if c.class != Class::Phone {
        return PhoneAction::Advertise;
    }
    let on_battery = !c.plugged;
    if c.thermal >= Thermal::Critical || (on_battery && c.battery < BATTERY_END_PCT) {
        PhoneAction::End
    } else if c.thermal >= Thermal::Severe || (on_battery && c.battery < BATTERY_LIMIT_PCT) {
        PhoneAction::Limited
    } else if c.thermal >= Thermal::Moderate
        || (on_battery && c.battery < BATTERY_ADVERTISE_PCT)
        || (c.metered && !c.metered_override)
    {
        PhoneAction::StopAdvertising
    } else {
        PhoneAction::Advertise
    }
}

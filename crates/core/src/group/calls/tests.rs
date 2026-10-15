use super::*;

fn href() -> HostRef {
    HostRef { holder_did: "did:key:z6Mk".into(), device_slot: 1, permit_digest: "p".into() }
}

fn conditions(class: GroupCallDeviceClass, plugged: bool, battery_pct: u8) -> GroupCallConditions {
    GroupCallConditions { device_class: class, plugged, battery_pct, metered: false, metered_allowed: false, thermal: GroupCallThermal::None }
}

#[test]
fn gcall_calls_no_advertising_or_hosting_before_the_first_conditions() {
    // GA-A1: an app that never reported its conditions must not pass for a desktop on mains.
    assert!(own_status("g", href(), None, false).is_none());
    assert_eq!(decline_reason(false, false, None), Some("conditions not known yet"));
    // Reported: a desktop on mains advertises, not busy, and hosts when asked.
    let d = Cond::from(conditions(GroupCallDeviceClass::Desktop, true, 100));
    let s = own_status("g", href(), Some(d), false).unwrap();
    assert_eq!((s.class, s.power, s.busy), (DeviceClass::Desktop, PowerState::Mains, false));
    assert_eq!(decline_reason(false, false, Some(d)), None);
    assert_eq!(decline_reason(true, false, Some(d)), Some("call hosting paused"));
    assert_eq!(decline_reason(false, true, Some(d)), Some("busy"));
    assert!(own_status("g", href(), Some(d), true).unwrap().busy);
}

#[test]
fn gcall_calls_reported_conditions_shape_the_status() {
    // A phone advertises as a phone and never hosts on request (its user starts its calls).
    let p = Cond::from(conditions(GroupCallDeviceClass::Phone, false, 80));
    let s = own_status("g", href(), Some(p), false).unwrap();
    assert_eq!((s.class, s.power, s.busy), (DeviceClass::Phone, PowerState::Battery(80), false));
    assert!(decline_reason(false, false, Some(p)).is_some());
    // Below the advertise cutoff, hot or on metered data: advertised busy.
    let low = Cond::from(conditions(GroupCallDeviceClass::Phone, false, 20));
    assert!(own_status("g", href(), Some(low), false).unwrap().busy);
    let hot = Cond::from(GroupCallConditions { thermal: GroupCallThermal::Moderate, ..conditions(GroupCallDeviceClass::Phone, true, 90) });
    assert_eq!(hot.thermal, Thermal::Moderate);
    assert!(own_status("g", href(), Some(hot), false).unwrap().busy);
    let metered = GroupCallConditions { metered: true, ..conditions(GroupCallDeviceClass::Phone, true, 90) };
    let s = own_status("g", href(), Some(metered.into()), false).unwrap();
    assert!(s.busy && s.network == NetworkKind::Metered);
    let allowed = GroupCallConditions { metered_allowed: true, ..metered };
    assert!(!own_status("g", href(), Some(allowed.into()), false).unwrap().busy);
    // A laptop on battery reports it; out-of-range percentages clamp.
    let l = Cond::from(conditions(GroupCallDeviceClass::Desktop, false, 250));
    assert_eq!(own_status("g", href(), Some(l), false).unwrap().power, PowerState::Battery(100));
}

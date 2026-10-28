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

#[test]
fn gcall_calls_refresh_local_busy_status_before_the_periodic_tick() {
    struct Quiet;
    impl node_runtime::NodeSink for Quiet {
        fn on_status(&self, _: node_runtime::NodeStatus) {}
        fn on_contacts_changed(&self) {}
        fn on_incoming_call(&self, _: node_runtime::CallInfo) {}
        fn on_call_state(&self, _: String, _: node_runtime::CallState) {}
        fn on_log(&self, _: String) {}
    }
    struct TestDir(std::path::PathBuf);
    impl Drop for TestDir {
        fn drop(&mut self) { let _ = std::fs::remove_dir_all(&self.0); }
    }
    let directory = TestDir(std::env::temp_dir().join(format!("gcall-status-{}", hex::encode(proto::new_device_secret()))));
    std::fs::create_dir(&directory.0).unwrap();
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let accounts = account_model::accounts::AccountDirs::open(directory.0.clone()).unwrap();
    let ctx = Arc::new(node_runtime::NodeCtx::new(runtime.handle().clone(), accounts, node_runtime::Shared::new(), Arc::new(Quiet)));
    let rt = Arc::new(GroupRt::new(ctx, Default::default()));
    let (core, gid, _) = crate::engine::own_content_core_for_test(identity::generate().0, 0, false);
    let host = HostRef { holder_did: core.did.clone(), ..href() };
    let now = now_ms();
    rt.gcall.set_cond(Cond::phone(20, false));
    rt.calls_publish_status(&core, &gid, host.clone(), now);
    let key = (host.holder_did.clone(), host.device_slot);
    assert!(rt.calls.st.lock().statuses[&gid][&key].1.busy);
    // Charging makes the host eligible immediately, without waiting for a refresh interval.
    rt.gcall.set_cond(Cond::phone(20, true));
    rt.calls_publish_status(&core, &gid, host.clone(), now + 1);
    let st = rt.calls.st.lock();
    let (blob, status) = &st.statuses[&gid][&key];
    assert!(!status.busy);
    assert_eq!(blob.open().unwrap(), *status);
    assert_eq!(rank::rank_admitted(&[status.clone()], &RankCtx {
        gid: &gid, call_id: "fresh", expected: 2, starter: Some(&host), now_ms: now + 1,
    }), vec![host]);
}

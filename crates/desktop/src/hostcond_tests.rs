use super::*;

fn s(kind: &str, online: Option<bool>, capacity: Option<u8>, status: Option<&str>) -> Supply {
    Supply { kind: kind.into(), online, capacity, status: status.map(Into::into), scope: None }
}

#[test]
fn no_battery_is_mains() {
    assert_eq!(power_from(&[]), (true, 100));
    assert_eq!(power_from(&[s("Mains", Some(false), None, None)]), (true, 100));
    // A wireless mouse's battery does not power the machine.
    let mouse = Supply { scope: Some("Device".into()), ..s("Battery", None, Some(10), Some("Discharging")) };
    assert_eq!(power_from(&[mouse]), (true, 100));
}

#[test]
fn laptop_power_states() {
    let on_battery = [s("Mains", Some(false), None, None), s("Battery", None, Some(57), Some("Discharging"))];
    assert_eq!(power_from(&on_battery), (false, 57));
    let charging = [s("Mains", Some(true), None, None), s("Battery", None, Some(57), Some("Charging"))];
    assert_eq!(power_from(&charging), (true, 57));
    // Held at a charge limit on AC (this machine: ADP1 online, BAT1 96 % "Not charging").
    let held = [s("Mains", Some(true), None, None), s("Battery", None, Some(96), Some("Not charging"))];
    assert_eq!(power_from(&held), (true, 100));
    // USB-C adapter only shows as a charging battery.
    assert_eq!(power_from(&[s("Battery", None, Some(40), Some("Charging"))]), (true, 40));
    // Two batteries: the lower one counts.
    let two = [s("Battery", None, Some(80), Some("Discharging")), s("Battery", None, Some(20), Some("Discharging"))];
    assert_eq!(power_from(&two), (false, 20));
}

#[test]
fn reads_a_power_supply_tree() {
    let root = std::env::temp_dir().join(format!("tinline-hostcond-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    for (dev, files) in [("ADP1", vec![("type", "Mains\n"), ("online", "0\n")]), ("BAT1", vec![("type", "Battery\n"), ("capacity", "42\n"), ("status", "Discharging\n")])] {
        std::fs::create_dir_all(root.join(dev)).unwrap();
        for (f, v) in files {
            std::fs::write(root.join(dev).join(f), v).unwrap();
        }
    }
    let c = conditions(&read_supplies(&root), false);
    let _ = std::fs::remove_dir_all(&root);
    assert_eq!((c.device_class, c.plugged, c.battery_pct, c.thermal), (GroupCallDeviceClass::Desktop, false, 42, GroupCallThermal::None));
    assert!(read_supplies(Path::new("/nonexistent/power_supply")).is_empty());
}

#[test]
fn nmcli_metered() {
    let unmetered = "GENERAL.DEVICE:wlan0\nGENERAL.STATE:100 (connected)\nGENERAL.METERED:no (guessed)\nIP4.GATEWAY:192.168.1.1\n\n\
                     GENERAL.DEVICE:lo\nGENERAL.STATE:100 (connected (externally))\nGENERAL.METERED:no (guessed)\nIP4.GATEWAY:\n\n\
                     GENERAL.DEVICE:p2p-dev-wlan0\nGENERAL.STATE:30 (disconnected)\nGENERAL.METERED:unknown\n";
    assert!(!metered_from_nmcli(unmetered));
    let hotspot = "GENERAL.DEVICE:wlan0\nGENERAL.STATE:100 (connected)\nGENERAL.METERED:yes (guessed)\nIP4.GATEWAY:172.20.10.1\n";
    assert!(metered_from_nmcli(hotspot));
    // Metered but not connected, or connected without a route out: not what we use.
    let idle = "GENERAL.DEVICE:wwan0\nGENERAL.STATE:30 (disconnected)\nGENERAL.METERED:yes\nIP4.GATEWAY:\n\n\
                GENERAL.DEVICE:eth0\nGENERAL.STATE:100 (connected)\nGENERAL.METERED:no\nIP4.GATEWAY:10.0.0.1\n";
    assert!(!metered_from_nmcli(idle));
    let vpn = "GENERAL.DEVICE:usb0\nGENERAL.STATE:100 (connected)\nGENERAL.METERED:yes\nIP4.GATEWAY:--\n";
    assert!(!metered_from_nmcli(vpn));
    assert!(!metered_from_nmcli(""));
}

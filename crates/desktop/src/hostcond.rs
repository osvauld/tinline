//! Call-hosting conditions for group calls (GA-A1, group-call-spec §2.4): this machine is a
//! desktop; power comes from `/sys/class/power_supply` (Linux; no battery = mains), metered data
//! from NetworkManager (`nmcli`) when it is there, else unmetered; no thermal reading. Pushed at
//! start and every 30 s; the core advertises no call hosting before the first push.

use std::path::Path;
use std::sync::{Arc, Weak};
use std::time::Duration;

use p2pcore::{GroupCallConditions, GroupCallDeviceClass, GroupCallThermal, Node};

const POLL: Duration = Duration::from_secs(30);
const POWER_SUPPLY: &str = "/sys/class/power_supply";

/// One entry of `/sys/class/power_supply`.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct Supply {
    /// `type`: Mains, USB, Battery, ...
    pub kind: String,
    /// `online` (adapters).
    pub online: Option<bool>,
    /// `capacity` in percent (batteries).
    pub capacity: Option<u8>,
    /// `status`: Charging, Discharging, Full, Not charging, Unknown.
    pub status: Option<String>,
    /// `scope`: Device for a peripheral's battery (mouse, headset), which does not power us.
    pub scope: Option<String>,
}

/// (plugged, battery %) from the supplies. No system battery: a desktop on mains (true, 100).
/// Plugged and not charging (full, or held below full by a charge limit) counts as mains.
pub fn power_from(supplies: &[Supply]) -> (bool, u8) {
    let batteries: Vec<&Supply> = supplies
        .iter()
        .filter(|s| s.kind == "Battery" && s.scope.as_deref() != Some("Device"))
        .collect();
    if batteries.is_empty() {
        return (true, 100);
    }
    let adapter = supplies.iter().any(|s| s.kind != "Battery" && s.online == Some(true));
    let status = |want: &[&str]| batteries.iter().any(|b| b.status.as_deref().is_some_and(|st| want.contains(&st)));
    let plugged = adapter || status(&["Charging", "Full", "Not charging"]);
    let charging = status(&["Charging"]);
    let pct = batteries.iter().filter_map(|b| b.capacity).min().unwrap_or(100).min(100);
    (plugged, if plugged && !charging { 100 } else { pct })
}

fn read_supplies(root: &Path) -> Vec<Supply> {
    let Ok(dir) = std::fs::read_dir(root) else { return vec![] };
    let field = |p: &Path, f: &str| std::fs::read_to_string(p.join(f)).ok().map(|s| s.trim().to_string());
    dir.flatten()
        .map(|e| {
            let p = e.path();
            Supply {
                kind: field(&p, "type").unwrap_or_default(),
                online: field(&p, "online").map(|v| v == "1"),
                capacity: field(&p, "capacity").and_then(|v| v.parse::<u32>().ok()).map(|v| v.min(100) as u8),
                status: field(&p, "status"),
                scope: field(&p, "scope"),
            }
        })
        .collect()
}

/// Metered from `nmcli -t -f GENERAL.DEVICE,GENERAL.STATE,GENERAL.METERED,IP4.GATEWAY dev show`:
/// any connected device with a default gateway whose METERED is "yes" / "yes (guessed)".
pub fn metered_from_nmcli(out: &str) -> bool {
    let mut metered = false;
    let (mut connected, mut gateway, mut yes) = (false, false, false);
    let mut flush = |c: &mut bool, g: &mut bool, y: &mut bool| {
        metered |= *c && *g && *y;
        (*c, *g, *y) = (false, false, false);
    };
    for line in out.lines() {
        let Some((k, v)) = line.split_once(':') else {
            if line.trim().is_empty() {
                flush(&mut connected, &mut gateway, &mut yes);
            }
            continue;
        };
        match k {
            "GENERAL.DEVICE" => flush(&mut connected, &mut gateway, &mut yes),
            "GENERAL.STATE" => connected = v.starts_with("100 "),
            "GENERAL.METERED" => yes = v.starts_with("yes"),
            "IP4.GATEWAY" | "IP6.GATEWAY" => gateway |= !v.trim().is_empty() && v.trim() != "--",
            _ => {}
        }
    }
    flush(&mut connected, &mut gateway, &mut yes);
    metered
}

fn metered() -> bool {
    let out = std::process::Command::new("nmcli")
        .args(["-t", "-f", "GENERAL.DEVICE,GENERAL.STATE,GENERAL.METERED,IP4.GATEWAY", "dev", "show"])
        .stdin(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .output();
    // nmcli exits non-zero when a listed device lacks a field; its stdout is still complete.
    out.map(|o| metered_from_nmcli(&String::from_utf8_lossy(&o.stdout))).unwrap_or(false)
}

pub fn conditions(supplies: &[Supply], metered: bool) -> GroupCallConditions {
    let (plugged, battery_pct) = power_from(supplies);
    GroupCallConditions {
        device_class: GroupCallDeviceClass::Desktop,
        plugged,
        battery_pct,
        metered,
        metered_allowed: false,
        thermal: GroupCallThermal::None,
    }
}

fn current() -> GroupCallConditions {
    let supplies = if cfg!(target_os = "linux") { read_supplies(Path::new(POWER_SUPPLY)) } else { vec![] };
    conditions(&supplies, cfg!(target_os = "linux") && metered())
}

/// Pushes the conditions now and every 30 s while the node lives (only when they change, and
/// at least every 10 minutes).
pub fn spawn(node: &Arc<Node>) {
    let weak: Weak<Node> = Arc::downgrade(node);
    let _ = std::thread::Builder::new().name("host-conditions".into()).spawn(move || {
        let mut last: Option<GroupCallConditions> = None;
        let mut since_push = 0u32;
        loop {
            let c = current();
            let Some(node) = weak.upgrade() else { return };
            if last != Some(c) || since_push >= 20 {
                node.group_call_set_conditions(c);
                last = Some(c);
                since_push = 0;
            }
            drop(node);
            since_push += 1;
            std::thread::sleep(POLL);
        }
    });
}

#[cfg(test)]
mod tests {
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
}

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
#[path = "hostcond_tests.rs"]
mod tests;

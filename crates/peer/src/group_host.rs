//! `p2p-peer group-host`: a headless host for public groups (docs/design/public-group.md §7).
//!
//!   p2p-peer --data DIR [--passphrase PASS] group-host [--name NAME] [--accept GROUP_ID]... [--join LINK]...
//!
//! Runs until SIGTERM or SIGINT, then stops the node and returns (exit code 0).

use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use p2pcore::{GroupEvents, GroupMessage, LockState, Node};

#[cfg(test)]
mod tests;

const DEFAULT_NAME: &str = "Tinline host";
const RECONCILE_EVERY: Duration = Duration::from_secs(10);
const LOG_EVERY: Duration = Duration::from_secs(60);

#[derive(Debug, Default, PartialEq)]
pub struct HostArgs {
    pub name: String,
    pub accept: Vec<String>,
    pub join: Vec<String>,
}

/// Parses the words after `group-host`.
pub fn parse_args(args: &[&str]) -> Result<HostArgs, String> {
    let mut out = HostArgs { name: DEFAULT_NAME.into(), ..Default::default() };
    let mut it = args.iter();
    while let Some(&flag) = it.next() {
        let mut value = || it.next().map(|v| v.to_string()).filter(|v| !v.is_empty()).ok_or(format!("{flag} needs a value"));
        match flag {
            "--name" => out.name = value()?,
            "--accept" => out.accept.push(value()?),
            "--join" => out.join.push(value()?),
            other => return Err(format!("group-host: unknown argument {other}")),
        }
    }
    Ok(out)
}

/// Marks that something about a group changed, so the next loop pass reconciles at once.
struct Dirty(Arc<AtomicBool>);

impl GroupEvents for Dirty {
    fn on_group_message(&self, _message: GroupMessage) {}
    fn on_group_changed(&self, _group_id: String) {
        self.0.store(true, Ordering::SeqCst);
    }
    fn on_group_presence(&self, _group_id: String) {}
    fn on_group_image(&self, _group_id: String, _message_id: String) {}
}

static STOP: AtomicBool = AtomicBool::new(false);

extern "C" fn on_signal(_: libc::c_int) {
    STOP.store(true, Ordering::SeqCst);
}

fn install_signal_handlers() {
    // SAFETY: the handler only stores to an atomic, which is async-signal-safe.
    unsafe {
        let handler = on_signal as extern "C" fn(libc::c_int) as libc::sighandler_t;
        libc::signal(libc::SIGTERM, handler);
        libc::signal(libc::SIGINT, handler);
    }
}

/// Runs the host. `start_online` is the caller's usual "start and wait for the relay" step.
pub fn run(node: &Node, passphrase: &str, args: &HostArgs, start_online: impl FnOnce(&Node) -> Result<(), String>) -> Result<(), String> {
    install_signal_handlers();
    match node.lock_state() {
        LockState::Locked => node.unlock(passphrase.to_string()).map_err(|e| e.to_string())?,
        LockState::NoIdentity => {
            // First run: a synthetic identity the owner can invite and appoint.
            node.create_identity(args.name.clone(), passphrase.to_string()).map_err(|e| e.to_string())?;
            node.commit_identity().map_err(|e| e.to_string())?;
            eprintln!("group-host: created identity \"{}\"", args.name);
        }
        LockState::Unlocked => {}
    }
    let me = node.profile().ok_or("no identity")?;
    println!("group-host: did={} device={} name={:?}", me.did, me.device, me.name);

    let dirty = Arc::new(AtomicBool::new(true));
    node.set_group_events(Arc::new(Dirty(dirty.clone())));
    start_online(node)?;
    println!("group-host: online endpoint={}", node.status().endpoint_id);

    let mut pending_logged: HashSet<String> = HashSet::new();
    let mut last_reconcile = Instant::now() - RECONCILE_EVERY;
    let mut last_log = Instant::now() - LOG_EVERY;
    while !STOP.load(Ordering::SeqCst) {
        if dirty.swap(false, Ordering::SeqCst) || last_reconcile.elapsed() >= RECONCILE_EVERY {
            last_reconcile = Instant::now();
            reconcile(node, args, &mut pending_logged);
        }
        if last_log.elapsed() >= LOG_EVERY {
            last_log = Instant::now();
            log_status(node);
        }
        std::thread::sleep(Duration::from_millis(250));
    }
    eprintln!("group-host: stopping");
    node.stop();
    Ok(())
}

fn reconcile(node: &Node, args: &HostArgs, pending_logged: &mut HashSet<String>) {
    let known: HashSet<String> = node.groups().unwrap_or_default().into_iter().map(|g| g.id).collect();
    for link in &args.join {
        match node.group_peek_invite(link.clone()) {
            Some(peek) if known.contains(&peek.group_id) => {}
            Some(peek) => match node.group_join(link.clone()) {
                Ok(id) => println!("group-host: joined {id} ({})", peek.name),
                Err(e) => eprintln!("group-host: join {} failed: {e}", peek.group_id),
            },
            None => eprintln!("group-host: --join link not understood"),
        }
    }
    for g in node.groups().unwrap_or_default() {
        let Ok(d) = node.group_detail(g.id.clone()) else { continue };
        if !d.host_requested_here {
            pending_logged.remove(&g.id);
            continue;
        }
        if args.accept.contains(&g.id) {
            match node.group_accept_host(g.id.clone(), true) {
                Ok(()) => println!("group-host: accepted host appointment for {} ({})", g.id, g.name),
                Err(e) => eprintln!("group-host: accept {} failed: {e}", g.id),
            }
        } else if pending_logged.insert(g.id.clone()) {
            println!("group-host: PENDING appointment for {} ({}); restart with --accept {}", g.id, g.name, g.id);
        }
    }
}

fn log_status(node: &Node) {
    for g in node.groups().unwrap_or_default() {
        let Ok(d) = node.group_detail(g.id.clone()) else { continue };
        let hosts_online = d.hosts.iter().filter(|h| h.online && !h.is_me).count();
        let online = node.group_online(g.id.clone()).map(|v| v.len()).unwrap_or(0);
        println!(
            "group-host: group={} name={:?} open={} active_host={} members={} other_hosts_online={} online={}",
            g.id, g.name, g.open, d.am_host, g.member_count, hosts_online, online
        );
    }
}

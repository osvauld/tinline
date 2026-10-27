use std::sync::Arc;

use parking_lot::Mutex;

use super::*;
use chat_runtime::ProfileSink;

/// The platform's profile screen: every DID it was told about.
#[derive(Default)]
struct Seen(Mutex<Vec<String>>);

impl ProfileSink for Seen {
    fn on_profile_changed(&self, did: String) {
        self.0.lock().push(did);
    }
}

fn seen(s: &Seen) -> Vec<String> {
    s.0.lock().clone()
}

#[test]
fn a_sink_of_the_open_slot_passes_events_until_the_slot_is_left() {
    let gate = Arc::new(SlotGate::new(false, None));
    gate.set(true);
    let screen = Arc::new(Seen::default());
    let sink = GatedProfile::new(screen.clone(), &gate);
    sink.on_profile_changed("a1".into());
    // The switch away: a task that kept this clone reports into nothing.
    gate.set(false);
    sink.on_profile_changed("a2".into());
    assert_eq!(seen(&screen), vec!["a1".to_string()]);
}

#[test]
fn a_sink_from_an_earlier_term_stays_shut_when_the_slot_opens_again() {
    let gate = Arc::new(SlotGate::new(false, None));
    gate.set(true);
    let screen = Arc::new(Seen::default());
    let old = GatedProfile::new(screen.clone(), &gate);
    gate.set(false);
    gate.set(true);
    let new = GatedProfile::new(screen.clone(), &gate);
    old.on_profile_changed("stale".into());
    new.on_profile_changed("fresh".into());
    assert_eq!(seen(&screen), vec!["fresh".to_string()], "only the sink installed for this term");
}

#[test]
fn a_sink_installed_while_shut_never_passes() {
    let gate = Arc::new(SlotGate::new(false, None));
    let screen = Arc::new(Seen::default());
    let sink = GatedProfile::new(screen.clone(), &gate);
    sink.on_profile_changed("x".into());
    gate.set(true);
    sink.on_profile_changed("y".into());
    assert!(seen(&screen).is_empty(), "opening starts a new term the sink is not of");
}

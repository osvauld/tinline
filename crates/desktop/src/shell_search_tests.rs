use super::*;
fn page() -> SearchResults { SearchResults { names: vec![], messages: vec![], next: None } }
#[test]
fn actual_node_search_returns_real_saved_messages_newest_first() {
    struct Quiet;
    impl p2pcore::NodeEvents for Quiet {
        fn on_status(&self, _: NodeStatus) {}
        fn on_contacts_changed(&self) {}
        fn on_incoming_call(&self, _: CallInfo) {}
        fn on_call_state(&self, _: String, _: CallState) {}
        fn on_log(&self, _: String) {}
    }
    let suffix = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
    let dir = std::env::temp_dir().join(format!("screens-shell-search-{}-{suffix}", std::process::id()));
    let node = Node::new(dir.to_string_lossy().into(), Arc::new(Quiet)).unwrap();
    node.create_identity("Search check".into(), String::new()).unwrap();
    node.commit_identity().unwrap();
    let me = keystore::current_did(&node).unwrap();
    let first = node.send_text(me.clone(), "first sol61needle message".into(), None).unwrap();
    std::thread::sleep(Duration::from_millis(2));
    let second = node.send_text(me, "newest sol61needle message".into(), None).unwrap();
    let result = node.search("sol61needle".into(), 20, None).unwrap();
    assert_eq!(result.messages.len(), 2);
    assert_eq!(result.messages[0].id, second.id); assert_eq!(result.messages[1].id, first.id);
    assert!(!result.messages[0].highlights.is_empty());
    drop(node); let _ = std::fs::remove_dir_all(dir);
}
#[test]
fn stale_results_are_rejected_after_query_or_account_change() {
    let mut s = State::default(); s.clear(); let old = s.generation;
    s.clear(); s.accept(old, false, Ok(page())); assert!(s.result.is_none());
    let current = s.generation; s.accept(current, false, Ok(page())); assert!(s.result.is_some());
    assert!(s.bind(Some("different account"))); assert!(s.result.is_none());
    s.accept(current, false, Ok(page())); assert!(s.result.is_none());
}
#[test]
fn highlighting_uses_scalar_offsets_after_emoji_and_indic_marks() {
    let p = marked_parts("😀 ऊ lunch", &[p2pcore::HitRange { start: 4, end: 9 }]);
    assert_eq!(p, vec![("😀 ऊ ".into(), false), ("lunch".into(), true)]);
}
#[test]
fn errors_are_not_silently_reported_as_no_matches() {
    let mut s = State::default(); s.busy = true;
    s.accept(0, false, Err("Account is locked".into()));
    assert!(!s.busy); assert_eq!(s.error.as_deref(), Some("Account is locked"));
}

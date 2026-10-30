use super::*;
use crate::doc::Entry;
use crate::record::Record;
use crate::state::MemberInfo;

const DAY: &str = "2026-10-30";

fn file() -> FileRef {
    FileRef { hash: [3; 32], key: [4; 32], size: 10, name: "a.pdf".into(), mime: "application/pdf".into() }
}

fn state(members: &[&str]) -> GroupState {
    let mut st = GroupState::default();
    for m in members {
        st.members.insert(m.to_string(), MemberInfo { did: m.to_string(), name: m.to_string(), joined_at: 0, invited_by: None, devices: Default::default() });
    }
    st
}

/// A visible `FileHeld` row by `did` on device `dev` at `at`.
fn held(did: &str, dev: u8, at: i64, hash: [u8; 32], held: bool) -> PostRow {
    let rec = Record {
        group: "g".into(),
        author: did.into(),
        dev: [dev; 32],
        at,
        seen: vec![],
        body: Body::FileHeld { post: "p1".into(), day: DAY.into(), hash, held },
    };
    PostRow { entry: Entry { id: format!("{did}-{dev}-{at}-{held}"), rec, peer: 1, counter: 0 }, vis: Vis::Visible }
}

// KIND: unit (group files) — holders come from the latest `FileHeld` per device of current
// members naming the signed hash: the author first (until they let it go), then the others in the
// order they took it; deleting, leaving, a wrong hash or a hidden record take a member off.
#[test]
fn holders_follow_the_latest_statement_of_current_members() {
    let f = file();
    let st = state(&["alice", "bob", "carol", "dave"]);
    // Nobody announced: the author holds it.
    assert_eq!(holders(&[], &st, "p1", "alice", &f), vec!["alice"]);

    let mut rows = vec![held("carol", 1, 20, f.hash, true), held("bob", 1, 10, f.hash, true)];
    assert_eq!(holders(&rows, &st, "p1", "alice", &f), vec!["alice", "bob", "carol"]);

    // Bob deletes his copy (a later statement); one of Carol's two devices still holds it.
    rows.push(held("bob", 1, 30, f.hash, false));
    rows.push(held("carol", 2, 31, f.hash, false));
    assert_eq!(holders(&rows, &st, "p1", "alice", &f), vec!["alice", "carol"]);
    assert_eq!(own_statement(&rows, "p1", &f.hash, "bob", &[1; 32]), Some(false));
    assert_eq!(own_statement(&rows, "p1", &f.hash, "dave", &[1; 32]), None);

    // A statement naming another hash counts for nothing; a hidden one neither.
    rows.push(held("dave", 1, 40, [9; 32], true));
    let mut hidden = held("dave", 2, 41, f.hash, true);
    hidden.vis = Vis::Hidden;
    rows.push(hidden);
    assert_eq!(holders(&rows, &st, "p1", "alice", &f), vec!["alice", "carol"]);

    // The author lets it go.
    rows.push(held("alice", 1, 50, f.hash, false));
    assert_eq!(holders(&rows, &st, "p1", "alice", &f), vec!["carol"]);

    // Carol leaves (not a member now), or is removed: she drops off at once.
    let left = state(&["alice", "bob", "dave"]);
    assert!(holders(&rows, &left, "p1", "alice", &f).is_empty());
    let mut removed = st.clone();
    removed.removed.insert("carol".into(), vec![]);
    assert!(holders(&rows, &removed, "p1", "alice", &f).is_empty());

    // Same `at`: the record id breaks the tie the same way on every device.
    let tie = vec![held("bob", 1, 60, f.hash, true), held("bob", 1, 60, f.hash, false)];
    let first = holders(&tie, &st, "p1", "alice", &f);
    let mut rev = tie;
    rev.reverse();
    assert_eq!(holders(&rev, &st, "p1", "alice", &f), first);
}

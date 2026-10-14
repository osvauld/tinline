use super::*;
use ComposerState::*;

#[test]
fn composer_table() {
    for (by_me, me, contact, want) in [
        (false, false, true, Open),
        (true, false, true, BlockedByMe),
        (true, true, false, BlockedByMe), // our block wins
        (false, true, true, BlockedMe),
        (false, true, false, BlockedMe), // kept chat after their block
        (false, false, false, NotContact),
    ] {
        assert_eq!(
            composer_state(by_me, me, contact),
            want,
            "{by_me} {me} {contact}"
        );
    }
}

#[test]
fn call_gate_table() {
    for (c, token, want) in [
        (Open, true, CallGate::Ok),
        (Open, false, CallGate::NotAllowed),
        (BlockedByMe, true, CallGate::Closed),
        (BlockedMe, true, CallGate::Closed),
        (NotContact, true, CallGate::Closed),
        (NotContact, false, CallGate::Closed),
    ] {
        assert_eq!(call_gate(c, token), want, "{c:?} {token}");
    }
}

#[test]
fn status_table() {
    assert_eq!(block_status(false, false), None);
    assert_eq!(block_status(true, false).as_deref(), Some("Blocked"));
    assert_eq!(block_status(false, true).as_deref(), Some("Blocked you"));
}

#[test]
fn copy_golden() {
    assert_eq!(block_dialog_title("Arjun Oommen".into()), "Block Arjun?");
    let all = block_dialog_lines("Arjun Oommen".into()).join(" ");
    for part in [
        "will see that you blocked them",
        "removes you from their contacts",
        "chat history with Arjun stays",
        "has to add you again",
        "Groups you share are not affected",
    ] {
        assert!(all.contains(part), "{part}");
    }
    assert!(
        !all.to_lowercase().contains("delete"),
        "a block deletes nothing"
    );
    assert_eq!(
        not_delivered_text("Meera K".into()),
        "Not delivered: Meera blocked you"
    );
    assert_eq!(blocked_you_title("Meera K".into()), "Meera blocked you");
    assert_eq!(
        blocked_you_body("Meera K".into()),
        "You can\u{2019}t message or call Meera. Your earlier messages stay here."
    );
    assert_eq!(
        blocked_by_me_line("Ann Lee".into()),
        "You blocked Ann. They can\u{2019}t message or call you."
    );
    assert_eq!(
        not_contact_line("Meera K".into()),
        "Meera is no longer in your contacts. Add Meera again with their code to message."
    );
    assert_eq!(
        cant_send_text("Ann Lee".into()),
        "You can\u{2019}t send messages to Ann."
    );
    assert_eq!(
        calls_not_allowed_text("Priya N".into()),
        "Priya hasn\u{2019}t allowed calls from you. You can still message them."
    );
    assert_eq!(
        cant_call_text("Ann Lee".into(), BlockedByMe),
        "You blocked Ann. Unblock to call."
    );
    assert_eq!(
        cant_call_text("Ann Lee".into(), BlockedMe),
        "Can\u{2019}t call: Ann blocked you"
    );
    assert_eq!(
        cant_call_text("Ann Lee".into(), NotContact),
        "Ann is no longer in your contacts."
    );
    assert_eq!(
        cant_call_text("Priya N".into(), Open),
        calls_not_allowed_text("Priya N".into())
    );
    assert!(unblock_note().ends_with("kept either way."));
}

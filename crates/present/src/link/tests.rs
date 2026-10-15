use super::*;

#[test]
fn failure_text_golden() {
    let t = |r: &str, d: Option<&str>| link_fail_text(r.into(), d.map(Into::into), "phone".into());
    for (r, want) in [
        ("expired", "The code expired. Try again."),
        ("cancelled", "Cancelled on the other device."),
        (
            "rejected",
            "That QR code was already used or isn\u{2019}t valid. Make a fresh one and try again.",
        ),
        (
            "bad_proof",
            "The other device didn\u{2019}t accept that QR code. Make a fresh one and try again.",
        ),
        (
            "mismatch",
            "These two devices don\u{2019}t belong to the same account, so nothing was shared.",
        ),
        (
            "timeout",
            "The other device didn\u{2019}t answer in time. Keep both open and try again.",
        ),
        (
            "net",
            "Couldn\u{2019}t reach the other device. Check that both are online.",
        ),
        (
            "locked",
            "The other device is locked. Unlock Tinline there and try again.",
        ),
        (
            "busy",
            "Tinline was busy with something else. Try again in a moment.",
        ),
        (
            "bad_grant",
            "The other device sent something this app couldn\u{2019}t read. Update Tinline on both and try again.",
        ),
        ("account_exists", "This account is already on this phone."),
        ("zzz", "Couldn\u{2019}t link. Try again."),
    ] {
        assert_eq!(t(r, None), want, "{r}");
    }
    assert_eq!(t("zzz", Some("core says hi")), "core says hi");
    assert_eq!(t("zzz", Some("  ")), "Couldn\u{2019}t link. Try again.");
    assert_eq!(
        link_fail_text("account_exists".into(), None, "computer".into()),
        "This account is already on this computer."
    );
    assert_eq!(link_failure("net".into()), LinkFailure::Net);
    assert_eq!(link_failure("".into()), LinkFailure::Other);
}

#[test]
fn qr_check_golden() {
    assert_eq!(link_qr_problem("OSVL1:AEBAGB".into()), None);
    assert_eq!(link_qr_problem("  OSVL1:AEBAGB\n".into()), None);
    assert_eq!(link_qr_kind("OSVC2:MFRGG".into()), QrKind::ContactCode);
    assert_eq!(
        link_qr_problem("OSVC2:MFRGG".into()).as_deref(),
        Some("That\u{2019}s a contact code, not a device-link code. Use Add contact for those.")
    );
    for t in ["", "hello", "osvl1:x", "https://example.com"] {
        assert_eq!(
            link_qr_problem(t.into()).as_deref(),
            Some("That isn\u{2019}t a Tinline device-link code."),
            "{t:?}"
        );
    }
}

//! Bounded persisted-day inventory, not full own-device transport integration.
use super::*;
use crate::own::contents::HISTORY_PAGE_DAYS;

fn seed(f: &Fixture, ordinal: i64) -> String {
    let at = ordinal * DAY_MS + 1000;
    let day = day_of(at);
    let which = DocId::Day(day.clone());
    let doc = GroupDoc::new_in(&f.gid, which.clone(), writer_id(f.author.did(), &f.dev));
    let record = Record {
        group: f.gid.clone(),
        author: f.author.did().into(),
        dev: f.dev,
        at,
        seen: f.meta.seen(),
        body: Body::Post {
            text: format!("archived day {ordinal}"),
            reply_to: None,
            image: None,
        },
    };
    let (_, update) = doc.write(&f.author, &record).unwrap();
    f.core
        .apply(&f.gid, &which, &update, |_, _, _| Ok(()))
        .unwrap();
    day
}

#[test]
fn retained_archive_pages_cross_recent_window_without_duplicates_or_loss() {
    let f = Fixture::new();
    let expected: Vec<_> = (0..130).map(|i| seed(&f, i)).collect();
    let mut after = None;
    let mut actual = Vec::new();
    let mut lengths = Vec::new();
    loop {
        let page = f
            .core
            .own_history_page(&f.gid, after.as_deref(), HISTORY_PAGE_DAYS)
            .unwrap();
        lengths.push(page.days.len());
        for (day, vv) in &page.days {
            assert_eq!(
                *vv,
                f.core.store.shard_meta(&f.gid, day).unwrap().unwrap().vv
            );
            assert_eq!(
                decode_vv(vv)
                    .unwrap()
                    .get(&writer_id(f.author.did(), &f.dev)),
                Some(&1)
            );
            actual.push(day.clone());
        }
        if page.days.is_empty() {
            assert!(page.next.is_none());
            break;
        }
        assert_eq!(
            page.next.as_deref(),
            page.days.last().map(|(d, _)| d.as_str())
        );
        after = page.next;
    }
    assert_eq!(lengths, [64, 64, 2, 0]);
    assert_eq!(actual, expected);
    assert_eq!(
        f.core.store.days(&f.gid).unwrap(),
        expected,
        "inventory never deletes retained history"
    );
}

#[test]
fn exclusive_cursor_and_small_page_keep_next_day_available() {
    let f = Fixture::new();
    let days: Vec<_> = (0..3).map(|i| seed(&f, i)).collect();
    let first = f.core.own_history_page(&f.gid, None, 1).unwrap();
    assert_eq!(first.days[0].0, days[0]);
    let second = f
        .core
        .own_history_page(&f.gid, first.next.as_deref(), 1)
        .unwrap();
    assert_eq!(second.days[0].0, days[1]);
    let absent = day_of(-DAY_MS);
    assert_eq!(
        f.core
            .own_history_page(&f.gid, Some(&absent), 1)
            .unwrap()
            .days[0]
            .0,
        days[0]
    );
}

#[test]
fn malformed_cursor_oversized_requests_and_unknown_groups_fail_closed() {
    let f = Fixture::new();
    seed(&f, 0);
    for limit in [0, HISTORY_PAGE_DAYS + 1, usize::MAX] {
        assert!(matches!(
            f.core.own_history_page(&f.gid, None, limit),
            Err(Error::Protocol(_))
        ));
    }
    for cursor in ["../meta", "1970-1-1", "2026-02-30", ""] {
        assert!(matches!(
            f.core.own_history_page(&f.gid, Some(cursor), 1),
            Err(Error::Protocol(_))
        ));
    }
    let other = Fixture::new();
    assert!(matches!(
        f.core.own_history_page(&other.gid, None, 1),
        Err(Error::Protocol(_))
    ));
}

#[test]
fn missing_persisted_frontier_is_not_an_empty_complete_page() {
    let f = Fixture::new();
    f.core
        .store
        .db
        .put(&format!("days/{}/1970-01-01", f.gid), &[])
        .unwrap();
    assert!(matches!(
        f.core.own_history_page(&f.gid, None, 1),
        Err(Error::Io(_))
    ));
}

#[test]
fn insertion_behind_cursor_is_repaired_by_another_pass_not_silently_claimed_complete() {
    let f = Fixture::new();
    let later = seed(&f, 10);
    let first = f.core.own_history_page(&f.gid, None, 1).unwrap();
    let earlier = seed(&f, 2);
    assert!(
        f.core
            .own_history_page(&f.gid, first.next.as_deref(), 1)
            .unwrap()
            .days
            .is_empty()
    );
    let rescan = f.core.own_history_page(&f.gid, None, 2).unwrap();
    assert_eq!(
        rescan.days.into_iter().map(|(d, _)| d).collect::<Vec<_>>(),
        [earlier, later]
    );
    // A terminal inventory page alone is not a route-readiness certificate.
}

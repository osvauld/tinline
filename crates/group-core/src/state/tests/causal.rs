//! Permits and admin grants follow causality, not author clocks (`state::causal_latest`).

use super::*;

/// Delivers `blobs` (meta updates) to a fresh replica in the order of `perm`, holding back an
/// update whose causal past has not arrived yet (as a device does until it re-syncs).
fn deliver(gid: &str, blobs: &[Vec<u8>], perm: &[usize]) -> GroupState {
    let mut f = Dev::new(gid, 90);
    let mut left: Vec<usize> = perm.to_vec();
    while !left.is_empty() {
        let before = left.len();
        left.retain(|&i| f.doc.apply_remote(&blobs[i], |_| Ok(())).is_err());
        assert!(
            left.len() < before,
            "every update applies once its past is here"
        );
    }
    f.state()
}

/// Every permutation of `0..n`.
fn perms(n: usize) -> Vec<Vec<usize>> {
    if n == 0 {
        return vec![Vec::new()];
    }
    let mut out = Vec::new();
    for p in perms(n - 1) {
        for i in 0..=p.len() {
            let mut q = p.clone();
            q.insert(i, n - 1);
            out.push(q);
        }
    }
    out
}

// KIND: unit (public-group follow-up, Fix A) — an admin with a forged clock (`at` near
// `i64::MAX`) permits a device to host chat and calls; the owner, who saw those permits,
// revokes them with an ordinary clock: revoked, also after the admin is demoted, and the
// demoted admin's next permit is inert. A fresh replica that gets every record in every
// order derives the same.
#[test]
fn forged_clock_permit_is_superseded_by_a_later_revoke() {
    let (gid, mut o, mut ms) = group(2);
    let a = ms[0].did();
    let (h, hdev) = (ms[1].did(), ms[1].dev);
    let mut blobs = Vec::new();
    blobs.push(o.doc.export_since(&Dev::new(&gid, 91).doc.vv()).unwrap());
    blobs.push(
        o.write(Body::Admin {
            member: a.clone(),
            admin: true,
        })
        .1,
    );
    sync(&mut o, &mut ms[0]);
    ms[0].clock = i64::MAX - 1_000;
    blobs.push(
        ms[0]
            .write(Body::HostPermit {
                host: h.clone(),
                host_dev: hdev,
                active: true,
            })
            .1,
    );
    blobs.push(
        ms[0]
            .write(Body::CallHostPermit {
                host: h.clone(),
                host_dev: hdev,
                active: true,
            })
            .1,
    );
    sync(&mut ms[0], &mut o);
    sync(&mut o, &mut ms[1]);
    blobs.push(ms[1].write(Body::HostAccept { active: true }).1);
    blobs.push(ms[1].write(Body::CallHostAccept { active: true }).1);
    sync(&mut ms[1], &mut o);
    let s = o.state();
    assert!(s.is_host(&h, &hdev) && s.is_call_host(&h, &hdev));
    // The owner saw both permits and revokes them with its own (much earlier) clock.
    blobs.push(
        o.write(Body::HostPermit {
            host: h.clone(),
            host_dev: hdev,
            active: false,
        })
        .1,
    );
    blobs.push(
        o.write(Body::CallHostPermit {
            host: h.clone(),
            host_dev: hdev,
            active: false,
        })
        .1,
    );
    let s = o.state();
    assert!(
        !s.is_host(&h, &hdev),
        "a later revoke supersedes the forged-clock permit"
    );
    assert!(!s.is_call_host(&h, &hdev), "the same for call hosting");
    // Demoted, a permits again (it saw its demotion): inert.
    blobs.push(
        o.write(Body::Admin {
            member: a.clone(),
            admin: false,
        })
        .1,
    );
    sync(&mut o, &mut ms[0]);
    blobs.push(
        ms[0]
            .write(Body::HostPermit {
                host: h.clone(),
                host_dev: hdev,
                active: true,
            })
            .1,
    );
    sync(&mut ms[0], &mut o);
    let s = o.state();
    assert!(
        !s.is_admin(&a) && !s.is_host(&h, &hdev),
        "a demoted admin's permit does not count"
    );
    // Delivery orders agree (the genesis/join blob first: it is everyone's causal past): the
    // identity, the reverse and 40 seeded shuffles of the 9 later updates.
    let mut orders: Vec<Vec<usize>> =
        vec![(1..blobs.len()).collect(), (1..blobs.len()).rev().collect()];
    let mut x: u64 = 0x9e37_79b9_7f4a_7c15;
    for _ in 0..40 {
        let mut o: Vec<usize> = (1..blobs.len()).collect();
        for i in (1..o.len()).rev() {
            x = x
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            o.swap(i, (x >> 33) as usize % (i + 1));
        }
        orders.push(o);
    }
    for rest in orders {
        let mut order = vec![0];
        order.extend(rest);
        assert_eq!(deliver(&gid, &blobs, &order), s, "order {order:?}");
    }
}

// KIND: unit (public-group follow-up, Fix A) — a clock-forged owner `Admin` grant does not
// outlive a later revoke that saw it (the owner's clock was far ahead, then corrected), and the
// revoked admin's flags written after it saw the revoke are inert.
#[test]
fn forged_clock_admin_grant_is_superseded_by_a_later_revoke() {
    let (gid, mut o, mut ms) = group(2);
    let a = ms[0].did();
    o.clock = i64::MAX - 1_000;
    o.write(Body::Admin {
        member: a.clone(),
        admin: true,
    });
    o.clock = 5_000;
    o.write(Body::Admin {
        member: a.clone(),
        admin: false,
    });
    assert!(!o.state().is_admin(&a), "the revoke saw the grant");
    sync(&mut o, &mut ms[0]);
    let target = ms[1].did();
    for _ in 0..3 {
        ms[0].write(Body::Flag {
            member: target.clone(),
        });
    }
    sync(&mut ms[0], &mut o);
    assert!(
        !o.state().is_removed(&target),
        "flags by a revoked admin do not count"
    );
    let mut f = Dev::new(&gid, 92);
    f.take(&o.doc.export_since(&f.doc.vv()).unwrap());
    assert_eq!(f.state(), o.state());
}

// KIND: unit (public-group follow-up, Fix A) — concurrent permits (neither saw the other) are
// decided by `(at, id)`, the same on every replica whichever arrives first; a decline answers
// only a permit it saw (or a concurrent earlier one), so a forged-future permit is still asked
// about only until the device answers it.
#[test]
fn concurrent_permits_tie_break_by_time_in_every_order() {
    let (gid, mut o, mut ms) = group(2);
    let a = ms[0].did();
    let (h, hdev) = (ms[1].did(), ms[1].dev);
    o.write(Body::Admin {
        member: a.clone(),
        admin: true,
    });
    for m in ms.iter_mut() {
        sync(&mut o, m);
    }
    let base = o.doc.export_since(&Dev::new(&gid, 93).doc.vv()).unwrap();
    // Concurrent: o revokes (never having seen a permit), a permits with a later clock.
    let revoke = o
        .write(Body::HostPermit {
            host: h.clone(),
            host_dev: hdev,
            active: false,
        })
        .1;
    ms[0].clock = o.clock + 1_000;
    let permit = ms[0]
        .write(Body::HostPermit {
            host: h.clone(),
            host_dev: hdev,
            active: true,
        })
        .1;
    let blobs = vec![base, revoke, permit];
    let a_then_o = deliver(&gid, &blobs, &[0, 2, 1]);
    let o_then_a = deliver(&gid, &blobs, &[0, 1, 2]);
    assert_eq!(a_then_o, o_then_a);
    assert!(
        a_then_o
            .host_requests
            .iter()
            .any(|r| r.did == h && r.dev == hdev),
        "the later concurrent permit is in force"
    );
    // The device declines after seeing it: no longer asked, in every order.
    let (m0, m1) = ms.split_at_mut(1);
    sync(&mut m0[0], &mut m1[0]);
    sync(&mut o, &mut ms[1]);
    let decline = ms[1].write(Body::HostAccept { active: false }).1;
    let mut blobs = blobs;
    blobs.push(decline);
    for p in perms(3) {
        let mut order = vec![0];
        order.extend(p.iter().map(|i| i + 1));
        let s = deliver(&gid, &blobs, &order);
        assert!(
            !s.host_requests.iter().any(|r| r.did == h),
            "declined, order {order:?}"
        );
        assert!(!s.is_host(&h, &hdev));
    }
}

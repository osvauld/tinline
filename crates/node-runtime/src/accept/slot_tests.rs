//! The reserved hello slots (`RESERVED_HELLOS`) are for contacts we do not block.

use iroh::address_lookup::memory::MemoryLookup;

use crate::push::tests::{befriend, node};

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn slot_reserved_only_for_unblocked_contacts() {
    let lookup = MemoryLookup::new();
    let (a, b, stranger) = (node(&lookup).await, node(&lookup).await, node(&lookup).await);
    befriend(&a, &b);
    assert!(a.ctx.reserved_slot_for(&b.me.device), "a contact's device may use a reserved slot");
    assert!(!a.ctx.reserved_slot_for(&stranger.me.device), "a stranger may not");

    a.edit(|st| {
        st.blocked.insert(b.did());
    });
    assert!(a.ctx.is_contact_device(&b.me.device), "a blocked contact is kept (U19)");
    assert!(!a.ctx.reserved_slot_for(&b.me.device), "but gets no reserved slot");

    a.edit(|st| {
        st.blocked.remove(&b.did());
    });
    assert!(a.ctx.reserved_slot_for(&b.me.device), "unblocking gives it back");
}

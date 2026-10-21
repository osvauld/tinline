use super::*;

fn store() -> ChatStore {
    ChatStore::new(Sealed::new(storage::Store::open_in_memory().unwrap(), [6; 32]))
}

#[test]
fn read_receipts_default_on_and_persist_off() {
    let s = store();
    assert!(s.read_receipts(), "default on");
    s.set_read_receipts(false).unwrap();
    assert!(!s.read_receipts());
    s.set_read_receipts(true).unwrap();
    assert!(s.read_receipts());
}

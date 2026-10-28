//! Drain queued chat updates in bounded chunks, retaining control-event order.
use crate::{ChatEv, Ev, EvRx};

pub const CHAT_BATCH: usize = 128;

pub async fn next(rx: &mut EvRx, pending: &mut Option<Ev>) -> Option<Ev> {
    let first = match pending.take() {
        Some(ev) => ev,
        None => rx.recv().await?,
    };
    let Ev::Chat(first) = first else { return Some(first) };
    let mut batch = vec![first];
    while batch.len() < CHAT_BATCH {
        match rx.try_recv() {
            Ok(Ev::Chat(ev)) => batch.push(ev),
            Ok(ev) => {
                *pending = Some(ev);
                break;
            }
            Err(_) => break,
        }
    }
    Some(Ev::Chat(ChatEv::Batch(batch)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn catch_up_chunks_are_bounded_and_control_events_keep_their_place() {
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        for i in 0..CHAT_BATCH + 3 {
            tx.send(Ev::Chat(ChatEv::Presence(i.to_string(), true))).ok().unwrap();
        }
        tx.send(Ev::Contacts).ok().unwrap();
        tx.send(Ev::Chat(ChatEv::Presence("after".into(), true))).ok().unwrap();
        drop(tx);
        let mut pending = None;
        for expected in [CHAT_BATCH, 3] {
            let Some(Ev::Chat(ChatEv::Batch(batch))) = next(&mut rx, &mut pending).await else { panic!("missing batch") };
            assert_eq!(batch.len(), expected);
        }
        assert!(matches!(next(&mut rx, &mut pending).await, Some(Ev::Contacts)));
        let Some(Ev::Chat(ChatEv::Batch(batch))) = next(&mut rx, &mut pending).await else { panic!("missing final batch") };
        assert!(matches!(&batch[..], [ChatEv::Presence(peer, true)] if peer == "after"));
        assert!(next(&mut rx, &mut pending).await.is_none());
    }
}

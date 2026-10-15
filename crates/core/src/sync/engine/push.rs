//! Outgoing own-device document, acknowledgment, and read-cursor updates.

use super::*;

impl Inner {
    pub(super) fn own_content_catalog(self: &Arc<Self>) -> Vec<(String, Vec<u8>)> {
        group_contents::catalog(
            self.chat_own_docs(),
            self.group_own_content_docs(MAX_OWN_DOCS),
        )
    }

    /// DM shards plus negotiated group meta/recent days, then read cursors.
    pub(super) async fn push_all_chat(
        self: &Arc<Self>,
        me: &Arc<Me>,
        remote: &[u8; 32],
        peer: &mut PeerChat,
        writer: &mut FrameWriter,
    ) -> Result<(), Error> {
        if peer.group_contents {
            // Bootstrap group tokens precede contents on this ordered stream.
            self.push_groups(peer, writer).await?;
            for (name, _) in self.own_content_catalog() {
                if group_documents::parse_doc(&name).is_some() {
                    self.push_group_doc(me, remote, &name, peer, writer).await?;
                } else {
                    self.push_chat_doc(me, &name, peer, writer).await?;
                }
            }
        } else {
            // Preserve the complete legacy DM slice, despite group names in Hello.
            for (name, _) in self.chat_own_docs() {
                self.push_chat_doc(me, &name, peer, writer).await?;
            }
        }
        self.push_read(peer, writer).await?;
        self.push_acks(peer, writer).await?;
        self.push_groups(peer, writer).await?;
        self.push_invitations(peer, writer).await
    }

    /// Our group invitations, if they changed since the last ones this session sent (G3).
    pub(super) async fn push_invitations(
        self: &Arc<Self>,
        peer: &mut PeerChat,
        writer: &mut FrameWriter,
    ) -> Result<(), Error> {
        let rows = self.group_own_invitations();
        if peer.invitations.as_ref() == Some(&rows)
            || (rows.is_empty() && peer.invitations.is_none())
        {
            return Ok(());
        }
        peer.invitations = Some(rows.clone());
        writer.send(&SelfMsg::Invitations { rows }).await
    }

    /// Our group list, if it changed since the last one this session sent (G2).
    pub(super) async fn push_groups(
        self: &Arc<Self>,
        peer: &mut PeerChat,
        writer: &mut FrameWriter,
    ) -> Result<(), Error> {
        let groups = self.group_own_tokens();
        if peer.groups.as_ref() == Some(&groups) || (groups.is_empty() && peer.groups.is_none()) {
            return Ok(());
        }
        peer.groups = Some(groups.clone());
        writer.send(&SelfMsg::Groups { groups }).await
    }

    pub(super) async fn push_chat_doc(
        self: &Arc<Self>,
        me: &Arc<Me>,
        name: &str,
        peer: &mut PeerChat,
        writer: &mut FrameWriter,
    ) -> Result<(), Error> {
        let theirs = peer.docs.get(name).cloned().unwrap_or_default();
        let out = match self.chat_own_export(name, &theirs).await {
            Ok(o) => o,
            Err(e) => {
                tracing::debug!("own-device export of {name}: {e}");
                None
            }
        };
        if let Some((update, vv)) = out {
            let sig = crate::chat::wire::sign_batch(&me.profile.device_secret, name, &update);
            writer
                .send(&SelfMsg::Sync {
                    doc: name.to_string(),
                    vv: vv.clone(),
                    update,
                    sig,
                })
                .await?;
            if let Some(v) = Self::peer_vv_of(&vv) {
                group_contents::remember(peer, name.to_string(), v);
            }
        }
        Ok(())
    }

    pub(super) async fn push_group_doc(
        self: &Arc<Self>,
        me: &Arc<Me>,
        remote: &[u8; 32],
        name: &str,
        peer: &mut PeerChat,
        writer: &mut FrameWriter,
    ) -> Result<(), Error> {
        if !peer.group_contents {
            return Ok(());
        }
        if peer.history.enabled {
            self.own_group_admit(me.epoch, remote)?;
            if let Some((group, _)) = group_documents::parse_doc(name) {
                writer.send(&SelfMsg::GroupHistoryChanged { group }).await?;
            }
            return Ok(());
        }
        let theirs = peer.docs.get(name).cloned().unwrap_or_default();
        let out = match self.group_own_content_export(me.epoch, remote, name, &theirs) {
            Ok(out) => out,
            Err(Error::Protocol(reason)) => {
                // Oversized history needs range batching. Keep records and pending
                // state; don't stall account/other-doc sync or fake a receipt.
                tracing::debug!("deferred own group export {name}: {reason}");
                return Ok(());
            }
            Err(e) => return Err(e),
        };
        if let Some((update, vv)) = out {
            let sig = crate::chat::wire::sign_batch(&me.profile.device_secret, name, &update);
            writer
                .send(&SelfMsg::Sync {
                    doc: name.into(),
                    vv: vv.clone(),
                    update,
                    sig,
                })
                .await?;
            if let Some(v) = Self::peer_vv_of(&vv) {
                group_contents::remember(peer, name.into(), v);
            }
        }
        Ok(())
    }

    /// The contacts' acks the peer is not known to hold yet.
    pub(super) async fn push_acks(
        &self,
        peer: &mut PeerChat,
        writer: &mut FrameWriter,
    ) -> Result<(), Error> {
        let mut acks = Vec::new();
        for (doc, vv) in self.chat_acks() {
            if peer.acks.get(&doc).is_some_and(|p| {
                p.partial_cmp(&vv)
                    .is_some_and(|o| o != std::cmp::Ordering::Less)
            }) {
                continue;
            }
            acks.push((doc.clone(), vv.encode()));
            peer.acks.insert(doc, vv);
        }
        if acks.is_empty() {
            return Ok(());
        }
        writer.send(&SelfMsg::Acks { acks }).await
    }

    pub(super) async fn push_read(
        &self,
        peer: &mut PeerChat,
        writer: &mut FrameWriter,
    ) -> Result<(), Error> {
        let cursors: Vec<(String, i64)> = self
            .chat_read_cursors()
            .into_iter()
            .filter(|(p, c)| peer.read.get(p).is_none_or(|x| c > x))
            .collect();
        if cursors.is_empty() {
            return Ok(());
        }
        for (p, c) in &cursors {
            peer.read.insert(p.clone(), *c);
        }
        writer.send(&SelfMsg::Read { cursors }).await
    }

    /// What the peer lacks of a whole-synced doc (`name`), once its `Hello` said what it holds.
    pub(super) async fn push_changes(
        &self,
        me: &Arc<Me>,
        name: &str,
        doc: &Doc,
        sent: &mut Option<VersionVector>,
        writer: &mut FrameWriter,
    ) -> Result<(), Error> {
        let Some(theirs) = sent.as_ref() else {
            return Ok(());
        };
        if let Some((update, vv)) = doc.export_since(theirs) {
            let sig = crate::chat::wire::sign_batch(&me.profile.device_secret, name, &update);
            writer
                .send(&SelfMsg::Sync {
                    doc: name.into(),
                    vv: vv.encode(),
                    update,
                    sig,
                })
                .await?;
            *sent = Some(vv);
        }
        Ok(())
    }
}

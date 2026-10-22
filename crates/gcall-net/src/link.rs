//! The QUIC links of a session: dial (participant side), serve (host side), framing.

use super::*;

impl Session {
    // ---- remote link (participant side) --------------------------------------------------

    pub(super) fn current(st: &St, seq: u64) -> bool {
        matches!(st.link, Link::Remote { seq: s, .. } if s == seq)
    }

    pub(super) async fn dial(self: Arc<Self>, seq: u64, addr: EndpointAddr) {
        let connected = async {
            let ep = self.inner.upgrade().and_then(|i| i.group_endpoint()).ok_or(Error::NotStarted)?;
            let conn = ep.connect(addr, GCALL_ALPN).await.map_err(Error::net)?;
            let (send, recv) = conn.open_bi().await.map_err(Error::net)?;
            Ok::<_, Error>((conn, send, recv))
        };
        let (conn, send, mut recv) = match tokio::time::timeout(DIAL_TIMEOUT, connected).await {
            Ok(Ok(x)) => x,
            other => {
                tracing::debug!("gcall: dial failed: {:?}", other.map(|r| r.map(|_| ())));
                self.with(|s, st, q| {
                    if Self::current(st, seq) {
                        st.link = Link::None;
                        if let Some(c) = st.client.as_mut() {
                            q.push_back(Work::C(c.dial_failed(s.now(), &*s.cfg.dir)));
                        }
                    }
                });
                return;
            }
        };
        let (tx, rx) = mpsc::unbounded_channel();
        let installed = self.with(|_, st, _| {
            let Link::Remote { seq: s, peer, queued } = &mut st.link else { return false };
            if *s != seq {
                return false;
            }
            for f in queued.drain(..) {
                let _ = tx.send(Out::Frame(f));
            }
            *peer = Some(Peer { conn: conn.clone(), tx: tx.clone() });
            true
        });
        if !installed {
            conn.close(CLOSE_BYE.into(), b"");
            return;
        }
        self.handle.spawn(writer(conn.clone(), send, rx));
        drop(tx);
        let mut buf = vec![];
        loop {
            tokio::select! {
                f = read_frame(&mut recv, &mut buf) => match f {
                    Some(msg) => self.with(|s, st, q| {
                        if Self::current(st, seq)
                            && let Some(c) = st.client.as_mut()
                            && let Ok(acts) = c.on_control(msg, s.now(), &*s.cfg.dir)
                        {
                            q.push_back(Work::C(acts));
                        }
                    }),
                    None => break,
                },
                d = conn.read_datagram() => match d {
                    Ok(d) => {
                        let now = self.now();
                        let mut st = self.st.lock();
                        if Self::current(&st, seq)
                            && let Some(c) = st.client.as_mut()
                        {
                            c.on_datagram(&d, now);
                        }
                    }
                    Err(_) => break,
                },
            }
        }
        let why = tokio::time::timeout(CLOSE_WAIT, conn.closed()).await;
        let elsewhere = matches!(&why, Ok(ConnectionError::ApplicationClosed(c)) if c.error_code == CLOSE_ANSWERED_ELSEWHERE.into());
        tracing::debug!("gcall: host link closed: {why:?}");
        self.with(|s, st, q| {
            if Self::current(st, seq) {
                st.link = Link::None;
                if let Some(c) = st.client.as_mut() {
                    q.push_back(Work::C(c.on_closed(elsewhere, s.now(), &*s.cfg.dir)));
                }
            }
        });
        conn.close(CLOSE_BYE.into(), b"");
    }

    // ---- host side -------------------------------------------------------------------------

    /// Serves one accepted participant connection whose first frame was `first`.
    pub(super) async fn serve(self: Arc<Self>, conn: Connection, send: SendStream, mut recv: RecvStream, mut buf: Vec<u8>, first: GroupCallMsg) {
        let remote: [u8; 32] = *conn.remote_id().as_bytes();
        let (tx, rx) = mpsc::unbounded_channel();
        let cid = self.with(|_, st, _| {
            let h = st.host.as_mut()?;
            let cid = ConnId(st.next_conn);
            st.next_conn += 1;
            h.eng.connected(cid, remote);
            h.conns.insert(cid, Peer { conn: conn.clone(), tx: tx.clone() });
            Some(cid)
        });
        let Some(cid) = cid else {
            conn.close(CLOSE_REFUSED.into(), b"");
            return;
        };
        self.handle.spawn(writer(conn.clone(), send, rx));
        drop(tx);
        self.host_control(cid, first);
        loop {
            tokio::select! {
                f = read_frame(&mut recv, &mut buf) => match f {
                    Some(msg) => self.host_control(cid, msg),
                    None => break,
                },
                d = conn.read_datagram() => match d {
                    Ok(d) => self.with(|s, st, q| {
                        if let Some(h) = st.host.as_mut()
                            && h.conns.contains_key(&cid)
                            && let Ok(acts) = h.eng.on_datagram(cid, &d, s.now())
                        {
                            q.push_back(Work::H(acts));
                        }
                    }),
                    Err(_) => break,
                },
            }
        }
        self.with(|_, st, q| {
            if let Some(h) = st.host.as_mut() {
                if let Some(p) = h.conns.remove(&cid) {
                    p.close(CLOSE_BYE);
                }
                q.push_back(Work::H(h.eng.disconnected(cid)));
            }
        });
    }

    pub(super) fn host_control(self: &Arc<Self>, cid: ConnId, msg: GroupCallMsg) {
        self.with(|s, st, q| {
            let Some(h) = st.host.as_mut() else { return };
            if !h.conns.contains_key(&cid) {
                return;
            }
            match h.eng.on_control(cid, msg, s.now(), &*s.cfg.dir) {
                Ok(acts) => {
                    h.taking = false;
                    q.push_back(Work::H(acts));
                }
                Err(e) => {
                    tracing::debug!("gcall: control refused: {e:?}");
                    if h.taking {
                        for (_, p) in std::mem::take(&mut h.conns) {
                            p.close(CLOSE_REFUSED);
                        }
                        st.host = None;
                        return;
                    }
                    // A connection without a participant has nothing more to say: let its
                    // client move on to the next candidate now instead of at its timeout.
                    let joined = h.eng.roster().values().any(|d| h.eng.conn_of(d) == Some(cid));
                    if !joined && let Some(p) = h.conns.remove(&cid) {
                        p.close(CLOSE_REFUSED);
                    }
                }
            }
        });
    }
}

/// Writes control frames in order; on `Close` finishes the stream, waits (bounded) until the
/// peer has read it, then closes the connection with the code.
pub(super) async fn writer(conn: Connection, mut send: SendStream, mut rx: mpsc::UnboundedReceiver<Out>) {
    let mut code = CLOSE_BYE;
    while let Some(out) = rx.recv().await {
        match out {
            Out::Frame(f) => {
                if send.write_all(&f).await.is_err() {
                    return;
                }
            }
            Out::Close(c) => {
                code = c;
                break;
            }
        }
    }
    let _ = send.finish();
    let _ = tokio::time::timeout(FLUSH_TIMEOUT, send.stopped()).await;
    conn.close(code.into(), b"");
}

/// The next control frame; `None` at end of stream, on a read error or a malformed frame.
pub(super) async fn read_frame(recv: &mut RecvStream, buf: &mut Vec<u8>) -> Option<GroupCallMsg> {
    loop {
        match w::decode_control(buf) {
            Ok(Some((m, n))) => {
                buf.drain(..n);
                return Some(m);
            }
            Ok(None) => {}
            Err(_) => return None,
        }
        let mut tmp = [0u8; 2048];
        match recv.read(&mut tmp).await {
            Ok(Some(n)) => buf.extend_from_slice(&tmp[..n]),
            _ => return None,
        }
    }
}

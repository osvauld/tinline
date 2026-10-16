//! The send and receive tasks and codec threads of one call.

use super::*;

/// Ladder rung cap for a peer that takes at most `max_pixels`.
pub(super) fn max_rung(ladder: &LadderConfig, max_pixels: u32) -> usize {
    ladder
        .rungs
        .iter()
        .rposition(|r| r.width as u64 * r.height as u64 <= max_pixels as u64)
        .unwrap_or(0)
        .min(ladder.max_rung)
}

pub(super) async fn run_send(conn: Connection, pipe: Arc<Pipe>, peer_max_pixels: u32) {
    let mut cfg = SenderConfig::default();
    cfg.ladder.max_rung = max_rung(&cfg.ladder, peer_max_pixels);
    let send = match VideoSend::accept(conn, cfg).await {
        Ok(s) => s,
        Err(e) => {
            tracing::info!("video send: {e}");
            return;
        }
    };
    let spawned = std::thread::Builder::new()
        .name("tl-video-enc".into())
        .spawn(move || encode_loop(pipe, send));
    if let Err(e) = spawned {
        tracing::warn!("video encoder thread: {e}");
    }
}

pub(super) fn encode_loop(pipe: Arc<Pipe>, send: VideoSend) {
    video::cpu::pin_to_fast_cores();
    let epoch = Instant::now();
    let mut enc: Option<(AomEncoder, usize, usize)> = None;
    let mut rate = 0;
    loop {
        let input = {
            let mut slot = pipe.input.lock();
            loop {
                if pipe.stop.load(Ordering::SeqCst) {
                    return;
                }
                if let Some(i) = slot.take() {
                    break i;
                }
                pipe.wake.wait(&mut slot);
            }
        };
        if send.is_closed() {
            tracing::info!("video send: the peer's receiver went away");
            return;
        }
        let capture_us = input.at.saturating_duration_since(epoch).as_micros() as u64;
        // One look at the sender: a ladder step in between would otherwise give a keyframe at
        // the old size and another at the new one.
        let ((rw, rh), bitrate, wants_key, trace) =
            send.with_sender(|s| (s.resolution(), s.bitrate(), s.wants_keyframe(), (s.trace, s.drops, s.draining())));
        let pic = match input.prepare(rw as usize, rh as usize) {
            Ok(p) => p,
            Err(e) => {
                tracing::debug!("video frame: {e}");
                continue;
            }
        };
        let (w, h) = (pic.width, pic.height);
        let mut key = wants_key | pipe.resume.swap(false, Ordering::SeqCst);
        if enc.as_ref().is_none_or(|e| (e.1, e.2) != (w, h)) {
            let cfg = EncoderConfig {
                width: w,
                height: h,
                fps: FPS,
                bitrate_bps: bitrate,
                threads: ENCODER_THREADS,
                ..Default::default()
            };
            match AomEncoder::new(&cfg) {
                Ok(e) => enc = Some((e, w, h)),
                Err(e) => {
                    tracing::warn!("video encoder: {e}");
                    continue;
                }
            }
            rate = bitrate;
            key = true;
        } else if bitrate != rate {
            let _ = enc.as_mut().expect("encoder").0.set_bitrate(bitrate);
            rate = bitrate;
        }
        let t = Instant::now();
        let packets = match enc.as_mut().expect("encoder").0.encode(&pic, key) {
            Ok(p) => p,
            Err(e) => {
                tracing::warn!("video encode: {e}");
                enc = None;
                continue;
            }
        };
        let took = t.elapsed();
        let mut st = pipe.stats.lock();
        for p in packets {
            if send.send(capture_us, p.key, &p.data).is_err() {
                return;
            }
            st.record_admitted(p.data.len());
            st.keyframes_sent += p.key as u64;
        }
        send.encode_time(took.as_micros() as u64);
        st.send_width = w as u32;
        st.send_height = h as u32;
        st.bitrate_bps = rate;
        st.encode_ms = took.as_secs_f32() * 1000.0;
        let (t, drops, draining) = trace;
        (st.rate_reports, st.rate_overuse, st.rate_starved, st.backlog_drops) = (t.reports, t.overuse, t.starved, drops);
        (st.last_queuing_us, st.last_delivered_bps, st.queuing_offset_us) = (t.last_queuing_us, t.last_delivered_bps, t.q_offset_us);
        st.draining = draining;
    }
}

pub(super) async fn run_recv(conn: Connection, pipe: Arc<Pipe>) {
    let mut recv = match VideoRecv::new(conn, RecvConfig::default()).await {
        Ok(r) => r,
        Err(e) => {
            tracing::info!("video recv: {e}");
            return;
        }
    };
    // Small: a decoder that falls behind stops the reading, which the sender sees as backlog.
    let (tx, rx) = mpsc::channel::<Frame>(2);
    let dec_pipe = pipe.clone();
    let spawned = std::thread::Builder::new()
        .name("tl-video-dec".into())
        .spawn(move || decode_loop(dec_pipe, rx));
    if let Err(e) = spawned {
        tracing::warn!("video decoder thread: {e}");
        return;
    }
    loop {
        tokio::select! {
            f = recv.recv() => match f {
                Ok(f) => {
                    pipe.stats.lock().record_received(f.payload.len());
                    if tx.send(f).await.is_err() {
                        return;
                    }
                }
                Err(e) => {
                    tracing::info!("video recv: {e}");
                    return;
                }
            },
            _ = pipe.key_wanted.notified() => recv.request_keyframe(),
        }
    }
}

pub(super) fn decode_loop(pipe: Arc<Pipe>, mut rx: mpsc::Receiver<Frame>) {
    video::cpu::pin_to_fast_cores();
    let mut dec = match Decoder::new(DECODER_THREADS, MAX_PIXELS) {
        Ok(d) => d,
        Err(e) => {
            tracing::warn!("video decoder: {e}");
            return;
        }
    };
    // After an error only a keyframe (a new group) can be decoded.
    let mut broken = false;
    while let Some(f) = rx.blocking_recv() {
        if broken && f.index != 0 {
            continue;
        }
        match dec.decode(&f.payload) {
            Ok(pics) => {
                broken = false;
                for p in pics {
                    let seq = pipe.seq.fetch_add(1, Ordering::SeqCst) + 1;
                    {
                        let mut st = pipe.stats.lock();
                        st.frames_decoded += 1;
                        st.recv_width = p.width as u32;
                        st.recv_height = p.height as u32;
                    }
                    let frame = VideoFrame { width: p.width as u32, height: p.height as u32, data: p.data.into(), seq };
                    *pipe.output.lock() = Some(frame);
                }
            }
            Err(e) => {
                tracing::debug!("video decode: {e}");
                broken = true;
                pipe.stats.lock().decode_errors += 1;
                pipe.key_wanted.notify_one();
            }
        }
    }
}

/// An I420 `VideoFrame` as RGBA.
pub(super) fn to_rgba(f: VideoFrame) -> VideoFrame {
    let pic = I420 { width: f.width as usize, height: f.height as usize, data: f.data.0 };
    VideoFrame { data: convert::to_rgba(&pic).into(), ..f }
}

/// Largest even size of `sw` x `sh`'s aspect within the rung `rw` x `rh` (turned to match the
/// picture's orientation), never upscaled.
pub(crate) fn fit(sw: usize, sh: usize, rw: usize, rh: usize) -> (usize, usize) {
    let (rw, rh) = if sh > sw { (rh, rw) } else { (rw, rh) };
    let (w, h) = if sw * rh <= sh * rw {
        let h = sh.min(rh);
        (sw * h / sh, h)
    } else {
        let w = sw.min(rw);
        (w, sh * w / sw)
    };
    ((w & !1).max(2), (h & !1).max(2))
}

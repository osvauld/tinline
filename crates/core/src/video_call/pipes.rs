//! The send and receive tasks and codec threads of one call.

use super::*;
use super::codecs::{Decoders, EncoderChoice, decoder_name, encoder_name, keyframe_is, new_encoder};

/// Ladder rung cap for a peer that takes at most `max_pixels`.
pub(super) fn max_rung(ladder: &LadderConfig, max_pixels: u32) -> usize {
    ladder
        .rungs
        .iter()
        .rposition(|r| r.width as u64 * r.height as u64 <= max_pixels as u64)
        .unwrap_or(0)
        .min(ladder.max_rung)
}

pub(super) async fn run_send(conn: Connection, pipe: Arc<Pipe>) {
    let mut cfg = live::sender_config(&pipe);
    (cfg.ladder.max_rung, cfg.ladder.max_fps) = pipe.policy.lock().own_caps();
    let send = match VideoSend::accept(conn, cfg).await {
        Ok(s) => s,
        Err(e) => {
            tracing::info!("video send: {e}");
            return;
        }
    };
    let send = Arc::new(send);
    *pipe.send.lock() = Some(send.clone());
    // A budget set while the sender was starting, and live limits set meanwhile.
    adapt::apply_own_caps(&pipe);
    live::apply_limits(&pipe);
    let spawned = std::thread::Builder::new()
        .name("tl-video-enc".into())
        .spawn(move || encode_loop(pipe, send));
    if let Err(e) = spawned {
        tracing::warn!("video encoder thread: {e}");
    }
}

pub(super) fn encode_loop(pipe: Arc<Pipe>, send: Arc<VideoSend>) {
    video::cpu::pin_to_fast_cores();
    let epoch = Instant::now();
    let mut enc: Option<(Box<dyn VideoEncoder>, usize, usize)> = None;
    let mut rate = 0;
    let mut choice: Option<EncoderChoice> = None;
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
        // the old size and another at the new one. Frames beyond the ladder's frame-rate tier
        // are skipped (the slot is latest-wins, so skipping is free); the encoder stays at
        // `FPS` and gets the bitrate scaled to the tier (`encoder_bitrate`).
        // A live switch (`live`): the new encoder starts with a keyframe, a new group. The peer
        // was told first when the codec changes.
        let want = *pipe.encoder.lock();
        let switched = want.is_some() && want != choice;
        if switched {
            if choice.is_some() {
                tracing::info!("video encoder {} -> {}", choice.map_or("", |c| c.id()), want.map_or("", |c| c.id()));
                // The new encoder's first frame is a keyframe (a new group, below); its first
                // frames are slower, so the CPU check starts again.
                send.with_sender(|s| s.ladder_mut().restart_encode_timing());
            }
            choice = want;
            enc = None;
            let mut st = pipe.stats.lock();
            if let Some(c) = choice {
                st.send_codec = c.codec.as_str().into();
                st.send_hardware = c.hardware;
                st.send_encoder = encoder_name(c).into();
            }
        }
        let Some(choice_now) = choice else { continue };
        let look = send.with_sender(|s| {
            s.take_frame(capture_us).then(|| {
                let look = (s.resolution(), s.encoder_bitrate(), s.poll_keyframe(capture_us));
                (look, s.bitrate(), (s.trace, s.drops, s.draining()), (s.fps(), s.ladder().caps_ignored()))
            })
        });
        let Some((((rw, rh), bitrate, wants_key), target_bps, trace, (tier_fps, ignored))) = look else { continue };
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
            match new_encoder(choice_now, &cfg) {
                Ok(e) => {
                    enc = Some((e, w, h));
                    pipe.stats.lock().tuning_problem.clear();
                }
                Err(e) => {
                    tracing::warn!("video encoder: {e}");
                    pipe.stats.lock().tuning_problem = format!("Video encoder unavailable at {w}x{h}: {e}");
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
                pipe.stats.lock().tuning_problem = format!("Video encode failed: {e}");
                enc = None;
                continue;
            }
        };
        let took = t.elapsed();
        // Stamped when handed to the transport, not at capture: the receiver measures queueing
        // from it, and a busy machine's slow encode is not a queue in the path.
        let sent_us = epoch.elapsed().as_micros() as u64;
        {
            let mut tm = pipe.timing.lock();
            if tm.encode_us.len() < 240 {
                tm.encode_us.push(took.as_micros().min(u32::MAX as u128) as u32);
                tm.lag_us.push(sent_us.saturating_sub(capture_us).min(u32::MAX as u64) as u32);
            }
        }
        let mut st = pipe.stats.lock();
        for p in packets {
            if send.send(sent_us, p.key, &p.data).is_err() {
                return;
            }
            st.record_admitted(p.data.len());
            st.keyframes_sent += p.key as u64;
        }
        send.encode_time(took.as_micros() as u64);
        st.send_width = w as u32;
        st.send_height = h as u32;
        // The rung this very frame was prepared for, from the same look as its size: a readout
        // can't pair one second's rung with another frame's size.
        st.send_rung_height = rw.min(rh);
        st.send_rung_fps = tier_fps;
        st.limits_ignored = ignored;
        st.bitrate_bps = target_bps;
        st.encode_ms = took.as_secs_f32() * 1000.0;
        let (t, drops, draining) = trace;
        (st.rate_reports, st.rate_overuse, st.rate_starved, st.backlog_drops) = (t.reports, t.overuse, t.starved, drops);
        (st.last_queuing_us, st.last_delivered_bps, st.queuing_offset_us) = (t.last_queuing_us, t.last_delivered_bps, t.q_offset_us);
        st.draining = draining;
    }
}

/// `hint_flags`: the peer's sender reads hint flags (its `VideoOffer.hint_flags`); `bursts`: it
/// reads reports with a burst rate (`VideoOffer.burst_probe`).
pub(super) async fn run_recv(conn: Connection, pipe: Arc<Pipe>, codec: VideoCodec, hint_flags: bool, bursts: bool) {
    let mut media = video::media::ReceiverConfig::default();
    media.report_burst = bursts;
    let mut recv = match VideoRecv::new(conn, RecvConfig { hint_flags, media, ..Default::default() }).await {
        Ok(r) => r,
        Err(e) => {
            tracing::info!("video recv: {e}");
            return;
        }
    };
    if let Some(h) = pipe.hint_slot.lock().take() {
        recv.set_hint(h);
    }
    // Small: a decoder that falls behind stops the reading, which the sender sees as backlog.
    let (tx, rx) = mpsc::channel::<Frame>(2);
    let dec_pipe = pipe.clone();
    let spawned = std::thread::Builder::new()
        .name("tl-video-dec".into())
        .spawn(move || decode_loop(dec_pipe, rx, codec));
    if let Err(e) = spawned {
        tracing::warn!("video decoder thread: {e}");
        return;
    }
    loop {
        tokio::select! {
            f = recv.recv() => match f {
                Ok(f) => {
                    pipe.stats.lock().record_received(f.payload.len());
                    let t = Instant::now();
                    if tx.send(f).await.is_err() {
                        return;
                    }
                    let waited = t.elapsed().as_micros().min(u32::MAX as u128) as u32;
                    if waited > 5_000 {
                        let mut tm = pipe.timing.lock();
                        tm.recv_waits += 1;
                        tm.recv_wait_max_us = tm.recv_wait_max_us.max(waited);
                    }
                }
                Err(e) => {
                    tracing::info!("video recv: {e}");
                    return;
                }
            },
            _ = pipe.key_wanted.notified() => recv.request_keyframe(),
            _ = pipe.hint_changed.notified() => {
                if let Some(h) = pipe.hint_slot.lock().take() {
                    recv.set_hint(h);
                }
            }
        }
    }
}

pub(super) fn decode_loop(pipe: Arc<Pipe>, rx: mpsc::Receiver<Frame>, codec: VideoCodec) {
    video::cpu::pin_to_fast_cores();
    decode_loop_with(pipe, rx, codec, Decoders::LOCAL)
}

/// `decode_loop` with its decoders from `decoders` (tests use stand-ins). Swaps the decoder at a
/// keyframe when the peer switches codecs (`Msg::VideoCodec`) or my decoder preference changes
/// (`VideoLive::recv_decoder`, asking the sender for that keyframe once).
pub(super) fn decode_loop_with(pipe: Arc<Pipe>, mut rx: mpsc::Receiver<Frame>, codec: VideoCodec, decoders: Decoders) {
    let mut cur = codec;
    let mut cur_hw = decoders.pick(codec, *pipe.decoder_pref.lock());
    let mut dec = match (decoders.make)(codec, cur_hw, MAX_PIXELS) {
        Ok(d) => d,
        Err(e) => {
            tracing::warn!("video decoder: {e}");
            return;
        }
    };
    set_recv_decoder(&pipe, cur, cur_hw);
    // After an error only a keyframe (a new group) can be decoded.
    let mut broken = false;
    // A keyframe the decoder refused: it may be the peer's new codec arriving before its
    // `Msg::VideoCodec` (another stream); decoded once the message is in.
    let mut held: Option<Frame> = None;
    // A keyframe was asked for to swap to another decoder of the same codec.
    let mut asked = false;
    while let Some(f) = rx.blocking_recv() {
        let mut todo = vec![f];
        // The peer switches codecs: swap at its first keyframe of the new one.
        let pending = {
            let mut p = pipe.recv_switch.lock();
            if *p == Some(cur) {
                *p = None;
            }
            *p
        };
        let pref = *pipe.decoder_pref.lock();
        let mut swap_to = None;
        if let Some(next) = pending {
            let f = &todo[0];
            if f.index == 0 && keyframe_is(next, &f.payload) {
                swap_to = Some(next);
            } else if held.as_ref().is_some_and(|h| keyframe_is(next, &h.payload)) {
                swap_to = Some(next);
                todo.insert(0, held.take().expect("checked"));
            }
        } else if decoders.pick(cur, pref) != cur_hw {
            // Same codec, the other decoder: at the next keyframe, which we ask for once.
            if todo[0].index == 0 {
                swap_to = Some(cur);
            } else if !asked {
                asked = true;
                pipe.key_wanted.notify_one();
            }
        }
        if let Some(next) = swap_to {
            let hw = decoders.pick(next, pref);
            match (decoders.make)(next, hw, MAX_PIXELS) {
                Ok(d) => {
                    tracing::info!("video decoder {}/{} -> {}/{}", cur.as_str(), hw_word(cur_hw), next.as_str(), hw_word(hw));
                    dec = d;
                    (cur, cur_hw, asked) = (next, hw, false);
                    broken = true;
                    *pipe.recv_switch.lock() = None;
                    set_recv_decoder(&pipe, cur, cur_hw);
                }
                Err(e) => tracing::warn!("video decoder {}: {e}", next.as_str()),
            }
        }
        for f in todo {
            decode_one(&pipe, &mut *dec, f, &mut broken, &mut held);
        }
    }
}

fn hw_word(hw: bool) -> &'static str {
    if hw { "hardware" } else { "software" }
}

fn set_recv_decoder(pipe: &Pipe, codec: VideoCodec, hw: bool) {
    {
        let mut st = pipe.stats.lock();
        st.recv_codec = codec.as_str().into();
        st.recv_hardware = hw;
        st.recv_decoder = decoder_name(codec, hw).into();
    }
    // A live codec switch changes how the decode-cost policy reads decode time (VP8 MediaCodec by
    // throughput, AV1 rav1d by CPU time), so it follows the decoder, not the call's first codec.
    pipe.policy.lock().recv_pipelined = super::codecs::pipelined_decoder(codec);
}

/// One frame through the decoder (`decode_loop`).
fn decode_one(pipe: &Pipe, dec: &mut dyn video::VideoDecoder, f: Frame, broken: &mut bool, held: &mut Option<Frame>) {
    {
        if *broken && f.index != 0 {
            return;
        }
        let t = Instant::now();
        let decoded = dec.decode(&f.payload);
        {
            let mut d = pipe.decode_ms.lock();
            d.push_back((t, t.elapsed().as_secs_f32() * 1000.0));
            while d.len() > 240 {
                d.pop_front();
            }
        }
        match decoded {
            Ok(pics) => {
                *broken = false;
                if f.index == 0 {
                    *held = None;
                }
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
                *broken = true;
                if f.index == 0 {
                    *held = Some(f);
                }
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

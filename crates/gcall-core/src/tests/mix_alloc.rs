//! Thread-local Rust allocation audit of the actual decoded client mix path.
//! Does not count C/Opus allocation, other threads, or claim a whole-process RSS result.
use super::world::*;
use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
thread_local! {
    static COUNTING: Cell<bool> = const { Cell::new(false) };
    static ALLOCS: Cell<usize> = const { Cell::new(0) };
}
struct CountingAllocator;
fn count() {
    if COUNTING.try_with(Cell::get).unwrap_or(false) {
        let _ = ALLOCS.try_with(|c| c.set(c.get() + 1));
    }
}
unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        count();
        unsafe { System.alloc(l) }
    }
    unsafe fn alloc_zeroed(&self, l: Layout) -> *mut u8 {
        count();
        unsafe { System.alloc_zeroed(l) }
    }
    unsafe fn realloc(&self, p: *mut u8, l: Layout, n: usize) -> *mut u8 {
        count();
        unsafe { System.realloc(p, l, n) }
    }
    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        unsafe { System.dealloc(p, l) }
    }
}
#[global_allocator]
static ALLOCATOR: CountingAllocator = CountingAllocator;
fn allocations(f: impl FnOnce()) -> usize {
    struct Guard;
    impl Drop for Guard {
        fn drop(&mut self) {
            COUNTING.with(|c| c.set(false));
        }
    }
    ALLOCS.with(|c| c.set(0));
    COUNTING.with(|c| c.set(true));
    let guard = Guard;
    f();
    drop(guard);
    ALLOCS.with(Cell::get)
}
#[test]
fn gcall_mix_mutes_without_stopping_receiver_clocks_or_replaying_on_unmute() {
    let sources = [BOB, CAROL, OLGA];
    let fixture = || {
        let (mut s, _) = base_joined(&[A1, BOB, CAROL, OLGA]);
        let slots = sources.map(|m| s.hosts[&H1].slot_of(&s.did(m)).unwrap());
        let c = s.clients.get_mut(&A1).unwrap();
        c.on_control(
            GroupCallMsg::Active {
                r#gen: c.active().0 + 1,
                slots: slots.to_vec(),
            },
            T0,
            &s.auth,
        )
        .unwrap();
        (s, slots)
    };
    let (mut actual, slots) = fixture();
    let (mut reference, ref_slots) = fixture();
    assert_eq!(slots, ref_slots);
    let dids = sources.map(|m| actual.did(m));
    let mut senders = sources.map(|_| audio::Sender::new(GROUP_BITRATE_BPS).unwrap());
    let mut tones =
        std::array::from_fn::<_, 3, _>(|i| audio::Tone::new(440. + i as f32 * 250., 6000.));
    let c = actual.clients.get_mut(&A1).unwrap();
    let r = reference.clients.get_mut(&A1).unwrap();
    for frame in 0..150 {
        let now = T0 + frame * FRAME_MS;
        if frame == 60 || frame == 100 {
            for did in &dids {
                c.set_muted(did, frame == 60);
            }
        }
        for i in 0..3 {
            let mut pcm = [0_i16; audio::FRAME];
            tones[i].next_frame(&mut pcm);
            let p = senders[i].encode(&pcm).unwrap();
            let d = w::down_audio(slots[i], &p);
            c.on_datagram(&d, now);
            r.on_datagram(&d, now);
        }
        let mut out = [0_i16; audio::FRAME];
        let mut expected = [0_i16; audio::FRAME];
        c.mix(&mut out, now);
        r.mix(&mut expected, now);
        if (60..100).contains(&frame) {
            assert_eq!(out, [0; audio::FRAME]);
        } else {
            assert_eq!(
                out, expected,
                "mute changed decoder timing at frame {frame}"
            );
        }
    }
}
#[test]
fn gcall_mix_steady_decoded_audio_has_no_rust_heap_allocations() {
    let (mut s, _) = base_joined(&[A1, BOB, CAROL, OLGA]);
    let sources = [BOB, CAROL, OLGA];
    let slots = sources.map(|m| s.hosts[&H1].slot_of(&s.did(m)).unwrap());
    let mut senders = sources.map(|_| audio::Sender::new(GROUP_BITRATE_BPS).unwrap());
    let mut tones =
        std::array::from_fn::<_, 3, _>(|i| audio::Tone::new(440. + i as f32 * 250., 6000.));
    let client = s.clients.get_mut(&A1).unwrap();
    client
        .on_control(
            GroupCallMsg::Active {
                r#gen: client.active().0 + 1,
                slots: slots.to_vec(),
            },
            T0,
            &s.auth,
        )
        .unwrap();
    let mut output = [0_i16; audio::FRAME];
    let mut total = 0;
    let mut heard = 0;
    for frame in 0..120 {
        let now = T0 + frame * FRAME_MS;
        for i in 0..3 {
            let mut pcm = [0_i16; audio::FRAME];
            tones[i].next_frame(&mut pcm);
            let packet = senders[i].encode(&pcm).unwrap();
            client.on_datagram(&w::down_audio(slots[i], &packet), now);
        }
        if frame < 30 {
            client.mix(&mut output, now);
        } else {
            total += allocations(|| client.mix(&mut output, now));
            heard += output.iter().filter(|s| s.unsigned_abs() > 500).count();
        }
    }
    assert!(
        heard > audio::FRAME,
        "audit must exercise decoded audio, not silent/unallocated receivers"
    );
    assert_eq!(
        total, 0,
        "per-slot mix buffers allocated {total} times over 90 warmed frames"
    );
}

//! Group voice messages: a voice post carries the 1:1 voice format (Ogg Opus, duration, 64-bar
//! waveform) and travels the group image path (sealed blob, the host fetches it from the author,
//! members fetch it from the host). Real nodes over real iroh endpoints, like `public_group.rs`.
#![allow(dead_code)]

use p2pcore::{GroupImageState as St, PictureDownload, VoiceDecoder, VoiceRecorder};

use crate::support;
use support::group::*;
use support::*;

const S: Suite = Suite("gvoice");

fn peer(name: &str) -> Peer {
    S.peer(name)
}

/// Records `ms` of a 440 Hz tone with the core recorder into `p`'s directory; returns the file and
/// its waveform.
fn recording(p: &Peer, ms: u32) -> (String, Vec<u8>) {
    std::fs::create_dir_all(&p.dir.path()).unwrap();
    let path = p.dir.path().join(format!("rec-{ms}.opus")).to_string_lossy().into_owned();
    let rec = VoiceRecorder::start(path.clone()).unwrap();
    let n = (ms as usize) * 16;
    let pcm: Vec<i16> = (0..n).map(|i| ((i as f32 * 440.0 * std::f32::consts::TAU / 16_000.0).sin() * 8000.0) as i16).collect();
    rec.push(pcm).unwrap();
    let info = rec.finish().unwrap();
    (path, info.waveform)
}

fn voice_of(p: &Peer, g: &str, id: &str) -> Option<p2pcore::GroupVoice> {
    all(p, g).into_iter().find(|m| m.id == id).and_then(|m| m.voice)
}

fn voice_state(p: &Peer, g: &str, id: &str) -> Option<St> {
    voice_of(p, g, id).map(|v| v.state)
}

fn voice_bytes(p: &Peer, g: &str, id: &str) -> Option<Vec<u8>> {
    p.node.group_voice_path(g.into(), id.into()).map(|f| std::fs::read(f).unwrap())
}

// KIND: acceptance (group voice) — a member's voice message reaches another member through the
// host: the post carries the duration (from the file) and the waveform, the sender has it at once,
// the host fetches it from the author, the other member (tap to load) fetches it from the host and
// gets the same bytes, which play for the same duration. The list row shows it as the last post.
#[test]
fn voice_post_roundtrip_via_host() {
    let o = peer("gv-o");
    let a = peer("gv-a");
    let b = peer("gv-b");
    let g = group_with(&o, &[&a, &b]);
    b.node.group_set_image_auto_download(PictureDownload::Tap).unwrap();

    let (path, wave) = recording(&a, 2_000);
    let sent = a.node.group_post_voice(g.clone(), path.clone(), wave.clone()).unwrap();
    assert_eq!(sent.text, "", "a voice post has no text");
    assert!(sent.image.is_none());
    let v = sent.voice.clone().expect("the post carries its recording");
    assert!((1_900..=2_100).contains(&v.duration_ms), "{v:?}");
    assert_eq!(v.waveform, wave);
    assert_eq!(v.size, std::fs::metadata(&path).unwrap().len());
    assert_eq!(v.state, St::Ready, "the sender has it");
    let mine = voice_bytes(&a, &g, &sent.id).expect("the sender opens it at once");
    assert_eq!(mine, std::fs::read(&path).unwrap());
    let last = a.node.groups().unwrap().into_iter().find(|s| s.id == g).and_then(|s| s.last).unwrap();
    assert_eq!(last.id, sent.id);
    assert!(last.voice.is_some(), "the list row knows it is a voice message");

    eventually(WAIT, "the host fetches it from a", || voice_state(&o, &g, &sent.id) == Some(St::Ready));
    assert_eq!(voice_bytes(&o, &g, &sent.id).unwrap(), mine);
    eventually(WAIT, "b sees the post", || voice_of(&b, &g, &sent.id).is_some());
    let seen = voice_of(&b, &g, &sent.id).unwrap();
    assert_eq!((seen.duration_ms, seen.waveform.clone(), seen.size), (v.duration_ms, wave, v.size));
    assert_eq!(seen.state, St::Remote, "on tap only: it waits");
    assert!(b.node.group_voice_path(g.clone(), sent.id.clone()).is_none());
    b.node.group_fetch_voice(g.clone(), sent.id.clone()).unwrap();
    eventually(WAIT, "b loads it from the host", || voice_state(&b, &g, &sent.id) == Some(St::Ready));
    assert_eq!(voice_bytes(&b, &g, &sent.id).unwrap(), mine, "the same recording");
    let file = b.node.group_voice_path(g.clone(), sent.id.clone()).unwrap();
    assert!(file.ends_with(".ogg"), "{file}");
    assert_eq!(VoiceDecoder::open(file).unwrap().duration_ms(), v.duration_ms, "it plays");
}

// KIND: acceptance (group voice) — what is not a voice message is refused before anything is
// posted (not Ogg Opus; a waveform over 64 bars), and in a channel a member's voice message is
// refused like a text post while the owner's goes out.
#[test]
fn voice_post_checks() {
    let o = peer("gvc-o");
    let a = peer("gvc-a");
    let g = group_with(&o, &[&a]);
    std::fs::create_dir_all(&a.dir.path()).unwrap();
    let junk = a.dir.path().join("junk.opus");
    std::fs::write(&junk, vec![7u8; 4096]).unwrap();
    assert!(a.node.group_post_voice(g.clone(), junk.to_string_lossy().into_owned(), vec![]).is_err(), "not Ogg Opus");
    let (path, _) = recording(&a, 800);
    assert!(a.node.group_post_voice(g.clone(), path.clone(), vec![1; 65]).is_err(), "65 bars");
    assert!(all(&a, &g).iter().all(|m| m.voice.is_none()), "nothing was posted");

    o.node.group_set_admins_only(g.clone(), true).unwrap();
    eventually(WAIT, "a sees admins only", || a.node.group_detail(g.clone()).is_ok_and(|d| d.admins_only));
    match a.node.group_post_voice(g.clone(), path, vec![]) {
        Err(p2pcore::Error::Rejected(r)) => assert_eq!(r, "admins only"),
        other => panic!("expected Rejected(admins only), got {other:?}"),
    }
    let (opath, owave) = recording(&o, 1_000);
    let ann = o.node.group_post_voice(g.clone(), opath, owave).unwrap();
    eventually(WAIT, "a gets the owner's voice message", || voice_state(&a, &g, &ann.id) == Some(St::Ready));
}

//! Cold segments of the search index (format version 2, `docs/search.md` "Index v2"): the
//! manifest, the posting-list and doc-chunk codecs, and the builder of one segment.
//!
//! ```text
//! m                       sealed manifest (`Manifest`)
//! x/{seg:08x}/{tok:016x}/{part:03x}
//!                         sealed postings of a long list of one term for the docs `part * PART_DOCS ..`: local
//!                         doc numbers, ascending (newest first), varint deltas from the part base
//! x/{seg:08x}/b{bucket:04x}
//!                         sealed bucket: the short posting lists whose term hash starts with
//!                         `bucket` (`Seg::bbits` bits), as `(tok u64 BE, part, len, list)`
//!                         entries sorted by term. Most lists are short, and a record of their
//!                         own costs ~100 bytes of key, seal and page overhead (measured: 100 MB
//!                         of 127 MB at 100k messages), so they share a record of ~1.2 KB.
//! y/{seg:08x}/{chunk:06x} sealed doc rows, `CHUNK` a chunk, strings shared inside the chunk
//!
//! Every value is kept well under a redb page (4 KiB): measured, large values grow the file by
//! tens of times their size over repeated transactions (1 GiB for 20 MB of records).
//! ```

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use super::index::DocRow;
use super::text::index_terms;

/// `(keyed hash of src, day number)`: one conversation's day.
pub type DayKey = (u64, u32);
/// 8 bytes standing for a version vector (the watermark of a day).
pub type Digest = [u8; 8];
/// "No watermark".
pub const NO_DIGEST: Digest = [0; 8];
/// Docs per `y/` chunk.
pub const CHUNK: usize = 8;
/// Docs covered by one posting-list record.
pub const PART_DOCS: usize = 2048;
/// A list whose encoding is at most this long goes into a bucket record.
pub const SMALL_LIST: usize = 400;
/// Aimed-at size of a bucket record.
pub const BUCKET_BYTES: usize = 1200;

/// One cold segment: the days it holds (a day lives in exactly one segment: the newest that
/// lists it) and bounds of its docs' times.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Seg {
    pub id: u32,
    /// UTC month, `year * 12 + month - 1` (0 for a day that is not a date).
    pub month: u32,
    pub max_at: i64,
    pub min_at: i64,
    pub docs: u32,
    /// Bits of the term hash that select a bucket record.
    #[serde(default)]
    pub bbits: u8,
    /// `(src hash, day number, watermark)`.
    pub days: Vec<(u64, u32, Digest)>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Manifest {
    pub next: u32,
    pub segs: Vec<Seg>,
}

/// The manifest as queries and writers see it (replaced as a whole, never mutated).
#[derive(Debug, Default)]
pub struct View {
    pub man: Manifest,
    /// Which segment owns a day, and its watermark.
    pub owner: HashMap<DayKey, (u32, Digest)>,
}

impl View {
    pub fn new(man: Manifest) -> Self {
        let mut owner = HashMap::new();
        let mut segs: Vec<&Seg> = man.segs.iter().collect();
        segs.sort_by_key(|s| s.id);
        for s in segs {
            for (src, day, d) in &s.days {
                owner.insert((*src, *day), (s.id, *d));
            }
        }
        Self { man, owner }
    }
}

pub fn rev_at(at: i64) -> u64 {
    u64::MAX - at.max(0) as u64
}

pub fn x_key(seg: u32, tok: u64, part: usize) -> String {
    format!("x/{seg:08x}/{tok:016x}/{part:03x}")
}

pub fn b_key(seg: u32, bucket: u32) -> String {
    format!("x/{seg:08x}/b{bucket:04x}")
}

pub fn bucket_of(tok: u64, bbits: u8) -> u32 {
    if bbits == 0 { 0 } else { (tok >> (64 - u32::from(bbits))) as u32 }
}

/// The list of `(tok, part)` in a bucket record, if it is there.
pub fn bucket_find(b: &[u8], tok: u64, part: usize) -> Option<Vec<u32>> {
    let mut pos = 0;
    while pos < b.len() {
        let t = u64::from_be_bytes(b.get(pos..pos + 8)?.try_into().ok()?);
        pos += 8;
        let p = get_var(b, &mut pos)? as usize;
        let n = get_var(b, &mut pos)? as usize;
        let payload = b.get(pos..pos.checked_add(n)?)?;
        pos += n;
        if t > tok {
            return None;
        }
        if t == tok && p == part {
            return decode_list(payload, (part * PART_DOCS) as u32);
        }
    }
    None
}

pub fn y_key(seg: u32, chunk: usize) -> String {
    format!("y/{seg:08x}/{chunk:06x}")
}

pub fn put_var(out: &mut Vec<u8>, mut v: u64) {
    while v >= 0x80 {
        out.push(v as u8 | 0x80);
        v >>= 7;
    }
    out.push(v as u8);
}

pub fn get_var(b: &[u8], pos: &mut usize) -> Option<u64> {
    let mut v = 0u64;
    let mut shift = 0;
    loop {
        let c = *b.get(*pos)?;
        *pos += 1;
        v |= u64::from(c & 0x7f).checked_shl(shift)?;
        if c < 0x80 {
            return Some(v);
        }
        shift += 7;
        if shift > 63 {
            return None;
        }
    }
}

/// Ascending doc numbers (at least `base`) as varint deltas from `base`.
pub fn encode_list(docs: &[u32], base: u32) -> Vec<u8> {
    let mut out = Vec::with_capacity(docs.len() + 4);
    let mut prev = base;
    for &d in docs {
        put_var(&mut out, u64::from(d - prev));
        prev = d;
    }
    out
}

pub fn decode_list(b: &[u8], base: u32) -> Option<Vec<u32>> {
    let mut out = Vec::with_capacity(b.len());
    let (mut pos, mut prev) = (0, u64::from(base));
    while pos < b.len() {
        prev += get_var(b, &mut pos)?;
        out.push(u32::try_from(prev).ok()?);
    }
    Some(out)
}

fn put_str(out: &mut Vec<u8>, s: &str) {
    put_var(out, s.len() as u64);
    out.extend_from_slice(s.as_bytes());
}

fn get_str(b: &[u8], pos: &mut usize) -> Option<String> {
    let n = get_var(b, pos)? as usize;
    let s = std::str::from_utf8(b.get(*pos..pos.checked_add(n)?)?).ok()?.to_string();
    *pos += n;
    Some(s)
}

/// A chunk of rows: the shared strings (src, conv, day, author), then per row the doc hash, the
/// four string indexes, the time, the id and the text.
pub fn encode_chunk(rows: &[(u64, &DocRow)]) -> Vec<u8> {
    let mut idx: HashMap<&str, u64> = HashMap::new();
    let mut table: Vec<&str> = Vec::new();
    let mut refs: Vec<[u64; 4]> = Vec::with_capacity(rows.len());
    for (_, r) in rows {
        let mut four = [0u64; 4];
        for (k, s) in [&r.src, &r.conv, &r.day, &r.author].into_iter().enumerate() {
            let next = table.len() as u64;
            let i = *idx.entry(s.as_str()).or_insert_with(|| {
                table.push(s.as_str());
                next
            });
            four[k] = i;
        }
        refs.push(four);
    }
    let mut out = Vec::new();
    put_var(&mut out, table.len() as u64);
    for s in &table {
        put_str(&mut out, s);
    }
    put_var(&mut out, rows.len() as u64);
    for ((h, r), four) in rows.iter().zip(&refs) {
        out.extend_from_slice(&h.to_le_bytes());
        for i in four {
            put_var(&mut out, *i);
        }
        put_var(&mut out, ((r.at << 1) ^ (r.at >> 63)) as u64);
        put_str(&mut out, &r.id);
        put_str(&mut out, &r.text);
    }
    out
}

pub fn decode_chunk(b: &[u8]) -> Option<Vec<(u64, DocRow)>> {
    let mut pos = 0;
    let n = get_var(b, &mut pos)? as usize;
    let mut table = Vec::with_capacity(n.min(1024));
    for _ in 0..n {
        table.push(get_str(b, &mut pos)?);
    }
    let rows = get_var(b, &mut pos)? as usize;
    let mut out = Vec::with_capacity(rows.min(1024));
    for _ in 0..rows {
        let h = u64::from_le_bytes(b.get(pos..pos + 8)?.try_into().ok()?);
        pos += 8;
        let s = |pos: &mut usize| -> Option<String> { table.get(get_var(b, pos)? as usize).cloned() };
        let (src, conv, day, author) = (s(&mut pos)?, s(&mut pos)?, s(&mut pos)?, s(&mut pos)?);
        let z = get_var(b, &mut pos)?;
        let at = ((z >> 1) as i64) ^ -((z & 1) as i64);
        let id = get_str(b, &mut pos)?;
        let text = get_str(b, &mut pos)?;
        out.push((h, DocRow { src, conv, day, id, at, author, text }));
    }
    Some(out)
}

/// One day of a conversation, as written into a segment.
pub struct DayData {
    pub key: DayKey,
    pub digest: Digest,
    pub rows: Vec<(u64, DocRow)>,
}

/// A segment ready to write: its manifest entry and its plain `(key, value)` records.
pub struct Built {
    pub seg: Seg,
    pub puts: Vec<(String, Vec<u8>)>,
}

/// Builds segment `id` of `month` from whole days. Docs are numbered newest first by
/// `(rev_at, h)`, which is also the order of the cursor.
pub fn build(id: u32, month: u32, days: &[&DayData], tok: &dyn Fn(&str) -> u64) -> Built {
    let mut docs: Vec<(u64, u64, &DocRow)> = days.iter().flat_map(|d| d.rows.iter().map(|(h, r)| (rev_at(r.at), *h, r))).collect();
    docs.sort_by_key(|(rev, h, _)| (*rev, *h));
    let mut lists: HashMap<u64, Vec<u32>> = HashMap::new();
    for (i, (_, _, r)) in docs.iter().enumerate() {
        for t in index_terms(&r.text) {
            lists.entry(tok(&t)).or_default().push(i as u32);
        }
    }
    let mut puts = Vec::with_capacity(docs.len() / CHUNK + 1);
    let mut toks: Vec<(u64, Vec<u32>)> = lists.into_iter().collect();
    toks.sort_by_key(|(t, _)| *t);
    let mut small: Vec<(u64, usize, Vec<u8>)> = Vec::new();
    for (t, l) in &toks {
        for part in l.chunk_by(|a, b| *a as usize / PART_DOCS == *b as usize / PART_DOCS) {
            let p = part[0] as usize / PART_DOCS;
            let enc = encode_list(part, (p * PART_DOCS) as u32);
            if enc.len() <= SMALL_LIST {
                small.push((*t, p, enc));
            } else {
                puts.push((x_key(id, *t, p), enc));
            }
        }
    }
    let total: usize = small.iter().map(|(_, _, e)| e.len() + 11).sum();
    let mut bbits = 0u8;
    while bbits < 16 && (total >> bbits) > BUCKET_BYTES {
        bbits += 1;
    }
    let mut buckets: std::collections::BTreeMap<u32, Vec<u8>> = Default::default();
    for (t, p, enc) in &small {
        let b = buckets.entry(bucket_of(*t, bbits)).or_default();
        b.extend_from_slice(&t.to_be_bytes());
        put_var(b, *p as u64);
        put_var(b, enc.len() as u64);
        b.extend_from_slice(enc);
    }
    for (b, v) in buckets {
        puts.push((b_key(id, b), v));
    }
    for (c, part) in docs.chunks(CHUNK).enumerate() {
        let rows: Vec<(u64, &DocRow)> = part.iter().map(|(_, h, r)| (*h, *r)).collect();
        puts.push((y_key(id, c), encode_chunk(&rows)));
    }
    let seg = Seg {
        id,
        month,
        max_at: docs.iter().map(|(_, _, r)| r.at).max().unwrap_or(0),
        min_at: docs.iter().map(|(_, _, r)| r.at).min().unwrap_or(0),
        docs: docs.len() as u32,
        bbits,
        days: days.iter().map(|d| (d.key.0, d.key.1, d.digest)).collect(),
    };
    Built { seg, puts }
}

#[cfg(test)]
mod tests {
    use super::*;

    // KIND: unit (v2) — the varint and chunk codecs round-trip.
    #[test]
    fn codecs_round_trip() {
        let l = vec![3000, 3001, 3005, 3130, 70_000, 4_000_000];
        assert_eq!(decode_list(&encode_list(&l, 2048), 2048).unwrap(), l);
        let r = |id: &str, at: i64| DocRow { src: "g.a".into(), conv: "a".into(), day: "2026-01-02".into(), id: id.into(), at, author: "did:x".into(), text: format!("héllo {id} 日本") };
        let rows = [r("1", 5), r("2", -3), r("3", 1 << 40)];
        let refs: Vec<(u64, &DocRow)> = rows.iter().enumerate().map(|(i, r)| (i as u64 + 9, r)).collect();
        let back = decode_chunk(&encode_chunk(&refs)).unwrap();
        assert_eq!(back.len(), 3);
        for ((h, got), (want_h, want)) in back.iter().zip(&refs) {
            assert_eq!((h, got), (want_h, *want));
        }
        assert!(decode_chunk(&[9, 9, 9]).is_none());
    }
}

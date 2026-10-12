//! Cold segments of the search index (format version 2, `docs/search.md` "Index v2"): the
//! manifest, the posting-list and doc-chunk codecs, and the builder of one segment.
//!
//! ```text
//! m                       sealed manifest (`Manifest`)
//! x/{seg:08x}/{tok:016x}/{part:03x}
//!                         sealed postings of one term for the docs `part * PART_DOCS ..`: local
//!                         doc numbers, ascending (newest first), varint deltas from the part base
//! y/{seg:08x}/{chunk:06x} sealed doc rows, `CHUNK` a chunk, strings shared inside the chunk
//! z/{seg:08x}/{part:04x}  sealed doc table (format 3): per doc a u16 LE, the index into the
//!                         segment's `days` (`Z_PART` docs a record), for find in chat
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
/// Docs per `z/` record (2 bytes each, so a record stays under a redb page).
pub const Z_PART: usize = 1024;
/// A doc whose day index does not fit a u16: the table cannot tell, the row decides.
pub const Z_NONE: u16 = u16::MAX;

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

pub fn y_key(seg: u32, chunk: usize) -> String {
    format!("y/{seg:08x}/{chunk:06x}")
}

pub fn z_key(seg: u32, part: usize) -> String {
    format!("z/{seg:08x}/{part:04x}")
}

pub fn decode_z(b: &[u8]) -> Option<Vec<u16>> {
    if b.len() % 2 != 0 {
        return None;
    }
    Some(b.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect())
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
    let mut docs: Vec<(u64, u64, &DocRow, u16)> = days
        .iter()
        .enumerate()
        .flat_map(|(di, d)| d.rows.iter().map(move |(h, r)| (rev_at(r.at), *h, r, u16::try_from(di).unwrap_or(Z_NONE).min(Z_NONE))))
        .collect();
    docs.sort_by_key(|(rev, h, _, _)| (*rev, *h));
    let mut lists: HashMap<u64, Vec<u32>> = HashMap::new();
    for (i, (_, _, r, _)) in docs.iter().enumerate() {
        for t in index_terms(&r.text) {
            lists.entry(tok(&t)).or_default().push(i as u32);
        }
    }
    let mut puts = Vec::with_capacity(lists.len() + docs.len() / CHUNK + 1);
    let mut toks: Vec<(u64, Vec<u32>)> = lists.into_iter().collect();
    toks.sort_by_key(|(t, _)| *t);
    for (t, l) in &toks {
        for part in l.chunk_by(|a, b| *a as usize / PART_DOCS == *b as usize / PART_DOCS) {
            let p = part[0] as usize / PART_DOCS;
            puts.push((x_key(id, *t, p), encode_list(part, (p * PART_DOCS) as u32)));
        }
    }
    for (c, part) in docs.chunks(CHUNK).enumerate() {
        let rows: Vec<(u64, &DocRow)> = part.iter().map(|(_, h, r, _)| (*h, *r)).collect();
        puts.push((y_key(id, c), encode_chunk(&rows)));
    }
    for (p, part) in docs.chunks(Z_PART).enumerate() {
        puts.push((z_key(id, p), part.iter().flat_map(|(_, _, _, z)| z.to_le_bytes()).collect()));
    }
    let seg = Seg {
        id,
        month,
        max_at: docs.iter().map(|(_, _, r, _)| r.at).max().unwrap_or(0),
        min_at: docs.iter().map(|(_, _, r, _)| r.at).min().unwrap_or(0),
        docs: docs.len() as u32,
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

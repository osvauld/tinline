/* Profile area runtime state (runes). The simulated core adapter's "app side": cached picture bytes as object URLs,
   keyed by (epoch | owner | hash), loaded from a worker-like async read, discarded when anything in the key changes.
   Accepted profiles live in `db` (store.svelte.ts); everything here is either derived from them or drawer-only. */
import { untrack } from 'svelte';
import { db } from '../store.svelte';
import { clock } from '../clock.svelte';
import { samplePortrait } from '../images';
import type { Profile } from '../types';

/** Simulated worker read of the cached bytes (the real core call is blocking and runs off the UI thread). Not a motion duration. */
const LOAD_MS = 140;

export interface LogLine { id: number; at: number; text: string }
export interface DeletionReq { owner: string; hash: string; at: number }
export interface Held { key: string; owner: string; hash: string; release: () => void }
export type PicState = { state: 'none' | 'awaiting' | 'ready' | 'broken'; url?: string };

class Px {
  /** Bumps on account switch and on lock: results of earlier loads must never land in the new view. */
  epoch = $state(1);
  urls = $state<Record<string, string>>({});
  failed = $state<Record<string, true>>({});
  held = $state<Held[]>([]);
  /** Drawer-only logs. Never shown in the app. */
  log = $state<LogLine[]>([]);
  deletions = $state<DeletionReq[]>([]);
  /** My accepted writes that have not been able to reach the network yet. */
  pending = $state(0);
  /** Updates from other people/devices that arrive once the network is back. */
  inbound = $state<{ label: string; run: () => void }[]>([]);
  sim = $state({
    failNextSave: false,
    unavailable: { arjun: false } as Record<string, boolean>,   // owner id -> the picture bytes can't be read right now
    stallHash: '' as string,                       // the next load of this hash stalls until released
  });
  /** A file offered from the drawer (acts like the picker): the My profile screen consumes it. */
  offered = $state<{ file: Blob; n: number } | null>(null);
}
export const px = new Px();

let logSeq = 0;
export function note(text: string) { px.log.unshift({ id: ++logSeq, at: clock.now, text }); if (px.log.length > 40) px.log.length = 40; }

/* ---------- the byte store the sim's core would hold (not reactive) ---------- */
const blobs = new Map<string, Blob>();
let hashSeq = 0;
export const newHash = () => 'p' + (++hashSeq).toString(16).padStart(6, '0') + 'c0ffee';
export const putBlob = (hash: string, b: Blob) => { blobs.set(hash, b); };
export const hasBlob = (hash: string) => blobs.has(hash) || hash.startsWith('sample:');
/** `sample:<seed>` hashes are generated on first read, so seed data stays synchronous. */
async function readBlob(hash: string): Promise<Blob> {
  const have = blobs.get(hash);
  if (have) return have;
  if (hash.startsWith('sample:')) {
    const seed = hash.slice(7);
    const b = await samplePortrait(seed, 384, { light: seed.includes('light') });
    blobs.set(hash, b);
    return b;
  }
  throw new Error('missing');
}

/* ---------- lookups ---------- */
export const profileOf = (owner: string): Profile | undefined => (owner === 'me' ? db.account.profile : db.people.find((p) => p.id === owner)?.profile);
const keyOf = (epoch: number, owner: string, hash: string) => `${epoch}|${owner}|${hash}`;
const nameOf = (owner: string) => (owner === 'me' ? 'you' : db.people.find((p) => p.id === owner)?.name ?? owner);

/** What an avatar should draw for this person right now. */
export function pictureFor(owner: string): PicState {
  const pic = profileOf(owner)?.picture;
  if (!pic || db.account.gate !== 'ready') return { state: 'none' };
  const key = keyOf(px.epoch, owner, pic.hash);
  if (px.failed[key]) return { state: 'broken' };
  const url = pic.ready ? px.urls[key] : undefined;
  return url ? { state: 'ready', url } : { state: 'awaiting' };
}

/* ---------- loading ---------- */
const inflight = new Set<string>();
const currentHash = (owner: string) => profileOf(owner)?.picture?.hash;

function load(epoch: number, owner: string, hash: string) {
  const key = keyOf(epoch, owner, hash);
  inflight.add(key);
  const finish = async () => {
    inflight.delete(key);
    const stale = () => epoch !== px.epoch || currentHash(owner) !== hash || db.account.gate !== 'ready';
    if (stale()) { note(`Dropped a finished image load for ${nameOf(owner)}: ${epoch !== px.epoch ? 'the account changed' : 'a newer picture is selected'}.`); return; }
    try {
      if (px.sim.unavailable[owner]) throw new Error('unavailable');
      const blob = await readBlob(hash);
      if (stale()) { note(`Dropped a finished image load for ${nameOf(owner)}: it changed while reading.`); return; }
      px.urls[key] = URL.createObjectURL(blob);
    } catch {
      if (!stale()) px.failed[key] = true;
    }
  };
  if (px.sim.stallHash === hash) {
    px.sim.stallHash = '';
    px.held.push({ key, owner, hash, release: () => { px.held = px.held.filter((h) => h.key !== key); void finish(); } });
    note(`Image load for ${nameOf(owner)} (${hash.slice(0, 12)}) is stalled.`);
    return;
  }
  setTimeout(finish, LOAD_MS);
}

/** Forget every cached image of this session: object URLs revoked, loads in flight will be dropped. */
export function clearImages(why: string) {
  for (const u of Object.values(px.urls)) URL.revokeObjectURL(u);
  px.urls = {}; px.failed = {};
  inflight.clear();                // loads still running will be dropped when they finish (the epoch moved)
  px.epoch++;
  note(`Cleared cached pictures: ${why}.`);
}

/** Drop one person's decoded image and read it again (after the drawer changes whether it can be read). */
export function reloadPicture(owner: string) {
  const h = currentHash(owner);
  if (!h) return;
  const k = keyOf(px.epoch, owner, h);
  if (px.urls[k]) { URL.revokeObjectURL(px.urls[k]); delete px.urls[k]; }
  delete px.failed[k];
  inflight.delete(k);
  px.failed = { ...px.failed };    // wakes the loader
}

export function resetProfileState() {
  clearImages('data reset');
  px.held = []; px.log = []; px.deletions = []; px.pending = 0; px.inbound = []; px.offered = null;
  px.sim.failNextSave = false; px.sim.unavailable = { arjun: false }; px.sim.stallHash = '';
}

/* Started a tick late: store.svelte.ts imports this file before `db` exists. */
if (typeof window !== 'undefined') queueMicrotask(() => {
  $effect.root(() => {
    /* Read every selected picture; start the load for the ones that are cached but not yet decoded; revoke the rest. */
    $effect(() => {
      if (db.account.gate !== 'ready') return;
      const epoch = px.epoch;
      const live = new Set<string>();
      const want: { owner: string; hash: string }[] = [];
      const own = db.account.profile?.picture;
      if (own?.ready) { live.add(keyOf(epoch, 'me', own.hash)); want.push({ owner: 'me', hash: own.hash }); }
      for (const p of db.people) {
        const pic = p.profile?.picture;
        if (pic?.ready) { live.add(keyOf(epoch, p.id, pic.hash)); want.push({ owner: p.id, hash: pic.hash }); }
      }
      const urls = px.urls, failed = px.failed;
      untrack(() => {
        for (const k of Object.keys(urls)) if (!live.has(k)) { URL.revokeObjectURL(urls[k]); delete px.urls[k]; }
        for (const k of Object.keys(failed)) if (!live.has(k)) delete px.failed[k];
        for (const w of want) {
          const k = keyOf(epoch, w.owner, w.hash);
          if (!urls[k] && !failed[k] && !inflight.has(k)) load(epoch, w.owner, w.hash);
        }
      });
    });
    /* Locking forgets this session's pictures; unlocking reads them again. */
    let was = db.account.gate;
    $effect(() => {
      const g = db.account.gate;
      untrack(() => { if (g !== 'ready' && was === 'ready') clearImages('the app locked'); was = g; });
    });
    /* Reconnect: my queued writes go out and queued updates from others arrive (catch-up). */
    $effect(() => {
      if (!db.env.networkOnline) return;
      untrack(() => {
        if (px.pending) { note(`Reconnected: ${px.pending} saved change${px.pending === 1 ? '' : 's'} sent to your devices and contacts.`); px.pending = 0; }
        const q = px.inbound; px.inbound = [];
        for (const u of q) u.run();
      });
    });
  });
});

if (typeof window !== 'undefined') (window as any).__px = px; // tests read the sim logs and held loads

/* Gate (sign-up / unlock) state machine. Transient flow state lives here (secrets are never put in `db`);
   the only thing written to the store is the outcome: db.account.name and db.account.gate = 'ready'.
   Mirrors the real onboarding order (Onboarding.kt / Unlock.kt on dev) except where noted in the sim notes.
   Not a `.ts` file: it holds runes, so it must be `.svelte.ts`. */
import { db } from '../store.svelte';
import { clock } from '../clock.svelte';
import { prefs } from '../prefs.svelte';
import { WORDS } from '../../../screens/gate/wordlist';

export type Step =
  | 'welcome' | 'choose'
  | 'name' | 'pass' | 'phrase' | 'confirm' | 'device' | 'terms' | 'perms' | 'done'
  | 'linkName' | 'link' | 'linkPass'
  | 'restore' | 'restoreName' | 'restorePass'
  | 'forgotPass'
  | 'unlock' | 'keylost' | 'savefailed' | 'savepass';

export type Flow = 'create' | 'restore' | 'link' | 'forgot';
export type LinkReason = 'expired' | 'cancelled' | 'rejected' | 'mismatch' | 'timeout' | 'net' | 'locked';
export type LinkPhase = 'showing' | 'scanned' | 'approved' | 'failed';
export type PermState = 'idle' | 'granted' | 'denied';

/** The recovery phrase every simulated account has (24 words from the real list, fixed for stable tests/screens). */
export const DEMO_PHRASE = 'abandon ability able about above absent absorb abstract absurd abuse access accident account accuse achieve acid acoustic acquire across act action actor actress actual'.split(' ');
/** 1-based word positions asked on the confirm step. */
export const CONFIRM_AT = [4, 11, 19] as const;

/** Copy for each link failure (verbatim from Link.kt linkFailText where it exists). */
export const LINK_FAIL: Record<LinkReason, string> = {
  expired: 'The code expired. Try again.',
  cancelled: 'Cancelled on the other device.',
  rejected: 'That QR code was already used or isn’t valid. Make a fresh one and try again.',
  mismatch: 'These two devices don’t belong to the same account, so nothing was shared.',
  timeout: 'The other device didn’t answer in time. Keep both open and try again.',
  net: 'Couldn’t reach the other device. Check that both are online.',
  locked: 'The other device is locked. Unlock Tinline there and try again.',
};

/** Simulation switches (GateSim.svelte). Defaults are the happy path. */
export const gateSim = $state({
  failSave: false,          // storage refuses to keep the account (key could not be stored)
  failRestore: false,       // "phrase restore fails": a well-formed phrase that opens nothing
  permResult: 'grant' as 'grant' | 'deny',
  passphrase: 'correct horse',   // what unlocks; replaced by whatever the user sets in the flow
  hasPassphrase: true,      // false => locked account has only the phone's own key => key lost
  maxTries: 5,              // PROPOSED: real app never locks out, it is only deliberately slow
  lockSeconds: 30,          // PROPOSED
  kdfMs: 700,               // how long "Encrypting…/Unlocking…" takes (simulated Argon2)
  remembered: true,         // the phone remembers the passphrase (Android Keystore)
});

class Gate {
  step = $state<Step>('welcome');
  flow = $state<Flow>('create');
  dir = $state<1 | -1>(1);          // 1 forward, -1 back; drives the slide direction
  history = $state<Step[]>([]);
  busy = $state(false);
  error = $state<string | null>(null);

  // form fields (transient)
  name = $state('');
  pass = $state('');
  pass2 = $state('');
  skippedPass = $state(false);
  words = $state<string[]>(Array(24).fill(''));
  agreed = $state(false);
  deviceLabel = $state('');
  perms = $state<Record<string, PermState>>({ mic: 'idle', notif: 'idle', lock: 'idle', battery: 'idle', camera: 'idle' });
  peer = $state('');                 // name of the device we linked to

  // link session
  link = $state({ phase: 'showing' as LinkPhase, expiresAt: 0, reason: null as LinkReason | null, nonce: 1 });

  // unlock
  tries = $state(0);
  lockedUntil = $state(0);
  remember = $state(true);

  get isAndroid() { return prefs.form === 'android'; }
  get noun() { return this.isAndroid ? 'phone' : 'computer'; }
  get defaultLabel() { return this.isAndroid ? 'Personal phone' : 'Home computer'; }

  /** Effective link state: a session past its expiry reads as expired. */
  get linkState(): { phase: LinkPhase; reason: LinkReason | null } {
    const l = this.link;
    if ((l.phase === 'showing' || l.phase === 'scanned') && clock.now >= l.expiresAt) return { phase: 'failed', reason: 'expired' };
    return { phase: l.phase, reason: l.reason };
  }
  get linkCode() { return '482 913'; }
  get linkLeftMs() { return Math.max(0, this.link.expiresAt - clock.now); }
  get lockLeft() { return Math.max(0, Math.ceil((this.lockedUntil - clock.now) / 1000)); }

  /* ---------- navigation ---------- */
  go(s: Step, dir: 1 | -1 = 1) {
    if (dir === 1) this.history.push(this.step);
    this.dir = dir;
    this.error = null;
    this.step = s;
  }
  back() {
    if (!this.canBack) return;
    const to = this.history.pop();
    if (!to) return;
    this.dir = -1; this.error = null; this.busy = false;
    if (this.step === 'link') this.cancelLinkSession();
    this.step = to;
  }
  /** Steps that cannot be backed out of (the account already exists or the step is a root). */
  get canBack() {
    return !(['welcome', 'phrase', 'terms', 'perms', 'done', 'unlock', 'keylost', 'savefailed', 'linkPass'] as Step[]).includes(this.step)
      && this.history.length > 0;
  }

  /** Reset every field and enter a root step. Called when the harness changes db.account.gate. */
  enter(gate: 'onboarding' | 'locked') {
    this.history = [];
    this.busy = false; this.error = null; this.dir = 1;
    this.name = ''; this.pass = ''; this.pass2 = ''; this.skippedPass = false;
    this.words = Array(24).fill(''); this.agreed = false; this.deviceLabel = ''; this.peer = '';
    this.perms = { mic: 'idle', notif: 'idle', lock: 'idle', battery: 'idle', camera: 'idle' };
    this.tries = 0; this.lockedUntil = 0; this.remember = gateSim.remembered;
    this.flow = 'create';
    this.step = gate === 'locked' ? (gateSim.hasPassphrase ? 'unlock' : 'keylost') : 'welcome';
  }
  /** Sim: jump straight to any step. Fills enough state for the step to make sense. */
  jump(s: Step, flow?: Flow) {
    const locked = s === 'unlock' || s === 'keylost';
    if (locked) { gateSim.hasPassphrase = s === 'unlock'; }
    db.account.gate = locked ? 'locked' : 'onboarding';
    this.enter(locked ? 'locked' : 'onboarding');
    if (flow) this.flow = flow; else this.flow = flowOf(s);
    this.name = this.flow === 'forgot' ? db.account.name : 'Maya Kurian';
    this.deviceLabel = this.defaultLabel;
    if (s === 'link') this.beginLinkSession();
    if (s === 'restore' || s === 'restoreName' || s === 'restorePass' || s === 'forgotPass') this.words = [...DEMO_PHRASE];
    this.dir = 1;
    this.step = s;
  }

  /* ---------- roots ---------- */
  startCreate() { this.flow = 'create'; this.go('name'); }
  startRestore() { this.flow = 'restore'; this.go('restore'); }
  startLink() { this.flow = 'link'; this.deviceLabel ||= this.defaultLabel; this.go('linkName'); }
  forgot() {
    this.flow = 'forgot'; this.name = db.account.name; this.error = null;
    this.words = Array(24).fill('');
    this.go('restore');
  }

  /* ---------- create / restore ---------- */
  private sleep(ms: number) { return new Promise<void>((r) => setTimeout(r, ms)); }

  /** Passphrase step submitted (or skipped). Persists the identity, the way finishIdentity() does. */
  async submitPass(skip: boolean) {
    if (this.busy) return;
    this.busy = true; this.error = null;
    await this.sleep(gateSim.kdfMs);
    this.busy = false;
    this.skippedPass = skip;
    if (!skip) { gateSim.passphrase = this.pass; gateSim.hasPassphrase = true; } else gateSim.hasPassphrase = false;
    this.pass = ''; this.pass2 = '';
    if (this.flow === 'restore' && gateSim.failRestore) {
      this.error = 'That phrase isn’t valid. Check each word and their order.';
      this.history = this.history.filter((h) => h !== 'restore' && h !== 'restoreName' && h !== 'restorePass');
      this.dir = -1; this.step = 'restore';
      return;
    }
    if (this.flow === 'forgot' && gateSim.failRestore) {
      this.error = 'That phrase doesn’t open this account. Check each word and their order.';
      this.dir = -1; this.step = 'restore'; return;
    }
    if (gateSim.failSave) { this.go('savefailed'); return; }
    this.afterPersist();
  }
  /** Retry from Save-failed (the sim switch decides whether it works this time). */
  async retrySave() {
    if (this.busy) return;
    this.busy = true;
    await this.sleep(gateSim.kdfMs);
    this.busy = false;
    if (gateSim.failSave) { this.error = 'Still couldn’t store the key. Nothing was saved yet.'; return; }
    this.error = null;
    this.afterPersist();
  }
  /** "Add a passphrase instead": the key is protected by the passphrase, not the system key store. */
  async savePassAlt() {
    if (this.busy) return;
    this.busy = true; this.error = null;
    await this.sleep(gateSim.kdfMs);
    this.busy = false;
    gateSim.passphrase = this.pass; gateSim.hasPassphrase = true; this.skippedPass = false;
    this.pass = ''; this.pass2 = '';
    this.afterPersist();
  }
  private afterPersist() {
    if (this.flow === 'create') this.go('phrase');
    else if (this.flow === 'restore') this.go('device');
    else if (this.flow === 'link') this.go('terms');
    else this.finish();
  }

  /** Words screen: validate against the list; the real validity (checksum) is decided at the passphrase step. */
  restoreWordsOk() { return this.words.every((w) => WORDS.includes(w)); }

  /** Confirm step: the asked words match. */
  confirmOk(answers: string[]) { return CONFIRM_AT.every((p, i) => answers[i]?.trim().toLowerCase() === DEMO_PHRASE[p - 1]); }

  /** Device label saved. */
  nextAfterDevice() {
    if (this.flow === 'link') { this.beginLinkSession(); this.go('link'); } else this.go('terms');
  }

  /** End of every flow. */
  finish() {
    if (this.name.trim()) db.account.name = this.name.trim();
    db.account.gate = 'ready';
    this.enter('onboarding');
  }

  /* ---------- permissions (OS prompt is simulated) ---------- */
  allow(k: string) { this.perms[k] = gateSim.permResult === 'grant' ? 'granted' : 'denied'; }

  /* ---------- link (new device) ---------- */
  beginLinkSession() {
    this.link = { phase: 'showing', expiresAt: clock.now + 5 * 60_000, reason: null, nonce: this.link.nonce + 1 };
  }
  cancelLinkSession() { this.link.phase = 'failed'; this.link.reason = 'cancelled'; }
  simScan() { if (this.linkState.phase === 'showing') { this.link.phase = 'scanned'; this.peer = 'Maya’s Pixel'; } }
  simApprove() {
    if (this.linkState.phase !== 'scanned') return;
    this.link.phase = 'approved';
    this.name = db.account.name;     // the account name arrives with the grant
    this.busy = true;
    this.sleep(gateSim.kdfMs).then(() => {
      this.busy = false;
      if (this.step === 'link' && this.link.phase === 'approved') this.go('linkPass');
    });
  }
  simExpire() { this.link.expiresAt = clock.now; }
  simCancel() { this.link.phase = 'failed'; this.link.reason = 'cancelled'; }
  simFail(r: LinkReason) { this.link.phase = 'failed'; this.link.reason = r; }

  /* ---------- unlock ---------- */
  async unlock(pass: string) {
    if (this.busy || this.lockLeft > 0) return;
    this.busy = true; this.error = null;
    await this.sleep(gateSim.kdfMs);
    this.busy = false;
    if (pass === gateSim.passphrase) {
      gateSim.remembered = this.isAndroid ? this.remember : false;
      this.tries = 0;
      db.account.gate = 'ready';
      this.enter('onboarding');
      return;
    }
    this.tries++;
    if (this.tries >= gateSim.maxTries) {
      this.lockedUntil = clock.now + gateSim.lockSeconds * 1000;
      this.tries = 0;
      this.error = 'Too many tries.';
    } else this.error = 'That passphrase didn’t open the account.';
  }
  /** Key-lost "Try again": the sim keeps the key missing unless the toggle was changed. */
  async retryKey() {
    this.busy = true; await this.sleep(gateSim.kdfMs); this.busy = false;
    if (gateSim.hasPassphrase) this.step = 'unlock'; else this.error = 'Still can’t find the key on this ' + this.noun + '.';
  }
}

export function flowOf(s: Step): Flow {
  if (s === 'linkName' || s === 'link' || s === 'linkPass') return 'link';
  if (s === 'restore' || s === 'restoreName' || s === 'restorePass') return 'restore';
  if (s === 'forgotPass') return 'forgot';
  return 'create';
}

export const gate = new Gate();

/** Progress dots: [n, total] or null for steps without a counter. Totals follow the real order. */
export function progressOf(step: Step, flow: Flow): [number, number] | null {
  const orders: Record<Flow, Step[]> = {
    create: ['name', 'pass', 'phrase', 'confirm', 'device', 'terms', 'perms'],
    restore: ['restore', 'restoreName', 'restorePass', 'device', 'terms', 'perms'],
    link: ['linkName', 'link', 'linkPass', 'terms', 'perms'],
    forgot: [],
  };
  const o = orders[flow];
  const i = o.indexOf(step);
  return i < 0 ? null : [i + 1, o.length];
}

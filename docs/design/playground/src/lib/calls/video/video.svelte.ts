/* The video half of a 1:1 call (PLAN step 5). Standalone state next to the call machine: call.svelte.ts only
   tells it when a call begins, connects, ends and resets (four one-line hooks), and asks it which kind to record.

   Rules shown here (core: AV1 video over the call connection, audio never waits behind it):
   - Camera is OFF by default. A call placed from the video button turns MY camera on once connected (that is the
     choice made by pressing it); a voice call, or an incoming call answered the plain way, starts with it off.
   - Front camera first; Flip switches (phones). Self-view is mirrored for the front camera only.
   - Either side can turn video on or off at any time: voice <-> video is just the camera switch.
   - Low bandwidth: the video freezes with a calm note, audio continues. Reconnecting freezes it too.
   - The camera can be denied by the OS, or missing (desktop): a calm note, the call goes on.
   Everything is simulated: generated feeds. The real webcam is used only when the drawer enables it. */

import { device } from '../../model/device.svelte';

export type Corner = 'tl' | 'tr' | 'bl' | 'br';
export type CamNotice = 'denied' | 'nocamera' | 'failed' | null;

class Vid {
  /* ---- per call ---- */
  intent = $state(false);          // the call was placed / received as a video call
  wantCam = $state(false);         // turn my camera on as soon as the call connects
  live = $state(false);            // connected at least once; the video layer exists only then
  camOn = $state(false);
  starting = $state(false);        // camera warming up (real webcam asks the browser)
  facing = $state<'front' | 'back'>('front');
  peerCam = $state(false);
  lowBw = $state(false);
  /** Sharper video for this call (2026-10-15: the HD chip on the call screen). Starts from Settings > Video. */
  hd = $state(false);
  /** A short line under the chip after a tap or a pause ('' = none). */
  hdNote = $state('');
  everVideo = $state(false);       // any camera was on during the call -> recorded as a video call
  corner = $state<Corner>('tr');
  chrome = $state(true);           // controls and title visible (they hide after a few idle seconds during video)
  notice = $state<CamNotice>(null);
  stream = $state<MediaStream | null>(null);

  /* ---- environment (the drawer; survives calls) ---- */
  perm = $state<'granted' | 'denied'>('granted');
  hasCamera = $state(true);
  source = $state<'sim' | 'real'>('sim');
  hideMs = 4000;
  peerDelay = 900;

  #hide = 0; #note = 0; #peer = 0; #seq = 0;

  get peerSeen() { return this.live && this.peerCam; }
  /** Their picture fills the screen (frozen while low bandwidth / reconnecting; the call machine adds that). */
  get stageOn() { return this.peerSeen; }
  /** Anything about this call is video: label, strip icon, "their camera is off" note. */
  get videoCall() { return this.intent || this.camOn || this.peerCam || this.everVideo; }
  recordKind(): 'voice' | 'video' { return this.intent || this.everVideo ? 'video' : 'voice'; }

  /* ---- hooks from the call machine ---- */
  begin(video: boolean, wantCam = video) {
    this.reset();
    this.hd = device.hd;
    this.intent = video; this.wantCam = wantCam;
  }
  /** Answering an incoming video call: with the camera off (default) or on. */
  answerWith(camera: boolean) { this.wantCam = camera; }
  connected() {
    this.live = true;
    if (this.wantCam) { this.wantCam = false; void this.setCamera(true); }
    if (this.intent) this.#peer = window.setTimeout(() => this.setPeerCam(true), this.peerDelay);
    this.poke();
  }
  ended() { this.stopCamera(); window.clearTimeout(this.#peer); }
  reset() {
    this.stopCamera(); window.clearTimeout(this.#hide); window.clearTimeout(this.#note); window.clearTimeout(this.#peer);
    this.intent = false; this.wantCam = false; this.live = false; this.peerCam = false; this.lowBw = false; this.everVideo = false;
    this.facing = 'front'; this.notice = null; this.chrome = true; this.starting = false;
  }

  /* ---- my camera ---- */
  async toggleCamera() { await this.setCamera(!this.camOn); }
  async setCamera(on: boolean) {
    if (!on) { this.stopCamera(); this.poke(); return; }
    if (this.camOn || this.starting) return;
    if (!this.hasCamera) { this.#show('nocamera'); return; }
    if (this.perm === 'denied') { this.#show('denied'); return; }
    const seq = ++this.#seq;
    if (this.source === 'real') {
      this.starting = true;
      try {
        const s = await navigator.mediaDevices.getUserMedia({ video: { facingMode: this.facing === 'front' ? 'user' : 'environment' }, audio: false });
        if (seq !== this.#seq || !this.live) { s.getTracks().forEach((t) => t.stop()); this.starting = false; return; }
        this.stream = s;
      } catch (e) {
        this.starting = false;
        const n = (e as DOMException)?.name;
        this.#show(n === 'NotFoundError' || n === 'OverconstrainedError' ? 'nocamera' : n === 'NotAllowedError' || n === 'SecurityError' ? 'denied' : 'failed');
        return;
      }
      this.starting = false;
    }
    this.camOn = true; this.everVideo = true; this.notice = null; this.poke();
  }
  stopCamera() {
    this.#seq++;
    this.stream?.getTracks().forEach((t) => t.stop()); this.stream = null;
    this.camOn = false; this.starting = false;
  }
  flip() {
    if (!this.camOn) return;
    this.facing = this.facing === 'front' ? 'back' : 'front';
    this.poke();
  }

  /* ---- their side (drawer) ---- */
  setPeerCam(on: boolean) { this.peerCam = on; if (on) this.everVideo = true; this.poke(); }
  setLowBw(on: boolean) {
    const was = this.hdPaused;
    this.lowBw = on;
    if (!was && this.hdPaused) this.#hdSay('HD paused · weak connection');
  }

  /* ---- sharper video (HD chip) ---- */
  /** On, but the connection can't carry it right now: the ladder stepped down by itself. */
  get hdPaused() { return this.hd && this.lowBw; }
  /** Applies at once: the camera restarts quietly (~0.7 s, the self-view shows it warming up). */
  setHd(on: boolean) {
    if (this.hd === on) return;
    this.hd = on;
    if (this.camOn) {
      this.starting = true;
      window.setTimeout(() => { this.starting = false; }, 700);
    }
    this.#hdSay(on ? (this.lowBw ? 'HD paused · weak connection' : 'Sharper video on') : 'Sharper video off');
    this.poke();
  }
  #hdT = 0;
  #hdSay(t: string) { this.hdNote = t; window.clearTimeout(this.#hdT); this.#hdT = window.setTimeout(() => (this.hdNote = ''), 2600); }

  /* ---- notices ---- */
  #show(n: Exclude<CamNotice, null>) {
    this.notice = n; this.poke();
    window.clearTimeout(this.#note);
    this.#note = window.setTimeout(() => (this.notice = null), 9000);
  }
  dismissNotice() { window.clearTimeout(this.#note); this.notice = null; }

  /* ---- self-view corner ---- */
  nextCorner() { const o: Corner[] = ['tr', 'br', 'bl', 'tl']; this.corner = o[(o.indexOf(this.corner) + 1) % 4]; }

  /* ---- controls auto-hide (only while their video fills the screen) ---- */
  poke() {
    window.clearTimeout(this.#hide);
    this.chrome = true;
    if (this.stageOn && !this.notice) this.#hide = window.setTimeout(() => (this.chrome = false), this.hideMs);
  }
  /** Tap on the picture: show, or hide at once. A mouse click only shows (moving the mouse already woke them). */
  tapStage(toggle = true) {
    if (!this.stageOn) return;
    if (this.chrome && toggle) { window.clearTimeout(this.#hide); this.chrome = false; } else this.poke();
  }
  /** Keep the controls up while the pointer or focus is on them. */
  hold() { window.clearTimeout(this.#hide); this.chrome = true; }
}

export const vid = new Vid();
if (typeof window !== 'undefined') (window as any).__pgVid = vid;

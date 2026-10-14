/* Live voice for the call cloud: the microphone instead of the made-up talk bursts in model.ts makeVoice.
   Same descriptors as the native audio-motion viewer (bench/group-audio-capacity: tools/audio-motion-sim/cloud.rs
   `microphone_input`, crates/gcall-bench/src/spectral.rs), so the playground and that viewer answer a voice alike:
   - level: RMS as -dBov (the level byte), drive = (60 - level) / 50 x sensitivity (0.5..3, default 1.6), clamped 0..1;
   - audible only above RMS 200 of 16-bit (about -44 dBFS), else no input at all;
   - balance: high + 0.5 x mid of the power spectrum, bands <500 Hz, 500-2500 Hz, >=2500 Hz, DC left out. Not pitch.
   The envelopes (45 ms attack, 320 ms release, ...) live in model.ts, as they do in cloud.rs `Field::step`.
   Playground only: in the apps this would come from the call's decoded audio (their voice), not a second mic. */

const KEY = 'tinline.pg.voicein';
const saved = (() => { try { return JSON.parse(localStorage.getItem(KEY) ?? 'null'); } catch { return null; } })();
/** RMS 200 of 16-bit full scale: the viewer's experimental audibility gate (not a VAD). */
const GATE = 200 / 32768;

class VoiceIn {
  /** true while the mic drives the field (the simulated voice is used otherwise). */
  on = $state(false);
  /** Last error from getUserMedia (permission refused, no device), shown in the sim drawer / lab. */
  error = $state<string | null>(null);
  /** Drive 0..1 after the gate and sensitivity, and spectral balance 0 (low) .. 1 (high), updated every frame. */
  level = $state(0);
  balance = $state(0);
  /** Raw RMS in dBFS and the band fractions of the last frame (for the lab's meters). */
  db = $state(-100);
  bands = $state<[number, number, number]>([0, 0, 0]);
  audible = $state(false);
  /** Visual gain on the drive only (never the gate): 0.5..3, the viewer's default 1.6. */
  sensitivity = $state<number>(saved?.sensitivity ?? 1.6);

  #ctx: AudioContext | null = null;
  #stream: MediaStream | null = null;
  #an: AnalyserNode | null = null;
  #buf: Float32Array<ArrayBuffer> | null = null;
  #spec: Float32Array<ArrayBuffer> | null = null;
  #raf = 0;

  async start() {
    if (this.on) return;
    this.error = null;
    try {
      // AGC off so loud and quiet speech stay different; echo cancellation so the call's own sound does not feed it.
      this.#stream = await navigator.mediaDevices.getUserMedia({ audio: { echoCancellation: true, noiseSuppression: true, autoGainControl: false } });
    } catch (e) {
      this.error = e instanceof Error ? e.message : String(e);
      return;
    }
    this.#ctx = new AudioContext();
    const src = this.#ctx.createMediaStreamSource(this.#stream);
    this.#an = this.#ctx.createAnalyser();
    this.#an.fftSize = 1024;               // as spectral.rs (N = 1024)
    this.#an.smoothingTimeConstant = 0;    // the model does the smoothing
    this.#buf = new Float32Array(this.#an.fftSize);
    this.#spec = new Float32Array(this.#an.frequencyBinCount);
    src.connect(this.#an);
    this.on = true;
    const tick = () => { this.#sample(); this.#raf = requestAnimationFrame(tick); };
    this.#raf = requestAnimationFrame(tick);
  }

  stop() {
    cancelAnimationFrame(this.#raf);
    this.#stream?.getTracks().forEach((t) => t.stop());
    void this.#ctx?.close();
    this.#ctx = null; this.#stream = null; this.#an = null; this.#buf = null; this.#spec = null;
    this.on = false; this.level = 0; this.balance = 0; this.db = -100; this.audible = false; this.bands = [0, 0, 0];
  }

  toggle() { if (this.on) this.stop(); else void this.start(); }

  save() {
    try { localStorage.setItem(KEY, JSON.stringify({ sensitivity: this.sensitivity })); } catch { /* private mode */ }
  }

  #sample() {
    if (!this.#an || !this.#buf || !this.#spec || !this.#ctx) return;
    this.#an.getFloatTimeDomainData(this.#buf);
    let s = 0;
    for (const x of this.#buf) s += x * x;
    const rms = Math.sqrt(s / this.#buf.length);
    const db = 20 * Math.log10(rms + 1e-9);
    this.db = db;
    this.audible = rms > GATE;
    if (!this.audible) { this.level = 0; this.balance = 0; this.bands = [0, 0, 0]; return; }  // cloud.rs: Input::default()
    const lvl = Math.min(127, Math.max(0, Math.round(-db)));
    const gain = Math.min(3, Math.max(0.5, this.sensitivity));
    this.level = Math.min(1, Math.max(0, (60 - lvl) / 50 * gain));
    this.#an.getFloatFrequencyData(this.#spec);
    const hzPer = this.#ctx.sampleRate / this.#an.fftSize;
    let lo = 0, mid = 0, hi = 0;
    for (let i = 1; i < this.#spec.length; i++) {   // DC left out
      const p = 10 ** (this.#spec[i] / 10), hz = i * hzPer;
      if (hz < 500) lo += p; else if (hz < 2500) mid += p; else hi += p;
    }
    const e = lo + mid + hi;
    if (e > 0) { lo /= e; mid /= e; hi /= e; this.bands = [lo, mid, hi]; this.balance = hi + 0.5 * mid; }
    else this.balance = 0.5;
  }
}

export const voiceIn = new VoiceIn();
if (typeof window !== 'undefined') (window as any).__pgVoiceIn = voiceIn;

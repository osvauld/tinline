// @ts-nocheck  (numerical port of the approved prototype aud/callfx/clouds/clouds.js; typed at the API edge in index.ts)
/* Pure model for the call-screen cloud field: no DOM, no timers, no audio. Time is passed in (ms).
   Differences from the prototype: the note schedule comes from the CURRENT sound palette (the caller
   passes the pattern snapshot audio.ts plays), cue start times are absolute (bus event + audio's own
   delay), the dark colours are retuned (see RAW), and there is a `reconnecting` state. */

  const F_MIN = 196, F_MAX = 1568;
  const K = 5;

  const clamp = (x, a, b) => Math.min(b, Math.max(a, x));
  const smooth = (x) => { x = clamp(x, 0, 1); return x * x * (3 - 2 * x); };
  const lerp = (a, b, s) => a + (b - a) * s;

  // ---- OKLab -------------------------------------------------------------------------------------
  const s2l = (c) => { c /= 255; return c <= .04045 ? c / 12.92 : Math.pow((c + .055) / 1.055, 2.4); };
  const l2s = (c) => { c = clamp(c, 0, 1); return 255 * (c <= .0031308 ? 12.92 * c : 1.055 * Math.pow(c, 1 / 2.4) - .055); };
  function rgbToLab(rgb) {
    const r = s2l(rgb[0]), g = s2l(rgb[1]), b = s2l(rgb[2]);
    const l = Math.cbrt(.4122214708 * r + .5363325363 * g + .0514459929 * b);
    const m = Math.cbrt(.2119034982 * r + .6806995451 * g + .1073969566 * b);
    const s = Math.cbrt(.0883024619 * r + .2817188376 * g + .6299787005 * b);
    return [.2104542553 * l + .7936177850 * m - .0040720468 * s, 1.9779984951 * l - 2.4285922050 * m + .4505937099 * s, .0259040371 * l + .7827717662 * m - .8086757660 * s];
  }
  function labToRgb(c) {
    const l = Math.pow(c[0] + .3963377774 * c[1] + .2158037573 * c[2], 3), m = Math.pow(c[0] - .1055613458 * c[1] - .0638541728 * c[2], 3), s = Math.pow(c[0] - .0894841775 * c[1] - 1.2914855480 * c[2], 3);
    return [l2s(4.0767416621 * l - 3.3077115913 * m + .2309699292 * s), l2s(-1.2684380046 * l + 2.6097574011 * m - .3413193965 * s), l2s(-.0041960863 * l - .7034186147 * m + 1.7076147010 * s)];
  }
  /** OKLab mix; chroma is interpolated separately so a teal->gold mid-mix does not go grey/olive. */
  function mixLab(a, b, s) {
    const L = lerp(a[0], b[0], s), A = lerp(a[1], b[1], s), B = lerp(a[2], b[2], s);
    const c0 = Math.hypot(a[1], a[2]), c1 = Math.hypot(b[1], b[2]), cm = Math.hypot(A, B), ct = lerp(c0, c1, s);
    const k = cm > 1e-5 ? ct / cm : 1;
    return [L, A * k, B * k];
  }

  const RAW = {
    light: { bg: [245, 246, 243], teal: [110, 210, 186], gold: [248, 206, 104], ink: '#17201d' },
    dark:  { bg: [15, 21, 19],    teal: [20, 184, 160], gold: [236, 126, 64], ink: '#e6ece9' },
  };
  const THEMES = {};
  for (const k of Object.keys(RAW)) THEMES[k] = Object.assign({ name: k, bgLab: rgbToLab(RAW[k].bg), tealLab: rgbToLab(RAW[k].teal), goldLab: rgbToLab(RAW[k].gold) }, RAW[k]);

  // ---- tunables ----------------------------------------------------------------------------------
  const DEFAULTS = {
    omega: .09,          // base Lissajous angular rate, rad/s of field time tau (spec 0.05-0.12)
    sigma0: .26,         // lobe radius, short-side units
    swell: .36,          // sigma_i = sigma0 (1 + s L)         (state value is eased; this is the default)
    gamma: .5,           // perceptual exponent on the PAT decay (as callfx.js)
    aNorm: .55,          // sum of note levels that gives tanh(1)
    spill: 1.6,          // band kernel half-width in bands: primary lobe 1, neighbour .375, 2 away 0
    colourMs: 150,       // lobe colour ease toward its latest note
    capLight: .76,       // max composite coverage in light mode  (contrast-tuned)
    capDark: .74,        // ... dark mode
    warpScale: 1.9,      // base noise frequency
    noiseRate: .35,      // noise z advance per unit tau
    dtMax: 100,          // ms; clamps integration step (tab hidden etc.)
    xfMs: 180,           // reduced-motion crossfade
    pullAmt: .65,        // fraction of the way lobes are pulled to the avatar when pull=1
  };

  const LJ = [ // irrational frequency ratios; home position is a fraction of the half-extent
    { hx: -.55, hy: -.50, ax: .30, ay: .34, wx: 1.0000, wy: 1.3247, px: .3, py: 1.9 },
    { hx: .62,  hy: -.20, ax: .34, ay: .28, wx: .7071,  wy: 1.6180, px: 2.2, py: .7 },
    { hx: -.30, hy: .10,  ax: .28, ay: .38, wx: 1.4142, wy: .8660,  px: 4.1, py: 3.3 },
    { hx: .50,  hy: .45,  ax: .36, ay: .30, wx: 1.1180, wy: 1.3591, px: 1.0, py: 5.0 },
    { hx: -.15, hy: .80,  ax: .32, ay: .32, wx: .9069,  wy: 1.7321, px: 5.5, py: 2.4 },
  ];

  // ---- note helpers (verbatim from callfx.js) ----------------------------------------------------
  function noteLevel(n, tau) {
    const [, , dur, peak, atk] = n;
    if (tau <= 0 || tau >= dur) return 0;
    if (tau < atk) return peak * tau / atk;
    return peak * Math.pow(0.0001 / peak, (tau - atk) / (dur - atk));
  }
  function pitchU(f) { return clamp(Math.log(f / F_MIN) / Math.log(F_MAX / F_MIN), 0, 1); }
  function activeNotes(p, tauMs, loop) {
    const out = [];
    if (!p || tauMs < 0) return out;
    const tau = tauMs / 1000, per = p.period / 1000;
    const maxEnd = Math.max(...p.notes.map((n) => n[1] + n[2]));
    const kHi = loop && per ? Math.floor(tau / per) : 0;
    const kLo = loop && per ? Math.max(0, Math.floor((tau - maxEnd) / per)) : 0;
    for (let k = kLo; k <= kHi; k++) for (const n of p.notes) {
      const s = tau - k * per - n[1];
      if (s > 0 && s < n[2]) out.push({ n, tau: s });
    }
    return out;
  }
  function rng(seed) { return () => { seed |= 0; seed = seed + 0x6D2B79F5 | 0; let t = Math.imul(seed ^ seed >>> 15, 1 | seed); t = t + Math.imul(t ^ t >>> 7, 61 | t) ^ t; return ((t ^ t >>> 14) >>> 0) / 4294967296; }; }
  function makeVoice(seed) {
    const r = rng(seed), bursts = []; let t = 600;
    const extend = (upTo) => { while (t < upTo) { const d = 700 + r() * 1800, g = 500 + r() * 1500; bursts.push({ s: t, e: t + d, amp: .55 + r() * .45, ph: r() * 6.28 }); t += d + g; } };
    return (ms) => {
      extend(ms + 6000); let v = 0;
      for (let i = bursts.length - 1; i >= 0 && i > bursts.length - 40; i--) {
        const b = bursts[i]; if (ms < b.s) continue;
        const up = smooth((ms - b.s) / 160), rel = ms > b.e ? Math.exp(-(ms - b.e) / 260) : 1;
        const syl = .78 + .22 * Math.sin(ms / 1000 * 2 * Math.PI * 3.6 + b.ph) * Math.sin(ms / 1000 * 2 * Math.PI * .9 + 1);
        v = Math.max(v, b.amp * up * rel * syl);
        if (ms > b.e + 2000) break;
      }
      return v;
    };
  }
  /** latest note onset <= t and next onset > t of a cue started at c0 (ms). */
  function scanOnsets(p, loop, c0, t) {
    let latest = null, next = null;
    if (!p) return { latest, next };
    const per = p.period, lo = loop && per ? Math.floor((t - c0) / per) : 0;
    for (let k = Math.max(0, lo - 1); k <= (loop && per ? lo + 1 : 0); k++) p.notes.forEach((n, i) => {
      const ot = c0 + k * per + n[1] * 1000;
      if (ot <= t) { if (!latest || ot >= latest.time) latest = { time: ot, key: k + ':' + i, f: n[0] }; }
      else if (next == null || ot < next) next = ot;
    });
    return { latest, next };
  }

  // ---- state table -------------------------------------------------------------------------------
  // v: field speed; a0: resting lobe alpha; warp: domain-warp amplitude; pull: attraction to avatar;
  // spread: outward dispersal; chroma: colour saturation; bias: 0 teal .. 1 gold; kappa: gust gain;
  // swell/ak: sigma swell and alpha gain per unit note level. All numeric fields are EASED (never snapped).
  // blend: ms to ~95% of any state change; tc: per-field time-constant overrides (ms).
  const STATES = {
    calm:         { text: 'Ready',            cue: null,        loop: false, v: .50, a0: 0.8, warp: .08,  pull: 0, spread: 0,  chroma: 1, bias: .35, kappa: 1,   swell: .30, ak: 0.45, blend: 900 },
    incoming:     { text: 'Incoming call',    cue: 'ring',      loop: true,  v: 1.00, a0: 0.9, warp: .12,  pull: 0, spread: 0,  chroma: 1, bias: .74, kappa: 2.6, swell: .38, ak: 1.0, blend: 300 },
    outgoing:     { text: 'Calling…',         cue: 'back',      loop: true,  v: .42, a0: 0.85, warp: .12,  pull: 0, spread: 0,  chroma: 1, bias: .08, kappa: 2.2, swell: .38, ak: 1.1, blend: 450 },
    connecting:   { text: 'Connecting…',      cue: null,        loop: false, v: .80, a0: .85, warp: .012, pull: 1, spread: 0,  chroma: 1, bias: .12, kappa: 1.6, swell: .30, ak: 0.7, blend: 350, tc: { warp: 330, pull: 520 } },
    active:       { text: 'Connected',        cue: 'connected', loop: false, v: .30, a0: 0.6, warp: .07,  pull: 0, spread: 0,  chroma: 1, bias: .30, kappa: 1,   swell: .30, ak: 0.6, blend: 1200, voice: true, tc: { pull: 1400 } },
    // Call waiting: the active field leaning toward the incoming tone, tinted by the soft waiting cue. Same parameters as the others.
    waiting:      { text: 'Connected',        cue: 'waiting',   loop: true,  v: .34, a0: 0.65, warp: .07, pull: 0, spread: 0,  chroma: 1, bias: .52, kappa: 1.2, swell: .32, ak: 0.8, blend: 900, voice: true },
    reconnecting: { text: 'Reconnecting…',    cue: 'pulse',     loop: false, v: .22, a0: 0.55, warp: .05, pull: .5, spread: 0, chroma: .45, bias: .20, kappa: 1.2, swell: .30, ak: 0.8, blend: 700, tc: { pull: 900, chroma: 500 } },
    ended:        { text: 'Call ended',       cue: 'ended',     loop: false, v: .60, a0: 0.8, warp: .10,  pull: 0, spread: .55, chroma: 1, bias: .40, kappa: 1.6, swell: .30, ak: 0.6, blend: 300, fade: { delay: 0, dur: 300 }, tc: { spread: 320 } },
    failed:       { text: 'Couldn’t connect', cue: 'failed',    loop: false, v: .30, a0: 0.7, warp: .05,  pull: 0, spread: 0,  chroma: 0, bias: .20, kappa: 1.2, swell: .30, ak: 0.6, blend: 500, fade: { delay: 350, dur: 650 }, tc: { chroma: 160 } },
  };
  const EASED = ['v', 'a0', 'warp', 'pull', 'spread', 'chroma', 'kappa', 'swell', 'ak'];
  
  function create(opts?: any) {
    opts = opts || {};
    const P = Object.assign({}, DEFAULTS, opts.params);
    let theme = THEMES[opts.theme || 'light'];
    let reduced = !!opts.reduced, muteG = 1, muted = false;
    const voice = makeVoice(opts.seed || 7);
    const view = { w: 390, h: 844, avx: 195, avy: 178 };
    let cur = null, prevs = [], id = 0;
    const S = {};                 // eased state parameters
    const lu = [.5, .5, .5, .5, .5], tu = [.5, .5, .5, .5, .5], key = [null, null, null, null, null];
    let tau = 0, lastT = null, fadeU = 1, lastFrame = null;
    let xf = null, rKey = null, rNoteU = null; // reduced-motion crossfade
    let disp = { alpha: [0, 0, 0, 0, 0], u: lu.slice(), chroma: 1 };

    const paletteU = (cfg, i) => clamp(cfg.bias + (i - 2) * .07, 0, 1);
    // low=teal, high=gold. Dark: a steeper curve, because a dark teal/amber mid-mix reads as olive,
    // so lobes commit to one colour and the blend happens spatially, where it is brief.
    const curve = (u) => { const x = smooth(clamp((u - .2) / .75, 0, 1)); return theme.name === 'dark' ? smooth(smooth(x)) : x; };
    const colourAt = (u, chroma) => { const c = mixLab(theme.tealLab, theme.goldLab, curve(u)); return [c[0], c[1] * chroma, c[2] * chroma]; };
    const halfExt = () => { const s = Math.min(view.w, view.h); return [view.w / s / 2, view.h / s / 2]; };
    const avatarPt = () => { const s = Math.min(view.w, view.h); return [(view.avx - view.w / 2) / s, (view.avy - view.h / 2) / s]; };

    function lobePos(i, tauV, H, pull, spread) {
      const j = LJ[i], w = P.omega, sx = Math.max(1, H[0] / .5), sy = Math.max(1, H[1] / .5);
      let x = j.hx * H[0] + j.ax * sx * Math.sin(j.wx * w * tauV + j.px);
      let y = j.hy * H[1] + j.ay * sy * Math.sin(j.wy * w * tauV + j.py);
      if (pull > 0) { const a = avatarPt(), g = pull * P.pullAmt; x = lerp(x, a[0], g); y = lerp(y, a[1], g); }
      if (spread) { x *= 1 + spread; y *= 1 + spread; }
      return [x, y];
    }

    /** o.cueStart: absolute model time (ms) of the cue's first note (the bus event time plus the same
        delay audio.ts applies); o.pat: the pattern snapshot audio.ts plays for this cue. */
    function setState(name, t, o) {
      const cfg = STATES[name]; if (!cfg) throw new Error('unknown state ' + name);
      if (cur) prevs.push({ st: cur, at: t, ms: 220 });
      cur = { id: ++id, name, t0: t, c0: (o && o.cueStart != null) ? o.cueStart : t, cfg, pat: (o && o.pat) || null };
      if (S.v === undefined) for (const k of EASED) S[k] = cfg[k];
      for (let i = 0; i < K; i++) { tu[i] = paletteU(cfg, i); key[i] = null; if (!lastFrame) lu[i] = tu[i]; }
      if (lastT === null) lastT = t;
      rKey = null; rNoteU = null;
      if (reduced) startXf(t);
    }

    // ---- reduced motion: static gradient, per-note colour crossfade --------------------------------
    function staticTarget(noteU) {
      const cfg = cur.cfg, a = cfg.fade ? 0 : cfg.a0, u = [];
      for (let i = 0; i < K; i++) { const p = paletteU(cfg, i); u.push(noteU == null ? p : lerp(p, noteU, .75)); }
      return { alpha: [a, a, a, a, a], u, chroma: cfg.chroma };
    }
    function startXf(t0) {
      const to = staticTarget(rNoteU);
      xf = { t0, from: { alpha: disp.alpha.slice(), u: disp.u.slice(), chroma: disp.chroma }, to };
    }
    function reducedFrame(t) {
      const cfg = cur.cfg, sc = scanOnsets(cur.pat, cfg.loop, cur.c0, t), lt = sc.latest;
      if (cfg.cue && cur.pat && lt && lt.key !== rKey) { rKey = lt.key; rNoteU = pitchU(lt.f); startXf(Math.max(lt.time, t - P.xfMs)); }
      if (!xf) startXf(t - P.xfMs);
      const e = smooth((t - xf.t0) / P.xfMs), f = xf.from, g = xf.to;
      const alpha = [], u = []; for (let i = 0; i < K; i++) { alpha.push(lerp(f.alpha[i], g.alpha[i], e)); u.push(lerp(f.u[i], g.u[i], e)); }
      const chroma = lerp(f.chroma, g.chroma, e);
      disp = { alpha, u: u.slice(), chroma };
      const H = halfExt(), lobes = [];
      for (let i = 0; i < K; i++) { const p = lobePos(i, 7.3, H, 0, 0); lobes.push({ x: p[0], y: p[1], sigma: P.sigma0, alpha: alpha[i], lab: colourAt(u[i], chroma) }); }
      const animating = e < 1;
      return { state: cur.name, text: cfg.text, t, tau: 7.3, E: 0, L: [0, 0, 0, 0, 0], lobes, warp: 0, bgLab: theme.bgLab, cap: capOf(), fade: 1, animating,
        wakeAt: animating ? null : sc.next, voice: 0, reduced: true };
    }
    const capOf = () => theme.name === 'dark' ? P.capDark : P.capLight;

    function addNotes(st, t, k, Lraw, mark) {
      const cfg = st.cfg; if (!cfg.cue || !st.pat || k <= 0) return;
      for (const { n, tau: s } of activeNotes(st.pat, t - st.c0, cfg.loop)) {
        const e = noteLevel(n, s), pk = n[3], lv = pk * Math.pow(e / pk, P.gamma) * k, uu = pitchU(n[0]) * K;
        for (let i = 0; i < K; i++) {
          const w = Math.max(0, 1 - Math.abs(uu - (i + .5)) / P.spill);
          if (w <= 0) continue;
          Lraw[i] += lv * w;
          if (mark && w >= .3) { const onset = Math.round(t - s * 1000), prev = mark[i]; if (!prev || onset >= prev.onset) mark[i] = { onset, f: n[0] }; }
        }
      }
    }

    function update(t) {
      if (!cur) throw new Error('setState first');
      const dt = lastT === null ? 0 : clamp(t - lastT, 0, P.dtMax); lastT = t;
      muteG += ((muted ? 0 : 1) - muteG) * (1 - Math.exp(-dt / 120));
      if (reduced) { const f = reducedFrame(t); lastFrame = f; return f; }
      const cfg = cur.cfg;
      // eased parameters (continuous by construction)
      for (const k of EASED) {
        const tc = ((cfg.tc && cfg.tc[k]) || cfg.blend) / 3.2;
        if (S[k] === undefined) S[k] = cfg[k];
        S[k] += (cfg[k] - S[k]) * (1 - Math.exp(-dt / tc));
      }
      // note levels per lobe (current cue + dying previous cues)
      const Lraw = [0, 0, 0, 0, 0], mark = [null, null, null, null, null];
      addNotes(cur, t, 1, Lraw, mark);
      prevs = prevs.filter((p) => t - p.at < p.ms);
      for (const p of prevs) addNotes(p.st, t, 1 - smooth((t - p.at) / p.ms), Lraw, null);
      const L = Lraw.map((x) => Math.tanh(x / P.aNorm));
      let E = 0; for (const x of L) E += x;
      // colours: ease toward the latest note of the lobe's band, else the state palette
      const ctc = Math.max(P.colourMs, cfg.blend / 4);
      for (let i = 0; i < K; i++) {
        if (mark[i]) { const k2 = cur.id + ':' + mark[i].onset; if (key[i] !== k2) { key[i] = k2; tu[i] = pitchU(mark[i].f); } }
        lu[i] += (tu[i] - lu[i]) * (1 - Math.exp(-dt / ctc));
      }
      // gust: integrate the field's own clock; speed rises with energy, never a jump
      const since = t - cur.t0, vo = cfg.voice ? voice(since) * muteG : 0;
      tau += dt / 1000 * S.v * (1 + S.kappa * E + .5 * vo);
      // fade
      const inFade = cfg.fade && since >= cfg.fade.delay;
      fadeU = clamp(fadeU + (inFade ? -dt / cfg.fade.dur : dt / 400), 0, 1);
      const F = smooth(fadeU);
      const H = halfExt(), lobes = [];
      for (let i = 0; i < K; i++) {
        const p = lobePos(i, tau, H, S.pull, S.spread);
        const sigma = P.sigma0 * (1 + S.swell * L[i] + .06 * vo) * (1 - .3 * S.pull);
        const alpha = (S.a0 + .07 * vo + S.ak * L[i]) * F;
        lobes.push({ x: p[0], y: p[1], sigma, alpha, lab: colourAt(lu[i], S.chroma) });
      }
      disp = { alpha: lobes.map((l) => l.alpha), u: lu.slice(), chroma: S.chroma };
      const animating = !(cfg.fade && fadeU <= 0);
      const f = { state: cur.name, text: cfg.text, t, tau, E, L, lobes, warp: S.warp, bgLab: theme.bgLab, cap: capOf(), fade: F, animating, wakeAt: null, voice: vo, reduced: false };
      lastFrame = f; return f;
    }

    return {
      P, setState, update,
      setView(w, h, avx, avy) { view.w = w; view.h = h; view.avx = avx; view.avy = avy; },
      setMute(m) { muted = !!m; },
      setTheme(n) { theme = THEMES[n]; },
      setReduced(r) {
        r = !!r; if (r === reduced) return;
        reduced = r;
        if (r) { rKey = null; rNoteU = null; xf = null; }
        else if (cur) { for (let i = 0; i < K; i++) lu[i] = disp.u[i]; for (const k of EASED) S[k] = cur.cfg[k]; }
      },
      get reduced() { return reduced; },
      get state() { return cur && cur.name; },
      get tau() { return tau; },
    };
  }


export { create, STATES, THEMES, K, rgbToLab, labToRgb };

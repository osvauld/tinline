// @ts-nocheck
/* Tinline mark D: parametric model + animation. Ported from the verified reference
   (session scratchpad aud/logo/mark/mark.js, checks a–j in verify.js) with the §6 corrections of
   MARK-MATH.md applied as defaults: exact parabola length, twang A = 0.5.
   Colours come from CSS vars: --tm-can, --tm-rim, --tm-mouth, --tm-str, --tm-ink (mono).
   Do not hand-edit geometry here; change parameters. */
const DEF = { c: 7.5, b: 5.5, theta: 30.6, mu: 0.62, L: 3, s0: 3.5, ell: null, strokeW: 1.5,
  phi0: 12, T: 1.6, A: 0.5, f: 7, tau: 0.11, noCap: false, lengthModel: 'exact', hitOver: 1.1,
  connectHz: 3, endZeta: 1.6, endMs: 260 };
const f3 = v => { const r = Math.round(v * 1000) / 1000; return String(r === 0 ? 0 : r); };
const rad = d => d * Math.PI / 180;

// ---- length model ---------------------------------------------------------
// exact parabola arc length for span w, sag s
function lenExact(w, s) { s = Math.abs(s); if (s < 1e-9) return w;
  return 0.5 * Math.sqrt(w * w + 16 * s * s) + (w * w / (8 * s)) * Math.asinh(4 * s / w); }
function lenApprox(w, s) { return w + 8 * s * s / (3 * w); }
function ellFromS0(w0, s0, model) { return model === 'exact' ? lenExact(w0, s0) : lenApprox(w0, s0); }
function sOfW(w, ell, model) {
  if (w >= ell) return 0;
  const sa = Math.sqrt(3 * w * (ell - w) / 8);
  if (model !== 'exact') return sa;
  let lo = 0, hi = Math.max(sa * 1.5, 1e-6);
  while (lenExact(w, hi) < ell) hi *= 2;
  for (let i = 0; i < 40; i++) { const m = (lo + hi) / 2; (lenExact(w, m) < ell ? (lo = m) : (hi = m)); }
  return (lo + hi) / 2;
}
function resolve(p) {
  const q = Object.assign({}, DEF, p);
  q.a = p && p.a != null ? p.a : q.b * Math.sin(rad(q.theta));
  q.w0 = 24 - 2 * q.c;
  if (q.ell == null) q.ell = ellFromS0(q.w0, q.s0, q.lengthModel);
  q.DT = Math.max(0, (q.ell - q.w0) / 2);
  return q;
}
function stringPath(c, delta, s, phi) {
  const x1 = c - delta, x2 = 24 - x1;
  return 'M ' + f3(x1) + ' 12 Q ' + f3(12 + 2 * s * Math.sin(phi)) + ' ' + f3(12 + 2 * s * Math.cos(phi)) + ' ' + f3(x2) + ' 12';
}
// pure: returns SVG path strings. p.delta = outward displacement (cans drawn at c - delta).
function markPaths(p) {
  const q = resolve(p), delta = q.delta || 0, phi = q.phi || 0;
  const cx = q.c - delta, a = q.a, b = q.b, L = q.L, ym = 12 - b, yp = 12 + b;
  const w = 24 - 2 * cx;
  const s = q.s != null ? q.s : sOfW(w, q.ell, q.lengthModel);
  const ell = (rx, ry) => 'M ' + f3(cx - rx) + ' 12 A ' + f3(rx) + ' ' + f3(ry) + ' 0 1 0 ' + f3(cx + rx) + ' 12 A ' + f3(rx) + ' ' + f3(ry) + ' 0 1 0 ' + f3(cx - rx) + ' 12 Z';
  const body = q.noCap
    ? 'M ' + f3(cx) + ' ' + f3(ym) + ' H ' + f3(cx - L) + ' V ' + f3(yp) + ' H ' + f3(cx) + ' Z'
    : 'M ' + f3(cx) + ' ' + f3(ym) + ' H ' + f3(cx - L) + ' A ' + f3(a) + ' ' + f3(b) + ' 0 0 0 ' + f3(cx - L) + ' ' + f3(yp) + ' H ' + f3(cx) + ' Z';
  const rim = ell(a, b), mouth = ell(q.mu * a, q.mu * b);
  const sil = q.noCap ? 'M ' + f3(cx - L) + ' ' + f3(ym) + ' H ' + f3(cx) + ' A ' + f3(a) + ' ' + f3(b) + ' 0 0 1 ' + f3(cx) + ' ' + f3(yp) + ' H ' + f3(cx - L) + ' Z' : 'M ' + f3(cx - L) + ' ' + f3(ym) + ' H ' + f3(cx) + ' A ' + f3(a) + ' ' + f3(b) + ' 0 0 1 ' + f3(cx) + ' ' + f3(yp) +
    ' H ' + f3(cx - L) + ' A ' + f3(a) + ' ' + f3(b) + ' 0 0 1 ' + f3(cx - L) + ' ' + f3(ym) + ' Z';
  const str = stringPath(q.c, delta, s, phi);
  const left = cx - L - a;
  return { canLeft: body, rim, mouth, string: str, silhouette: sil, mono: sil + ' ' + mouth,
    bbox: { x0: left, x1: 24 - left, y0: ym, y1: yp, w: 24 - 2 * left, h: 2 * b },
    s, w, ell: q.ell, DT: q.DT, strokeW: q.strokeW };
}
// size-tier overrides (px = rendered size)
function tier(px) {
  if (px >= 32) return { strokeW: 1.5 };
  if (px >= 24) return { strokeW: 1.6, mu: 0.58 };
  return { strokeW: 1.75, noCap: false };
}

// ---- DOM ------------------------------------------------------------------
const NS = 'http://www.w3.org/2000/svg';
const el = (n, at) => { const e = document.createElementNS(NS, n); for (const k in at) e.setAttribute(k, at[k]); return e; };
const MIRROR = 'translate(24 0) scale(-1 1)';
function build(svg, mono) {
  svg.textContent = '';
  const mk = () => {
    const g = el('g', {});
    if (mono) { const m = el('path', { 'fill-rule': 'evenodd' }); m.style.fill = 'var(--tm-ink,currentColor)'; g.append(m); return { g, m }; }
    const body = el('path', {}), rim = el('path', {}), mouth = el('path', {});
    body.style.fill = 'var(--tm-can,#0B6B5B)'; rim.style.fill = 'var(--tm-rim,var(--tm-can,#0B6B5B))'; mouth.style.fill = 'var(--tm-mouth,#9FD6C8)';
    g.append(body, rim, mouth); return { g, body, rim, mouth };
  };
  const L = mk(), R = mk(); R.g.setAttribute('transform', MIRROR);
  const str = el('path', { fill: 'none', 'stroke-linecap': 'round' });
  str.style.stroke = mono ? 'var(--tm-ink,currentColor)' : 'var(--tm-str,#C98A1E)';
  svg.append(L.g, R.g, str);
  return { L, R, str, mono: !!mono };
}
function paintCans(r, d) {
  for (const s of [r.L, r.R]) {
    if (r.mono) s.m.setAttribute('d', d.mono);
    else { s.body.setAttribute('d', d.canLeft); s.rim.setAttribute('d', d.rim); s.mouth.setAttribute('d', d.mouth); }
  }
}
// static render by the same markPaths
function render(svg, params) {
  svg.setAttribute('viewBox', (params && params.viewBox) || '0 0 24 24');
  const p = Object.assign({}, params), r = build(svg, p.mono);
  const d = markPaths(p); paintCans(r, d);
  r.str.setAttribute('d', d.string); r.str.setAttribute('stroke-width', f3(d.strokeW));
  return d;
}

// ---- animation ------------------------------------------------------------
function attach(svg, opts) {
  opts = opts || {};
  let P = resolve(opts.params || {});
  const clock = { now: opts.now || (() => performance.now()),
    raf: opts.raf || (f => requestAnimationFrame(f)), caf: opts.caf || (i => cancelAnimationFrame(i)) };
  let timeScale = opts.timeScale || 1;
  let forceReduced = opts.reduced == null ? null : !!opts.reduced;
  const refs = build(svg, P.mono);
  svg.setAttribute('viewBox', P.viewBox || svg.getAttribute('viewBox') || '0 0 24 24');
  const ev = { hit: [], settled: [], frame: [] };
  const emit = (n, a) => ev[n].forEach(fn => fn(a));
  // state
  const S = { D: 0, V: 0, phi: 0, phv: 0, ring: false, ringPhase: 0, target: 'slack', hit: false, tw: -1, rafId: 0, last: 0, running: false, t: 0, endSwingPending: false, wasMoving: false };
  let lastStr = '', lastL = '', lastR = '', lastSW = '';

  function reduced() {
    if (forceReduced != null) return forceReduced;
    if (document.documentElement.getAttribute('data-motion') === 'reduce') return true;
    return !!(root.matchMedia && root.matchMedia('(prefers-reduced-motion: reduce)').matches);
  }
  const omega = () => 2 * Math.PI * P.connectHz;
  let eoKey='',eoVal=0;
  function endOmega() { const k0=P.endZeta+'|'+P.endMs+'|'+P.DT; if(k0===eoKey)return eoVal; eoKey=k0; eoVal=endOmega0(); return eoVal; }
  function endOmega0() { // calibrate omega so |D|<0.01 (from DT) takes endMs for zeta=endZeta
    const z = P.endZeta, k = z + Math.sqrt(z * z - 1), m = z - Math.sqrt(z * z - 1);
    const thr = 0.01 / Math.max(P.DT, 1e-6);
    const x = u => (k * Math.exp(-m * u) - m * Math.exp(-k * u)) / (k - m); // unit-omega, x(0)=1, v(0)=0
    let lo = 0, hi = 200; for (let i = 0; i < 60; i++) { const mid = (lo + hi) / 2; (x(mid) > thr ? (lo = mid) : (hi = mid)); }
    return hi / (P.endMs / 1000);
  }
  // sagOnly tiers (<= 20 px): the cans never move; taut is shown by the sag easing from s0 to sT
  const mv = () => (P.sagOnly ? 0 : 1);
  function sag() { if (P.sagOnly) { const k = P.DT > 0 ? Math.min(1, S.D / P.DT) : 0; return P.s0 + (P.sT - P.s0) * k; }
    const w = 24 - 2 * (P.c - S.D); return sOfW(w, P.ell, P.lengthModel); }
  function twang() { if (S.tw < 0) return 0; return P.A * Math.exp(-S.tw / P.tau) * Math.sin(2 * Math.PI * P.f * S.tw); }
  function state() { const s = sag() + twang(); return { delta: S.D * mv(), w: 24 - 2 * (P.c - S.D), s, sBase: sag(), twang: twang(), phi: S.phi * 180 / Math.PI, t: S.t, DT: P.DT, ell: P.ell }; }

  function paint() {
    const x = state();
    const sd = stringPath(P.c, S.D * mv(), x.s, S.phi);
    if (sd !== lastStr) { refs.str.setAttribute('d', sd); lastStr = sd; }
    const tl = 'translate(' + f3(-S.D * mv()) + ' 0)', tr = MIRROR + ' translate(' + f3(-S.D * mv()) + ' 0)';
    if (tl !== lastL) { refs.L.g.setAttribute('transform', tl); lastL = tl; }
    if (tr !== lastR) { refs.R.g.setAttribute('transform', tr); lastR = tr; }
    emit('frame', x);
  }
  function staticBuild() { const d = markPaths(Object.assign({}, P, { delta: 0, s: 0 })); paintCans(refs, d);
    const sw = f3(P.strokeW); refs.str.setAttribute('stroke-width', sw); lastStr = ''; lastL = ''; lastR = ''; }

  function stepOnce(h) {
    const wc = omega();
    // delta spring
    let z, w, tgt;
    if (S.target === 'taut') { z = 1; w = wc; tgt = P.DT * P.hitOver; }
    else { z = P.endZeta; w = endOmega(); tgt = 0; }
    const a = -2 * z * w * S.V - w * w * (S.D - tgt);
    S.V += a * h; S.D += S.V * h;
    if (S.target === 'taut' && S.D >= P.DT) { S.D = P.DT; S.V = 0; if (!S.hit) { S.hit = true; S.tw = 0; S.hitT = S.t; emit('hit', { t: clock.now(), tSim: S.t }); } }
    if (S.D < 0) { S.D = 0; if (S.V < 0) S.V = 0; }
    // phi
    if (S.ring) {
      S.ringPhase += 2 * Math.PI / P.T * h;
      const tgtPhi = rad(P.phi0) * Math.sin(S.ringPhase), k = 1 - Math.exp(-h / 0.05), old = S.phi;
      S.phi += (tgtPhi - S.phi) * k; S.phv = (S.phi - old) / h;
    } else {
      const zz = S.target === 'taut' ? 1 : 0.5, ww = S.target === 'taut' ? wc : 2 * Math.PI * 3.5;
      S.phv += (-2 * zz * ww * S.phv - ww * ww * S.phi) * h; S.phi += S.phv * h;
    }
    if (S.tw >= 0) { S.tw += h; if (P.A * Math.exp(-S.tw / P.tau) < 0.03) S.tw = -1; }
    S.t += h;
  }
  function active() {
    if (S.ring) return true;
    if (S.tw >= 0) return true;
    if (S.target === "taut" ? S.D < P.DT : S.D >= 0.01) return true;
    if (Math.abs(S.phi) > rad(0.2) || Math.abs(S.phv) > rad(2)) return true;
    return false;
  }
  function finish() {
    if (S.target === 'slack') { S.D = 0; S.V = 0; }
    S.phi = 0; S.phv = 0; S.tw = -1; paint();
    emit('settled', { t: clock.now(), tSim: S.t, state: S.target });
  }
  function tick() {
    S.rafId = 0;
    const now = clock.now();
    let dt = Math.min((now - S.last) / 1000, 0.05) * timeScale; S.last = now;
    if (reduced()) { snap(S.target === 'taut'); return; }
    const n = Math.max(1, Math.ceil(dt / (1 / 480))), h = dt / n;
    for (let i = 0; i < n; i++) stepOnce(h);
    paint();
    if (active()) schedule(); else { S.running = false; finish(); }
  }
  function schedule() { if (!S.rafId) { S.running = true; S.rafId = clock.raf(tick); } }
  function wake() { if (!S.rafId) { S.last = clock.now(); schedule(); } }
  function stop() { if (S.rafId) { clock.caf(S.rafId); S.rafId = 0; } S.running = false; }
  function snap(taut) { // reduced-motion jump
    stop(); S.ring = false; S.target = taut ? 'taut' : 'slack'; S.D = taut ? P.DT : 0; S.V = 0; S.phi = 0; S.phv = 0; S.tw = -1;
    paint(); const fresh = taut && !S.hit; if (taut) S.hit = true; else S.hit = false;
    if (fresh) emit('hit', { t: clock.now(), tSim: S.t, reduced: true });
    emit('settled', { t: clock.now(), tSim: S.t, state: S.target, reduced: true });
  }

  const api = {
    ring(on) {
      on = on !== false;
      if (reduced()) { S.ring = false; if (S.target === 'taut' || S.D > 0) snap(false); return api; }
      S.ring = on; if (on) { S.target = 'slack'; S.hit = false; } wake(); return api;
    },
    connect() {
      if (reduced()) { snap(true); return api; }
      S.ring = false; S.target = 'taut'; S.hit = S.D >= P.DT && S.hit; wake(); return api;
    },
    end() {
      if (reduced()) { snap(false); return api; }
      S.ring = false; S.target = 'slack'; S.hit = false; S.tw = -1;
      if (Math.abs(S.phi) < rad(1)) S.phv += rad(242) * 1; // kick: ~6 deg swing as the slack returns
      wake(); return api;
    },
    pluck() {
      if (reduced()) return api;
      if (S.target === 'taut' && S.D >= P.DT - 1e-9) { S.tw = 0; } else { S.phv += rad(90); }
      wake(); return api;
    },
    set(st) {
      stop(); S.ring = false;
      if (st === 'slack') st = { delta: 0, phi: 0 }; else if (st === 'taut') st = { delta: P.DT, phi: 0 };
      S.D = Math.min(Math.max(st.delta || 0, 0), P.DT || 0); S.V = 0; S.phi = rad(st.phi || 0); S.phv = 0;
      S.tw = -1; S.target = S.D > 0 ? 'taut' : 'slack'; S.hit = S.D >= P.DT && P.DT > 0; paint(); return api;
    },
    on(name, fn) { ev[name].push(fn); return () => { ev[name] = ev[name].filter(g => g !== fn); }; },
    state,
    setParams(p) {
      const q = Object.assign({}, p), w0 = 24 - 2 * (q.c != null ? q.c : P.c);
      const raw = Object.assign({}, P, q);
      if (q.s0 != null && q.ell == null) raw.ell = ellFromS0(w0, q.s0, raw.lengthModel);
      else if (q.ell == null && (q.c != null || q.lengthModel != null)) { /* keep ell */ }
      P = resolve(raw); if (S.D > P.DT) S.D = P.DT; staticBuild(); paint(); return api;
    },
    get params() { return P; },
    get timeScale() { return timeScale; }, set timeScale(v) { timeScale = v; },
    setReduced(v) { forceReduced = v == null ? null : !!v; return api; },
    get running() { return S.running; },
    destroy() { stop(); },
  };
  staticBuild(); paint();
  return api;
}

export const TinMark = { markPaths, attach, render, tier, sOfW, lenExact, lenApprox, ellFromS0, resolve, stringPath, DEFAULTS: DEF };

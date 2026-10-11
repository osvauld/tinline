/* Tinline refined studies — the one live call, shared by home.html, chat.html and groups.html.
   SPEC §In-conversation calls, "The call outlives the page".

   State lives in sessionStorage["tinline.call"] = {kind:'direct'|'group', id, name, ringAt, connectAt, muted}.
   Ringing → connected is derived from the clock (connectAt = ringAt + 2500), so a page loaded mid-call
   shows the right status and the MM:SS timer continues without resetting or jumping.
   Other same-origin documents (iframes, the embedding page) hear changes through the `storage` event
   and a postMessage nudge, so ending the call anywhere clears the strip everywhere at once.
   Silent simulation: this file never creates audio.

   API: TinCall.get() · start({kind,id,name}) · update({muted}) · end() · subscribe(fn) · onTick(fn)
        mmss(secs) · bar({id}) → the shared 40px "return to call" strip (button, hidden when idle). */
(function () {
  'use strict';
  const KEY = 'tinline.call', MSG = 'tinline-call', CONNECT_MS = 2500;
  let mem = null, lastJson, timer = 0, lastKey = '';
  const subs = new Set(), ticks = new Set(), bars = new Set();

  function raw() {
    try { const v = JSON.parse(sessionStorage.getItem(KEY) || 'null'); return v && v.id && v.kind ? v : null; }
    catch { return mem; }
  }
  function persist(v) {
    mem = v;
    try { v ? sessionStorage.setItem(KEY, JSON.stringify(v)) : sessionStorage.removeItem(KEY); } catch { /* memory only */ }
  }
  const pad = n => String(n).padStart(2, '0');
  const mmss = s => pad(Math.floor(s / 60)) + ':' + pad(s % 60);
  const preLabel = kind => kind === 'group' ? 'Connecting…' : 'Ringing…';

  // Snapshot with derived fields; null when no call is live.
  function get() {
    const v = raw(); if (!v) return null;
    const now = Date.now(), connected = now >= v.connectAt, secs = connected ? Math.floor((now - v.connectAt) / 1000) : 0;
    return { ...v, connected, secs, time: connected ? mmss(secs) : '', label: connected ? mmss(secs) : preLabel(v.kind), preLabel: preLabel(v.kind) };
  }

  // Tell every other same-origin document in this tab. The storage event usually does this already;
  // the message makes it immediate and dependable across nested frames.
  function windows(w, out) { out.push(w); try { for (let i = 0; i < w.frames.length; i++) windows(w.frames[i], out); } catch { } return out; }
  function broadcast() {
    let all; try { all = windows(window.top, []); } catch { all = [window]; }
    for (const w of all) if (w !== window) try { w.postMessage({ type: MSG }, '*'); } catch { }
  }

  function sync() {
    const v = raw(), json = JSON.stringify(v);
    if (json === lastJson) return;
    lastJson = json; lastKey = '';
    const snap = get();
    subs.forEach(fn => { try { fn(snap); } catch (e) { console.error(e); } });
    paintBars(); schedule();
  }

  // One timer per document, aligned to the call's own second boundary so every page flips together.
  function schedule() {
    clearTimeout(timer); const v = raw(); if (!v) return;
    const now = Date.now();
    const wait = now < v.connectAt ? v.connectAt - now : 1000 - ((now - v.connectAt) % 1000);
    timer = setTimeout(tick, Math.max(16, wait + 8));
  }
  function tick() {
    const c = get(); if (!c) return;
    const key = c.connected + ':' + c.secs;
    if (key !== lastKey) { lastKey = key; ticks.forEach(fn => { try { fn(c); } catch (e) { console.error(e); } }); paintBars(); }
    schedule();
  }

  /* ---------- shared strip: identical markup + CSS on every page ---------- */
  const CSS = `
.t-callbar{box-sizing:border-box;display:flex;align-items:center;justify-content:center;gap:8px;width:100%;height:40px;min-height:40px;margin:0;padding:0 16px;border:0;border-radius:0;
  background:var(--accent);color:var(--on-accent);font:600 13px/18px system-ui,-apple-system,"Segoe UI",Roboto,"Noto Sans",sans-serif;font-size:var(--fs-sub);
  letter-spacing:normal;text-align:center;text-transform:none;font-variant-numeric:tabular-nums;white-space:nowrap;overflow:hidden;cursor:pointer;flex:none;
  -webkit-tap-highlight-color:transparent;-webkit-font-smoothing:antialiased}
.t-callbar[hidden]{display:none!important}
.t-callbar:hover{filter:brightness(1.08)}
.t-callbar:focus{outline:0}
.t-callbar:focus-visible{outline:2px solid var(--on-accent);outline-offset:-4px}
.t-callbar .t-cb-dot{width:8px;height:8px;border-radius:50%;background:currentColor;flex:none;animation:t-cb-pulse 1.6s ease-in-out infinite}
.t-callbar .t-cb-nm{min-width:0;overflow:hidden;text-overflow:ellipsis}
.t-callbar .t-cb-sep{flex:none}
.t-callbar .t-cb-tm{flex:none;display:inline-grid;justify-items:start}
.t-callbar .t-cb-tm>span,.t-callbar .t-cb-tm::after{grid-area:1/1}
.t-callbar .t-cb-tm::after{content:attr(data-size);visibility:hidden}
@keyframes t-cb-pulse{50%{opacity:.35}}
@media(prefers-reduced-motion:reduce){.t-callbar .t-cb-dot{animation:none}}
:root[data-motion=reduce] .t-callbar .t-cb-dot{animation:none}
@media(forced-colors:active){.t-callbar{border-bottom:1px solid CanvasText}}`;
  function injectCss() {
    if (document.getElementById('t-callbar-css')) return;
    const s = document.createElement('style'); s.id = 't-callbar-css'; s.textContent = CSS;
    (document.head || document.documentElement).append(s);
  }
  function el(tag, cls) { const e = document.createElement(tag); if (cls) e.className = cls; return e; }
  // The button reads "● <name> · MM:SS". Its accessible name is fixed ("Return to call with <name>"),
  // so the ticking timer is never announced. The time slot is as wide as the pre-connect label, so
  // "Ringing…" → "00:00" never moves the dot or the name.
  function bar(opts = {}) {
    injectCss();
    const b = el('button', 't-callbar'); b.type = 'button'; if (opts.id) b.id = opts.id; b.hidden = true;
    const dot = el('span', 't-cb-dot'), nm = el('span', 't-cb-nm'), sep = el('span', 't-cb-sep'), tm = el('span', 't-cb-tm'), tv = el('span');
    dot.setAttribute('aria-hidden', 'true'); sep.setAttribute('aria-hidden', 'true'); tm.append(tv);
    sep.textContent = ' · '; // edge spaces collapse in layout (flex item) but keep textContent readable
    b.append(dot, nm, sep, tm); bars.add(b); paintBar(b);
    return b;
  }
  function paintBar(b) {
    const c = get(); b.dataset.live = c ? '1' : '';
    if (!c) { b.removeAttribute('aria-label'); return; }
    const nm = b.querySelector('.t-cb-nm'), tm = b.querySelector('.t-cb-tm');
    if (nm.textContent !== c.name) nm.textContent = c.name;
    tm.dataset.size = c.preLabel; tm.firstChild.textContent = c.label;
    b.setAttribute('aria-label', 'Return to call with ' + c.name);
  }
  function paintBars() { bars.forEach(paintBar); }

  /* ---------- mutations ---------- */
  function start({ kind, id, name }) {
    const live = get(); if (live) return live; // one call at a time
    const ringAt = Date.now();
    persist({ kind: kind === 'group' ? 'group' : 'direct', id: String(id), name: String(name), ringAt, connectAt: ringAt + CONNECT_MS, muted: false });
    sync(); broadcast(); return get();
  }
  function update(patch) {
    const v = raw(); if (!v) return null;
    if (patch && 'muted' in patch) v.muted = !!patch.muted;
    persist(v); sync(); broadcast(); return get();
  }
  function end() { if (!raw()) return; persist(null); sync(); broadcast(); }
  function subscribe(fn) { subs.add(fn); return () => subs.delete(fn); }
  function onTick(fn) { ticks.add(fn); return () => ticks.delete(fn); }

  addEventListener('storage', e => { if (e.key === KEY || e.key === null) sync(); });
  addEventListener('message', e => { if (e.data && e.data.type === MSG) sync(); });
  addEventListener('pageshow', () => { sync(); tick(); }); // back/forward cache restores a stale page
  document.addEventListener('visibilitychange', () => { if (!document.hidden) { sync(); tick(); } });

  lastJson = JSON.stringify(raw()); schedule();
  window.TinCall = { get, start, update, end, subscribe, onTick, mmss, bar, KEY };
})();

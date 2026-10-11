/* Tinline refined studies — the one live call, shared by home.html, chat.html and groups.html.
   SPEC §In-conversation calls, "The call outlives the page".

   State lives in sessionStorage["tinline.call"] = {kind:'direct'|'group', id, name, ringAt, connectAt, muted,
   and for a group call: k, roster:[{id, name, ini, me?, host?, fwd?, muted?, join?, leave?}]}.
   roster is everyone the call knows about: fwd = in the host's forwarded (speaking) set of k, host = the
   member whose device hosts the call, join/leave = ms after connectAt when someone arrives/goes (absent =
   already there). Who is present, and who is the current speaker, is derived from the clock like the timer,
   so every page shows the same roster at the same moment without writing anything back.
   Ringing → connected is derived from the clock (connectAt = ringAt + 2500), so a page loaded mid-call
   shows the right status and the MM:SS timer continues without resetting or jumping.
   Other same-origin documents (iframes, the embedding page) hear changes through the `storage` event
   and a postMessage nudge, so ending the call anywhere clears the strip everywhere at once.
   This file never creates audio; call-audio.js follows this state and plays the cues.

   API: TinCall.get() · start({kind,id,name,roster,k}) · update({muted}) · end() · subscribe(fn) · onTick(fn)
        mmss(secs) · bar({id}) → the shared 40px "return to call" strip (button, hidden when idle)
        stage() → the shared group-call roster (header strip, speaking tiles, quiet row, full-roster sheet);
                  chat.html and groups.html both put this one element in their overlay for a group call. */
(function () {
  'use strict';
  const KEY = 'tinline.call', MSG = 'tinline-call', CONNECT_MS = 2500, SPEAK_MS = 4000;
  let mem = null, lastJson, timer = 0, lastKey = '';
  const subs = new Set(), ticks = new Set(), bars = new Set(), stages = new Set();

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

  // Who is in a group call right now. You arrive on connect; the speaker moves between the unmuted
  // forwarded people every SPEAK_MS — a level change, not a meter.
  function people(v, now) {
    if (v.kind !== 'group' || !Array.isArray(v.roster)) return null;
    const t = now - v.connectAt;
    const out = v.roster.filter(p => (p.me ? t >= 0 : !(p.join > 0) || t >= p.join) && !(p.leave > 0 && t >= p.leave))
      .map(p => ({ id: p.id, name: p.name, ini: p.ini || initials(p.name), me: !!p.me, host: !!p.host, fwd: !!p.fwd, muted: p.me ? !!v.muted : !!p.muted, speaking: false }));
    const cand = out.filter(p => p.fwd && !p.muted && !p.me);
    if (cand.length) cand[Math.floor(Math.max(0, t) / SPEAK_MS) % cand.length].speaking = true;
    return out;
  }
  // Snapshot with derived fields; null when no call is live.
  function get() {
    const v = raw(); if (!v) return null;
    const now = Date.now(), connected = now >= v.connectAt, secs = connected ? Math.floor((now - v.connectAt) / 1000) : 0;
    return { ...v, connected, secs, time: connected ? mmss(secs) : '', label: connected ? mmss(secs) : preLabel(v.kind), preLabel: preLabel(v.kind), people: people(v, now) };
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
    paintBars(); paintStages(); schedule();
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
    if (key !== lastKey) { lastKey = key; ticks.forEach(fn => { try { fn(c); } catch (e) { console.error(e); } }); paintBars(); paintStages(); }
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

  /* ---------- shared group-call roster: identical markup + CSS in chat.html and groups.html ----------
     SPEC §In-conversation calls, "Group call". Header strip (44px glyph, name, "<n> in the call", timer),
     up to k speaking tiles (72px, first name, "Hosting" tag, mic-off badge, accent ring on the speaker),
     then a quiet row of 40px avatars (6 shown, then "+<n>" opening the whole roster in a sheet).
     Lists are rebuilt only when who is here / muted / speaking changes; the timer is the only per-second write. */
  const GCSS = `
.t-gc{box-sizing:border-box;flex:1;min-height:0;width:100%;display:flex;flex-direction:column;align-items:center;gap:8px;color:var(--ink)}
.t-gc[hidden],.t-gc [hidden]{display:none!important}
.t-gc .t-vh{position:absolute;width:1px;height:1px;overflow:hidden;clip:rect(0 0 0 0);white-space:nowrap;border:0;padding:0;margin:-1px}
.t-gc-head{box-sizing:border-box;width:100%;max-width:560px;display:flex;align-items:center;gap:12px;padding:0 8px;text-align:left}
.t-gc-glyph{display:grid;place-items:center;width:44px;height:44px;border-radius:50%;background:var(--surface-2);color:var(--accent-ink);flex:none}
.t-gc svg.t-gc-i{width:22px;height:22px;fill:none;stroke:currentColor;stroke-width:1.8;stroke-linecap:round;stroke-linejoin:round}
.t-gc-ttl{flex:1;min-width:0;display:flex;flex-direction:column}
.t-gc .t-gc-nm{margin:0;font-size:var(--fs-title);font-weight:600;line-height:22px;white-space:nowrap;overflow:hidden;text-overflow:ellipsis}
.t-gc-sub{font-size:var(--fs-sub);line-height:18px;color:var(--muted)}
.t-gc-tm{flex:none;display:inline-grid;justify-items:end;font-size:var(--fs-body);line-height:22px;font-variant-numeric:tabular-nums}
.t-gc-tm>span,.t-gc-tm::after{grid-area:1/1}
.t-gc-tm::after{content:"00:00";visibility:hidden}
.t-gc-room{box-sizing:border-box;flex:1;min-height:0;width:100%;display:flex;flex-direction:column;align-items:center;justify-content:center;gap:32px;overflow:auto;padding:8px 0}
.t-gc-spk,.t-gc-quiet,.t-gc-list{list-style:none;margin:0;padding:0}
.t-gc-spk{display:flex;flex-wrap:wrap;justify-content:center;gap:20px 16px;max-width:100%}
.t-gc-tile{box-sizing:border-box;width:120px;display:flex;flex-direction:column;align-items:center;gap:6px;text-align:center}
.t-gc-av{position:relative;display:inline-grid;flex:none}
.t-gc-tile .t-avatar{width:72px;height:72px;font-size:24px}
.t-gc-tile.on .t-avatar{outline:2px solid var(--accent);outline-offset:3px}
.t-gc-fn{max-width:100%;font-size:var(--fs-sub);font-weight:600;line-height:18px;white-space:nowrap;overflow:hidden;text-overflow:ellipsis}
.t-gc-tile .t-badge{flex:none}
.t-gc-mic{position:absolute;right:-2px;bottom:-2px;width:24px;height:24px;border-radius:50%;display:grid;place-items:center;background:var(--surface);color:var(--ink);box-shadow:0 0 0 2px var(--bg)}
.t-gc svg.t-gc-mi,.t-gc-sheet svg.t-gc-mi{width:14px;height:14px;fill:none;stroke:currentColor;stroke-width:2;stroke-linecap:round;stroke-linejoin:round}
.t-gc-quiet{display:flex;flex-wrap:wrap;justify-content:center;align-items:center;gap:8px;max-width:100%}
.t-gc-quiet .t-avatar{width:40px;height:40px;font-size:14px}
.t-gc-quiet .t-gc-mic{width:18px;height:18px;right:-3px;bottom:-3px}
.t-gc-quiet svg.t-gc-mi{width:10px;height:10px;stroke-width:2.4}
.t-gc-more{display:grid;place-items:center;width:44px;height:44px;margin:-2px;padding:0;border:0;border-radius:50%;background:transparent;color:var(--ink);cursor:pointer;font:inherit;-webkit-tap-highlight-color:transparent}
.t-gc-more>span{display:grid;place-items:center;width:40px;height:40px;border-radius:50%;background:var(--surface-2);font-size:var(--fs-sub);font-weight:700;font-variant-numeric:tabular-nums}
.t-gc-more:hover>span{background:var(--line)}
.t-gc-more:focus{outline:0}
.t-gc-more:focus-visible>span{outline:2px solid var(--accent);outline-offset:2px}
dialog.t-gc-sheet{box-sizing:border-box;width:min(440px,calc(100vw - 32px));max-height:min(640px,calc(100dvh - 48px));padding:20px 20px 16px;border:0;border-radius:var(--r-sheet);background:var(--surface);color:var(--ink);box-shadow:var(--shadow-sheet)}
dialog.t-gc-sheet[open]{display:flex;flex-direction:column}
.t-gc-sheet::backdrop{background:rgb(10 16 14 / .5)}
.t-gc-sheet h2{margin:0 0 8px;font-size:var(--fs-title);font-weight:600;line-height:22px}
.t-gc-sheet .t-gc-list{overflow:auto;min-height:0;margin:0 -4px;padding:0 4px}
.t-gc-list li{display:flex;align-items:center;gap:12px;min-height:52px}
.t-gc-list .t-avatar{width:36px;height:36px;font-size:13px}
.t-gc-list .t-gc-who{flex:1;min-width:0;display:flex;flex-direction:column}
.t-gc-list .t-gc-who b{font-size:var(--fs-body);font-weight:600;line-height:20px;white-space:nowrap;overflow:hidden;text-overflow:ellipsis}
.t-gc-list .t-gc-who span{font-size:var(--fs-sub);line-height:18px;color:var(--muted)}
.t-gc-list .t-gc-mi{color:var(--muted);flex:none}
.t-gc-acts{display:flex;justify-content:flex-end;padding-top:8px}
.t-gc-done{min-height:44px;padding:0 18px;border:0;border-radius:999px;background:transparent;color:var(--accent-ink);font:inherit;font-weight:600;cursor:pointer}
.t-gc-done:hover{background:var(--surface-2)}
@media(max-width:699px){dialog.t-gc-sheet{width:100%;max-width:none;margin:auto 0 0;border-radius:var(--r-sheet) var(--r-sheet) 0 0;max-height:85dvh;padding-bottom:calc(16px + env(safe-area-inset-bottom,0px))}}`;
  const SVGNS = 'http://www.w3.org/2000/svg';
  const GLYPH = '<circle cx="9" cy="8" r="3.5"/><path d="M2.5 20c.5-3.5 3.2-5.5 6.5-5.5s6 2 6.5 5.5"/><circle cx="17" cy="9" r="2.5"/><path d="M16.5 14.6c2.6.2 4.5 2 5 4.9"/>';
  const MICOFF = '<path d="M12 3a3 3 0 0 1 3 3v6a3 3 0 0 1-6 0V6a3 3 0 0 1 3-3z"/><path d="M5 11a7 7 0 0 0 14 0M12 18v3M4 4l16 16"/>';
  const QUIET_MAX = 6, QUIET_ROW = 6 * 40 + 44 + 6 * 8; // px the full quiet row needs
  const hue = n => 'av' + ([...n].reduce((a, c) => a + c.codePointAt(0), 0) % 6); // shared hash, SPEC §Avatars
  function initials(n) { return String(n).split(' ').map(w => w[0]).slice(0, 2).join(''); }
  const firstName = p => p.me ? 'You' : p.name.split(' ')[0];
  function svg(paths, cls) { const s = document.createElementNS(SVGNS, 'svg'); s.setAttribute('viewBox', '0 0 24 24'); s.setAttribute('class', cls); s.setAttribute('aria-hidden', 'true'); s.innerHTML = paths; return s; }
  function injectGcss() {
    if (document.getElementById('t-gc-css')) return;
    const s = document.createElement('style'); s.id = 't-gc-css'; s.textContent = GCSS;
    (document.head || document.documentElement).append(s);
  }
  function txt(tag, cls, t) { const e = el(tag, cls); e.textContent = t; return e; }
  function face(p, cls) { const a = txt('span', 't-avatar ' + hue(p.name) + (cls ? ' ' + cls : ''), p.ini); a.setAttribute('aria-hidden', 'true'); return a; }
  function micBadge(named) {
    const m = el('span', 't-gc-mic'); m.append(svg(MICOFF, 't-gc-mi'));
    if (named) { m.setAttribute('role', 'img'); m.setAttribute('aria-label', 'Muted'); } else m.setAttribute('aria-hidden', 'true');
    return m;
  }
  function stage() {
    injectCss(); injectGcss();
    const root = el('div', 't-gc'); root.hidden = true;
    const head = el('div', 't-gc-head'), glyph = el('span', 't-gc-glyph'), ttl = el('div', 't-gc-ttl');
    const nm = el('h2', 't-gc-nm'), sub = el('span', 't-gc-sub'), tm = el('span', 't-gc-tm'), tv = el('span');
    glyph.setAttribute('aria-hidden', 'true'); glyph.append(svg(GLYPH, 't-gc-i')); tm.append(tv); tm.setAttribute('aria-hidden', 'true');
    ttl.append(nm, sub); head.append(glyph, ttl, tm);
    const room = el('div', 't-gc-room'), spk = el('ul', 't-gc-spk'), quiet = el('ul', 't-gc-quiet'), live = el('div', 't-vh');
    spk.setAttribute('aria-label', 'Speaking'); quiet.setAttribute('aria-label', 'Also in the call');
    live.setAttribute('role', 'status'); live.setAttribute('aria-live', 'polite');
    room.append(spk, quiet); root.append(head, room, live);
    const st = { root, nm, sub, tv, spk, quiet, live, key: '', ids: null, ringAt: 0, sheet: null, max: QUIET_MAX };
    // "Up to 6" quiet avatars: on a very narrow phone (320px) 5 + "+<n>" keeps the row on one line.
    if (window.ResizeObserver) new ResizeObserver(() => { const w = root.clientWidth, m = w && w < QUIET_ROW ? QUIET_MAX - 1 : QUIET_MAX; if (m !== st.max) { st.max = m; st.key = ''; paintStage(st); } }).observe(root);
    root._gc = st; stages.add(st); paintStage(st);
    return root;
  }
  function tileFor(p) {
    const li = el('li', 't-gc-tile' + (p.speaking ? ' on' : '')); li.dataset.id = p.id;
    const av = el('span', 't-gc-av'); av.append(face(p)); if (p.muted) av.append(micBadge(true));
    li.append(av, txt('span', 't-gc-fn', firstName(p)));
    if (p.host) li.append(txt('span', 't-badge muted t-gc-host', 'Hosting'));
    if (p.speaking) li.append(txt('span', 't-vh', 'Speaking'));
    return li;
  }
  function quietFor(p) {
    const li = el('li'); li.dataset.id = p.id;
    const av = el('span', 't-gc-av'); av.setAttribute('role', 'img');
    av.setAttribute('aria-label', (p.me ? 'You' : p.name) + (p.host ? ', hosting' : '') + (p.muted ? ', muted' : ''));
    av.append(face(p)); if (p.muted) av.append(micBadge(false));
    li.append(av); return li;
  }
  function sheetRows(st, c) {
    const ul = st.sheet.querySelector('.t-gc-list'); ul.replaceChildren();
    for (const p of c.people) {
      const tags = [p.host ? 'Hosting' : '', p.speaking ? 'Speaking' : '', p.muted ? 'Muted' : ''].filter(Boolean);
      const li = el('li'), who = el('span', 't-gc-who'); li.dataset.id = p.id;
      who.append(txt('b', '', p.me ? 'You' : p.name)); if (tags.length) who.append(txt('span', '', tags.join(' · ')));
      li.append(face(p), who); if (p.muted) li.append(svg(MICOFF, 't-gc-mi'));
      ul.append(li);
    }
    st.sheet.querySelector('h2').textContent = c.people.length + ' in the call';
  }
  function openSheet(st, opener) {
    const c = get(); if (!c || !c.people) return;
    if (!st.sheet) {
      const d = el('dialog', 't-gc-sheet'), h2 = el('h2'), ul = el('ul', 't-gc-list'), acts = el('div', 't-gc-acts'), done = txt('button', 't-gc-done', 'Done');
      h2.id = 't-gc-sheet-h'; d.setAttribute('aria-labelledby', h2.id); done.type = 'button';
      done.onclick = () => d.close(); acts.append(done); d.append(h2, ul, acts);
      d.addEventListener('click', e => { if (e.target === d) { const r = d.getBoundingClientRect(); if (e.clientX < r.left || e.clientX > r.right || e.clientY < r.top || e.clientY > r.bottom) d.close(); } }); // scrim
      d.addEventListener('keydown', e => { if (e.key === 'Escape') e.stopPropagation(); }); // close the sheet, never minimize the call
      d.addEventListener('close', () => { const m = st.quiet.querySelector('.t-gc-more'); if (m && document.contains(m)) m.focus({ preventScroll: true }); });
      document.body.append(d); st.sheet = d;
    }
    sheetRows(st, c); st.sheet.showModal(); st.sheet.querySelector('.t-gc-done').focus();
  }
  function paintStage(st) {
    const c = get();
    if (!c || c.kind !== 'group' || !c.people) {
      st.key = ''; st.ids = null; st.spk.replaceChildren(); st.quiet.replaceChildren();
      if (st.sheet && st.sheet.open) st.sheet.close();
      return;
    }
    if (st.nm.textContent !== c.name) st.nm.textContent = c.name;
    const sub = c.connected ? c.people.length + ' in the call' : c.preLabel;
    if (st.sub.textContent !== sub) st.sub.textContent = sub;
    st.tv.textContent = c.time;
    const key = c.ringAt + '|' + (c.k || 3) + '|' + c.people.map(p => p.id + (p.muted ? 'm' : '') + (p.speaking ? 's' : '')).join(',');
    if (key === st.key) return;
    st.key = key;
    // Who arrived or left since the last paint of this same call (announced once, politely).
    const ids = c.people.filter(p => !p.me).map(p => p.id);
    if (st.ids && st.ringAt === c.ringAt) {
      const by = new Map(c.people.map(p => [p.id, p])), was = new Set(st.ids), now = new Set(ids), old = st.names || {};
      const came = ids.filter(i => !was.has(i)).map(i => firstName(by.get(i)) + ' joined the call');
      const went = st.ids.filter(i => !now.has(i)).map(i => (old[i] || 'Someone') + ' left the call');
      if (came.length || went.length) st.live.textContent = came.concat(went).join('. ');
    }
    st.ids = ids; st.ringAt = c.ringAt; st.names = Object.fromEntries(c.people.map(p => [p.id, firstName(p)]));
    const k = Math.max(1, c.k || 3), fwd = c.people.filter(p => p.fwd).slice(0, k), rest = c.people.filter(p => !fwd.includes(p));
    rest.sort((a, b) => (b.me ? 1 : 0) - (a.me ? 1 : 0)); // you first in the quiet row
    const hadMore = st.quiet.contains(document.activeElement);
    st.spk.replaceChildren(...fwd.map(tileFor)); st.spk.hidden = !fwd.length;
    const shown = rest.length > st.max ? rest.slice(0, st.max) : rest, extra = rest.length - shown.length;
    st.quiet.replaceChildren(...shown.map(quietFor)); st.quiet.hidden = !rest.length;
    // "+<n>" always opens the whole roster; with nothing hidden it is not needed.
    if (extra > 0) {
      const li = el('li'), b = el('button', 't-gc-more'), n = el('span');
      b.type = 'button'; b.setAttribute('aria-haspopup', 'dialog');
      b.setAttribute('aria-label', 'Show everyone in the call, ' + extra + ' more');
      n.textContent = '+' + extra; n.setAttribute('aria-hidden', 'true'); b.append(n);
      b.onclick = () => openSheet(st, b); li.append(b); st.quiet.append(li);
      if (hadMore) b.focus({ preventScroll: true });
    }
    if (st.sheet && st.sheet.open) sheetRows(st, c);
  }
  function paintStages() { stages.forEach(paintStage); }

  /* ---------- mutations ---------- */
  function start({ kind, id, name, roster, k }) {
    const live = get(); if (live) return live; // one call at a time
    const ringAt = Date.now(), group = kind === 'group';
    const v = { kind: group ? 'group' : 'direct', id: String(id), name: String(name), ringAt, connectAt: ringAt + CONNECT_MS, muted: false };
    if (group && Array.isArray(roster)) {
      v.k = Math.max(1, Math.min(6, k | 0 || 3));
      v.roster = roster.map(p => {
        const r = { id: String(p.id), name: String(p.name), ini: String(p.ini || initials(p.name)) };
        for (const f of ['me', 'host', 'fwd', 'muted']) if (p[f]) r[f] = true;
        for (const f of ['join', 'leave']) if (p[f] > 0) r[f] = +p[f];
        return r;
      });
    }
    persist(v);
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
  window.TinCall = { get, start, update, end, subscribe, onTick, mmss, bar, stage, KEY };
})();

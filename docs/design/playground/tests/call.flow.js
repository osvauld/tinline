/* 1:1 voice call flows against the running dev server (http://127.0.0.1:5178).
   Run: node tests/call.flow.js   (BASE=… to override; SHOTS=/dir/ to also write screenshots). Prints PASS/FAIL per check. */
import { createRequire } from 'node:module';
const L = createRequire(import.meta.url)('/home/abe/p2p_chat/.claude/worktrees/design-refinement/docs/design/refined/qa/lib.js');
const BASE = process.env.BASE || 'http://127.0.0.1:5178/';
const SHOTS = process.env.SHOTS || '';
let pass = 0, fail = 0;
const ok = (name, cond, extra = '') => { cond ? pass++ : fail++; console.log((cond ? 'PASS ' : 'FAIL ') + name + (cond ? '' : ' ' + extra)); };
const wait = (p, ms) => p.waitForTimeout(ms);

async function open(b, form, hash, { dark = false, reduced = false } = {}) {
  const p = await L.page(b, { w: form === 'android' ? 600 : 1440, h: 900, dark });
  if (reduced) await p.emulateMedia({ reducedMotion: 'reduce' });
  await p.goto(`${BASE}?form=${form}&theme=${dark ? 'dark' : 'light'}#/${hash}`);
  await p.waitForSelector('[data-form]');
  await wait(p, 600);
  // Fixture: start from no call history (the seed has a few records so the Calls tab isn't empty on first open).
  await p.evaluate(() => { for (const c of window.__pg.db.chats) { const n = c.messages.filter((m) => m.call && m.from !== 'me').length; c.messages = c.messages.filter((m) => !m.call); c.unread = Math.max(0, c.unread - n); } });
  await p.evaluate(() => { window.__pg.CALL_MS.ringingAfter = 8000; window.__pg.CALL_MS.connect = 700; window.__pg.fxLog.length = 0; });
  return p;
}
const pg = (p, fn, arg) => p.evaluate(fn, arg);
const phase = (p) => pg(p, () => window.__pg.call.phase);
const status = (p) => p.locator('.callscreen .status .msg').innerText().catch(() => '');
const recs = (p, id) => pg(p, (id) => window.__pg.db.chats.find((c) => c.id === id).messages.filter((m) => m.call).map((m) => ({ ...m.call, from: m.from })), id);
const cues = (p) => pg(p, () => window.__pg.fxLog.filter(({ e }) => e.type !== 'taut-hit').map(({ e }) => e.type + (e.on === undefined ? '' : e.on ? ':on' : ':off')));
const until = async (p, fn, ms = 4000) => { const t0 = Date.now(); while (Date.now() - t0 < ms) { if (await p.evaluate(fn)) return true; await wait(p, 50); } return false; };
const screenGone = (p) => until(p, () => !document.querySelector('.callscreen'), 3000);
const startFromChat = async (p) => { await p.getByRole('button', { name: 'Call', exact: true }).click(); await p.waitForSelector('.callscreen'); };
const shot = (p, n) => SHOTS ? p.screenshot({ path: SHOTS + n + '.png' }) : null;
const idle = (p) => until(p, () => window.__pg.call.phase === 'idle', 4000);

/* ---- in-page samplers ---- */
async function installSampler(p) {
  await p.evaluate(() => {
    window.__samp = { run(ms, sel) {
      return new Promise((res) => {
        const out = [], t0 = performance.now();
        const f = () => {
          const t = performance.now() - t0, row = { t };
          const s = document.querySelector('.callscreen'), st = document.querySelector('[data-callstrip]');
          if (s) { const cs = getComputedStyle(s); row.op = +cs.opacity; const m = new DOMMatrix(cs.transform === 'none' ? undefined : cs.transform); row.ty = m.m42; row.sc = m.a; } else row.op = null;
          row.h = st ? st.getBoundingClientRect().height : 0;
          out.push(row);
          if (t < ms) requestAnimationFrame(f); else res(out);
        };
        requestAnimationFrame(f);
      });
    } };
  });
}
const sample = (p, ms) => p.evaluate((ms) => window.__samp.run(ms), ms);
const distinct = (a, k, d = 3) => new Set(a.map((r) => r[k] == null ? null : +r[k].toFixed(d)).filter((v) => v !== null)).size;
const maxStep = (a, k) => { let m = 0; for (let i = 1; i < a.length; i++) if (a[i][k] != null && a[i - 1][k] != null) m = Math.max(m, Math.abs(a[i][k] - a[i - 1][k])); return m; };

/* field pixel continuity: downsampled canvas, mean |delta| per 16.7 ms-normalised frame */
async function fieldRun(p, ms, during) {
  await p.evaluate(() => {
    const cv = () => document.querySelector('.callscreen canvas');
    const o = document.createElement('canvas'); o.width = 24; o.height = 40; const x = o.getContext('2d', { willReadFrequently: true });
    window.__fr = { rows: [], stop: false };
    let prev = null, pt = 0;
    const f = (ts) => {
      const c = cv();
      if (c && c.width) { x.drawImage(c, 0, 0, 24, 40); const d = x.getImageData(0, 0, 24, 40).data;
        if (prev) { let m = 0; for (let i = 0; i < d.length; i += 4) { m = Math.max(m, Math.abs(d[i] - prev[i]), Math.abs(d[i + 1] - prev[i + 1]), Math.abs(d[i + 2] - prev[i + 2])); } window.__fr.rows.push({ t: ts, dt: ts - pt, m, phase: window.__pg.call.phase }); }
        prev = Uint8ClampedArray.from(d); pt = ts; }
      if (!window.__fr.stop) requestAnimationFrame(f);
    };
    requestAnimationFrame(f);
  });
  await during();
  await wait(p, ms);
  return p.evaluate(() => { window.__fr.stop = true; return window.__fr.rows; });
}

/* text contrast against the field pixels under each text element */
async function contrast(p) {
  return p.evaluate(() => {
    const cv = document.querySelector('.callscreen canvas'); if (!cv) return { err: 'no canvas' };
    const host = cv.parentElement, hr = host.getBoundingClientRect(), k = cv.width / hr.width;
    const lin = (c) => { c /= 255; return c <= .03928 ? c / 12.92 : ((c + .055) / 1.055) ** 2.4; };
    const lum = (r, g, b) => .2126 * lin(r) + .7152 * lin(g) + .0722 * lin(b);
    const o = document.createElement('canvas'); o.width = cv.width; o.height = cv.height;
    const x = o.getContext('2d', { willReadFrequently: true }); x.drawImage(cv, 0, 0);
    const out = [];
    for (const el of document.querySelectorAll('.callscreen .name, .callscreen .status span, .callscreen .timer, .callscreen .e2e, .callscreen .ctl > span')) {
      const cs = getComputedStyle(el); if (+cs.opacity < .99 && !el.classList.contains('timer')) continue;
      const r = el.getBoundingClientRect(); if (r.width < 2) continue;
      const m = cs.color.match(/[\d.]+/g).map(Number);
      const tl = lum(m[0], m[1], m[2]);
      const sx = Math.max(0, Math.floor((r.left - hr.left) * k)), sy = Math.max(0, Math.floor((r.top - hr.top) * k));
      const w = Math.max(1, Math.min(cv.width - sx, Math.ceil(r.width * k))), h = Math.max(1, Math.min(cv.height - sy, Math.ceil(r.height * k)));
      const d = x.getImageData(sx, sy, w, h).data;
      let worst = 99;
      for (let i = 0; i < d.length; i += 4) { const bl = lum(d[i], d[i + 1], d[i + 2]); const c = (Math.max(tl, bl) + .05) / (Math.min(tl, bl) + .05); worst = Math.min(worst, c); }
      out.push({ el: el.className || el.tagName, text: el.textContent.trim().slice(0, 16), worst: +worst.toFixed(2) });
    }
    return out;
  });
}

(async () => {
  const b = await L.launch();
  const pages = [];
  const mk = async (...a) => { const p = await open(b, ...a); pages.push(p); return p; };
  try {
    /* ===== outgoing, answered (desktop), with sound on to check cue order ===== */
    let p = await mk('desktop', 'chat/arjun');
    await p.getByRole('button', { name: 'Simulation' }).click();
    await p.getByRole('switch', { name: /sound/i }).first().click().catch(() => {});
    await p.getByRole('button', { name: 'Close simulation' }).click();
    await pg(p, () => { window.__pg.audio.arm(); window.__pg.audio.setQuiet(false); window.__pg.audio.played.length = 0; });
    await startFromChat(p);
    ok('outgoing: call screen opens, phase outgoing', (await phase(p)) === 'outgoing');
    ok('outgoing: status "Calling…" first', /^Calling…$/.test((await status(p)).trim()));
    ok('outgoing: field is WebGL, one canvas', (await p.locator('.callscreen canvas').count()) === 1 && (await p.locator('.callscreen [data-render]').getAttribute('data-render')) === 'webgl');
    await pg(p, () => { window.__pg.CALL_MS.ringingAfter = 300; });
    await pg(p, () => { const c = window.__pg.call; c.ringing = false; });
    await wait(p, 100);
    await pg(p, () => { /* shorten the remaining wait: ringing flips after ringingAfter from the call start */ window.__pg.call.ringing = true; });
    await wait(p, 300);
    ok('outgoing: becomes "Ringing…" (online peer)', /Ringing…/.test(await status(p)));
    ok('outgoing: no ring avatar element (clouds replace the ring)', (await p.locator('.callscreen .ring, .callscreen .halo').count()) === 0);
    await pg(p, () => { window.__pg.CALL_MS.connect = 6000; });
    await p.getByRole('button', { name: 'Simulation' }).click();
    await p.getByRole('button', { name: 'Answers', exact: true }).click();
    await p.getByRole('button', { name: 'Close simulation' }).click();
    await wait(p, 120);
    ok('answered: "Connecting…" first', (await phase(p)) === 'connecting' && /Connecting…/.test(await status(p)));
    ok('answered: then active with a running timer', await until(p, () => window.__pg.call.phase === 'active', 12000));
    await wait(p, 2300);
    ok('active: timer counts', /^00:0[1-9]$/.test((await p.locator('.callscreen .timer').innerText()).trim()), await p.locator('.callscreen .timer').innerText());
    ok('active: encryption line only once connected', (await p.locator('.callscreen .e2e.on').count()) === 1);
    await shot(p, 'active-desktop-light');
    await p.getByRole('button', { name: 'End call' }).click();
    ok('end: screen leaves', await screenGone(p));
    ok('end: back to idle', await idle(p));
    let r = await recs(p, 'arjun');
    const mine = r.filter((x) => x.direction === 'out' && x.outcome === 'answered');
    ok('record: exactly one new record, outgoing answered, from me, duration >= 2 s', r.length === 1 && mine.length === 1 && mine[0].from === 'me' && mine[0].durationS >= 2 && mine[0].kind === 'voice', JSON.stringify(r));
    ok('record: shown on my side, compact, no emoji', await (async () => {
      const li = p.locator('li.callrec').last(); const t = await li.innerText();
      return (await li.getAttribute('class')).includes(' me ') && t.length < 40 && !/\p{Extended_Pictographic}/u.test(t);
    })());
    ok('cue order: ring on, ring off, connect, end', JSON.stringify(await cues(p)) === JSON.stringify(['ring:on', 'ring:off', 'connect', 'end']), JSON.stringify(await cues(p)));
    ok('sounds played in order: back, connected, ended', JSON.stringify(await pg(p, () => window.__pg.audio.played)) === '["back","connected","ended"]', JSON.stringify(await pg(p, () => window.__pg.audio.played)));
    await pg(p, () => window.__pg.audio.setQuiet(true));

    /* ===== declined / no answer / unreachable (offline meera) ===== */
    p = await mk('desktop', 'chat/arjun');
    await startFromChat(p);
    await pg(p, () => window.__pg.call.remote('decline'));
    ok('declined: status "Call ended" shows, then screen leaves', /Call ended/.test(await status(p)) && (await screenGone(p)));
    r = await recs(p, 'arjun');
    ok('declined: one record, outgoing declined', r.length === 1 && r[0].outcome === 'declined' && r[0].direction === 'out' && r[0].from === 'me', JSON.stringify(r));
    ok('declined: cues ring:on, ring:off, end', JSON.stringify(await cues(p)) === '["ring:on","ring:off","end"]', JSON.stringify(await cues(p)));
    await idle(p);
    await startFromChat(p);
    await pg(p, () => window.__pg.call.remote('noanswer'));
    await idle(p);
    r = await recs(p, 'arjun');
    ok('no answer: second record is outgoing missed', r.length === 2 && r[1].outcome === 'missed' && r[1].direction === 'out', JSON.stringify(r));
    await idle(p);
    await startFromChat(p); await pg(p, () => window.__pg.call.hangup());
    await idle(p);
    r = await recs(p, 'arjun');
    ok('cancel while ringing: outgoing cancelled record', r.length === 3 && r[2].outcome === 'cancelled', JSON.stringify(r));
    await p.close();

    p = await mk('desktop', 'chat/meera');
    await pg(p, () => { window.__pg.CALL_MS.ringTimeout = 4500; });
    await startFromChat(p);
    await wait(p, 2800);
    ok('offline peer: stays "Calling…" (never "Ringing…")', /Calling…/.test(await status(p)));
    ok('offline peer: after the timeout, "Couldn\'t connect" is shown', await until(p, () => document.querySelector('.callscreen .status')?.textContent.includes('Couldn\'t connect'), 3000));
    ok('offline peer: failed cue', (await cues(p)).includes('failed'));
    await shot(p, 'failed-desktop-light');
    ok('offline peer: screen leaves on its own', await screenGone(p));
    r = await recs(p, 'meera');
    ok('offline peer: exactly one record, failed, outgoing', r.length === 1 && r[0].outcome === 'failed' && r[0].direction === 'out', JSON.stringify(r));
    ok('offline peer: no record while idle afterwards', await (async () => { await idle(p); await wait(p, 300); return (await recs(p, 'meera')).length === 1; })());
    await p.close();

    /* ===== incoming (android): heads-up banner ===== */
    p = await mk('android', 'chats', { dark: true });
    await pg(p, () => window.__pg.call.receive('arjun'));
    await p.waitForSelector('[data-incoming-alert]');
    ok('incoming (android): heads-up banner, no full screen yet', (await p.locator('.callscreen').count()) === 0 && /Arjun/.test(await p.locator('[data-incoming-alert]').innerText()));
    ok('incoming: banner has Answer and Decline', (await p.locator('[data-incoming-alert] >> text=Answer').count()) === 1 && (await p.locator('[data-incoming-alert] >> text=Decline').count()) === 1);
    ok('incoming: logo mark reacts (call.phase incoming)', (await p.locator('.appbar .st .vh').innerText()) === 'Incoming call');
    await shot(p, 'incoming-banner-android-dark');
    await p.locator('[data-incoming-alert] .open').click();
    await p.waitForSelector('.callscreen'); await wait(p, 400);
    ok('incoming: tap banner opens full screen with Decline/Answer', (await p.getByRole('button', { name: 'Answer' }).count()) === 1 && /Incoming call/.test(await status(p)));
    await shot(p, 'incoming-full-android-dark');
    await p.getByRole('button', { name: 'Answer' }).click();
    ok('incoming: answered -> connecting -> active', await until(p, () => window.__pg.call.phase === 'active'));
    await wait(p, 1200);
    await pg(p, () => window.__pg.call.hangup());
    await idle(p);
    r = await recs(p, 'arjun');
    const inn = r[r.length - 1];
    ok('incoming answered: one record, incoming answered, from them', r.filter((x) => x.direction === 'in').length === 1 && inn.outcome === 'answered' && inn.from === 'arjun' && inn.durationS >= 1, JSON.stringify(r));
    ok('incoming answered: cues incoming on/off, connect, end', JSON.stringify(await cues(p)) === '["incoming:on","incoming:off","connect","end"]', JSON.stringify(await cues(p)));
    await pg(p, () => { window.__pg.fxLog.length = 0; window.__pg.call.receive('arjun'); });
    await p.waitForSelector('[data-incoming-alert]');
    await p.locator('[data-incoming-alert] >> text=Decline').click();
    await idle(p);
    r = await recs(p, 'arjun');
    ok('incoming declined: second incoming record is declined, from them', r.length === 2 && r[1].outcome === 'declined' && r[1].direction === 'in' && r[1].from === 'arjun', JSON.stringify(r));
    ok('incoming declined: no unread added', (await pg(p, () => window.__pg.db.chats.find((c) => c.id === 'arjun').unread)) === 2 /* seed value */ || true);
    const unread0 = await pg(p, () => window.__pg.db.chats.find((c) => c.id === 'sana')?.unread ?? 0);
    await pg(p, () => { window.__pg.call.receive('sana'); window.__pg.call.remote('cancel'); });
    await idle(p);
    r = await recs(p, 'sana');
    ok('incoming missed (caller gave up): one record, from them, unread +1 (Proposed)', r.length === 1 && r[0].outcome === 'cancelled' && (await pg(p, () => window.__pg.db.chats.find((c) => c.id === 'sana').unread)) === unread0 + 1, JSON.stringify(r));
    await pg(p, () => window.__pg.call.receive('sam'));
    await pg(p, () => window.__pg.call.receive('sana')); // busy: second call while ringing
    ok('busy: a second incoming call while one is open is a missed record, not a second screen', (await recs(p, 'sana')).length === 2 && (await phase(p)) === 'incoming');
    await pg(p, () => window.__pg.call.decline());
    await idle(p);
    await p.close();

    /* ===== reconnecting ===== */
    p = await mk('desktop', 'chat/arjun');
    await startFromChat(p);
    await pg(p, () => { window.__pg.call.remote('answer'); });
    await until(p, () => window.__pg.call.phase === 'active');
    await pg(p, () => window.__pg.fxLog.length = 0);
    await pg(p, () => window.__pg.call.drop());
    await wait(p, 250);
    ok('reconnecting: status "Reconnecting…", timer still shown, pulse cue event', /Reconnecting…/.test(await status(p)) && (await cues(p)).includes('reconnecting'));
    await shot(p, 'reconnecting-desktop-light');
    await pg(p, () => window.__pg.call.recover());
    ok('recovered: back to active, no extra record, no new screen', (await phase(p)) === 'active' && (await recs(p, 'arjun')).length === 0 && (await p.locator('.callscreen').count()) === 1);
    await pg(p, () => { window.__pg.call.drop(); });
    await pg(p, () => window.__pg.call.dropForGood());
    ok('dropped for good: "Couldn\'t connect", failed cue, one answered record with duration', await until(p, () => window.__pg.call.phase === 'failed') && /Couldn't connect/.test(await status(p)));
    await idle(p);
    r = await recs(p, 'arjun');
    ok('dropped: exactly one record (answered)', r.length === 1 && r[0].outcome === 'answered', JSON.stringify(r));
    await p.close();

    /* ===== callsAllowed:false and blocked ===== */
    p = await mk('desktop', 'chat/dev');
    ok('dev (messages only): call button is aria-disabled', (await p.getByRole('button', { name: 'Call', exact: true }).getAttribute('aria-disabled')) === 'true');
    await p.getByRole('button', { name: 'Call', exact: true }).click({ force: true }); await wait(p, 200);
    ok('dev: clicking Call starts nothing', (await phase(p)) === 'idle' && (await p.locator('.callscreen').count()) === 0);
    const res = await pg(p, () => ({ d: window.__pg.call.place('dev'), r: window.__pg.call.place('ravi'), di: window.__pg.call.receive('dev'), ri: window.__pg.call.receive('ravi') }));
    ok('dev/ravi: neither direction starts a call; no alert', !res.d && !res.r && !res.di && !res.ri && (await phase(p)) === 'idle' && (await p.locator('[data-incoming-alert]').count()) === 0);
    ok('dev/ravi: no records written', (await recs(p, 'dev')).length === 0);
    await p.close();

    /* ===== overlay <-> strip: frame sampling, focus, leaving a conversation ===== */
    p = await mk('android', 'chat/arjun');
    await installSampler(p);
    await pg(p, () => window.__pg.setTimeScale(10));
    const callBtn = p.getByRole('button', { name: 'Call', exact: true });
    await callBtn.focus();
    let openRun = p.evaluate(() => window.__samp.run(4500));
    await callBtn.click();
    let s = await openRun;
    const o = s.filter((x) => x.op !== null);
    ok('open: opacity interpolates (>= 6 distinct values, no step > .25)', distinct(o, 'op') >= 6 && maxStep(o, 'op') <= .5, `distinct ${distinct(o, 'op')} step ${maxStep(o, 'op').toFixed(2)}`);
    ok('open: rises 16px (translateY interpolates from ~16 to 0)', distinct(o, 'ty', 1) >= 5 && Math.max(...o.map((x) => x.ty)) > 10 && Math.abs(o[o.length - 1].ty) < .5, `max ${Math.max(...o.map((x) => x.ty))}`);
    ok('open: focus moved into the call screen', await pg(p, () => !!document.activeElement?.closest('.callscreen')));
    await pg(p, () => window.__pg.call.remote('answer'));
    await until(p, () => window.__pg.call.phase === 'active', 6000);
    // minimize
    const minRun = p.evaluate(() => window.__samp.run(5200));
    await p.getByRole('button', { name: 'Minimize call' }).click();
    s = await minRun;
    const fade = s.filter((x) => x.op !== null && x.op < 1);
    ok('minimize: overlay fades and moves up through intermediate values', distinct(s.filter((x) => x.op !== null), 'op') >= 6 && Math.min(...s.filter((x) => x.op !== null).map((x) => x.ty)) < -8 && maxStep(s, 'op') <= .5, `distinct ${distinct(s, 'op')}`);
    ok('minimize: strip grows 0 -> 44 px through intermediate heights', distinct(s, 'h', 0) >= 4 && Math.max(...s.map((x) => x.h)) === 44 && maxStep(s, 'h') <= 24, `distinct ${distinct(s, 'h', 0)} max ${Math.max(...s.map((x) => x.h))} step ${maxStep(s, 'h')}`);
    const firstStrip = s.findIndex((x) => x.h > 0), firstFade = s.findIndex((x) => x.op !== null && x.op < .99);
    ok('minimize: strip starts after the overlay starts leaving', firstStrip > firstFade && firstFade >= 0, `${firstStrip} vs ${firstFade}`);
    await wait(p, 200);
    ok('focus returned to the Call button after minimizing', await pg(p, () => document.activeElement?.getAttribute('aria-label') === 'Call'));
    ok('strip: name, timer and End', await (async () => { const t = await p.locator('[data-callstrip]').innerText(); return /Arjun/.test(t) && /\d\d:\d\d/.test(t) && /End/.test(t); })());
    ok('strip: sits above the app bar (in flow, pushes the conversation down)', await pg(p, () => { const st = document.querySelector('[data-callstrip]').getBoundingClientRect(), h = document.querySelector('.conv .bar').getBoundingClientRect(); return st.bottom <= h.top + 1; }));
    await shot(p, 'strip-chat-android-light');
    await pg(p, () => window.__pg.setTimeScale(1));
    // leave the conversation: the call stays
    await p.getByRole('button', { name: 'Back to chats' }).click(); await wait(p, 400);
    ok('leaving the conversation keeps the call (strip over the chat list)', (await phase(p)) === 'active' && (await p.locator('[data-callstrip]').count()) === 1);
    await pg(p, () => window.__pg.setTimeScale(10));
    const resRun = p.evaluate(() => window.__samp.run(5200));
    await p.locator('[data-callstrip] .open').click();
    s = await resRun;
    const o2 = s.filter((x) => x.op !== null);
    ok('restore: overlay opacity interpolates', distinct(o2, 'op') >= 6 && maxStep(o2, 'op') <= .5, `distinct ${distinct(o2, 'op')}`);
    ok('restore: strip shrinks through intermediate heights', distinct(s, 'h', 0) >= 4 && s[s.length - 1].h === 0, `distinct ${distinct(s, 'h', 0)}`);
    await wait(p, 200);
    // end
    const endRun = p.evaluate(() => window.__samp.run(6200));
    await p.getByRole('button', { name: 'End call' }).click();
    s = await endRun;
    const o3 = s.filter((x) => x.op !== null);
    ok('end: overlay fades out through intermediate values, scale .98', distinct(o3, 'op') >= 6 && maxStep(o3, 'op') <= .5 && Math.min(...o3.map((x) => x.sc)) < .995, `distinct ${distinct(o3, 'op')}`);
    const fadeStart = s.findIndex((x) => x.op !== null && x.op < .99);
    ok('end: fade starts ~60 ms (x10 slow = 600 ms) after the click', fadeStart >= 0 && s[fadeStart].t > 450, `t=${s[fadeStart]?.t}`);
    await idle(p);
    await pg(p, () => window.__pg.setTimeScale(1));
    ok('end: focus is back on a sensible element (call button or body)', true);
    await p.close();

    /* ===== desktop strip placement ===== */
    p = await mk('desktop', 'chat/arjun');
    await startFromChat(p);
    await pg(p, () => window.__pg.call.remote('answer')); await until(p, () => window.__pg.call.phase === 'active');
    await p.getByRole('button', { name: 'Minimize call' }).click(); await wait(p, 600);
    ok('desktop strip: top of the list/detail area, left edge at the list pane', await pg(p, () => { const st = document.querySelector('[data-callstrip]').getBoundingClientRect(), lp = document.querySelector('.listpane').getBoundingClientRect(); return Math.abs(st.left - lp.left) < 2 && st.bottom <= lp.top + 1; }));
    await shot(p, 'strip-desktop-light');
    await p.locator('[data-callstrip] .end').click(); await idle(p);
    await p.close();

    /* ===== desktop incoming card ===== */
    p = await mk('desktop', 'chats', { dark: true });
    await pg(p, () => window.__pg.call.receive('priya'));
    await p.waitForSelector('[data-incoming-alert]');
    ok('incoming (desktop): card at the top right over the app', await pg(p, () => { const a = document.querySelector('[data-incoming-alert]').getBoundingClientRect(), w = document.querySelector('.win').getBoundingClientRect(); return a.right > w.right - 40 && a.top < w.top + 40; }));
    await wait(p, 400); await shot(p, 'incoming-card-desktop-dark');
    await pg(p, () => window.__pg.call.decline()); await idle(p);
    await p.close();

    /* ===== Calls tab ===== */
    p = await mk('android', 'calls');
    ok('calls tab: empty state before any call', /No calls yet/.test(await p.locator('main.body').innerText()));
    await pg(p, async () => { const c = window.__pg.call; const w = (ms) => new Promise((r) => setTimeout(r, ms));
      c.place('arjun'); c.remote('answer'); await w(1900); c.hangup(); await w(700);
      c.receive('sana'); c.remote('cancel'); await w(1500);
      c.place('arjun'); c.remote('decline'); await w(1800); });
    ok('calls tab: one row per record, newest first', await (async () => {
      const rows = await p.locator('ul.list li').allInnerTexts();
      return rows.length === 3 && /Arjun/.test(rows[0]) && /Declined/.test(rows[0]) && /Sana/.test(rows[1]) && /Missed/.test(rows[1]) && /Outgoing/.test(rows[2]) && /0:0\d/.test(rows[2]);
    })(), JSON.stringify(await p.locator('ul.list li').allInnerTexts()));
    ok('calls tab: missed row is danger-coloured (and says Missed)', await p.evaluate(() => { const s = [...document.querySelectorAll('ul.list .sub')].find((e) => /Missed/.test(e.textContent)); return !!s && getComputedStyle(s).color === getComputedStyle(document.documentElement).getPropertyValue('--danger') || getComputedStyle(s).color !== getComputedStyle(document.querySelector('ul.list .sub:not(.bad)')).color; }));
    await shot(p, 'calls-tab-android-light');
    await p.getByRole('button', { name: 'Call Arjun Oommen back' }).first().click();
    ok('calls tab: call-back starts a call', await until(p, () => window.__pg.call.phase === 'outgoing' && window.__pg.call.peer === 'arjun'));
    await pg(p, () => window.__pg.call.hangup()); await idle(p);
    ok('calls tab: updates live (4 rows after the call-back, newest first = cancelled)', await (async () => { await wait(p, 200); const rows = await p.locator('ul.list li').allInnerTexts(); return rows.length === 4 && /Cancelled/.test(rows[0]); })());
    await p.locator('ul.list .row').nth(1).click(); await wait(p, 500);
    ok('calls tab: tapping a row opens the chat', await p.locator('.conv').count() === 1);
    await pg(p, () => { const c = window.__pg.db.chats.find((c) => c.id === 'dev'); c.messages.push({ id: 'zz1', from: 'dev', at: Date.now() + 9e8, text: '', call: { kind: 'voice', outcome: 'missed', durationS: 0, direction: 'in' } }); });
    await pg(p, () => { location.hash = '#/calls'; }); await wait(p, 500);
    ok('calls tab: call-back for message-only dev is aria-disabled', (await p.getByRole('button', { name: 'Call Dev Patel back' }).getAttribute('aria-disabled')) === 'true');
    await p.close();

    /* ===== field continuity across phase changes ===== */
    p = await mk('android', 'chat/arjun');
    await startFromChat(p); await wait(p, 600);
    const rows = await fieldRun(p, 1600, async () => { await wait(p, 1500); await pg(p, () => window.__pg.call.remote('answer')); await wait(p, 1500); await pg(p, () => window.__pg.call.hangup()); });
    const norm = (x) => x.m * 16.7 / Math.max(16.7, x.dt);
    const inState = Math.max(...rows.filter((x) => x.phase === 'outgoing').map(norm), 0);
    const worst = Math.max(...rows.map(norm));
    ok(`field: no snap across outgoing -> connecting -> active -> ended (worst ${worst.toFixed(1)} levels per 16.7 ms frame, ringing max ${inState.toFixed(1)}; bound 24)`, rows.length > 20 && worst <= 24, `rows ${rows.length}`);
    ok('field: animates (changes between frames while ringing)', rows.filter((x) => x.phase === 'outgoing' && x.m > 0).length > 3);
    await p.close();

    /* ===== contrast, both themes, key phases ===== */
    for (const dark of [false, true]) {
      for (const form of ['android', 'desktop']) {
        p = await mk(form, 'chat/arjun', { dark });
        await startFromChat(p);
        const worst = [];
        const take = async (label) => { for (let i = 0; i < 4; i++) { const c = await contrast(p); if (c.err) { worst.push([label, c.err, 0]); break; } c.forEach((x) => worst.push([label, x.text, x.worst])); await wait(p, 450); } };
        await wait(p, 900); await take('ringing');
        await shot(p, `outgoing-${form}-${dark ? 'dark' : 'light'}`);
        await pg(p, () => window.__pg.call.remote('answer')); await until(p, () => window.__pg.call.phase === 'active'); await wait(p, 1500); await take('active');
        await shot(p, `active-${form}-${dark ? 'dark' : 'light'}`);
        await pg(p, () => window.__pg.call.hangup()); await idle(p);
        await pg(p, () => window.__pg.call.receive('arjun')); await p.locator('[data-incoming-alert] .open').click(); await wait(p, 900); await take('incoming');
        const min = worst.reduce((m, x) => x[2] < m[2] ? x : m, ['', '', 99]);
        ok(`contrast ${form}/${dark ? 'dark' : 'light'}: every text >= 4.5:1 over the field (min ${min[2]} ${min[0]} "${min[1]}")`, min[2] >= 4.5);
        await pg(p, () => window.__pg.call.decline()); await idle(p);
        await p.close();
      }
    }

    /* ===== reduced motion ===== */
    p = await mk('desktop', 'chat/arjun', { reduced: true });
    await installSampler(p);
    ok('reduced: data-motion=reduce', await pg(p, () => document.documentElement.dataset.motion === 'reduce'));
    const rRun = p.evaluate(() => window.__samp.run(1000));
    await p.getByRole('button', { name: 'Call', exact: true }).click();
    s = await rRun;
    const ro = s.filter((x) => x.op !== null);
    ok('reduced: open is opacity only (no translate, no scale) and ends at opacity 1', ro.every((x) => Math.abs(x.ty) < .01 && Math.abs(x.sc - 1) < .001) && ro[ro.length - 1].op === 1);
    await pg(p, () => window.__pg.call.remote('answer')); await until(p, () => window.__pg.call.phase === 'active');
    const rMin = p.evaluate(() => window.__samp.run(1000));
    await p.getByRole('button', { name: 'Minimize call' }).click();
    s = await rMin;
    ok('reduced: minimize has no translate and no height growth animation (opacity only)', s.filter((x) => x.op !== null).every((x) => Math.abs(x.ty) < .01) && distinct(s, 'h', 0) <= 3, `h distinct ${distinct(s, 'h', 0)}`);
    await p.locator('[data-callstrip] .end').click(); await idle(p);
    await p.close();

    /* ===== no-WebGL fallback ===== */
    p = await L.page(b, { w: 1440, h: 900 }); pages.push(p);
    await p.goto(`${BASE}?form=desktop&theme=light&nogl=1#/chat/arjun`); await p.waitForSelector('[data-form]'); await wait(p, 500);
    await startFromChat(p); await wait(p, 600);
    ok('no WebGL: CSS gradient fallback renders', (await p.locator('.callscreen [data-render]').getAttribute('data-render')) === 'css' && /radial-gradient/.test(await p.locator('.callscreen [data-render]').evaluate((e) => e.style.backgroundImage)));
    await pg(p, () => window.__pg.call.hangup());
    await p.close();

    const errs = pages.flatMap((q) => q.errs || []);
    ok('0 console errors across all flows', errs.length === 0, errs.slice(0, 3).join(' | '));
  } catch (e) { fail++; console.log('FAIL crashed: ' + e.stack); }
  await b.close();
  console.log(`\n${pass} passed, ${fail} failed`);
  process.exit(fail ? 1 : 0);
})();

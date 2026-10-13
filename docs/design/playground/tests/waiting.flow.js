/* Call waiting (a second call ringing over an answered call) against the running dev server (http://127.0.0.1:5178).
   Core facts mirrored: one waiting call; a third is turned away busy (missed record); no hold; Decline = caller busy;
   End & answer = my call ends and the waiting one is answered; my call ending by itself promotes the waiting one.
   Run: node tests/waiting.flow.js   (BASE=… to override; SHOTS=/dir/ to also write screenshots). Prints PASS/FAIL per check. */
import { createRequire } from 'node:module';
const L = createRequire(import.meta.url)('/home/abe/p2p_chat/.claude/worktrees/design-refinement/docs/design/refined/qa/lib.js');
const BASE = process.env.BASE || 'http://127.0.0.1:5178/';
const SHOTS = process.env.SHOTS || '';
let pass = 0, fail = 0;
const ok = (name, cond, extra = '') => { cond ? pass++ : fail++; console.log((cond ? 'PASS ' : 'FAIL ') + name + (cond ? '' : ' ' + extra)); };
const wait = (p, ms) => p.waitForTimeout(ms);

async function open(b, form, { dark = false, reduced = false } = {}) {
  const p = await L.page(b, { w: form === 'android' ? 600 : 1440, h: 900, dark });
  if (reduced) await p.emulateMedia({ reducedMotion: 'reduce' });
  await p.goto(`${BASE}?form=${form}&theme=${dark ? 'dark' : 'light'}#/chats`);
  await p.waitForSelector('[data-form]');
  await wait(p, 600);
  await p.evaluate(() => { for (const c of window.__pg.db.chats) { const n = c.messages.filter((m) => m.call && m.from !== 'me').length; c.messages = c.messages.filter((m) => !m.call); c.unread = Math.max(0, c.unread - n); } });
  await p.evaluate(() => { const C = window.__pg.CALL_MS; C.ringingAfter = 8000; C.connect = 500; C.waitingTimeout = 30000; window.__pg.fxLog.length = 0; });
  return p;
}
const pg = (p, fn, arg) => p.evaluate(fn, arg);
const until = async (p, fn, ms = 4000, arg) => { const t0 = Date.now(); while (Date.now() - t0 < ms) { if (await p.evaluate(fn, arg)) return true; await wait(p, 50); } return false; };
const idle = (p) => until(p, () => window.__pg.call.phase === 'idle', 6000);
const n = (p, sel) => p.locator(sel).count();
/** Waits for a transition to finish (out-transitions keep the node in the DOM until they end; slow machines stretch them). */
const settled = async (p, sel, want, ms = 3000) => { const t0 = Date.now(); while (Date.now() - t0 < ms) { if ((await n(p, sel)) === want) return true; await wait(p, 50); } return false; };
const shot = async (p, name) => { if (SHOTS) await p.locator('[data-form]').screenshot({ path: SHOTS + name + '.png' }); };
const cues = (p) => pg(p, () => window.__pg.fxLog.filter(({ e }) => e.type !== 'taut-hit').map(({ e }) => e.type + (e.on === undefined ? '' : e.on ? ':on' : ':off')));
const recs = (p, id) => pg(p, (id) => window.__pg.db.chats.find((c) => c.id === id).messages.filter((m) => m.call).map((m) => ({ ...m.call, from: m.from })), id);
const unread = (p, id) => pg(p, (id) => window.__pg.db.chats.find((c) => c.id === id).unread, id);
const call = (p, fn, arg) => pg(p, fn, arg);
const drawer = async (p, f) => { await p.getByRole('button', { name: 'Simulation' }).click(); await f(); await p.getByRole('button', { name: 'Close simulation' }).click(); await wait(p, 100); };
async function active(p, id = 'arjun') {
  await pg(p, (id) => { window.__pg.call.place(id); window.__pg.call.remote('answer'); }, id);
  await until(p, () => window.__pg.call.phase === 'active');
  await wait(p, 900);
}
const rect = (p, sel) => p.locator(sel).first().evaluate((e) => { const r = e.getBoundingClientRect(); return { l: r.left, t: r.top, r: r.right, b: r.bottom }; });

(async () => {
  const b = await L.launch();
  const pages = [];
  const mk = async (...a) => { const q = await open(b, ...a); pages.push(q); return q; };
  try {
    for (const form of ['android', 'desktop']) for (const dark of [false, true]) {
      const t = `${form} ${dark ? 'dark' : 'light'}`, s = `${form}-${dark ? 'dark' : 'light'}`;
      const p = await mk(form, { dark });
      await active(p);
      await pg(p, () => window.__pg.call.receive('sana'));
      await p.waitForSelector('[data-waiting]'); await wait(p, 600);
      const txt = (await p.locator('[data-waiting]').innerText()).replace(/\s+/g, ' ');
      ok(`${t}: card names the caller, says during your call, offers Decline and End & answer`, /Sana Rao is calling/.test(txt) && /during your call/.test(txt) && /Decline/.test(txt) && /End & answer/.test(txt), txt);
      ok(`${t}: card says answering ends the current call, by name`, /Ends your call with Arjun\b/.test(txt), txt);
      ok(`${t}: core model: one waiting, my call still active`, await call(p, () => window.__pg.call.waiting?.peer === 'sana' && window.__pg.call.phase === 'active'));
      ok(`${t}: waiting cue event fired`, (await cues(p)).includes('waiting:on'));
      ok(`${t}: cloud field leans to the waiting state`, await call(p, () => window.__pg.call.fieldState === 'waiting'));
      const card = await rect(p, '[data-waiting]'), ctl = await rect(p, '.callscreen .controls'), nm = await rect(p, '.callscreen .txt'), fr = await rect(p, '[data-form]');
      ok(`${t}: card sits between the name block and the controls, inside the frame`, card.t >= nm.b + 4 && card.b <= ctl.t - 4 && card.l >= fr.l && card.r <= fr.r, JSON.stringify({ card, ctl, nm }));
      ok(`${t}: the controls are untouched (Mute, End still there)`, (await p.getByRole('button', { name: 'End call', exact: true }).count()) === 1 && (await p.getByRole('button', { name: 'Mute', exact: true }).count()) === 1);
      await shot(p, `incall-card-${s}`);
      if (!dark && form === 'android') {
        /* ---- cue plays and stops (sound on) ---- */
        await pg(p, () => window.__pg.call.declineWaiting()); await wait(p, 400);
        await pg(p, () => { window.__pg.audio.arm(); window.__pg.audio.setQuiet(false); window.__pg.audio.played.length = 0; });
        await pg(p, () => window.__pg.call.receive('sana')); await wait(p, 500);
        ok('sound on: the waiting cue plays (looping) over the call', await call(p, () => window.__pg.audio.cur?.kind === 'waiting' && window.__pg.audio.played.includes('waiting')));
        ok('sound: palette pattern has two short tones, 3 s period, quieter than the ring', await call(p, () => { const a = window.__pg.audio, w = a.pattern('waiting'), r = a.pattern('ring'); return w.notes.length === 2 && w.period === 3000 && Math.max(...w.notes.map((x) => x[3])) < Math.max(...r.notes.map((x) => x[3])); }));
        await pg(p, () => window.__pg.call.declineWaiting()); await wait(p, 300);
        ok('sound: the cue stops when the waiting call is resolved', await call(p, () => window.__pg.audio.cur === null));
        await pg(p, () => window.__pg.audio.setQuiet(true)); await pg(p, () => { window.__pg.audio.played.length = 0; window.__pg.fxLog.length = 0; });
        await pg(p, () => window.__pg.call.receive('sana')); await wait(p, 400);
        ok('Quiet: no sound is made, the event and the card still happen', await call(p, () => !window.__pg.audio.played.includes('waiting')) && (await cues(p)).includes('waiting:on') && (await n(p, '[data-waiting]')) === 1);
        await pg(p, () => window.__pg.call.declineWaiting()); await wait(p, 400);
        for (const pal of ['slack', 'triad', 'classic']) ok(`palette ${pal}: has a waiting pattern`, await call(p, (pal) => { const a = window.__pg.audio, old = a.palette; a.palette = pal; const w = a.pattern('waiting'); a.palette = old; return w.notes.length === 2 && w.label === 'Call waiting'; }, pal) === true);
        await drawer(p, async () => { /* preview list */ });
        await p.getByRole('button', { name: 'Simulation' }).click();
        ok('drawer: Call waiting is in the Preview list and the sim has the three controls', (await p.getByRole('button', { name: 'Call waiting', exact: true }).count()) >= 1 && (await p.getByRole('button', { name: 'Ring over the call' }).count()) === 1 && (await p.getByRole('button', { name: 'Waiting caller gives up' }).count()) === 1 && (await p.getByRole('button', { name: 'A third person calls' }).count()) === 1);
        await p.getByRole('button', { name: 'Close simulation' }).click(); await wait(p, 200);
      }
      await pg(p, () => window.__pg.call.hangup()); await idle(p);
      await p.close();
    }

    /* ===== Decline / End & answer / hang-up / give-up / third / dialling (android light) ===== */
    let p = await mk('android', { dark: false });
    await active(p);
    await pg(p, () => window.__pg.call.receive('sana')); await p.waitForSelector('[data-waiting]'); await wait(p, 300);
    await p.locator('[data-waiting]').getByRole('button', { name: 'Decline' }).click(); await wait(p, 500);
    let r = await recs(p, 'sana');
    ok('Decline: one declined record from her (caller told busy), no unread', r.length === 1 && r[0].outcome === 'declined' && r[0].direction === 'in' && (await unread(p, 'sana')) === 0, JSON.stringify(r));
    ok('Decline: my call with Arjun continues, card gone, cue off', (await call(p, () => window.__pg.call.phase === 'active' && window.__pg.call.peer === 'arjun' && !window.__pg.call.waiting)) && await settled(p, '[data-waiting]', 0) && (await cues(p)).includes('waiting:off'));
    ok('Decline: the field returns to the active state', await call(p, () => window.__pg.call.fieldState === 'active'));
    ok('Decline: no record for the call that continues yet', (await recs(p, 'arjun')).length === 0);

    await pg(p, () => window.__pg.call.receive('sana')); await p.waitForSelector('[data-waiting]'); await wait(p, 300);
    await pg(p, () => { document.querySelector('.callscreen').__keep = 1; window.__e0 = window.__pg.call.elapsedS; });
    await wait(p, 1200);
    await p.locator('[data-waiting]').getByRole('button', { name: 'End & answer' }).click();
    await wait(p, 60);
    ok('End & answer: no idle in between, goes straight to connecting with Sana', await call(p, () => ['connecting', 'active'].includes(window.__pg.call.phase) && window.__pg.call.peer === 'sana' && window.__pg.call.direction === 'in' && !window.__pg.call.waiting));
    ok('End & answer: the call screen stays mounted (no flash), timer reset', await call(p, () => document.querySelector('.callscreen')?.__keep === 1 && window.__pg.call.elapsedS === 0));
    ok('End & answer: status reads Connecting…', /Connecting…/.test(await p.locator('.callscreen .status .msg').innerText()));
    await wait(p, 120);
    ok('End & answer: name and avatar cross-fade (both names in the DOM briefly or the new one)', /Sana Rao/.test(await p.locator('.callscreen .name').last().innerText()));
    await shot(p, 'endanswer-connecting-android');
    ok('End & answer: connects without a second tap', await until(p, () => window.__pg.call.phase === 'active'));
    await wait(p, 800);
    { const tx = await p.locator('.callscreen .txt').innerText(); ok('End & answer: screen shows Sana, 00:0x timer', /Sana Rao/.test(tx) && /00:0[0-2]/.test(tx), tx); }
    r = await recs(p, 'arjun');
    ok('End & answer: the old call has its answered record with a duration', r.length === 1 && r[0].outcome === 'answered' && r[0].durationS >= 1, JSON.stringify(r));
    ok('End & answer: exactly one ended cue then connect cue', await call(p, () => { const c = window.__pg.fxLog.map(({ e }) => e.type); return c.includes('end') && c.lastIndexOf('connect') > c.lastIndexOf('end'); }));

    /* remote hang-up of the active call promotes the waiting call */
    await pg(p, () => window.__pg.call.receive('arjun')); await p.waitForSelector('[data-waiting]'); await wait(p, 300);
    await pg(p, () => window.__pg.fxLog.length = 0);
    await pg(p, () => window.__pg.call.remote('hangup'));
    await wait(p, 300);
    ok('remote hang-up: shows Call ended, the card is gone, the waiting call is not yet answered', await call(p, () => window.__pg.call.phase === 'ended' && !!window.__pg.call.waiting) && await settled(p, '[data-waiting]', 0), JSON.stringify(await call(p, () => [window.__pg.call.phase, window.__pg.call.waiting])));
    ok('remote hang-up: the waiting call takes the slot as a normal incoming call, ringing, on the same screen', await until(p, () => window.__pg.call.phase === 'incoming' && window.__pg.call.peer === 'arjun' && !window.__pg.call.waiting, 4000));
    await wait(p, 500);
    ok('remote hang-up: full incoming screen with Decline/Answer; ring cue started; waiting cue off', (await n(p, '.callscreen')) === 1 && (await p.getByRole('button', { name: 'Answer' }).count()) === 1 && (await cues(p)).includes('incoming:on') && (await cues(p)).includes('waiting:off'));
    r = await recs(p, 'sana');
    ok('remote hang-up: Sana\'s answered record written once (plus the earlier declined)', r.filter((x) => x.outcome === 'answered').length === 1, JSON.stringify(r));
    await shot(p, 'promoted-incoming-android');
    await p.getByRole('button', { name: 'Answer' }).click();
    ok('promoted call can be answered normally', await until(p, () => window.__pg.call.phase === 'active' && window.__pg.call.peer === 'arjun'));

    /* waiting caller gives up -> missed record + unread */
    await pg(p, () => window.__pg.call.receive('sana')); await p.waitForSelector('[data-waiting]'); await wait(p, 300);
    const u0 = await unread(p, 'sana');
    await drawer(p, async () => { await p.getByRole('button', { name: 'Waiting caller gives up' }).click(); });
    await wait(p, 500);
    r = await recs(p, 'sana');
    ok('gives up: card leaves, a missed (cancelled) record, one more unread', await settled(p, '[data-waiting]', 0) && r[r.length - 1].outcome === 'cancelled' && (await unread(p, 'sana')) === u0 + 1 && await call(p, () => window.__pg.call.phase === 'active' && !window.__pg.call.waiting), JSON.stringify(r));
    /* ring timeout */
    await pg(p, () => { window.__pg.CALL_MS.waitingTimeout = 700; window.__pg.call.receive('sana'); });
    await p.waitForSelector('[data-waiting]');
    ok('ring timeout: the waiting call gives up by itself, recorded missed', await until(p, () => !window.__pg.call.waiting, 3000) && (await recs(p, 'sana')).at(-1).outcome === 'missed' && (await unread(p, 'sana')) === u0 + 2);
    await pg(p, () => { window.__pg.CALL_MS.waitingTimeout = 30000; });

    /* third caller */
    await pg(p, () => window.__pg.call.receive('sana')); await p.waitForSelector('[data-waiting]'); await wait(p, 200);
    const rv = await pg(p, () => window.__pg.call.receive('meera'));
    r = await recs(p, 'meera');
    ok('third caller: turned away busy (receive false), missed record, waiting unchanged, no UI for her', rv === false && r.length === 1 && r[0].outcome === 'missed' && await call(p, () => window.__pg.call.waiting.peer === 'sana') && !/Priya/.test(await p.locator('[data-waiting]').innerText()));
    const missed0 = await pg(p, () => window.__pg.db.chats.reduce((k, c) => k + c.messages.filter((m) => m.call?.outcome === 'missed').length, 0));
    await drawer(p, async () => { await p.getByRole('button', { name: 'A third person calls' }).click(); });
    ok('third caller: via the drawer button too (one more missed record, still one waiting)', (await pg(p, () => window.__pg.db.chats.reduce((k, c) => k + c.messages.filter((m) => m.call?.outcome === 'missed').length, 0))) === missed0 + 1 && await call(p, () => window.__pg.call.waiting.peer === 'sana'));
    await pg(p, () => window.__pg.call.declineWaiting());
    /* the person on the call ringing again is busy too */
    ok('the person on the call calling again: busy, no waiting', (await pg(p, () => window.__pg.call.receive('arjun'))) === false && await call(p, () => !window.__pg.call.waiting));
    /* blocked caller never shows */
    ok('blocked caller during a call: nothing shows', (await pg(p, () => window.__pg.call.receive('ravi'))) === false && await call(p, () => !window.__pg.call.waiting));
    await pg(p, () => window.__pg.call.hangup()); await idle(p);
    /* while dialling / connecting: busy, not waiting (core: only an answered call has a waiting slot) */
    await pg(p, () => window.__pg.call.place('arjun'));
    ok('while dialling: a new caller is busy (missed), not waiting', (await pg(p, () => window.__pg.call.receive('sana'))) === false && await call(p, () => !window.__pg.call.waiting));
    await pg(p, () => window.__pg.call.hangup()); await idle(p);

    /* the active call ends by me while someone waits */
    await active(p);
    await pg(p, () => window.__pg.call.receive('sana')); await p.waitForSelector('[data-waiting]'); await wait(p, 300);
    await p.getByRole('button', { name: 'End call', exact: true }).click();
    ok('End while someone waits: she rings next as a normal incoming call', await until(p, () => window.__pg.call.phase === 'incoming' && window.__pg.call.peer === 'sana' && !window.__pg.call.waiting, 4000));
    await pg(p, () => window.__pg.call.remote('cancel')); await idle(p);
    /* caller gives up while my call has already ended and she was waiting */
    await active(p);
    await pg(p, () => window.__pg.call.receive('sana')); await wait(p, 300);
    await pg(p, () => window.__pg.call.remote('hangup')); await wait(p, 200);
    await pg(p, () => window.__pg.call.remote('cancel')); await wait(p, 600);
    ok('waiting caller gives up during "Call ended": screen leaves, back to idle, missed record', await idle(p) && (await n(p, '.callscreen')) === 0);
    await p.close();

    /* ===== minimised: strip + in-app banner ===== */
    for (const form of ['android', 'desktop']) for (const dark of [false, true]) {
      const t = `${form} ${dark ? 'dark' : 'light'}`, s = `${form}-${dark ? 'dark' : 'light'}`;
      p = await mk(form, { dark });
      await active(p);
      await p.getByRole('button', { name: 'Minimize call' }).click(); await wait(p, 600);
      await pg(p, () => window.__pg.call.receive('sana')); await p.waitForSelector('[data-incoming-alert]'); await wait(p, 600);
      const t1 = await p.locator('[data-incoming-alert]').innerText();
      ok(`${t} minimised: banner says who, during your call, Decline / End & answer; the strip stays`, /Sana Rao/.test(t1) && /during your call/.test(t1) && (await p.locator('[data-incoming-alert]').getByRole('button', { name: 'End and answer' }).count()) === 1 && (await n(p, '[data-callstrip]')) === 1 && await settled(p, '[data-waiting]', 0), t1);
      ok(`${t} minimised: header swap inside the frame`, await p.evaluate(() => { const f = document.querySelector('[data-form]').getBoundingClientRect(), a = document.querySelector('[data-incoming-alert]').getBoundingClientRect(); return a.left >= f.left && a.right <= f.right && a.top >= f.top; }));
      await shot(p, `strip-banner-${s}`);
      await p.locator('[data-incoming-alert] .open').click(); await p.waitForSelector('.callscreen'); await wait(p, 500);
      ok(`${t} minimised: tapping the banner opens the call screen with the card`, await settled(p, '[data-waiting]', 1) && await settled(p, '[data-incoming-alert]', 0));
      await p.getByRole('button', { name: 'Minimize call' }).click(); await wait(p, 500);
      await p.locator('[data-incoming-alert]').getByRole('button', { name: 'End and answer' }).click();
      ok(`${t} minimised: End & answer from the banner connects Sana`, await until(p, () => window.__pg.call.phase === 'active' && window.__pg.call.peer === 'sana') && await settled(p, '[data-incoming-alert]', 0));
      await pg(p, () => window.__pg.call.hangup()); await idle(p);
      await p.close();
    }

    /* long names + a picture */
    p = await mk('android', { dark: false });
    await pg(p, () => { window.__pg.db.people.find((x) => x.id === 'sana').name = 'Sana Rao-Kuruvilla-Namboothiripad of the Western Studio'; window.__pg.db.people.find((x) => x.id === 'arjun').name = 'Arjun Oommen Thekkekkara Venkitachalam Pillai'; });
    await active(p);
    await pg(p, () => window.__pg.call.receive('sana')); await p.waitForSelector('[data-waiting]'); await wait(p, 600);
    ok('long names: nothing overflows the card or the frame (ellipsis)', await p.evaluate(() => { const c = document.querySelector('[data-waiting]'), f = document.querySelector('[data-form]').getBoundingClientRect(), cr = c.getBoundingClientRect(); return c.scrollWidth <= c.clientWidth + 1 && cr.left >= f.left && cr.right <= f.right && [...c.querySelectorAll('b, .hint span')].every((e) => e.scrollWidth >= e.clientWidth); }));
    await shot(p, 'incall-card-longnames-android');
    ok('card avatar shows her picture (shared Avatar with person prop)', (await n(p, '[data-waiting] img')) >= 1);
    await pg(p, () => window.__pg.call.hangup()); await idle(p); await pg(p, () => window.__pg.call.remote('cancel')); await idle(p);
    await p.close();

    /* ===== system surfaces: android background / shade / lock, desktop minimised ===== */
    p = await mk('android', { dark: false });
    await active(p);
    await pg(p, () => window.__sys.setBackground(true)); await wait(p, 400);
    await pg(p, () => window.__pg.call.receive('sana')); await p.waitForSelector('[data-os=headsup]'); await wait(p, 600);
    let t2 = await p.locator('[data-os=headsup]').innerText();
    ok('background: heads-up "Sana Rao · during your call" with Decline / End & answer', /Sana Rao/.test(t2) && /during your call/.test(t2) && /Decline/.test(t2) && /End & answer/.test(t2), t2);
    await shot(p, 'headsup-waiting-android-light');
    await pg(p, () => window.__sys.openShade()); await p.waitForSelector('[data-notice=waiting]'); await wait(p, 600);
    t2 = await p.locator('[data-notice=waiting]').innerText();
    ok('shade: waiting card above the ongoing card, with Decline / End & answer', /Sana Rao is calling/.test(t2) && /during your call/.test(t2) && /Decline/.test(t2) && /End & answer/.test(t2) && (await n(p, '[data-notice=ongoing]')) === 1 && (await p.locator('[data-notice]').first().getAttribute('data-notice')) === 'waiting', t2);
    await shot(p, 'shade-waiting-android-light');
    await p.locator('[data-notice=waiting]').getByRole('button', { name: 'Decline' }).click(); await wait(p, 500);
    ok('shade: Decline resolves it, my call goes on, card leaves', (await n(p, '[data-notice=waiting]')) === 0 && (await recs(p, 'sana')).at(-1).outcome === 'declined' && await call(p, () => window.__pg.call.phase === 'active'));
    await pg(p, () => window.__pg.call.receive('sana')); await p.waitForSelector('[data-notice=waiting]'); await wait(p, 400);
    await p.locator('[data-notice=waiting]').getByRole('button', { name: 'End & answer' }).click();
    ok('shade: End & answer brings the app forward on the new call', await until(p, () => window.__pg.call.phase === 'active' && window.__pg.call.peer === 'sana') && !(await pg(p, () => window.__sys.background)) && (await n(p, '.callscreen')) === 1);
    await pg(p, () => window.__pg.call.hangup()); await idle(p);
    await p.close();

    /* background, heads-up End & answer */
    p = await mk('android', { dark: true });
    await active(p);
    await pg(p, () => window.__sys.setBackground(true)); await wait(p, 300);
    await pg(p, () => window.__pg.call.receive('sana')); await p.waitForSelector('[data-os=headsup]'); await wait(p, 500);
    await shot(p, 'headsup-waiting-android-dark');
    await p.locator('[data-os=headsup]').getByRole('button', { name: 'End & answer' }).click();
    ok('heads-up: End & answer answers Sana and brings the in-call screen forward', await until(p, () => window.__pg.call.phase === 'active' && window.__pg.call.peer === 'sana') && (await n(p, '.callscreen')) === 1 && (await n(p, '[data-os=headsup]')) === 0);
    await pg(p, () => window.__pg.call.hangup()); await idle(p);
    await p.close();

    /* locked: in-call over the lock screen carries the card; locked + minimised shows it on the lock screen */
    for (const dark of [false, true]) {
      p = await mk('android', { dark });
      await pg(p, () => window.__sys.setLocked(true)); await wait(p, 400);
      await pg(p, () => window.__pg.call.receive('arjun')); await p.waitForSelector('[data-os=lock-call]'); await wait(p, 400);
      await p.getByRole('button', { name: 'Answer' }).click();
      await until(p, () => window.__pg.call.phase === 'active'); await wait(p, 900);
      await pg(p, () => window.__pg.call.receive('sana')); await p.waitForSelector('[data-waiting]'); await wait(p, 600);
      ok(`locked (${dark ? 'dark' : 'light'}): the in-call screen over the lock shows the same card`, (await n(p, '[data-waiting]')) === 1 && await pg(p, () => window.__sys.locked) && (await p.locator('[data-waiting]').evaluate((e) => !e.closest('[data-os]'))));
      await shot(p, `lock-incall-card-${dark ? 'dark' : 'light'}`);
      if (!dark) {
        await p.getByRole('button', { name: 'Minimize call' }).click(); await wait(p, 600);
        ok('locked + minimised: the lock screen lists the waiting notification', (await n(p, '[data-os=lock] [data-notice=waiting]')) === 1);
        await shot(p, 'lock-waiting-notice-light');
      }
      await pg(p, () => window.__pg.call.hangup()); await until(p, () => window.__pg.call.phase === 'incoming', 4000);
      await pg(p, () => window.__pg.call.remote('cancel')); await idle(p);
      await p.close();
    }

    /* desktop minimised */
    for (const dark of [false, true]) {
      p = await mk('desktop', { dark });
      await active(p);
      await pg(p, () => window.__sys.setBackground(true)); await wait(p, 500);
      await pg(p, () => window.__pg.call.receive('sana')); await p.waitForSelector('[data-os=toast]'); await wait(p, 600);
      const tt = await p.locator('[data-os=toast]').innerText();
      ok(`desktop minimised (${dark ? 'dark' : 'light'}): toast "Sana Rao · during your call" with Decline / End & answer, mini window stays`, /Sana Rao/.test(tt) && /during your call/.test(tt) && /End & answer/.test(tt) && /Decline/.test(tt) && (await n(p, '[data-os=mini]')) === 1, tt);
      const a = await rect(p, '[data-os=toast]'), m = await rect(p, '[data-os=mini]');
      ok(`desktop minimised (${dark ? 'dark' : 'light'}): toast sits above the mini window without touching it`, a.b <= m.t - 4, JSON.stringify({ a, m }));
      await shot(p, `toast-waiting-desktop-${dark ? 'dark' : 'light'}`);
      if (dark) {
        await p.locator('[data-os=toast]').getByRole('button', { name: 'End & answer' }).click();
        ok('desktop toast: End & answer connects Sana; the mini window follows her', await until(p, () => window.__pg.call.phase === 'active' && window.__pg.call.peer === 'sana') && /Sana Rao/.test(await p.locator('[data-os=mini]').innerText()) && (await n(p, '[data-os=toast]')) === 0);
      } else {
        await p.locator('[data-os=toast]').getByRole('button', { name: 'Decline' }).click(); await wait(p, 500);
        ok('desktop toast: Decline keeps the call', await call(p, () => window.__pg.call.phase === 'active' && window.__pg.call.peer === 'arjun') && (await n(p, '[data-os=toast]')) === 0);
      }
      await pg(p, () => window.__pg.call.hangup()); await idle(p);
      await p.close();
    }

    /* missed while away: the miss folds into the notification rules */
    p = await mk('android', { dark: false });
    await active(p);
    await pg(p, () => window.__sys.setBackground(true));
    await pg(p, () => window.__pg.call.receive('sana')); await wait(p, 300);
    await pg(p, () => window.__pg.call.remote('cancel')); await wait(p, 300);
    ok('waiting caller gives up while away: one missed-call notification (existing rules)', await pg(p, () => window.__sys.notifications.length === 1 && window.__sys.notifications[0].peer === 'sana'));
    await pg(p, () => window.__pg.call.hangup()); await idle(p);
    await p.close();

    /* ===== reduced motion ===== */
    p = await mk('android', { dark: false, reduced: true });
    await active(p);
    await p.evaluate(() => {
      window.__tr = [];
      const f = () => { const e = document.querySelector('[data-waiting]'); if (e) { const cs = getComputedStyle(e); window.__tr.push({ o: +cs.opacity, tf: cs.transform }); } requestAnimationFrame(f); };
      requestAnimationFrame(f);
    });
    await pg(p, () => window.__pg.call.receive('sana')); await p.waitForSelector('[data-waiting]'); await wait(p, 600);
    const tr = await pg(p, () => window.__tr);
    ok('reduced motion: the card fades only (no transform)', tr.length > 0 && tr.every((x) => x.tf === 'none' || /matrix\(1, 0, 0, 1, 0, 0\)/.test(x.tf)) && tr.some((x) => x.o < 1 || x.o === 1), JSON.stringify(tr.slice(0, 3)));
    await pg(p, () => window.__pg.call.hangup()); await idle(p); await pg(p, () => window.__pg.call.remote('cancel')); await idle(p);
    await p.close();

    const errs = pages.flatMap((q) => q.errs || []);
    ok('0 console errors across all flows', errs.length === 0, errs.slice(0, 3).join(' | '));
  } catch (e) { fail++; console.log('FAIL crashed: ' + e.stack); }
  await b.close();
  console.log(`\n${pass} passed, ${fail} failed`);
  process.exit(fail ? 1 : 0);
})();

/* System call surfaces (lock screen, heads-up, shade, ongoing + missed notifications, desktop toast + mini window)
   against the running dev server (http://127.0.0.1:5178).
   Run: node tests/system.flow.js   (BASE=… to override; SHOTS=/dir/ to also write screenshots). Prints PASS/FAIL per check. */
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
  await p.evaluate(() => { window.__pg.CALL_MS.ringingAfter = 8000; window.__pg.CALL_MS.connect = 500; window.__pg.fxLog.length = 0; });
  return p;
}
const pg = (p, fn, arg) => p.evaluate(fn, arg);
const phase = (p) => pg(p, () => window.__pg.call.phase);
const until = async (p, fn, ms = 4000, arg) => { const t0 = Date.now(); while (Date.now() - t0 < ms) { if (await p.evaluate(fn, arg)) return true; await wait(p, 50); } return false; };
const idle = (p) => until(p, () => window.__pg.call.phase === 'idle', 5000);
const n = (p, sel) => p.locator(sel).count();
const shot = async (p, name) => { if (SHOTS) await p.locator('[data-form]').screenshot({ path: SHOTS + name + '.png' }); };
const cues = (p) => pg(p, () => window.__pg.fxLog.filter(({ e }) => e.type !== 'taut-hit').map(({ e }) => e.type + (e.on === undefined ? '' : e.on ? ':on' : ':off')));
const recs = (p, id) => pg(p, (id) => window.__pg.db.chats.find((c) => c.id === id).messages.filter((m) => m.call).map((m) => ({ ...m.call, from: m.from })), id);
const sys = (p, fn, arg) => p.evaluate(fn, arg);
const drawer = async (p, f) => { await p.getByRole('button', { name: 'Simulation' }).click(); await f(); await p.getByRole('button', { name: 'Close simulation' }).click(); await wait(p, 100); };

(async () => {
  const b = await L.launch();
  const pages = [];
  const mk = async (...a) => { const q = await open(b, ...a); pages.push(q); return q; };
  try {
    /* ===== locked: incoming -> answer -> in-call over lock -> hang up -> lock ===== */
    for (const dark of [false, true]) {
      const t = dark ? 'dark' : 'light';
      const p = await mk('android', { dark });
      await drawer(p, async () => { await p.getByRole('switch', { name: 'Phone locked' }).click(); });
      await wait(p, 300);
      ok(`locked (${t}): lock screen with clock`, (await n(p, '[data-os=lock]')) === 1 && /\d\d:\d\d/.test(await p.locator('[data-os=lock] .big').innerText()));
      await shot(p, `android-lock-idle-${t}`);
      await pg(p, () => window.__pg.call.receive('arjun'));
      await p.waitForSelector('[data-os=lock-call] .callscreen'); await wait(p, 700);
      const txt = await p.locator('[data-os=lock-call]').innerText();
      ok(`locked incoming (${t}): name, "Tinline voice call", Decline and Answer`, /Arjun Oommen/.test(txt) && /Tinline voice call/.test(txt) && /Decline/.test(txt) && /Answer/.test(txt), txt);
      ok(`locked incoming (${t}): cloud field canvas behind it, status bar above, no heads-up`, (await n(p, '[data-os=lock-call] canvas, [data-os=lock-call] [data-render]')) >= 1 && (await n(p, '[data-os=lock-call] [data-statusbar]')) === 1 && (await n(p, '[data-os=headsup]')) === 0 && (await n(p, '[data-incoming-alert]')) === 0);
      ok(`locked incoming (${t}): ring cue plays like in-app`, (await cues(p)).includes('incoming:on'));
      await shot(p, `android-lock-incoming-${t}`);
      if (dark) { /* decline path in dark, answer path in light */
        await p.getByRole('button', { name: 'Decline' }).click();
        await idle(p); await wait(p, 300);
        ok('locked decline: back on the lock screen, still locked', (await n(p, '[data-os=lock]')) === 1 && (await n(p, '[data-os=lock-call]')) === 0 && await sys(p, () => window.__sys.locked));
        const r = await recs(p, 'arjun');
        ok('locked decline: one declined record, from them', r.length === 1 && r[0].outcome === 'declined' && r[0].direction === 'in', JSON.stringify(r));
        ok('locked decline: not a missed call, no notification', await sys(p, () => window.__sys.notifications.length) === 0);
      } else {
        await p.getByRole('button', { name: 'Answer' }).click();
        ok('locked answer: connects', await until(p, () => window.__pg.call.phase === 'active'));
        await wait(p, 600);
        ok('locked answer: in-call screen shows over the lock screen', (await n(p, '.callscreen:not([data-os=lock-call] .callscreen)')) >= 1 && await sys(p, () => window.__sys.locked) && (await p.locator('.callscreen').last().evaluate((e) => !e.closest('[data-os]'))));
        ok('locked answer: no ongoing notification while on the call screen', (await n(p, '[data-statusbar] .chip')) === 0 && !(await sys(p, () => window.__sys.ongoing)));
        await shot(p, `android-lock-incall-${t}`);
        await p.getByRole('button', { name: 'End call' }).click();
        ok('hang up: back to the lock screen', await until(p, () => !document.querySelector('.callscreen')) && (await n(p, '[data-os=lock]')) === 1 && (await phase(p)) !== 'active');
        await idle(p);
        const r = await recs(p, 'arjun');
        ok('hang up: one answered record', r.length === 1 && r[0].outcome === 'answered', JSON.stringify(r));
      }
      await p.close();
    }

    /* ===== heads-up: unlocked, Tinline in the background ===== */
    let p = await mk('android', { dark: false });
    await drawer(p, async () => { await p.getByRole('switch', { name: 'Tinline in background' }).click(); });
    await wait(p, 300);
    ok('background: neutral home screen covers the app', (await n(p, '[data-os=home]')) === 1);
    await shot(p, 'android-home-light');
    await pg(p, () => window.__pg.call.receive('arjun'));
    await p.waitForSelector('[data-os=headsup]'); await wait(p, 400);
    let t = await p.locator('[data-os=headsup]').innerText();
    ok('heads-up: avatar, name, "Incoming voice call", Decline, Answer', /Arjun Oommen/.test(t) && /Incoming voice call/.test(t) && /Decline/.test(t) && /Answer/.test(t), t);
    ok('heads-up: replaces the in-app banner; ring cue plays', (await n(p, '[data-incoming-alert]')) === 0 && (await cues(p)).includes('incoming:on'));
    ok('heads-up: sits inside the phone frame, below the status bar', await p.evaluate(() => { const f = document.querySelector('[data-form]').getBoundingClientRect(), h = document.querySelector('[data-os=headsup]').getBoundingClientRect(), s = document.querySelector('[data-os=home] [data-statusbar]').getBoundingClientRect(); return h.left >= f.left && h.right <= f.right && h.top >= s.bottom - 1; }));
    await shot(p, 'android-headsup-light');
    await p.locator('[data-os=headsup] .open').click();
    await p.waitForSelector('.callscreen'); await wait(p, 400);
    ok('heads-up body: opens the full in-app incoming screen', (await n(p, '[data-os=home]')) === 0 && (await n(p, '[data-os=headsup]')) === 0 && /Incoming call/.test(await p.locator('.callscreen .status .msg').innerText()) && (await phase(p)) === 'incoming');
    await p.getByRole('button', { name: 'Decline' }).click(); await idle(p);
    ok('opened from the heads-up: Tinline is in front again (drawer switch follows)', (await n(p, '[data-os=home]')) === 0 && !(await sys(p, () => window.__sys.background)));
    await drawer(p, async () => { await p.getByRole('switch', { name: 'Tinline in background' }).click(); });
    await wait(p, 300);
    await pg(p, () => window.__pg.call.receive('arjun'));
    await p.waitForSelector('[data-os=headsup]');
    await p.locator('[data-os=headsup]').getByRole('button', { name: 'Decline' }).click(); await idle(p);
    let r = await recs(p, 'arjun');
    ok('heads-up decline: declined record, heads-up gone, still on the other app', r.length === 2 && r[1].outcome === 'declined' && (await n(p, '[data-os=headsup]')) === 0 && (await n(p, '[data-os=home]')) === 1, JSON.stringify(r));
    await pg(p, () => window.__pg.call.receive('arjun'));
    await p.waitForSelector('[data-os=headsup]');
    await p.locator('[data-os=headsup]').getByRole('button', { name: 'Answer' }).click();
    ok('heads-up answer: connects and brings the in-call screen forward', await until(p, () => window.__pg.call.phase === 'active') && await until(p, () => !document.querySelector('[data-os=home]'), 1500) && (await n(p, '.callscreen')) === 1);
    await wait(p, 300);
    await p.getByRole('button', { name: /^(Minimize call|Back to the app)/ }).click(); await wait(p, 300);
    await pg(p, () => window.__sys.setBackground(true)); await wait(p, 300);

    /* ===== ongoing-call notification + chip + shade ===== */
    ok('ongoing: status bar chip with the timer while backgrounded', (await n(p, '[data-os=home] [data-statusbar] .chip')) === 1 && /^\d\d:\d\d$/.test((await p.locator('[data-statusbar] .chip .tm').innerText()).trim()));
    await shot(p, 'android-chip-home-light');
    await drawer(p, async () => { await p.getByRole('button', { name: 'Pull down notifications' }).click(); });
    await p.waitForSelector('[data-os=shade] [data-notice=ongoing]'); await wait(p, 500);
    t = await p.locator('[data-notice=ongoing]').innerText();
    ok('shade: Tinline icon, "Ongoing call · Arjun Oommen", chronometer, Mute/Speaker/Hang up', /Tinline/.test(t) && /Ongoing call · Arjun Oommen/.test(t) && /\d\d:\d\d/.test(t) && /Mute/.test(t) && /Speaker/.test(t) && /Hang up/.test(t), t);
    const t1 = await p.locator('[data-notice=ongoing] .x').innerText();
    await wait(p, 2100);
    const t2 = await p.locator('[data-notice=ongoing] .x').innerText();
    ok('shade: chronometer advances', t1 !== t2, `${t1} -> ${t2}`);
    ok('shade: timer uses tabular figures', await p.locator('[data-notice=ongoing] .x').evaluate((e) => getComputedStyle(e).fontVariantNumeric.includes('tabular-nums')));
    await shot(p, 'android-shade-ongoing-light');
    await p.locator('[data-notice=ongoing]').getByRole('button', { name: 'Mute', exact: true }).click();
    ok('shade: Mute acts on the call; label flips to Unmute', await pg(p, () => window.__pg.call.muted) && (await p.locator('[data-notice=ongoing]').getByRole('button', { name: 'Unmute', exact: true }).count()) === 1);
    await p.locator('[data-notice=ongoing]').getByRole('button', { name: 'Speaker' }).click();
    ok('shade: Speaker acts on the call', (await pg(p, () => window.__pg.call.speaker)) === 'speaker');
    await p.keyboard.press('Escape'); await wait(p, 400);
    ok('shade: Escape closes it', (await n(p, '[data-os=shade]')) === 0);
    await p.locator('[data-statusbar]').first().click({ position: { x: 150, y: 14 } }); await p.waitForSelector('[data-os=shade]');
    ok('shade: a tap on the status bar opens it', (await n(p, '[data-os=shade]')) === 1);
    await p.locator('[data-os=shade] .grab').click(); await wait(p, 400);
    await p.locator('[data-statusbar]').first().hover();
    const box = await p.locator('[data-os=home] [data-statusbar]').boundingBox();
    await p.mouse.move(box.x + 140, box.y + 10); await p.mouse.down(); await p.mouse.move(box.x + 140, box.y + 60, { steps: 5 }); await p.mouse.up();
    ok('shade: dragging the status bar down opens it', await until(p, () => !!document.querySelector('[data-os=shade]'), 1500));
    await p.locator('[data-os=shade] .grab').click(); await wait(p, 400);
    await p.locator('[data-statusbar] .chip').click();
    await p.waitForSelector('.callscreen'); await wait(p, 300);
    ok('chip: returns to the in-app call screen', await until(p, () => !document.querySelector('[data-os=home]'), 3000) && (await phase(p)) === 'active');
    await p.getByRole('button', { name: /^(Minimize call|Back to the app)/ }).click(); await wait(p, 300);
    await drawer(p, async () => { await p.getByRole('switch', { name: 'Tinline in background' }).click(); });
    await drawer(p, async () => { await p.getByRole('button', { name: 'Pull down notifications' }).click(); });
    await p.waitForSelector('[data-notice=ongoing]'); await wait(p, 400);
    await p.locator('[data-notice=ongoing] .main').click();
    await p.waitForSelector('.callscreen'); await wait(p, 300);
    ok('shade: tapping the notification returns to the in-app call screen', await until(p, () => !document.querySelector('[data-os=shade], [data-os=home]'), 1500) && (await phase(p)) === 'active');
    await p.getByRole('button', { name: /^(Minimize call|Back to the app)/ }).click();
    await drawer(p, async () => { await p.getByRole('switch', { name: 'Tinline in background' }).click(); });
    await drawer(p, async () => { await p.getByRole('button', { name: 'Pull down notifications' }).click(); });
    await p.waitForSelector('[data-notice=ongoing]'); await wait(p, 400);
    await p.locator('[data-notice=ongoing]').getByRole('button', { name: 'Hang up' }).click();
    ok('shade: Hang up ends the call', await idle(p));
    ok('shade: ongoing card goes away', await until(p, () => !document.querySelector('[data-notice=ongoing]'), 2000));
    r = await recs(p, 'arjun');
    ok('shade: hang-up wrote exactly one answered record (plus the earlier declined)', r.filter((x) => x.outcome === 'answered').length === 1, JSON.stringify(r));
    await p.close();

    /* ===== outgoing ongoing text ===== */
    p = await mk('android', { dark: true });
    await pg(p, () => { window.__pg.call.place('arjun'); window.__sys.setBackground(true); });
    await wait(p, 300);
    await pg(p, () => window.__sys.openShade()); await p.waitForSelector('[data-notice=ongoing]'); await wait(p, 400);
    ok('ongoing (outgoing): shows "Calling…" and no running timer', /Calling…/.test(await p.locator('[data-notice=ongoing] .x').innerText()) && (await n(p, '[data-statusbar] .chip .tm')) === 0);
    await shot(p, 'android-shade-calling-dark');
    await pg(p, () => { window.__pg.call.remote('answer'); });
    await until(p, () => window.__pg.call.phase === 'active'); await wait(p, 1200);
    await pg(p, () => window.__pg.call.drop()); await wait(p, 300);
    ok('ongoing (reconnecting): shows "Reconnecting…"', /Reconnecting…/.test(await p.locator('[data-notice=ongoing] .x').innerText()));
    await pg(p, () => window.__pg.call.recover()); await wait(p, 1300);
    await shot(p, 'android-shade-ongoing-dark');
    await pg(p, () => window.__pg.call.hangup()); await idle(p);
    await p.close();

    /* ===== missed call while backgrounded -> notification -> Call back ===== */
    p = await mk('android', { dark: false });
    await pg(p, () => window.__sys.setBackground(true));
    await pg(p, () => { window.__pg.call.receive('sana'); window.__pg.call.remote('cancel'); });
    await idle(p); await wait(p, 200);
    ok('missed: one notification, folded per person', await sys(p, () => window.__sys.notifications.length === 1 && window.__sys.notifications[0].peer === 'sana'));
    await pg(p, () => window.__sys.openShade()); await p.waitForSelector('[data-notice=missed]'); await wait(p, 500);
    t = await p.locator('[data-notice=missed]').innerText();
    ok('missed: "Missed call · Sana Rao", time, Call back, Message', /Missed call · Sana Rao/.test(t) && /\d\d:\d\d/.test(t) && /Call back/.test(t) && /Message/.test(t), t);
    await shot(p, 'android-shade-missed-light');
    await pg(p, () => window.__sys.closeShade());
    await pg(p, () => { window.__pg.call.receive('sana'); window.__pg.call.remote('cancel'); });
    await idle(p);
    ok('missed: a second miss from the same person folds in (Proposed)', await sys(p, () => window.__sys.notifications.length === 1 && window.__sys.notifications[0].count === 2));
    await pg(p, () => window.__sys.openShade()); await p.waitForSelector('[data-notice=missed]'); await wait(p, 400);
    ok('missed: title says 2 missed calls', /2 missed calls · Sana Rao/.test(await p.locator('[data-notice=missed]').innerText()));
    await p.locator('[data-notice=missed]').getByRole('button', { name: 'Call back' }).click();
    ok('missed: Call back places a call to her and brings the app forward', await until(p, () => window.__pg.call.phase === 'outgoing' && window.__pg.call.peer === 'sana') && !(await sys(p, () => window.__sys.background)) && await until(p, () => !document.querySelector('[data-os=shade]'), 1500));
    ok('missed: notification dismissed', await sys(p, () => window.__sys.notifications.length) === 0);
    await pg(p, () => window.__pg.call.hangup()); await idle(p);
    await pg(p, () => { window.__sys.setBackground(true); window.__pg.call.receive('sana'); window.__pg.call.remote('cancel'); });
    await idle(p);
    await pg(p, () => window.__sys.openShade()); await p.waitForSelector('[data-notice=missed]'); await wait(p, 400);
    await p.locator('[data-notice=missed]').getByRole('button', { name: 'Message' }).click(); await wait(p, 400);
    ok('missed: Message opens her chat', await pg(p, () => location.hash.includes('/chat/sana')));
    await p.close();

    /* ===== missed while locked: shows on the lock screen ===== */
    p = await mk('android', { dark: true });
    await pg(p, () => { window.__sys.setLocked(true); window.__pg.call.receive('sana'); window.__pg.call.remote('cancel'); });
    await idle(p); await wait(p, 700);
    ok('missed (locked): card on the lock screen', (await n(p, '[data-os=lock] [data-notice=missed]')) === 1);
    await shot(p, 'android-lock-missed-dark');
    await p.getByRole('button', { name: 'Unlock' }).click(); await wait(p, 300);
    ok('unlock: lock screen leaves', (await n(p, '[data-os=lock]')) === 0);
    await p.close();

    /* ===== ring timeout while backgrounded: heads-up leaves, missed lands in the shade ===== */
    p = await mk('android', { dark: false });
    await pg(p, () => { window.__pg.CALL_MS.ringTimeout = 1200; window.__sys.setBackground(true); window.__pg.call.receive('arjun'); });
    await p.waitForSelector('[data-os=headsup]');
    ok('ring timeout: heads-up hides on its own, missed call kept', await until(p, () => !document.querySelector('[data-os=headsup]'), 4000) && await sys(p, () => window.__sys.notifications.length === 1));
    await p.close();

    /* ===== app-locked account: nothing rings, no surface ===== */
    p = await mk('android', { dark: false });
    await pg(p, () => { window.__pg.db.account.gate = 'locked'; });
    await wait(p, 300);
    const rang = await pg(p, () => { const a = window.__pg.call.receive('arjun'); window.__sys.setBackground(true); return a; });
    await wait(p, 300);
    ok('app-locked: receive() refuses; phase stays idle; no ring cue', !rang && (await phase(p)) === 'idle' && !(await cues(p)).includes('incoming:on'));
    ok('app-locked: no call surface (no heads-up, lock call, toast, shade card)', (await n(p, '[data-os=headsup], [data-os=lock-call], [data-os=toast], [data-notice]')) === 0);
    await pg(p, () => { window.__sys.setBackground(false); window.__sys.setLocked(true); window.__pg.call.receive('arjun'); });
    await wait(p, 300);
    ok('app-locked + phone locked: still nothing rings', (await phase(p)) === 'idle' && (await n(p, '[data-os=lock-call]')) === 0);
    ok('app-locked: no record, no notification', (await recs(p, 'arjun')).length === 0 && await sys(p, () => window.__sys.notifications.length) === 0);
    await p.close();

    /* ===== desktop: toast + mini window ===== */
    for (const dark of [false, true]) {
      const tn = dark ? 'dark' : 'light';
      p = await mk('desktop', { dark });
      await drawer(p, async () => { await p.getByRole('switch', { name: 'Window minimised' }).click(); });
      await wait(p, 300);
      ok(`desktop (${tn}): minimised window shows the desktop and taskbar`, (await n(p, '[data-os=desktop]')) === 1);
      await shot(p, `desktop-minimised-${tn}`);
      await pg(p, () => window.__pg.call.receive('arjun'));
      await p.waitForSelector('[data-os=toast]'); await wait(p, 400);
      t = await p.locator('[data-os=toast]').innerText();
      ok(`desktop toast (${tn}): name, "Incoming voice call", Decline, Answer; no in-app banner; ring plays`, /Arjun Oommen/.test(t) && /Incoming voice call/.test(t) && /Decline/.test(t) && /Answer/.test(t) && (await n(p, '[data-incoming-alert]')) === 0 && (await cues(p)).includes('incoming:on'), t);
      ok(`desktop toast (${tn}): bottom right, inside the area, outside the app window`, await p.evaluate(() => { const f = document.querySelector('[data-form]').getBoundingClientRect(), h = document.querySelector('[data-os=toast]').getBoundingClientRect(); return h.right <= f.right - 8 && h.bottom <= f.bottom && h.left > f.left + f.width / 2; }));
      await shot(p, `desktop-toast-${tn}`);
      if (!dark) {
        await p.locator('[data-os=toast] .open').click();
        await p.waitForSelector('.callscreen'); await wait(p, 400);
        ok('desktop toast click: restores the window to the in-app incoming screen', (await n(p, '[data-os=desktop]')) === 0 && (await n(p, '[data-os=toast]')) === 0 && /Incoming call/.test(await p.locator('.callscreen .status .msg').innerText()));
        await p.getByRole('button', { name: 'Answer' }).click();
        await until(p, () => window.__pg.call.phase === 'active'); await wait(p, 300);
        await p.getByRole('button', { name: /^(Minimize call|Back to the app)/ }).click(); await wait(p, 300);
        await drawer(p, async () => { await p.getByRole('switch', { name: 'Window minimised' }).click(); });
        await p.waitForSelector('[data-os=mini]'); await wait(p, 1500);
        t = await p.locator('[data-os=mini]').innerText();
        ok('desktop mini window: name, running timer, mute, hang up', /Arjun Oommen/.test(t) && /\d\d:\d\d/.test(t) && (await n(p, '[data-os=mini] [aria-label=Mute]')) === 1 && (await n(p, '[data-os=mini] [aria-label="Hang up"]')) === 1, t);
        await shot(p, `desktop-mini-${tn}`);
        await p.locator('[data-os=mini] [aria-label=Mute]').click();
        ok('desktop mini window: Mute acts on the call', await pg(p, () => window.__pg.call.muted));
        await p.locator('[data-os=mini] .open').click();
        await p.waitForSelector('.callscreen'); await wait(p, 300);
        ok('desktop mini window: click restores the window to the call screen', await until(p, () => !document.querySelector('[data-os=desktop], [data-os=mini]'), 1500) && (await phase(p)) === 'active');
        await p.getByRole('button', { name: /^(Minimize call|Back to the app)/ }).click(); await wait(p, 300);
        await drawer(p, async () => { await p.getByRole('switch', { name: 'Window minimised' }).click(); });
        await p.waitForSelector('[data-os=mini]');
        await p.locator('[data-os=mini] [aria-label="Hang up"]').click();
        ok('desktop mini window: Hang up ends the call', await idle(p) && await until(p, () => !document.querySelector('[data-os=mini]'), 2000));
      } else {
        await p.locator('[data-os=toast]').getByRole('button', { name: 'Decline' }).click(); await idle(p);
        ok('desktop toast decline: declined record, toast gone', (await recs(p, 'arjun'))[0]?.outcome === 'declined' && (await n(p, '[data-os=toast]')) === 0);
        await pg(p, () => window.__pg.call.place('arjun')); await pg(p, () => window.__pg.call.remote('answer'));
        await until(p, () => window.__pg.call.phase === 'active'); await wait(p, 1500);
        await p.getByRole('button', { name: /^(Minimize call|Back to the app)/ }).click().catch(() => {});
        await shot(p, `desktop-mini-${tn}`);
        await pg(p, () => window.__pg.call.hangup()); await idle(p);
      }
      await p.close();
    }

    /* ===== the drawer: switches per form, Proposed notes ===== */
    p = await mk('android', { dark: false });
    await p.getByRole('button', { name: 'Simulation' }).click();
    ok('drawer (android): Phone locked, Tinline in background, Pull down notifications', (await n(p, '[role=switch][aria-label="Phone locked"]')) === 1 && (await n(p, '[role=switch][aria-label="Tinline in background"]')) === 1 && (await n(p, 'button:has-text("Pull down notifications")')) === 1);
    ok('drawer: Proposed notes for lock-screen name and per-person folding; key-remembered note', /caller's name/.test(await p.locator('.drawer').innerText()) && /fold into one per person/.test(await p.locator('.drawer').innerText()) && /Calls ring only when the account key is remembered on this device/.test(await p.locator('.drawer').innerText()));
    ok('app copy has no Proposed text', (await p.locator('[data-form] >> text=Proposed').count()) === 0);
    await p.close();

    /* ===== reduced motion ===== */
    p = await mk('android', { dark: false, reduced: true });
    await pg(p, () => window.__sys.setBackground(true)); await wait(p, 300);
    await pg(p, () => {
      window.__rm = []; const f = () => { const h = document.querySelector('[data-os=headsup]'); if (h) { const m = new DOMMatrix(getComputedStyle(h).transform === 'none' ? undefined : getComputedStyle(h).transform); window.__rm.push(m.m42); } requestAnimationFrame(f); }; f();
      window.__pg.call.receive('arjun');
    });
    await p.waitForSelector('[data-os=headsup]'); await wait(p, 500);
    ok('reduced: heads-up fades without any translate', (await pg(p, () => window.__rm)).every((y) => Math.abs(y) < .01));
    await pg(p, () => window.__pg.call.decline()); await idle(p);
    await pg(p, () => { window.__rm = []; const f = () => { const h = document.querySelector('[data-os=shade] .panel'); if (h) { const m = new DOMMatrix(getComputedStyle(h).transform === 'none' ? undefined : getComputedStyle(h).transform); window.__rm.push(m.m42); } requestAnimationFrame(f); }; f(); window.__sys.openShade(); });
    await wait(p, 500);
    ok('reduced: shade appears without sliding', (await pg(p, () => window.__rm)).length > 0 && (await pg(p, () => window.__rm)).every((y) => Math.abs(y) < .01));
    await p.close();

    /* ===== motion (full): heads-up slides a short way ===== */
    p = await mk('android', { dark: false });
    await pg(p, () => window.__sys.setBackground(true)); await wait(p, 300);
    await pg(p, () => { window.__pg.setTimeScale(4); window.__rm = []; const f = () => { const h = document.querySelector('[data-os=headsup]'); if (h) { const m = new DOMMatrix(getComputedStyle(h).transform === 'none' ? undefined : getComputedStyle(h).transform); window.__rm.push(m.m42); } requestAnimationFrame(f); }; f(); window.__pg.call.receive('arjun'); });
    await p.waitForSelector('[data-os=headsup]'); await wait(p, 1200);
    const ys = await pg(p, () => window.__rm);
    ok('motion: heads-up slides 12-16 px (not a big swoop) and settles', Math.min(...ys) <= -10 && Math.min(...ys) >= -16 && Math.abs(ys[ys.length - 1]) < .01, `min ${Math.min(...ys)}`);
    await pg(p, () => { window.__pg.setTimeScale(1); window.__pg.call.decline(); });
    await p.close();

    const errs = pages.flatMap((q) => q.errs || []);
    ok('0 console errors across all flows', errs.length === 0, errs.slice(0, 3).join(' | '));
  } catch (e) { fail++; console.log('FAIL crashed: ' + e.stack); }
  await b.close();
  console.log(`\n${pass} passed, ${fail} failed`);
  process.exit(fail ? 1 : 0);
})();

/* Group call flows against the running dev server (http://127.0.0.1:5178).
   Run: node tests/gcall.flow.js   (BASE=… to override). Prints PASS/FAIL per check, ends with the console-error count. */
import { createRequire } from 'node:module';
const L = createRequire(import.meta.url)('/home/abe/p2p_chat/.claude/worktrees/design-refinement/docs/design/refined/qa/lib.js');
const BASE = process.env.BASE || 'http://127.0.0.1:5178/';
let pass = 0, fail = 0;
const ok = (name, cond, extra = '') => { cond ? pass++ : fail++; console.log((cond ? 'PASS ' : 'FAIL ') + name + (cond ? '' : ' ' + extra)); };
const wait = (p, ms) => p.waitForTimeout(ms);
const ev = (p, fn, arg) => p.evaluate(fn, arg);
const until = async (p, fn, arg, ms = 6000) => { const t0 = Date.now(); while (Date.now() - t0 < ms) { if (await p.evaluate(fn, arg)) return true; await wait(p, 50); } return false; };

async function open(b, form, hash, dark = false) {
  const p = await L.page(b, { w: form === 'android' ? 400 : 1440, h: form === 'android' ? 860 : 900, dark });
  await p.goto(`${BASE}?form=${form}&theme=${dark ? 'dark' : 'light'}#/${hash}`);
  await p.waitForSelector('[data-form]');
  await wait(p, 500);
  await ev(p, () => {
    window.__gc.chatter = false;
    const m = window.__gcms; m.ask = 150; m.refuse = 100; m.connect = 150; m.redialCrash = 400; m.redialPlanned = 250; m.redialRotate = 250; m.bannerHold = 1500; m.ring = 8000;
    window.__pg.CALL_MS.connect = 150;
  });
  return p;
}
const gphase = (p) => ev(p, () => window.__gc.phase);
const active = (p) => until(p, () => window.__gc.phase === 'active');
const recs = (p, id) => ev(p, (id) => window.__pg.db.chats.find((c) => c.id === id).messages.filter((m) => m.call).map((m) => ({ ...m.call, from: m.from })), id);

(async () => {
  const b = await L.launch();
  const pages = [];
  const mk = async (...a) => { const p = await open(b, ...a); pages.push(p); return p; };
  try {
    /* ===== header button, start, roster, host line (Android) ===== */
    let p = await mk('android', 'chat/hike');
    ok('header: group call button is there and says Start group call', (await p.getByRole('button', { name: 'Start group call' }).count()) === 1);
    await ev(p, () => { window.__gcms.ask = 60000; });
    await p.getByRole('button', { name: 'Start group call' }).click();
    await p.waitForSelector('[data-gcall]');
    ok('start: screen opens, finding a call host first', (await gphase(p)) === 'starting' && /Finding a call host/.test(await p.locator('[data-gcall] .status .msg').innerText()));
    ok('start: the host line is empty while asking (row is reserved)', (await p.locator('[data-gcall] .host.on').count()) === 0);
    ok('start: Leave while still asking cancels cleanly', await (async () => { await ev(p, () => window.__gc.leave()); return until(p, () => window.__gc.phase === 'idle', null, 4000); })());
    await ev(p, () => { window.__gcms.ask = 150; window.__gc.start('hike'); });
    ok('start: connects through the call host', await active(p));
    ok('start: Arjun’s laptop carries it', (await ev(p, () => window.__gc.current.hostId)) === 'arjun');
    ok('screen: host line names the call host', /Call host: Arjun’s laptop/.test(await p.locator('[data-gcall] .host').innerText()));
    ok('screen: field is WebGL, one canvas', (await p.locator('[data-gcall] canvas').count()) === 1);
    ok('screen: group name and a roster with me', (await p.locator('[data-gcall] .name').innerText()) === 'Weekend hike' && (await p.locator('[data-gcall] [data-part="me"]').count()) === 1);
    ok('screen: nobody else yet reads "Only you so far"', /Only you so far/.test(await p.locator('[data-gcall] .cnt').innerText()));
    ok('notice: "You started a call" lands in the thread', await ev(p, () => window.__pg.db.chats.find((c) => c.id === 'hike').messages.some((m) => m.system && /You started a call/.test(m.text))));
    ok('Android: Speaker control is there', (await p.getByRole('button', { name: 'Speaker' }).count()) === 1);

    /* ---- join, speaking, K loudest ---- */
    await ev(p, () => { for (const id of ['sam', 'meera', 'priya', 'leo', 'kavya']) window.__gc.remoteJoin('hike', id); });
    await wait(p, 400);
    ok('roster: six tiles, "6 people in the call"', (await p.locator('[data-gcall] .tile').count()) === 6 && /6 people in the call/.test(await p.locator('[data-gcall] .cnt').innerText()));
    await ev(p, () => { for (const id of ['sam', 'meera', 'priya', 'leo', 'kavya']) window.__gc.setSpeaking(id, true); });
    await wait(p, 900);
    const sp = await ev(p, () => window.__gc.loud.filter((x) => x !== 'me'));
    ok('speaking: a computer host forwards every speaker (Q2)', sp.length === 5, JSON.stringify(sp));
    ok('speaking: exactly those tiles are lit', (await p.locator('[data-gcall] .tile.speaking').count()) === 5);
    ok('speaking: a speaking line names them', /speaking/.test(await p.locator('[data-gcall] .spk').innerText()));
    await ev(p, () => { for (const id of ['sam', 'meera', 'priya', 'leo', 'kavya']) window.__gc.setSpeaking(id, false); });
    ok('speaking: quiet again, nobody lit', await until(p, () => document.querySelectorAll('[data-gcall] .tile.speaking').length === 0, null, 4000));

    /* ---- mute ---- */
    await p.getByRole('button', { name: 'Mute', exact: true }).click();
    ok('mute: my mic is muted, my tile shows it', (await ev(p, () => window.__gc.muted)) && (await p.locator('[data-gcall] [data-part="me"] .mb').count()) === 1);
    await ev(p, () => window.__gc.remoteMute('sam', true));
    await wait(p, 150);
    ok('mute: a muted participant shows the mic-off badge', (await p.locator('[data-gcall] [data-part="sam"] .mb').count()) === 1);
    await p.getByRole('button', { name: 'Muted', exact: true }).click().catch(() => {});

    /* ---- info ---- */
    await p.getByRole('button', { name: /About the call host/ }).first().click();
    ok('info: the card says the call host can hear the call (U25)', /can hear this call/.test(await p.locator('[data-gcall] .info').innerText()));
    await p.keyboard.press('Escape');
    ok('info: Escape closes it and the call stays', await until(p, () => !document.querySelector('[data-gcall] .info'), null, 4000) && (await gphase(p)) === 'active');

    /* ---- minimise and restore ---- */
    await p.getByRole('button', { name: /^(Minimize call|Back to the app)/ }).click();
    await p.waitForSelector('[data-gstrip]');
    ok('minimise: the strip shows the group and a timer', /Weekend hike/.test(await p.locator('[data-gstrip]').innerText()) && !(await p.locator('[data-gcall]').count()));
    ok('minimise: the strip carries the call, so the join banner is gone', (await p.locator('[data-gbanner]').count()) === 0);
    await p.locator('[data-gstrip] .open').click();
    await p.waitForSelector('[data-gcall]');
    ok('restore: the screen is back', (await gphase(p)) === 'active');

    /* ---- the ongoing notification opens the group call screen ---- */
    await ev(p, () => window.__sys.setBackground(true));
    ok('away: the group call shows as an ongoing notification, not a screen', (await ev(p, () => window.__sys.gOngoing)) && await until(p, () => !document.querySelector('[data-gcall]'), null, 4000));
    await ev(p, () => window.__sys.openCall());
    ok('away: tapping it opens the group call screen', await until(p, () => !!document.querySelector('[data-gcall]'), null, 3000));

    /* ---- leave and the record ---- */
    await p.locator('[data-gcall]').getByRole('button', { name: 'Leave call' }).click();
    ok('leave: screen goes', await until(p, () => !document.querySelector('[data-gcall]'), null, 3000));
    ok('leave: back to idle', await until(p, () => window.__gc.phase === 'idle', null, 3000));
    const r = (await recs(p, 'hike')).filter((x) => x.from === 'me');   // the seed holds two earlier group calls from others
    ok('record: one "Group call" record, from me', r.length === 1 && r[0].group === true && r[0].direction === 'out', JSON.stringify(r));
    ok('record: the pill reads "Group call · …"', /Group call · \d+ s|Group call · \d+ min/.test(await p.locator('.callrec .pill').first().innerText()));
    ok('call goes on without me (others still in)', await ev(p, () => !!window.__gc.liveFor('hike') && !window.__gc.liveFor('hike').parts.includes('me')));
    ok('banner: Join is offered again', (await p.locator('[data-gbanner] button', { hasText: 'Join' }).count()) === 1);

    /* ---- join from the banner ---- */
    await p.locator('[data-gbanner] button', { hasText: 'Join' }).click();
    await p.waitForSelector('[data-gcall]');
    ok('join: from the banner, connects', await active(p));
    ok('join: I am in the roster again', await ev(p, () => window.__gc.current.parts.includes('me')));

    /* ---- 1:1 call arriving during the group call is turned away Busy ---- */
    const got = await ev(p, () => window.__pg.call.receive('arjun'));
    ok('busy: a 1:1 call during a group call is turned away', got === false && (await ev(p, () => window.__pg.call.phase)) === 'idle');
    ok('busy: it is left as a missed call in Arjun’s chat', (await recs(p, 'arjun')).some((c) => c.outcome === 'missed' && c.direction === 'in'));
    ok('busy: placing a 1:1 call is refused', (await ev(p, () => window.__pg.call.place('arjun'))) === false);
    await ev(p, () => window.__gc.leave());
    await until(p, () => window.__gc.phase === 'idle');
    await ev(p, () => { for (const id of ['sam','meera','priya','leo','kavya']) window.__gc.remoteLeave('hike', id); });
    await wait(p, 200);
    ok('last person leaves: the call is over for everyone', await ev(p, () => !window.__gc.liveFor('hike')));

    /* ---- InCall the other way round ---- */
    await ev(p, () => { window.__pg.call.place('arjun'); });
    ok('incall: 1:1 outgoing is up', (await ev(p, () => window.__pg.call.phase)) === 'outgoing');
    ok('incall: starting a group call is refused with a note', (await ev(p, () => window.__gc.start('hike'))) === false && /on a call/i.test(await ev(p, () => window.__gc.toast?.text ?? '')));
    await ev(p, () => window.__pg.call.hangup());
    await until(p, () => window.__pg.call.phase === 'idle');

    /* ---- host lost: no backup -> "Call ended — host lost" + Rejoin ---- */
    await ev(p, () => window.__gc.start('hike'));
    await active(p);
    await ev(p, () => window.__gc.hostEvent('hike', 'lost'));
    ok('host lost: reconnecting first', await until(p, () => window.__gc.phase === 'reconnecting', null, 1500) && /Reconnecting/.test(await p.locator('[data-gcall] .status .msg').innerText()));
    ok('host lost: no backup answers, call ended — host lost', await until(p, () => window.__gc.phase === 'ended' && window.__gc.endReason === 'host-lost'));
    ok('host lost: the screen stays with Rejoin and Close', (await p.getByRole('button', { name: 'Rejoin' }).count()) === 1 && (await p.getByRole('button', { name: 'Close' }).count()) === 1);
    ok('host lost: the world call is gone', await ev(p, () => !window.__gc.liveFor('hike')));
    await p.getByRole('button', { name: 'Rejoin' }).click();
    ok('host lost: Rejoin starts again through a call host', await active(p));
    await ev(p, () => window.__gc.leave());
    await until(p, () => window.__gc.phase === 'idle');

    /* ---- host lost with a backup: redial and go on ---- */
    await ev(p, () => { const g = window.__pg.db.chats.find((c) => c.id === 'hike'); const s = g.members.find((m) => m.id === 'sam'); s.callHost = true; window.__gc.conds.sam.class = 'desktop'; window.__gc.conds.sam.device = 'desktop'; });
    await ev(p, () => window.__gc.start('hike'));
    await active(p);
    ok('backup: Sam’s computer is the next in line', (await ev(p, () => window.__gc.backups('hike'))).includes('sam'));
    await ev(p, () => window.__gc.hostEvent('hike', 'lost'));
    ok('backup: host lost -> reconnecting -> back on the backup', await until(p, () => window.__gc.phase === 'reconnecting', null, 1500) && await until(p, () => window.__gc.phase === 'active' && window.__gc.current.hostId === 'sam', null, 3000));
    ok('backup: the host line follows', /Sam’s desktop/.test(await p.locator('[data-gcall] .host').innerText()));
    await ev(p, () => window.__gc.hostEvent('hike', 'rotate'));
    ok('rotate: a host rotation redials the same host', await until(p, () => window.__gc.phase === 'reconnecting', null, 1500) && await until(p, () => window.__gc.phase === 'active' && window.__gc.current.hostId === 'sam', null, 3000));
    await ev(p, () => window.__gc.leave());
    await until(p, () => window.__gc.phase === 'idle');
    await ev(p, () => { const g = window.__pg.db.chats.find((c) => c.id === 'hike'); g.members.find((m) => m.id === 'sam').callHost = false; });

    /* ---- refusal chain: Arjun refuses and nobody else can host -> No call host available ---- */
    await ev(p, () => { window.__gc.conds.arjun.refuses = true; });
    await ev(p, () => window.__gc.start('hike'));
    ok('refusal: no call host available', await until(p, () => window.__gc.phase === 'failed' && window.__gc.endReason === 'no-host') && /No call host available/.test(await p.locator('[data-gcall] .status .msg').innerText()));
    await until(p, () => window.__gc.phase === 'idle', null, 8000);
    await ev(p, () => { window.__gc.conds.arjun.refuses = false; });

    /* ---- someone else starts a call: banner, list pill, notice, join cap ---- */
    await ev(p, () => window.__gc.remoteStart('hike', 'sam'));
    ok('started: Call started by Sam is in the thread', await ev(p, () => window.__pg.db.chats.find((c) => c.id === 'hike').messages.some((m) => m.system && /Call started by Sam/.test(m.text))));
    ok('started: the conversation banner shows, 1 joined', /Call in progress/.test(await p.locator('[data-gbanner]').innerText()) && /1 joined/.test(await p.locator('[data-gbanner]').innerText()));
    ok('started: a group call rings (ring cue on, 1:1 phase stays idle)', (await ev(p, () => window.__pg.call.phase)) === 'idle' && (await ev(p, () => window.__gc.announce?.ringing === true && window.__pg.fxLog.some(({ e }) => e.type === 'incoming' && e.on))));
    ok('ring: the card says Sam is calling the group, Join / Not now', /Sam is calling the group/.test(await p.locator('[data-gcall-announce]').innerText().catch(() => '')) && (await p.locator('[data-gcall-announce]').getByRole('button', { name: 'Not now' }).count()) === 1);
    await p.locator('[data-gcall-announce]').getByRole('button', { name: 'Not now' }).click();
    ok('ring: Not now stops the ring; the quiet banner stays', (await ev(p, () => window.__gc.announce?.ringing === false && window.__pg.fxLog.filter(({ e }) => e.type === 'incoming').at(-1).e.on === false)) && /Call in progress/.test(await p.locator('[data-gbanner]').innerText()));
    await ev(p, () => window.__gc.remoteLeave('hike', 'sam'));
    await ev(p, () => { window.__pg.db.chats.find((c) => c.id === 'hike').muted = true; });
    await ev(p, () => window.__gc.remoteStart('hike', 'sam'));
    ok('ring: a muted group does not ring', (await ev(p, () => window.__gc.announce?.ringing)) === false);
    await ev(p, () => { window.__gc.remoteLeave('hike', 'sam'); window.__pg.db.chats.find((c) => c.id === 'hike').muted = false; });
    await ev(p, () => { window.__gcms.ring = 800; window.__gc.remoteStart('hike', 'sam'); });
    ok('ring: ends by itself after the ring time', await until(p, () => window.__gc.announce?.ringing === false, null, 3000));
    await ev(p, () => { window.__gc.conds.arjun.class = 'phone'; window.__gc.conds.arjun.power = 'mains'; });
    for (const id of ['meera', 'priya', 'leo', 'kavya', 'dev', 'nisha', 'amma']) await ev(p, (id) => { const g = window.__pg.db.chats.find((c) => c.id === 'hike'); if (!g.members.some((m) => m.id === id)) g.members.push({ id, role: 'member', host: false, callHost: false, callHostPaused: false, since: 0 }); window.__gc.remoteJoin('hike', id); }, id);
    ok('cap: a phone host stops at 8 people', (await ev(p, () => window.__gc.liveFor('hike').parts.length)) === 8 && (await ev(p, () => window.__gc.remoteJoin('hike', 'sana'))) === false);
    await ev(p, () => window.__gc.join('hike'));
    ok('cap: joining a full call is refused with a note', /full/.test(await ev(p, () => window.__gc.toast?.text ?? '')) && (await gphase(p)) === 'idle');
    await ev(p, () => { for (const id of ['meera', 'priya', 'leo', 'kavya', 'dev', 'nisha', 'amma', 'sam']) window.__gc.remoteLeave('hike', id); });

    /* ---- Groups list shows the call, Group info has Call hosts ---- */
    await ev(p, () => window.__gc.remoteStart('hike', 'sam'));
    await p.evaluate(() => { location.hash = '#/groups'; });
    await wait(p, 600);
    ok('list: the Groups row says Call in progress', /Call in progress/.test(await p.locator('.row', { hasText: 'Weekend hike' }).innerText().catch(() => '')));
    await ev(p, () => window.__gc.remoteLeave('hike', 'sam'));

    p = await mk('desktop', 'chat/hike/info');
    await p.waitForSelector('[data-callhosts]');
    ok('group info: Call hosts section lists Arjun', (await p.locator('[data-callhost="arjun"]').count()) === 1);
    ok('group info: copy says a call host can hear the call and is separate from chat hosting', /can hear the call/.test(await p.locator('[data-callhosts]').innerText()) && /separate from chat hosting/.test(await p.locator('[data-callhosts]').innerText()));
    await p.locator('[data-member="me"]').click();
    await p.getByRole('button', { name: /Host calls on this device/ }).click();
    await p.waitForSelector('[data-callhost="me"]');
    ok('group info: appointing my device waits for it to accept', /Waiting to accept/.test(await p.locator('[data-callhost="me"]').innerText()));
    await p.locator('[data-callhost="me"]').getByRole('button', { name: 'Accept' }).click();
    await wait(p, 400);
    ok('group info: after Accept it is a call host with its own switch', (await ev(p, () => window.__pg.db.chats.find((c) => c.id === 'hike').members.find((m) => m.id === 'me').callHost)) && (await p.getByRole('switch', { name: 'Host calls on this device' }).count()) === 1);

    ok('no console errors', pages.every((x) => x.errs.length === 0), pages.map((x) => x.errs.join(' | ')).join(' || '));
  } finally {
    await b.close();
    console.log(`\n${pass} passed, ${fail} failed`);
    process.exitCode = fail ? 1 : 0;
  }
})();

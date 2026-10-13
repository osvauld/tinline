/* 1:1 video call flows against a running dev server.
   Run: BASE=http://127.0.0.1:5182/ node tests/video.flow.js   (SHOTS=/dir/ also writes screenshots, android + desktop, light + dark). */
import { createRequire } from 'node:module';
const L = createRequire(import.meta.url)('../../refined/qa/lib.js');
const BASE = process.env.BASE || 'http://127.0.0.1:5182/';
const SHOTS = process.env.SHOTS || '';
let pass = 0, fail = 0;
const ok = (name, cond, extra = '') => { cond ? pass++ : fail++; console.log((cond ? 'PASS ' : 'FAIL ') + name + (cond ? '' : ' ' + extra)); };
const wait = (p, ms) => p.waitForTimeout(ms);
const until = async (p, fn, ms = 4000) => { const t0 = Date.now(); while (Date.now() - t0 < ms) { if (await p.evaluate(fn)) return true; await wait(p, 50); } return false; };
const shot = (p, n) => SHOTS ? p.screenshot({ path: SHOTS + n + '.png' }) : null;
const V = (p, fn, a) => p.evaluate(fn, a);

async function open(b, form, hash, { dark = false } = {}) {
  const p = await L.page(b, { w: form === 'android' ? 390 : 1440, h: form === 'android' ? 844 : 900, dark });
  await p.goto(`${BASE}?form=${form}&theme=${dark ? 'dark' : 'light'}#/${hash}`);
  await p.waitForSelector('[data-form]');
  await wait(p, 500);
  await V(p, () => {
    for (const c of window.__pg.db.chats) { const n = c.messages.filter((m) => m.call && m.from !== 'me').length; c.messages = c.messages.filter((m) => !m.call); c.unread = Math.max(0, c.unread - n); }
    window.__pg.CALL_MS.ringingAfter = 8000; window.__pg.CALL_MS.connect = 300; window.__pgVid.peerDelay = 300; window.__pgVid.hideMs = 700;
  });
  return p;
}
const rect = (p, sel) => p.locator(sel).first().evaluate((e) => { const r = e.getBoundingClientRect(); return [Math.round(r.x * 10) / 10, Math.round(r.y * 10) / 10, Math.round(r.width * 10) / 10, Math.round(r.height * 10) / 10].join(','); }).catch(() => 'none');
const connect = async (p) => { await V(p, () => window.__pg.call.remote('answer')); await until(p, () => window.__pg.call.phase === 'active', 3000); };
const chat = (p, id) => V(p, (id) => window.__pg.db.chats.find((c) => c.id === id).messages.filter((m) => m.call).map((m) => ({ ...m.call, from: m.from })), id);
const vidState = (p) => V(p, () => { const v = window.__pgVid; return { camOn: v.camOn, facing: v.facing, peerCam: v.peerCam, lowBw: v.lowBw, notice: v.notice, chrome: v.chrome, intent: v.intent, corner: v.corner }; });
const idle = (p) => until(p, () => window.__pg.call.phase === 'idle', 4000);

async function scenario(b, form, dark, assert) {
  const tag = `${form}-${dark ? 'dark' : 'light'}`;
  const p = await open(b, form, 'chat/arjun', { dark });
  const T = (n, c, x) => assert && ok(`${tag}: ${n}`, c, x);

  /* --- start from the header video button --- */
  await p.getByRole('button', { name: 'Video call', exact: true }).click();
  await p.waitForSelector('.callscreen');
  T('video button opens the call screen as a video call', (await V(p, () => window.__pgVid.intent)) === true && /Video call with/.test(await p.locator('.callscreen').getAttribute('aria-label')));
  T('camera is off while calling', (await vidState(p)).camOn === false);
  await shot(p, `${tag}-1-calling`);
  await connect(p);
  await until(p, () => window.__pgVid.camOn, 2000);
  T('connected: my camera turned on (chosen by pressing the video button), front camera', (await vidState(p)).camOn && (await vidState(p)).facing === 'front');
  await until(p, () => window.__pgVid.peerCam, 2000);
  await wait(p, 600);
  T('their picture fills the screen once their camera is on', (await p.locator('[data-stage]').count()) === 1);
  T('self-view is mirrored (front camera)', await p.locator('[data-selfview] .mir').evaluate((e) => new DOMMatrix(getComputedStyle(e).transform).a < 0));
  T('self-view starts top-right', (await vidState(p)).corner === 'tr');
  for (const n of ['Mute', 'Camera', 'End call']) T(`control "${n}" present`, (await p.getByRole('button', { name: n, exact: true }).count()) === 1);
  if (form === 'android') { T('phone has Flip and Speaker', (await p.getByRole('button', { name: 'Flip camera' }).count()) === 1 && (await p.getByRole('button', { name: 'Speaker', exact: true }).count()) === 1); }
  else T('desktop has no Flip and no Speaker', (await p.getByRole('button', { name: 'Flip camera' }).count()) === 0 && (await p.getByRole('button', { name: 'Speaker', exact: true }).count()) === 0);
  await V(p, () => { window.__pgVid.hideMs = 60000; window.__pgVid.poke(); });
  await p.mouse.move(5, 5);
  await shot(p, `${tag}-2-video-controls`);

  /* --- controls auto-hide during video, never during voice; layout is stable --- */
  await V(p, () => { window.__pgVid.hideMs = 700; window.__pgVid.poke(); });
  const endBox = await rect(p, '[aria-label="End call"]');
  await p.mouse.move(5, 5);
  await wait(p, 1300);
  T('controls hide after idle seconds over their video', await p.locator('.callscreen').evaluate((e) => e.classList.contains('idle')) && (await p.locator('.controls').evaluate((e) => +getComputedStyle(e).opacity)) < 0.05);
  T('hidden controls do not move', (await rect(p, '[aria-label="End call"]')) === endBox);
  await shot(p, `${tag}-3-controls-hidden`);
  await V(p, () => { window.__pgVid.hideMs = 5000; });
  await p.locator('[data-stage]').click({ position: { x: 20, y: 300 } });
  await wait(p, 400);
  T('a click on the picture shows them again', !(await p.locator('.callscreen').evaluate((e) => e.classList.contains('idle'))));
  await V(p, () => { window.__pgVid.hideMs = 60000; window.__pgVid.poke(); });

  /* --- self-view drag + snap --- */
  const pip = p.locator('[data-selfview]');
  const bb = await pip.boundingBox(), host = await p.locator('.callscreen').boundingBox();
  await p.mouse.move(bb.x + bb.width / 2, bb.y + bb.height / 2);
  await p.mouse.down();
  await p.mouse.move(host.x + 60, host.y + host.height - 260, { steps: 8 });
  await shot(p, `${tag}-4-dragging`);
  await p.mouse.up();
  await wait(p, 500);
  T('dropping the self-view near a corner snaps it there (bottom-left)', (await vidState(p)).corner === 'bl');
  const nb = await pip.boundingBox();
  T('snapped tile sits inside the screen, above the controls', nb.x >= host.x && nb.y + nb.height < (await p.locator('.controls').boundingBox()).y + 4);
  await pip.focus(); await p.keyboard.press('ArrowUp'); await wait(p, 400);
  T('arrow keys move it between corners', (await vidState(p)).corner === 'tl');
  await p.keyboard.press('ArrowRight'); await wait(p, 400);

  /* --- flip (phones) --- */
  if (form === 'android') {
    await p.getByRole('button', { name: 'Flip camera' }).click(); await wait(p, 300);
    T('flip switches to the back camera', (await vidState(p)).facing === 'back');
    T('back camera is not mirrored', await p.locator('[data-selfview] .mir').evaluate((e) => new DOMMatrix(getComputedStyle(e).transform).a > 0));
    await shot(p, `${tag}-5-back-camera`);
    await p.getByRole('button', { name: 'Flip camera' }).click(); await wait(p, 300);
    T('flip again returns to the front camera', (await vidState(p)).facing === 'front');
  }

  /* --- low bandwidth --- */
  await V(p, () => window.__pgVid.setLowBw(true)); await wait(p, 400);
  T('low bandwidth: "Video paused, audio continues" is shown, audio call goes on', (await p.locator('[data-chip="paused"]').count()) === 1 && /Video paused, audio continues/.test(await p.locator('[data-chip] .pill').innerText()) && (await V(p, () => window.__pg.call.phase)) === 'active');
  T('low bandwidth: their picture is frozen and softened', await p.locator('[data-stage]').evaluate((e) => e.classList.contains('frozen')));
  await shot(p, `${tag}-6-low-bandwidth`);
  await V(p, () => window.__pgVid.setLowBw(false)); await wait(p, 300);

  /* --- reconnecting --- */
  await V(p, () => window.__pg.call.drop()); await wait(p, 400);
  T('reconnecting: header says Reconnecting…, their picture is frozen', /Reconnecting/.test(await p.locator('.vt').innerText()) && await p.locator('[data-stage]').evaluate((e) => e.classList.contains('frozen')));
  await shot(p, `${tag}-7-reconnecting`);
  await V(p, () => window.__pg.call.recover()); await wait(p, 300);

  /* --- peer camera off: voice look with their avatar; the chip swaps without moving anything --- */
  await V(p, () => window.__pgVid.setPeerCam(false)); await until(p, () => !document.querySelector('[data-stage]'), 2000); await wait(p, 400);
  T('their camera off: picture leaves, avatar on the cloud field returns', (await p.locator('[data-stage]').count()) === 0 && (await p.locator('.who').evaluate((e) => +getComputedStyle(e).opacity)) > 0.95);
  T('their camera off: calm note "<Name>’s camera is off"', (await p.locator('[data-chip="off"]').count()) === 1 && /Arjun.s camera is off/.test(await p.locator('[data-chip] .pill').innerText()));
  T('my self-view stays while theirs is off', (await p.locator('[data-selfview]').count()) === 1);
  const nm1 = await rect(p, '.callscreen .name'), av1 = await rect(p, '.callscreen .av'), pill1 = await rect(p, '[data-chip] .pill');
  await V(p, () => { window.__pgVid.setPeerCam(true); }); await wait(p, 200);
  await V(p, () => { window.__pgVid.setPeerCam(false); window.__pgVid.setLowBw(true); }); await wait(p, 500);
  T('text swaps in the note move nothing (name, avatar and pill box)', (await rect(p, '.callscreen .name')) === nm1 && (await rect(p, '.callscreen .av')) === av1 && (await rect(p, '[data-chip] .pill')) === pill1);
  await V(p, () => window.__pgVid.setLowBw(false));
  await shot(p, `${tag}-8-peer-camera-off`);
  T('controls do not auto-hide when their video is not on screen', await (async () => { await V(p, () => { window.__pgVid.hideMs = 400; window.__pgVid.poke(); }); await wait(p, 900); return !(await p.locator('.callscreen').evaluate((e) => e.classList.contains('idle'))); })());

  /* --- camera toggle: off, then permission denied --- */
  await p.getByRole('button', { name: 'Camera', exact: true }).click(); await wait(p, 400);
  T('camera button turns my camera off, self-view leaves', (await vidState(p)).camOn === false && (await p.locator('[data-selfview]').count()) === 0);
  await V(p, () => { window.__pgVid.perm = 'denied'; });
  await p.getByRole('button', { name: 'Camera', exact: true }).click(); await wait(p, 500);
  T('permission denied: camera stays off, calm note with the Settings hint', (await vidState(p)).camOn === false && (await p.locator('[data-vnotice="denied"]').count()) === 1 && /Settings|privacy settings/.test(await p.locator('[data-vnotice]').innerText()) && /call goes on/.test(await p.locator('[data-vnotice]').innerText()));
  await shot(p, `${tag}-9-permission-denied`);
  await wait(p, 500); { const r = await p.locator('[data-vnotice] .ok').boundingBox(); await p.mouse.click(r.x + r.width / 2, r.y + r.height / 2); } await wait(p, 400);
  await until(p, () => !document.querySelector('[data-vnotice]'), 2000);
  T('the note can be dismissed', (await p.locator('[data-vnotice]').count()) === 0, await p.evaluate(() => { const o = document.querySelector('[data-vnotice] .ok'); if (!o) return 'gone'; const r = o.getBoundingClientRect(), e = document.elementFromPoint(r.x + r.width / 2, r.y + r.height / 2); return JSON.stringify([r.x, r.y]) + e.tagName + '.' + e.className + ' p=' + getComputedStyle(o).pointerEvents; }));
  await V(p, () => { window.__pgVid.perm = 'granted'; window.__pgVid.hasCamera = false; });
  if (form === 'desktop') {
    await p.getByRole('button', { name: 'Camera', exact: true }).click(); await wait(p, 500);
    T('desktop with no camera: "No camera found", call goes on', (await p.locator('[data-vnotice="nocamera"]').count()) === 1 && (await V(p, () => window.__pg.call.phase)) === 'active');
    await shot(p, `${tag}-10-no-camera`);
    await wait(p, 500); { const r = await p.locator('[data-vnotice] .ok').boundingBox(); await p.mouse.click(r.x + r.width / 2, r.y + r.height / 2); } await wait(p, 400);
  }
  await V(p, () => { window.__pgVid.hasCamera = true; });
  await p.getByRole('button', { name: 'Camera', exact: true }).click(); await wait(p, 400);
  T('voice -> video: camera button turns the camera back on', (await vidState(p)).camOn === true);

  /* --- minimise --- */
  await V(p, () => { window.__pgVid.hideMs = 60000; window.__pgVid.setPeerCam(true); }); await wait(p, 500);
  await p.getByRole('button', { name: 'Minimize call' }).click(); await wait(p, 700);
  if (form === 'android') {
    T('android minimised: floating video tile plus the strip', (await p.locator('[data-minivideo]').count()) === 1 && (await p.locator('[data-callstrip]').count()) === 1);
    await shot(p, `${tag}-11-minimised`);
    await p.locator('[data-minivideo]').click(); await wait(p, 600);
    T('tapping the tile returns to the call', (await p.locator('.callscreen').count()) === 1);
  } else {
    T('desktop minimised: only the strip (video icon), no floating tile', (await p.locator('[data-minivideo]').count()) === 0 && (await p.locator('[data-callstrip]').count()) === 1);
    await shot(p, `${tag}-11-minimised`);
    await p.locator('[data-callstrip] .open').click(); await wait(p, 600);
  }

  /* --- end + record --- */
  await wait(p, 1100);
  await p.getByRole('button', { name: 'End call' }).click(); await idle(p); await wait(p, 300);
  const recs = await chat(p, 'arjun');
  T('one record, kind video, answered, outgoing', recs.length === 1 && recs[0].kind === 'video' && recs[0].outcome === 'answered' && recs[0].direction === 'out', JSON.stringify(recs));
  T('camera released after the call', (await vidState(p)).camOn === false);
  T('thread shows "Video call · m:ss"', /Video call · \d+:\d\d/.test(await p.locator('.callrec').last().innerText()), await p.locator('.callrec').last().innerText().catch(() => ''));
  await shot(p, `${tag}-12-record`);
  return p;
}

(async () => {
  const b = await L.launch();
  const pages = [];
  try {
    if (SHOTS) {
      for (const [f, d] of [['android', false], ['android', true], ['desktop', false], ['desktop', true]]) pages.push(await scenario(b, f, d, f === 'android' && !d || f === 'desktop' && !d));
    } else {
      pages.push(await scenario(b, 'android', false, true));
      pages.push(await scenario(b, 'desktop', false, true));
    }

    /* ===== incoming video call: label, answer with camera off / on ===== */
    let p = await open(b, 'android', 'chat/arjun'); pages.push(p);
    await V(p, () => window.__pg.call.receive('sana', true));
    await wait(p, 500);
    ok('incoming video: banner says "Video call"', /Video call/.test(await p.locator('[data-incoming-alert]').innerText()));
    await shot(p, 'android-light-13-incoming-banner');
    await p.locator('[data-incoming-alert] .open').click(); await wait(p, 500);
    ok('incoming video: call screen says "Incoming video call"', /Incoming video call/.test(await p.locator('.callscreen .status').innerText()));
    ok('incoming video: offers Answer (camera off) and With camera', (await p.getByRole('button', { name: 'Answer', exact: true }).count()) === 1 && (await p.getByRole('button', { name: 'Answer with camera' }).count()) === 1);
    await shot(p, 'android-light-14-incoming-screen');
    await p.getByRole('button', { name: 'Answer', exact: true }).click(); await until(p, () => window.__pg.call.phase === 'active'); await wait(p, 500);
    ok('answered plain: camera stays off', (await vidState(p)).camOn === false);
    await V(p, () => window.__pg.call.hangup()); await idle(p);
    await V(p, () => window.__pg.call.receive('sana', true)); await wait(p, 300);
    await p.locator('[data-incoming-alert] .open').click(); await wait(p, 400);
    await p.getByRole('button', { name: 'Answer with camera' }).click(); await until(p, () => window.__pg.call.phase === 'active'); await wait(p, 500);
    ok('answered with camera: camera is on', (await vidState(p)).camOn === true);
    await V(p, () => window.__pg.call.hangup()); await idle(p);
    const r = await chat(p, 'sana');
    ok('records: both answered video calls are kind video', r.length === 2 && r.every((x) => x.kind === 'video'), JSON.stringify(r));
    await p.close();

    /* ===== plain voice call stays a voice call ===== */
    p = await open(b, 'android', 'chat/arjun'); pages.push(p);
    await p.getByRole('button', { name: 'Call', exact: true }).click(); await p.waitForSelector('.callscreen'); await connect(p); await wait(p, 400);
    ok('voice: camera off, no self-view, no stage, no auto-hide', (await vidState(p)).camOn === false && (await p.locator('[data-selfview], [data-stage]').count()) === 0);
    await V(p, () => { window.__pgVid.hideMs = 300; window.__pgVid.poke(); }); await wait(p, 900);
    ok('voice: controls never hide', !(await p.locator('.callscreen').evaluate((e) => e.classList.contains('idle'))));
    ok('voice: no camera note', (await p.locator('[data-chip="off"]').count()) === 0);
    await V(p, () => window.__pg.call.hangup()); await idle(p);
    const rv = await chat(p, 'arjun');
    ok('voice: recorded as voice', rv.length === 1 && rv[0].kind === 'voice');
    await p.close();

    /* ===== calling back from a record: video record -> video call, voice record -> voice call ===== */
    p = await open(b, 'android', 'chat/arjun'); pages.push(p);
    await p.getByRole('button', { name: 'Video call', exact: true }).click(); await p.waitForSelector('.callscreen'); await connect(p); await wait(p, 300);
    await V(p, () => window.__pg.call.hangup()); await idle(p); await wait(p, 400);
    const pill = p.getByRole('button', { name: /Video call Arjun back/ });
    ok('call-back: a video record offers "Video call Arjun back"', (await pill.count()) === 1);
    await pill.click(); await p.waitForSelector('.callscreen');
    ok('call-back: pressing a video record starts a video call', (await V(p, () => window.__pgVid.intent)) === true && /Video call with/.test(await p.locator('.callscreen').getAttribute('aria-label')));
    await V(p, () => window.__pg.call.hangup()); await idle(p);
    await p.getByRole('button', { name: 'Call', exact: true }).click(); await p.waitForSelector('.callscreen'); await connect(p);
    await V(p, () => window.__pg.call.hangup()); await idle(p); await wait(p, 400);
    const vp = p.getByRole('button', { name: /Voice call.*Call Arjun back/ });
    await vp.last().click(); await p.waitForSelector('.callscreen');
    ok('call-back: pressing a voice record starts a voice call', (await V(p, () => window.__pgVid.intent)) === false && /Voice call with/.test(await p.locator('.callscreen').getAttribute('aria-label')));
    await V(p, () => window.__pg.call.hangup()); await idle(p);
    await p.close();

    const errs = pages.flatMap((q) => q.errs || []);
    ok('0 console errors across all flows', errs.length === 0, errs.slice(0, 3).join(' | '));
  } catch (e) { fail++; console.log('FAIL crashed: ' + e.stack); }
  await b.close();
  console.log(`\n${pass} passed, ${fail} failed`);
  process.exit(fail ? 1 : 0);
})();

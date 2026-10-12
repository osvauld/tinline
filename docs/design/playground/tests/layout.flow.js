/* Layout stability: "nothing in the UI moves when some text is added". Against the running dev server (http://127.0.0.1:5178).
   Every check changes a piece of text / a caption / a counter / a badge / a status and asserts that NOTHING ELSE moved:
   - Chromium's Layout Instability API (tests/layout.js watchShifts): CLS-style sum for the step must stay under a tiny epsilon;
   - a whole-DOM geometry diff (tests/layout.js movers): no element is pushed (top-left changed) except the ones listed in `ignore`;
   - anchors sampled every frame (tests/layout.js stable) for the call screen and the waiting card.
   Intentional layout changes (a new message at the bottom of a chat, opening a sheet or an edit form, a role change) are not asserted here.
   Run: node tests/layout.flow.js   (BASE=… to override; ONLY=chat,calls,… to run some sections). Prints PASS/FAIL per check. */
import { createRequire } from 'node:module';
import * as LY from './layout.js';
const L = createRequire(import.meta.url)('/home/abe/p2p_chat/.claude/worktrees/design-refinement/docs/design/refined/qa/lib.js');
const BASE = process.env.BASE || 'http://127.0.0.1:5178/';
const ONLY = (process.env.ONLY || '').split(',').filter(Boolean);
const want = (s) => !ONLY.length || ONLY.includes(s);
let pass = 0, fail = 0;
const ok = (name, cond, extra = '') => { cond ? pass++ : fail++; console.log((cond ? 'PASS ' : 'FAIL ') + name + (cond ? '' : ' ' + extra)); };
const wait = (p, ms) => p.waitForTimeout(ms);
const EPS = 0.002;   // CLS epsilon: text re-centring inside its own box counts a few 1e-4; a pushed sibling counts 1e-2 and up

async function open(b, form, hash, { dark = false, reduced = false } = {}) {
  const p = await L.page(b, { w: form === 'android' ? 600 : 1440, h: 900, dark });
  if (reduced) await p.emulateMedia({ reducedMotion: 'reduce' });
  await LY.watchShifts(p);
  await p.goto(`${BASE}?form=${form}&theme=${dark ? 'dark' : 'light'}#/${hash}`);
  await p.waitForSelector('[data-form]');
  await wait(p, 700);
  await p.evaluate(() => { const C = window.__pg.CALL_MS; C.ringingAfter = 900; C.connect = 600; C.failedHold = 1200; C.remoteEndHold = 900; C.waitingTimeout = 60000; });
  return p;
}
const pg = (p, fn, arg) => p.evaluate(fn, arg);

/* Run `action`, then assert: no element pushed (movers) and CLS under EPS. `ignore` = selector of elements allowed to change themselves. */
async function probe(p, name, action, { ignore = '', settle = 700, max = EPS, root, seen } = {}) {
  await LY.shifts(p);
  let saw = true;   // `seen(p)`: the state under test really appeared (a check that moves nothing because nothing happened is worthless)
  const act = seen ? async () => { await action(); await wait(p, 150); saw = !!(await seen(p)); } : action;
  const mv = await LY.movers(p, act, { ignore, settle, ...(root ? { root } : {}) });
  const sh = await LY.shifts(p);
  const c = LY.cls(sh);
  ok(name, saw && mv.length === 0 && c <= max, `${saw ? '' : 'THE STATE UNDER TEST NEVER APPEARED | '}movers: ${LY.fmtMovers(mv)} | cls ${c.toFixed(4)}\n${LY.fmtShifts(sh).slice(0, 500)}`);
}
/* Per-frame anchor check. */
async function anchors(p, name, sels, action, o = {}) {
  await LY.shifts(p);
  const r = await LY.stable(p, sels, action, { centered: ['.name', '.status', '.e2e', 'span', '.act', '.when'], ...o });
  const c = LY.cls(await LY.shifts(p));
  ok(name, r.moved.length === 0 && c <= (o.max ?? EPS), `moved: ${JSON.stringify(r.moved)} | cls ${c.toFixed(4)}`);
}
const drawerOpen = async (p) => { await p.getByRole('button', { name: 'Simulation', exact: true }).click(); await wait(p, 450); };
const drawerClose = async (p) => { await p.getByRole('button', { name: 'Close simulation' }).click(); await wait(p, 350); };
const TA = 'textarea[aria-label^="Message to"]';
const sendKey = (form) => (form === 'desktop' ? 'Enter' : 'Control+Enter');
const until = async (p, fn, ms = 4000, arg) => { const t0 = Date.now(); while (Date.now() - t0 < ms) { if (await p.evaluate(fn, arg)) return true; await wait(p, 50); } return false; };

/* ------------------------------------------------------------------ chat */
async function chatSection(b, form) {
  const t = (s) => `${form} chat: ${s}`;
  let p = await open(b, form, 'chat/arjun');
  await probe(p, t('offline strip appears: no message moves'), () => pg(p, () => { window.__pg.db.env.networkOnline = false; }), { ignore: '.strip, .strip *, .pr', seen: (q) => q.evaluate(() => !!document.querySelector('.strip')) });
  await probe(p, t('offline strip leaves: no message moves'), () => pg(p, () => { window.__pg.db.env.networkOnline = true; }), { ignore: '.strip, .strip *, .pr', seen: (q) => q.evaluate(() => !document.querySelector('.strip')) });
  await probe(p, t('peer goes offline (strip + header status): nothing moves'), () => pg(p, () => { window.__pg.db.people.find((x) => x.id === 'arjun').online = false; }), { ignore: '.strip, .strip *, .pr', seen: (q) => q.evaluate(() => /offline/i.test(document.querySelector('.strip')?.innerText ?? '')) });
  await probe(p, t('peer back online: nothing moves'), () => pg(p, () => { window.__pg.db.people.find((x) => x.id === 'arjun').online = true; }), { ignore: '.strip, .strip *, .pr', seen: (q) => q.evaluate(() => !document.querySelector('.strip')) });
  await anchors(p, t('typing 1 -> 3 lines: header does not move'), ['.bar', '.head', '.head .tx', '.ib'], async () => { await p.fill(TA, 'one\ntwo\nthree'); }, { max: 1 });   // the list moving up with the growing composer is intended
  await anchors(p, t('clearing the draft: header does not move'), ['.bar', '.head', '.head .tx', '.ib'], async () => { await p.fill(TA, ''); }, { max: 1 });
  // sending -> delivered: the new bubble is the intended change; the tick/label swap that follows must move nothing
  await p.fill(TA, 'layout check'); await p.keyboard.press(sendKey(form)); await wait(p, 300);
  await probe(p, t('Sending -> Delivered tick: nothing moves'), () => wait(p, 1000), { seen: (q) => q.evaluate(() => /Delivered/.test([...document.querySelectorAll('li.m.me .vh')].at(-1)?.textContent ?? '')) });
  // edited marker (own text message)
  const mid = await pg(p, () => { const c = window.__pg.db.chats.find((x) => x.id === 'arjun'); return c.messages.filter((m) => m.from === 'me' && !m.call && m.text).at(-1).id; });
  await probe(p, t('"edited" marker: nothing outside the bubble moves'), () => pg(p, (mid) => { const m = window.__pg.db.chats.find((x) => x.id === 'arjun').messages.find((x) => x.id === mid); m.edited = true; m.editedAt = Date.now(); }, mid), { ignore: `[data-mid="${mid}"] .bub, [data-mid="${mid}"] .bub *, [data-mid="${mid}"] .more`, seen: (q) => q.evaluate(() => /edited/.test([...document.querySelectorAll('li.m.me .bub')].at(-1)?.textContent ?? '')) });
  // failed -> Retry on the newest message, then sending again
  await probe(p, t('failed / Retry line on the newest message: nothing moves'), () => pg(p, (mid) => { window.__pg.db.chats.find((x) => x.id === 'arjun').messages.find((x) => x.id === mid).delivery = 'failed'; }, mid), { ignore: '.fail, .fail *', seen: (q) => q.evaluate(() => !!document.querySelector('.fail.tail [data-act=retry]')) });
  await probe(p, t('Retry (failed -> sending -> delivered): nothing moves'), async () => { await p.locator('[data-act=retry]').click(); await wait(p, 1000); }, { ignore: '.fail, .fail *' });
  // file progress text
  await p.getByRole('button', { name: 'Attach a file' }).click(); await wait(p, 400);
  await p.getByRole('button', { name: /wedding-clip/ }).click(); await wait(p, 600);
  const fid = await pg(p, () => window.__pg.db.chats.find((x) => x.id === 'arjun').messages.at(-1).id);
  await probe(p, t('file progress ("· 37%", "· 100%", then sent): nothing outside the file bubble moves'), () => wait(p, 3500), { ignore: `[data-mid="${fid}"] .bub, [data-mid="${fid}"] .bub *`, settle: 400 });
  await p.context().close();

  // group host strip + queued caption
  p = await open(b, form, 'chat/hike');
  await probe(p, t('group: host-offline strip appears: no message moves'), () => pg(p, () => { window.__pg.db.env.hostsOnline = false; }), { ignore: '.strip, .strip *', seen: (q) => q.evaluate(() => !!document.querySelector('.strip.warn')) });
  await p.fill(TA, 'queued post'); await p.keyboard.press(sendKey(form)); await wait(p, 350);
  await probe(p, t('group: "Waiting for a host" caption fades in under the newest post: nothing moves'), () => wait(p, 1200), { ignore: '.wait, .wait *', seen: (q) => q.evaluate(() => !!document.querySelector('.wait.tail')) });
  await probe(p, t('group: host back (caption leaves, tick turns to a check): nothing moves'), () => pg(p, () => { window.__pg.db.env.hostsOnline = true; }), { ignore: '.strip, .strip *, .wait, .wait *', settle: 1500, seen: (q) => q.evaluate(() => !document.querySelector('.strip')) });
  ok(t('group: ...and the caption is gone, the post is delivered'), await pg(p, () => !document.querySelector('.wait') && /Delivered/.test([...document.querySelectorAll('li.m.me .vh')].at(-1)?.textContent ?? '')));
  await p.context().close();

  // chats list: badges, drafts, waiting line
  p = await open(b, form, 'chats');
  const row = (id) => `li:has(.name:text("${id}"))`;
  await probe(p, t('list: unread badge appears: no row, time or text moves'), () => pg(p, () => { window.__pg.db.chats.find((c) => c.id === 'arjun').unread = 3; }), { ignore: '.t-badge, .dot, .pv, .pv *', seen: (q) => q.evaluate(() => [...document.querySelectorAll('.t-badge')].some((e) => e.textContent.trim() === '3')) });
  await probe(p, t('list: badge grows to 99+: nothing moves'), () => pg(p, () => { window.__pg.db.chats.find((c) => c.id === 'arjun').unread = 120; }), { ignore: '.t-badge, .dot, .pv, .pv *', seen: (q) => q.evaluate(() => [...document.querySelectorAll('.t-badge')].some((e) => e.textContent.trim() === '99+')) });
  await probe(p, t('list: badge clears: nothing moves'), () => pg(p, () => { window.__pg.db.chats.find((c) => c.id === 'arjun').unread = 0; }), { ignore: '.t-badge, .dot, .pv, .pv *' });
  await probe(p, t('list: draft label appears and clears: nothing moves'), async () => { await pg(p, () => { window.__pg.db.chats.find((c) => c.id === 'dev').draft = 'a draft that is quite long and will need to be cut off with an ellipsis'; }); await wait(p, 300); await pg(p, () => { window.__pg.db.chats.find((c) => c.id === 'dev').draft = ''; }); });
  await pg(p, () => { const c = window.__pg.db.chats.find((x) => x.id === 'dev'); c.messages.push({ id: 'zz9', from: 'me', at: Date.now() + 1000, text: 'hello', delivery: 'sending' }); });
  await wait(p, 900);
  await probe(p, t('list: sending -> Waiting -> delivered preview line: no row moves'), async () => { await pg(p, () => { window.__pg.db.chats.find((x) => x.id === 'dev').messages.at(-1).delivery = 'waiting'; }); await wait(p, 300); await pg(p, () => { window.__pg.db.chats.find((x) => x.id === 'dev').messages.at(-1).delivery = 'delivered'; }); });
  await p.context().close();
}

/* ----------------------------------------------------------------- calls */
const CALLA = ['.callscreen .av', '.callscreen .name', '.callscreen .status', '.callscreen .e2e', '.callscreen .controls .ctl', '.callscreen .controls .round', '.callscreen .controls .ctl > span'];
async function callsSection(b, form) {
  const t = (s) => `${form} call: ${s}`;
  let sawRecon = false;
  const p = await open(b, form, 'chat/arjun');
  await pg(p, () => window.__pg.call.place('arjun'));
  await p.waitForSelector('.callscreen'); await wait(p, 1000);
  await anchors(p, t('Calling... -> Ringing...: avatar, name, status, controls stay'), CALLA, () => wait(p, 1300), { settle: 100 });
  await anchors(p, t('Ringing... -> Connecting... -> timer + "End-to-end encrypted" (timer ticks): nothing moves'), CALLA, async () => { await pg(p, () => window.__pg.call.remote('answer')); await wait(p, 3300); });
  ok(t('...the timer and the "End-to-end encrypted" line are showing'), await pg(p, () => !!document.querySelector('.callscreen .timer.on') && /End-to-end/.test(document.querySelector('.callscreen .e2e.on')?.textContent ?? '')));
  await anchors(p, t('Mute / Muted label and icon: nothing moves'), CALLA, async () => { await p.getByRole('button', { name: 'Mute', exact: true }).click(); await wait(p, 500); await p.getByRole('button', { name: 'Mute', exact: true }).click(); await wait(p, 500); });
  if (form === 'android') await anchors(p, t('Speaker toggle: nothing moves'), CALLA, async () => { await p.getByRole('button', { name: 'Speaker', exact: true }).click(); await wait(p, 500); });
  await anchors(p, t('active -> Reconnecting... (timer under the status) -> active: nothing moves'), CALLA, async () => { await pg(p, () => window.__pg.call.drop()); await wait(p, 1000); sawRecon = await pg(p, () => /Reconnecting/.test(document.querySelector('.callscreen .status .msg')?.textContent ?? '')); await pg(p, () => window.__pg.call.recover()); await wait(p, 1000); });
  ok(t('...Reconnecting... was really shown'), sawRecon);
  await anchors(p, t('waiting card slides in over the call: name, avatar, status and controls stay'), CALLA, async () => { await pg(p, () => window.__pg.call.receive('sana')); await wait(p, 1100); });
  const card = await pg(p, () => !!document.querySelector('[data-waiting]'));
  ok(t('the waiting card is on screen'), card);
  await anchors(p, t('waiting card text/avatar are stable while it shows (timer keeps ticking)'), [...CALLA, '[data-waiting]', '[data-waiting] .who', '[data-waiting] .acts', '[data-waiting] .hint'], () => wait(p, 1500), { settle: 100 });
  await anchors(p, t('waiting card leaves (Decline): name, avatar, status and controls stay'), CALLA, async () => { await pg(p, () => window.__pg.call.declineWaiting()); await wait(p, 1100); }, { max: 0.03 });   // the call record joins the chat list behind the screen
  await anchors(p, t('remote hang-up: "Call ended" + length line: nothing moves'), CALLA, async () => { await pg(p, () => window.__pg.call.remote('hangup')); await wait(p, 500); }, { settle: 100, max: 0.03 });   // ditto: the record lands behind the screen
  await until(p, () => window.__pg.call.phase === 'idle', 5000);
  // Couldn't connect
  await pg(p, () => { window.__pg.db.people.find((x) => x.id === 'arjun').online = false; });
  await pg(p, () => window.__pg.call.place('arjun')); await p.waitForSelector('.callscreen'); await wait(p, 1000);
  await anchors(p, t('Calling... -> Couldn\'t connect: nothing moves'), CALLA.slice(0, 5), async () => { await pg(p, () => window.__pg.call.hangup()); await wait(p, 100); }, { settle: 100, max: 0.03 });
  await until(p, () => window.__pg.call.phase === 'idle', 5000);
  await pg(p, () => { window.__pg.db.people.find((x) => x.id === 'arjun').online = true; });
  await p.context().close();
}

async function callSystemSection(b, form) {
  const t = (s) => `${form} system: ${s}`;
  const p = await open(b, form, 'chat/arjun');
  await pg(p, () => { window.__pg.call.place('arjun'); window.__pg.call.remote('answer'); });
  await until(p, () => window.__pg.call.phase === 'active'); await wait(p, 600);
  if (form === 'android') {
    await pg(p, () => window.__pg.call.minimize()); await wait(p, 700);
    await pg(p, () => window.__sys.openShade()); await p.waitForSelector('[data-notice=ongoing]'); await wait(p, 700);
    await anchors(p, t('shade: Mute <-> Unmute swaps its label, Speaker and Hang up do not slide'), ['[data-notice=ongoing] .act', '[data-notice=ongoing] .t', '[data-notice=ongoing] .x'], async () => { await p.getByRole('button', { name: 'Mute', exact: true }).first().click(); await wait(p, 500); await p.getByRole('button', { name: 'Unmute', exact: true }).first().click(); await wait(p, 500); });
    await anchors(p, t('shade: ongoing-call timer ticks: nothing moves'), ['[data-notice=ongoing]', '[data-notice=ongoing] .x', '[data-notice=ongoing] .act'], () => wait(p, 1600), { settle: 100 });
    await pg(p, () => window.__sys.closeShade()); await wait(p, 600);
    await anchors(p, t('status-bar call chip: timer ticks, clock stays'), ['[data-statusbar] .clk', '[data-statusbar] .chip', '[data-statusbar] .gl'], () => wait(p, 1500), { settle: 100 });
  } else {
    await pg(p, () => window.__sys.setBackground(true)); await p.waitForSelector('[data-os=mini]'); await wait(p, 800);
    await anchors(p, t('mini window: timer ticks and Mute toggles, nothing moves'), ['[data-os=mini]', '[data-os=mini] .open', '[data-os=mini] .r'], async () => { await p.getByRole('button', { name: 'Mute', exact: true }).click(); await wait(p, 600); await p.getByRole('button', { name: 'Unmute', exact: true }).click(); await wait(p, 900); });
  }
  await pg(p, () => window.__pg.call.hangup()); await until(p, () => window.__pg.call.phase === 'idle', 5000);
  await p.context().close();
}

/* ---------------------------------------------------------------- groups */
async function groupsSection(b, form) {
  const t = (s) => `${form} groups: ${s}`;
  const p = await open(b, form, 'chat/hike/info');
  await drawerOpen(p);
  const sim = (n) => p.getByRole('button', { name: n, exact: true }).click();
  await sim('Make me admin'); await wait(p, 600);
  await probe(p, t('flag count appears on a member row ("1 of 3 flags"): no row moves'), () => sim('Flag'), { ignore: '.fc, .rq', seen: (q) => q.evaluate(() => /1 of 3 flags/.test(document.querySelector('.fc')?.textContent ?? '')) });
  await probe(p, t('flag count 1 -> 2: no row moves'), () => sim('Flag'), { ignore: '.fc, .rq', seen: (q) => q.evaluate(() => /2 of 3 flags/.test(document.querySelector('.fc')?.textContent ?? '')) });
  const hl = () => pg(p, () => document.querySelector('.stk .cur')?.textContent ?? '');
  const hl0 = await hl();
  await probe(p, t('hosts offline line (Hosts section): nothing moves'), () => p.getByRole('switch', { name: 'Hosts online for groups', exact: true }).click(), { ignore: '.stk *' });
  ok(t('...the hosts line really changed'), (await hl()) !== hl0);
  await probe(p, t('hosts back online line: nothing moves'), () => p.getByRole('switch', { name: 'Hosts online for groups', exact: true }).click(), { ignore: '.stk *' });
  await drawerClose(p);
  // invite link: Copy -> Copied
  await p.getByRole('button', { name: /Create invite link/ }).click(); await wait(p, 700);
  await probe(p, t('"Copy" -> "Copied": the button beside it does not slide'), () => p.getByRole('button', { name: 'Copy', exact: true }).click(), { settle: 2400, seen: (q) => q.evaluate(() => !!document.querySelector('.cp.on')) });
  // chat request notes
  await p.locator('[data-member=kavya]').click(); await wait(p, 400);
  await p.getByRole('button', { name: /Ask Kavya for a private chat/ }).click(); await wait(p, 400);
  await p.getByRole('button', { name: 'Send request' }).click(); await wait(p, 600);
  await drawerOpen(p);
  await probe(p, t('"Declined your chat request" note on the member row: no row moves'), () => sim('Kavya declines'), { ignore: '.rq', seen: (q) => q.evaluate(() => /Declined your chat request/.test(document.querySelector('[data-member=kavya]')?.textContent ?? '')) });
  await drawerClose(p);
  await p.context().close();

  // groups list: request line changes
  const q = await open(b, form, 'groups');
  await drawerOpen(q);
  await q.getByRole('button', { name: 'Nisha sends me a request', exact: true }).click(); await wait(q, 700);
  await drawerClose(q);
  await q.close();
}

/* --------------------------------------------------------------- profile */
async function profileSection(b, form) {
  const t = (s) => `${form} profile: ${s}`;
  const p = await open(b, form, 'me');
  const field = (k) => p.locator(`[data-field=${k}]`);
  // Saved tag
  await field('status').locator('button.main').click(); await wait(p, 400);
  await p.locator('[data-field=status] input').fill('Out for chai');
  await p.getByRole('button', { name: 'Save', exact: true }).click(); await wait(p, 450);
  ok(t('...the "Saved" tag is showing'), await pg(p, () => !!document.querySelector('[data-field=status] .saved')));
  await probe(p, t('the "Saved" tag leaves the label line: nothing moves'), () => wait(p, 1900), { ignore: '.saved, .saved *' });
  // byte counter near the limit + refused save
  await field('about').locator('button.main').click(); await wait(p, 400);
  await anchors(p, t('byte counter appears near the limit while typing: the buttons stay'), ['[data-field=about] .acts', '[data-field=about] textarea', '[data-field=about] .acts .b'], async () => { await p.locator('[data-field=about] textarea').fill('é'.repeat(1700)); await wait(p, 300); });
  ok(t('...the byte counter is showing'), await pg(p, () => /of 4096 bytes/.test(document.querySelector('[data-field=about] .count')?.textContent ?? '')));
  await anchors(p, t('a refused save (too long) shows its reason: the buttons stay'), ['[data-field=about] .acts', '[data-field=about] textarea', '[data-field=about] .acts .b', '.pf-col .pf-sec:nth-of-type(2)'], async () => { await p.locator('[data-field=about] textarea').fill('é'.repeat(2049)); await p.getByRole('button', { name: 'Save', exact: true }).click(); await wait(p, 400); });
  ok(t('...the refusal is showing'), await pg(p, () => /too long/.test(document.querySelector('[data-field=about] .err')?.textContent ?? '')));
  await p.getByRole('button', { name: 'Cancel', exact: true }).click(); await wait(p, 400);
  // pending strip (the app is offline and a save is waiting to sync)
  await probe(p, t('"Saved on this device. Will sync when you’re connected" floats in: nothing moves'), () => pg(p, () => { window.__px.pending = 1; }), { ignore: '[data-pending], [data-pending] *', settle: 600, seen: (q) => q.evaluate(() => !!document.querySelector('[data-pending]')) });
  await probe(p, t('the pending note leaves after reconnecting: nothing moves'), () => pg(p, () => { window.__px.pending = 0; }), { ignore: '[data-pending], [data-pending] *', settle: 600 });
  // availability: Saved tag
  let sawAv = false;
  await probe(p, t('availability: the "Saved" tag appears and leaves the status line: the note below stays'), async () => {
    await p.getByRole('radio', { name: 'Available', exact: true }).click(); await wait(p, 400); sawAv = await pg(p, () => !!document.querySelector('.av .saved'));
  }, { ignore: '.av *', settle: 2200 });
  ok(t('...the tag was really shown'), sawAv);
  // refused picture file: floating, nothing moves
  await probe(p, t('a refused picture shows its reason floating: nothing moves'), async () => {
    await p.locator('[data-file]').setInputFiles({ name: 'notes.txt', mimeType: 'text/plain', buffer: Buffer.from('not a picture') }); await wait(p, 700);
  }, { ignore: '[data-pic-error], [data-pic-error] *' });
  ok(t('...and the reason is shown'), await pg(p, () => !!document.querySelector('[data-pic-error]')));
  await p.context().close();

  // person profile: Busy chip, Allow calls caption
  const q = await open(b, form, 'contact/arjun');
  await probe(q, t('person: Busy chip appears: the buttons below stay'), () => pg(q, () => { const pr = window.__pg.db.people.find((x) => x.id === 'arjun'); pr.profile = { ...pr.profile, fields: { ...pr.profile?.fields, availability: { available: false } } }; }), { ignore: '.avl, .avl *', seen: (q) => q.evaluate(() => !!document.querySelector('.avl')) });
  ok(t('...and the chip is really shown'), await pg(q, () => !!document.querySelector('.avl')));
  await probe(q, t('person: Busy chip leaves: nothing moves'), () => pg(q, () => { const pr = window.__pg.db.people.find((x) => x.id === 'arjun'); pr.profile = { ...pr.profile, fields: { ...pr.profile.fields, availability: { available: true } } }; }), { ignore: '.avl, .avl *' });
  const sw = q.getByRole('switch', { name: 'Allow calls', exact: true });
  await sw.scrollIntoViewIfNeeded(); await wait(q, 300);
  await probe(q, t('person: "Allow calls" off adds the "Messages only" line: the settings card stays'), () => sw.click(), { ignore: '.why, .why *, .knob', seen: (q) => q.evaluate(() => /Messages only/.test(document.querySelector('.why')?.textContent ?? '')) });
  await probe(q, t('person: "Allow calls" on again: nothing moves'), () => sw.click(), { ignore: '.why, .why *, .knob' });
  await q.context().close();
}

/* ------------------------------------------------------------------ gate */
async function gateSection(b, form) {
  const t = (s) => `${form} gate: ${s}`;
  const p = await open(b, form, 'chats');
  const jump = async (step) => { await drawerOpen(p); await p.getByLabel('Jump to step').selectOption(step); await drawerClose(p); await p.waitForFunction((s) => document.querySelectorAll('.pane').length === 1 && document.querySelector('[data-gate-step]')?.dataset.gateStep === s, step, { timeout: 5000 }); await wait(p, 300); };
  await drawerOpen(p); await p.getByLabel('App state').selectOption('onboarding'); await drawerClose(p); await wait(p, 600);
  await jump('pass');
  const pw = p.getByLabel('Passphrase', { exact: true });
  await anchors(p, t('passphrase: hint turns into a rating ("Short" -> "Fair" -> "Strong"): the note below stays'), ['.step h1', '.step .lead', '.step .field', '.step .meter', '.step .n'], async () => { await pw.pressSequentially('abc', { delay: 60 }); await wait(p, 250); await pw.pressSequentially('defghi', { delay: 60 }); await wait(p, 250); await pw.pressSequentially('jklmnopqrst', { delay: 60 }); await wait(p, 300); });
  ok(t('...the rating really reads Strong'), await pg(p, () => document.querySelector('.says .cur')?.textContent === 'Strong'));
  await anchors(p, t('passphrase: show / hide toggle: nothing moves'), ['.step h1', '.step .field', '.step .meter', '.step .n'], async () => { await p.getByRole('button', { name: /Show passphrase/ }).click(); await wait(p, 250); await p.getByRole('button', { name: /Hide passphrase/ }).click(); await wait(p, 250); });
  await drawerOpen(p); await p.getByLabel('Key derivation time').fill('1500'); await drawerClose(p);
  let busy = false;
  await anchors(p, t('passphrase: Continue -> Encrypting...: the footer buttons do not change width'), ['.step .foot .btn', '.step .foot'], async () => { await p.getByRole('button', { name: 'Continue', exact: true }).click(); await wait(p, 300); busy = await pg(p, () => !!document.querySelector('.step .foot .btn[aria-busy]')); await wait(p, 400); }, { settle: 50, size: true });
  ok(t('...and the button really was in its busy state'), busy);
  await p.waitForFunction(() => document.querySelector('[data-gate-step]')?.dataset.gateStep !== 'pass', null, { timeout: 6000 }); await wait(p, 700);
  // check my copy
  await jump('confirm');
  await anchors(p, t('check my copy: wrong word shows its error, the fields below stay'), ['.step .field', '.step .field input', '.step .foot .btn'], async () => { const ins = p.locator('.step input'); await ins.nth(0).fill('zzz'); await ins.nth(1).fill('zzz'); await ins.nth(2).fill('zzz'); await p.getByRole('button', { name: 'Done', exact: true }).click(); await wait(p, 500); });
  // unlock with lockout
  await drawerOpen(p);
  await p.getByLabel('Tries before lockout').fill('3'); await p.getByLabel('Lockout seconds').fill('3'); await p.getByLabel('Key derivation time').fill('100');
  await p.getByLabel('App state').selectOption('locked'); await drawerClose(p);
  await p.waitForFunction(() => document.querySelector('[data-gate-step]')?.dataset.gateStep === 'unlock' && document.querySelectorAll('.pane').length === 1, null, { timeout: 5000 }); await wait(p, 600);
  const UA = ['.step h1', '.step .lead', '.step .av', '.step .field', '.step .field input', '.step .rem', '.step .foot .btn'];
  await anchors(p, t('unlock: a wrong passphrase shows its error: title, field, remember row and buttons stay'), UA, async () => { await p.getByLabel('Passphrase', { exact: true }).fill('nope'); await p.getByRole('button', { name: 'Unlock', exact: true }).click(); await p.waitForSelector('text=didn’t open the account', { timeout: 3000 }); await wait(p, 400); });
  await anchors(p, t('unlock: lockout message + hint, countdown ticking: nothing moves'), UA, async () => { for (let i = 0; i < 2; i++) { await p.getByLabel('Passphrase', { exact: true }).fill('nope' + i); await p.getByRole('button', { name: 'Unlock', exact: true }).click(); await wait(p, 400); } await p.waitForSelector('text=Too many tries', { timeout: 3000 }); await wait(p, 1300); });
  await p.context().close();
}

/* ----------------------------------------------------------------- shell */
async function shellSection(b, form) {
  const t = (s) => `${form} shell: ${s}`;
  const p = await open(b, form, 'chats');
  await probe(p, t('tab badge count changes (unread 0 -> 5): nothing moves'), () => pg(p, () => { window.__pg.db.chats.forEach((c) => (c.unread = 0)); window.__pg.db.chats.find((c) => c.id === 'arjun').unread = 5; }), { ignore: '.t-badge, .dot, .pv *, .pv', seen: (q) => q.evaluate(() => [...document.querySelectorAll('.t-badge, .dot')].some((e) => e.textContent.trim() === '5')) });
  // The drawer takes its own column (App.svelte): the frame moves aside on purpose, but must never sit under it.
  await drawerOpen(p); await wait(p, 300);
  ok(t('Simulation drawer open: the app frame sits beside it, never under it'), await p.evaluate(() => { const f = document.querySelector('[data-form]').getBoundingClientRect(), d = document.querySelector('aside.drawer').getBoundingClientRect(); return f.right <= d.left + 0.5; }));
  await drawerClose(p); await wait(p, 300);
  await p.context().close();
  // theme must not change geometry: same boxes in light and dark
  const geom = async (dark, hash) => { const q = await open(b, form, hash, { dark }); await wait(q, 400); const g = await q.evaluate(() => Array.from(document.querySelector('[data-form]').querySelectorAll('*')).filter((e) => !e.closest('svg') || e.tagName === 'svg').map((e) => { const r = e.getBoundingClientRect(); return [Math.round(r.left), Math.round(r.top), Math.round(r.width), Math.round(r.height)].join(','); }).join('|')); await q.context().close(); return g; };
  for (const hash of ['chats', 'chat/arjun', 'me', 'chat/hike/info']) {
    const [a, d] = [await geom(false, hash), await geom(true, hash)];
    ok(t(`light and dark have identical boxes on #/${hash}`), a === d);
  }
}

(async () => {
  const b = await L.launch();
  try {
    for (const form of ['android', 'desktop']) {
      if (want('chat')) await chatSection(b, form);
      if (want('calls')) await callsSection(b, form);
      if (want('system')) await callSystemSection(b, form);
      if (want('groups')) await groupsSection(b, form);
      if (want('profile')) await profileSection(b, form);
      if (want('gate')) await gateSection(b, form);
      if (want('shell')) await shellSection(b, form);
    }
  } catch (e) { fail++; console.log('FAIL exception ' + (e.stack || e).toString().split('\n').slice(0, 4).join(' | ')); }
  await b.close();
  console.log(`\n${pass} passed, ${fail} failed`);
  process.exit(fail ? 1 : 0);
})();

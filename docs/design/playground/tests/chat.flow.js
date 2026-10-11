/* Chat flow checks against the running dev server (http://127.0.0.1:5178). Prints PASS/FAIL per check.
   Run: node tests/chat.flow.js   (BASE=… to override) */
import { createRequire } from 'node:module';
const L = createRequire(import.meta.url)('/home/abe/p2p_chat/.claude/worktrees/design-refinement/docs/design/refined/qa/lib.js');
const BASE = process.env.BASE || 'http://127.0.0.1:5178/';
let pass = 0, fail = 0;
const ok = (name, cond, extra = '') => { cond ? pass++ : fail++; console.log((cond ? 'PASS ' : 'FAIL ') + name + (cond ? '' : ' ' + extra)); };

const TA = 'textarea[aria-label^="Message to"]';
const lastMsg = (p) => p.locator('li.m:not(.callrec)').last();
async function open(b, form, hash, opts = {}) {
  const p = await L.page(b, { w: form === 'android' ? 600 : 1440, h: 900, dark: false, mobile: !!opts.mobile });
  await p.goto(`${BASE}?form=${form}&theme=light#/${hash}`);
  await p.waitForSelector('.conv');
  await p.waitForTimeout(300);
  return p;
}
async function sim(p, label, on) {          // set a Simulation switch
  await p.getByRole('button', { name: 'Simulation' }).click();
  const sw = p.getByRole('switch', { name: label, exact: true });
  if ((await sw.getAttribute('aria-checked')) !== String(on)) await sw.click();
  await p.getByRole('button', { name: 'Close simulation' }).click();
}
async function simBtn(p, text) {
  await p.getByRole('button', { name: 'Simulation' }).click();
  await p.getByRole('button', { name: text }).click();
  await p.getByRole('button', { name: 'Close simulation' }).click();
}
async function menuOn(p, li, item) {         // desktop: hover chevron -> menu item
  await li.locator('.bub').hover();
  await li.locator('.more').click();
  await p.getByRole('menuitem', { name: item, exact: true }).click();
}
async function send(p, text) { await p.fill(TA, text); await p.keyboard.press('Enter'); }

(async () => {
  const b = await L.launch();
  const errs = [];
  const track = (p) => { p.on('pageerror', (e) => errs.push(e.message)); };
  try {
    /* ---------- desktop 1:1 ---------- */
    let p = await open(b, 'desktop', 'chat/arjun'); track(p);
    ok('opening marks read (unread cleared, divider shown)', await p.locator('.unread').count() === 1 && /2 new messages/.test(await p.locator('.unread').innerText()));
    ok('own bubble on the right, theirs on the left', await (async () => {
      const a = await p.locator('li.m.me .bub').first().boundingBox(), t = await p.locator('li.m.them .bub').first().boundingBox();
      return a.x > t.x;
    })());

    await send(p, 'hello there');
    let li = lastMsg(p);
    ok('send: bubble appears', (await li.innerText()).includes('hello there'));
    ok('send: starts as Sending', /Sending/.test(await li.locator('.vh').innerText()));
    await p.waitForTimeout(1100);
    ok('send: becomes Delivered', /Delivered/.test(await li.locator('.vh').innerText()));
    ok('send: draft cleared', (await p.inputValue(TA)) === '');
    await p.screenshot({ path: process.env.SHOTS ? process.env.SHOTS + 'f-sent.png' : '/dev/null' }).catch(() => {});

    // reply
    const target = p.locator('li.m.them').last();
    await menuOn(p, target, 'Reply');
    ok('reply: chip shown', await p.locator('.ctx').innerText().then((t) => /Replying to Arjun/.test(t)));
    await send(p, 'got it');
    li = lastMsg(p);
    ok('reply: quote inside bubble', (await li.locator('.quote').count()) === 1);
    await li.locator('.quote').click();
    await p.waitForTimeout(150);
    ok('reply: tap quote highlights original', (await p.locator('.bub.flash').count()) === 1);
    await p.waitForTimeout(1500);
    ok('reply: highlight clears itself', (await p.locator('.bub.flash').count()) === 0);

    // edit
    await p.waitForTimeout(900);
    li = p.locator('li.m.me', { hasText: 'hello there' });
    await menuOn(p, li, 'Edit');
    ok('edit: field prefilled', (await p.inputValue(TA)) === 'hello there');
    ok('edit: chip says Editing', /Editing message/.test(await p.locator('.ctx').innerText()));
    await p.fill(TA, 'hello again');
    await p.keyboard.press('Enter');
    li = p.locator('li.m.me', { hasText: 'hello again' });
    ok('edit: bubble updated with edited marker', (await li.count()) === 1 && /edited/.test(await li.innerText()));
    ok('edit: draft restored after save', (await p.inputValue(TA)) === '');

    // delete everyone + undo
    await menuOn(p, li, 'Delete');
    ok('delete: confirm dialog', (await p.getByRole('dialog', { name: 'Delete message' }).count()) === 1);
    await p.getByRole('button', { name: 'Delete for everyone' }).click();
    ok('delete: tombstone', (await p.locator('.bub.gone', { hasText: 'You deleted this message' }).count()) === 1);
    await p.getByRole('button', { name: 'Undo' }).click();
    ok('delete: undo restores text', (await p.locator('li.m.me', { hasText: 'hello again' }).count()) === 1 && (await p.locator('.bub.gone').count()) === 0);
    // delete for me
    await menuOn(p, p.locator('li.m.me', { hasText: 'hello again' }), 'Delete');
    await p.getByRole('button', { name: 'Delete for me' }).click();
    ok('delete for me: row removed', (await p.locator('li.m.me', { hasText: 'hello again' }).count()) === 0);
    await p.getByRole('button', { name: 'Undo' }).click();
    ok('delete for me: undo brings it back', (await p.locator('li.m.me', { hasText: 'hello again' }).count()) === 1);

    // message info
    await menuOn(p, p.locator('li.m.me', { hasText: 'hello again' }), 'Message info');
    ok('info: dialog with Sent row', /Sent/.test(await p.getByRole('dialog', { name: 'Message info' }).innerText()));
    await p.keyboard.press('Escape');
    await p.waitForTimeout(300);
    ok('info: Escape closes', (await p.getByRole('dialog').count()) === 0);

    // file cancel / retry
    await p.getByRole('button', { name: 'Attach a file' }).click();
    await p.getByRole('button', { name: /wedding-clip\.mp4/ }).click();
    await p.waitForSelector('[role=progressbar]');
    ok('file: progress bar shown', true);
    const v0 = +(await p.locator('[role=progressbar]').getAttribute('aria-valuenow'));
    await p.waitForTimeout(1200);
    const v1 = +(await p.locator('[role=progressbar]').getAttribute('aria-valuenow'));
    ok('file: progress advances', v1 > v0, `${v0}->${v1}`);
    await p.getByRole('button', { name: /Cancel sending/ }).click();
    ok('file: cancelled shows Retry', /Cancelled/.test(await lastMsg(p).locator('xpath=..').locator('.fail').last().innerText().catch(() => '')) || (await p.locator('.fail', { hasText: 'Cancelled' }).count()) === 1);
    await p.getByRole('button', { name: /Retry sending/ }).click();
    await p.waitForSelector('[role=progressbar]');
    ok('file: retry restarts', (+(await p.locator('[role=progressbar]').getAttribute('aria-valuenow'))) < 60);
    await p.getByRole('button', { name: /Cancel sending/ }).click(); // leave it tidy

    // offline peer / device -> waiting; back online -> delivered
    await sim(p, 'This device online', false);
    await send(p, 'while offline');
    await p.waitForTimeout(1100);
    ok('network down: waiting', /offline/i.test(await lastMsg(p).locator('.vh').innerText()), await lastMsg(p).locator('.vh').innerText());
    await sim(p, 'This device online', true);
    await p.waitForTimeout(1900);
    ok('network back: delivered', /Delivered/.test(await lastMsg(p).locator('.vh').innerText()));

    // fail + retry
    await sim(p, 'Next send fails', true);
    await send(p, 'this will fail');
    await p.waitForTimeout(1100);
    ok('fail: Not delivered + Retry', (await p.locator('.fail', { hasText: 'Not delivered' }).count()) === 1);
    await p.getByRole('button', { name: 'Retry sending', exact: true }).click();
    await p.waitForTimeout(1200);
    ok('fail: retry succeeds', /Delivered/.test(await lastMsg(p).locator('.vh').innerText()) && (await p.locator('.fail', { hasText: 'Not delivered' }).count()) === 0);

    // call record: incoming missed on their side, compact pill, no emoji
    await simBtn(p, 'Add missed incoming call');
    const rec = p.locator('li.callrec').last();
    const rb = await rec.locator('.pill').boundingBox(), cb = await p.locator('.msgs').boundingBox();
    ok('call record: incoming sits on their side', rb.x + rb.width / 2 < cb.x + cb.width / 2);
    ok('call record: text, no emoji, one line', /Missed voice call/.test(await rec.innerText()) && rb.height < 44 && !/\p{Extended_Pictographic}/u.test(await rec.innerText()));
    await simBtn(p, 'Add outgoing call record');
    const ob = await p.locator('li.callrec').last().locator('.pill').boundingBox();
    ok('call record: outgoing sits on my side', ob.x + ob.width / 2 > cb.x + cb.width / 2);

    // draft shown in list
    await p.fill(TA, 'unsent draft');
    ok('draft: appears in the list preview', /Draft:\s*unsent draft/.test(await p.locator('.row.sel .pv').innerText()));
    await p.context().close();

    /* ---------- desktop: offline peer, message-only, group ---------- */
    p = await open(b, 'desktop', 'chat/meera'); track(p);
    ok('offline peer: strip + status', /offline/i.test(await p.locator('.strip').innerText()) && /Offline/.test(await p.locator('.pr').innerText()));
    await send(p, 'ping');
    await p.waitForTimeout(1100);
    ok('offline peer: message waits', /Waiting for Meera/.test(await lastMsg(p).locator('.vh').innerText()));
    await p.context().close();

    p = await open(b, 'desktop', 'chat/dev'); track(p);
    ok('message-only: call button aria-disabled, status says so', (await p.locator('button[aria-label="Call"]').getAttribute('aria-disabled')) === 'true' && /messages only/.test(await p.locator('.pr').innerText()));
    ok('draft restored into composer', (await p.inputValue(TA)) === 'Lunch Thursday?');
    await p.context().close();

    p = await open(b, 'desktop', 'chat/hike'); track(p);
    ok('group: unread divider "3 new messages"', /3 new messages/.test(await p.locator('.unread').innerText()));
    ok('group: no attach button', (await p.getByRole('button', { name: 'Attach a file' }).count()) === 0);
    ok('group: sender names on first bubble of a run', (await p.locator('.who').count()) >= 3 && /Sam/.test(await p.locator('.who').first().innerText()));
    ok('group: system line centred', (await p.locator('li.sys').count()) === 1);
    const mine = p.locator('li.m.me').last();
    await mine.locator('.bub').hover(); await mine.locator('.more').click();
    const items = await p.getByRole('menuitem').allInnerTexts();
    ok('group: menu has no Edit/Delete', !items.some((t) => /Edit|Delete/.test(t)) && items.some((t) => /Reply/.test(t)), items.join('|'));
    await p.keyboard.press('Escape');
    await sim(p, 'Group hosts online', false);
    ok('group: host-offline strip', /No group host/.test(await p.locator('.strip').innerText()));
    await send(p, 'anyone there');
    await p.waitForTimeout(1100);
    ok('group: post waits for a host', /Waiting for a host/.test(await lastMsg(p).locator('.vh').innerText()));
    await sim(p, 'Group hosts online', true);
    await p.waitForTimeout(1900);
    ok('group: delivered when host returns', /Delivered/.test(await lastMsg(p).locator('.vh').innerText()));
    await p.context().close();

    p = await open(b, 'desktop', 'chat/book'); track(p);
    ok('removed group: read-only footer, no composer', /You were removed from this group/.test(await p.locator('.readonly').innerText()) && (await p.locator(TA).count()) === 0);
    await p.context().close();

    /* ---------- android (touch) ---------- */
    p = await open(b, 'android', 'chat/arjun', { mobile: true }); track(p);
    ok('android: own header with back', (await p.getByRole('button', { name: 'Back to chats' }).count()) === 1);
    await p.fill(TA, 'line one');
    await p.keyboard.press('Enter');
    ok('android: Enter adds a newline, does not send', (await p.inputValue(TA)).includes('\n') && (await p.locator('li.m.me:has-text("line one")').count()) === 0);
    await p.keyboard.press('Control+Enter');
    ok('android: Ctrl+Enter sends', (await p.locator('li.m.me:has-text("line one")').count()) === 1);
    const cdp = await p.context().newCDPSession(p);
    const bub = p.locator('li.m.them .bub').first();
    const bx = await bub.boundingBox();
    const pt = [{ x: bx.x + bx.width / 2, y: bx.y + bx.height / 2, id: 1 }];
    await cdp.send('Input.dispatchTouchEvent', { type: 'touchStart', touchPoints: pt });
    await p.waitForTimeout(250);
    ok('touch: no menu before 500ms', (await p.getByRole('menu').count()) === 0);
    await p.waitForTimeout(450);
    ok('touch: long-press opens bottom sheet', (await p.getByRole('menu').count()) === 1);
    await cdp.send('Input.dispatchTouchEvent', { type: 'touchEnd', touchPoints: [] });
    await p.waitForTimeout(300);
    ok('touch: sheet stays open after release', (await p.getByRole('menu').count()) === 1);
    const rows = await p.getByRole('menuitem').evaluateAll((els) => els.map((e) => e.getBoundingClientRect().height));
    ok('touch: menu rows are >= 44px', rows.every((h) => h >= 44), rows.join(','));
    ok('touch: Message info is in the menu', (await p.getByRole('menuitem', { name: 'Message info' }).count()) === 1);
    await p.keyboard.press('Escape');
    await p.waitForTimeout(300);
    ok('touch: Escape closes, focus returns to the message', (await p.evaluate(() => document.activeElement?.getAttribute('aria-label') ?? '')).startsWith('Actions for'));
    // move cancels the press
    await cdp.send('Input.dispatchTouchEvent', { type: 'touchStart', touchPoints: pt });
    await cdp.send('Input.dispatchTouchEvent', { type: 'touchMove', touchPoints: [{ ...pt[0], y: pt[0].y + 30 }] });
    await p.waitForTimeout(650);
    await cdp.send('Input.dispatchTouchEvent', { type: 'touchEnd', touchPoints: [] });
    ok('touch: moving cancels the long-press', (await p.getByRole('menu').count()) === 0);
    // keyboard access to the menu (hidden button)
    await p.locator('li.m.them .more').first().focus();
    await p.keyboard.press('Enter');
    await p.waitForSelector('[role=menu]');
    const first = await p.evaluate(() => document.activeElement?.textContent);
    await p.keyboard.press('ArrowDown');
    const second = await p.evaluate(() => document.activeElement?.textContent);
    await p.keyboard.press('End');
    const lastT = await p.evaluate(() => document.activeElement?.textContent);
    ok('menu keyboard: first focused, arrows rove, End jumps', /Reply/.test(first) && second !== first && /Message info/.test(lastT), [first, second, lastT].join('|'));
    await p.keyboard.press('Tab');
    ok('menu keyboard: Tab stays inside the menu', await p.evaluate(() => !!document.activeElement?.closest('[role=menu]')));
    await p.keyboard.press('Escape');
    await p.waitForTimeout(300);
    ok('menu keyboard: Escape returns focus to the message button', (await p.evaluate(() => document.activeElement?.getAttribute('aria-label') ?? '')).startsWith('Actions for'));
    await p.context().close();
  } catch (e) { fail++; console.log('FAIL (exception) ' + e.message); }
  ok('no page errors', errs.length === 0, errs.join(' | '));
  await b.close();
  console.log(`\n${pass} passed, ${fail} failed`);
  process.exit(fail ? 1 : 0);
})();

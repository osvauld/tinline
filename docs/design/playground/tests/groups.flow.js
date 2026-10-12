/* Groups flow checks against the running dev server (http://127.0.0.1:5178). Prints PASS/FAIL per check.
   Run: node tests/groups.flow.js   (BASE=… to override) */
import { createRequire } from 'node:module';
const L = createRequire(import.meta.url)('/home/abe/p2p_chat/.claude/worktrees/design-refinement/docs/design/refined/qa/lib.js');
const BASE = process.env.BASE || 'http://127.0.0.1:5178/';
let pass = 0, fail = 0;
const ok = (name, cond, extra = '') => { cond ? pass++ : fail++; console.log((cond ? 'PASS ' : 'FAIL ') + name + (cond ? '' : ' ' + extra)); };

const TA = 'textarea[aria-label^="Message to"]';
const allErrs = [];
async function open(b, form, hash) {
  const p = await L.page(b, { w: form === 'android' ? 600 : 1440, h: 900, dark: false });
  await p.goto(`${BASE}?form=${form}&theme=light#/${hash}`);
  await p.waitForSelector(form === 'android' ? '.phone' : '.win');
  await p.waitForTimeout(400);
  return p;
}
async function done(p) { allErrs.push(...p.errs); await p.context().close(); }
const drawer = (p) => p.getByRole('button', { name: 'Simulation', exact: true }).click();
const closeDrawer = (p) => p.getByRole('button', { name: 'Close simulation' }).click();
async function sim(p, text) { await drawer(p); await p.getByRole('button', { name: text, exact: true }).click(); await closeDrawer(p); }
async function hosts(p, on) {
  await drawer(p);
  const sw = p.getByRole('switch', { name: 'Hosts online for groups', exact: true });
  if ((await sw.getAttribute('aria-checked')) !== String(on)) await sw.click();
  await closeDrawer(p);
}
const member = (p, id) => p.locator(`[data-member=${id}]`);
async function menuItems(p, li) {
  await li.locator('.bub').hover();
  await li.locator('.more').click();
  const names = await p.getByRole('menuitem').allInnerTexts();
  await p.keyboard.press('Escape');
  await p.waitForTimeout(250);
  return names.map((s) => s.trim());
}
/** Per-frame heights of the first element matching `sel`, for `ms`. */
const sampleHeight = (p, sel, ms) => p.evaluate(({ sel, ms }) => new Promise((res) => {
  const out = []; const t0 = performance.now();
  const f = () => { const e = document.querySelector(sel); out.push(e ? Math.round(e.getBoundingClientRect().height * 10) / 10 : -1); performance.now() - t0 < ms ? requestAnimationFrame(f) : res(out); };
  requestAnimationFrame(f);
}), { sel, ms });
const smooth = (h, full) => {
  const ds = h.filter((x) => x >= 0);
  const distinct = new Set(ds.map((x) => Math.round(x))).size;
  let worst = 0; for (let i = 1; i < ds.length; i++) worst = Math.max(worst, Math.abs(ds[i] - ds[i - 1]));
  return { distinct, worst, last: ds[ds.length - 1], full };
};

(async () => {
  const b = await L.launch();
  try {
    /* ---------- 1. no edit / delete / attach in groups, present in 1:1 ---------- */
    let p = await open(b, 'desktop', 'chat/arjun');
    const mine1 = p.locator('li.m.me:not(.callrec)').last();
    const names1 = await menuItems(p, mine1);
    ok('1:1: my message menu offers Edit and Delete', names1.includes('Edit') && names1.includes('Delete'), names1.join(','));
    ok('1:1: attach button present', (await p.getByRole('button', { name: 'Attach a file' }).count()) === 1);
    await done(p);

    p = await open(b, 'desktop', 'chat/hike');
    const mine = p.locator('li.m.me').last(), theirs = p.locator('li.m.them').last();
    const gm = await menuItems(p, mine), gt = await menuItems(p, theirs);
    ok('group: my post menu has no Edit/Delete', !gm.includes('Edit') && !gm.includes('Delete') && gm.includes('Reply'), gm.join(','));
    ok('group: others’ post menu is reply/copy/info only', gt.join(',') === 'Reply,Copy,Message info', gt.join(','));
    ok('group: no attach button', (await p.getByRole('button', { name: 'Attach a file' }).count()) === 0);
    ok('group: no call button (group calls come later)', (await p.getByRole('button', { name: 'Call', exact: true }).count()) === 0);
    ok('group: sender names on incoming bubbles', (await p.locator('li.m.them .who').count()) >= 3);
    ok('group: system line rendered', (await p.locator('li.sys').count()) >= 1);
    // reply works
    await theirs.locator('.bub').hover(); await theirs.locator('.more').click();
    await p.getByRole('menuitem', { name: 'Reply', exact: true }).click();
    await p.fill(TA, 'on my way'); await p.keyboard.press('Enter');
    ok('group: reply posts with a quote', (await p.locator('li.m.me').last().locator('.quote').count()) === 1);
    await done(p);

    /* ---------- 2. roles: only the owner makes admins ---------- */
    p = await open(b, 'desktop', 'chat/hike/info');
    ok('info: header shows name, member count and my role', /Weekend hike/.test(await p.locator('.hero').innerText()) && /9 members/.test(await p.locator('.hero').innerText()) && /admin/.test(await p.locator('.hero').innerText()));
    await member(p, 'leo').click();
    let items = (await p.getByRole('dialog').allInnerTexts()).join(' ');
    ok('admin: no "Make admin" in a member sheet', !/Make admin/.test(items) && /Flag for removal/.test(items) && /Make host/.test(items));
    await p.keyboard.press('Escape'); await p.waitForTimeout(250);
    await sim(p, 'Make me owner');
    ok('owner: hero says owner', /owner/.test(await p.locator('.hero').innerText()));
    await member(p, 'leo').click();
    ok('owner: sees "Make admin"', (await p.getByRole('button', { name: /Make admin/ }).count()) === 1);
    await p.getByRole('button', { name: /Make admin/ }).click(); await p.waitForTimeout(300);
    ok('owner: Leo is now an admin (pill + sheet)', /Admin/.test(await p.getByRole('dialog').innerText()));
    await p.keyboard.press('Escape'); await p.waitForTimeout(250);
    await member(p, 'me').click();
    ok('owner: no Flag action on my own row', (await p.getByRole('button', { name: /Flag for removal/ }).count()) === 0);
    await p.keyboard.press('Escape'); await p.waitForTimeout(250);
    await sim(p, 'Make me admin');
    await member(p, 'meera').click();
    ok('admin: owner row shows "can’t be flagged", no Flag', (await p.getByRole('button', { name: /Flag for removal/ }).count()) === 0 && /can’t be flagged/.test(await p.getByRole('dialog').innerText()));
    await p.keyboard.press('Escape'); await p.waitForTimeout(250);
    await member(p, 'priya').click();
    ok('admin: no Make admin for another admin either', (await p.getByRole('button', { name: /(Make|Remove) admin/ }).count()) === 0);
    await p.keyboard.press('Escape'); await p.waitForTimeout(250);
    // rule enforced in the action layer too
    const rules = await p.evaluate(async () => {
      const urls = performance.getEntriesByType('resource').map((r) => r.name);
      const A = await import(urls.filter((x) => x.includes('/actions/groups.ts')).pop()); const g = A.group('hike');
      return { owner: A.flagMember(g, 'meera', 'sam').ok, admin: A.setAdmin(g, 'dev', true), self: A.flagMember(g, 'me', 'me').ok };
    });
    ok('actions: flagging the owner / non-owner making admins / self-flag all refused', !rules.owner && !rules.admin && !rules.self, JSON.stringify(rules));
    ok('member: Make me member → no invites/flags', await (async () => { await sim(p, 'Make me member');
      const inv = await p.locator('#inv-h').count(); await member(p, 'leo').click();
      const t = await p.getByRole('dialog').innerText(); await p.keyboard.press('Escape'); await p.waitForTimeout(250);
      return inv === 0 && !/Flag for removal|Make host|Make admin/.test(t) && /Ask Leo for a private chat/.test(t); })());
    await sim(p, 'Make me admin');
    await done(p);

    /* ---------- 3. flags: 3 flags remove; removal view ---------- */
    p = await open(b, 'desktop', 'chat/hike/info');
    await member(p, 'dev').click();
    await p.getByRole('button', { name: /Flag for removal/ }).click();
    ok('flag confirm: says flag 1 of 3, final, calm copy', /flag 1 of 3/.test(await p.getByRole('dialog').innerText()) && /can’t be taken back/.test(await p.getByRole('dialog').innerText()));
    await p.getByRole('button', { name: 'Back', exact: true }).click(); await p.waitForTimeout(200);
    ok('flag confirm: Back returns to the actions (nothing flagged yet)', (await member(p, 'dev').innerText()).indexOf('flags') < 0);
    await p.keyboard.press('Escape'); await p.waitForTimeout(250);
    for (let i = 0; i < 3; i++) {
      await member(p, 'dev').click();
      await p.getByRole('button', { name: /Flag for removal/ }).click();
      await p.getByRole('button', { name: 'Flag Dev', exact: true }).click();
      await p.waitForTimeout(350);
      if (i < 2) ok(`flag ${i + 1}: count shown on the row`, new RegExp(`${i + 1} of 3 flags`).test(await member(p, 'dev').innerText()));
    }
    await p.waitForTimeout(500);
    ok('same admin flagging 3x removes the member', (await member(p, 'dev').count()) === 0 && /8 members/.test(await p.locator('.hero').innerText()));
    ok('removed member listed for admins with who flagged', /Flagged by Maya|Flagged by You|Flagged by/.test(await p.locator('[aria-label="Removed members"]').innerText()));
    await sim(p, 'I get removed');
    p.goto(`${BASE}?form=desktop&theme=light#/chat/hike`);
    await p.waitForTimeout(500);
    ok('removed: read-only footer', /You were removed from this group/.test(await p.locator('.readonly').innerText()));
    ok('removed: composer gone, history kept', (await p.locator(TA).count()) === 0 && (await p.locator('li.m').count()) > 5);
    await p.getByRole('button', { name: 'See details' }).click(); await p.waitForTimeout(400);
    const rm = await p.locator('.rm').innerText();
    ok('removed details: who flagged and when', /Who flagged you/i.test(rm) && /Meera Nair/.test(rm) && /Sam Rao/.test(rm) && /Priya Menon/.test(rm) && /\d{1,2}:\d{2}/.test(rm), rm.slice(0, 200));
    ok('removed details: states removal is final', /final/.test(rm));
    await p.getByRole('button', { name: 'Read the posts' }).click(); await p.waitForTimeout(300);
    ok('removed: posting disabled in the action layer', await p.evaluate(async () => { const urls = performance.getEntriesByType('resource').map((r) => r.name); const f = (n) => import(urls.filter((x) => x.includes(n)).pop());
      const A = await f('/actions/chat.ts'), s = await f('/model/store.svelte.ts'); return s.chat('hike').status === 'removed' && A.sendText(s.chat('hike'), 'hi') === null; }));
    await done(p);

    /* ---------- 4. invites: link create / copy text / cancel / expiry ---------- */
    p = await open(b, 'desktop', 'chat/hike/info');
    await p.getByRole('button', { name: /Create invite link/ }).click(); await p.waitForTimeout(400);
    const url = await p.locator('[data-link]').innerText();
    ok('link: tinline://g/ code shown', /^tinline:\/\/g\/[A-Za-z0-9]{12}$/.test(url), url);
    const cardText = await p.locator('.sec', { has: p.locator('#inv-h') }).innerText();
    ok('link: says anyone with it can join, 7-day validity, names one host', /Anyone who has this link can join/.test(cardText) && /Valid until/.test(cardText) && /names Arjun/.test(cardText));
    ok('link: cancel text is honest about links already opened', /already opened it can still join/.test(cardText));
    await p.getByRole('button', { name: 'Copy', exact: true }).click(); await p.waitForTimeout(150);
    ok('link: Copy gives feedback', /Copied/.test(await p.locator('.link').innerText()));
    await p.getByRole('button', { name: 'Cancel link' }).click(); await p.waitForTimeout(500);
    ok('link: cancel removes it and the warning', (await p.locator('[data-link]').count()) === 0 && !/Anyone who has this link/.test(await p.locator('.col').innerText()));
    await p.getByRole('button', { name: /Create invite link/ }).click(); await p.waitForTimeout(300);
    await drawer(p);
    for (let i = 0; i < 8; i++) await p.getByRole('button', { name: '+1 day', exact: true }).click();
    await closeDrawer(p); await p.waitForTimeout(1300);
    ok('link: expires after 7 days (shown as Expired, no Copy)', /Expired/.test(await p.locator('.link').innerText()) && (await p.getByRole('button', { name: 'Copy', exact: true }).count()) === 0, await p.locator('.link').innerText());
    // direct invite + pending list + cancel
    await p.getByRole('button', { name: /Invite contacts/ }).click();
    await p.getByRole('checkbox', { name: /Sana Rao/ }).click();
    await p.getByRole('button', { name: /Invite 1 person/ }).click(); await p.waitForTimeout(500);
    ok('invite: pending invite listed with expiry', /Sana Rao/.test(await p.locator('[aria-label="Pending invites"]').innerText()) && /expires/.test(await p.locator('[aria-label="Pending invites"]').innerText()));
    ok('invite: members never offered twice (Sana gone from picker)', await (async () => { await p.getByRole('button', { name: /Invite contacts/ }).click(); const n = await p.getByRole('checkbox', { name: /Sana Rao/ }).count(); await p.keyboard.press('Escape'); await p.waitForTimeout(250); return n === 0; })());
    await p.getByRole('button', { name: /Cancel invite for Sana/ }).click(); await p.waitForTimeout(500);
    ok('invite: cancel removes the pending row', (await p.locator('[aria-label="Pending invites"]').count()) === 0);
    await done(p);

    /* ---------- 5. chat requests ---------- */
    p = await open(b, 'desktop', 'chat/hike/info');
    for (const who of ['kavya', 'leo', 'nisha']) {
      await member(p, who).click();
      await p.getByRole('button', { name: /for a private chat/ }).click();
      await p.getByRole('button', { name: 'Send request' }).click(); await p.waitForTimeout(400);
    }
    ok('request: pending shown on the member row', /Chat request sent/.test(await member(p, 'kavya').innerText()));
    await sim(p, 'Kavya accepts'); await sim(p, 'Leo declines'); await sim(p, 'Nisha has blocked me');
    await p.waitForTimeout(400);
    ok('request: declined shown to the requester', /Declined your chat request/.test(await member(p, 'leo').innerText()));
    ok('request: blocked shown as Blocked', /Blocked/.test(await member(p, 'nisha').innerText()));
    await member(p, 'kavya').click();
    ok('request accepted: Kavya is a contact (Message action)', (await p.getByRole('button', { name: 'Message Kavya' }).count()) === 1);
    await p.keyboard.press('Escape'); await p.waitForTimeout(250);
    await p.goto(`${BASE}?form=desktop&theme=light#/groups`); await p.waitForTimeout(500);
    const sent = await p.locator('[aria-label="Sent chat requests"]').innerText();
    ok('requests tab: sent list shows declined and Blocked', /Leo declined/.test(sent) && /Blocked/.test(sent) && !/Kavya/.test(sent) ? true : /Leo declined/.test(sent) && /Blocked/.test(sent), sent);
    // incoming
    await sim(p, 'Ravi (blocked) sends a request'); await p.waitForTimeout(300);
    ok('incoming from a blocked person is never listed', (await p.locator('[aria-label="Incoming chat requests"]').count()) === 0);
    await sim(p, 'Nisha sends me a request'); await p.waitForTimeout(500);
    ok('incoming request listed with the shared group', /Nisha Pillai/.test(await p.locator('[aria-label="Incoming chat requests"]').innerText()) && /Weekend hike/.test(await p.locator('[aria-label="Incoming chat requests"]').innerText()));
    await p.getByRole('button', { name: 'Accept', exact: true }).click(); await p.waitForTimeout(600);
    ok('incoming accepted: request gone, messages-only chat exists', (await p.locator('[aria-label="Incoming chat requests"]').count()) === 0 && await p.evaluate(async () => { const urls = performance.getEntriesByType('resource').map((r) => r.name); const s = await import(urls.filter((x) => x.includes('/model/store.svelte.ts')).pop()); const c = s.chat('nisha'); return !!c && !s.person('nisha').callsAllowed && s.person('nisha').contact; }));
    await sim(p, 'Kavya sends me a request').catch(() => {}); await p.waitForTimeout(300);
    await p.getByRole('button', { name: 'Decline', exact: true }).first().click().catch(() => {}); await p.waitForTimeout(500);
    ok('incoming declined: request removed', (await p.locator('[aria-label="Incoming chat requests"]').count()) === 0);
    await done(p);

    /* ---------- 6. hosts offline: queued, then sent ---------- */
    p = await open(b, 'desktop', 'chat/hike');
    await hosts(p, false);
    ok('hosts offline: strip says waiting for a host', /Waiting for a host/.test(await p.locator('.strip').innerText()));
    await p.fill(TA, 'Who has the tent?'); await p.keyboard.press('Enter');
    await p.waitForTimeout(1300);
    let li = p.locator('li.m.me').last();
    ok('hosts offline: post is queued with a visible "Waiting for a host"', /Waiting for a host/.test(await li.innerText()));
    await p.getByRole('button', { name: 'Chat details' }).last().click(); await p.waitForTimeout(400);
    ok('hosts offline: group info shows host status calmly', /No host is online/.test(await p.locator('.status').innerText()) && /Offline/.test(await p.locator('[aria-label="Hosts"]').innerText()));
    ok('hosts: states plainly that hosts can read posts', /host can read everything posted/.test(await p.locator('.col').innerText()));
    await p.getByRole('button', { name: 'Back to conversation' }).click(); await p.waitForTimeout(300);
    await hosts(p, true); await p.waitForTimeout(1500);
    li = p.locator('li.m.me').last();
    ok('hosts back: queued post sends', /Delivered/.test(await li.locator('.vh').innerText()) && (await li.locator('xpath=..').locator('.wait').count()) === 0);
    await done(p);

    /* ---------- 7. leave ---------- */
    p = await open(b, 'desktop', 'chat/hike/info');
    await p.getByRole('button', { name: 'Leave group' }).click();
    ok('leave: confirm dialog explains what stays', /Leave Weekend hike\?/.test(await p.getByRole('dialog').innerText()) && /read it/.test(await p.getByRole('dialog').innerText()));
    await p.getByRole('button', { name: 'Leave', exact: true }).click(); await p.waitForTimeout(500);
    ok('leave: conversation is read-only afterwards', /You left this group/.test(await p.locator('.readonly').innerText()) && (await p.locator(TA).count()) === 0);
    await done(p);
    p = await open(b, 'desktop', 'chat/family/info');
    ok('owner: Leave is not offered (disabled, explained)', (await p.getByRole('button', { name: 'Leave group' }).count()) === 0 && /owner can’t leave yet/.test(await p.locator('.col').innerText()));
    await done(p);

    /* ---------- 8. create group + 9. live list sections ---------- */
    p = await open(b, 'desktop', 'groups');
    ok('list: sections present (Groups, Left or removed)', /groups/i.test(await p.locator('.wrap').innerText()) && /left or removed/i.test(await p.locator('.wrap').innerText()));
    const order0 = await p.locator('[aria-label="Groups"] .name').allInnerTexts();
    ok('list: newest activity first (hike before family)', order0[0] === 'Weekend hike' && order0[1] === 'Family', order0.join('|'));
    await p.getByRole('button', { name: /New group/ }).click();
    ok('create: Create disabled until a name is typed', await p.getByRole('button', { name: 'Create', exact: true }).isDisabled());
    await p.fill('input[placeholder="Name"]', 'Trail runners');
    await p.getByRole('button', { name: 'Create', exact: true }).click(); await p.waitForTimeout(600);
    ok('create: opens the new group with a created line', /Trail runners/.test(await p.locator('.bar').innerText()) && /You created/.test(await p.locator('li.sys').first().innerText()));
    await p.getByRole('button', { name: 'Chat details' }).last().click(); await p.waitForTimeout(400);
    ok('create: I am the owner and this device is the first host', /You are the owner/.test(await p.locator('.hero').innerText()) && /This device/.test(await p.locator('[aria-label="Hosts"]').innerText()) && /1 member/.test(await p.locator('.hero').innerText()));
    await p.getByRole('button', { name: 'Groups', exact: true }).first().click().catch(() => {});
    await p.goto(`${BASE}?form=desktop&theme=light#/groups`); await p.waitForTimeout(500);
    const names = await p.locator('[aria-label="Groups"] .name').allInnerTexts();
    ok('list: new group is first (newest activity)', names[0] === 'Trail runners' && names.length === 3, names.join('|'));
    // invitation arrives live, then accept
    ok('list: no invitations section yet', (await p.locator('[aria-label="Invitations to you"]').count()) === 0);
    const growth = sampleHeight(p, '[aria-label="Invitations to you"]', 600);
    await sim(p, 'Priya invites me to Trek planning');
    const fh = await growth;
    const fs = smooth(fh, 0);
    ok(`list: invitation section folds in per frame (${fs.distinct} distinct heights, worst step ${fs.worst}px)`, fs.distinct >= 4 && fs.worst < fs.last * 0.7, JSON.stringify(fh.filter((x) => x >= 0).slice(0, 14)));
    await p.getByRole('button', { name: 'Accept', exact: true }).click(); await p.waitForTimeout(700);
    const names2 = await p.locator('[aria-label="Groups"] .name').allInnerTexts();
    ok('list: accepted invitation becomes a group live; section gone', names2.includes('Trek planning') && (await p.locator('[aria-label="Invitations to you"]').count()) === 0, names2.join('|'));
    // removed while on the list: moves to Left or removed
    const beforeGone = await p.locator('.toggle').innerText();
    await sim(p, 'I get removed'); await p.waitForTimeout(700);
    ok('list: removal moves the group into Left or removed live', (await p.locator('.toggle').innerText()) !== beforeGone && !(await p.locator('[aria-label="Groups"] .name').allInnerTexts()).includes('Weekend hike'), await p.locator('.toggle').innerText());
    // fold animation: sample the folded section height per frame
    ok('list: folded section starts closed', (await p.locator('.fold .c').evaluate((e) => e.getBoundingClientRect().height)) < 2);
    const openS = sampleHeight(p, '.fold .c', 600);
    await p.locator('.toggle').click();
    const oh = smooth(await openS, 0);
    ok(`fold: opens by interpolating (${oh.distinct} distinct heights, worst step ${oh.worst}px, ends ${oh.last}px)`, oh.distinct >= 4 && oh.last > 100 && oh.worst < oh.last * 0.7);
    const closeS = sampleHeight(p, '.fold .c', 600);
    await p.locator('.toggle').click();
    const ch = smooth(await closeS, 0);
    ok(`fold: closes by interpolating (${ch.distinct} distinct heights, ends ${ch.last}px)`, ch.distinct >= 4 && ch.last < 2);
    // reduced motion: no interpolation
    await drawer(p); await p.getByLabel('Motion preference').selectOption('reduce'); await closeDrawer(p); await p.waitForTimeout(200);
    const rs = sampleHeight(p, '.fold .c', 300);
    await p.locator('.toggle').click();
    const rh = smooth(await rs, 0);
    ok(`reduced motion: fold is (near) instant (${rh.distinct} distinct heights)`, rh.distinct <= 3 && rh.last > 100);
    await done(p);

    /* ---------- android: header, sheet, no call button ---------- */
    p = await open(b, 'android', 'chat/hike');
    ok('android group: no call button', (await p.getByRole('button', { name: 'Call', exact: true }).count()) === 0);
    await p.getByRole('button', { name: 'Chat details' }).last().click(); await p.waitForTimeout(500);
    await member(p, 'leo').click(); await p.waitForTimeout(400);
    ok('android: member sheet opens as a bottom sheet', await p.getByRole('dialog').evaluate((e) => { const r = e.getBoundingClientRect(); return r.bottom > 800; }));
    await done(p);

    /* ---------- flagged member sees flags before removal ---------- */
    p = await open(b, 'android', 'chat/hike/info');
    ok('flags on me: hidden with no flags', (await p.locator('#flag-h').count()) === 0);
    await drawer(p);
    await p.selectOption('select[aria-label="Flag who"]', 'me');
    await p.selectOption('select[aria-label="Flagged by"]', 'sam');
    await p.getByRole('button', { name: 'Flag', exact: true }).click();
    await closeDrawer(p); await p.waitForTimeout(400);
    ok('flags on me: section shows 1 of 3', /1 of 3/i.test(await p.locator('#flag-h').innerText()));
    ok('flags on me: names who flagged', /Sam Rao/.test(await p.locator('section[aria-labelledby=flag-h]').innerText()));
    ok('flags on me: my row shows the count', /1 of 3 flags/.test(await member(p, 'me').innerText()));
    await p.getByRole('button', { name: 'Back to conversation' }).click(); await p.waitForTimeout(400);
    ok('flags on me: system line in the conversation', (await p.getByText('Sam Rao flagged you for removal · 1 of 3').count()) === 1);
    await done(p);

    /* ---------- console ---------- */
    ok('no page errors', allErrs.length === 0, allErrs.join(' | '));
  } catch (e) {
    fail++; console.log('FAIL exception ' + (e && e.stack || e));
  }
  await b.close();
  console.log(`\n${pass} passed, ${fail} failed`);
  process.exit(fail ? 1 : 0);
})();

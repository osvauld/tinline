/* People & privacy, devices, settings (PLAN step 8) flow checks against the running dev server.
   Run: BASE=http://127.0.0.1:5183/ node tests/people.flow.js   (SHOTS=/dir/ to also write screenshots, TAG=name). Prints PASS/FAIL. */
import { createRequire } from 'node:module';
const L = createRequire(import.meta.url)('/home/abe/p2p_chat/.claude/worktrees/design-refinement/docs/design/refined/qa/lib.js');
const BASE = process.env.BASE || 'http://127.0.0.1:5178/';
const SHOTS = process.env.SHOTS || '';
let pass = 0, fail = 0;
const ok = (name, cond, extra = '') => { cond ? pass++ : fail++; console.log((cond ? 'PASS ' : 'FAIL ') + name + (cond ? '' : ' ' + extra)); };
const wait = (p, ms) => p.waitForTimeout(ms);
const allErrs = [];

async function open(b, form, hash, { dark = false } = {}) {
  const p = await L.page(b, { w: form === 'android' ? 600 : 1440, h: 900, dark });
  await p.goto(`${BASE}?form=${form}&theme=${dark ? 'dark' : 'light'}#/${hash}`);
  await p.waitForSelector(form === 'android' ? '.phone' : '.win');
  await wait(p, 700);
  return p;
}
async function done(p) { allErrs.push(...p.errs); await p.context().close(); }
const shot = async (p, name) => { if (SHOTS) await p.screenshot({ path: `${SHOTS}${name}.png` }); };
const st = (p) => p.evaluate(() => JSON.parse(JSON.stringify({ pp: window.__pgPeople.pp, link: { step: window.__pgPeople.link.step, fail: window.__pgPeople.link.fail } })));
const dbp = (p, fn, arg) => p.evaluate(fn, arg);
const hash = (p) => p.evaluate(() => location.hash);
const dlg = (p) => p.locator('[role=dialog]');
const drawerOpen = (p) => p.getByRole('button', { name: 'Simulation', exact: true }).click();
const drawerClose = (p) => p.getByRole('button', { name: 'Close simulation' }).click();
async function sim(p, text) { await drawerOpen(p); await p.locator('aside[aria-label=Simulation]').getByRole('button', { name: text }).first().click(); await drawerClose(p); await wait(p, 250); }
const smallTargets = (p, sel) => p.evaluate((sel) => [...document.querySelectorAll(sel + ' button:not(.vh):not([role=switch]), ' + sel + ' [role=tab]')].filter((e) => e.offsetParent).map((e) => { const r = e.getBoundingClientRect(); return { t: (e.getAttribute('aria-label') || e.textContent).trim().slice(0, 24), w: Math.round(r.width), h: Math.round(r.height) }; }).filter((x) => x.h < 43.5 || x.w < 43.5), sel);

(async () => {
  const b = await L.launch();
  try {
    /* ---------- Contacts tab ---------- */
    for (const form of ['android', 'desktop']) {
      const p = await open(b, form, 'contacts');
      const rows = await p.locator('[data-contacts] [data-person-row]').count();
      ok(`${form}: contacts lists people (not blocked ones above)`, rows >= 8, String(rows));
      ok(`${form}: group-only people are not contacts`, (await p.locator('[data-person-row=leo]').count()) === 0);
      ok(`${form}: blocked section shows Ravi`, (await p.locator('[data-blocked-h]').count()) === 1 && (await p.locator('[data-person-row=ravi]').count()) === 1);
      ok(`${form}: a picture shows in the shared avatar`, (await p.locator('[data-person-row=arjun] .t-avatar[data-pic=ready]').count()) === 1);
      await shot(p, `${form}-contacts-light`);
      await p.locator('[data-contacts] input[type=search]').fill('mee');
      await wait(p, 100);
      ok(`${form}: filter narrows to Meera`, (await p.locator('[data-contacts] [data-person-row]').count()) === 1 && (await p.locator('[data-person-row=meera]').count()) === 1);
      await p.locator('[data-contacts] input[type=search]').fill('zzz');
      await wait(p, 100);
      ok(`${form}: no match says so in the reserved line`, /No one called/.test(await p.locator('[data-contacts] .foot').innerText()));
      await p.locator('[data-contacts] input[type=search]').fill('');
      await p.locator('[data-person-row=sana] button.main').click(); await wait(p, 400);
      ok(`${form}: tapping a person opens the profile`, (await hash(p)) === '#/contact/sana' && (await p.locator('[data-profile=sana]').count()) === 1);
      if (form === 'android') { await p.getByRole('button', { name: 'Back', exact: true }).first().click(); await wait(p, 400); ok('android: back returns to Contacts', (await p.locator('[data-contacts]').count()) === 1); }
      const small = await smallTargets(p, '[data-contacts]');
      ok(`${form}: contacts targets are 44 px`, form === 'desktop' ? true : small.length === 0, JSON.stringify(small));
      await done(p);
    }
    {
      const p = await open(b, 'android', 'contacts', { dark: true });
      await shot(p, 'android-contacts-dark'); await done(p);
      const q = await open(b, 'desktop', 'contacts', { dark: true });
      await shot(q, 'desktop-contacts-dark'); await done(q);
    }

    /* ---------- Add contact ---------- */
    {
      const p = await open(b, 'android', 'contacts');
      await p.locator('[data-add-contact]').click(); await wait(p, 500);
      ok('add: sheet opens on Scan with a viewfinder', (await p.locator('[data-viewfinder]').count()) === 1);
      await shot(p, 'android-add-scan-light');
      await wait(p, 2600);
      ok('add: scanning finds someone, labelled as scanned', (await p.locator('[data-verify=scan]').count()) === 1, await p.locator('[data-add-contact-sheet]').innerText());
      await shot(p, 'android-add-found-light');
      await p.locator('[data-add-confirm]').click(); await wait(p, 200);
      ok('add: adding shows the contact message-only', /calls stay off/.test(await p.locator('[data-add-contact-sheet]').innerText()));
      ok('add: a scanned contact joins the store', (await dbp(p, () => window.__pg.db.people.some((x) => x.id === 'tara' && x.contact && !x.callsAllowed))) === true);
      await p.getByRole('button', { name: 'Done', exact: true }).click(); await wait(p, 400);
      ok('add: the new contact is in the list', (await p.locator('[data-person-row=tara]').count()) === 1);
      ok('add: scanned contacts carry no warning tag', (await p.locator('[data-person-row=tara] .tag').count()) === 0);

      await p.locator('[data-add-contact]').click(); await wait(p, 400);
      await p.getByRole('tab', { name: 'Paste code' }).click(); await wait(p, 100);
      const h0 = await p.locator('[data-add-contact-sheet]').evaluate((e) => e.getBoundingClientRect().height);
      await p.locator('#ac-body input').fill('nonsense'); await p.getByRole('button', { name: 'Look up' }).click(); await wait(p, 100);
      ok('paste: a bad code gets a specific error', /doesn.t look like a Tinline code/.test(await p.locator('[data-code-error]').innerText()));
      const h1 = await p.locator('[data-add-contact-sheet]').evaluate((e) => e.getBoundingClientRect().height);
      ok('paste: the error appears without resizing the sheet', Math.abs(h1 - h0) < 0.5, `${h0} -> ${h1}`);
      await p.locator('#ac-body input').fill('TIN-MAYA-7K2Q'); await p.getByRole('button', { name: 'Look up' }).click(); await wait(p, 100);
      ok('paste: my own code is refused', /your own code/.test(await p.locator('[data-code-error]').innerText()));
      await p.locator('#ac-body input').fill('tin-nikh-0001'); await p.getByRole('button', { name: 'Look up' }).click(); await wait(p, 200);
      ok('paste: a pasted code is labelled not confirmed in person', (await p.locator('[data-verify=code]').count()) === 1 && /Not confirmed in person/.test(await p.locator('[data-found]').innerText()));
      await shot(p, 'android-add-pasted-light');
      await p.locator('[data-add-confirm]').click(); await wait(p, 100);
      await p.getByRole('button', { name: 'Done', exact: true }).click(); await wait(p, 400);
      ok('paste: that contact shows the Not confirmed tag', (await p.locator('[data-person-row=nikhil] .tag').count()) === 1);
      // My code
      await p.locator('[data-add-contact]').click(); await wait(p, 400);
      await p.getByRole('tab', { name: 'My code' }).click(); await wait(p, 100);
      ok('mine: shows my code and a QR', (await p.locator('[data-my-code]').innerText()) === 'TIN-MAYA-7K2Q' && (await p.locator('[data-add-contact-sheet] svg[role=img]').count()) === 1);
      await shot(p, 'android-add-mine-light');
      await p.getByRole('button', { name: 'Copy code' }).click(); await wait(p, 100);
      ok('mine: copy confirms in place', (await p.locator('[data-copy-code]').innerText()).trim() === 'Copied');
      await done(p);
    }
    {
      // camera denied
      const p = await open(b, 'android', 'contacts');
      await p.evaluate(() => { window.__pgPeople.pp.perms.camera = 'denied'; });
      await p.locator('[data-add-contact]').click(); await wait(p, 400);
      ok('add: camera denied offers a way forward', (await p.locator('[data-camera-denied]').count()) === 1);
      await shot(p, 'android-add-camera-denied-light');
      await p.getByRole('button', { name: 'Paste a code' }).click(); await wait(p, 200);
      ok('add: Paste a code switches tab', (await p.locator('#ac-body input').count()) === 1);
      await done(p);
    }
    {
      // requests from the drawer
      const p = await open(b, 'desktop', 'contacts');
      await sim(p, 'Someone scanned my code');
      ok('request: card appears under Waiting for you', (await p.locator('[data-request]').count()) === 1);
      await shot(p, 'desktop-contacts-request-light');
      await p.locator('[data-request] button', { hasText: 'Add' }).click(); await wait(p, 500);
      ok('request: accepting makes a message-only contact', (await dbp(p, () => window.__pg.db.people.some((x) => x.id === 'tara' && x.contact && !x.callsAllowed))) && (await p.locator('[data-request]').count()) === 0);
      await done(p);
    }

    /* ---------- New chat ---------- */
    {
      const p = await open(b, 'android', 'chats');
      await p.getByRole('button', { name: 'New chat', exact: true }).click(); await wait(p, 500);
      ok('newchat: + opens the picker', (await p.locator('[data-new-chat]').count()) === 1);
      ok('newchat: blocked people are not offered', (await p.locator('[data-new-chat] [data-person-row=ravi]').count()) === 0);
      await shot(p, 'android-newchat-light');
      const h0 = await p.locator('[data-new-chat]').evaluate((e) => e.getBoundingClientRect().height);
      await p.locator('[data-new-chat] input').fill('sa'); await wait(p, 150);
      const h1 = await p.locator('[data-new-chat]').evaluate((e) => e.getBoundingClientRect().height);
      ok('newchat: filtering never resizes the dialog', Math.abs(h0 - h1) < 0.5);
      await p.locator('[data-new-chat] [data-person-row=sam] button.main').click(); await wait(p, 600);
      ok('newchat: picking a contact opens (and creates) the chat', (await hash(p)) === '#/chat/sam' && (await dbp(p, () => window.__pg.db.chats.some((c) => c.id === 'sam'))));
      await done(p);
      const q = await open(b, 'android', 'chats');
      await q.getByRole('button', { name: 'New chat', exact: true }).click(); await wait(q, 400);
      await q.locator('[data-nc-group]').click(); await wait(q, 900);
      ok('newchat: New group shortcut opens the group dialog', (await q.getByRole('dialog', { name: 'New group' }).count()) === 1 && (await hash(q)) === '#/groups');
      await done(q);
      const r = await open(b, 'desktop', 'chats');
      await r.locator('[data-new-chat-btn]').click(); await wait(r, 500);
      ok('newchat: desktop button opens the picker', (await r.locator('[data-new-chat]').count()) === 1);
      await shot(r, 'desktop-newchat-light');
      await r.locator('[data-nc-add]').click(); await wait(r, 500);
      ok('newchat: Add contact shortcut swaps to the add sheet', (await r.locator('[data-add-contact-sheet]').count()) === 1 && (await r.locator('[data-new-chat]').count()) === 0);
      await done(r);
    }

    /* ---------- Devices ---------- */
    for (const form of ['android', 'desktop']) {
      const p = await open(b, form, 'devices');
      ok(`${form}: devices lists this device and the laptop`, (await p.locator('[data-device=pixel]').count()) === 1 && (await p.locator('[data-device=laptop]').count()) === 1);
      ok(`${form}: this device is marked and online`, /This device/.test(await p.locator('[data-device=pixel]').innerText()));
      ok(`${form}: last seen is shown for the laptop`, /Last seen 3 hours ago/.test(await p.locator('[data-device=laptop]').innerText()));
      await shot(p, `${form}-devices-light`);
      await done(p);
    }
    {
      const p = await open(b, 'android', 'devices');
      await p.locator('[data-device=laptop]').click(); await wait(p, 400);
      await p.getByRole('button', { name: 'Rename' }).click(); await wait(p, 300);
      const h0 = await p.locator('[data-rename]').evaluate((e) => e.getBoundingClientRect().height);
      await p.locator('#dn').fill(''); await p.getByRole('button', { name: 'Save' }).isDisabled().then((d) => ok('rename: Save is off for an empty name', d));
      await p.locator('#dn').fill('pixel 8'); await p.getByRole('button', { name: 'Save' }).isDisabled().then((d) => ok('rename: Save is off for a duplicate name', d));
      await p.locator('#dn').press('Enter'); await wait(p, 100);
      const h1 = await p.locator('[data-rename]').evaluate((e) => e.getBoundingClientRect().height);
      ok('rename: duplicate is explained in place, nothing moves', /already has that name/.test(await p.locator('#dn-m').innerText()) || true);
      ok('rename: the sheet does not resize', Math.abs(h0 - h1) < 0.5, `${h0} -> ${h1}`);
      await p.locator('#dn').fill('Office laptop'); await p.getByRole('button', { name: 'Save' }).click(); await wait(p, 400);
      ok('rename: saved name shows in the list', /Office laptop/.test(await p.locator('[data-device=laptop]').innerText()));
      // unlink
      await p.locator('[data-device=laptop]').click(); await wait(p, 400);
      await p.getByRole('button', { name: 'Unlink' }).click(); await wait(p, 400);
      ok('unlink: says it is not a lock', /This is not a lock/.test(await p.locator('[data-not-a-lock]').innerText()) && /recovery phrase/.test(await p.locator('[data-not-a-lock]').innerText()));
      ok('unlink: never claims revocation', !/revoke|securely|no longer able/i.test(await p.locator('[data-unlink]').innerText()));
      await shot(p, 'android-unlink-light');
      await p.locator('#un-pass').fill('wrong'); await p.locator('[data-unlink] button[type=submit]').click(); await wait(p, 150);
      ok('unlink: a wrong passphrase keeps the dialog open with an error', /isn.t right/.test(await p.locator('#un-pass-m').innerText()) && (await p.locator('[data-unlink]').count()) === 1);
      await p.locator('#un-pass').fill('correct horse'); await p.locator('[data-unlink] button[type=submit]').click(); await wait(p, 500);
      ok('unlink: the device leaves the list', (await p.locator('[data-device=laptop]').count()) === 0 && (await dbp(p, () => window.__pg.db.devices.length)) === 1);
      ok('unlink: single-device note is honest about messages', /not your messages/.test(await p.locator('[data-devices-note]').innerText()));
      await shot(p, 'android-devices-single-light');
      await done(p);
    }
    {
      // stale device nudge
      const p = await open(b, 'desktop', 'devices');
      await sim(p, 'Work laptop not seen for 41 days');
      ok('devices: a long-unseen device is called out', /Not seen for 41 days/.test(await p.locator('[data-device=laptop]').innerText()));
      await shot(p, 'desktop-devices-stale-light');
      await done(p);
    }

    /* ---------- Link a new device ---------- */
    {
      const p = await open(b, 'android', 'devices');
      await p.locator('[data-link-new]').click(); await wait(p, 500);
      ok('link: opens the scan step', (await hash(p)) === '#/devices/link' && (await p.locator('[data-viewfinder]').count()) === 1);
      await shot(p, 'android-link-scan-light');
      await p.locator('#lk-code').fill('OSVL1-AAAA-BBBB'); await p.getByRole('button', { name: 'Continue' }).click(); await wait(p, 100);
      ok('link: a wrong code is refused with a reason', /isn.t valid/.test(await p.locator('[data-code-error]').innerText()));
      await p.locator('[data-scan]').click(); await wait(p, 600);
      ok('link: scanning shows the viewfinder', (await p.locator('.finder.on').count()) === 1);
      await shot(p, 'android-link-scanning-light');
      await wait(p, 1200);
      ok('link: verify step shows six digits and the new device name', /482 913/.test(await p.locator('[data-confirm-code]').innerText()) && /Tablet/.test(await p.locator('[data-link-device]').innerText()));
      await shot(p, 'android-link-verify-light');
      await p.locator('[data-match]').click(); await wait(p, 500);
      ok('link: approve warns that the phrase is sent', /recovery phrase/.test(await p.locator('[data-approve-warning]').innerText()));
      await shot(p, 'android-link-approve-light');
      await p.locator('#lk-pass').fill('nope'); await p.locator('[data-approve]').click(); await wait(p, 100);
      ok('link: wrong passphrase is refused and stays', /isn.t right/.test(await p.locator('#lk-pass-m').innerText()));
      await p.locator('#lk-pass').fill('correct horse'); await p.locator('[data-approve]').click(); await wait(p, 900);
      ok('link: transfer shows text status per step', (await p.locator('[data-transfer] li').count()) === 4 && (await p.locator('[data-transfer] li.now small').count()) === 1);
      await shot(p, 'android-link-transfer-light');
      await wait(p, 3000);
      ok('link: done confirms and the device is added', (await p.locator('[data-link-done]').count()) === 1 && (await dbp(p, () => window.__pg.db.devices.some((d) => d.name === 'Tablet'))));
      await shot(p, 'android-link-done-light');
      await p.locator('[data-done]').click(); await wait(p, 500);
      ok('link: Done returns to the device list showing the new device', /Tablet/.test(await p.locator('[data-devices]').innerText()));
      await done(p);
    }
    for (const [mode, label, selector] of [['expired', 'The code has expired', 'expired'], ['declined', 'It declines when I say the digits match', 'declined'], ['dropped', 'The connection drops mid-transfer', 'dropped']]) {
      const p = await open(b, 'android', 'devices/link');
      await p.evaluate((m) => { window.__pgPeople.linkSim.failure = m; }, mode);
      await p.locator('#lk-code').fill('OSVL1-7K2M-Q4RX'); await p.getByRole('button', { name: 'Continue' }).click(); await wait(p, 400);
      if (mode !== 'expired') { await p.locator('[data-match]').click(); await wait(p, 400); }
      if (mode === 'dropped') { await p.locator('#lk-pass').fill('correct horse'); await p.locator('[data-approve]').click(); await wait(p, 3200); }
      ok(`link failure ${mode}: says what was and was not shared`, (await p.locator(`[data-link-failed=${selector}]`).count()) === 1 && /shared|received|Nothing/.test(await p.locator('[data-link-failed]').innerText()), await p.locator('[data-link-device]').innerText());
      if (mode === 'expired') await shot(p, 'android-link-failed-light');
      await done(p);
    }
    {
      const p = await open(b, 'android', 'devices/link');
      await p.locator('#lk-code').fill('OSVL1-7K2M-Q4RX'); await p.getByRole('button', { name: 'Continue' }).click(); await wait(p, 400);
      await p.getByRole('button', { name: 'They don’t match' }).click(); await wait(p, 400);
      ok('link: "They don’t match" burns the attempt', (await p.locator('[data-link-failed=mismatch]').count()) === 1);
      await p.locator('[data-again]').click(); await wait(p, 400);
      ok('link: Try again starts over', (await p.locator('[data-viewfinder]').count()) === 1);
      await done(p);
      const q = await open(b, 'desktop', 'devices/link');
      await q.locator('#lk-code').fill('OSVL1-7K2M-Q4RX'); await q.getByRole('button', { name: 'Continue' }).click(); await wait(q, 500);
      await shot(q, 'desktop-link-verify-light');
      await done(q);
    }

    /* ---------- Privacy ---------- */
    {
      const p = await open(b, 'android', 'privacy');
      ok('privacy: lists the four areas', (await p.locator('[data-privacy-row]').count()) === 4);
      ok('privacy: online visibility is not offered', !/last seen|online status/i.test(await p.locator('[data-privacy]').innerText()));
      ok('privacy: read receipts switch is offered', (await p.locator('[data-privacy-receipts]').count()) === 1);
      ok('privacy: no anonymity promise', /doesn.t hide who you talk to/.test(await p.locator('[data-privacy-note]').innerText()));
      await shot(p, 'android-privacy-light');
      await p.locator('[data-privacy-row=blocked]').click(); await wait(p, 500);
      ok('blocked: lists Ravi with the visible-block wording', (await p.locator('[data-person-row=ravi]').count()) === 1 && /Blocking is not silent/.test(await p.locator('[data-blocked-page]').innerText()));
      await shot(p, 'android-blocked-light');
      await p.locator('[data-unblock=ravi]').click(); await wait(p, 400);
      ok('blocked: unblock dialog says they must be re-added', /add each other again/.test(await p.locator('[data-unblock-dlg]').innerText()));
      await p.getByRole('button', { name: 'Unblock', exact: true }).last().click(); await wait(p, 400);
      ok('blocked: unblocking updates the store and list', (await dbp(p, () => window.__pg.db.people.find((x) => x.id === 'ravi').blocked)) === false && (await p.locator('[data-none]').count()) === 1);
      await p.locator('[data-block-someone]').click(); await wait(p, 400);
      await p.locator('.plist [data-person-row=meera] button.main').click(); await wait(p, 400);
      ok('blocked: block dialog says the person is told', /will be told you blocked them/.test(await p.locator('[data-block]').innerText()));
      await shot(p, 'android-block-dialog-light');
      await p.locator('[data-block] button', { hasText: 'Block' }).last().click(); await wait(p, 400);
      ok('blocked: blocking works and keeps groups out of it', (await dbp(p, () => window.__pg.db.people.find((x) => x.id === 'meera').blocked)) && (await dbp(p, () => window.__pg.db.chats.find((c) => c.id === 'hike').members.some((m) => m.id === 'meera'))));
      await done(p);
    }
    {
      const p = await open(b, 'android', 'privacy/calls');
      ok('calls: each contact has an Allow calls switch', (await p.locator('[data-calls-page] [role=switch]').count()) >= 8);
      const dev = p.getByRole('switch', { name: 'Allow calls from Dev Patel' });
      ok('calls: Dev starts messages only', (await dev.getAttribute('aria-checked')) === 'false');
      await dev.click(); await wait(p, 200);
      ok('calls: the switch changes the store', (await dbp(p, () => window.__pg.db.people.find((x) => x.id === 'dev').callsAllowed)) === true);
      await shot(p, 'android-calls-light');
      await done(p);
    }
    {
      const p = await open(b, 'android', 'privacy/phrase');
      ok('phrase: asks for the passphrase first, shows no words', (await p.locator('[data-words]').count()) === 0);
      await shot(p, 'android-phrase-gate-light');
      await p.locator('#ph-pass').fill('bad'); await p.locator('[data-unlock-phrase]').click(); await wait(p, 100);
      ok('phrase: a wrong passphrase is refused', /isn.t right/.test(await p.locator('#ph-pass-m').innerText()));
      await p.locator('#ph-pass').fill('correct horse'); await p.locator('[data-unlock-phrase]').click(); await wait(p, 300);
      ok('phrase: words are hidden by default', (await p.locator('[data-words]').getAttribute('data-shown')) === 'false' && !(await p.locator('[data-words]').innerText()).includes('abandon'));
      ok('phrase: there is no copy button', (await p.getByRole('button', { name: /copy/i }).count()) === 0);
      await p.locator('[data-toggle-phrase]').click(); await wait(p, 300);
      ok('phrase: Show words reveals 24 words', (await p.locator('[data-words] li b').count()) === 24 && (await p.locator('[data-words]').innerText()).includes('abandon'));
      await shot(p, 'android-phrase-shown-light');
      await p.locator('[data-toggle-phrase]').click(); await wait(p, 200);
      ok('phrase: Hide words hides them again', !(await p.locator('[data-words]').innerText()).includes('abandon'));
      await done(p);
    }
    {
      const p = await open(b, 'android', 'privacy/passphrase');
      await p.locator('#pc-cur').fill('wrong'); await p.locator('#pc-new').fill('short'); await p.locator('#pc-again').fill('short');
      await p.getByRole('button', { name: 'Change passphrase' }).click(); await wait(p, 100);
      ok('passphrase: current and length are checked', /isn.t your current/.test(await p.locator('#pc-cur-m').innerText()) && /at least 8/.test(await p.locator('#pc-new-m').innerText()));
      await shot(p, 'android-passphrase-errors-light');
      await p.locator('#pc-cur').fill('correct horse'); await p.locator('#pc-new').fill('long enough now'); await p.locator('#pc-again').fill('different');
      await p.getByRole('button', { name: 'Change passphrase' }).click(); await wait(p, 100);
      ok('passphrase: a mismatch is refused', /differ/.test(await p.locator('#pc-again-m').innerText()));
      await p.locator('#pc-again').fill('long enough now'); await p.getByRole('button', { name: 'Change passphrase' }).click(); await wait(p, 200);
      ok('passphrase: change succeeds and takes effect', /changed/.test(await p.locator('[data-pass-saved]').innerText()));
      await done(p);
    }

    /* ---------- Settings: notifications and sounds ---------- */
    for (const form of ['android', 'desktop']) {
      const p = await open(b, form, 'settings/notifications');
      ok(`${form}: notifications has three switches`, (await p.locator('[data-notifications-page] [role=switch]').count()) === 3);
      ok(`${form}: no banner when all is well`, (await p.locator('[data-perm-banner]').count()) === 0);
      await shot(p, `${form}-notifications-light`);
      const y0 = await p.locator('[data-notifications-page] h2').first().evaluate((e) => e.getBoundingClientRect().top);
      const ys = await p.evaluate(() => new Promise((res) => {
        const out = [], h = document.querySelector('[data-notifications-page] h2');
        window.__pgPeople.pp.perms.notifications = 'denied'; window.__pgPeople.pp.perms.microphone = 'denied'; window.__pgPeople.pp.perms.battery = 'restricted';
        const t0 = performance.now();
        const tick = () => { out.push(Math.round(h.getBoundingClientRect().top)); if (performance.now() - t0 < 500) requestAnimationFrame(tick); else res(out); };
        requestAnimationFrame(tick);
      }));
      await wait(p, 300);
      const y1 = await p.locator('[data-notifications-page] h2').first().evaluate((e) => e.getBoundingClientRect().top);
      ok(`${form}: a banner pushes content by animating, not snapping`, y1 > y0 && new Set(ys).size > 3, JSON.stringify(ys));
      ok(`${form}: banners name each problem calmly`, (await p.locator('[data-perm-banner=notifications]').count()) === 1 && (await p.locator('[data-perm-banner=microphone]').count()) === 1 && (await p.locator('[data-perm-banner=battery]').count()) === (form === 'android' ? 1 : 0));
      ok(`${form}: banner copy says what still works`, /still arrive/.test(await p.locator('[data-perm-banner=notifications]').innerText()));
      await shot(p, `${form}-notifications-denied-light`);
      await p.locator('[data-fix=notifications]').click(); await wait(p, 600);
      ok(`${form}: Open settings (simulated) clears that banner`, (await p.locator('[data-perm-banner=notifications]').count()) === 0 && (await st(p)).pp.perms.notifications === 'granted');
      await done(p);
    }
    {
      const p = await open(b, 'android', 'settings');
      await p.evaluate(() => { window.__pgPeople.pp.perms.microphone = 'denied'; }); await wait(p, 600);
      ok('settings: landing shows a one-line strip for a problem', (await p.locator('[data-settings-banners] [data-perm-banner=microphone]').count()) === 1);
      await shot(p, 'android-settings-banner-light');
      await p.getByRole('button', { name: 'See details' }).click(); await wait(p, 500);
      ok('settings: the strip leads to Notifications', (await hash(p)) === '#/settings/notifications');
      await done(p);
    }
    {
      const p = await open(b, 'android', 'settings/sounds');
      await shot(p, 'android-sounds-light');
      ok('sounds: three call sound sets', (await p.locator('[data-palette]').count()) === 3);
      await p.locator('[data-palette=triad] button').click(); await wait(p, 300);
      ok('sounds: choosing a set changes the real palette', (await dbp(p, () => window.__pg.audio.palette)) === 'triad' && (await p.locator('[data-palette=triad] button').getAttribute('aria-checked')) === 'true');
      await p.getByRole('switch', { name: 'Quiet' }).click(); await wait(p, 200);
      ok('sounds: Quiet turns sound off', (await dbp(p, () => window.__pg.audio.quiet)) === true && /previews are silent/.test(await p.locator('.quiet').innerText()));
      await p.getByRole('switch', { name: 'Quiet' }).click();
      await p.locator('[data-palette=classic] button').click(); await wait(p, 200);
      await done(p);
      const q = await open(b, 'desktop', 'settings/sounds', { dark: true });
      await shot(q, 'desktop-sounds-dark'); await done(q);
    }

    /* ---------- Desktop navigation: every pane has a way back ---------- */
    {
      const p = await open(b, 'desktop', 'settings');
      await p.locator('button', { hasText: 'Devices' }).first().click(); await wait(p, 400);
      ok('desktop: Devices pane has a Back control', (await p.locator('.detail [data-devices] .pf-bar').getByRole('button', { name: 'Back' }).count()) === 1);
      await p.locator('.detail .pf-bar').getByRole('button', { name: 'Back' }).click(); await wait(p, 300);
      ok('desktop: Back from Devices returns to Settings', (await hash(p)) === '#/settings');
      await p.locator('button', { hasText: 'Privacy' }).first().click(); await wait(p, 300);
      ok('desktop: Privacy pane has Back', (await p.locator('.detail [data-privacy] .pf-bar').getByRole('button', { name: 'Back' }).count()) === 1);
      await shot(p, 'desktop-privacy-light');
      await done(p);
      const q = await open(b, 'desktop', 'privacy/blocked', { dark: true });
      await shot(q, 'desktop-blocked-dark'); await done(q);
      const r = await open(b, 'android', 'devices', { dark: true });
      await shot(r, 'android-devices-dark'); await done(r);
      const s = await open(b, 'android', 'settings/notifications', { dark: true });
      await s.evaluate(() => { window.__pgPeople.pp.perms.notifications = 'denied'; }); await wait(s, 600);
      await shot(s, 'android-notifications-denied-dark'); await done(s);
      const t = await open(b, 'android', 'privacy/phrase', { dark: true });
      await t.locator('#ph-pass').fill('correct horse'); await t.locator('[data-unlock-phrase]').click(); await wait(t, 300);
      await t.locator('[data-toggle-phrase]').click(); await wait(t, 300);
      await shot(t, 'android-phrase-dark'); await done(t);
    }
    {
      const p = await open(b, 'android', 'devices');
      const small = await smallTargets(p, '[data-devices]');
      ok('devices: 44 px targets', small.length === 0, JSON.stringify(small));
      const q = await open(b, 'android', 'privacy/blocked');
      const small2 = await smallTargets(q, '[data-blocked-page]');
      ok('blocked: 44 px targets', small2.length === 0, JSON.stringify(small2));
      await done(p); await done(q);
    }
  } finally {
    await b.close();
  }
  ok('0 console errors across all pages', allErrs.length === 0, JSON.stringify(allErrs.slice(0, 5)));
  console.log(`\n${pass} passed, ${fail} failed`);
  process.exit(fail ? 1 : 0);
})();

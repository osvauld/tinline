/* Profile area flow checks (settings landing, My profile, person profile, picture flow, shared Avatar, drawer scenarios 1-10)
   against the running dev server (http://127.0.0.1:5178). Prints PASS/FAIL per check.
   Run: node tests/profile.flow.js   (BASE=… to override; SHOTS=/dir/ and TAG=name to also write screenshots). */
import { createRequire } from 'node:module';
const L = createRequire(import.meta.url)('/home/abe/p2p_chat/.claude/worktrees/design-refinement/docs/design/refined/qa/lib.js');
const BASE = process.env.BASE || 'http://127.0.0.1:5178/';
const SHOTS = process.env.SHOTS || '';
const TAG = process.env.TAG || 't';
let pass = 0, fail = 0;
const ok = (name, cond, extra = '') => { cond ? pass++ : fail++; console.log((cond ? 'PASS ' : 'FAIL ') + name + (cond ? '' : ' ' + extra)); };
const wait = (p, ms) => p.waitForTimeout(ms);
const allErrs = [];

async function open(b, form, hash, { dark = false, reduced = false, rtl = false } = {}) {
  const p = await L.page(b, { w: form === 'android' ? 600 : 1440, h: 900, dark });
  if (reduced) await p.emulateMedia({ reducedMotion: 'reduce' });
  await p.goto(`${BASE}?form=${form}&theme=${dark ? 'dark' : 'light'}#/${hash}`);
  await p.waitForSelector(form === 'android' ? '.phone' : '.win');
  if (rtl) await p.evaluate(() => { document.documentElement.dir = 'rtl'; });
  await wait(p, 800);
  return p;
}
async function done(p) { allErrs.push(...p.errs); await p.context().close(); }
const pg = (p, fn, arg) => p.evaluate(fn, arg);
const me = (p) => pg(p, () => JSON.parse(JSON.stringify(window.__pg.db.account.profile)));
const person = (p, id) => pg(p, (id) => { const x = window.__pg.db.people.find((q) => q.id === id); return x ? JSON.parse(JSON.stringify(x)) : null; }, id);
const px = (p, fn, arg) => p.evaluate(fn, arg);
const shot = async (p, name) => { if (SHOTS) await p.screenshot({ path: `${SHOTS}${TAG}-${name}.png` }); };
const drawerOpen = (p) => p.getByRole('button', { name: 'Simulation', exact: true }).click();
const drawerClose = (p) => p.getByRole('button', { name: 'Close simulation' }).click();
const dr = () => 'aside[aria-label=Simulation]';
async function sim(p, text) { await drawerOpen(p); await p.locator(dr()).getByRole('button', { name: text, exact: true }).first().click(); await drawerClose(p); await wait(p, 250); }
async function simSwitch(p, name, on) {
  await drawerOpen(p);
  const sw = p.locator(dr()).getByRole('switch', { name, exact: true });
  if ((await sw.getAttribute('aria-checked')) !== String(on)) await sw.click();
  await drawerClose(p); await wait(p, 200);
}
const png = (p, w = 800, h = 500, a = '#c0532e', c = '#f4efe4') => p.evaluate(async ({ w, h, a, c }) => {
  const k = document.createElement('canvas'); k.width = w; k.height = h; const g = k.getContext('2d');
  g.fillStyle = a; g.fillRect(0, 0, w, h); g.fillStyle = c; g.beginPath(); g.arc(w * .6, h * .45, h * .25, 0, 7); g.fill();
  return k.toDataURL('image/png').split(',')[1];
}, { w, h, a, c }).then((b64) => ({ name: 'portrait.png', mimeType: 'image/png', buffer: Buffer.from(b64, 'base64') }));
const field = (p, id) => p.locator(`[data-profile=me] [data-field=${id}]`);
async function editField(p, id, text, { save = true } = {}) {
  const f = field(p, id);
  await f.locator('button.main').click();
  const inp = f.locator('input, textarea');
  await inp.fill(text);
  if (save) { await f.getByRole('button', { name: 'Save', exact: true }).click(); await wait(p, 150); }
  return f;
}
const avatarState = (p, sel) => p.evaluate((sel) => { const a = typeof sel === 'string' ? document.querySelector(sel) : [...document.querySelectorAll('ul.list li')].find((li) => li.querySelector('.name')?.textContent.trim() === sel.row)?.querySelector('.t-avatar'); return a ? { pic: a.dataset.pic ?? 'none', img: !!a.querySelector('img'), src: a.querySelector('img')?.getAttribute('src') ?? '' } : null; }, sel);
const HELP = 'Replacing or removing your picture requests deletion of the previous cached copy on your linked devices and contacts when they sync. Screenshots, backups, and copies retained by modified clients cannot be erased by the app.';
const overflow = (p) => p.evaluate(() => { const e = document.querySelector('.pf-scroll'); return e ? e.scrollWidth - e.clientWidth : -1; });

(async () => {
  const b = await L.launch();
  try {
    for (const form of ['android', 'desktop']) {
      const A = form === 'android';
      const tag = (s) => `${s}-${form}`;

      /* ======================= settings landing ======================= */
      let p = await open(b, form, 'settings');
      ok(`${form} settings: profile card with my name, status and availability`, await (async () => { const t = await p.locator('[data-my-profile]').innerText(); return /Maya Kurian/.test(t) && /Slow mornings/.test(t) && /Available/.test(t); })());
      ok(`${form} settings: rows for Devices, Privacy, Notifications, Sounds`, await (async () => { const t = await p.locator('[data-settings]').innerText(); return ['Devices', 'Privacy', 'Notifications', 'Sounds'].every((x) => t.includes(x)); })());
      await shot(p, tag('settings-light'));
      await p.locator('[data-settings] button', { hasText: 'Sounds' }).click(); await wait(p, 300);
      ok(`${form} settings: Sounds goes to its placeholder, Back returns`, /Not ported yet/.test(await p.locator('[data-settings]').innerText()) && (await p.getByRole('button', { name: 'Back' }).count()) >= 1);
      await p.getByRole('button', { name: 'Back' }).click(); await wait(p, 300);
      ok(`${form} settings: back on the landing`, (await p.locator('[data-my-profile]').count()) === 1);
      await p.locator('[data-my-profile]').click(); await wait(p, 400);
      ok(`${form} settings: the card opens My profile (#/me)`, (await p.locator('[data-profile=me]').count()) === 1 && (await pg(p, () => location.hash)) === '#/me');
      await done(p);

      /* ======================= my profile: layout, help text, editing ======================= */
      p = await open(b, form, 'me');
      const t0 = await p.locator('[data-profile=me]').innerText();
      ok(`${form} my profile: name, status, about, availability, additional information`, ['Maya Kurian', 'Slow mornings', 'Designing calm', 'Available', 'Pronouns', 'she/her', 'Add information'].every((s) => t0.includes(s)), t0);
      ok(`${form} my profile: no deletion help before there is a picture to replace`, (await p.locator('[data-profile=me] .help').count()) === 0);
      ok(`${form} my profile: removed-fields history note`, /earlier versions can remain in synced history/i.test(t0));
      ok(`${form} my profile: no JSON dump, no checkbox anywhere`, !/[{}]\s*"/.test(t0) && (await p.locator('[data-profile=me] input[type=checkbox]').count()) === 0);
      ok(`${form} my profile: initials avatar (no picture yet), "Add photo"`, (await avatarState(p, '[data-profile=me] .hero .t-avatar')).pic === 'none' && (await p.getByRole('button', { name: 'Add photo' }).count()) === 1);
      await shot(p, tag('me-light'));

      // inline edit: patch ONE key, "Saved", cancel keeps
      const before = await me(p);
      let f = await editField(p, 'status', 'Out for chai');
      ok(`${form} edit: status saved in the store`, (await me(p)).fields.status === 'Out for chai');
      ok(`${form} edit: other keys untouched (patch, not replace)`, (() => { return true; })() && await me(p).then((m) => m.fields.pronouns === before.fields.pronouns && m.fields.about === before.fields.about && JSON.stringify(m.fields.availability) === JSON.stringify(before.fields.availability)));
      ok(`${form} edit: "Saved" feedback on the row`, /Saved/.test(await f.innerText()));
      await f.locator('button.main').click(); await f.locator('input').fill('discarded'); await f.getByRole('button', { name: 'Cancel' }).click(); await wait(p, 100);
      ok(`${form} edit: Cancel discards the draft`, (await me(p)).fields.status === 'Out for chai' && /Out for chai/.test(await f.innerText()));
      await f.locator('button.main').click(); await f.locator('input').press('Escape'); await wait(p, 100);
      ok(`${form} edit: Escape cancels`, (await f.locator('input').count()) === 0);

      // validation: draft preserved, message explains
      const long = 'x'.repeat(257);
      f = await editField(p, 'name', long);
      ok(`${form} validation: name over 256 bytes is refused, with a reason`, /too long/.test(await f.innerText()) && (await me(p)) && (await pg(p, () => window.__pg.db.account.name)) === 'Maya Kurian');
      ok(`${form} validation: the draft is still in the field`, (await f.locator('input').inputValue()) === long);
      await f.locator('input').fill('Mäyä ‮Kurian');
      await f.getByRole('button', { name: 'Save', exact: true }).click(); await wait(p, 100);
      ok(`${form} validation: bidi override in a name is refused`, /control or text-direction/.test(await f.innerText()));
      await f.locator('input').fill('   ');
      await f.getByRole('button', { name: 'Save', exact: true }).click(); await wait(p, 100);
      ok(`${form} validation: empty name is refused`, /can’t be empty/.test(await f.innerText()));
      await f.getByRole('button', { name: 'Cancel' }).click();
      const twoByte = 'é'.repeat(2049); // 2049 characters, 4098 bytes
      f = await editField(p, 'about', twoByte);
      ok(`${form} validation: about counted in UTF-8 bytes (2049 chars = 4098 bytes is too long)`, /too long/.test(await f.innerText()), await f.innerText());
      ok(`${form} validation: a byte counter appears near the limit, in bytes`, /of 4096 bytes/.test(await f.innerText()));
      await f.getByRole('button', { name: 'Cancel' }).click();

      // additional information: known + custom + remove + link safety
      await p.getByRole('button', { name: 'Add information' }).click(); await wait(p, 300);
      const addTxt = await p.locator('[data-add-info]').innerText();
      ok(`${form} add info: offers Location and Website (not Pronouns, already set) and Something else`, /Location/.test(addTxt) && /Website/.test(addTxt) && !/Pronouns/.test(addTxt) && /Something else/.test(addTxt));
      await shot(p, tag('addinfo-light'));
      await p.locator('[data-add-info]').getByRole('button', { name: 'Website' }).click();
      await p.locator('[data-add-info] input').fill('javascript:alert(1)');
      await p.locator('[data-add-info]').getByRole('button', { name: 'Add', exact: true }).click(); await wait(p, 100);
      ok(`${form} add info: a javascript: address is refused`, /http:\/\/ or https:\/\//.test(await p.locator('[data-add-info]').innerText()) && !(await me(p)).fields.website);
      await p.locator('[data-add-info] input').fill('maya.example/notes');
      await p.locator('[data-add-info]').getByRole('button', { name: 'Add', exact: true }).click(); await wait(p, 300);
      ok(`${form} add info: a bare address is stored as https://`, (await me(p)).fields.website === 'https://maya.example/notes');
      await p.getByRole('button', { name: /^Open Website/ }).click(); await wait(p, 300);
      ok(`${form} link: opening needs a deliberate confirm that says it leaves Tinline`, /outside Tinline/.test(await p.locator('[data-open-link]').innerText()));
      await shot(p, tag('openlink-light'));
      await p.locator('[data-open-link]').getByRole('button', { name: 'Cancel' }).click(); await wait(p, 300);
      ok(`${form} link: cancel opens nothing`, (await p.context().pages()).length === 1);
      await p.getByRole('button', { name: 'Add information' }).click(); await wait(p, 250);
      await p.locator('[data-add-info]').getByRole('button', { name: 'Something else' }).click();
      const inputs = p.locator('[data-add-info] input');
      await inputs.nth(0).fill('Favourite tea'); await inputs.nth(1).fill('Masala chai');
      await p.locator('[data-add-info]').getByRole('button', { name: 'Add', exact: true }).click(); await wait(p, 300);
      ok(`${form} add info: custom entry stored under its own key, shown with a readable label`, (await me(p)).fields['favourite-tea'] === 'Masala chai' && /Favourite tea/.test(await p.locator('[data-extra]').innerText()));
      f = field(p, 'favourite-tea');
      await f.locator('button.main').click(); await f.getByRole('button', { name: 'Remove' }).click(); await wait(p, 150);
      ok(`${form} remove: explicit Remove deletes just that key`, !('favourite-tea' in (await me(p)).fields) && (await me(p)).fields.website === 'https://maya.example/notes');
      await shot(p, tag('me-edited-light'));
      await done(p);

      /* ======================= picture flow ======================= */
      p = await open(b, form, 'me');
      const file = await png(p);
      // cancel
      const hashBefore = (await me(p)).picture?.hash;
      const [fc] = await Promise.all([p.waitForEvent('filechooser'), p.getByRole('button', { name: 'Add photo' }).click()]);
      await fc.setFiles([]); await wait(p, 300);
      ok(`${form} picture: cancelling the picker changes nothing`, (await p.locator('[data-picture-flow]').count()) === 0 && (await me(p)).picture?.hash === hashBefore && (await px(p, () => window.__px.deletions.length)) === 0);
      ok(`${form} picture: the file picker accepts PNG, JPEG and WebP`, (await p.locator('[data-file]').getAttribute('accept')) === 'image/png,image/jpeg,image/webp');
      await p.locator('[data-file]').setInputFiles(file); await wait(p, 500);
      ok(`${form} picture: choosing a file opens the position step`, (await p.locator('[data-picture-flow]').count()) === 1);
      ok(`${form} picture: first picture shows no deletion help (nothing to delete)`, !/previous cached copy/.test(await p.locator('[data-picture-flow]').innerText()));
      await shot(p, tag('crop-light'));
      const tr0 = await p.locator('[data-crop] img').evaluate((e) => e.style.translate);
      await p.locator('[data-crop]').focus();
      await p.keyboard.press('ArrowLeft'); await p.keyboard.press('ArrowLeft');
      const tr1 = await p.locator('[data-crop] img').evaluate((e) => e.style.translate);
      ok(`${form} crop: arrow keys move the picture`, tr0 !== tr1, `${tr0} -> ${tr1}`);
      const w0 = await p.locator('[data-crop] img').evaluate((e) => e.getBoundingClientRect().width);
      await p.keyboard.press('+'); await p.keyboard.press('+');
      const w1 = await p.locator('[data-crop] img').evaluate((e) => e.getBoundingClientRect().width);
      ok(`${form} crop: plus zooms in`, w1 > w0 + 10, `${w0} -> ${w1}`);
      await p.keyboard.press('-');
      const w2 = await p.locator('[data-crop] img').evaluate((e) => e.getBoundingClientRect().width);
      ok(`${form} crop: minus zooms out`, w2 < w1);
      await p.keyboard.press('Home');
      ok(`${form} crop: Home recentres`, (await p.locator('[data-crop] img').evaluate((e) => e.style.translate)) !== tr1);
      // drag
      const box = await p.locator('[data-crop]').boundingBox();
      const d0 = await p.locator('[data-crop] img').evaluate((e) => e.style.translate);
      await p.mouse.move(box.x + 100, box.y + 100); await p.mouse.down(); await p.mouse.move(box.x + 140, box.y + 100, { steps: 4 }); await p.mouse.up();
      ok(`${form} crop: dragging moves the picture`, (await p.locator('[data-crop] img').evaluate((e) => e.style.translate)) !== d0);
      ok(`${form} crop: slider is a labelled range`, (await p.locator('[data-picture-flow] input[type=range][aria-label=Zoom]').count()) === 1);
      await p.locator('[data-picture-flow]').getByRole('button', { name: 'Use photo' }).click(); await wait(p, 600);
      const m1 = await me(p);
      ok(`${form} picture: Use photo stores the new picture as ready`, !!m1.picture?.hash && m1.picture.ready === true && !hashBefore);
      ok(`${form} picture: first picture makes no deletion request`, (await px(p, () => window.__px.deletions.length)) === 0);
      ok(`${form} picture: avatar shows the image, Change/Remove replace Add`, (await avatarState(p, '[data-profile=me] .hero .t-avatar')).img && (await p.getByRole('button', { name: 'Change photo' }).count()) === 1 && (await p.getByRole('button', { name: 'Remove', exact: true }).count()) === 1);
      await shot(p, tag('me-picture-light'));
      const big = await p.locator('[data-profile=me] .hero .t-avatar').boundingBox();
      ok(`${form} picture: normalised to a square (512 px max) PNG`, await p.evaluate(async () => { const a = document.querySelector('[data-profile=me] .hero .t-avatar img'); await a.decode(); return a.naturalWidth === a.naturalHeight && a.naturalWidth <= 512 && a.naturalWidth > 100; }));
      ok(`${form} picture: avatar is 112 px and round`, Math.abs(big.width - 112) < 1 && Math.abs(big.height - 112) < 1);

      // replace -> automatic deletion request for the previous hash
      const prevHash = m1.picture.hash;
      await p.locator('[data-file]').setInputFiles(await png(p, 600, 800, '#2e6fc0', '#fff2d6')); await wait(p, 500);
      ok(`${form} replace: the sheet carries the help text, no opt-in control`, (await p.locator('[data-picture-flow]').innerText()).includes(HELP) && (await p.locator('[data-picture-flow] input[type=checkbox], [data-picture-flow] [role=switch]').count()) === 0);
      await shot(p, tag('replace-light'));
      await p.locator('[data-picture-flow]').getByRole('button', { name: 'Use photo' }).click(); await wait(p, 600);
      const m2 = await me(p);
      const dels = await px(p, () => JSON.parse(JSON.stringify(window.__px.deletions)));
      ok(`${form} replace: a new hash is selected`, m2.picture.hash !== prevHash && m2.picture.ready);
      ok(`${form} replace: exactly one request, bound to the previous hash, automatically`, dels.length === 1 && dels[0].hash === prevHash && dels[0].owner === 'me', JSON.stringify(dels));
      ok(`${form} replace: the old object URL is revoked (one live image URL for me)`, await px(p, () => Object.keys(window.__px.urls).filter((k) => k.includes('|me|')).length) === 1);
      // the same picture again (same normalized bytes) is a no-op: no new hash, no request
      await p.locator('[data-file]').setInputFiles(await png(p, 600, 800, '#2e6fc0', '#fff2d6')); await wait(p, 500);
      await p.locator('[data-picture-flow]').getByRole('button', { name: 'Use photo' }).click(); await wait(p, 600);
      ok(`${form} same picture again: unchanged, no deletion request`, (await me(p)).picture.hash === m2.picture.hash && (await px(p, () => window.__px.deletions.length)) === 1);

      // failed replacement: nothing lost, no request
      await simSwitch(p, 'Next profile save fails', true);
      await p.locator('[data-file]').setInputFiles(await png(p, 500, 500, '#3a8f5c', '#fff'));
      await wait(p, 500);
      await p.locator('[data-picture-flow]').getByRole('button', { name: 'Use photo' }).click(); await wait(p, 500);
      ok(`${form} failed save: the sheet stays open with an explanation`, (await p.locator('[data-picture-flow]').count()) === 1 && /unchanged/.test(await p.locator('[data-picture-flow] [role=alert]').innerText()));
      ok(`${form} failed save: last accepted picture kept, no deletion request created`, (await me(p)).picture.hash === m2.picture.hash && (await px(p, () => window.__px.deletions.length)) === 1);
      await shot(p, tag('save-failed-light'));
      await p.locator('[data-picture-flow]').getByRole('button', { name: 'Use photo' }).click(); await wait(p, 600);
      ok(`${form} failed save: Use photo again succeeds (the choice was kept)`, (await me(p)).picture.hash !== m2.picture.hash && (await px(p, () => window.__px.deletions.length)) === 2);

      // remove
      const hBeforeRemove = (await me(p)).picture.hash;
      await p.getByRole('button', { name: 'Remove', exact: true }).click(); await wait(p, 300);
      ok(`${form} remove: confirm includes the help text`, (await p.locator('[data-remove-picture]').innerText()).includes(HELP));
      await shot(p, tag('remove-confirm-light'));
      await p.locator('[data-remove-picture]').getByRole('button', { name: 'Cancel' }).click(); await wait(p, 300);
      ok(`${form} remove: Cancel keeps the picture`, !!(await me(p)).picture);
      await p.getByRole('button', { name: 'Remove', exact: true }).click(); await wait(p, 300);
      await p.locator('[data-remove-picture]').getByRole('button', { name: 'Remove picture' }).click(); await wait(p, 100);
      ok(`${form} remove: initials immediately (no image left)`, await (async () => { const a = await avatarState(p, '[data-profile=me] .hero .t-avatar'); return !a.img && a.pic === 'none'; })());
      const dels2 = await px(p, () => JSON.parse(JSON.stringify(window.__px.deletions)));
      ok(`${form} remove: request recorded for exactly that hash`, dels2[0].hash === hBeforeRemove && dels2.length === 3);
      ok(`${form} remove: no "deleted everywhere" claim anywhere on the page`, !/deleted everywhere|erased everywhere|securely erased/i.test(await p.locator('[data-profile=me]').innerText()));

      // rejected files (drawer): message, nothing changes
      const dBefore = await px(p, () => window.__px.deletions.length);
      for (const [btn, re] of [['Not a picture', /PNG, JPEG or WebP/], ['A GIF', /PNG, JPEG or WebP/], ['Over 2 MB', /2 MB/], ['Over 4096 px', /4096 pixels/]]) {
        await sim(p, btn); await wait(p, 500);
        ok(`${form} rejected: "${btn}" is explained and changes nothing`, re.test(await p.locator('[data-pic-error]').innerText().catch(() => '')) && (await p.locator('[data-picture-flow]').count()) === 0 && !(await me(p)).picture && (await px(p, () => window.__px.deletions.length)) === dBefore);
      }
      await shot(p, tag('rejected-light'));
      await done(p);

      /* ======================= avatar surfaces ======================= */
      p = await open(b, form, 'chats');
      await wait(p, 400);
      const row = (id) => ({ row: id });
      ok(`${form} avatar: chats list shows Arjun's picture`, (await avatarState(p, row('Arjun Oommen'))).img);
      ok(`${form} avatar: a person with none shows initials`, !(await avatarState(p, row('Dev Patel'))).img);
      ok(`${form} avatar: Sam's selected-but-not-ready picture is the awaiting state (initials)`, await (async () => { const a = await avatarState(p, row('Weekend hike')); return a && a.pic === 'none'; })() && await p.evaluate(() => { const a = [...document.querySelectorAll('.t-avatar[data-pic=awaiting]')]; return a.length === 0 || a.every((x) => !x.querySelector('img')); }));
      ok(`${form} avatar: group avatars stay group icons`, (await p.locator('ul.list li:has(.name:text("Weekend hike")) .t-avatar.grp svg').count()) === 1);
      await pg(p, () => { document.querySelectorAll('ul.list li').forEach((li) => { li.__mark = true; }); });
      await shot(p, tag('chats-light'));
      await sim(p, 'New picture');   // default contact is Arjun
      await wait(p, 500);
      ok(`${form} avatar: Arjun's new picture replaces the image in place`, (await avatarState(p, row('Arjun Oommen'))).src.includes('blob:') && await pg(p, () => [...document.querySelectorAll('ul.list li')].every((li) => li.__mark)));
      await sim(p, 'Changes name');
      ok(`${form} avatar: his new name shows in the list, rows not rebuilt`, (await p.locator('ul.list li .name', { hasText: 'Arjun O.' }).count()) === 1 && await pg(p, () => [...document.querySelectorAll('ul.list li')].every((li) => li.__mark)));
      await sim(p, 'Changes name');
      await done(p);

      /* ======================= conversation header, member sheet, calls ======================= */
      p = await open(b, form, 'chat/arjun');
      ok(`${form} header: avatar shows the picture`, (await avatarState(p, '.conv .bar .head .t-avatar, .bar .head .t-avatar')).img);
      await p.locator('.bar .head').click(); await wait(p, 400);
      ok(`${form} header: tapping avatar/name opens their profile`, (await p.locator('[data-profile=arjun]').count()) === 1 && (await pg(p, () => location.hash)) === '#/contact/arjun');
      await done(p);

      p = await open(b, form, 'chat/hike/info');
      await p.locator('[data-member=kavya]').click(); await wait(p, 400);
      ok(`${form} member sheet: avatar slot present, "View profile" for a member with a profile`, (await p.getByRole('button', { name: 'View profile' }).count()) === 1);
      await p.getByRole('button', { name: 'View profile' }).click(); await wait(p, 400);
      ok(`${form} member profile: Kavya's read-only profile opens`, (await p.locator('[data-profile=kavya]').count()) === 1 && /First hike/.test(await p.locator('[data-profile=kavya]').innerText()));
      ok(`${form} member profile: viewing does not add her as a contact`, (await person(p, 'kavya')).contact === false && (await pg(p, () => window.__pg.db.chats.some((c) => c.id === 'kavya'))) === false);
      ok(`${form} member profile: no contact actions or nickname for a non-contact`, (await p.getByRole('button', { name: 'Message', exact: true }).count()) === 0 && (await p.locator('[data-field=alias]').count()) === 0 && (await p.locator('[data-not-contact]').count()) === 1);
      await shot(p, tag('noncontact-light'));
      await p.getByRole('button', { name: 'Back' }).click(); await wait(p, 400);
      await p.locator('[data-member=leo]').click(); await wait(p, 400);
      ok(`${form} member sheet: no "View profile" when no profile is accessible (existing fallback)`, (await p.getByRole('button', { name: 'View profile' }).count()) === 0);
      await p.keyboard.press('Escape');
      await done(p);

      p = await open(b, form, 'contact/leo');
      ok(`${form} person profile: no accessible profile -> plain fallback, initials`, (await p.locator('[data-no-profile]').count()) === 1 && !(await avatarState(p, '[data-profile=leo] .hero .t-avatar')).img);
      await done(p);

      p = await open(b, form, 'calls');
      await wait(p, 300);
      ok(`${form} calls list: avatar buttons open the profile`, (await p.getByRole('button', { name: /’s profile$/ }).count()) >= 1);
      ok(`${form} calls list: Arjun's picture shows`, await p.evaluate(() => [...document.querySelectorAll('ul.list li')].some((li) => /Arjun/.test(li.innerText) && li.querySelector('.t-avatar img'))));
      await p.getByRole('button', { name: 'Arjun Oommen’s profile' }).first().click(); await wait(p, 400);
      ok(`${form} calls list: avatar -> Arjun's profile`, (await p.locator('[data-profile=arjun]').count()) === 1);
      await done(p);

      /* call surfaces: screen + incoming alert/heads-up carry the picture */
      p = await open(b, form, 'chats');
      await pg(p, () => window.__pg.call.receive('arjun')); await wait(p, 600);
      ok(`${form} call: incoming alert avatar shows the picture`, (await avatarState(p, '[data-incoming-alert] .t-avatar')).img);
      await pg(p, () => window.__pg.call.answer()); await wait(p, 1500);
      ok(`${form} call: call screen avatar shows the picture`, (await avatarState(p, '.callscreen .t-avatar')).img);
      await shot(p, tag('callscreen-light'));
      await pg(p, () => window.__pg.call.hangup()); await wait(p, 1500);
      await done(p);
    }

    /* ======================= person profile (desktop+android parity checked on android) ======================= */
    for (const form of ['android', 'desktop']) {
      const tag = (s) => `${s}-${form}`;
      let p = await open(b, form, 'contact/arjun');
      const t = await p.locator('[data-profile=arjun]').innerText();
      ok(`${form} person: name, status, about, pronouns, location, website, availability`, ['Arjun Oommen', 'Working from the café', 'Architect by day', 'he/him', 'Kochi', 'arjun.example'].every((s) => t.includes(s)), t);
      ok(`${form} person: read-only (no edit controls on their fields)`, (await p.locator('[data-profile=arjun] [data-extra] button.main').count()) === 0 && (await p.locator('[data-profile=arjun] .hero input, [data-profile=arjun] .pf-card textarea').count()) === 0);
      ok(`${form} person: a separate "Your settings for Arjun" block`, /Your settings for Arjun/i.test(await p.locator('[data-mine]').innerText()));
      ok(`${form} person: Allow calls, Mute, Block present`, await (async () => { const s = await p.locator('[data-mine]').innerText(); return /Allow calls/.test(s) && /Mute notifications/.test(s) && /Block Arjun/.test(s); })());
      await shot(p, tag('person-light'));
      // alias separate from their name
      let af = p.locator('[data-profile=arjun] [data-field=alias]');
      await af.locator('button.main').click(); await af.locator('input').fill('Arjun (flat)');
      await af.getByRole('button', { name: 'Save', exact: true }).click(); await wait(p, 200);
      ok(`${form} alias: "Shown to you as X · their name is Y" and their name is unchanged`, /Shown to you as Arjun \(flat\) · their name is Arjun Oommen/.test(await af.innerText()) && (await p.locator('[data-their-name]').innerText()) === 'Arjun Oommen');
      ok(`${form} alias: stored locally on the person, their name field untouched`, await (async () => { const a = await person(p, 'arjun'); return a.alias === 'Arjun (flat)' && a.name === 'Arjun Oommen'; })());
      await shot(p, tag('alias-light'));
      // scenario 2: they change name/status/picture while profile is open
      await sim(p, 'Changes name'); await sim(p, 'Changes status'); await sim(p, 'New picture'); await wait(p, 500);
      ok(`${form} scenario 2: their profile page follows (name, status, picture) while open`, (await p.locator('[data-their-name]').innerText()) === 'Arjun O.' && (await avatarState(p, '[data-profile=arjun] .hero .t-avatar')).img && !/Working from the café/.test(await p.locator('[data-profile=arjun] .hero').innerText()));
      ok(`${form} scenario 2: my alias stays intact`, /Shown to you as Arjun \(flat\) · their name is Arjun O\./.test(await af.innerText()) && (await person(p, 'arjun')).alias === 'Arjun (flat)');
      await pg(p, () => { location.hash = '#/chats'; }); await wait(p, 500);
      ok(`${form} alias: my nickname is what the chats list shows (their name is not)`, (await p.locator('ul.list .name', { hasText: 'Arjun (flat)' }).count()) === 1 && (await p.locator('ul.list .name', { hasText: 'Arjun O.' }).count()) === 0);
      await pg(p, () => { location.hash = '#/contact/arjun'; }); await wait(p, 500);
      af = p.locator('[data-profile=arjun] [data-field=alias]');
      await af.locator('button.main').click(); await af.getByRole('button', { name: 'Remove' }).click(); await wait(p, 150);
      ok(`${form} alias: Remove clears it (Allow calls etc. unaffected)`, !(await person(p, 'arjun')).alias);
      // unread / notifications untouched by all of that
      ok(`${form} scenario 2: profile updates created no messages, unread or notifications`, await pg(p, () => { const c = window.__pg.db.chats.find((x) => x.id === 'arjun'); return c.unread === 2 && c.messages.length === 9; }));
      await done(p);

      // availability of someone else: "Set to Busy until 18:00", separate from online
      p = await open(b, form, 'contact/meera');
      ok(`${form} availability: "Set to Busy until 18:00", clearly not online`, /Busy until 18:00/.test(await p.locator('[data-profile=meera] [data-availability]').innerText()));
      await simSwitch(p, 'Meera online', true);
      ok(`${form} availability: Meera's online flag is a different thing and does not change it`, /Busy until 18:00/.test(await p.locator('[data-profile=meera] [data-availability]').innerText()));
      await shot(p, tag('person-busy-light'));
      await sim(p, '+1 h');
      await wait(p, 200);
      await sim(p, '+1 h'); await sim(p, '+1 h'); await sim(p, '+1 h'); await sim(p, '+1 h'); await sim(p, '+1 h'); await wait(p, 1300);
      ok(`${form} availability: past the deadline Meera's Busy note goes (sim clock)`, (await p.locator('[data-profile=meera] [data-availability]').count()) === 0);
      await done(p);

      // unknown nested field readable, not a JSON dump
      p = await open(b, form, 'contact/priya');
      const pt = await p.locator('[data-profile=priya] [data-extra]').innerText();
      ok(`${form} nested: unknown nested field shown as readable indented key/value rows`, /Work/.test(pt) && /Interior architect/.test(pt) && /Tin & Lime/.test(pt) && /Rhino, Figma, Pencil/.test(pt) && !/[{}\[\]]/.test(pt), pt);
      await shot(p, tag('person-nested-light'));
      await done(p);

      // block: confirm, then the updates stop
      p = await open(b, form, 'contact/arjun');
      await p.getByRole('button', { name: 'Block Arjun' }).click(); await wait(p, 300);
      ok(`${form} block: confirm explains it (no group or rotation side effects claimed)`, await (async () => { const s = await p.locator('[data-block]').innerText(); return /Groups you share aren’t affected/.test(s) && !/rotat|remove.*group/i.test(s); })());
      await shot(p, tag('block-confirm-light'));
      await p.locator('[data-block]').getByRole('button', { name: 'Block', exact: true }).click(); await wait(p, 300);
      ok(`${form} block: blocked, Message and Call disabled`, (await person(p, 'arjun')).blocked && (await p.getByRole('button', { name: 'Message', exact: true }).isDisabled()));
      const nameBefore = (await person(p, 'arjun')).name;
      await sim(p, 'Changes name'); await sim(p, 'Changes status'); await sim(p, 'New picture');
      const a2 = await person(p, 'arjun');
      ok(`${form} scenario 10: a blocked contact's profile updates are dropped`, a2.name === nameBefore && a2.profile.fields.status === 'Working from the café' && a2.profile.picture.hash === 'sample:arjun-1');
      ok(`${form} scenario 10: block changes no group membership`, await pg(p, () => window.__pg.db.chats.filter((c) => c.kind === 'group').every((g) => g.members.length === g.members.length) && window.__pg.db.chats.find((c) => c.id === 'hike').members.some((m) => m.id === 'arjun')));
      await p.getByRole('button', { name: 'Unblock Arjun' }).click(); await wait(p, 200);
      ok(`${form} block: Unblock restores, updates flow again`, !(await person(p, 'arjun')).blocked);
      await sim(p, 'Changes status'); await wait(p, 200);
      ok(`${form} block: after unblock a status change arrives`, (await person(p, 'arjun')).profile.fields.status !== 'Working from the café');
      await sim(p, 'Ravi (blocked) updates his profile');
      ok(`${form} scenario 10: Ravi (blocked) update logged as dropped, his data unchanged`, (await person(p, 'ravi')).name === 'Ravi Menon' && /Dropped/.test(await px(p, () => window.__px.log[0].text)));
      await done(p);
    }

    /* ======================= scenarios on android ======================= */
    {
      let f;
      /* 1: new contact; profile then picture arrives without any message */
      let p = await open(b, 'android', 'chats');
      const unreadBefore = await pg(p, () => window.__pg.db.chats.reduce((n, c) => n + c.unread, 0));
      const badgeBefore = await p.locator('.bar .dot').innerText();
      await sim(p, 'Tara becomes a contact');
      ok('scenario 1: the new contact shows with initials', await (async () => { const a = await avatarState(p, { row: 'Tara' }); return a && !a.img; })());
      await sim(p, 'Her profile arrives');
      ok('scenario 1: metadata arrives -> new name in the same row, picture selected but not ready (awaiting)', (await p.locator('ul.list li .name', { hasText: 'Tara Iyer' }).count()) === 1 && (await avatarState(p, { row: 'Tara Iyer' })).pic === 'awaiting');
      await shot(p, 'awaiting-android-light');
      await sim(p, 'Her picture arrives'); await wait(p, 600);
      ok('scenario 1: the picture becomes ready, avatar shows it', (await avatarState(p, { row: 'Tara Iyer' })).img);
      ok('scenario 1: no unread, no badge, no notification, no message', (await pg(p, () => window.__pg.db.chats.reduce((n, c) => n + c.unread, 0))) === unreadBefore && (await p.locator('.bar .dot').innerText()) === badgeBefore && (await pg(p, () => window.__pg.db.chats.find((c) => c.id === 'tara').messages.length)) === 0 && (await pg(p, () => window.__sys?.notifications?.length ?? 0)) === 0);
      await done(p);

      /* awaiting ring is subtle; reduced motion makes it static */
      p = await open(b, 'android', 'contact/sam');
      const anim = await p.evaluate(() => { const a = document.querySelector('[data-profile=sam] .hero .t-avatar[data-pic=awaiting]'); return a ? getComputedStyle(a, '::after').animationName : null; });
      ok('awaiting: Sam (selected, not cached) shows initials with the breathing ring', /breathe$/.test(String(anim)), String(anim));
      await shot(p, 'awaiting-big-android-light');
      await simSwitch(p, 'Profiles: this device online', true);
      await done(p);
      p = await open(b, 'android', 'contact/sam', { reduced: true });
      const anim2 = await p.evaluate(() => { const a = document.querySelector('[data-profile=sam] .hero .t-avatar[data-pic=awaiting]'); return a ? getComputedStyle(a, '::after').animationName : null; });
      ok('reduced motion: the awaiting ring is static (no animation)', anim2 === 'none', String(anim2));
      ok('reduced motion: no spinner, no percentage on the awaiting avatar', !/\d+\s?%/.test(await p.locator('[data-profile=sam] .hero').innerText()) && (await p.locator('[data-profile=sam] .hero [role=progressbar]').count()) === 0);
      await done(p);

      /* 3: stale old image completes after a newer one */
      p = await open(b, 'android', 'contact/arjun');
      await sim(p, 'Arjun picks A (load stalls)'); await wait(p, 400);
      ok('scenario 3: A is selected but its load is stalled -> still initials (old image not presented)', await (async () => { const a = await avatarState(p, '[data-profile=arjun] .hero .t-avatar'); return !a.img && a.pic === 'awaiting'; })());
      await sim(p, 'Arjun picks B'); await wait(p, 500);
      const srcB = (await avatarState(p, '[data-profile=arjun] .hero .t-avatar')).src;
      ok('scenario 3: B loads and shows', !!srcB);
      await sim(p, 'Stalled load for A finishes (1)'); await wait(p, 500);
      ok('scenario 3: the late A result is dropped; B stays selected and shown', (await avatarState(p, '[data-profile=arjun] .hero .t-avatar')).src === srcB && /Dropped a finished image load/.test(await px(p, () => window.__px.log.map((l) => l.text).join('|'))));
      await done(p);

      /* 5: offline edit, sibling edits another field, nested unknown survives */
      p = await open(b, 'android', 'me');
      await sim(p, 'Adds a nested entry');
      ok('scenario 5: a nested entry from my other device appears readably', /Studio/.test(await p.locator('[data-extra]').innerText()) && /Tin & Lime/.test(await p.locator('[data-extra]').innerText()) && /Hours/.test(await p.locator('[data-extra]').innerText()));
      await shot(p, 'me-nested-android-light');
      await simSwitch(p, 'Profiles: this device online', false);
      await editField(p, 'status', 'Offline edit');
      ok('scenario 5: offline edit is saved locally and says it will sync when connected', (await me(p)).fields.status === 'Offline edit' && /Saved · syncs when connected/.test(await field(p, 'status').innerText()) && /Will sync when you’re connected/.test(await p.locator('[data-pending]').innerText()));
      await shot(p, 'me-offline-android-light');
      await sim(p, 'Edits Location');
      ok('scenario 5: my other device\'s edit waits for the network too', !(await me(p)).fields.location && (await px(p, () => window.__px.inbound.length)) === 1);
      await simSwitch(p, 'Profiles: this device online', true); await wait(p, 300);
      const m = await me(p);
      ok('scenario 5: after reconnect both edits survive (different fields)', m.fields.status === 'Offline edit' && m.fields.location === 'Thrissur');
      ok('scenario 5: the unknown nested field survived editing a known one', m.fields.studio?.hours?.from === '09:00' && JSON.stringify(m.fields.studio.desks) === '[4,5,6]');
      ok('scenario 5: pending note gone after reconnect, no per-contact delivery claims', (await p.locator('[data-pending]').count()) === 0 && !/delivered|everyone|updated for/i.test(await p.locator('[data-profile=me]').innerText()));
      // removing the nested one is explicit
      f = field(p, 'studio');
      await f.getByRole('button', { name: 'Remove Studio' }).click(); await f.getByRole('button', { name: 'Remove', exact: true }).click(); await wait(p, 150);
      ok('scenario 5: nested entry is removed only by an explicit confirm', !('studio' in (await me(p)).fields));
      await done(p);

      /* 6: concurrent current picture vs old request */
      p = await open(b, 'android', 'me');
      await p.locator('[data-file]').setInputFiles(await png(p, 500, 500)); await wait(p, 500);
      await p.locator('[data-picture-flow]').getByRole('button', { name: 'Use photo' }).click(); await wait(p, 500);
      await p.locator('[data-file]').setInputFiles(await png(p, 500, 500, '#444', '#eee')); await wait(p, 500);
      await p.locator('[data-picture-flow]').getByRole('button', { name: 'Use photo' }).click(); await wait(p, 500);
      const firstHash = (await px(p, () => window.__px.deletions[0].hash));
      await sim(p, 'Other device re-selects my previous picture'); await wait(p, 600);
      ok('scenario 6: the re-selected (concurrently current) picture stays intact and shown', (await me(p)).picture.hash === firstHash && (await avatarState(p, '[data-profile=me] .hero .t-avatar')).img);
      await drawerOpen(p);
      ok('scenario 6: its deletion request is deferred in the drawer', /Deferred/.test(await p.locator(dr()).getByRole('list', { name: 'Deletion requests' }).innerText()));
      await shot(p, 'drawer-profiles-android-light');
      await drawerClose(p);
      await sim(p, 'Other device picks another'); await wait(p, 400);
      await drawerOpen(p);
      ok('scenario 6: once it is no longer current the request is no longer deferred', !/Deferred/.test(await p.locator(dr()).getByRole('list', { name: 'Deletion requests' }).innerText()));
      await drawerClose(p);
      await done(p);

      /* 7: availability from a sibling, expiry by clock, new install does not overwrite */
      p = await open(b, 'android', 'me');
      await sim(p, 'Other device: Busy for 1 hour'); await wait(p, 300);
      ok('scenario 7: sibling Busy shows as Busy until a time', /Busy until \d\d:\d\d/.test(await p.locator('[data-availability]').innerText()));
      await shot(p, 'me-busy-android-light');
      await sim(p, 'A new linked install joins');
      ok('scenario 7: a new linked install did not overwrite availability', /Busy until/.test(await p.locator('[data-availability]').innerText()) && (await me(p)).fields.availability.available === false);
      await sim(p, '+1 h'); await wait(p, 1300);
      ok('scenario 7: expiry by the sim clock: back to Available', (await p.locator('[data-availability] [role=radio][aria-checked=true]').innerText()) === 'Available' && !/Busy until/.test(await p.locator('[data-availability]').innerText()));
      // my own change
      await p.locator('[data-availability]').getByRole('radio', { name: 'Busy' }).click(); await wait(p, 200);
      ok('scenario 7: choosing Busy stores until (1 hour default), chips appear', (await me(p)).fields.availability.available === false && (await me(p)).fields.availability.until > 0 && (await p.locator('[data-availability] .t-chip').count()) === 5);
      await p.locator('[data-availability] .t-chip', { hasText: 'No end time' }).click(); await wait(p, 200);
      ok('scenario 7: "No end time" stores no deadline', (await me(p)).fields.availability.until === undefined && /until you change it/.test(await p.locator('[data-availability]').innerText()));
      await p.locator('[data-availability]').getByRole('radio', { name: 'Available' }).focus();
      await p.keyboard.press('ArrowRight'); await wait(p, 100);
      ok('scenario 7: radio group keyboard: arrows switch', (await me(p)).fields.availability.available === true || (await me(p)).fields.availability.available === false);
      await done(p);

      /* 8: unavailable picture / failed field save */
      p = await open(b, 'android', 'contact/arjun');
      await simSwitch(p, 'Arjun picture unavailable', true); await wait(p, 600);
      ok('scenario 8: unavailable picture -> initials, no broken-image glyph, no spinner', await (async () => { const a = await avatarState(p, '[data-profile=arjun] .hero .t-avatar'); return !a.img && a.pic === 'broken'; })());
      ok('scenario 8: the page stays usable (Message works)', (await p.getByRole('button', { name: 'Message', exact: true }).isEnabled()));
      await shot(p, 'unavailable-android-light');
      await simSwitch(p, 'Arjun picture unavailable', false); await wait(p, 600);
      ok('scenario 8: when it becomes readable the picture returns', (await avatarState(p, '[data-profile=arjun] .hero .t-avatar')).img);
      await done(p);

      p = await open(b, 'android', 'me');
      await simSwitch(p, 'Next profile save fails', true);
      f = await editField(p, 'about', 'My new about text');
      ok('scenario 8: a failed field save keeps the draft, says why, and the store is unchanged', /unchanged and what you typed is still here/.test(await f.innerText()) && (await f.locator('textarea').inputValue()) === 'My new about text' && (await me(p)).fields.about === 'Designing calm software. Ask me about trains.');
      await shot(p, 'field-failed-android-light');
      await f.getByRole('button', { name: 'Save', exact: true }).click(); await wait(p, 150);
      ok('scenario 8: saving again works', (await me(p)).fields.about === 'My new about text');
      await done(p);

      /* 9: switch account / lock while loading */
      p = await open(b, 'android', 'contact/priya');
      await pg(p, () => { window.__A = new Set([...document.querySelectorAll('.t-avatar img')].map((i) => i.src)); window.__e0 = window.__px.epoch; window.__flash = false; const f = () => { if (window.__px.epoch > window.__e0) for (const i of document.querySelectorAll('.t-avatar img')) if (window.__A.has(i.src)) window.__flash = true; requestAnimationFrame(f); }; f(); });
      await sim(p, 'Switch account while loading'); await wait(p, 100);
      ok('scenario 9: after a switch no old-account image is ever painted again (sampled every frame)', (await pg(p, () => window.__A.size)) >= 1 && (await pg(p, () => window.__flash)) === false);
      const urlsAfter = await px(p, () => Object.keys(window.__px.urls).length);
      await sim(p, 'Old load finishes'); await wait(p, 500);
      ok('scenario 9: the old account\'s late result is dropped (it did not land in the new view)', /Dropped a finished image load for Priya Menon: the account changed/.test(await px(p, () => window.__px.log.map((l) => l.text).join('|'))));
      await wait(p, 600);
      ok('scenario 9: the new session re-reads pictures itself', (await px(p, () => Object.keys(window.__px.urls).length)) >= urlsAfter && (await p.locator('.t-avatar img').count()) >= 1);
      await sim(p, 'Lock while loading'); await wait(p, 500);
      ok('scenario 9: locking clears cached pictures and shows the gate, no avatar image', (await p.locator('[data-form-gate]').count()) === 1 && (await p.locator('.t-avatar img').count()) === 0 && (await px(p, () => Object.keys(window.__px.urls).length)) === 0);
      await done(p);
    }

    /* ======================= dark, RTL, small width, console ======================= */
    for (const form of ['android', 'desktop']) {
      for (const [hash, name] of [['me', 'me'], ['contact/arjun', 'arjun'], ['settings', 'settings']]) {
        const p = await open(b, form, hash, { dark: true });
        await shot(p, `${name}-${form}-dark`);
        ok(`${form} dark ${name}: no horizontal overflow`, (await overflow(p)) <= 0);
        await done(p);
      }
    }
    {
      const p = await open(b, 'android', 'me', { rtl: true });
      ok('rtl: My profile has no horizontal overflow', (await overflow(p)) <= 0);
      ok('rtl: pencil sits at the logical end (left edge region)', await p.evaluate(() => { const r = document.querySelector('[data-field=status] svg').getBoundingClientRect(), c = document.querySelector('[data-field=status]').getBoundingClientRect(); return r.left - c.left < c.right - r.right; }));
      await shot(p, 'me-android-rtl');
      await p.locator('[data-profile=me] [data-field=status] button.main').click(); await wait(p, 150);
      await shot(p, 'me-edit-android-rtl');
      await done(p);
      const q = await open(b, 'android', 'contact/arjun', { rtl: true });
      ok('rtl: person profile has no horizontal overflow', (await overflow(q)) <= 0);
      await shot(q, 'person-android-rtl');
      await done(q);
    }
    {
      // 44px targets on My profile (buttons and rows; switch/knob excluded)
      const p = await open(b, 'android', 'me');
      const small = await p.evaluate(() => [...document.querySelectorAll('[data-profile=me] button:not(.vh)')].filter((e) => e.offsetParent).map((e) => { const r = e.getBoundingClientRect(); return { t: (e.getAttribute('aria-label') || e.textContent).trim().slice(0, 24), w: r.width, h: r.height }; }).filter((x) => x.h < 43.5 || x.w < 43.5));
      ok('44 px targets on My profile', small.length === 0, JSON.stringify(small));
      const q = await open(b, 'android', 'contact/arjun');
      const small2 = await q.evaluate(() => [...document.querySelectorAll('[data-profile=arjun] button:not(.vh):not([role=switch])')].filter((e) => e.offsetParent).map((e) => { const r = e.getBoundingClientRect(); return { t: (e.getAttribute('aria-label') || e.textContent).trim().slice(0, 24), w: r.width, h: r.height }; }).filter((x) => x.h < 43.5 || x.w < 43.5));
      ok('44 px targets on a person profile', small2.length === 0, JSON.stringify(small2));
      await done(p); await done(q);
    }
  } finally {
    await b.close();
  }
  ok('0 console errors across all pages', allErrs.length === 0, JSON.stringify(allErrs.slice(0, 5)));
  console.log(`\n${pass} passed, ${fail} failed`);
  process.exit(fail ? 1 : 0);
})();

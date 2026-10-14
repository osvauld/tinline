/* Gate flow test: node tests/gate.flow.js  (dev server on :5178). Prints PASS/FAIL per check, exit 1 on any failure.
   SHOTS=<dir> additionally screenshots every step (android+desktop x light+dark) into <dir>. */
import { createRequire } from 'module';
const require = createRequire(import.meta.url);
const L = require('/home/abe/p2p_chat/.claude/worktrees/design-refinement/docs/design/refined/qa/lib.js');
const BASE = process.env.BASE || process.env.PG_URL || 'http://127.0.0.1:5178/';
let fails = 0;
const ok = (c, m) => { console.log((c ? 'PASS ' : 'FAIL ') + m); if (!c) fails++; };
const step = (p) => p.locator('[data-gate-step]').getAttribute('data-gate-step');
const waitStep = (p, s) => p.waitForSelector(`[data-gate-step="${s}"] .pane h1`, { timeout: 4000 }).then(() => p.waitForTimeout(40)).then(async () => {
  await p.waitForFunction((s) => document.querySelectorAll('.pane').length === 1 && document.querySelector('[data-gate-step]').dataset.gateStep === s, s, { timeout: 4000 });
});

async function drawer(p, fn) {
  await p.getByRole('button', { name: 'Simulation', exact: true }).click();
  await fn(p.locator('aside[aria-label=Simulation]'));
  await p.getByRole('button', { name: 'Close simulation' }).click();
}
async function appState(p, v) { await drawer(p, (d) => d.getByLabel('App state').selectOption(v)); }
async function open(b, form, dark, motionReduce) {
  const w = form === 'android' ? 600 : 1440;
  const p = await L.page(b, { w, h: 900, dark });
  if (motionReduce) await p.emulateMedia({ reducedMotion: 'reduce' });
  await p.goto(`${BASE}?form=${form}&theme=${dark ? 'dark' : 'light'}`);
  await p.waitForSelector('.device');
  await appState(p, 'onboarding');
  await waitStep(p, 'welcome');
  return p;
}
const btn = (p, n) => p.getByRole('button', { name: n, exact: true });
const next = (p) => btn(p, 'Continue');

async function createPath(p, form) {
  await btn(p, 'Create my account').click(); await waitStep(p, 'name');
  ok(await p.getByRole('heading', { name: 'What should people call you?' }).isVisible(), `[${form}] name step`);
  ok(await p.evaluate(() => document.activeElement?.tagName) === 'INPUT', `[${form}] name field focused`);
  ok(await next(p).isDisabled(), `[${form}] Continue disabled while name empty`);
  await p.getByLabel('Your nickname').fill('Test User'); await next(p).click(); await waitStep(p, 'pass');
  await p.getByLabel('Passphrase', { exact: true }).fill('purple elephant dance now');
  ok(await p.getByLabel(/^Confirm/).count() === 0, `[${form}] no confirm-passphrase field`);
  ok(await p.getByText('Strong').isVisible(), `[${form}] strength hint`);
  await p.getByRole('button', { name: 'Show passphrase' }).click();
  ok(await p.getByLabel('Passphrase', { exact: true }).getAttribute('type') === 'text', `[${form}] show/hide passphrase`);
  await next(p).click(); await waitStep(p, 'phrase');
  ok(await p.locator('[data-testid=phrase]').getAttribute('aria-hidden') === 'true', `[${form}] phrase hidden until revealed`);
  ok(await btn(p, 'I’ve saved them').isDisabled(), `[${form}] cannot continue before reveal`);
  await btn(p, 'Show the words').click();
  ok(await btn(p, 'I’ve saved them').isEnabled(), `[${form}] reveal enables continue`);
  // optional check: open, flag a wrong word, fix, return to the phrase
  await btn(p, 'Check my copy').click(); await waitStep(p, 'confirm');
  await p.getByLabel('Word 4', { exact: true }).fill('wrong'); await p.getByLabel('Word 11', { exact: true }).fill('access'); await p.getByLabel('Word 19', { exact: true }).fill('across');
  await btn(p, 'Done').click();
  ok(await p.getByText('That isn’t word 4. Check your written copy.').isVisible(), `[${form}] check: wrong word flagged`);
  await p.getByLabel('Word 4', { exact: true }).fill('about'); await btn(p, 'Done').click(); await waitStep(p, 'phrase');
  ok(true, `[${form}] optional check returns to the phrase`);
  await btn(p, 'I’ve saved them').click(); await waitStep(p, 'device');
  ok(await p.locator('.prog small').innerText() === '4 of 6', `[${form}] progress shows shorter flow (4 of 6)`);
  ok((await p.getByLabel('Device name').inputValue()) === (form === 'android' ? 'Personal phone' : 'Home computer'), `[${form}] device name default`);
  await next(p).click(); await waitStep(p, 'terms');
  ok(await btn(p, 'Agree and continue').isDisabled(), `[${form}] terms need consent`);
  await p.getByRole('checkbox').check(); await btn(p, 'Agree and continue').click(); await waitStep(p, 'perms');
  ok(await btn(p, 'Not now').isVisible(), `[${form}] permissions allow Not now`);
  await p.getByRole('button', { name: 'Allow microphone' }).click();
  ok(await p.getByRole('img', { name: 'Microphone allowed' }).isVisible(), `[${form}] permission granted state`);
  await btn(p, 'Continue').click(); await waitStep(p, 'done');
  await btn(p, 'Open Tinline').click();
  await p.waitForSelector('nav[aria-label=Main]');
  ok(true, `[${form}] create path reaches the app`);
  if (form === 'desktop') ok((await p.evaluate(() => document.body.innerText)).includes('Test'), `[${form}] account name applied (sidebar)`);
}

(async () => {
  const b = await L.launch();
  for (const form of ['android', 'desktop']) { const p = await open(b, form, false); await createPath(p, form); ok(p.errs.length === 0, `[${form}] no console errors ${p.errs.join('|')}`); await p.context().close(); }

  // ---- direction of the step transition (forward vs back), sampled per frame
  { const p = await open(b, 'android', false);
    const clickSample = (sel, text) => p.evaluate(([sel, text]) => new Promise((res) => {
      const el = [...document.querySelectorAll(sel)].find((e) => !text || e.textContent.trim() === text || e.getAttribute('aria-label') === text); el.click();
      const xs = []; let n = 0; const f = () => { xs.push([...document.querySelectorAll('.pane')].map((e) => new DOMMatrix(getComputedStyle(e).transform).m41));
        if (++n < 14) requestAnimationFrame(f); else res(xs); }; requestAnimationFrame(f); }), [sel, text]);
    const fwd = await clickSample('button', 'Create my account'); const flat = fwd.flat();
    ok(flat.some((x) => x > 1) && flat.some((x) => x < -1), 'forward: incoming starts at inline-end, outgoing leaves toward inline-start');
    ok(new Set(flat.map((x) => Math.round(x))).size > 4, 'step transition interpolates (several distinct offsets)');
    await waitStep(p, 'name');
    const bk = await clickSample('button', 'Back'); const f2 = bk.flat();
    ok(f2.some((x) => x < -1) && f2.some((x) => x > 1), 'back: slides the other way');
    if (process.env.DBG) console.log(JSON.stringify(bk));
    const two = bk.find((fr) => fr.length === 2);
    ok(!!two && two[1] < -1, `back: incoming enters from inline-start, outgoing leaves toward inline-end (${two})`);
    await p.context().close(); }

  // ---- link path: approve
  { const p = await open(b, 'android', false);
    await btn(p, 'Link my other device').click(); await waitStep(p, 'linkName');
    await next(p).click(); await waitStep(p, 'link');
    ok(await p.getByRole('img', { name: /QR code/ }).isVisible(), '[link] QR shown on the new device');
    ok(/Expires in 4:5\d|Expires in 5:00/.test(await p.getByRole('timer').innerText()), '[link] expiry countdown');
    await drawer(p, (d) => d.getByRole('button', { name: 'Other device scans' }).click());
    await p.waitForSelector('text=482 913');
    ok(await p.getByRole('heading', { name: 'Same code on both?' }).isVisible(), '[link] 6-digit code shown after scan');
    await drawer(p, (d) => d.getByRole('button', { name: 'Other device approves' }).click());
    await waitStep(p, 'linkPass');
    ok(await p.getByRole('heading', { name: 'Protect this phone' }).isVisible(), '[link] approve -> protect this phone');
    await btn(p, 'Skip for now').click(); await waitStep(p, 'terms');
    await p.getByRole('checkbox').check(); await btn(p, 'Agree and continue').click(); await waitStep(p, 'perms');
    await btn(p, 'Not now').click(); await waitStep(p, 'done');
    ok(await p.getByRole('heading', { name: /^Linked to/ }).isVisible(), '[link] done copy names the other device');
    await btn(p, 'Open Tinline').click(); await p.waitForSelector('nav[aria-label=Main]'); ok(true, '[link] reaches the app');
    await p.context().close(); }

  // ---- link path: expire, cancel, fail
  { const p = await open(b, 'android', false);
    await btn(p, 'Link my other device').click(); await waitStep(p, 'linkName');
    await next(p).click(); await waitStep(p, 'link');
    await drawer(p, (d) => d.getByRole('button', { name: 'Code expires' }).click());
    await p.waitForSelector('text=The code expired. Try again.', { timeout: 3000 });
    ok(true, '[link] expiry shows the real "code expired" copy');
    await btn(p, 'Try again').click(); await p.waitForSelector('role=img[name=/QR code/]');
    ok(true, '[link] Try again makes a fresh QR');
    await drawer(p, (d) => d.getByRole('button', { name: 'Other device cancels' }).click());
    ok(await p.getByText('Cancelled on the other device.').isVisible(), '[link] cancel message');
    await btn(p, 'Try again').click();
    await drawer(p, async (d) => { await d.getByLabel('Failure reason').selectOption('mismatch'); await d.getByRole('button', { name: 'Fail', exact: true }).click(); });
    ok(await p.getByText(/don’t belong to the same account/).isVisible(), '[link] mismatch failure copy');
    await btn(p, 'Restore with recovery phrase instead').click(); await waitStep(p, 'restore');
    ok(true, '[link] failure offers restore from phrase');
    await p.context().close(); }

  // ---- restore with a wrong word, then fixed; and "phrase restore fails"
  { const p = await open(b, 'android', false);
    await btn(p, 'Restore my account').click(); await waitStep(p, 'restore');
    const words = 'abandon ability able about abondon absent absorb abstract absurd abuse access accident account accuse achieve acid acoustic acquire across act action actor actress actual'.split(' ');
    await p.getByLabel('Word 1', { exact: true }).fill(words.join(' '));
    await next(p).click();
    ok(await p.getByText(/Word 5 isn’t in the recovery word list/).isVisible(), '[restore] wrong word named in error');
    ok(await p.getByLabel('Word 5', { exact: true }).getAttribute('aria-invalid') === 'true', '[restore] wrong field marked invalid');
    ok(await p.evaluate(() => document.activeElement?.getAttribute('aria-invalid')) === 'true', '[restore] focus moves to the bad word');
    await p.getByLabel('Word 5', { exact: true }).fill('abs');
    ok(await p.getByRole('button', { name: 'absent' }).isVisible(), '[restore] suggestions for a prefix');
    await p.getByRole('button', { name: 'absent' }).click();
    await next(p).click(); await waitStep(p, 'restoreName');
    await p.getByLabel('Your nickname').fill('Test User'); await next(p).click(); await waitStep(p, 'restorePass');
    await drawer(p, (d) => d.getByLabel('Phrase restore fails').click());
    await btn(p, 'Skip for now').click(); await waitStep(p, 'restore');
    ok(await p.getByText('That phrase isn’t valid. Check each word and their order.').isVisible(), '[restore] simulated restore failure returns to words with error');
    await drawer(p, (d) => d.getByLabel('Phrase restore fails').click());
    await next(p).click(); await waitStep(p, 'restoreName'); await next(p).click(); await waitStep(p, 'restorePass');
    await btn(p, 'Skip for now').click(); await waitStep(p, 'device');
    ok(true, '[restore] success continues to device name');
    await p.context().close(); }

  // ---- unlock: wrong x N then right
  { const p = await L.page(b, { w: 600, h: 900 }); await p.goto(BASE + '?form=android&theme=light'); await p.waitForSelector('.device');
    await drawer(p, async (d) => { await d.getByLabel('Tries before lockout').fill('3'); await d.getByLabel('Lockout seconds').fill('2'); await d.getByLabel('Key derivation time').fill('100'); });
    await appState(p, 'locked'); await waitStep(p, 'unlock');
    ok(await p.getByRole('heading', { name: 'Welcome back, Maya' }).isVisible(), '[unlock] screen');
    ok(await p.getByRole('switch', { name: 'Remember on this phone' }).isVisible(), '[unlock] Android shows Remember on this phone');
    for (let i = 0; i < 3; i++) { await p.getByLabel('Passphrase', { exact: true }).fill('nope' + i); await btn(p, 'Unlock').click(); await p.waitForSelector(i < 2 ? 'text=didn’t open the account' : 'text=Too many tries', { timeout: 3000 }); await p.waitForTimeout(150); }
    ok(await p.getByText(/You can try again in \d:\d\d/).isVisible(), '[unlock] lockout wait shown after N wrong');
    ok(await p.getByLabel('Passphrase', { exact: true }).isDisabled(), '[unlock] field disabled during lockout');
    await p.waitForSelector('text=/try again in/', { state: 'detached', timeout: 6000 });
    await p.getByLabel('Passphrase', { exact: true }).fill('correct horse'); await btn(p, 'Unlock').click();
    await p.waitForSelector('nav[aria-label=Main]', { timeout: 4000 }); ok(true, '[unlock] right passphrase opens the app');
    await appState(p, 'locked'); await waitStep(p, 'unlock'); await btn(p, 'Forgot passphrase?').click(); await waitStep(p, 'restore');
    ok(await p.getByText(/You’ll set a new passphrase/).isVisible(), '[unlock] Forgot passphrase -> restore from phrase');
    await p.context().close(); }

  // ---- welcome offers Link and Restore on both forms; restore on a device with an account ADDS one
  for (const form of ['desktop', 'android']) {
    const p = await open(b, form, false);
    ok(await btn(p, 'Link my other device').isVisible() && await btn(p, 'Restore my account').isVisible() && await btn(p, 'Create my account').isVisible(), `[${form}] welcome offers Create, Link and Restore`);
    const bx = await Promise.all(['Create my account', 'Link my other device', 'Restore my account'].map((n) => btn(p, n).boundingBox()));
    const frame = await p.locator('.card').boundingBox();
    ok(bx.every((r) => r && r.x >= frame.x - 1 && r.x + r.width <= frame.x + frame.width + 1), `[${form}] welcome buttons stay inside the card`);
    await p.context().close();
  }
  { const p = await L.page(b, { w: 1440, h: 900 }); await p.goto(BASE + '?form=desktop&theme=light#/settings'); await p.waitForSelector('.device');
    await p.waitForSelector('[data-accounts]');
    ok(/Only the selected account is online/.test(await p.locator('[data-accounts]').innerText()), '[add] settings says only the selected account is online');
    await btn(p, 'Add another account').click(); await waitStep(p, 'welcome');
    ok(await p.getByRole('heading', { name: 'Add an account' }).isVisible(), '[add] welcome says Add an account');
    await btn(p, 'Restore my account').click(); await waitStep(p, 'restore');
    await drawer(p, (d) => d.getByRole('button', { name: 'Fill demo phrase (restore)' }).click());
    await next(p).click(); await waitStep(p, 'restoreName');
    await p.getByLabel('Your nickname').fill('Anu Joseph'); await next(p).click(); await waitStep(p, 'restorePass');
    await btn(p, 'Skip for now').click(); await waitStep(p, 'device');
    await next(p).click(); await waitStep(p, 'terms');
    await p.getByRole('checkbox').check(); await btn(p, 'Agree and continue').click(); await waitStep(p, 'perms');
    await btn(p, 'Not now').click(); await waitStep(p, 'done');
    ok(await p.getByRole('heading', { name: 'Added Anu Joseph to this computer' }).isVisible(), '[add] success reads "Added <name> to this computer"');
    if (process.env.SHOTS) await p.screenshot({ path: process.env.SHOTS + '/add-done-desktop.png' });
    await btn(p, 'Open Tinline').click(); await p.waitForSelector('[data-accounts]');
    const txt = await p.locator('[data-accounts]').innerText();
    ok(/Anu Joseph/.test(txt) && /Maya Kurian/.test(txt), '[add] both accounts listed in Settings');
    await p.getByRole('button', { name: 'Switch to Maya Kurian' }).click(); await p.waitForTimeout(200);
    ok(/Maya Kurian\s+Selected/.test(await p.locator('[data-accounts]').innerText()), '[add] switcher selects the other account');
    if (process.env.SHOTS) await p.screenshot({ path: process.env.SHOTS + '/accounts-desktop.png' });
    await p.context().close(); }

  // ---- reduced motion: step changes must not animate
  { const p = await open(b, 'android', false, true);
    ok(await p.evaluate(() => document.documentElement.dataset.motion) === 'reduce', '[reduced] data-motion=reduce from OS setting');
    const lens = p.evaluate(() => new Promise((res) => { const d = []; const f = () => { document.getAnimations().forEach((a) => { const t = a.effect?.getComputedTiming?.().duration; if (a.effect?.target?.classList?.contains('pane')) d.push(+t); }); }; const mo = new MutationObserver(f); mo.observe(document.body, { childList: true, subtree: true }); setTimeout(() => res(d), 700); }));
    await btn(p, 'Create my account').click(); await waitStep(p, 'name');
    const d = await lens; ok(d.every((x) => x <= 1), `[reduced] pane transition durations <=1ms (${d.join(',') || 'none'})`);
    await p.context().close(); }

  if (process.env.SHOTS) await (await import('./gate.shots.js')).default(b, process.env.SHOTS, { drawer, appState, waitStep, open });
  await b.close();
  console.log(fails ? `\n${fails} FAILED` : '\nALL PASSED'); process.exit(fails ? 1 : 0);
})().catch((e) => { console.error('CRASH', e); process.exit(2); });

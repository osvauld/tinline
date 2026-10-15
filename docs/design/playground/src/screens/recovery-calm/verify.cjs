// Browser-only fixture checks. This does not validate native recovery, authentication or cryptography.
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const L = require('../../../../refined/qa/lib.js');
const BASE = process.env.RECOVERY_LAB_URL || 'http://127.0.0.1:5180/src/screens/recovery-calm/index.html';
const OUT = process.env.RECOVERY_LAB_SHOTS || '/tmp/claude-1000/-home-abe-p2p-chat/6f1f15a0-05df-4e79-8378-c444aaf8d38e/scratchpad/pg/recovery-calm';
const run = Date.now();
const settle = p => p.waitForFunction(() => [...document.querySelectorAll('[data-phone] .stage')].every(n => getComputedStyle(n).opacity === '1'));
const select = async (p, flow) => { await p.locator(`[data-flow=${flow}]`).click(); await settle(p); };
const phone = p => p.locator('[data-phone]');
const screen = (p, name) => p.locator(`[data-page=${name}]`).waitFor();
async function noInnerScroll(p) {
  const sizes = await phone(p).locator('.scroll').evaluate(n => ({ height: n.clientHeight, content: n.scrollHeight, width: n.clientWidth, contentWidth: n.scrollWidth }));
  assert(sizes.content <= sizes.height + 1, `Unnecessary inner scroll: ${JSON.stringify(sizes)}`);
  assert(sizes.contentWidth <= sizes.width + 1, `Inner horizontal overflow: ${JSON.stringify(sizes)}`);
}
async function shot(p, name, report) {
  await settle(p);
  const file = path.join(OUT, `${run}-${name}.png`);
  await p.screenshot({ path: file, fullPage: true }); report.screenshots.push(file);
}
async function unlock(p) {
  await phone(p).locator('[data-profile-recovery]').click();
  await phone(p).locator('#auth-pass').fill('correct horse');
  await phone(p).getByRole('button', { name: 'Continue', exact: true }).click();
  await screen(p, 'backup');
}
(async () => {
  fs.mkdirSync(OUT, { recursive: true });
  const b = await L.launch();
  const report = { result: 'PASS', screenshots: [], assertions: [] };
  try {
    for (const mobile of [false, true]) for (const dark of [false, true]) {
      const p = await L.page(b, { w: mobile ? 600 : 1440, h: 900, mobile, dark });
      // Record only sample data, never a real clipboard. The UI still awaits the Promise.
      await p.addInitScript(() => {
        Object.defineProperty(navigator, 'clipboard', { configurable: true, value: {
          writeText: async value => { window.__sampleClipboard = value; },
        } });
        Object.defineProperty(navigator, 'share', { configurable: true, value: async data => { window.__sampleShare = data; } });
      });
      await p.goto(`${BASE}?theme=${dark ? 'dark' : 'light'}`);
      const suffix = `${mobile ? 'android' : 'desktop'}-${dark ? 'dark' : 'light'}`;
      await screen(p, 'share');
      await settle(p);
      await noInnerScroll(p);
      assert.equal(await phone(p).locator('[data-username]').textContent(), 'maya');
      await shot(p, `${suffix}-share`, report);
      const buttonPosition = () => phone(p).locator('[data-copy-contact]').evaluate(n => {
        const r = n.getBoundingClientRect(), frame = n.closest('[data-phone]').getBoundingClientRect();
        return [r.x - frame.x, r.y - frame.y, r.width, r.height];
      });
      const before = await buttonPosition();
      await phone(p).locator('[data-copy-contact]').click();
      await phone(p).getByRole('button', { name: 'Copied', exact: true }).waitFor();
      assert.match(await p.evaluate(() => window.__sampleClipboard), /^OSVC2:/);
      const after = await buttonPosition();
      assert.deepEqual(after, before, 'Copy feedback moves its primary action');
      await phone(p).locator('[data-share-contact]').click();
      assert.deepEqual(await p.evaluate(() => window.__sampleShare), { text: await p.evaluate(() => window.__sampleClipboard) });
      await phone(p).getByRole('button', { name: 'Show QR instead' }).click();
      await screen(p, 'qr');
      await noInnerScroll(p);
      assert.equal(await phone(p).getByRole('img', { name: /Sample contact QR/ }).count(), 1);
      await shot(p, `${suffix}-qr`, report);
      await phone(p).locator('[data-choose-image]').click();
      await screen(p, 'pickImage');
      await phone(p).getByRole('button', { name: 'Cancel', exact: true }).click();
      await screen(p, 'qr');
      for (const [kind, message] of [['none', 'No QR found'], ['multiple', 'More than one QR']]) {
        await phone(p).locator('[data-choose-image]').click();
        await phone(p).locator(`[data-image=${kind}]`).click();
        await phone(p).getByRole('status').filter({ hasText: message }).waitFor();
      }
      await phone(p).locator('[data-choose-image]').click();
      await shot(p, `${suffix}-image-picker`, report);
      await phone(p).locator('[data-image=contact]').click();
      await screen(p, 'contactPreview');
      assert.equal(await phone(p).locator('[data-found-username]').textContent(), 'tara');
      assert.equal(await phone(p).getByText('Read from image', { exact: true }).count(), 1);
      const requests = () => p.evaluate(async () => {
        const { pp } = await import('/src/lib/model/actions/people.svelte.ts'); return pp.sent.length;
      });
      assert.equal(await requests(), 0, 'Choosing an image sent a request without confirmation');
      await phone(p).locator('[data-confirm-contact]').click();
      await screen(p, 'requestSent');
      assert.equal(await requests(), 1);
      await select(p, 'restore');
      assert.equal(await phone(p).getByRole('button', { name: 'Continue', exact: true }).isDisabled(), true);
      await phone(p).locator('#recovery-input').fill('tlkey-wrong');
      await phone(p).getByRole('button', { name: 'Continue', exact: true }).click();
      await phone(p).getByRole('status').filter({ hasText: 'Check your recovery key' }).waitFor();
      await p.locator('[data-use-sample]').click();
      await shot(p, `${suffix}-restore`, report);
      await phone(p).getByRole('button', { name: 'Continue', exact: true }).click();
      await screen(p, 'review');
      assert.equal(await phone(p).locator('#recovery-input').count(), 0);
      assert.equal((await phone(p).textContent()).includes('tlkey-'), false);
      await phone(p).getByRole('button', { name: 'Use this account' }).click();
      await screen(p, 'protect');
      await phone(p).getByRole('button', { name: 'Not now' }).click();
      await screen(p, 'done');
      await phone(p).getByRole('button', { name: 'Continue', exact: true }).click();
      await screen(p, 'profile');
      await shot(p, `${suffix}-profile`, report);
      await unlock(p);
      assert.equal(await phone(p).locator('[data-copy-key]').isDisabled(), true);
      assert.equal(await phone(p).locator('[data-share-contact]').count(), 0, 'Recovery key has a Share action');
      assert.equal(await phone(p).locator('[data-recovery-value]').count(), 0);
      await phone(p).locator('[data-reveal]').click();
      await phone(p).locator('[data-copy-key]').click();
      const key = await p.evaluate(() => window.__sampleClipboard);
      assert.match(key, /^tlkey-/);
      await shot(p, `${suffix}-backup`, report);
      await phone(p).getByRole('button', { name: '24 words', exact: true }).click();
      assert.equal(await phone(p).locator('ol li').count(), 0, 'Switching format reveals words without consent');
      await phone(p).locator('[data-reveal]').click();
      assert.equal(await phone(p).locator('ol li').count(), 24);
      assert.equal(await phone(p).locator('[data-copy-key]').count(), 0, 'Word-copy policy silently changed');
      await phone(p).getByRole('button', { name: 'Back', exact: true }).click();
      assert.equal((await phone(p).textContent()).includes(key), false);
      await phone(p).locator('[data-profile-share]').click();
      await phone(p).locator('[data-copy-contact]').click();
      assert.match(await p.evaluate(() => window.__sampleClipboard), /^OSVC2:/, 'Recovery secret leaked into contact sharing');
      const o = await L.overflow(p); assert(o.sw <= o.cw, `Overflow: ${JSON.stringify(o)}`);
      assert.deepEqual(p.errs, []); assert.deepEqual(p.fails, []);
      await p.close();
    }
    report.assertions.push('Four viewport/theme combinations; shared identity and contact fixtures; key and word restore; profile shortcut; gated backup; no console errors or failed requests.');
    const p = await L.page(b, { w: 600, h: 900, mobile: true });
    await p.addInitScript(() => Object.defineProperty(navigator, 'clipboard', { configurable: true, value: { writeText: async () => {} } }));
    await p.goto(`${BASE}?theme=light`);
    await p.getByLabel('Clipboard unavailable', { exact: true }).check();
    await phone(p).locator('[data-copy-contact]').click();
    await phone(p).getByRole('status').filter({ hasText: 'Couldn’t copy' }).waitFor();
    assert.equal(await phone(p).getByRole('button', { name: 'Copied', exact: true }).count(), 0);
    await p.getByLabel('Clipboard unavailable', { exact: true }).uncheck();
    await p.evaluate(() => Object.defineProperty(navigator, 'share', { configurable: true, value: undefined }));
    await phone(p).locator('[data-share-contact]').click();
    await phone(p).getByRole('status').filter({ hasText: 'Sharing isn’t available' }).waitFor();
    await p.evaluate(() => Object.defineProperty(navigator, 'share', { configurable: true, value: async () => { throw new DOMException('Cancelled', 'AbortError'); } }));
    await phone(p).locator('[data-share-contact]').click();
    assert.equal((await phone(p).getByRole('status').textContent()).trim(), '');
    await p.evaluate(() => Object.defineProperty(navigator, 'share', { configurable: true, value: async () => { throw new DOMException('Denied', 'NotAllowedError'); } }));
    await phone(p).locator('[data-share-contact]').click();
    await phone(p).getByRole('status').filter({ hasText: 'Couldn’t open sharing' }).waitFor();
    await select(p, 'restore');
    await phone(p).getByRole('button', { name: '24 words', exact: true }).click();
    await p.locator('[data-use-sample]').click();
    await phone(p).getByRole('button', { name: 'Continue', exact: true }).click();
    await screen(p, 'review');
    await select(p, 'profile');
    await phone(p).locator('[data-profile-recovery]').click();
    await phone(p).locator('#auth-pass').fill('wrong');
    await phone(p).getByRole('button', { name: 'Continue', exact: true }).click();
    await phone(p).getByRole('status').filter({ hasText: 'isn’t right' }).waitFor();
    assert.equal(await phone(p).locator('[data-secret]').count(), 0);
    await select(p, 'profile');
    await unlock(p);
    await phone(p).locator('[data-reveal]').click();
    await p.evaluate(() => window.dispatchEvent(new Event('blur')));
    await screen(p, 'auth');
    assert.equal(await phone(p).locator('[data-recovery-value]').count(), 0);
    await p.getByLabel('Account has a passphrase', { exact: true }).uncheck();
    await phone(p).getByRole('button', { name: 'Use screen lock', exact: true }).click();
    await screen(p, 'backup');
    await phone(p).locator('[data-reveal]').click();
    // Older shared Playwright has no page.clock; exercise the real security timeout instead.
    await phone(p).locator('[data-secret][data-shown="false"]').waitFor({ timeout: 32000 });
    assert.equal(await phone(p).locator('[data-secret]').getAttribute('data-shown'), 'false');
    assert.equal(await phone(p).locator('[data-copy-key]').isDisabled(), true);
    await p.getByLabel('Motion', { exact: true }).selectOption('reduce');
    await p.getByLabel('Right to left', { exact: true }).check();
    await select(p, 'share');
    await phone(p).getByRole('button', { name: 'Show QR instead' }).click();
    assert.equal(await phone(p).getAttribute('dir'), 'rtl');
    assert.equal(await p.evaluate(() => document.documentElement.dataset.motion), 'reduce');
    await shot(p, 'rtl-reduced-qr', report);
    const targets = await phone(p).locator('button').evaluateAll(nodes => nodes.map(n => {
      const r = n.getBoundingClientRect(); return { text: n.textContent, w: r.width, h: r.height };
    }));
    assert(targets.every(t => t.w >= 44 && t.h >= 44), JSON.stringify(targets));
    await p.setViewportSize({ width: 360, height: 800 });
    await noInnerScroll(p);
    const overflow = await L.overflow(p); assert(overflow.sw <= overflow.cw);
    assert.deepEqual(p.errs, []); assert.deepEqual(p.fails, []);
    await p.close();
    report.assertions.push('Clipboard refusal never reports success; Share passes only bare contact code and handles absence, cancellation and failure; recovery has no Share button; simulated image-picker cancel/no-QR/multiple-QR/confirmation paths; no request before confirmation; wrong passphrase cannot reveal; backgrounding locks; format switching hides; real 30-second security timeout; no word-copy button; RTL/reduced motion; 44 px targets; 360 px viewport; sharing and QR fit without inner scrolling; intact usernames.');
    fs.writeFileSync(`/tmp/recovery-calm-${run}-report.json`, JSON.stringify(report, null, 2));
    console.log(JSON.stringify(report, null, 2));
  } finally { await b.close(); }
})().catch(e => { console.error(e); process.exit(1); });

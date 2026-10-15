// Deterministic browser checks for this isolated motion study, not Android integration proof.
// Run from the repository root via scripts/buildlock.py --run -- node <this file>.
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const L = require('../../../../refined/qa/lib.js');
const BASE = process.env.TAB_LAB_URL || 'http://127.0.0.1:5178/src/screens/tab-motion/index.html';
const OUT = process.env.TAB_LAB_SHOTS || '/tmp/claude-1000/-home-abe-p2p-chat/6f1f15a0-05df-4e79-8378-c444aaf8d38e/scratchpad/pg/tab-motion';
const run = Date.now();
const kinds = ['through', 'crossfade', 'slide'];
const settle = (p) => p.waitForFunction(() => [...document.querySelectorAll('[data-phone]')].every(n => n.querySelectorAll('[data-panel]').length === 1 && getComputedStyle(n.querySelector('[data-panel]')).opacity === '1'));
const snapshot = (p) => p.evaluate(() => [...document.querySelectorAll('[data-phone]')].map(n => ({
  kind: n.dataset.phone, tab: n.querySelector('[role=tab][aria-selected=true]').dataset.tab,
  panels: [...n.querySelectorAll('[data-panel]')].map(x => x.dataset.panel),
})));

(async () => {
  fs.mkdirSync(OUT, { recursive: true });
  const b = await L.launch();
  const report = { screenshots: [], assertions: [], frames: {} };
  try {
    for (const mobile of [false, true]) for (const dark of [false, true]) {
      const p = await L.page(b, { w: mobile ? 600 : 1440, h: 900, dark, mobile });
      await p.goto(`${BASE}?theme=${dark ? 'dark' : 'light'}`);
      await p.locator('[data-phone=slide] [data-panel=chats]').waitFor();
      await settle(p);
      const name = path.join(OUT, `${run}-${mobile ? 'android' : 'desktop'}-${dark ? 'dark' : 'light'}.png`);
      await p.screenshot({ path: name, fullPage: true });
      report.screenshots.push(name);
      assert.deepEqual(p.errs, []);
      assert.deepEqual(p.fails, []);
      const overflow = await L.overflow(p);
      assert(overflow.sw <= overflow.cw, `Horizontal overflow: ${JSON.stringify(overflow)}`);
      // Contacts has no top search field, but every preview keeps its bottom search dock.
      await p.locator('[data-compare=contacts]').click();
      await settle(p);
      assert.equal(await p.locator('[data-phone] .dock .search').count(), 3);
      assert.equal(await p.locator('[data-panel=contacts] input[type=search]').count(), 0);
      await p.close();
    }
    report.assertions.push('Four viewport/theme screenshots; no horizontal overflow, console errors or failed requests; Contacts bottom-search layout.');

    const p = await L.page(b, { w: 1440, h: 900 });
    await p.goto(`${BASE}?theme=light`);
    await p.locator('[data-phone=slide]').waitFor();
    await settle(p);
    await p.evaluate(() => document.fonts.ready);
    // Warm all three list mounts before measuring; first-load compilation/font work is not transition choreography.
    await p.locator('[data-compare=groups]').click(); await settle(p);
    await p.locator('[data-compare=chats]').click(); await settle(p);
    const frames = await p.evaluate(async () => {
      const start = performance.now();
      const samples = [];
      const chrome = () => [...document.querySelectorAll('[data-phone]')].map(n => ['.appbar', '[data-dock]', '[role=tablist]'].map(s => {
        const r = n.querySelector(s).getBoundingClientRect(); return [r.x, r.y, r.width, r.height];
      }));
      const fixed = chrome();
      document.querySelector('[data-compare=groups]').click();
      await new Promise(resolve => {
        function frame(now) {
          samples.push({ at: now - start, chrome: chrome(), panels: [...document.querySelectorAll('[data-phone]')].map(n => {
            const el = n.querySelector('[data-panel=groups]');
            if (!el) return null;
            const css = getComputedStyle(el);
            return { kind: n.dataset.phone, opacity: Number(css.opacity), transform: css.transform };
          }) });
          if (now - start < 600) requestAnimationFrame(frame); else resolve();
        }
        requestAnimationFrame(frame);
      });
      return { fixed, samples };
    });
    fs.writeFileSync(path.join(OUT, `${run}-frames.json`), JSON.stringify(frames, null, 2));
    for (const kind of kinds) {
      const samples = frames.samples.flatMap(f => f.panels.filter(x => x?.kind === kind));
      assert(samples.filter(s => s.opacity > 0 && s.opacity < 1).length >= 2, `${kind} did not interpolate`);
      assert.equal(samples.at(-1).opacity, 1);
      if (kind === 'slide') assert(samples.some(s => s.transform !== 'none' && Math.abs(Number(s.transform.split(',')[4])) > .1), 'No lateral movement');
      report.frames[kind] = samples;
    }
    for (const frame of frames.samples) assert.deepEqual(frame.chrome, frames.fixed, 'Fixed chrome moved during a tab transition');
    report.assertions.push('Frame-sampled opacity interpolates in all three treatments; lateral motion exists only in slide; app bar/dock/nav stay fixed.');

    // Same-tab taps must not start a new transition.
    await p.locator('[data-compare=groups]').click();
    assert((await snapshot(p)).every(x => x.panels.length === 1 && x.tab === 'groups'));
    // Rapid reversals and non-adjacent jumps must settle to the last destination without leftover panels.
    await p.evaluate(async () => {
      for (const t of ['chats', 'contacts', 'calls', 'groups', 'chats', 'contacts']) {
        document.querySelector(`[data-compare=${t}]`).click();
        await new Promise(requestAnimationFrame);
      }
    });
    await settle(p);
    assert((await snapshot(p)).every(x => x.panels.length === 1 && x.tab === 'contacts'));
    report.assertions.push('Same-tab no-op, rapid reversals and non-adjacent jumps leave exactly one correct panel.');

    await p.getByLabel('Switch together').uncheck();
    await p.locator('[data-phone=crossfade] [data-tab=calls]').click();
    await settle(p);
    assert.deepEqual((await snapshot(p)).map(x => x.tab), ['contacts', 'calls', 'contacts']);
    await p.locator('[data-phone=crossfade] [data-tab=calls]').press('ArrowLeft');
    await settle(p);
    assert.equal((await snapshot(p))[1].tab, 'groups');
    await p.getByLabel('RTL', { exact: true }).check();
    await p.locator('[data-phone=crossfade] [data-tab=groups]').press('ArrowRight');
    await settle(p);
    assert.equal((await snapshot(p))[1].tab, 'chats');
    report.assertions.push('Independent previews and keyboard tab navigation, including RTL.');
    await p.getByLabel('RTL', { exact: true }).uncheck();

    await p.getByLabel('Motion', { exact: true }).selectOption('reduce');
    await p.locator('[data-compare=calls]').click();
    await settle(p);
    assert((await snapshot(p)).every(x => x.panels.length === 1 && x.tab === 'calls'));
    assert.equal(await p.locator('html').getAttribute('data-motion'), 'reduce');
    assert.deepEqual(await p.locator('.caption > span').allTextContents(), ['Instant', 'Instant', 'Instant']);
    assert((await p.locator('[data-panel]').evaluateAll(nodes => nodes.map(n => getComputedStyle(n).transform))).every(t => t === 'none' || t === 'matrix(1, 0, 0, 1, 0, 0)'));
    report.assertions.push('Reduced motion completes without transition tails.');

    await p.getByLabel('Motion', { exact: true }).selectOption('full');
    await p.getByRole('button', { name: '¼ speed', exact: true }).click();
    assert.equal(await p.locator('[data-candidate=through] .caption > span').innerText(), '1200 ms');
    await p.locator('[data-compare=groups]').click();
    await settle(p);
    await p.locator('[data-compare=contacts]').click();
    await p.waitForTimeout(180);
    const mid = path.join(OUT, `${run}-slow-transition.png`);
    await p.screenshot({ path: mid, fullPage: true });
    report.screenshots.push(mid);
    await settle(p);
    const contactsShot = path.join(OUT, `${run}-contacts.png`);
    await p.screenshot({ path: contactsShot, fullPage: true });
    report.screenshots.push(contactsShot);
    await p.getByRole('button', { name: '1×', exact: true }).click();
    report.assertions.push('Slow-motion multiplier applies to token-based transition durations.');
    await p.getByRole('button', { name: 'Walk all tabs', exact: true }).click();
    await p.getByRole('button', { name: 'Stop walkthrough', exact: true }).waitFor();
    await p.locator('[data-compare=contacts]').click();
    await settle(p);
    await p.waitForTimeout(1200);
    assert((await snapshot(p)).every(x => x.tab === 'contacts'));
    assert.equal(await p.getByRole('button', { name: 'Walk all tabs', exact: true }).count(), 1);
    report.assertions.push('Manual navigation cancels the walkthrough without a later surprise tab change.');

    await p.setViewportSize({ width: 1440, height: 700 });
    await p.locator('[data-compare=chats]').click();
    await settle(p);
    const scrolls = await p.evaluate(() => [...document.querySelectorAll('[data-scroll=chats]')].map(n => {
      n.scrollTop = 80; n.dispatchEvent(new Event('scroll')); return n.scrollTop;
    }));
    assert(scrolls.every(x => x > 0), 'Test fixture must be scrollable');
    await p.locator('[data-compare=groups]').click(); await settle(p);
    await p.locator('[data-compare=chats]').click(); await settle(p);
    assert.deepEqual(await p.locator('[data-scroll=chats]').evaluateAll(nodes => nodes.map(n => n.scrollTop)), scrolls);
    report.assertions.push('Per-preview, per-tab scroll positions survive switching.');
    assert.deepEqual(p.errs, []); assert.deepEqual(p.fails, []);
    await p.close();
    fs.writeFileSync(path.join(OUT, `${run}-report.json`), JSON.stringify(report, null, 2));
    console.log(JSON.stringify({ result: 'PASS', ...report, frames: 'Saved in report JSON' }, null, 2));
  } finally { await b.close(); }
})().catch(e => { console.error(e); process.exitCode = 1; });

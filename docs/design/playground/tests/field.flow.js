/* Call-screen colour field textures (Fog / Clouds) against the running dev server (http://127.0.0.1:5178).
   Run: node tests/field.flow.js   (BASE=… to override). Prints PASS/FAIL per check. */
import { createRequire } from 'node:module';
const L = createRequire(import.meta.url)('/home/abe/p2p_chat/.claude/worktrees/design-refinement/docs/design/refined/qa/lib.js');
const BASE = process.env.BASE || 'http://127.0.0.1:5178/';
let pass = 0, fail = 0;
const ok = (name, cond, extra = '') => { cond ? pass++ : fail++; console.log((cond ? 'PASS ' : 'FAIL ') + name + (cond ? '' : ' ' + extra)); };
const wait = (p, ms) => p.waitForTimeout(ms);
const until = async (p, fn, ms = 4000) => { const t0 = Date.now(); while (Date.now() - t0 < ms) { if (await p.evaluate(fn)) return true; await wait(p, 50); } return false; };

async function open(b, { dark = false, reduced = false, form = 'desktop', tex = null } = {}) {
  const p = await L.page(b, { w: form === 'android' ? 600 : 1440, h: 900, dark });
  if (reduced) await p.emulateMedia({ reducedMotion: 'reduce' });
  if (tex) await p.addInitScript((t) => { if (!sessionStorage.getItem('seeded')) { localStorage.setItem('tinline.pg.field', t); sessionStorage.setItem('seeded', '1'); } }, tex);
  await p.goto(`${BASE}?form=${form}&theme=${dark ? 'dark' : 'light'}#/chat/arjun`);
  await p.waitForSelector('[data-form]'); await wait(p, 500);
  await p.evaluate(() => { window.__pg.CALL_MS.connect = 700; window.__pg.CALL_MS.ringingAfter = 8000; });
  return p;
}
const startCall = async (p) => { await p.getByRole('button', { name: 'Call', exact: true }).click(); await p.waitForSelector('.callscreen'); };
const setTex = (p, t) => p.evaluate((t) => window.__pgField.set(t), t);
const hangup = async (p) => { await p.evaluate(() => window.__pg.call.hangup()); await until(p, () => !document.querySelector('.callscreen'), 3000); };

/* canvas stats: luminance spread over the field, and a 24x40 signature for frame differences */
const SIG = `(() => {
  const cv = document.querySelector('.callscreen canvas'); if (!cv || !cv.width) return null;
  const o = document.createElement('canvas'); o.width = 24; o.height = 40; const x = o.getContext('2d', { willReadFrequently: true });
  x.drawImage(cv, 0, 0, 24, 40); const d = x.getImageData(0, 0, 24, 40).data; return Array.from(d);
})()`;
const sig = (p) => p.evaluate(SIG);
const spread = (d) => { const l = []; for (let i = 0; i < d.length; i += 4) l.push(.2126 * d[i] + .7152 * d[i + 1] + .0722 * d[i + 2]); const m = l.reduce((a, b) => a + b) / l.length; return { sd: Math.sqrt(l.reduce((a, b) => a + (b - m) ** 2, 0) / l.length), min: Math.min(...l), max: Math.max(...l) }; };
const diff = (a, b) => { let m = 0; for (let i = 0; i < a.length; i += 4) m += (Math.abs(a[i] - b[i]) + Math.abs(a[i + 1] - b[i + 1]) + Math.abs(a[i + 2] - b[i + 2])) / 3; return m / (a.length / 4); };
async function motion(p, ms) {   // mean frame-to-frame difference over a window
  await p.evaluate(`(() => { window.__m = { s: [], stop: false }; const f = () => { const d = ${SIG}; if (d) window.__m.s.push(d); if (!window.__m.stop) requestAnimationFrame(f); }; requestAnimationFrame(f); })()`);
  await wait(p, ms);
  const s = await p.evaluate(() => { window.__m.stop = true; return window.__m.s; });
  let t = 0; for (let i = 1; i < s.length; i++) t += diff(s[i - 1], s[i]);
  return { mean: s.length > 1 ? t / (s.length - 1) : 0, n: s.length };
}

async function nameStatusContrast(p) {
  return p.evaluate(() => {
    const cv = document.querySelector('.callscreen canvas'), host = cv.parentElement, hr = host.getBoundingClientRect(), k = cv.width / hr.width;
    const lin = (c) => { c /= 255; return c <= .03928 ? c / 12.92 : ((c + .055) / 1.055) ** 2.4; };
    const lum = (r, g, b) => .2126 * lin(r) + .7152 * lin(g) + .0722 * lin(b);
    const o = document.createElement('canvas'); o.width = cv.width; o.height = cv.height; const x = o.getContext('2d', { willReadFrequently: true }); x.drawImage(cv, 0, 0);
    const out = [];
    for (const el of document.querySelectorAll('.callscreen .name, .callscreen .status span, .callscreen .timer')) {
      const cs = getComputedStyle(el); if (+cs.opacity < .99) continue; const r = el.getBoundingClientRect(); if (r.width < 2) continue;
      const m = cs.color.match(/[\d.]+/g).map(Number), tl = lum(m[0], m[1], m[2]);
      const sx = Math.max(0, Math.floor((r.left - hr.left) * k)), sy = Math.max(0, Math.floor((r.top - hr.top) * k));
      const w = Math.max(1, Math.min(cv.width - sx, Math.ceil(r.width * k))), h = Math.max(1, Math.min(cv.height - sy, Math.ceil(r.height * k)));
      const d = x.getImageData(sx, sy, w, h).data; let worst = 99;
      for (let i = 0; i < d.length; i += 4) { const bl = lum(d[i], d[i + 1], d[i + 2]); worst = Math.min(worst, (Math.max(tl, bl) + .05) / (Math.min(tl, bl) + .05)); }
      out.push(+worst.toFixed(2));
    }
    return out;
  });
}

(async () => {
  const b = await L.launch();
  const pages = [];
  try {
    /* ===== default, live toggle through the drawer, no remount ===== */
    let p = await open(b); pages.push(p);
    ok('default texture is fog, nothing persisted yet', (await p.evaluate(() => window.__pgField.texture === 'fog' && localStorage.getItem('tinline.pg.field') === null)));
    await startCall(p); await p.evaluate(() => window.__pg.call.remote('answer')); await until(p, () => window.__pg.call.phase === 'active', 3000); await wait(p, 500);
    await p.evaluate(() => { document.querySelector('.callscreen').__mark = 1; document.querySelector('.callscreen canvas').__mark = 1; });
    const a0 = await sig(p);
    await p.getByRole('button', { name: 'Simulation' }).click();
    ok('drawer: "Call background" select offers Fog and Clouds', (await p.locator('#callsim-field option').allInnerTexts()).join() === 'Fog,Clouds');
    await p.locator('#callsim-field').selectOption('clouds');
    await wait(p, 200);
    ok('toggle: texture is clouds and persisted under tinline.pg.field', await p.evaluate(() => window.__pgField.texture === 'clouds' && localStorage.getItem('tinline.pg.field') === 'clouds'));
    ok('toggle: call screen and canvas were not remounted', await p.evaluate(() => !!document.querySelector('.callscreen').__mark && !!document.querySelector('.callscreen canvas').__mark));
    const a1 = await sig(p);
    ok('toggle: the frame changes live (fog -> clouds)', diff(a0, a1) > 1.2, 'diff ' + diff(a0, a1).toFixed(2));
    await p.locator('#callsim-field').selectOption('fog'); await wait(p, 200);
    ok('toggle back: fog, still the same canvas', await p.evaluate(() => window.__pgField.texture === 'fog' && !!document.querySelector('.callscreen canvas').__mark));
    await p.getByRole('button', { name: 'Close simulation' }).click();
    await hangup(p);

    /* ===== both textures, light and dark: non-flat, responds to the tune, name/status contrast ===== */
    for (const dark of [false, true]) for (const tex of ['fog', 'clouds']) {
      const tag = `${tex}/${dark ? 'dark' : 'light'}`;
      p = await open(b, { dark, tex }); pages.push(p);
      await p.evaluate(() => { window.__pg.call.receive('arjun'); });
      await wait(p, 300);
      if (await p.locator('[data-incoming-alert] .open').count()) await p.locator('[data-incoming-alert] .open').click();
      await p.waitForSelector('.callscreen').catch(() => {});
      if (!(await p.locator('.callscreen').count())) { await p.setViewportSize({ width: 1440, height: 900 }); }
      await wait(p, 900);
      ok(`${tag}: renders with WebGL`, (await p.locator('.callscreen [data-render]').getAttribute('data-render')) === 'webgl');
      const s0 = await sig(p), sp = spread(s0);
      ok(`${tag}: frame is not flat (luma sd ${sp.sd.toFixed(1)}, range ${sp.min.toFixed(0)}-${sp.max.toFixed(0)})`, sp.sd > 4 && sp.max - sp.min > 18);
      const cr = await nameStatusContrast(p);
      ok(`${tag}: name/status contrast >= 4.5 over the field (${cr.join(', ')})`, cr.length >= 2 && Math.min(...cr) >= 4.5);
      const ring = await motion(p, 1800);
      ok(`${tag}: field moves while ringing (mean frame diff ${ring.mean.toFixed(3)}, ${ring.n} frames)`, ring.mean > 0.01);
      await p.evaluate(() => window.__pg.call.remote('cancel')); await wait(p, 1200);
      if (!(await p.locator('.callscreen').count())) { await p.evaluate(() => window.__pg.call.hangup()); }
      await p.close();
    }

    /* notes steer the field: right after answering, the connect cue's notes move the field more than the settled call */
    for (const tex of ['fog', 'clouds']) {
      p = await open(b, { tex }); pages.push(p);
      await startCall(p);
      await p.evaluate(() => { window.__pg.CALL_MS.connect = 300; window.__pg.call.remote('answer'); });
      await until(p, () => window.__pg.call.phase === 'active', 3000);
      const during = await motion(p, 700);
      await wait(p, 2500);
      const settled = await motion(p, 1200);
      ok(`${tex}: frame difference rises on the connect notes (${during.mean.toFixed(3)} vs settled ${settled.mean.toFixed(3)})`, during.mean > settled.mean * 1.15);
      await hangup(p); await p.close();
    }

    /* ===== reduced motion: static between notes, both textures ===== */
    for (const tex of ['fog', 'clouds']) {
      p = await open(b, { reduced: true, tex }); pages.push(p);
      await p.evaluate(() => window.__pg.call.receive('arjun')); await wait(p, 300);
      if (await p.locator('[data-incoming-alert] .open').count()) await p.locator('[data-incoming-alert] .open').click();
      await wait(p, 800);
      const rs = await sig(p);
      ok(`reduced/${tex}: field is not flat`, rs && spread(rs).sd > 3);
      const m = await motion(p, 1000);
      ok(`reduced/${tex}: no drift between notes (mean frame diff ${m.mean.toFixed(4)})`, m.mean < 0.35);
      await setTex(p, tex === 'fog' ? 'clouds' : 'fog'); await wait(p, 300);
      ok(`reduced/${tex}: toggle still switches the texture`, diff(rs, await sig(p)) > .8);
      await p.evaluate(() => window.__pg.call.remote('cancel')); await wait(p, 400); await p.close();
    }

    /* ===== persisted choice survives a reload; Clouds also works with the CSS fallback ===== */
    p = await open(b); pages.push(p);
    await setTex(p, 'clouds');
    await p.reload(); await p.waitForSelector('[data-form]'); await wait(p, 400);
    ok('reload: clouds persisted', await p.evaluate(() => window.__pgField.texture === 'clouds'));
    await startCall(p); await wait(p, 600);
    ok('reload: the call screen draws with the persisted texture', (await spread(await sig(p))).sd > 3);
    await hangup(p);
    await p.goto(`${BASE}?form=desktop&theme=light&nogl=1#/chat/arjun`); await p.waitForSelector('[data-form]'); await wait(p, 400);
    await startCall(p); await wait(p, 500);
    const css = await p.locator('.callscreen [data-render]').evaluate((e) => ({ kind: e.dataset.render, bg: e.style.backgroundImage }));
    ok('no WebGL: clouds use the CSS fallback (layered radial gradients)', css.kind === 'css' && (css.bg.match(/radial-gradient/g) || []).length > 10, css.kind);
    await hangup(p);

    const errs = pages.flatMap((q) => q.errs || []);
    ok('0 console errors across all flows', errs.length === 0, errs.slice(0, 3).join(' | '));
  } catch (e) { fail++; console.log('FAIL crashed: ' + e.stack); }
  await b.close();
  console.log(`\n${pass} passed, ${fail} failed`);
  process.exit(fail ? 1 : 0);
})();

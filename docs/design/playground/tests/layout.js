/* Layout-stability helpers for the playground's flows (Chromium only).
   - watchShifts(page)  Layout Instability API: records every layout-shift entry with its sources (node, before -> after rect).
     shifts(page) reads and clears them; cls(list) sums the values.
   - stable(page, selectors, action)  anchors: bounding boxes before, per animation frame during, and after `action`; reports any move > tol px.
   - movers(page, action)  whole-DOM diff: every element's rect before/after; returns the elements that MOVED (top-left changed) while their
     parent did not move by the same amount, i.e. the ones that were pushed, not merely carried. Elements added/removed by the action are not listed.
   Both families are used together: the API catches what the browser thinks moved, the geometry catches small shifts and per-frame jumps. */

export async function watchShifts(page) {
  const install = () => {
    if (window.__ls) return;
    window.__ls = [];
    const desc = (n) => {
      if (!n || n.nodeType !== 1) return n ? '#text' : '?';
      let s = n.tagName.toLowerCase();
      if (n.id) s += '#' + n.id;
      const c = typeof n.className === 'string' ? n.className.trim().split(/\s+/).filter(Boolean).slice(0, 3).join('.') : '';
      if (c) s += '.' + c;
      for (const a of n.attributes) if (a.name.startsWith('data-')) s += `[${a.name}${a.value ? '=' + a.value : ''}]`;
      const t = (n.textContent || '').trim().replace(/\s+/g, ' ').slice(0, 24);
      return t ? `${s} "${t}"` : s;
    };
    const box = (r) => r && [Math.round(r.x * 10) / 10, Math.round(r.y * 10) / 10, Math.round(r.width * 10) / 10, Math.round(r.height * 10) / 10];
    new PerformanceObserver((list) => {
      for (const e of list.getEntries()) window.__ls.push({ value: e.value, hadRecentInput: e.hadRecentInput, at: Math.round(e.startTime),
        sources: (e.sources || []).map((s) => ({ node: desc(s.node), from: box(s.previousRect), to: box(s.currentRect) })) });
    }).observe({ type: 'layout-shift', buffered: true });
  };
  await page.context().addInitScript(install);
  await page.evaluate(install);
}
export const shifts = (page) => page.evaluate(() => { const o = window.__ls || []; window.__ls = []; return o; });
export const cls = (list, { input = true } = {}) => list.filter((e) => input || !e.hadRecentInput).reduce((a, e) => a + e.value, 0);
export const fmtShifts = (list) => list.map((e) => `${e.value.toFixed(4)}@${e.at}: ` + e.sources.slice(0, 3).map((s) => `${s.node} ${JSON.stringify(s.from)}->${JSON.stringify(s.to)}`).join(' | ')).join('\n');

const rectOf = (page, sel) => page.evaluate((sel) => Array.from(document.querySelectorAll(sel)).map((e) => { const r = e.getBoundingClientRect(); return [r.left, r.top, r.width, r.height]; }), sel);

/* Run `action`, then let things settle for `settle` ms, sampling the anchors every frame. Returns { moved: [...], frames }. */
export async function stable(page, selectors, action, { settle = 900, tol = 0.5, size = false, centered = [] } = {}) {
  await page.evaluate(() => { window.__st = { stop: false, rows: [] }; });
  await page.evaluate(([sels, centered]) => {
    const read = () => sels.map((s) => Array.from(document.querySelectorAll(s)).map((e) => { const r = e.getBoundingClientRect(); return [centered.some((c) => s.includes(c)) ? r.left + r.width / 2 : r.left, r.top, r.width, r.height]; }));
    window.__st.rows.push(read());
    const f = () => { window.__st.rows.push(read()); if (!window.__st.stop) requestAnimationFrame(f); };
    requestAnimationFrame(f);
  }, [selectors, centered]);
  await action();
  await page.waitForTimeout(settle);
  const rows = await page.evaluate(() => { window.__st.stop = true; return window.__st.rows; });
  const moved = [];
  selectors.forEach((sel, i) => {
    const first = rows[0][i]; let worst = 0, at = -1, what = '';
    rows.forEach((r, k) => { const cur = r[i]; if (!first || !cur || first.length !== cur.length) return;
      cur.forEach((b, j) => { const a = first[j]; const n = size ? 4 : 2;
        for (let q = 0; q < n; q++) { const d = Math.abs(b[q] - a[q]); if (d > worst) { worst = d; at = k; what = `#${j} ${['x', 'y', 'w', 'h'][q]} ${a[q].toFixed(1)}->${b[q].toFixed(1)}`; } } }); });
    if (worst > tol) moved.push({ sel, by: +worst.toFixed(2), frame: at, what });
  });
  return { moved, frames: rows.length };
}

/* Whole-DOM diff around an action. `root` limits the scan; `ignore` is a CSS selector for elements allowed to move. */
export async function movers(page, action, { root = '[data-form]', ignore = '', settle = 700, tol = 0.5 } = {}) {
  await page.evaluate(([root]) => {
    const m = new Map();
    for (const e of document.querySelector(root).querySelectorAll('*')) { if (e.closest('svg') && e.tagName.toLowerCase() !== 'svg') continue; const r = e.getBoundingClientRect(); m.set(e, [r.left, r.top, r.width, r.height]); }
    window.__mv = m;
  }, [root]);
  await action();
  await page.waitForTimeout(settle);
  return page.evaluate(([tol, ignore]) => {
    const desc = (n) => { let s = n.tagName.toLowerCase(); const c = typeof n.className === 'string' ? n.className.trim().split(/\s+/).filter(Boolean).slice(0, 3).join('.') : ''; if (c) s += '.' + c; for (const a of n.attributes) if (a.name.startsWith('data-')) s += `[${a.name}${a.value ? '=' + a.value : ''}]`; const t = (n.textContent || '').trim().replace(/\s+/g, ' ').slice(0, 22); return t ? `${s} "${t}"` : s; };
    const out = [];
    for (const [e, a] of window.__mv) {
      if (!e.isConnected) continue;
      if (ignore && e.closest(ignore)) continue;
      const r = e.getBoundingClientRect(), dx = r.left - a[0], dy = r.top - a[1];
      if (Math.abs(dx) <= tol && Math.abs(dy) <= tol) continue;
      const p = e.parentElement, pa = window.__mv.get(p);
      if (p && pa) { const pr = p.getBoundingClientRect(); if (Math.abs((pr.left - pa[0]) - dx) <= tol && Math.abs((pr.top - pa[1]) - dy) <= tol) continue; }
      out.push({ el: desc(e), dx: +dx.toFixed(1), dy: +dy.toFixed(1) });
    }
    window.__mv = null;
    return out;
  }, [tol, ignore]);
}
export const fmtMovers = (l) => l.slice(0, 6).map((m) => `${m.el} dx${m.dx} dy${m.dy}`).join(' | ') + (l.length > 6 ? ` (+${l.length - 6})` : '');

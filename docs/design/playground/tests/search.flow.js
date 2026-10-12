/* Search flow checks against the running dev server (http://127.0.0.1:5178). Prints PASS/FAIL per check.
   Run: node tests/search.flow.js   (BASE=… to override; SHOTS=/dir/ to also write screenshots; ONLY=rules,android,desktop,layout,motion)
   Covers: matching rules (AND, prefix, case, diacritics, Indic marks, CJK, min length, names), snippet + highlight ranges, newest first + paging,
   section order, jump-to-message + highlight, find in chat, Ctrl+K / Ctrl+F, both positions, Android + desktop, light + dark, reduced motion,
   live updates, "getting ready", and the hard rule: nothing moves while results change (tests/layout.js). */
import { createRequire } from 'node:module';
import * as LY from './layout.js';
const L = createRequire(import.meta.url)('/home/abe/p2p_chat/.claude/worktrees/design-refinement/docs/design/refined/qa/lib.js');
const BASE = process.env.BASE || 'http://127.0.0.1:5178/';
const SHOTS = process.env.SHOTS || '';
const ONLY = (process.env.ONLY || '').split(',').filter(Boolean);
const want = (s) => !ONLY.length || ONLY.includes(s);
let pass = 0, fail = 0;
const ok = (name, cond, extra = '') => { cond ? pass++ : fail++; console.log((cond ? 'PASS ' : 'FAIL ') + name + (cond ? '' : ' ' + extra)); };
const wait = (p, ms) => p.waitForTimeout(ms);
const EPS = 0.002;
/* CLS of element shifts only: a snippet re-wrapping its own text (#text nodes inside a fixed-size row) is the text changing, not UI moving. */
const clsEl = (list) => list.filter((e) => e.sources.some((x) => x.node !== '#text')).reduce((a, e) => a + e.value, 0);
const allErrs = [];

async function open(b, form, hash, { dark = false, reduced = false } = {}) {
  const p = await L.page(b, { w: form === 'android' ? 600 : 1440, h: 900, dark });
  if (reduced) await p.emulateMedia({ reducedMotion: 'reduce' });
  await LY.watchShifts(p);
  await p.goto(`${BASE}?form=${form}&theme=${dark ? 'dark' : 'light'}#/${hash}`);
  await p.waitForSelector('[data-form]');
  await wait(p, 600);
  return p;
}
const done = async (p) => { allErrs.push(...p.errs); await p.context().close(); };
const shot = (p, name) => (SHOTS ? p.screenshot({ path: SHOTS + name + '.png' }) : null);
const sr = (p, fn, arg) => p.evaluate(fn, arg);
const S = (p, q, pages = 1) => p.evaluate(([q, pages]) => window.__pgSearch.searchPages(q, pages), [q, pages]);
const drawerOpen = async (p) => { await p.getByRole('button', { name: 'Simulation', exact: true }).click(); await wait(p, 350); };
const drawerClose = async (p) => { await p.getByRole('button', { name: 'Close simulation' }).click(); await wait(p, 300); };
async function simSelect(p, label, value) { await drawerOpen(p); await p.getByLabel(label, { exact: true }).selectOption(value); await drawerClose(p); }
async function simSwitch(p, label, on) {
  await drawerOpen(p);
  const sw = p.getByRole('switch', { name: label, exact: true });
  if ((await sw.getAttribute('aria-checked')) !== String(on)) await sw.click();
  await drawerClose(p);
}
async function simBtn(p, text) { await drawerOpen(p); await p.getByRole('button', { name: text, exact: true }).click(); await drawerClose(p); }
const field = (form) => (form === 'android' ? 'input[aria-label="Search"]' : '[data-search-field] input');
async function startSearch(p, form) {
  if (form === 'android') { await p.click('[data-search-dock] .pill'); await p.waitForSelector('[data-search-screen]'); await wait(p, 350); }
  else { await p.keyboard.press('Control+k'); await wait(p, 200); }
}
async function typeQ(p, form, q, delay = 0) { await p.click(field(form)); await p.fill(field(form), ''); await (delay ? p.keyboard.type(q, { delay }) : p.keyboard.insertText(q)); await wait(p, 420); }
const rowTexts = (p) => p.$$eval('[data-search-results] .row', (rs) => rs.map((r) => r.innerText.replace(/\s+/g, ' ')));
const bolds = (p) => p.$$eval('[data-search-results] .row .snip b', (bs) => bs.map((b) => b.textContent));

/* ================================================================== rules */
async function rules(b) {
  const p = await open(b, 'desktop', 'chats');
  const msgs = async (q) => (await S(p, q)).messages;
  const texts = async (q) => (await msgs(q)).map((m) => m.snippet);

  ok('rules: AND of terms (both words in one message)', (await msgs('landlord deposit')).length === 1 && (await msgs('landlord numbers')).length === 1);
  ok('rules: AND: words in different messages do not match', (await msgs('flat deposit')).length === 0);
  ok('rules: prefix match', (await msgs('land')).length === 2);
  ok('rules: prefix only, not substring', (await msgs('andlord')).length === 0);
  ok('rules: case folded', JSON.stringify(await texts('LUNCH')) === JSON.stringify(await texts('lunch')) && (await msgs('LuNcH')).length === 5);
  ok('rules: Latin diacritics fold both ways (cafe finds café, café finds cafe)', (await msgs('cafe')).length === 2 && (await msgs('café')).length === 2);
  ok('rules: Indic marks are kept (ഊണ് found; a different vowel sign is not)', (await msgs('ഊണ്')).length === 1 && (await msgs('ഊണ')).length === 1 && (await msgs('ഊണി')).length === 0);
  ok('rules: CJK runs match as substrings', (await msgs('会議')).length === 1 && (await msgs('議は')).length === 1 && (await msgs('議議')).length === 0);
  ok('rules: messages need 2 characters', (await S(p, 'l')).messages.length === 0 && (await S(p, 'lu')).messages.length > 0);
  ok('rules: a 1-char term only finds 1-char words (indexed as itself): "lunch 1" finds "Lunch is at 1", "lunch s" finds nothing', (await msgs('lunch 1')).length === 1 && (await msgs('lunch s')).length === 0);
  ok('rules: system lines are not found', (await msgs('joined')).length === 0);
  ok('rules: call records are not found', (await msgs('missed')).length === 0 && (await msgs('voice call')).length === 0);
  ok('rules: removed/left groups are searchable, marked', (await msgs('chapter')).length === 1 && (await msgs('chapter'))[0].status === 'removed');
  const r = await S(p, 'lunch');
  ok('rules: hit fields (kind, conv, conv_name, day, id, at, author, author_name, snippet, highlights)',
    r.messages.every((m) => ['dm', 'group'].includes(m.kind) && m.conv && m.conv_name && /^\d{4}-\d\d-\d\d$/.test(m.day) && m.id && m.at && m.author && m.author_name && m.snippet && Array.isArray(m.highlights)));
  ok('rules: newest first', r.messages.every((m, i, a) => i === 0 || a[i - 1].at >= m.at));
  ok('rules: highlights cover the matched prefix', r.messages.every((m) => m.highlights.length && m.highlights.every((h) => /^lunch/i.test(Array.from(m.snippet).slice(h.start, h.end).join('')))));
  ok('rules: file name is indexed', await (async () => { await p.evaluate(() => { const c = window.__pg.db.chats.find((x) => x.id === 'arjun'); c.messages.push({ id: 'tf1', from: 'arjun', at: c.messages.at(-1).at + 1000, text: '', file: { name: 'holiday-coast.jpg', size: 1, kind: 'image', progress: 1, state: 'done' } }); }); return (await msgs('holiday')).length === 1; })());
  ok('rules: deleted messages are removed from the index', await (async () => { await p.evaluate(() => { const c = window.__pg.db.chats.find((x) => x.id === 'arjun'); const m = c.messages.find((x) => x.id === 'tf1'); m.deleted = true; m.text = ''; m.file = undefined; }); return (await msgs('holiday')).length === 0; })());

  // names
  const names = async (q) => (await S(p, q)).names.map((n) => `${n.kind}:${n.name}`);
  ok('names: from 1 character, substring, case-folded', (await names('A')).length >= 5 && (await names('ARJ')).join() === 'contact:Arjun Oommen' && (await names('oomm')).join() === 'contact:Arjun Oommen');
  ok('names: groups and "You" (also my profile name)', (await names('hike')).join() === 'group:Weekend hike' && (await names('yo')).includes('me:You') && (await names('kurian')).includes('me:You'));
  ok('names: a blocked person without a chat and people known only through a group are not listed', (await names('ravi')).length === 0 && (await names('kavya')).length === 0);
  ok('names: left/removed groups are listed with their state', (await S(p, 'book')).names[0]?.status === 'removed');
  ok('names: a contact without a conversation is listed (Amma has a group post only)', (await names('amma')).join() === 'contact:Amma');
  ok('names: only with the first page (a cursor page has none)', await p.evaluate(() => { const a = window.__pgSearch.search('a', 5, null); return a.names.length > 0 && window.__pgSearch.search('lu', 1, null).next !== null && window.__pgSearch.search('lu', 1, window.__pgSearch.search('lu', 1, null).next).names.length === 0; }));

  // snippet
  await p.evaluate(() => { const c = window.__pg.db.chats.find((x) => x.id === 'arjun'); const w = 'alpha bravo charlie delta echo foxtrot golf hotel india juliet kilo lima mike november oscar papa quebec romeo sierra tango uniform victor whiskey xray yankee zulu'; c.messages.push({ id: 'tl1', from: 'arjun', at: c.messages.at(-1).at + 2000, text: w + ' needle at the far end of a long message with plenty of words after it to cut ' + w }); c.messages.push({ id: 'te1', from: 'arjun', at: c.messages.at(-1).at + 3000, text: '😀😀 hello zebra and 😀 more' }); });
  const long = (await S(p, 'needle')).messages[0];
  const chars = Array.from(long.snippet);
  ok('snippet: about 80 chars around the first match, cut with an ellipsis', chars.length <= 84 && chars.length >= 40 && long.snippet.startsWith('…') && long.snippet.endsWith('…'), long.snippet);
  ok('snippet: highlight is the match', chars.slice(long.highlights[0].start, long.highlights[0].end).join('') === 'needle', JSON.stringify(long.highlights) + long.snippet);
  const em = (await S(p, 'zebra')).messages[0];
  ok('snippet: highlight offsets are characters, not UTF-16 units (emoji before the match)', Array.from(em.snippet).slice(em.highlights[0].start, em.highlights[0].end).join('') === 'zebra' && em.highlights[0].start === 9, JSON.stringify(em));

  // paging
  await simBtn(p, 'Import 60 older messages (Arjun)');
  const p1 = await p.evaluate(() => window.__pgSearch.search('lunch', 20, null));
  ok('paging: 20 a page, cursor present', p1.messages.length === 20 && !!p1.next);
  const p2 = await p.evaluate((c) => window.__pgSearch.search('lunch', 20, c), p1.next);
  ok('paging: next page continues strictly older, names only on page one', p2.messages.length === 20 && p2.names.length === 0 && p2.messages[0].at <= p1.messages[19].at && p2.messages.every((m) => !p1.messages.some((x) => x.id === m.id)));
  const all = await p.evaluate(() => window.__pgSearch.searchPages('lunch', 10));
  ok('paging: all pages = every hit, sorted, no duplicates', all.messages.length > 40 && new Set(all.messages.map((m) => m.id)).size === all.messages.length && all.messages.every((m, i, a) => i === 0 || a[i - 1].at >= m.at) && !all.more);
  ok('paging: limit clamps to 1..100', await p.evaluate(() => window.__pgSearch.search('lunch', 0, null).messages.length === 1 && window.__pgSearch.search('lunch', 5000, null).messages.length <= 100));
  await done(p);
}

/* ================================================================== Android */
async function android(b, pos, dark) {
  const t = (s) => `android ${pos}${dark ? ' dark' : ''}: ${s}`;
  const p = await open(b, 'android', 'chats', { dark });
  if (pos === 'top') await simSelect(p, 'Search position', 'top');
  await shot(p, `android-${pos}-${dark ? 'dark' : 'light'}-chats`);
  const dockEdge = await p.getAttribute('[data-search-dock]', 'data-search-dock');
  ok(t('pill is at the ' + pos), dockEdge === pos);
  if (pos === 'bottom') {
    const d = await p.locator('[data-search-dock]').boundingBox(), nav = await p.locator('nav.bar').boundingBox();
    ok(t('pill row sits directly above the tab bar'), Math.abs(d.y + d.height - nav.y) < 1.5, JSON.stringify([d, nav]));
    const pill = await p.locator('[data-search-dock] .pill').boundingBox(), add = await p.locator('[data-search-dock] .add').boundingBox();
    ok(t('pill 48 high, new-chat button 48 round, same row, centred'), pill.height === 48 && add.width === 48 && add.height === 48 && Math.abs(pill.y - add.y) < 0.5);
  } else {
    const ti = await p.locator('h1.title').boundingBox(), d = await p.locator('[data-search-dock]').boundingBox();
    ok(t('field row is right under the title'), d.y >= ti.y + ti.height - 1 && d.y - (ti.y + ti.height) < 16);
  }
  ok(t('app bar search icon hidden on Chats (the pill is the entry)'), (await p.getByRole('button', { name: 'Search', exact: true }).count()) === 1);
  await p.locator('nav.bar .tab', { hasText: 'Calls' }).click(); await wait(p, 500);
  ok(t('app bar search icon shown on Calls (no pill there)'), (await p.locator('header.appbar').getByRole('button', { name: 'Search' }).count()) === 1 && (await p.locator('[data-search-dock]').count()) === 0);
  await p.locator('nav.bar .tab', { hasText: 'Groups' }).click(); await wait(p, 500);
  ok(t('pill on Groups too'), (await p.locator('[data-search-dock]').count()) === 1);
  await shot(p, `android-${pos}-${dark ? 'dark' : 'light'}-groups`);
  await p.locator('[data-search-dock] .add').click(); await wait(p, 300);
  ok(t('Groups: the + button opens New group'), (await p.getByRole('dialog', { name: 'New group' }).count()) + (await p.locator('input[placeholder="Name"]').count()) > 0);
  await p.keyboard.press('Escape'); await wait(p, 300);
  await p.locator('nav.bar .tab', { hasText: 'Chats' }).click(); await wait(p, 500);

  await startSearch(p, 'android');
  ok(t('search screen opens with the field focused'), await p.evaluate(() => document.activeElement?.getAttribute('aria-label') === 'Search'));
  const fb = await p.locator('.sf').boundingBox(), ph = await p.locator('[data-form]').boundingBox();
  ok(t('field at the ' + pos + ' of the screen'), pos === 'bottom' ? fb.y > ph.y + ph.height - 120 : fb.y < ph.y + 90, JSON.stringify(fb));
  await shot(p, `android-${pos}-${dark ? 'dark' : 'light'}-empty`);
  await typeQ(p, 'android', 'l');
  ok(t('1 character: names only, hint for messages'), (await rowTexts(p)).length === 0 && /2 or more/.test(await p.locator('.hint').innerText()) && (await p.locator('.tile').count()) > 0);
  await shot(p, `android-${pos}-${dark ? 'dark' : 'light'}-one-char`);
  await typeQ(p, 'android', 'lunch');
  const rows = await rowTexts(p);
  ok(t('results: rows with name, date, snippet'), rows.length === 5 && /Family/.test(rows[0]) && /Yesterday/.test(rows[0]), JSON.stringify(rows));
  ok(t('matches are bold'), (await bolds(p)).length >= 5 && (await bolds(p)).every((x) => /^lunch$/i.test(x)));
  const w = await p.$$eval('.snip b', (bs) => getComputedStyle(bs[0]).fontWeight);
  ok(t('bold weight 700'), w === '700', w);
  const order = await p.$$eval('[data-search-results] h3', (hs) => hs.map((h) => h.textContent));
  ok(t('sections: People & groups, then Messages'), order.join('|') === 'People & groups|Messages', order.join('|'));
  await shot(p, `android-${pos}-${dark ? 'dark' : 'light'}-results`);
  await typeQ(p, 'android', 'arj');
  ok(t('people tile for a name match; messages for the same query'), (await p.locator('.tile').count()) === 1 && /Arjun/.test(await p.locator('.tile').first().innerText()));
  await shot(p, `android-${pos}-${dark ? 'dark' : 'light'}-people`);
  await typeQ(p, 'android', 'zzzz');
  ok(t('no results: calm text, no rows'), (await rowTexts(p)).length === 0 && /No messages match “zzzz”/.test(await p.locator('.hint').innerText()) && (await p.locator('.none.on').count()) === 1);
  await shot(p, `android-${pos}-${dark ? 'dark' : 'light'}-none`);
  await p.click('button[aria-label="Clear search"]'); await wait(p, 300);
  ok(t('clear button empties the field and keeps focus'), (await p.inputValue(field('android'))) === '' && (await p.evaluate(() => document.activeElement?.getAttribute('aria-label') === 'Search')));
  ok(t('recent searches shown after clearing'), /recent/i.test(await p.locator("[data-search-results]").innerText()));
  await done(p);
}

async function androidJump(b) {
  const t = (s) => `android jump: ${s}`;
  const p = await open(b, 'android', 'chats');
  await startSearch(p, 'android');
  await typeQ(p, 'android', 'landlord');
  const hit = p.locator('[data-search-results] .row', { hasText: 'deposit by Friday' });
  await hit.click(); await wait(p, 700);
  ok(t('tapping a message opens its conversation'), (await p.locator('.conv').count()) === 1 && (await p.getAttribute('.conv', 'data-conv')) === 'arjun');
  const f = p.locator('.bub.found');
  ok(t('exactly that message is highlighted'), (await f.count()) === 1 && /deposit by Friday/.test(await f.innerText()));
  const inView = await p.evaluate(() => { const b = document.querySelector('.bub.found').getBoundingClientRect(), l = document.querySelector('.msgs').getBoundingClientRect(); return b.top >= l.top && b.bottom <= l.bottom; });
  ok(t('it is scrolled into view (not at the end of the chat)'), inView && await p.evaluate(() => { const l = document.querySelector('.msgs'); return l.scrollHeight - l.scrollTop - l.clientHeight > 40; }));
  const o = await p.evaluate(() => getComputedStyle(document.querySelector('.bub.found'), '::after').opacity);
  ok(t('highlight is visible'), Number(o) > 0.9, o);
  await shot(p, 'android-jump-highlight');
  await wait(p, 2800);
  ok(t('highlight goes away by itself'), (await p.locator('.bub.found').count()) === 0);
  await p.getByRole('button', { name: 'Back to chats' }).click(); await wait(p, 600);
  ok(t('back returns to the results with the query kept'), (await p.inputValue(field('android'))) === 'landlord' && (await rowTexts(p)).length === 2);
  // person tile opens the chat
  await typeQ(p, 'android', 'meera');
  await p.locator('.tile').first().click(); await wait(p, 600);
  ok(t('tapping a person opens the chat (no highlight)'), (await p.getAttribute('.conv', 'data-conv')) === 'meera' && (await p.locator('.bub.found').count()) === 0);
  await p.getByRole('button', { name: 'Back to chats' }).click(); await wait(p, 500);
  await typeQ(p, 'android', 'book');
  await p.locator('.tile').first().click(); await wait(p, 600);
  ok(t('a removed group opens read-only'), (await p.getAttribute('.conv', 'data-conv')) === 'book' && (await p.locator('.readonly').count()) === 1);
  await p.getByRole('button', { name: 'Back to chats' }).click(); await wait(p, 500);
  await p.keyboard.press('Escape'); await wait(p, 600);
  ok(t('Esc closes search'), (await p.locator('[data-search-screen]').count()) === 0 && (await p.locator('[data-search-dock]').count()) === 1);
  // deep link
  const q = await open(b, 'android', 'search?q=station');
  ok(t('deep link #/search?q=station fills the field and searches'), (await q.inputValue(field('android'))) === 'station' && (await rowTexts(q)).length >= 3, JSON.stringify(await rowTexts(q)));
  await done(q);
  await done(p);
}

/* ================================================================== desktop */
async function desktop(b, pos, dark) {
  const t = (s) => `desktop ${pos}${dark ? ' dark' : ''}: ${s}`;
  const p = await open(b, 'desktop', 'chats', { dark });
  if (pos === 'bottom') await simSelect(p, 'Search position', 'bottom');
  await shot(p, `desktop-${pos}-${dark ? 'dark' : 'light'}-chats`);
  const fb = await p.locator('[data-search-field]').boundingBox(), lp = await p.locator('.listpane').boundingBox();
  ok(t('field at the ' + pos + ' of the list pane'), pos === 'top' ? fb.y < lp.y + 20 : fb.y > lp.y + lp.height - 80, JSON.stringify([fb, lp]));
  await p.locator('.nav', { hasText: 'Calls' }).click(); await wait(p, 400);
  await p.keyboard.press('Control+k'); await wait(p, 250);
  ok(t('Ctrl+K focuses the field from any tab'), await p.evaluate(() => document.activeElement?.getAttribute('aria-label') === 'Search'));
  await p.keyboard.type('lun'); await wait(p, 450);
  ok(t('typing shows results in the pane (list replaced)'), (await rowTexts(p)).length === 5 && (await p.locator('.chips').count()) === 0);
  await shot(p, `desktop-${pos}-${dark ? 'dark' : 'light'}-results`);
  await p.keyboard.press('ArrowDown'); await p.keyboard.press('ArrowDown'); await wait(p, 100);
  ok(t('arrow down selects (aria-activedescendant)'), await p.evaluate(() => { const i = document.querySelector('[data-search-field] input'); const id = i.getAttribute('aria-activedescendant'); return !!id && document.getElementById(id)?.getAttribute('aria-selected') === 'true'; }));
  await shot(p, `desktop-${pos}-${dark ? 'dark' : 'light'}-selected`);
  await p.keyboard.press('Enter'); await wait(p, 800);
  ok(t('Enter opens the selected hit in the detail pane, highlighted'), (await p.locator('.detail .conv').count()) === 1 && (await p.locator('.bub.found').count()) === 1);
  ok(t('results stay visible beside the conversation'), (await rowTexts(p)).length === 5);
  await shot(p, `desktop-${pos}-${dark ? 'dark' : 'light'}-opened`);
  await p.click(field('desktop')); await p.keyboard.press('Escape'); await wait(p, 300);
  ok(t('Esc clears the text'), (await p.inputValue(field('desktop'))) === '');
  await p.keyboard.press('Escape'); await wait(p, 400);
  ok(t('second Esc leaves the field; the tab list is back'), (await p.locator('.listpane h1').count()) === 1 && (await p.locator('[data-search-results]').count()) === 0);
  // names with arrows
  await p.keyboard.press('Control+k'); await p.keyboard.type('a'); await wait(p, 400);
  await p.keyboard.press('ArrowDown'); await p.keyboard.press('ArrowRight'); await wait(p, 80);
  ok(t('Right moves along the people tiles'), await p.evaluate(() => document.getElementById(document.querySelector('[data-search-field] input').getAttribute('aria-activedescendant'))?.id === 'sr-opt-1'));
  await shot(p, `desktop-${pos}-${dark ? 'dark' : 'light'}-tiles`);
  await p.keyboard.press('Enter'); await wait(p, 600);
  ok(t('Enter on a tile opens that chat'), (await p.locator('.detail .conv').count()) === 1 && (await p.locator('.bub.found').count()) === 0);
  await done(p);
}

async function find(b, dark) {
  const t = (s) => `desktop find${dark ? ' dark' : ''}: ${s}`;
  const p = await open(b, 'desktop', 'chat/arjun', { dark });
  await p.keyboard.press('Control+f'); await wait(p, 300);
  ok(t('Ctrl+F opens the find bar over the header, input focused'), (await p.locator('[role=search] input').count()) === 1 && await p.evaluate(() => document.activeElement?.getAttribute('aria-label') === 'Find in this chat'));
  const hb = await p.locator('.conv .bar').boundingBox(), fbb = await p.locator('[role=search]').boundingBox();
  ok(t('bar covers exactly the header box'), Math.abs(hb.height - fbb.height) <= 1.5 && Math.abs(hb.y - fbb.y) < 1);
  await p.keyboard.type('the'); await wait(p, 300);
  const cnt = async () => (await p.locator('.fb .cnt').innerText()).trim();
  const total = Number((await cnt()).split('/')[1]);
  ok(t('count n/m, starting at the newest'), (await cnt()).startsWith('1/') && total >= 3, await cnt());
  ok(t('matches are highlighted in bubbles'), (await p.locator('.bub mark').count()) >= total && (await p.locator('.bub mark.cur').count()) >= 1);
  await shot(p, `desktop-find-${dark ? 'dark' : 'light'}`);
  const curText = () => p.$eval('.bub mark.cur', (m) => m.closest('.bub').innerText);
  const a = await curText();
  await p.keyboard.press('Enter'); await wait(p, 500);
  ok(t('Enter goes to the next older match (2/m)'), (await cnt()).startsWith('2/') && (await curText()) !== a);
  await p.keyboard.press('Shift+Enter'); await wait(p, 300);
  ok(t('Shift+Enter goes back (1/m)'), (await cnt()).startsWith('1/') && (await curText()) === a);
  await p.getByRole('button', { name: 'Older match' }).click(); await p.getByRole('button', { name: 'Older match' }).click(); await wait(p, 300);
  ok(t('older button steps'), (await cnt()).startsWith('3/'));
  await p.getByRole('button', { name: 'Newer match' }).click(); await wait(p, 300);
  ok(t('newer button steps'), (await cnt()).startsWith('2/'));
  const vis = await p.evaluate(() => { const b = document.querySelector('.bub mark.cur').getBoundingClientRect(), l = document.querySelector('.msgs').getBoundingClientRect(); return b.top >= l.top && b.bottom <= l.bottom; });
  ok(t('the current match is scrolled into view'), vis);
  await p.fill('[role=search] input', 'zzzzqq'); await wait(p, 300);
  ok(t('no matches: 0/0, steppers disabled, marks gone'), (await cnt()) === '0/0' && (await p.locator('.bub mark').count()) === 0 && await p.getByRole('button', { name: 'Older match' }).isDisabled());
  await p.fill('[role=search] input', 'LANDLORD'); await wait(p, 300);
  ok(t('same matching rules as search (case)'), (await cnt()) === '1/2');
  await p.keyboard.press('Escape'); await wait(p, 400);
  ok(t('Esc closes the bar and removes the highlights'), (await p.locator('[role=search]').count()) === 0 && (await p.locator('.bub mark').count()) === 0);
  ok(t('the header is back'), (await p.locator('.conv .bar .head').isVisible()));
  // live: new matching message while open
  await p.keyboard.press('Control+f'); await p.keyboard.type('xylophone'); await wait(p, 250);
  const c0 = await cnt();
  await p.evaluate(() => { const c = window.__pg.db.chats.find((x) => x.id === 'arjun'); c.messages.push({ id: 'tx1', from: 'arjun', at: c.messages.at(-1).at + 1000, text: 'Playing the xylophone tonight' }); });
  await wait(p, 300);
  ok(t('a message arriving while finding is counted live'), c0 === '0/0' && (await cnt()) === '1/1');
  await done(p);
}

/* ================================================================== live, getting ready */
async function live(b) {
  const t = (s) => `live: ${s}`;
  const p = await open(b, 'desktop', 'chats');
  await startSearch(p, 'desktop');
  await typeQ(p, 'desktop', 'lunch');
  const n0 = (await rowTexts(p)).length;
  await drawerOpen(p);
  await p.getByLabel('Message text', { exact: true }).fill('Lunch on Thursday at the café');
  await p.getByRole('button', { name: 'Receive a matching message', exact: true }).click();
  await drawerClose(p);
  await wait(p, 300);
  const rows = await rowTexts(p);
  ok(t('a new matching message appears at the top, without retyping'), rows.length === n0 + 1 && /Arjun/.test(rows[0]) && /Lunch on Thursday at the café/.test(rows[0]), rows[0]);
  await p.evaluate(() => { const c = window.__pg.db.chats.find((x) => x.id === 'arjun'); const m = c.messages.at(-1); m.text = 'Dinner on Thursday'; m.edited = true; });
  await wait(p, 300);
  ok(t('an edit that no longer matches removes the hit'), (await rowTexts(p)).length === n0);
  await p.evaluate(() => { const c = window.__pg.db.chats.find((x) => x.id === 'arjun'); const m = c.messages.at(-1); m.text = 'lunch again'; });
  await wait(p, 300);
  ok(t('an edit that matches adds it back'), (await rowTexts(p)).length === n0 + 1);
  await p.evaluate(() => { const c = window.__pg.db.chats.find((x) => x.id === 'arjun'); const m = c.messages.at(-1); m.deleted = true; m.text = ''; });
  await wait(p, 300);
  ok(t('a deleted message disappears from results'), (await rowTexts(p)).length === n0);
  await simSwitch(p, 'Search index getting ready', true);
  await wait(p, 500);
  const n1 = (await rowTexts(p)).length;
  ok(t('getting ready: calm notice, only recent messages found'), (await p.locator('.getting.on').count()) === 1 && n1 < n0 && /getting ready/.test(await p.locator('.getting').innerText()), `${n0} -> ${n1}`);
  await shot(p, 'desktop-getting-ready');
  await simSwitch(p, 'Search index getting ready', false);
  await wait(p, 500);
  ok(t('ready again: everything is found'), (await rowTexts(p)).length === n0 && (await p.locator('.getting.on').count()) === 0);
  await done(p);
}

/* ================================================================== layout: nothing moves */
const WATCH = (form) => (form === 'android' ? ['.sf', '.sf input', '[data-search-results] h3', '.strip', 'nav.bar, footer.dock, header.top'] : ['.sf', '.sf input', '[data-search-results] h3', '.strip', '.side', '.detail']);
async function stableSeq(p, name, form, action, o = {}) {
  await LY.shifts(p);
  const r = await LY.stable(p, WATCH(form), action, { settle: 500, ...o });
  const c = clsEl(await LY.shifts(p));
  ok(name, r.moved.length === 0 && c <= EPS, `moved: ${JSON.stringify(r.moved)} | cls ${c.toFixed(4)}`);
}
async function noPush(p, name, action, { ignore = '.list, .list *, .strip *, .hint, .none, .getting, .recents, .recents *, .blank', settle = 600 } = {}) {
  await LY.shifts(p);
  const mv = await LY.movers(p, action, { ignore, settle });
  const c = clsEl(await LY.shifts(p));
  ok(name, mv.length === 0 && c <= EPS, `movers: ${LY.fmtMovers(mv)} | cls ${c.toFixed(4)}`);
}
async function layout(b, form, pos, dark) {
  const t = (s) => `layout ${form} ${pos}${dark ? ' dark' : ''}: ${s}`;
  const p = await open(b, form, 'chats', { dark });
  if (pos !== (form === 'android' ? 'bottom' : 'top')) await simSelect(p, 'Search position', pos);
  await startSearch(p, form);
  await typeQ(p, form, 'l');   // into the results view first
  const f = field(form);
  await stableSeq(p, t('typing a query char by char: field, headers, strip and chrome never move'), form, async () => { await p.fill(f, ''); await p.keyboard.type('lunch plans', { delay: 70 }); }, { settle: 600 });
  await noPush(p, t('typing: nothing outside the result rows moves (whole-DOM diff)'), async () => { await p.fill(f, ''); await p.keyboard.type('arjun', { delay: 60 }); });
  await stableSeq(p, t('a query with no results ("No messages match"): nothing moves'), form, async () => { await p.fill(f, 'zzzzqq'); await wait(p, 400); });
  ok(t('...and the notice is up'), /No messages match/.test(await p.locator('.hint').innerText()));
  await noPush(p, t('no-results text appears/disappears without pushing anything'), async () => { await p.fill(f, 'lunch'); await wait(p, 300); await p.fill(f, 'qqqqzz'); });
  await stableSeq(p, t('1-character query (hint) then 2 characters: nothing moves'), form, async () => { await p.fill(f, 'a'); await wait(p, 350); await p.fill(f, 'ar'); await wait(p, 350); });
  await stableSeq(p, t('result snippets changing (query changes): nothing moves'), form, async () => { for (const q of ['station', 'lunch', 'call', 'flat', 'lunch']) { await p.fill(f, q); await wait(p, 330); } });
  await p.fill(f, 'lunch'); await wait(p, 400);
  await noPush(p, t('a live new match at the top does not move chrome'), async () => { await p.evaluate(() => { const c = window.__pg.db.chats.find((x) => x.id === 'arjun'); c.messages.push({ id: 'tl' + Date.now(), from: 'arjun', at: c.messages.at(-1).at + 1000, text: 'lunch live' }); }); });
  await noPush(p, t('"Search is getting ready" fades in and out without pushing anything'), async () => { await p.evaluate(() => { window.__pgSearch.searchSim.rebuilding = true; }); await wait(p, 450); await p.evaluate(() => { window.__pgSearch.searchSim.rebuilding = false; }); });
  await stableSeq(p, t('clearing the field: nothing moves'), form, async () => { await p.click('button[aria-label="Clear search"]'); await wait(p, 400); });
  if (form === 'android') {
    await typeQ(p, form, 'lunch');
    await stableSeq(p, t('mock keyboard toggled: field keeps its place relative to the keyboard, headers stay'), form, async () => { await p.evaluate(() => { document.querySelector('[data-search-screen]'); }); }, { settle: 100 });
  }
  await done(p);
}

async function layoutPaging(b, form) {
  const t = (s) => `layout ${form}: ${s}`;
  const p = await open(b, form, 'chats');
  await simBtn(p, 'Import 60 older messages (Arjun)');
  await startSearch(p, form);
  await typeQ(p, form, 'lunch');
  const n0 = (await rowTexts(p)).length;
  ok(t('first page is 20 rows'), n0 === 20, String(n0));
  await shot(p, `${form}-paging-p1`);
  await stableSeq(p, t('scrolling to the end loads the next page: chrome and existing rows stay'), form, async () => { await p.evaluate(() => { const l = document.querySelector('[data-search-results] .list'); l.scrollTop = l.scrollHeight; }); await wait(p, 500); });
  const n1 = (await rowTexts(p)).length;
  ok(t('second page appended (40 rows)'), n1 === 40, String(n1));
  const same = await p.evaluate(() => { const l = document.querySelector('[data-search-results] .list'); const r = [...l.querySelectorAll('.row')].map((x) => x.getBoundingClientRect().height); return r.every((h) => h === r[0]); });
  ok(t('every row has the same fixed height'), same);
  await p.evaluate(() => { const l = document.querySelector('[data-search-results] .list'); l.scrollTop = l.scrollHeight; }); await wait(p, 500);
  ok(t('all pages end at the real total'), (await rowTexts(p)).length === (await S(p, 'lunch', 10)).messages.length);
  await shot(p, `${form}-paging-end`);
  await done(p);
}

/* ================================================================== reduced motion */
async function motion(b) {
  const t = (s) => `reduced motion: ${s}`;
  const p = await open(b, 'android', 'chats', { reduced: true });
  await startSearch(p, 'android');
  await typeQ(p, 'android', 'landlord');
  await p.locator('[data-search-results] .row', { hasText: 'deposit by Friday' }).click(); await wait(p, 500);
  ok(t('jump still highlights (static wash, no animation needed)'), (await p.locator('.bub.found').count()) === 1 && Number(await p.evaluate(() => getComputedStyle(document.querySelector('.bub.found'), '::after').opacity)) > 0.9);
  const anim = await p.evaluate(() => document.querySelector('.bub.found').getAnimations({ subtree: true }).filter((a) => a.playState === 'running').length);
  ok(t('and nothing is animating'), anim === 0, String(anim));
  await wait(p, 2700);
  ok(t('highlight still goes away'), (await p.locator('.bub.found').count()) === 0);
  await done(p);
}

/* ================================================================== dark detail */
async function darkShots(b) {
  const p = await open(b, 'android', 'chats', { dark: true });
  await startSearch(p, 'android');
  await typeQ(p, 'android', 'lunch');
  const c = await p.evaluate(() => { const s = getComputedStyle(document.querySelector('.snip b')), r = getComputedStyle(document.querySelector('.row')); return [s.color, s.fontWeight]; });
  ok('dark: bold match is lighter than the muted rest (ink vs muted)', c[1] === '700' && c[0] !== 'rgb(166, 178, 173)', c.join());
  await done(p);
}

(async () => {
  const b = await L.launch();
  try {
    if (want('rules')) await rules(b);
    if (want('android')) {
      for (const pos of ['bottom', 'top']) for (const dark of [false, true]) await android(b, pos, dark);
      await androidJump(b);
    }
    if (want('desktop')) {
      for (const pos of ['top', 'bottom']) for (const dark of [false, true]) await desktop(b, pos, dark);
      await find(b, false); await find(b, true);
      await live(b);
    }
    if (want('layout')) {
      for (const [form, pos] of [['android', 'bottom'], ['android', 'top'], ['desktop', 'top'], ['desktop', 'bottom']]) await layout(b, form, pos, false);
      await layout(b, 'android', 'bottom', true);
      await layoutPaging(b, 'android'); await layoutPaging(b, 'desktop');
    }
    if (want('motion')) { await motion(b); await darkShots(b); }
  } finally { await b.close(); }
  ok('0 console errors', allErrs.length === 0, allErrs.slice(0, 3).join(' | '));
  console.log(`\n${pass} passed, ${fail} failed`);
  process.exit(fail ? 1 : 0);
})();

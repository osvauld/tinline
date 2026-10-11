/* Screenshots of every gate step: android + desktop x light + dark. Called from gate.flow.js with SHOTS=<dir>. */
import { createRequire } from 'module';
const require = createRequire(import.meta.url);
const L = require('/home/abe/p2p_chat/.claude/worktrees/design-refinement/docs/design/refined/qa/lib.js');

export default async function shots(b, dir, { drawer }) {
  const tag = Date.now().toString(36);
  const STEPS = ['welcome', 'choose', 'name', 'pass', 'phrase', 'confirm', 'device', 'terms', 'perms', 'done', 'linkName', 'link', 'linkPass',
    'restore', 'restoreName', 'restorePass', 'forgotPass', 'unlock', 'keylost', 'savefailed', 'savepass'];
  for (const form of ['android', 'desktop']) for (const dark of [false, true]) {
    const p = await L.page(b, { w: form === 'android' ? 600 : 1440, h: 900, dark });
    await p.goto(`http://127.0.0.1:5178/?form=${form}&theme=${dark ? 'dark' : 'light'}`); await p.waitForSelector('.device');
    const name = (s) => `${dir}/${tag}-${form}-${dark ? 'dark' : 'light'}-${s}.png`;
    const snap = async (s) => { await p.waitForTimeout(450); await p.locator('.device').screenshot({ path: name(s) }); };
    const jump = (s) => drawer(p, (d) => d.getByLabel('Jump to step').selectOption(s));
    for (const s of STEPS) { await jump(s); if (s === 'savefailed') await p.waitForTimeout(0); await snap(s); }
    // variants
    await jump('pass'); await p.getByLabel('Passphrase', { exact: true }).fill('purple elephant'); await p.getByRole('button', { name: 'Show passphrase' }).click(); await snap('pass-revealed');
    await jump('phrase'); await p.getByRole('button', { name: 'Show the words' }).click(); await snap('phrase-shown');
    await jump('confirm'); await p.getByLabel('Word 4', { exact: true }).fill('wrong'); await p.getByLabel('Word 11', { exact: true }).fill('access'); await p.getByLabel('Word 19', { exact: true }).fill('across'); await p.getByRole('button', { name: 'Done' }).click(); await snap('confirm-wrong');
    await jump('perms'); await p.getByRole('button', { name: 'Allow microphone' }).click(); await p.getByRole('button', { name: 'Allow notifications' }).click(); await snap('perms-some');
    await jump('terms'); await p.getByRole('checkbox').check(); await snap('terms-agreed');
    await jump('restore'); await p.getByLabel('Word 5', { exact: true }).fill('abondon'); await p.getByLabel('Word 6', { exact: true }).fill('abs'); await p.getByRole('button', { name: 'Continue' }).click(); await p.getByLabel('Word 6', { exact: true }).focus(); await snap('restore-error');
    await jump('link'); await drawer(p, (d) => d.getByRole('button', { name: 'Other device scans' }).click()); await snap('link-scanned');
    await drawer(p, (d) => d.getByRole('button', { name: 'Other device approves' }).click()); await p.waitForTimeout(0); await snap('link-approving');
    await jump('link'); await drawer(p, (d) => d.getByRole('button', { name: 'Code expires' }).click()); await snap('link-expired');
    await jump('link'); await drawer(p, async (d) => { await d.getByLabel('Failure reason').selectOption('net'); await d.getByRole('button', { name: 'Fail', exact: true }).click(); }); await snap('link-failed-net');
    await jump('unlock'); await p.getByLabel('Passphrase', { exact: true }).fill('wrong'); await p.getByRole('button', { name: 'Unlock' }).click(); await p.waitForTimeout(900); await snap('unlock-wrong');
    await drawer(p, async (d) => { await d.getByLabel('Tries before lockout').fill('1'); await d.getByLabel('Lockout seconds').fill('90'); });
    await jump('unlock'); await p.getByLabel('Passphrase', { exact: true }).fill('wrong'); await p.getByRole('button', { name: 'Unlock' }).click(); await p.waitForTimeout(900); await snap('unlock-lockout');
    await jump('done'); await p.evaluate(() => 0); await snap('done-create');
    await jump('restorePass'); await p.getByRole('button', { name: 'Skip for now' }).click(); await p.waitForTimeout(1500); await jump('done'); await snap('done-restore-or-create');
    console.log(`${form} ${dark ? 'dark' : 'light'}: console errors ${p.errs.length}`, p.errs.join('|'));
    await p.context().close();
  }
  console.log('shots in', dir, 'prefix', tag);
}

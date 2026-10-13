// Golden frames for the native call-field ports (desktop call_field.rs, Android CallField.kt).
// Runs the playground's own model.ts (Node >= 23 strips the types) through fixed scenarios and prints
// one line per sampled frame: scenario, t, tau, warp, then x y sigma alpha L a b for each of the 5 lobes.
// Regenerate: node tools/call-field/golden.mjs > crates/desktop/src/call_field_golden.txt
//             (and copy to android/app/src/test/resources/call_field_golden.txt)
import { create } from '../../docs/design/playground/src/lib/calls/field/model.ts';

// Classic palette note patterns (playground fx/audio.ts PAT): the field's note schedule.
const PAT = {
  ring: { period: 3200, notes: [[784, 0, 1, .5, .02], [988, .18, 1, .45, .02], [1175, .36, 1, .4, .02], [1568, .7, 1.4, .3, .02]] },
  back: { period: 4000, notes: [[392, 0, 1.2, .4, .18], [494, 0, 1.2, .35, .18]] },
  connected: { period: 0, notes: [[523, 0, .5, .45, .02], [784, .12, .7, .45, .02]] },
  ended: { period: 0, notes: [[784, 0, .45, .4, .02], [523, .12, .7, .4, .02]] },
  failed: { period: 0, notes: [[247, 0, .5, .45, .02], [196, .24, .8, .45, .02]] },
  pulse: { period: 0, notes: [[330, 0, .6, .35, .02]] },
  waiting: { period: 3000, notes: [[784, 0, .16, .22, .012], [784, .22, .2, .22, .012]] },
};
const CUE = { incoming: 'ring', outgoing: 'back', active: 'connected', waiting: 'waiting', reconnecting: 'pulse', ended: 'ended', failed: 'failed' };

// [time ms, state, cue delay ms]; frames every 16 ms (plus a few odd steps to exercise dt).
const SCEN = {
  out: [[0, 'outgoing', 0], [2600, 'connecting', 0], [3300, 'active', 217 - 120], [9000, 'waiting', 0], [12000, 'active', 0], [14000, 'reconnecting', 0], [16000, 'active', 97], [18000, 'ended', 0]],
  inc: [[0, 'incoming', 0], [4000, 'connecting', 0], [4700, 'active', 97], [8000, 'failed', 0]],
};
const out = [];
function run(name, theme, reduced, muteAt) {
  const fx = create({ theme, reduced });
  fx.setView(390, 844, 195, 300);
  const sc = SCEN[name.split('-')[0]];
  let si = 0;
  const end = 20000;
  for (let t = 0; t <= end; t += (t % 1000 === 500 ? 7 : 16)) {
    while (si < sc.length && sc[si][0] <= t) {
      const [at, st, d] = sc[si++];
      fx.setState(st, at, { cueStart: at + d, pat: PAT[CUE[st]] ?? null });
    }
    if (muteAt != null && t >= muteAt) fx.setMute(true);
    const f = fx.update(t);
    if (t % 160 < 16 || (t > 3200 && t < 4200)) {
      const nums = [f.tau, f.warp];
      for (const l of f.lobes) nums.push(l.x, l.y, l.sigma, l.alpha, ...l.lab);
      out.push([name, t, ...nums.map((v) => v.toPrecision(12))].join(' '));
    }
  }
}
run('out-light', 'light', false, null);
run('out-dark', 'dark', false, 10000);
run('inc-light', 'light', false, null);
run('out-reduced', 'light', true, null);
console.log(out.join('\n'));

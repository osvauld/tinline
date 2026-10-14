// Read-only numerical reference from the sibling playground. No app/browser/audio access.
import { readFileSync } from 'node:fs';
import vm from 'node:vm';
const source = readFileSync(process.argv[2], 'utf8')
  .replace('opts?: any', 'opts').replace(/export \{[^}]+\};/, '');
const sandbox = { Math };
vm.runInNewContext(`${source}\nthis.createField = create;`, sandbox);
const m = sandbox.createField({ theme: 'dark' });
m.setView(390, 844, 195, 844 * .35);
m.setState('active', 0, { pat: null }); m.update(0);
m.setState('incoming', 0, { pat: null });
let f;
for (let i = 1; i <= 30; i++) f = m.update(i * 1000 / 60);
console.log(JSON.stringify({ tau: f.tau, warp: f.warp, cap: f.cap, bg: f.bgLab,
  lobes: f.lobes.map(l => [l.x, l.y, l.sigma, l.alpha, ...l.lab]) }, null, 2));

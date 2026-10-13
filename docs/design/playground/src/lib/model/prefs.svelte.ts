/* Harness preferences (not app data): form factor and theme. Persisted per browser. */
import type { FormFactor, Theme } from './types';

const load = <T extends string>(k: string, d: T, ok: readonly T[]): T => {
  const q = new URLSearchParams(location.search).get(k) as T | null;
  if (q && ok.includes(q)) return q;
  try { const v = localStorage.getItem('tinline.pg.' + k) as T | null; if (v && ok.includes(v)) return v; } catch { /* private mode */ }
  return d;
};

class Prefs {
  form = $state<FormFactor>(load('form', innerWidth < 700 ? 'android' : 'desktop', ['android', 'desktop'] as const));
  theme = $state<Theme>(load('theme', 'system', ['system', 'light', 'dark'] as const));
  save() {
    try { localStorage.setItem('tinline.pg.form', this.form); localStorage.setItem('tinline.pg.theme', this.theme); } catch { /* ignore */ }
  }
}
export const prefs = new Prefs();

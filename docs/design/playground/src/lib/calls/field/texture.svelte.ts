/* Which texture the call screen's colour field uses. Same persistence as the sound palette. */
export type FieldTexture = 'fog' | 'clouds';
const KEY = 'tinline.pg.field';
const saved = (() => { try { const v = localStorage.getItem(KEY); return v === 'clouds' || v === 'fog' ? v : null; } catch { return null; } })();

class FieldPref {
  /** 'fog': soft Gaussian lobes (default). 'clouds': puffier lit cloud forms. */
  texture = $state<FieldTexture>(saved ?? 'fog');
  set(t: FieldTexture) {
    this.texture = t;
    try { localStorage.setItem(KEY, t); } catch { /* private mode */ }
  }
}
export const field = new FieldPref();
if (typeof window !== 'undefined') (window as any).__pgField = field;

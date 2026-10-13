/* One reduced-motion signal: the OS setting OR the drawer override. Every animation consults
   `motion.reduced`; CSS uses :root[data-motion=reduce] (set here), never its own media query. */

type Pref = 'system' | 'reduce' | 'full';

class MotionPref {
  pref = $state<Pref>('system');
  #os = $state(false);
  constructor() {
    if (typeof window === 'undefined') return;
    const mq = matchMedia('(prefers-reduced-motion: reduce)');
    this.#os = mq.matches;
    mq.addEventListener('change', (e) => (this.#os = e.matches));
  }
  get reduced() { return this.pref === 'reduce' || (this.pref === 'system' && this.#os); }
}

export const motion = new MotionPref();

/** Keep <html data-motion> in sync; call once from the app root inside an $effect. */
export function syncMotionAttr() {
  document.documentElement.dataset.motion = motion.reduced ? 'reduce' : 'full';
}

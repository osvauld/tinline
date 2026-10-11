/* Navigation. One route model for both form factors:
   - android: a stack; the top route fills the phone, back pops.
   - desktop: `tab` picks the sidebar section, `detail` fills the right pane, overlays stack on top.
   Route names follow the Android app's Route enum where one exists. The hash mirrors the state
   (#/chats/arjun) so any screen is a deep link and survives reload. */

export type Tab = 'chats' | 'groups' | 'calls' | 'contacts';

export type Route =
  | { name: 'home'; tab: Tab }
  | { name: 'conversation'; chat: string }
  | { name: 'groupInfo'; chat: string }
  | { name: 'contact'; person: string }
  | { name: 'search'; q?: string }
  | { name: 'settings'; section?: string }
  | { name: 'devices' }
  | { name: 'privacy'; section?: string };

const enc = (r: Route): string => {
  switch (r.name) {
    case 'home': return `/${r.tab}`;
    case 'conversation': return `/chat/${r.chat}`;
    case 'groupInfo': return `/chat/${r.chat}/info`;
    case 'contact': return `/contact/${r.person}`;
    case 'search': return `/search${r.q ? '/' + encodeURIComponent(r.q) : ''}`;
    case 'settings': return `/settings${r.section ? '/' + r.section : ''}`;
    case 'devices': return '/devices';
    case 'privacy': return `/privacy${r.section ? '/' + r.section : ''}`;
  }
};

const TABS: Tab[] = ['chats', 'groups', 'calls', 'contacts'];

function dec(path: string): Route[] {
  const p = path.replace(/^#?\/?/, '').split('/').filter(Boolean);
  const home = (tab: Tab = 'chats'): Route => ({ name: 'home', tab });
  if (!p.length) return [home()];
  if (TABS.includes(p[0] as Tab)) return [home(p[0] as Tab)];
  if (p[0] === 'chat' && p[1]) {
    const conv: Route = { name: 'conversation', chat: p[1] };
    return p[2] === 'info' ? [home(), conv, { name: 'groupInfo', chat: p[1] }] : [home(), conv];
  }
  if (p[0] === 'contact' && p[1]) return [home('contacts'), { name: 'contact', person: p[1] }];
  if (p[0] === 'search') return [home(), { name: 'search', q: p[1] ? decodeURIComponent(p[1]) : '' }];
  if (p[0] === 'settings') return [home(), { name: 'settings', section: p[1] }];
  if (p[0] === 'devices') return [home(), { name: 'settings' }, { name: 'devices' }];
  if (p[0] === 'privacy') return [home(), { name: 'settings' }, { name: 'privacy', section: p[1] }];
  return [home()];
}

class Router {
  stack = $state<Route[]>(dec(typeof location !== 'undefined' ? location.hash : ''));
  /** Direction of the last change, for push/pop transitions. */
  dir = $state<'push' | 'pop' | 'none'>('none');

  get top(): Route { return this.stack[this.stack.length - 1]; }
  get tab(): Tab { const h = this.stack[0]; return h.name === 'home' ? h.tab : 'chats'; }
  /** Desktop right pane: the deepest non-home route. */
  get detail(): Route | null { return this.stack.length > 1 ? this.stack[this.stack.length - 1] : null; }
  /** The conversation currently open anywhere in the stack (for list selection highlight). */
  get openChat(): string | null {
    for (let i = this.stack.length - 1; i >= 0; i--) { const r = this.stack[i]; if (r.name === 'conversation' || r.name === 'groupInfo') return r.chat; }
    return null;
  }

  constructor() {
    if (typeof window === 'undefined') return;
    addEventListener('hashchange', () => {
      const want = location.hash.replace(/^#/, '');
      if (want !== enc(this.top)) { this.dir = 'none'; this.stack = dec(want); }
    });
  }
  #sync() { const h = '#' + enc(this.top); if (location.hash !== h) history.replaceState(null, '', h); }

  push(r: Route) { this.dir = 'push'; this.stack.push(r); this.#sync(); }
  /** Replace everything above home (desktop list clicks: open X, don't stack conversations). */
  open(r: Route) { this.dir = 'none'; this.stack = [this.stack[0], r]; this.#sync(); }
  back() { if (this.stack.length > 1) { this.dir = 'pop'; this.stack.pop(); this.#sync(); } }
  setTab(tab: Tab) { this.dir = 'none'; this.stack = [{ name: 'home', tab }]; this.#sync(); }
  reset() { this.setTab('chats'); }
}

export const nav = new Router();

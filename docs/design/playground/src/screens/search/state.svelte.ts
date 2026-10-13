/* Shared UI state of search: the field's text (and its 150 ms debounced twin), where the field lives, recent searches,
   and the one way every hit is opened. Not app data: the data is the store; this is only what the search UI remembers. */
import { nav, type Route } from '../../lib/nav/router.svelte';
import { prefs } from '../../lib/model/prefs.svelte';
import { SEARCH, jump } from '../../lib/model/actions/search';
import type { MessageHit, NameHit } from '../../lib/model/actions/search';
import { db } from '../../lib/model/store.svelte';

export type Position = 'bottom' | 'top';
const RECENT_KEY = 'tinline.pg.recent-searches';
const loadRecents = (): string[] => { try { const v = JSON.parse(localStorage.getItem(RECENT_KEY) ?? '[]'); return Array.isArray(v) ? v.filter((x) => typeof x === 'string').slice(0, 8) : []; } catch { return []; } };

class SearchUi {
  #q = $state('');
  /** What the results show: `q` after the debounce (immediately when empty). */
  applied = $state('');
  focused = $state(false);
  /** 'default' = bottom on Android, top on desktop (drawer toggle to compare). */
  placement = $state<'default' | Position>('default');
  /** Mock soft keyboard (drawer), to check the keyboard-safe area on the Android search screen. */
  kbd = $state(false);
  recents = $state<string[]>(typeof localStorage === 'undefined' ? [] : loadRecents());
  el = $state<HTMLInputElement | null>(null);
  /** Registered by the results view: handles arrows/Enter from the field. Returns true if it used the key. */
  keys: ((e: KeyboardEvent) => boolean) | null = null;
  #timer = 0;

  get q() { return this.#q; }
  set q(v: string) {
    this.#q = v;
    clearTimeout(this.#timer);
    if (!v.trim()) this.applied = '';
    else this.#timer = window.setTimeout(() => { this.applied = this.#q; }, SEARCH.debounceMs);
  }
  /** Set the text and show its results now (a recent search, a deep link). */
  set(v: string) { this.q = v; clearTimeout(this.#timer); this.applied = v; }
  clear() { this.set(''); }

  get position(): Position { return this.placement !== 'default' ? this.placement : prefs.form === 'android' ? 'bottom' : 'top'; }
  /** Desktop: the list pane shows results instead of the list while the field has focus or text. */
  get active() { return this.focused || this.#q !== ''; }

  focus() { this.el?.focus(); this.el?.select(); }

  remember() {
    const t = this.#q.trim();
    if (Array.from(t).length < SEARCH.minChars) return;
    this.recents = [t, ...this.recents.filter((r) => r.toLowerCase() !== t.toLowerCase())].slice(0, 8);
    try { localStorage.setItem(RECENT_KEY, JSON.stringify(this.recents)); } catch { /* private mode */ }
  }
  forgetAll() { this.recents = []; try { localStorage.removeItem(RECENT_KEY); } catch { /* ignore */ } }

  /** Open the search screen (Android) from the pill, the top field or the app bar. */
  openScreen(q = '') { this.set(q); nav.push({ name: 'search', q }); }

  /** Ctrl/Cmd+K, anywhere. Desktop: focus the list pane's field. Android (a hardware keyboard): open the search screen. */
  hotkey(e: KeyboardEvent) {
    if (!(e.ctrlKey || e.metaKey) || e.altKey || e.key.toLowerCase() !== 'k') return;
    e.preventDefault();
    if (db.account.gate !== 'ready') return;
    if (prefs.form === 'desktop') this.focus();
    else if (nav.top.name !== 'search') this.openScreen();
  }
}
export const ui = new SearchUi();

const go = (r: Route) => (prefs.form === 'desktop' ? nav.open(r) : nav.push(r));

export function openName(n: NameHit) {
  ui.remember();
  if (n.kind === 'me') go({ name: 'profile' });
  else if (n.kind === 'group' || db.chats.some((c) => c.id === n.id)) go({ name: 'conversation', chat: n.id });
  else go({ name: 'contact', person: n.id });   // a contact without a conversation yet
}
export function openHit(h: MessageHit) {
  ui.remember();
  jump.set(h.conv, h.id);
  go({ name: 'conversation', chat: h.conv });
}

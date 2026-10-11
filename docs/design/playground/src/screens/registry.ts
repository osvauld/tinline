/* Route → screen component. Areas replace their Placeholder entry when ported (PLAN.md §3). */
import type { Component } from 'svelte';
import type { Route, Tab } from '../lib/nav/router.svelte';
import ChatsList from './home/ChatsList.svelte';
import Conversation from './chat/Conversation.svelte';
import Placeholder from './Placeholder.svelte';
import Gate from './gate/Gate.svelte';

type Entry = { component: Component<any>; props: (r: any) => Record<string, unknown> };
const ph = (title: string, step: string): Entry => ({ component: Placeholder, props: () => ({ title, step }) });

/** Shown instead of the app while db.account.gate !== 'ready' (sign-up, unlock). Step 2b. */
export const GATE: Entry = { component: Gate, props: () => ({}) };

/** Home tab lists (left pane on desktop, the home screen body on Android). */
export const TAB_LISTS: Record<Tab, Entry & { title: string }> = {
  chats: { title: 'Chats', component: ChatsList, props: () => ({}) },
  groups: { title: 'Groups', ...ph('Groups', '4') },
  calls: { title: 'Calls', ...ph('Calls', '3') },
  contacts: { title: 'Contacts', ...ph('Contacts', '8') },
};

export const SCREENS: Record<Exclude<Route['name'], 'home'>, Entry> = {
  conversation: { component: Conversation, props: (r) => ({ chatId: r.chat }) },
  groupInfo: ph('Group info', '4'),
  contact: ph('Contact', '8'),
  search: ph('Search', '7'),
  settings: ph('Settings', '8'),
  devices: ph('Devices', '8'),
  privacy: ph('Privacy', '8'),
};

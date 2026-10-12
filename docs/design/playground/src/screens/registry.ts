/* Route → screen component. Areas replace their Placeholder entry when ported (PLAN.md §3). */
import type { Component } from 'svelte';
import type { Route, Tab } from '../lib/nav/router.svelte';
import ChatsList from './home/ChatsList.svelte';
import Conversation from './chat/Conversation.svelte';
import Placeholder from './Placeholder.svelte';
import CallsList from './calls/CallsList.svelte';
import Gate from './gate/Gate.svelte';
import GroupsList from './groups/GroupsList.svelte';
import GroupInfo from './groups/GroupInfo.svelte';
import Settings from './settings/Settings.svelte';
import MyProfile from './profile/MyProfile.svelte';
import PersonProfile from './profile/PersonProfile.svelte';
import SearchScreen from './search/SearchScreen.svelte';
import ContactsList from './people/ContactsList.svelte';
import Devices from './devices/Devices.svelte';
import Privacy from './privacy/Privacy.svelte';

type Entry = { component: Component<any>; props: (r: any) => Record<string, unknown> };
const ph = (title: string, step: string): Entry => ({ component: Placeholder, props: () => ({ title, step }) });

/** Shown instead of the app while db.account.gate !== 'ready' (sign-up, unlock). Step 2b. */
export const GATE: Entry = { component: Gate, props: () => ({}) };

/** Home tab lists (left pane on desktop, the home screen body on Android). */
export const TAB_LISTS: Record<Tab, Entry & { title: string }> = {
  chats: { title: 'Chats', component: ChatsList, props: () => ({}) },
  groups: { title: 'Groups', component: GroupsList, props: () => ({}) },
  calls: { title: 'Calls', component: CallsList, props: () => ({}) },
  contacts: { title: 'Contacts', component: ContactsList, props: () => ({}) },
};

export const SCREENS: Record<Exclude<Route['name'], 'home'>, Entry> = {
  conversation: { component: Conversation, props: (r) => ({ chatId: r.chat }) },
  groupInfo: { component: GroupInfo, props: (r) => ({ chatId: r.chat }) },
  contact: { component: PersonProfile, props: (r) => ({ personId: r.person }) },
  profile: { component: MyProfile, props: () => ({}) },
  search: { component: SearchScreen, props: (r) => ({ q: r.q }) },
  settings: { component: Settings, props: (r) => ({ section: r.section }) },
  devices: { component: Devices, props: (r) => ({ section: r.section }) },
  privacy: { component: Privacy, props: (r) => ({ section: r.section }) },
};

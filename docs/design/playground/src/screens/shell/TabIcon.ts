import type { Tab } from '../../lib/nav/router.svelte';
export const TAB_ICON: Record<Tab, string> = { chats: 'chat', groups: 'users', calls: 'phone', contacts: 'contact' };
export const TABS: Tab[] = ['chats', 'groups', 'calls', 'contacts'];

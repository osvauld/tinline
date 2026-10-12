import { db, displayName, person } from '../../lib/model/store.svelte';
import type { Id } from '../../lib/model/types';

/** Full name for lists ("Maya Kurian" for me, "You" is added by the caller where needed). */
export const fullName = (id: Id) => (id === 'me' ? db.account.name : displayName(id));
export const hueOf = (id: Id) => (id === 'me' ? db.account.hue : (person(id)?.hue ?? 0));
export const plural = (n: number, one: string, many = one + 's') => `${n} ${n === 1 ? one : many}`;

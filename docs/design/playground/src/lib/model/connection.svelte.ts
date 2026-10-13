/* This device's link to the network (simulated). "online" means connected to the network only,
   not that any particular person can reach you; the UI must not imply more. */
import { db } from './store.svelte';

type Link = 'offline' | 'connecting' | 'online';
const CONNECT_MS = 1200; // simulated network handshake, not a motion duration

class Connection {
  state = $state<Link>('connecting');
  #t = 0;
  constructor() {
    if (typeof window === 'undefined') return;
    $effect.root(() => {
      $effect(() => {
        const want = db.env.networkOnline;
        clearTimeout(this.#t);
        if (!want) { this.state = 'offline'; return; }
        if (this.state === 'online') return;
        this.state = 'connecting';
        this.#t = window.setTimeout(() => (this.state = 'online'), CONNECT_MS);
      });
    });
  }
}
export const connection = new Connection();

<script lang="ts">
  /* Direct invite: pick contacts who aren't in the group yet. Each gets a private invite, valid 7 days (PROVISIONAL). */
  import Avatar from '../../lib/ui/Avatar.svelte';
  import Icon from '../../lib/ui/Icon.svelte';
  import Layer from '../chat/Layer.svelte';
  import Btn from './Btn.svelte';
  import { db } from '../../lib/model/store.svelte';
  import { inviteContacts, member } from '../../lib/model/actions/groups';
  import type { Group } from '../../lib/model/types';
  import { plural } from './util';

  let { g, onclose }: { g: Group; onclose: () => void } = $props();
  let picked = $state<string[]>([]);
  const choices = $derived(db.people.filter((p) => p.contact && !p.blocked && !member(g, p.id) && !g.invited.some((i) => i.id === p.id)));
  const toggle = (id: string) => (picked = picked.includes(id) ? picked.filter((x) => x !== id) : [...picked, id]);
  function send() { inviteContacts(g, picked); onclose(); }
</script>

<Layer variant="dialog" label="Invite contacts" {onclose}>
  <div class="dlg">
    <h2>Invite to {g.name}</h2>
    {#if choices.length}
      <div class="list" role="group" aria-label="Contacts">
        {#each choices as p (p.id)}
          {@const on = picked.includes(p.id)}
          <button type="button" class="li press" role="checkbox" aria-checked={on} onclick={() => toggle(p.id)}>
            <Avatar name={p.name} hue={p.hue} person={p.id} size={40} />
            <span class="nm">{p.name}</span>
            <span class="box" class:on>{#if on}<Icon name="check" size={16} />{/if}</span>
          </button>
        {/each}
      </div>
      <p>Each person gets a private invite that works for 7 days. Anyone you invite can read the group’s posts once they join.</p>
    {:else}
      <p class="none">Everyone in your contacts is already in this group or has an invite waiting.</p>
    {/if}
    <div class="acts">
      <Btn variant="quiet" data-autofocus onclick={onclose}>Cancel</Btn>
      <Btn variant="primary" disabled={!picked.length} onclick={send}>{picked.length ? `Invite ${plural(picked.length, 'person', 'people')}` : 'Invite'}</Btn>
    </div>
  </div>
</Layer>

<style>
  .dlg { padding: 20px 20px 16px; }
  .dlg h2 { margin: 0 0 6px; font-size: var(--fs-title); font-weight: 650; line-height: 24px; overflow-wrap: anywhere; }
  .dlg p { margin: 10px 0 0; color: var(--muted); font-size: var(--fs-sub); line-height: 19px; }
  .list { display: grid; margin: 8px -8px 0; max-block-size: 320px; overflow: auto; }
  .li { display: flex; align-items: center; gap: 12px; min-block-size: 56px; padding: 6px 10px; border: 0; border-radius: var(--r-ctl); background: none; text-align: start; inline-size: 100%; color: var(--ink); font: inherit; cursor: pointer; }
  .li:hover { background: var(--surface-2); }
  .nm { flex: 1; min-inline-size: 0; font-weight: 600; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .box { inline-size: 22px; block-size: 22px; border-radius: 7px; box-shadow: inset 0 0 0 2px var(--line); display: grid; place-items: center; color: var(--on-accent); flex: none;
    transition: background-color var(--dur-switch) var(--ease-out), box-shadow var(--dur-switch) var(--ease-out); }
  .box.on { background: var(--accent); box-shadow: none; }
  .acts { display: flex; gap: 8px; justify-content: flex-end; margin-block-start: 18px; }
</style>

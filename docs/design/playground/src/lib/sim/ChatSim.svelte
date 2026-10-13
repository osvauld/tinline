<script lang="ts">
  /* Chat simulation controls: who is online, incoming messages and files, call records.
     Lives only in the drawer, never in the app frame. */
  import Switch from '../ui/Switch.svelte';
  import { db, chat, isDirect } from '../model/store.svelte';
  import { addCallRecord, receiveFile, receiveText } from '../model/actions/chat';
  import { nav } from '../nav/router.svelte';

  const REPLIES = ['On my way', 'Sounds good', 'Can you send that again?', 'Ha, yes', 'Give me ten minutes', 'See you there'];
  let n = 0;
  const direct = $derived(db.chats.filter(isDirect).map((c) => db.people.find((p) => p.id === c.person)!).filter(Boolean));
  // The conversation open now, if any: sim buttons act on it first.
  const open = $derived(chat(nav.openChat));

  const reply = (id: string) => receiveText(id, id, REPLIES[n++ % REPLIES.length]);
  const hikePost = () => receiveText('hike', 'sam', REPLIES[n++ % REPLIES.length]);
  const file = (id: string) => receiveFile(id, id, { name: 'trip-photos.zip', size: 8.2e6, kind: 'other' });
  const call = (id: string, direction: 'in' | 'out', outcome: 'answered' | 'missed') =>
    addCallRecord(id, { kind: 'voice', outcome, direction, durationS: outcome === 'answered' ? 134 : 0 });
</script>

<div class="rows">
  {#each direct as p (p.id)}
    <label class="row"><span>{p.name.split(' ')[0]} online</span><Switch label="{p.name} online" bind:checked={p.online} /></label>
  {/each}
  <div class="btns">
    <button type="button" onclick={() => reply('arjun')}>Arjun replies</button>
    <button type="button" onclick={() => reply('meera')}>Meera replies</button>
    <button type="button" onclick={hikePost}>Sam posts in Weekend hike</button>
    <button type="button" onclick={() => file(open && isDirect(open) ? open.id : 'arjun')}>Simulate incoming file</button>
  </div>
  <div class="btns">
    <button type="button" onclick={() => call(open && isDirect(open) ? open.id : 'arjun', 'out', 'answered')}>Add outgoing call record</button>
    <button type="button" onclick={() => call(open && isDirect(open) ? open.id : 'arjun', 'in', 'missed')}>Add missed incoming call</button>
  </div>
  <p class="note">Proposed, not decided: per-message delete (for me / for everyone, with Undo here). Group posts stay text + reply only, per the core.</p>
  <p class="note">Proposed, not decided: sending a message plucks the logo's string (silent; a small twang when online, a tiny swing when not; nothing under reduced motion).</p>
</div>

<style>
  .rows { display: grid; gap: 4px; }
  .row { display: flex; align-items: center; justify-content: space-between; gap: 12px; min-height: 40px; font-size: var(--fs-sub); }
  .btns { display: flex; flex-wrap: wrap; gap: 6px; margin-top: 8px; }
  button { border: 1px solid var(--line); background: var(--surface); border-radius: 10px; padding: 6px 10px; font-size: var(--fs-sub); cursor: pointer; }
  .note { font-size: var(--fs-sub); color: var(--muted); margin: 8px 0 0; }
</style>

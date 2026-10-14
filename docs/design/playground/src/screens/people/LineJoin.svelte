<script lang="ts">
  /* Two people and the line between them: me · mark · them. Shown on BOTH sides while a contact is being added.
     slack    the code is recognised, nothing sent yet
     ringing  the handshake is running: the same "connecting" sway a call uses (status.svelte.ts), so "being
              connected" looks the same everywhere in Tinline
     taut     the handshake is complete: two tugs (LINE_SPRING, as sign-up Done and the empty state). The `line` cue
              comes from adding the contact (addPerson), on both sides at the same moment.
     One mark instance across all three states, so each change animates. Fixed 64 high: nothing moves. */
  import Avatar from '../../lib/ui/Avatar.svelte';
  import Mark from '../../lib/brand/Mark.svelte';
  import { LINE_SPRING } from '../../lib/brand/springs';
  import { db } from '../../lib/model/store.svelte';
  type Who = { id: string; name: string; hue: number };
  let { them, state }: { them: Who; state: 'slack' | 'ringing' | 'taut' } = $props();
  const CONNECTING = { period: 1.2, amp: 6 };
</script>

<div class="join" data-line={state}>
  <Avatar name={db.account.name} hue={db.account.hue} person="me" size={56} />
  <span class="ln"><Mark size={64} live lineColor {state} sway={CONNECTING} spring={LINE_SPRING}
    label={state === 'taut' ? 'Connected' : state === 'ringing' ? 'Connecting' : 'Not connected yet'} /></span>
  <Avatar name={them.name} hue={them.hue} person={them.id} size={56} />
</div>

<style>
  .join { display: flex; align-items: center; justify-content: center; gap: 6px; block-size: 64px; color: var(--ink); }
  .ln { display: grid; place-items: center; }
</style>

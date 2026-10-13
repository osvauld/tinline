<script lang="ts">
  /* Blocked people. A block is visible (U5): the person is told, their app stops contacting me and removes me from their
     contacts (U19/U20); my chat history stays (U24). A block is personal: it changes nothing in groups. */
  import Layer from '../chat/Layer.svelte';
  import Btn from '../groups/Btn.svelte';
  import PersonRow from '../people/PersonRow.svelte';
  import Icon from '../../lib/ui/Icon.svelte';
  import { db, firstName, displayName, person } from '../../lib/model/store.svelte';
  import { block, unblock } from '../../lib/model/actions/people.svelte';

  type L = null | { k: 'pick' } | { k: 'block' | 'unblock'; id: string };
  let layer = $state<L>(null);
  let opener: HTMLElement | null = null;
  let said = $state('');
  const ask = (l: L, e?: Event) => { if (e) opener = e.currentTarget as HTMLElement; layer = l; };
  const close = () => { layer = null; queueMicrotask(() => opener?.isConnected && opener.focus({ preventScroll: true })); };
  const blocked = $derived(db.people.filter((p) => p.blocked).sort((a, b) => displayName(a.id).localeCompare(displayName(b.id))));
  const choices = $derived(db.people.filter((p) => p.contact && !p.blocked).sort((a, b) => displayName(a.id).localeCompare(displayName(b.id))));
  const target = $derived(layer && layer.k !== 'pick' ? person(layer.id) : undefined);
  const fn = $derived(target ? firstName(target.id) : '');
  function confirm() {
    if (!layer || layer.k === 'pick') return;
    const n = firstName(layer.id);
    if (layer.k === 'block') { block(layer.id); said = `${n} is blocked.`; } else { unblock(layer.id); said = `${n} is unblocked.`; }
    layer = null;
  }
</script>

<div class="pf-scroll" inert={!!layer} data-blocked-page>
<div class="pf-col">
  <p class="vh" role="status" aria-live="polite">{said}</p>
  <section class="pf-sec first" aria-labelledby="bl-h">
    <h2 id="bl-h">Blocked</h2>
    <div class="pf-card">
      {#each blocked as p (p.id)}
        <PersonRow {p} sub="Told you blocked them" pad={12}>
          {#snippet end()}<Btn variant="outline" data-unblock={p.id} label="Unblock {firstName(p.id)}" onclick={(e) => ask({ k: 'unblock', id: p.id }, e)}>Unblock</Btn>{/snippet}
        </PersonRow>
      {:else}
        <p class="none" data-none>No one is blocked.</p>
      {/each}
      <button type="button" class="pf-act press" data-block-someone onclick={(e) => ask({ k: 'pick' }, e)}><Icon name="off" size={20} /><span class="t"><b>Block someone</b></span></button>
    </div>
    <p class="pf-note">Blocking is not silent. The person is told, their app stops contacting you and removes you from their contacts. Your chat history stays on your devices. Groups you share aren’t affected.</p>
  </section>
</div>
</div>

{#if layer?.k === 'pick'}
  <Layer variant="dialog" label="Block someone" onclose={close}>
    <div class="pf-dlg pick"><h2>Block someone</h2><p>Pick a contact.</p>
      <div class="plist">
        {#each choices as p (p.id)}<PersonRow {p} sub="" pad={0} onclick={() => ask({ k: 'block', id: p.id })} />{:else}<p class="none">No one left to block.</p>{/each}
      </div>
      <div class="pf-acts"><Btn variant="quiet" data-autofocus onclick={close}>Cancel</Btn></div></div>
  </Layer>
{:else if layer && target}
  <Layer variant="dialog" label="{layer.k === 'block' ? 'Block' : 'Unblock'} {fn}" onclose={close}>
    {#if layer.k === 'block'}
      <div class="pf-dlg" data-block>
        <h2>Block {fn}?</h2>
        <p class="lead">{fn} will be told you blocked them. Their app stops contacting you and removes you from their contacts, and you stop getting their calls, messages and profile updates.</p>
        <p>Your chat stays on your devices. Groups you share aren’t affected. You can unblock {fn} here at any time.</p>
        <div class="pf-acts"><Btn variant="quiet" data-autofocus onclick={close}>Cancel</Btn><Btn variant="danger" onclick={confirm}>Block</Btn></div>
      </div>
    {:else}
      <div class="pf-dlg" data-unblock-dlg>
        <h2>Unblock {fn}?</h2>
        <p class="lead">{fn}’s app can reach you again. It removed you from their contacts when you blocked them, so you’ll both need to add each other again before you can message or call.</p>
        <p>Your chat history is still here.</p>
        <div class="pf-acts"><Btn variant="quiet" data-autofocus onclick={close}>Cancel</Btn><Btn variant="primary" onclick={confirm}>Unblock</Btn></div>
      </div>
    {/if}
  </Layer>
{/if}

<style>
  .first { margin-block-start: 16px; }
  .none { margin: 0; padding: 18px 16px; color: var(--muted); text-align: center; font-size: var(--fs-sub); }
  .plist { margin-block-start: 8px; max-block-size: 320px; overflow: auto; }
</style>

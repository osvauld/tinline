<script lang="ts">
  /* Add a piece of information: pick a known kind (it gets the right control) or name your own. Patches ONE key. */
  import Layer from '../chat/Layer.svelte';
  import Icon from '../../lib/ui/Icon.svelte';
  import Btn from '../groups/Btn.svelte';
  import { KNOWN, labelOf, normaliseLink } from './info';
  import { checkKey, isReserved, setField } from '../../lib/model/actions/profile';

  let { have, onclose }: { have: string[]; onclose: () => void } = $props();

  const ICON: Record<string, string> = { pronouns: 'users', location: 'info', website: 'link' };
  const choices = $derived(Object.keys(KNOWN).filter((k) => !have.includes(k)));
  let pick = $state<string | null>(null);        // a KNOWN key, or '' for something else
  let name = $state('');
  let val = $state('');
  let error = $state('');
  const uid = $props.id();

  const key = $derived(pick ? pick : name.trim().toLowerCase().replace(/\s+/g, '-'));
  const kind = $derived(pick && KNOWN[pick]?.kind === 'link' ? 'link' : 'text');

  function save(e?: Event) {
    e?.preventDefault();
    if (pick === '') {
      const c = checkKey(name);
      if (!c.ok) return (error = c.error);
      if (isReserved(key) || key in KNOWN || have.includes(key)) return (error = 'You already have information with that name.');
    }
    let v: string | null = val.trim();
    if (!v) return (error = 'Add something to save.');
    if (kind === 'link') { v = normaliseLink(val); if (!v) return (error = 'Enter a web address that starts with http:// or https://.'); }
    const r = setField(key, v);
    if (r.ok) onclose(); else error = r.error;
  }
  const focusIn = (el: HTMLElement, on = true) => { if (on) el.focus(); };
</script>

<Layer variant="dialog" label="Add information" {onclose}>
  <div class="pf-dlg" data-add-info>
    {#if pick === null}
      <h2>Add information</h2>
      <p>Your contacts will see it on your profile.</p>
      <div class="list" role="group" aria-label="Kinds of information">
        {#each choices as k (k)}
          <button type="button" class="pf-act press" onclick={() => (pick = k)}><Icon name={ICON[k] ?? 'info'} size={20} /><span class="t"><b>{labelOf(k)}</b></span></button>
        {/each}
        <button type="button" class="pf-act press" onclick={() => (pick = '')} data-autofocus={choices.length ? undefined : ''}><Icon name="plus" size={20} /><span class="t"><b>Something else</b><small>Name it yourself</small></span></button>
      </div>
      <div class="pf-acts"><Btn variant="quiet" onclick={onclose}>Cancel</Btn></div>
    {:else}
      <h2>{pick ? labelOf(pick) : 'Something else'}</h2>
      <form onsubmit={save} novalidate>
        {#if pick === ''}
          <label for="{uid}-n">What is it called?</label>
          <input id="{uid}-n" type="text" bind:value={name} placeholder="For example, Favourite tea" autocomplete="off" use:focusIn />
        {/if}
        <label for="{uid}-v">{pick ? labelOf(pick) : 'Value'}</label>
        <input id="{uid}-v" type="text" bind:value={val} placeholder={pick ? KNOWN[pick].hint : ''} autocapitalize="none" autocomplete="off" spellcheck="false" inputmode={kind === 'link' ? 'url' : undefined} use:focusIn={pick !== ''} />
        {#if error}<p class="pf-err" role="alert">{error}</p>{/if}
        <div class="pf-acts"><Btn variant="quiet" onclick={() => { pick = null; error = ''; }}>Back</Btn><Btn variant="primary" type="submit">Add</Btn></div>
      </form>
    {/if}
  </div>
</Layer>

<style>
  .list { margin: 10px -12px 0; display: grid; }
  .list .pf-act { border-radius: var(--r-ctl); padding-inline: 12px; }
  form { display: grid; gap: 6px; margin-block-start: 12px; }
  label { font-size: var(--fs-sub); font-weight: 600; color: var(--muted); line-height: 18px; margin-block-start: 6px; }
  input { inline-size: 100%; padding: 10px 14px; border-radius: var(--r-ctl); border: 1px solid var(--line); background: var(--surface); font-size: var(--fs-body); line-height: 21px; color: var(--ink); min-block-size: var(--btn); }
  input:focus { outline: 0; border-color: var(--accent); box-shadow: 0 0 0 3px color-mix(in srgb, var(--accent-ink) 14%, transparent); }
</style>

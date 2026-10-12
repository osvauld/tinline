<script lang="ts">
  /* My availability: Available or Busy (until a time, or until I change it). One setting for the whole account, synced
     across my devices, and separate from whether I'm online. A deadline that passes means Available again (sim clock). */
  import Icon from '../../lib/ui/Icon.svelte';
  import { clock } from '../../lib/model/clock.svelte';
  import { BUSY_CHOICES, effectiveAvailability, setAvailability, type Result } from '../../lib/model/actions/profile';
  import { untilLabel } from './info';

  let { offline = false, onsaved }: { offline?: boolean; onsaved?: () => void } = $props();

  const av = $derived(effectiveAvailability('me', clock.now));
  let choice = $state('1h');
  let error = $state('');
  let saved = $state(false);
  let timer = 0;
  const SAVED_MS = 1800; // how long "Saved" stays; a UI constant, not a motion timing
  const active = $derived(av.available ? null : av.until === undefined ? 'open' : choice);
  const line = $derived(av.available ? 'Available' : av.until === undefined ? 'Busy until you change it' : `Busy until ${untilLabel(av.until)}`);

  function apply(r: Result) {
    if (!r.ok) { error = r.error; return; }
    error = ''; saved = true; onsaved?.();
    clearTimeout(timer); timer = window.setTimeout(() => (saved = false), SAVED_MS);
  }
  function setBusy(id: string) { choice = id; const c = BUSY_CHOICES.find((x) => x.id === id)!; apply(setAvailability({ available: false, until: c.until(clock.now) })); }
  function radioKey(e: KeyboardEvent) {
    if (!['ArrowRight', 'ArrowLeft', 'ArrowDown', 'ArrowUp'].includes(e.key)) return;
    e.preventDefault();
    if (av.available) setBusy(choice); else apply(setAvailability({ available: true }));
    queueMicrotask(() => (e.currentTarget as HTMLElement).parentElement?.querySelector<HTMLElement>('[aria-checked=true]')?.focus());
  }
</script>

<div class="av" data-availability>
  <div class="seg" role="radiogroup" aria-label="Availability">
    <button type="button" role="radio" class="s press" aria-checked={av.available} tabindex={av.available ? 0 : -1} onkeydown={radioKey} onclick={() => apply(setAvailability({ available: true }))}>Available</button>
    <button type="button" role="radio" class="s press" aria-checked={!av.available} tabindex={av.available ? -1 : 0} onkeydown={radioKey} onclick={() => { if (av.available) setBusy(choice); }}>Busy</button>
  </div>
  <p class="now" role="status"><span class="ln" class:err={!!error}>{#if error}<span class="bad" role="alert">{error}</span>{:else}{line}{/if}</span>{#if saved}<span class="saved" class:lead={!av.available}><Icon name="check" size={13} />{offline ? 'Saved · syncs when connected' : 'Saved'}</span>{/if}</p>
  {#if !av.available}
    <div class="for" role="group" aria-label="For how long">
      {#each BUSY_CHOICES as c (c.id)}
        <button type="button" class="t-chip press" aria-pressed={active === c.id} onclick={() => setBusy(c.id)}>{c.label}</button>
      {/each}
    </div>
  {/if}
</div>

<style>
  .av { padding: 14px 16px 14px; display: grid; gap: 10px; }
  .seg { display: grid; grid-template-columns: 1fr 1fr; gap: 4px; padding: 4px; border-radius: 16px; background: var(--surface-2); }
  .s { min-block-size: var(--btn); border: 0; border-radius: 12px; background: transparent; color: var(--muted); font-weight: 600; cursor: pointer;
    transition: background-color var(--dur-switch) var(--ease-out), color var(--dur-switch) var(--ease-out), box-shadow var(--dur-switch) var(--ease-out); }
  .s[aria-checked='true'] { background: var(--surface); color: var(--ink); box-shadow: 0 1px 2px rgb(9 23 17 / .12), inset 0 0 0 1px var(--hairline); }
  .now { margin: 0; font-size: var(--fs-sub); line-height: 19px; color: var(--ink); font-variant-numeric: tabular-nums; display: flex; align-items: center; gap: 10px; white-space: nowrap; }
  .now { min-block-size: 19px; }   /* always there: the line (Available / Busy until…) holds the Saved tag */
  .ln { min-inline-size: 0; overflow: hidden; text-overflow: ellipsis; }
  .ln.err { white-space: normal; overflow: visible; }   /* a refusal is read in full; it replaces the line, so it only grows if it must wrap */
  .bad { color: var(--danger); }
  .saved { flex: none; margin-inline-start: auto; }
  .saved { display: inline-flex; align-items: center; gap: 3px; font-weight: 600; color: var(--accent-ink); animation: fadein var(--dur-enter) var(--ease-out); }
  @keyframes fadein { from { opacity: 0; } }
  .for { display: flex; flex-wrap: wrap; gap: 8px; }
  .for .t-chip { min-block-size: 36px; cursor: pointer; }
</style>

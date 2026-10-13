<script lang="ts">
  import Icon from '../../lib/ui/Icon.svelte';
  import Btn from './Btn.svelte';
  import StepShell from './StepShell.svelte';
  import { gate } from '../../lib/model/actions/gate.svelte';
  const pts = [
    ['shield', 'No ads, no tracking.', 'We don’t collect your contacts, calls or location.'],
    ['lock', 'Calls are end-to-end encrypted.', 'When a direct line isn’t possible, an encrypted relay passes the call along. It can’t hear it.'],
    ['key', 'You hold the keys.', 'Lose your ' + gate.noun + ' and your recovery phrase, and nobody, not even Osvauld, can get your account back.'],
    ['warning', 'Not for emergency calls.', ''],
  ];
</script>

<StepShell title="Before you start">
  <ul>
    {#each pts as [ic, b, r] (b)}
      <li><span class="ic"><Icon name={ic} size={20} /></span><span><b>{b}</b>{#if r}<br />{r}{/if}</span></li>
    {/each}
  </ul>
  <label class="agree">
    <input type="checkbox" bind:checked={gate.agreed} />
    <span>I agree to the <button type="button" class="lk">Terms of Use</button> and have read the <button type="button" class="lk">Privacy Policy</button>.</span>
  </label>
  {#snippet foot()}
    <Btn disabled={!gate.agreed} onclick={() => gate.go('perms')}>Agree and continue</Btn>
  {/snippet}
</StepShell>

<style>
  ul { list-style: none; margin: 0; padding: 0; display: grid; gap: 14px; }
  li { display: flex; gap: 14px; align-items: flex-start; font-size: var(--fs-body); line-height: 21px; }
  li span:last-child { color: var(--muted); } li b { color: var(--ink); }
  .ic { display: grid; place-items: center; inline-size: 36px; block-size: 36px; border-radius: 50%; background: var(--accent-soft); color: var(--accent-ink); flex: none; }
  .agree { display: flex; gap: 12px; align-items: center; min-block-size: 48px; font-size: var(--fs-sub); cursor: pointer; }
  .agree input { inline-size: 24px; block-size: 24px; accent-color: var(--accent); flex: none; margin: 10px; }
  .lk { border: 0; background: none; padding: 0; color: var(--accent-ink); text-decoration: underline; font: inherit; cursor: pointer; }
</style>

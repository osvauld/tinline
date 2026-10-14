<script lang="ts">
  import Icon from '../../lib/ui/Icon.svelte';
  import Btn from './Btn.svelte';
  import StepShell from './StepShell.svelte';
  import { gate } from '../../lib/model/actions/gate.svelte';
  const pts = [
    ['shield', 'Free of ads and tracking.', 'Tinline doesn’t collect your contacts, calls or location.'],
    ['lock', 'Calls are end-to-end encrypted.', 'When a direct line isn’t possible, an encrypted relay passes the call along. It can’t hear it.'],
    ['key', 'Your account is yours.', 'It lives on your devices, not on a server. Your recovery words are the way back in, so keep them safe.'],
    ['warning', 'Not for emergency calls.', ''],
  ];
</script>

<StepShell title="Before you start">
  <ul>
    {#each pts as [ic, b, r] (b)}
      <li><span class="ic"><Icon name={ic} size={20} /></span><span><b>{b}</b>{#if r}<br />{r}{/if}</span></li>
    {/each}
  </ul>
  <!-- No checkbox: the next button is the agreement, and this line sits right above it saying so. -->
  <p class="agree">By continuing, you agree to the <button type="button" class="lk">Terms of Use</button> and have read the <button type="button" class="lk">Privacy Policy</button>.</p>
  {#snippet foot()}
    <Btn next label="I agree" onclick={() => { gate.agreed = true; gate.go('perms'); }}>I agree</Btn>
  {/snippet}
</StepShell>

<style>
  ul { list-style: none; margin: 0; padding: 0; display: grid; gap: 14px; }
  li { display: flex; gap: 14px; align-items: flex-start; font-size: var(--fs-body); line-height: 21px; }
  li span:last-child { color: var(--muted); } li b { color: var(--ink); }
  .ic { display: grid; place-items: center; inline-size: 36px; block-size: 36px; border-radius: 50%; background: var(--accent-soft); color: var(--accent-ink); flex: none; }
  .agree { margin: auto 0 0; font-size: var(--fs-sub); line-height: 19px; color: var(--muted); }   /* bottom of the body, next to the button */
  .lk { border: 0; background: none; padding: 0; color: var(--accent-ink); text-decoration: underline; font: inherit; cursor: pointer; }
</style>

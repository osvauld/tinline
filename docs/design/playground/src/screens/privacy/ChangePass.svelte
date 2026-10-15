<script lang="ts">
  /* Change the passphrase. It protects Tinline on THIS device; the app cannot recover a forgotten one (the recovery phrase
     restores the account instead). */
  import Btn from '../groups/Btn.svelte';
  import PassField from '../people/PassField.svelte';
  import Icon from '../../lib/ui/Icon.svelte';
  import { gateSim, canRemember, rememberCopy } from '../../lib/model/actions/gate.svelte';
  import Switch from '../../lib/ui/Switch.svelte';

  let cur = $state(''), next = $state(''), again = $state('');
  let errs = $state({ cur: '', next: '', again: '' });
  let done = $state(false);
  const clear = () => { errs = { cur: '', next: '', again: '' }; done = false; };

  function save() {
    const e = { cur: '', next: '', again: '' };
    if (gateSim.hasPassphrase && cur !== gateSim.passphrase) e.cur = 'That isn’t your current passphrase.';
    if (next.length < 8) e.next = 'Use at least 8 characters.';
    else if (next === cur) e.next = 'Pick one that is different from the current one.';
    if (!e.next && again !== next) e.again = 'The two passphrases differ.';
    errs = e;
    if (e.cur || e.next || e.again) return;
    gateSim.passphrase = next; gateSim.hasPassphrase = true;
    cur = next = again = ''; done = true;
  }
</script>

<div class="pf-scroll" data-passphrase-page>
<div class="pf-col">
  <!-- Remember (2026-10-15): the same switch as Unlock. A computer without a keyring says why it keeps asking. -->
  <section class="pf-sec first" data-remember>
    <div class="pf-card">
      {#if canRemember()}
        {@const rc = rememberCopy()}
        <label class="pf-act sw"><Icon name="key" size={20} /><span class="t"><b>{rc.label}</b><small>{rc.sub}</small></span><Switch label={rc.label} bind:checked={gateSim.remembered} /></label>
      {:else}
        <div class="pf-act sw"><Icon name="key" size={20} /><span class="t"><b>Remember on this computer</b><small>This computer has no keyring, so Tinline asks for your passphrase each time it starts.</small></span></div>
      {/if}
    </div>
  </section>
  <section class="pf-sec">
    <form class="f" onsubmit={(e) => { e.preventDefault(); save(); }}>
      {#if gateSim.hasPassphrase}<PassField id="pc-cur" bind:value={cur} error={errs.cur} label="Current passphrase" oninput={clear} />{/if}
      <PassField id="pc-new" bind:value={next} error={errs.next} label="New passphrase" hint="At least 8 characters" oninput={clear} />
      <PassField id="pc-again" bind:value={again} error={errs.again} label="New passphrase again" oninput={clear} />
      <div class="row">
        <p class="saved" role="status" data-pass-saved>{#if done}<Icon name="check" size={16} />Passphrase changed{/if}</p>
        <Btn variant="primary" type="submit" disabled={!next || !again}>Change passphrase</Btn>
      </div>
    </form>
    <p class="pf-note">The passphrase protects Tinline on this device only; your other devices keep their own. Tinline can’t recover a forgotten passphrase. If that happens, your recovery phrase restores your account, though not your messages.</p>
  </section>
</div>
</div>

<style>
  .sw { cursor: default; }
  .first { margin-block-start: 16px; }
  .f { display: grid; gap: 6px; }
  .row { display: flex; align-items: center; justify-content: space-between; gap: 12px; margin-block-start: 4px; }
  .saved { margin: 0; display: inline-flex; align-items: center; gap: 6px; min-block-size: 22px; color: var(--accent-ink); font-weight: 600; font-size: var(--fs-sub); }
</style>

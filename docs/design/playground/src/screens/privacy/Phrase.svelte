<script lang="ts">
  /* View the recovery phrase. Behind the passphrase; words hidden until asked; hide again after 30 s or when the app leaves
     the foreground; no copy button anywhere (Proposed, from the devices study). It is the only backup of the account itself. */
  import { onDestroy } from 'svelte';
  import Icon from '../../lib/ui/Icon.svelte';
  import Btn from '../groups/Btn.svelte';
  import PassField from '../people/PassField.svelte';
  import { DEMO_PHRASE, DEMO_KEY, gateSim } from '../../lib/model/actions/gate.svelte';

  const SECS = 30;
  let authed = $state(false);
  let pass = $state(''), err = $state('');
  let shown = $state(false), left = $state(SECS);
  /* Words | Key (core recovery_key.rs): the key alone can be copied, since it is meant for a password manager. */
  let asKey = $state(false), copied = $state(false);
  async function copyKey() { try { await navigator.clipboard.writeText(DEMO_KEY); copied = true; } catch { /* clipboard may be unavailable */ } }
  let timer = 0;

  function unlock() {
    if (gateSim.hasPassphrase && pass !== gateSim.passphrase) { err = 'That passphrase isn’t right.'; return; }
    authed = true; pass = ''; err = '';
  }
  function hide() { shown = false; clearInterval(timer); left = SECS; }
  function reveal() {
    shown = true; left = SECS; clearInterval(timer);
    timer = window.setInterval(() => { left--; if (left <= 0) hide(); }, 1000);
  }
  const away = () => { if (document.hidden) hide(); };
  onDestroy(() => clearInterval(timer));
</script>

<svelte:document onvisibilitychange={away} />

<div class="pf-scroll" data-phrase-page>
<div class="pf-col">
  {#if !authed}
    <section class="pf-sec first">
      <p class="lead">Your recovery phrase is the only way to get your account back if you lose every device. Anyone who has it can act as you.</p>
      <form class="gate" onsubmit={(e) => { e.preventDefault(); unlock(); }}>
        {#if gateSim.hasPassphrase}<PassField id="ph-pass" bind:value={pass} error={err} autofocus label="Enter your passphrase to continue" oninput={() => (err = '')} />{/if}
        <Btn variant="primary" type="submit" data-unlock-phrase>{gateSim.hasPassphrase ? 'Continue' : 'Confirm with phone lock'}</Btn>
      </form>
    </section>
  {:else}
    <section class="pf-sec first">
      <div class="head">
        <p class="lead">{asKey ? 'Your recovery key is the same as your 24 words, as one line. A password manager is a good place for it.' : 'Write these 24 words down, in order, and keep them somewhere private.'}</p>
        <div class="seg" role="group" aria-label="Show as">
          <button type="button" aria-pressed={!asKey} onclick={() => { asKey = false; copied = false; }}>Words</button>
          <button type="button" aria-pressed={asKey} onclick={() => (asKey = true)}>Key</button>
        </div>
      </div>
      {#if asKey}
        <div class="pf-card keyc" class:hidden={!shown} data-key><code>{shown ? DEMO_KEY : '•'.repeat(24)}</code></div>
      {:else}
      <div class="pf-card words" class:hidden={!shown} data-words data-shown={shown}>
        <ol>
          {#each DEMO_PHRASE as w, i (i)}
            <li><span class="n">{i + 1}</span>{#if shown}<b>{w}</b>{:else}<b class="dots" aria-hidden="true">••••••</b><span class="vh">hidden</span>{/if}</li>
          {/each}
        </ol>
      </div>
      {/if}
      <div class="bar">
        <Btn variant={shown ? 'outline' : 'primary'} data-toggle-phrase onclick={() => (shown ? hide() : reveal())}><Icon name={shown ? 'eyeoff' : 'eye'} size={18} />{shown ? (asKey ? 'Hide key' : 'Hide words') : (asKey ? 'Show key' : 'Show words')}</Btn>
        {#if asKey}<Btn variant="outline" onclick={copyKey}><Icon name={copied ? 'check' : 'copy'} size={18} />{copied ? 'Copied' : 'Copy my key'}</Btn>{/if}
        <span class="count" aria-hidden="true">{shown ? `Hides in ${left} s` : ''}</span>
      </div>
      <p class="pf-note">{#if asKey}Anyone who has it can act as you, just like the words. Tinline never asks for it except to restore your account.{:else}There is no copy button on purpose: the phrase stays off the clipboard. Tinline never asks for it except to restore your account. The phrase brings back your account, not your messages.{/if}</p>
    </section>
  {/if}
</div>
</div>

<style>
  .first { margin-block-start: 16px; }
  .lead { margin: 0 4px 12px; line-height: 21px; }
  .gate { display: grid; gap: 8px; }
  .gate :global(.b) { justify-self: end; }
  .words { padding: 14px 16px; }
  .seg { display: inline-flex; gap: 4px; padding: 3px; border-radius: 12px; background: var(--surface-2); margin: 0 4px 12px; }
  .seg button { border: 0; padding: 0 14px; min-block-size: 36px; border-radius: 9px; font: inherit; font-size: var(--fs-sub); background: transparent; color: var(--ink); cursor: pointer; }
  .seg button[aria-pressed='true'] { background: var(--surface-raised, var(--bg)); box-shadow: var(--shadow-lift); font-weight: 600; }
  .keyc { padding: 14px 16px; }
  .keyc code { font: 15px/24px ui-monospace, 'SF Mono', Menlo, Consolas, monospace; word-break: break-all; }
  ol { list-style: none; margin: 0; padding: 0; display: grid; grid-template-columns: repeat(2, 1fr); grid-auto-flow: column; grid-template-rows: repeat(12, auto); gap: 6px 16px; }
  @media (min-width: 540px) { ol { grid-template-columns: repeat(3, 1fr); grid-template-rows: repeat(8, auto); } }
  li { display: flex; align-items: baseline; gap: 8px; min-block-size: 26px; font-size: var(--fs-body); }
  .n { inline-size: 22px; text-align: end; color: var(--muted); font-size: var(--fs-sub); font-variant-numeric: tabular-nums; }
  b { font-weight: 600; font-variant-numeric: tabular-nums; }
  .dots { color: var(--muted); letter-spacing: .1em; font-weight: 400; }
  .bar { display: flex; align-items: center; justify-content: space-between; gap: 12px; margin-block-start: 12px; }
  .count { color: var(--muted); font-size: var(--fs-sub); font-variant-numeric: tabular-nums; min-inline-size: 90px; text-align: end; }
</style>

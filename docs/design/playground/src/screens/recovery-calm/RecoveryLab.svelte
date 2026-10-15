<script lang="ts">
  import RecoveryPhone from './RecoveryPhone.svelte';
  import { prefs } from '../../lib/model/prefs.svelte';
  import { motion, syncMotionAttr } from '../../lib/fx/reduced.svelte';
  import { gateSim } from '../../lib/model/actions/gate.svelte';

  const flows = [ ['restore', 'Restore account'], ['profile', 'Recovery in Profile'], ['share', 'Copy my code'] ] as const;
  let flow = $state<'restore' | 'profile' | 'share'>('share');
  let sample = $state(0);
  let revision = $state(0);
  let rtl = $state(false);
  let denyClipboard = $state(false);
  function select(next: typeof flow) { flow = next; sample = 0; revision++; }
  $effect(() => {
    if (prefs.theme === 'system') delete document.documentElement.dataset.theme;
    else document.documentElement.dataset.theme = prefs.theme;
  });
  $effect(() => syncMotionAttr());
</script>

<div class="lab">
  <header class="intro"><p class="eyebrow">Tinline interaction lab · Proposed</p><h1>A quieter way back in.</h1><p>Recovery, a private place in Profile, and a simpler code to share.</p></header>
  <nav class="flows" aria-label="Preview flow">{#each flows as [id, label]}<button type="button" data-flow={id} aria-pressed={flow === id} onclick={() => select(id)}>{label}</button>{/each}</nav>
  <div class="study">
    <div class="preview">{#key revision}<RecoveryPhone {flow} {rtl} {sample} {denyClipboard} />{/key}</div>
    <aside aria-label="Simulation controls and notes">
      <section class="controls"><h2>Try it</h2>
        <label>Theme <select aria-label="Theme" bind:value={prefs.theme}><option value="system">System</option><option value="light">Light</option><option value="dark">Dark</option></select></label>
        <label>Motion <select aria-label="Motion" bind:value={motion.pref}><option value="system">System</option><option value="reduce">Reduced</option><option value="full">Full</option></select></label>
        <label><input type="checkbox" bind:checked={rtl} /> Right to left</label>
        <label><input type="checkbox" bind:checked={gateSim.hasPassphrase} /> Account has a passphrase</label>
        <label><input type="checkbox" bind:checked={denyClipboard} /> Clipboard unavailable</label>
        {#if flow === 'restore'}<button type="button" data-use-sample onclick={() => sample++}>Use sample recovery backup</button>{/if}
        <button type="button" onclick={() => select(flow)}>Start this flow again</button>
        <p class="hint">Demo passphrase: <code>{gateSim.passphrase}</code></p>
      </section>
      <section class="notes"><h2>Less at once</h2><p>One short lead. One main action. Sharing and QR fit without an inner scrollbar at the standard preview size. Scrolling remains available for long word lists, expanded help and larger text.</p><p>The name is one user-chosen username, never a first/last-name pair. This lab uses <b>maya</b> and displays the whole value, without inventing handle syntax or uniqueness rules.</p><p><b>Contact code → share it.</b><br /><b>Recovery key → keep it private.</b><br />They never use the same card or button label.</p></section>
      <section class="notes safety"><h2>Demo only</h2><p>Don’t enter a real recovery key or passphrase here. Use the sample button. Only the shared synthetic fixtures are accepted; no real cryptographic validation or restoration runs in this browser.</p><p>Clipboard and Share use sample contact data only. “Copied” appears only after the clipboard succeeds. Browser Share uses your system chooser when supported; otherwise use Copy.</p><p>Choose image opens a simulated photo picker here. Try Contact QR, A photo, Two codes or Cancel. Android uses the real system picker and local ZXing decoding, with no Google Play Services dependency.</p></section>
      <details class="notes"><summary>Scope & open decisions</summary><p><b>Proposed:</b> a private “Account recovery” shortcut in My profile, leading to the existing recovery gate—not a public profile field. No recovery material belongs in synced profile data.</p><p>Restore already accepts keys and words in Android; recovery viewing already lives under Privacy. This study improves discoverability and copy, not the cryptographic protocol.</p><p>This is a new-phone restore preview. On the same phone, recovery can reopen existing phrase-keyed local stores; legacy unreadable files must be preserved, not silently wiped. This key is not a message backup.</p><p>Passphrase checking and screen-lock confirmation are simulations. Native re-authentication, FLAG_SECURE, clipboard policy, device naming, terms and permission steps must remain in the actual app.</p><p>Backup text hides after 30 seconds, locks when the browser loses focus or becomes hidden, and is cleared on navigation. These browser affordances do not prove native secret handling.</p><p>Contact QR and gallery thumbnails are illustrative, not scannable. Selecting the sample QR only opens a preview; Add records a simulated pending request in the shared store. No changes to code lifetime, endpoint rotation or contact acceptance are proposed here.</p><p>Android’s Choose image and contact-code Share are implemented in this worktree, not yet merged or installed. The calmer recovery/profile layouts remain proposals.</p></details>
    </aside>
  </div>
</div>

<style>
  :global(body) { overflow: auto; }
  .lab { max-inline-size: 1040px; margin-inline: auto; padding: 32px; }
  .eyebrow { color: var(--accent-ink); font-size: 13px; margin: 0 0 10px; font-weight: 600; }
  h1 { font-size: clamp(28px, 4vw, 38px); letter-spacing: -.04em; font-weight: 600; margin: 0; }
  .intro > p:last-child { color: var(--muted); line-height: 1.5; }
  .flows { display: flex; flex-wrap: wrap; gap: 8px; margin-block: 24px 32px; }
  button, select { min-block-size: 44px; border: 1px solid var(--line); border-radius: 12px; background: var(--surface); color: var(--ink); padding-inline: 14px; font: inherit; cursor: pointer; }
  button[aria-pressed='true'] { background: var(--accent-soft); border-color: var(--accent); color: var(--accent-ink); }
  .study { display: grid; grid-template-columns: minmax(0, 390px) minmax(0, 360px); gap: 64px; align-items: start; }
  .preview { min-inline-size: 0; }
  aside { min-inline-size: 0; }
  h2 { font-size: 16px; font-weight: 600; margin: 0 0 14px; }
  .controls { display: grid; gap: 8px; }
  label { min-block-size: 44px; display: flex; align-items: center; gap: 8px; font-size: 14px; }
  label select { margin-inline-start: auto; }
  input { accent-color: var(--accent); inline-size: 18px; block-size: 18px; }
  .hint, .notes { color: var(--muted); font-size: 13px; line-height: 1.7; }
  .hint { margin: 8px 0 0; }
  .notes { margin-block-start: 28px; padding-block-start: 20px; border-block-start: 1px solid var(--line); }
  .notes h2, .notes b { color: var(--ink); }
  .notes p { margin-block: 10px 0; }
  summary { min-block-size: 44px; cursor: pointer; align-content: center; }
  @media (max-width: 800px) { .study { grid-template-columns: minmax(0, 390px); justify-content: center; gap: 32px; } .lab { padding: 24px; } }
  @media (max-width: 440px) { .lab { padding: 16px; } .flows { gap: 6px; } .flows button { font-size: 13px; padding-inline: 10px; } }
</style>

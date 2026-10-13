<script lang="ts">
  import Icon from '../../lib/ui/Icon.svelte';
  import StepShell from './StepShell.svelte';
  import { gate } from '../../lib/model/actions/gate.svelte';
</script>

<StepShell title="Use your account here" lead="Your account is its recovery phrase, so you can bring it to this {gate.noun} two ways.">
  <div class="opts">
    <button type="button" class="opt press" onclick={() => gate.startLink()}>
      <span class="ic"><Icon name="qr" size={24} /></span>
      <span class="tx"><b>Link from another device</b><small>Scan a QR code with a device that already has your account. Contacts and chats come along.</small></span>
      <Icon name="chev" size={20} class="go" />
    </button>
    <button type="button" class="opt press" onclick={() => gate.startRestore()}>
      <span class="ic"><Icon name="key" size={24} /></span>
      <span class="tx"><b>Restore from recovery phrase</b><small>Type your 24 words. This {gate.noun} starts with an empty contact list.</small></span>
      <Icon name="chev" size={20} class="go" />
    </button>
  </div>
</StepShell>

<style>
  .opts { display: grid; gap: 12px; }
  .opt { display: flex; align-items: center; gap: 14px; padding: 16px; border: 1px solid var(--line); border-radius: var(--r-ctl); background: var(--surface); text-align: start; cursor: pointer; min-block-size: 76px; }
  .opt:hover { background: var(--surface-2); }
  .ic { display: grid; place-items: center; inline-size: 44px; block-size: 44px; border-radius: 50%; background: var(--accent-soft); color: var(--accent-ink); flex: none; }
  .tx { display: grid; gap: 2px; flex: 1; min-inline-size: 0; }
  .tx small { color: var(--muted); font-size: var(--fs-sub); line-height: 18px; }
  .opt :global(.go) { transform: rotate(-90deg); color: var(--muted); }
  :global([dir='rtl']) .opt :global(.go) { transform: rotate(90deg); }
</style>

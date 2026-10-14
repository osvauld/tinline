<script lang="ts">
  import Icon from '../../lib/ui/Icon.svelte';
  import Btn from './Btn.svelte';
  import StepShell from './StepShell.svelte';
  import { gate } from '../../lib/model/actions/gate.svelte';

  type Row = { k: string; icon: string; title: string; why: string; optional?: boolean };
  const rows: Row[] = $derived(gate.isAndroid ? [
    { k: 'mic', icon: 'mic', title: 'Microphone', why: 'So they can hear you. Used only during a call.' },
    { k: 'notif', icon: 'bell', title: 'Notifications', why: 'So you see incoming calls and messages.' },
    { k: 'lock', icon: 'phone', title: 'Ring over lock screen', why: 'Shows incoming calls full screen, like the phone app.' },
    { k: 'battery', icon: 'battery', title: 'Run in background', why: 'Android pauses idle apps to save battery. This keeps your line open, using a little battery.' },
    { k: 'camera', icon: 'camera', title: 'Camera', why: 'Only for video calls. You choose when it is on.', optional: true },
  ] : [
    { k: 'notif', icon: 'bell', title: 'Notifications', why: 'So you see incoming calls and messages.' },
    { k: 'mic', icon: 'mic', title: 'Microphone', why: 'So they can hear you. Used only during a call.' },
    { k: 'camera', icon: 'camera', title: 'Camera', why: 'Only for video calls. You choose when it is on.', optional: true },
  ]);
  const touched = $derived(rows.some((r) => gate.perms[r.k] !== 'idle'));
</script>

<StepShell title="So calls can reach you" lead="Tinline works like a phone line: it stays open quietly in the background. Each of these is optional, and you can change them later.">
  <ul>
    {#each rows as r (r.k)}
      {@const st = gate.perms[r.k]}
      <li>
        <span class="ic"><Icon name={r.icon} size={20} /></span>
        <span class="tx"><b>{r.title}{#if r.optional} <em>Optional</em>{/if}</b><small>{r.why}</small>
          {#if st === 'denied'}<small class="den">Off for now. You can turn it on in system settings.</small>{/if}</span>
        {#if st === 'granted'}<span class="ok" role="img" aria-label="{r.title} allowed"><Icon name="check" size={22} /></span>
        {:else}<button type="button" class="allow press" aria-label="Allow {r.title.toLowerCase()}" onclick={() => gate.allow(r.k)}>{st === 'denied' ? 'Retry' : 'Allow'}</button>{/if}
      </li>
    {/each}
  </ul>
  {#snippet foot()}
    <Btn enter kind={touched ? 'primary' : 'secondary'} onclick={() => gate.go('done')}>{touched ? 'Continue' : 'Not now'}</Btn>
  {/snippet}
</StepShell>

<style>
  ul { list-style: none; margin: 0; padding: 0; display: grid; gap: 4px; }
  li { display: flex; gap: 14px; align-items: center; padding-block: 8px; }
  .ic { display: grid; place-items: center; inline-size: 40px; block-size: 40px; border-radius: 50%; background: var(--accent-soft); color: var(--accent-ink); flex: none; }
  .tx { display: grid; gap: 1px; flex: 1; min-inline-size: 0; }
  .tx small { color: var(--muted); font-size: var(--fs-sub); line-height: 18px; }
  .tx .den { color: var(--muted); }
  em { font-style: normal; font-weight: 600; font-size: var(--fs-meta); color: var(--muted); border: 1px solid var(--line); border-radius: 8px; padding: 1px 6px; margin-inline-start: 4px; }
  .allow { min-inline-size: 72px; min-block-size: 44px; border-radius: 22px; border: 1px solid var(--line); background: transparent; color: var(--accent-ink); font-weight: 650; cursor: pointer; flex: none; }
  .allow:hover { background: var(--surface-2); }
  .ok { display: grid; place-items: center; inline-size: 72px; color: var(--accent-ink); flex: none; }
</style>

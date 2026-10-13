<script lang="ts">
  /* Settings > Notifications: what Tinline may alert me about, and the OS permissions it depends on. */
  import Switch from '../../lib/ui/Switch.svelte';
  import Icon from '../../lib/ui/Icon.svelte';
  import Btn from '../groups/Btn.svelte';
  import PermBanner from './PermBanner.svelte';
  import { fix, problems } from './perms';
  import { pp } from '../../lib/model/actions/people.svelte';
  import { prefs } from '../../lib/model/prefs.svelte';

  const probs = $derived(problems());
  const off = $derived(pp.perms.notifications === 'denied');
  const PERMS = $derived([
    { key: 'notifications' as const, icon: 'bell', label: 'Notifications', ok: pp.perms.notifications === 'granted', yes: 'Allowed', no: 'Off in system settings' },
    { key: 'microphone' as const, icon: 'mic', label: 'Microphone', ok: pp.perms.microphone === 'granted', yes: 'Allowed', no: 'Off in system settings' },
    ...(prefs.form === 'android' ? [{ key: 'battery' as const, icon: 'battery', label: 'Battery use', ok: pp.perms.battery === 'ok', yes: 'Unrestricted', no: 'Restricted' }] : []),
  ]);
</script>

<div class="pf-scroll" data-notifications-page>
<div class="pf-col">
  <div class="pf-sec first banners" aria-label="Things to fix">
    {#each probs as p (p.key)}<PermBanner {p} />{/each}
  </div>

  <section class="pf-sec top" aria-labelledby="nt-h">
    <h2 id="nt-h">Alert me about</h2>
    <div class="pf-card" class:off inert={off}>
      <label class="pf-act row-sw"><Icon name="chat" size={20} /><span class="t"><b>Messages</b><small>From people, not groups</small></span><Switch label="Message notifications" bind:checked={pp.notif.messages} /></label>
      <label class="pf-act row-sw"><Icon name="phone" size={20} /><span class="t"><b>Calls</b><small>Ringing when someone calls</small></span><Switch label="Call notifications" bind:checked={pp.notif.calls} /></label>
      <label class="pf-act row-sw"><Icon name="users" size={20} /><span class="t"><b>Groups</b><small>Posts, invites and requests</small></span><Switch label="Group notifications" bind:checked={pp.notif.groups} /></label>
    </div>
    <p class="pf-note">A muted chat never alerts you, whatever is switched on here. Missed calls from one person fold into one notification.</p>
  </section>

  <section class="pf-sec" aria-labelledby="pm-h">
    <h2 id="pm-h">Permissions</h2>
    <div class="pf-card">
      {#each PERMS as r (r.key)}
        <div class="pf-act perm" data-perm={r.key}>
          <Icon name={r.icon} size={20} /><span class="t"><b>{r.label}</b><small class:bad={!r.ok}>{r.ok ? r.yes : r.no}</small></span>
          <span class="end">{#if !r.ok}<Btn variant="outline" data-fix-row={r.key} onclick={() => fix(r.key)}>Fix</Btn>{/if}</span>
        </div>
      {/each}
    </div>
    <p class="pf-note">These are set by your system, not by Tinline. Tinline asks only when it needs one.</p>
  </section>
</div>
</div>

<style>
  .first { margin-block-start: 16px; }
  .banners { padding-block-end: 0; }
  .banners:empty { display: none; }
  .top { margin-block-start: 12px; }
  .row-sw { cursor: default; }
  .off { opacity: .55; }
  .perm { cursor: default; }
  .perm small.bad { color: var(--warn-ink); font-weight: 600; }
  :global(:root[data-theme='dark']) .perm small.bad { color: var(--gold-ink); }
  .end { flex: none; min-inline-size: 64px; display: inline-flex; justify-content: flex-end; }
  .end :global(.b) { min-block-size: 36px; padding: 0 14px; }
</style>

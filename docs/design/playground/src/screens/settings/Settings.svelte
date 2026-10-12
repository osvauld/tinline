<script lang="ts">
  /* Settings landing (route `settings`). Minimal on purpose: my profile card, then plain rows for the areas step 8 fills in.
     `section` is set for the areas that don't have their own route yet (they show the placeholder). */
  import '../profile/profile.css';
  import Avatar from '../../lib/ui/Avatar.svelte';
  import Icon from '../../lib/ui/Icon.svelte';
  import IconButton from '../../lib/ui/IconButton.svelte';
  import Notifications from './Notifications.svelte';
  import Sounds from './Sounds.svelte';
  import PermBanner from './PermBanner.svelte';
  import { problems } from './perms';
  import { db } from '../../lib/model/store.svelte';
  import { clock } from '../../lib/model/clock.svelte';
  import { nav } from '../../lib/nav/router.svelte';
  import { effectiveAvailability } from '../../lib/model/actions/profile';
  import { profileOf } from '../../lib/model/actions/profile.svelte';
  import { untilLabel } from '../profile/info';

  let { section }: { section?: string } = $props();

  const status = $derived(profileOf('me')?.fields.status);
  const av = $derived(effectiveAvailability('me', clock.now));
  const avText = $derived(av.available ? 'Available' : av.until === undefined ? 'Busy' : `Busy until ${untilLabel(av.until)}`);
  const AREAS = [
    { icon: 'devices', label: 'Devices', go: () => nav.push({ name: 'devices' }) },
    { icon: 'shield', label: 'Privacy', go: () => nav.push({ name: 'privacy' }) },
    { icon: 'bell', label: 'Notifications', go: () => nav.push({ name: 'settings', section: 'notifications' }) },
    { icon: 'vol', label: 'Sounds', go: () => nav.push({ name: 'settings', section: 'sounds' }) },
  ];
  const title = $derived(section ? section[0].toUpperCase() + section.slice(1) : 'Settings');
</script>

<div class="pf" data-settings>
  <header class="pf-bar"><IconButton icon="back" label="Back" onclick={() => nav.back()} /><b>{title}</b></header>
  {#if section}
    {#if section === 'notifications'}<Notifications />{:else if section === 'sounds'}<Sounds />{/if}
  {:else}
  <div class="pf-scroll">
  <div class="pf-col">
    <!-- Permission problems: said once here, with the details on the Notifications page. -->
    <div class="pf-sec" data-settings-banners>{#each problems().slice(0, 1) as p (p.key)}<PermBanner {p} onopen={() => nav.push({ name: 'settings', section: 'notifications' })} />{/each}</div>
    <section class="pf-sec first" aria-label="Your profile">
      <div class="pf-card">
        <button type="button" class="pf-act me press" data-my-profile onclick={() => nav.push({ name: 'profile' })}>
          <Avatar name={db.account.name} hue={db.account.hue} person="me" size={64} />
          <span class="t"><b>{db.account.name}</b>
            {#if typeof status === 'string' && status}<small>{status}</small>{/if}
            <small class="avl" class:busy={!av.available}>{avText}</small></span>
          <Icon name="chevr" size={18} class="chev" />
        </button>
      </div>
    </section>

    <section class="pf-sec" aria-label="Settings areas">
      <div class="pf-card">
        {#each AREAS as a (a.label)}
          <button type="button" class="pf-act press" onclick={a.go}><Icon name={a.icon} size={20} /><span class="t"><b>{a.label}</b></span><Icon name="chevr" size={18} class="chev" /></button>
        {/each}
      </div>
    </section>
  </div>
  </div>
  {/if}
</div>

<style>
  .first { margin-block-start: 16px; }
  .me { min-block-size: 88px; gap: 16px; padding-block: 12px; }
  .me .t b { font-size: var(--fs-title); line-height: 24px; }
  .avl { font-weight: 600; color: var(--muted); font-variant-numeric: tabular-nums; }
  .avl.busy { color: var(--warn-ink); }
  [data-settings-banners]:empty { display: none; }
</style>

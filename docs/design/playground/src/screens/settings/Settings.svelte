<script lang="ts">
  /* Settings landing (route `settings`). Minimal on purpose: my profile card, then plain rows for the areas step 8 fills in.
     `section` is set for the areas that don't have their own route yet (they show the placeholder). */
  import '../profile/profile.css';
  import Avatar from '../../lib/ui/Avatar.svelte';
  import Icon from '../../lib/ui/Icon.svelte';
  import IconButton from '../../lib/ui/IconButton.svelte';
  import Notifications from './Notifications.svelte';
  import Sounds from './Sounds.svelte';
  import CallsVideo from './CallsVideo.svelte';
  import Storage from './Storage.svelte';
  import About from './About.svelte';
  import PermBanner from './PermBanner.svelte';
  import { problems } from './perms';
  import { db } from '../../lib/model/store.svelte';
  import { clock } from '../../lib/model/clock.svelte';
  import { nav } from '../../lib/nav/router.svelte';
  import { effectiveAvailability } from '../../lib/model/actions/profile';
  import { profileOf } from '../../lib/model/actions/profile.svelte';
  import { untilLabel } from '../profile/info';
  import { gate } from '../../lib/model/actions/gate.svelte';
  import Names from './Names.svelte';
  import { prefs } from '../../lib/model/prefs.svelte';

  let { section }: { section?: string } = $props();

  const status = $derived(profileOf('me')?.fields.status);
  const av = $derived(effectiveAvailability('me', clock.now));
  const avText = $derived(av.available ? 'Available' : av.until === undefined ? 'Busy' : `Busy until ${untilLabel(av.until)}`);
  /* Appearance (2026-10-14): the same choice the sign-up switch makes, kept here for later. "Match my device" follows the
     phone or computer; it switches at once and nothing moves. */
  const LOOKS = [
    { k: 'system', label: `Match my ${gate.noun}`, sub: `Light or dark, as this ${gate.noun} is set` },
    { k: 'light', label: 'Light', sub: '' },
    { k: 'dark', label: 'Dark', sub: '' },
  ] as const;
  const AREAS = [
    { icon: 'devices', label: 'Devices', go: () => nav.push({ name: 'devices' }) },
    { icon: 'shield', label: 'Privacy', go: () => nav.push({ name: 'privacy' }) },
    { icon: 'bell', label: 'Notifications', go: () => nav.push({ name: 'settings', section: 'notifications' }) },
    { icon: 'vol', label: 'Sounds', go: () => nav.push({ name: 'settings', section: 'sounds' }) },
    { icon: 'video', label: 'Video', go: () => nav.push({ name: 'settings', section: 'video' }) },
    { icon: 'save', label: 'Storage', go: () => nav.push({ name: 'settings', section: 'storage' }) },
    { icon: 'info', label: 'About Tinline', go: () => nav.push({ name: 'settings', section: 'about' }) },
  ];
  const TITLES: Record<string, string> = { notifications: 'Notifications', sounds: 'Sounds', video: 'Video', storage: 'Storage', about: 'About Tinline' };
  const title = $derived(section ? TITLES[section] ?? section : 'Settings');
</script>

<div class="pf" data-settings>
  <header class="pf-bar"><IconButton icon="back" label="Back" onclick={() => nav.back()} /><b>{title}</b></header>
  {#if section}
    {#if section === 'notifications'}<Notifications />{:else if section === 'sounds'}<Sounds />{:else if section === 'video'}<CallsVideo />{:else if section === 'storage'}<Storage />{:else if section === 'about'}<About />{/if}
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

    <Names />

    <section class="pf-sec" aria-labelledby="ap-h">
      <h2 id="ap-h">Appearance</h2>
      <div class="pf-card" role="radiogroup" aria-label="Appearance">
        {#each LOOKS as l (l.k)}
          <div class="pf-act opt">
            <button type="button" role="radio" class="pick press" aria-checked={prefs.theme === l.k} onclick={() => (prefs.theme = l.k)}>
              <span class="radio" aria-hidden="true"></span>
              <span class="t"><b>{l.label}</b>{#if l.sub}<small>{l.sub}</small>{/if}</span>
            </button>
          </div>
        {/each}
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
  /* Appearance radios: the Sounds page's rows (one look for every single choice in Settings). */
  .opt { padding: 0; }
  .pick { display: flex; align-items: center; gap: 14px; inline-size: 100%; min-block-size: 56px; padding: 8px 16px; border: 0; background: none; color: inherit; font: inherit; text-align: start; cursor: pointer; }
  .pick:hover { background: var(--surface-2); }
  .radio { flex: none; inline-size: 22px; block-size: 22px; border-radius: 50%; box-shadow: inset 0 0 0 2px var(--line); display: grid; place-items: center; transition: box-shadow var(--dur-switch) var(--ease-out); }
  .radio::after { content: ''; inline-size: 12px; block-size: 12px; border-radius: 50%; background: var(--accent); transform: scale(0); transition: transform var(--dur-switch) var(--ease-out); }
  [aria-checked='true'] .radio { box-shadow: inset 0 0 0 2px var(--accent); }
  [aria-checked='true'] .radio::after { transform: scale(1); }
  .me { min-block-size: 88px; gap: 16px; padding-block: 12px; }
  .me .t b { font-size: var(--fs-title); line-height: 24px; }
  .avl { font-weight: 600; color: var(--muted); font-variant-numeric: tabular-nums; }
  .avl.busy { color: var(--warn-ink); }
  [data-settings-banners]:empty { display: none; }
</style>

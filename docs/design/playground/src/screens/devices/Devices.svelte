<script lang="ts">
  /* Devices (route `devices`): this device, the devices linked to my account, and linking a new one. Linking is the
     EXISTING device's side: the new device shows the QR and code (decided model), this one approves. Unlinking is honest:
     it stops syncing and ringing, and it is not a lock (anyone with the recovery phrase can still act as me). */
  import '../profile/profile.css';
  import { fade } from 'svelte/transition';
  import Icon from '../../lib/ui/Icon.svelte';
  import IconButton from '../../lib/ui/IconButton.svelte';
  import Layer from '../chat/Layer.svelte';
  import Btn from '../groups/Btn.svelte';
  import PassField from '../people/PassField.svelte';
  import LinkDevice from './LinkDevice.svelte';
  import { db } from '../../lib/model/store.svelte';
  import { clock, T } from '../../lib/model/clock.svelte';
  import { nav } from '../../lib/nav/router.svelte';
  import { gateSim } from '../../lib/model/actions/gate.svelte';
  import { isStale, lastSeenText, renameDevice, unlinkDevice, checkDeviceName } from '../../lib/model/actions/people.svelte';

  let { section }: { section?: string } = $props();

  const me = $derived(db.devices.find((d) => d.self));
  const others = $derived(db.devices.filter((d) => !d.self));
  const online = (at: number) => clock.now - at < 2 * T.MIN;
  const sub = (d: { lastSeen: number; self: boolean }) => (d.self ? 'This device · Online' : isStale(d.lastSeen) ? `Not seen for ${Math.round((clock.now - d.lastSeen) / T.DAY)} days` : online(d.lastSeen) ? 'Online now' : lastSeenText(d.lastSeen, false));

  /* One layer at a time: a device's actions, renaming it, unlinking it. */
  let layer = $state<null | { id: string; mode: 'menu' | 'rename' | 'unlink' }>(null);
  const dev = $derived(layer ? db.devices.find((d) => d.id === layer!.id) : undefined);
  let name = $state(''), nameErr = $state(''), pass = $state(''), passErr = $state('');
  let opener: HTMLElement | null = null;
  function show(id: string, mode: 'menu' | 'rename' | 'unlink', e?: Event) {
    if (e) opener = e.currentTarget as HTMLElement;
    layer = { id, mode }; nameErr = ''; passErr = ''; pass = '';
    name = db.devices.find((d) => d.id === id)?.name ?? '';
  }
  function close() { layer = null; queueMicrotask(() => opener?.isConnected && opener.focus({ preventScroll: true })); }
  function saveName() {
    if (!layer) return;
    const r = renameDevice(layer.id, name);
    if (!r.ok) { nameErr = r.error; return; }
    announce = `Renamed to ${r.name}.`; close();
  }
  function doUnlink() {
    if (!layer || !dev) return;
    if (gateSim.hasPassphrase && pass !== gateSim.passphrase) { passErr = 'That passphrase isn’t right.'; return; }
    const n = dev.name; unlinkDevice(layer.id); announce = `${n} was unlinked.`; layer = null;
    queueMicrotask(() => document.querySelector<HTMLElement>('[data-link-new]')?.focus({ preventScroll: true }));
  }
  let announce = $state('');
  const nameOk = $derived(layer?.mode === 'rename' ? checkDeviceName(name, layer.id).ok : true);
  const title = $derived(section === 'link' ? 'Link a new device' : 'Devices');
</script>

<div class="pf" data-devices>
  <header class="pf-bar"><IconButton icon="back" label="Back" onclick={() => nav.back()} /><b>{title}</b></header>
  {#if section === 'link'}
    <LinkDevice />
  {:else}
  <div class="pf-scroll" inert={!!layer}>
  <div class="pf-col">
    <p class="vh" role="status" aria-live="polite">{announce}</p>
    {#if me}
    <section class="pf-sec first" aria-labelledby="me-h">
      <h2 id="me-h">This device</h2>
      <div class="pf-card">
        <button type="button" class="pf-act press" data-device={me.id} onclick={(e) => show(me.id, 'menu', e)}>
          <Icon name={me.kind === 'phone' ? 'smartphone' : 'laptop'} size={22} />
          <span class="t"><b>{me.name}</b><small>{sub(me)}</small></span><Icon name="chevr" size={18} class="chev" />
        </button>
      </div>
    </section>
    {/if}

    <section class="pf-sec" aria-labelledby="oth-h">
      <h2 id="oth-h">Linked devices</h2>
      <div class="pf-card">
        {#each others as d (d.id)}
          <button type="button" class="pf-act press" data-device={d.id} onclick={(e) => show(d.id, 'menu', e)} transition:fade={{ duration: 120 }}>
            <Icon name={d.kind === 'phone' ? 'smartphone' : 'laptop'} size={22} />
            <span class="t"><b>{d.name}</b><small class:stale={isStale(d.lastSeen)} class:live={online(d.lastSeen)}>{sub(d)}</small></span><Icon name="chevr" size={18} class="chev" />
          </button>
        {/each}
        <button type="button" class="pf-act press" data-link-new onclick={() => nav.push({ name: 'devices', section: 'link' })}>
          <Icon name="plus" size={22} /><span class="t"><b>Link a new device</b><small>Show a QR code on the new device, then approve it here</small></span><Icon name="chevr" size={18} class="chev" />
        </button>
      </div>
      <p class="pf-note" data-devices-note>
        {#if !others.length}Only this device has your account. Link another to keep a second copy of your chats. Your recovery phrase brings back your account, not your messages.
        {:else}Every linked device is a full copy of you: it can read your chats and act as you. Names here are private to your devices; contacts never see them.{/if}
      </p>
    </section>
  </div>
  </div>
  {/if}

  {#if layer && dev}
    <Layer variant="dialog" label={layer.mode === 'rename' ? 'Rename device' : layer.mode === 'unlink' ? `Unlink ${dev.name}` : dev.name} onclose={close}>
      {#if layer.mode === 'menu'}
        <div class="pf-dlg" data-device-menu>
          <h2>{dev.name}</h2>
          <p>{dev.self ? 'This device' : sub(dev)}</p>
          <div class="stack">
            <button type="button" class="pf-act press" data-autofocus onclick={() => show(dev.id, 'rename')}><Icon name="edit" size={20} /><span class="t"><b>Rename</b></span></button>
            {#if !dev.self}<button type="button" class="pf-act press danger" onclick={() => show(dev.id, 'unlink')}><Icon name="leave" size={20} /><span class="t"><b>Unlink</b></span></button>{/if}
          </div>
          <div class="pf-acts"><Btn variant="quiet" onclick={close}>Close</Btn></div>
        </div>
      {:else if layer.mode === 'rename'}
        <form class="pf-dlg" data-rename onsubmit={(e) => { e.preventDefault(); saveName(); }}>
          <h2>Rename device</h2>
          <p>Only your own devices see this name.</p>
          <div class="fld"><label for="dn">Name</label>
            <input id="dn" data-autofocus type="text" maxlength="40" bind:value={name} oninput={() => (nameErr = '')} aria-invalid={nameErr ? 'true' : undefined} aria-describedby="dn-m" autocomplete="off" />
            <p class="m err" id="dn-m" role="alert">{nameErr}</p></div>
          <div class="pf-acts"><Btn variant="quiet" onclick={close}>Cancel</Btn><Btn variant="primary" type="submit" disabled={!nameOk}>Save</Btn></div>
        </form>
      {:else}
        <form class="pf-dlg" data-unlink onsubmit={(e) => { e.preventDefault(); doUnlink(); }}>
          <h2>Unlink {dev.name}?</h2>
          <p class="lead">{dev.name} will stop syncing with your account and stop ringing when you’re called. {online(dev.lastSeen) ? 'We’ll tell it now.' : 'It is offline, so it finds out the next time it connects.'}</p>
          <p class="notlock" data-not-a-lock><Icon name="warning" size={18} /><span><b>This is not a lock.</b> Anyone who still has your recovery phrase, or can unlock {dev.name}, can still act as you. If it is lost or stolen, the only full fix is a new account.</span></p>
          <p>Contacts you haven’t blocked get your updated device list when they’re next online.</p>
          {#if gateSim.hasPassphrase}<PassField id="un-pass" bind:value={pass} error={passErr} autofocus label="Enter your passphrase to confirm" oninput={() => (passErr = '')} />{/if}
          <div class="pf-acts"><Btn variant="quiet" data-autofocus={gateSim.hasPassphrase ? undefined : ''} onclick={close}>Cancel</Btn><Btn variant="danger" type="submit">Unlink</Btn></div>
        </form>
      {/if}
    </Layer>
  {/if}
</div>

<style>
  .first { margin-block-start: 16px; }
  .pf-act small.stale { color: var(--warn-ink); font-weight: 600; }
  :global(:root[data-theme='dark']) .pf-act small.stale { color: var(--gold-ink); }
  .pf-act small.live { color: var(--accent-ink); font-weight: 600; }
  .stack { margin: 12px -20px 0; border-block-start: 1px solid var(--hairline); }
  .stack > * + * { border-block-start: 1px solid var(--hairline); }
  .fld { display: grid; gap: 6px; margin-block-start: 12px; }
  .fld label { font-weight: 600; font-size: var(--fs-sub); }
  .fld input { min-block-size: var(--btn); padding: 0 14px; border-radius: var(--r-ctl); border: 1px solid var(--line); background: var(--surface); font: inherit; font-size: var(--fs-body); color: var(--ink); }
  .fld input[aria-invalid='true'] { border-color: var(--danger); }
  .m { margin: 0; min-block-size: 19px; }
  .err { color: var(--danger) !important; }
  .notlock { display: flex; gap: 10px; align-items: flex-start; margin: 12px 0 0 !important; padding: 10px 12px; border-radius: 12px; background: var(--warn-bg); color: var(--warn-ink) !important; }
  .notlock > :global(svg) { flex: none; margin-block-start: 1px; }
  .notlock b { font-weight: 650; }
  .pf-dlg :global(.pw) { margin-block-start: 12px; }
</style>

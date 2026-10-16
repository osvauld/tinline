<script lang="ts">
  /* People and devices simulation: requests arriving, devices linking and dropping off, the new device's behaviour while I link
     it, and the OS taking permissions away. Lives only in the drawer; the app frame never mentions any of it. */
  import { db } from '../model/store.svelte';
  import { clock, T } from '../model/clock.svelte';
  import { NEW_DEVICE_CODE, MY_CODE, DIRECTORY, incomingRequest, link, linkSim, otherDeviceUnlinks, pp, setLastSeen } from '../model/actions/people.svelte';
  import { newId } from '../model/store.svelte';

  const others = $derived(db.devices.filter((d) => !d.self));
  const laptop = $derived(db.devices.find((d) => d.id === 'laptop'));
  const linkedElsewhere = () => db.devices.push({ id: newId('dev'), name: 'Home PC', kind: 'desktop', self: false, lastSeen: clock.now });
  const waker = () => laptop && setLastSeen(laptop.id, 0);
</script>

<div class="rows">
  <h4>Contacts</h4>
  <div class="btns">
    <button type="button" onclick={() => incomingRequest()}>Someone scanned my code</button>
    <button type="button" onclick={() => (pp.requests = [])} disabled={!pp.requests.length}>Clear requests ({pp.requests.length})</button>
  </div>
  <div class="sub">Codes are long <code>OSVC2:…</code> tickets. Copy a sample to paste:
    {#each DIRECTORY as d (d.id)}<button type="button" onclick={() => navigator.clipboard?.writeText('Add me on Tinline: ' + d.card)}>{d.name}</button>{/each}
    (any other OSVC2: code picks one; mine is {MY_CODE.slice(0, 14)}…)</div>

  <h4>Devices</h4>
  <div class="btns">
    <button type="button" onclick={linkedElsewhere}>Another device links from elsewhere</button>
    {#each others as d (d.id)}<button type="button" onclick={() => otherDeviceUnlinks(d.id)}>{d.name} unlinks itself</button>{/each}
    <button type="button" disabled={!laptop} onclick={() => laptop && setLastSeen(laptop.id, 41 * T.DAY)}>Work laptop not seen for 41 days</button>
    <button type="button" disabled={!laptop} onclick={waker}>Work laptop is online now</button>
  </div>

  <h4>Linking a new device (I approve)</h4>
  <div class="sub">The new device shows the code <code>{NEW_DEVICE_CODE}</code> and the digits <code>{link.confirmCode}</code>. It is named:</div>
  <label class="row"><span>Name</span><input type="text" bind:value={linkSim.name} aria-label="New device name" /></label>
  <label class="row"><span>Kind</span>
    <select bind:value={linkSim.kind} aria-label="New device kind"><option value="phone">Phone / tablet</option><option value="desktop">Computer</option></select></label>
  <label class="row"><span>What goes wrong</span>
    <select bind:value={linkSim.failure} aria-label="Link failure">
      <option value="none">Nothing (happy path)</option><option value="expired">The code has expired</option>
      <option value="declined">It declines when I say the digits match</option><option value="dropped">The connection drops mid-transfer</option>
    </select></label>

  <h4>Permissions the OS gave</h4>
  <div class="btns">
    <button type="button" onclick={() => (pp.perms.notifications = pp.perms.notifications === 'granted' ? 'denied' : 'granted')}>Notifications: {pp.perms.notifications}</button>
    <button type="button" onclick={() => (pp.perms.microphone = pp.perms.microphone === 'granted' ? 'denied' : 'granted')}>Microphone: {pp.perms.microphone}</button>
    <button type="button" onclick={() => (pp.perms.camera = pp.perms.camera === 'granted' ? 'denied' : 'granted')}>Camera: {pp.perms.camera}</button>
    <button type="button" onclick={() => (pp.perms.battery = pp.perms.battery === 'ok' ? 'restricted' : 'ok')}>Battery (Android): {pp.perms.battery === 'ok' ? 'unrestricted' : 'restricted'}</button>
  </div>
  <div class="sub">In the app, “Open settings” / “Fix” stands for the person changing it in system settings and coming back: it turns the permission on.</div>

  <div class="notes">
    <p><b>Decided</b> (HANDOFF U1-U24, 2026-10-12 log).</p>
    <ul>
      <li>A block is visible: the person is told, their app stops contacting me and requests removal of my contact entry; chats are never deleted (U5, U19, U20, U24). Blocking changes nothing in groups and never rotates an address.</li>
      <li>Block replaces “Remove contact” for contacts; Remove exists only for group members (U19).</li>
      <li>Linking a device moves the recovery phrase, so every linked device is a full peer. Unlinking is not revocation: a phrase holder can still act as me. Only a new identity cuts off fully (device-linking.md, multi-device.md).</li>
      <li>The device list is pushed to unblocked contacts when it changes (U8; HANDOFF says the push exists in the core).</li>
    </ul>
    <p><b>Proposed, not decided.</b></p>
    <ul>
      <li>Verification wording on Add contact: “Scanned from their screen” vs “Not confirmed in person”. The core has no verified flag yet (HANDOFF open item: verified flag after a pushed device list).</li>
      <li>“Someone scanned my code” request cards (accept or Not now). Whether a decline is told to the other person is undecided; the app says nothing about it.</li>
      <li>A default for “New contacts can call me” (off). The core only has the per-contact Allow calls.</li>
      <li>Unblock copy: you both add each other again before messaging. Follows U20 (their app deleted my contact entry); the exact re-add flow is not designed.</li>
      <li>Read receipts: a core account setting, default on (no reciprocity, no backdating when turned back on). Online visibility is not offered: the core has none.</li>
      <li>Device names, six-digit confirmation, 5-minute code, “not seen for 30+ days” nudge, passphrase to unlink and to approve (devices study P9, E4, E5).</li>
      <li>Recovery phrase view: behind the passphrase, hidden by default, hides after 30 s or when the app leaves the foreground, no copy button.</li>
      <li>Passphrase is per device. Changing it here does not change other devices&rsquo;.</li>
      <li>Call sound choice and Quiet as real settings (the palette itself is still the sound-identity candidates). Notification switches for messages, calls and groups; the OS-permission copy and the “Open settings” hand-off.</li>
      <li>Playground only: the camera, QR and system settings are simulated; the 6 digits are fixed.</li>
    </ul>
  </div>
</div>

<style>
  .rows { display: grid; gap: 4px; }
  .row { display: flex; align-items: center; justify-content: space-between; gap: 12px; min-height: 40px; font-size: var(--fs-sub); }
  h4 { margin: 12px 0 2px; font-size: var(--fs-sub); color: var(--muted); font-weight: 650; }
  .sub { font-size: var(--fs-sub); color: var(--muted); margin-top: 4px; }
  code { font: 12px ui-monospace, Menlo, monospace; color: var(--ink); }
  .btns { display: flex; flex-wrap: wrap; gap: 6px; margin-top: 4px; }
  button:not([role]) { border: 1px solid var(--line); background: var(--surface); border-radius: 10px; padding: 6px 10px; font-size: var(--fs-sub); cursor: pointer; }
  button:disabled { opacity: .45; cursor: default; }
  select, input { border: 1px solid var(--line); background: var(--surface); border-radius: 10px; padding: 6px 8px; font-size: var(--fs-sub); max-width: 60%; color: var(--ink); }
  .notes { margin-top: 12px; font-size: var(--fs-sub); color: var(--muted); }
  .notes p { margin: 0 0 4px; }
  .notes ul { margin: 0 0 8px; padding-inline-start: 18px; display: grid; gap: 3px; }
</style>

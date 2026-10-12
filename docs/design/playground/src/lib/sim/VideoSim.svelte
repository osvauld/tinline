<script lang="ts">
  /* Video simulation: their camera, the network, and what this device allows. Drawer only. */
  import { call } from '../calls/call.svelte';
  import { vid } from '../calls/video/video.svelte';
  import { prefs } from '../model/prefs.svelte';
  import Switch from '../ui/Switch.svelte';

  import { db } from '../model/store.svelte';
  let from = $state('arjun');
  const callers = $derived(db.people.filter((p) => p.id !== 'me'));
  const android = $derived(prefs.form === 'android');
  const live = $derived(call.phase === 'active' || call.phase === 'reconnecting');
  const realOk = typeof navigator !== 'undefined' && !!navigator.mediaDevices?.getUserMedia;
</script>

<div class="rows">
  <p class="state">Now: <b>{vid.videoCall ? 'video call' : 'voice'}</b> · my camera <b>{vid.camOn ? 'on (' + vid.facing + ')' : 'off'}</b> · theirs <b>{vid.peerCam ? 'on' : 'off'}</b>{#if vid.lowBw}{' '}· <b>video paused</b>{/if}</p>

  <h4>Incoming video call</h4>
  <div class="row">
    <select id="vsim-from" aria-label="Incoming video call from" bind:value={from}>{#each callers as p (p.id)}<option value={p.id}>{p.name}</option>{/each}</select>
    <button type="button" onclick={() => { call.receive(from, true); }}>Ring me (video)</button>
  </div>
  <p class="note">Rings like a voice call, labelled "Video call". Answer plain (camera off) or "With camera".</p>

  <h4>Their side</h4>
  <label class="row"><span>Their camera is on</span><Switch label="Their camera is on" checked={vid.peerCam} onchange={(v) => vid.setPeerCam(v)} /></label>
  <label class="row"><span>Low bandwidth (video paused)</span><Switch label="Low bandwidth" checked={vid.lowBw} onchange={(v) => vid.setLowBw(v)} /></label>

  <h4>This {android ? 'phone' : 'computer'}</h4>
  <label class="row"><span>Camera permission denied</span><Switch label="Camera permission denied" checked={vid.perm === 'denied'} onchange={(v) => (vid.perm = v ? 'denied' : 'granted')} /></label>
  {#if !android}
    <label class="row"><span>No camera connected</span><Switch label="No camera connected" checked={!vid.hasCamera} onchange={(v) => (vid.hasCamera = !v)} /></label>
  {/if}
  <label class="row"><span>Use my real webcam (asks the browser)</span><Switch label="Use my real webcam" checked={vid.source === 'real'} onchange={(v) => (vid.source = v ? 'real' : 'sim')} /></label>
  <p class="note">Off: the pictures are generated, nothing is recorded or sent. On: the browser asks for the camera only when you turn the camera on in a call.</p>

  <h4>Timings</h4>
  <label class="row"><span>Controls hide after (s)</span><input type="number" min="1" max="30" value={vid.hideMs / 1000} onchange={(e) => (vid.hideMs = Math.max(1, +e.currentTarget.value) * 1000)} /></label>

  <p class="note">Decided (shipped app): the camera is off by default, the front camera is used first, the self-view is mirrored. Video is AV1; audio never waits behind it.</p>
  <p class="note"><i class="prop">Proposed</i> A call started from the video button turns my camera on when it connects. A voice call, or an incoming video call answered with "Answer", starts with the camera off; "With camera" answers with it on.</p>
  <p class="note"><i class="prop">Proposed</i> Either person can turn video on or off at any time: voice and video are one call, the camera button is the switch. It is recorded as a video call if any camera was on.</p>
  <p class="note"><i class="prop">Proposed</i> While their video fills the screen, the controls and header hide after 4 s without a touch; a tap shows them (or hides them at once). Never during voice.</p>
  <p class="note"><i class="prop">Proposed</i> "Video paused, audio continues" freezes and softens their last picture; reconnecting does the same. When their camera is off the call keeps the voice look with a note.</p>
  <p class="note"><i class="prop">Proposed</i> Self-view is draggable and snaps to the nearest corner; arrow keys move it too. Minimised on Android: a small floating tile plus the strip; on desktop only the strip.</p>
</div>

<style>
  .rows { display: grid; gap: 4px; }
  .state { font-size: var(--fs-sub); margin: 0 0 4px; }
  .row { display: flex; align-items: center; gap: 8px; min-height: 40px; font-size: var(--fs-sub); justify-content: space-between; }
  select { border: 1px solid var(--line); background: var(--surface); color: var(--ink); border-radius: 10px; padding: 6px 8px; font-size: var(--fs-sub); min-width: 0; }
  input { border: 1px solid var(--line); background: var(--surface); color: var(--ink); border-radius: 10px; padding: 6px 8px; font-size: var(--fs-sub); inline-size: 64px; }
  h4 { font-size: var(--fs-sub); margin: 12px 0 2px; }
  .note { font-size: var(--fs-sub); color: var(--muted); margin: 6px 0 0; }
  .prop { font-style: normal; font-weight: 700; color: var(--gold-ink); margin-inline-end: 4px; }
</style>

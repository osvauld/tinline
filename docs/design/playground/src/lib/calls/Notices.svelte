<script lang="ts">
  /* The notifications Tinline has right now: the ongoing call (while one runs and its screen is not in
     front) and missed-call cards. Shown in the shade and on the lock screen. Actions drive the call machine. */
  import Notice from './Notice.svelte';
  import { call, fmtDuration } from './call.svelte';
  import { system } from './system.svelte';
  import { displayName } from '../model/store.svelte';
  import { hhmm } from '../model/clock.svelte';
  import { headsUpIn as noticeIn, headsUpOut as noticeOut } from './motion';

  const name = $derived(call.peer ? displayName(call.peer) : '');
  const text = $derived(system.ongoingActive ? fmtDuration(call.elapsedS) : call.statusText);
</script>

<div class="list">
  {#if system.waitingOutside && call.waitingNow}
    {@const w = call.waitingNow}
    <div in:noticeIn out:noticeOut data-notice="waiting">
      <Notice title="{displayName(w.peer)} is calling" text="Voice call · during your call" person={w.peer} onopen={() => system.openCall()}>
        {#snippet actions()}
          <button type="button" class="act bad" onclick={() => call.declineWaiting()}>Decline</button>
          <button type="button" class="act" onclick={() => { system.foreground(); call.answerWaiting(); }}>End &amp; answer</button>
        {/snippet}
      </Notice>
    </div>
  {/if}
  {#if system.ongoing}
    <div in:noticeIn out:noticeOut data-notice="ongoing">
      <Notice title="Ongoing call · {name}" {text} person={call.peer ?? undefined} tabular={system.ongoingActive} onopen={() => system.openCall()}>
        {#snippet actions()}
          <button type="button" class="act" onclick={() => call.toggleMute()}>{call.muted ? 'Unmute' : 'Mute'}</button>
          <button type="button" class="act" aria-pressed={call.speaker === 'speaker'} onclick={() => call.toggleSpeaker()}>Speaker</button>
          <button type="button" class="act bad" onclick={() => call.hangup()}>Hang up</button>
        {/snippet}
      </Notice>
    </div>
  {/if}
  {#each system.notifications as n (n.id)}
    {@const who = displayName(n.peer)}
    <div in:noticeIn out:noticeOut data-notice="missed">
      <Notice title="{n.count > 1 ? `${n.count} missed calls` : 'Missed call'} · {who}" text="Voice call" when={hhmm(n.at)} person={n.peer} onopen={() => system.openMissed(n)}>
        {#snippet actions()}
          <button type="button" class="act" onclick={() => system.callBack(n)}>Call back</button>
          <button type="button" class="act" onclick={() => system.openMissed(n)}>Message</button>
        {/snippet}
      </Notice>
    </div>
  {/each}
</div>

<style>
  .list { display: grid; gap: 8px; }
</style>

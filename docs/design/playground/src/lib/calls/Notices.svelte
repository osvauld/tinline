<script lang="ts">
  /* The notifications Tinline has right now: the ongoing call (while one runs and its screen is not in
     front) and missed-call cards. Shown in the shade and on the lock screen. Actions drive the call machine. */
  import Notice from './Notice.svelte';
  import { call, fmtDuration } from './call.svelte';
  import { gcall } from './gcall.svelte';
  import { system } from './system.svelte';
  import { displayName, chat } from '../model/store.svelte';
  import { hhmm } from '../model/clock.svelte';
  import { headsUpIn as noticeIn, headsUpOut as noticeOut } from './motion';

  const name = $derived(call.peer ? displayName(call.peer) : '');
  const text = $derived(system.ongoingActive ? fmtDuration(call.elapsedS) : call.statusText);
</script>

<div class="list">
  {#if system.gStarted}
    {@const a = system.gStarted}
    {@const gn = gcall.group?.name ?? (chat(a.gid)?.kind === 'group' ? (chat(a.gid) as { name: string }).name : '')}
    {@const n = gcall.liveFor(a.gid)?.parts.length ?? 0}
    <div in:noticeIn out:noticeOut data-notice="gstarted">
      <Notice title={gn} text="{displayName(a.by)} started a call · {n} joined" group={gn} onopen={() => system.openGroup(a.gid)}>
        {#snippet actions()}
          <button type="button" class="act" onclick={() => system.joinFromNotice(a.gid)}>Join</button>
          <button type="button" class="act" onclick={() => { gcall.dismissAnnounce(); }}>Dismiss</button>
        {/snippet}
      </Notice>
    </div>
  {/if}
  {#if system.gOngoing}
    <div in:noticeIn out:noticeOut data-notice="gongoing">
      <Notice title="Group call · {gcall.group?.name ?? ''}" text={system.gOngoingActive ? fmtDuration(gcall.elapsedS) : gcall.statusText} group={gcall.group?.name ?? ''} tabular={system.gOngoingActive} onopen={() => system.openCall()}>
        {#snippet actions()}
          <button type="button" class="act" onclick={() => gcall.toggleMute()}><span class="lbl"><span aria-hidden="true">Unmute</span><span>{gcall.muted ? 'Unmute' : 'Mute'}</span></span></button>
          <button type="button" class="act" aria-pressed={gcall.speaker === 'speaker'} onclick={() => gcall.toggleSpeaker()}>Speaker</button>
          <button type="button" class="act bad" onclick={() => gcall.leave()}>Leave</button>
        {/snippet}
      </Notice>
    </div>
  {/if}
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
          <button type="button" class="act" onclick={() => call.toggleMute()}><span class="lbl"><span aria-hidden="true">Unmute</span><span>{call.muted ? 'Unmute' : 'Mute'}</span></span></button>
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
  /* Mute / Unmute share one box (the longer word reserves the width), so the buttons beside it never slide. */
  .lbl { display: grid; justify-items: center; }
  .lbl > :global(*) { grid-area: 1 / 1; }
  .lbl > :global(:first-child) { visibility: hidden; }
</style>

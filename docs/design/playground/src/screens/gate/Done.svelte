<script lang="ts">
  import Mark from '../../lib/brand/Mark.svelte';
  import Icon from '../../lib/ui/Icon.svelte';
  import Btn from './Btn.svelte';
  import StepShell from './StepShell.svelte';
  import { gate } from '../../lib/model/actions/gate.svelte';
  import { nav } from '../../lib/nav/router.svelte';
  import { open } from '../../lib/model/actions/people.svelte';
  import { onMount } from 'svelte';
  import { timeScale } from '../../lib/fx/motion';
  import { LINE_SPRING } from '../../lib/brand/springs';
  import { emitFx } from '../../lib/fx/bus';
  /* "Your line is open": the string arrives slack (as on Welcome) and pulls straight once the step has slid in,
     with the mark's own spring and its soft taut cue. Reduced motion: the mark settles without the spring. */
  let line = $state<'slack' | 'taut'>('slack');
  /* Two tugs, in the rhythm of the `line` cue (two plucks): the line pulls tight on its first note, gives a little,
     and tugs again on the second, then rests. The gap is read from the cue so sound and motion stay locked.
     The line warms to gold on the first tug. */
  const DONE_SPRING = LINE_SPRING;
  function tug(n: number) { if (n === 1) emitFx({ type: 'line' }); }
  onMount(() => { const t = setTimeout(() => (line = 'taut'), 450 * timeScale()); return () => clearTimeout(t); });
  /* A new account lands on Chats (its empty hero) with Add contact open over it: closing the sheet after adding
     someone reveals "Your line is open · Message {name}". */
  function addFirst() { gate.finish(); nav.setTab('chats'); open('addContact'); }
  const first = $derived(gate.name.trim().split(/\s+/)[0] ?? '');
  const copy = $derived(
    gate.adding && gate.flow === 'restore' ? ['Added ' + (gate.name.trim() || 'your account') + ' to this ' + gate.noun, 'Only the account you select is online at a time, so ' + (gate.name.trim() || 'this account') + ' is online now and your other account is paused. You can switch in Settings.']
    : gate.flow === 'restore' ? ['Welcome back' + (first ? ', ' + first : ''), 'Your account is back on this ' + gate.noun + '. Your contacts, chats and call history are kept on your other devices, not with us. This ' + gate.noun + ' starts with an empty contact list.']
    : gate.flow === 'link' ? ['Linked to ' + (gate.peer || 'your other device'), 'Your contacts and chats are arriving from ' + (gate.peer || 'your other device') + ' and will keep in step on both.']
    : ['Your line is open' + (first ? ', ' + first : ''), 'Scan their code or share yours, and you can call each other.'],
  );
  /* Two gentle "good to know" notes after a new account: said as tips at the happy moment, not as warnings. */
  const tips = $derived(gate.flow !== 'create' ? [] : [
    ['devices', 'Two devices are better than one', 'Link another device when you can, so losing one never locks you out.'],
    gate.isAndroid
      ? ['laptop', 'Groups are hosted on a computer', 'To start a group, add Tinline on a computer that can stay on.']
      : ['users', 'This computer can host your groups', 'Groups are hosted on a computer. Keep Tinline open and your groups stay reachable.'],
  ]);
</script>

<StepShell title={copy[0]} lead={copy[1]} center>
  <div class="hero"><Mark size={96} state={line} live lineColor spring={DONE_SPRING} ontug={tug} label="Tinline" /></div>
  {#if tips.length}
    <ul class="tips">
      {#each tips as [ic, b, r] (b)}<li><span class="ic"><Icon name={ic} size={20} /></span><span><b>{b}</b><br />{r}</span></li>{/each}
    </ul>
  {/if}
  {#snippet foot()}
    {#if gate.flow === 'create'}
      <Btn enter onclick={addFirst}>Add my first contact</Btn>
      <Btn kind="text" onclick={() => gate.finish()}>Not now</Btn>
    {:else}
      <Btn enter onclick={() => gate.finish()}>Open Tinline</Btn>
    {/if}
  {/snippet}
</StepShell>

<style>
  .hero { order: -1; display: grid; place-items: center; padding-block: 28px 4px; color: var(--ink); }
  .tips { list-style: none; margin: 8px 0 0; padding: 0; display: grid; gap: 12px; inline-size: min(100%, 420px); text-align: start; }
  .tips li { display: flex; gap: 14px; align-items: flex-start; font-size: var(--fs-sub); line-height: 19px; color: var(--muted); }
  .tips b { color: var(--ink); font-size: var(--fs-body); }
  .ic { display: grid; place-items: center; inline-size: 36px; block-size: 36px; border-radius: 50%; background: var(--accent-soft); color: var(--accent-ink); flex: none; }
</style>

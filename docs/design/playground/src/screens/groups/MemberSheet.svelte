<script lang="ts">
  /* A member's sheet: per-member actions according to MY role, then the two confirm steps (flag, chat request).
     Bottom sheet on Android, centred card on desktop (Layer 'dialog'). */
  import Avatar from '../../lib/ui/Avatar.svelte';
  import Icon from '../../lib/ui/Icon.svelte';
  import Layer from '../chat/Layer.svelte';
  import Btn from './Btn.svelte';
  import Pill from './Pill.svelte';
  import { db, firstName, person } from '../../lib/model/store.svelte';
  import { nav } from '../../lib/nav/router.svelte';
  import { prefs } from '../../lib/model/prefs.svelte';
  import { gx } from '../../lib/model/actions/groups.svelte';
  import {
    FLAGS_TO_REMOVE, appointHost, canAskPrivately, canFlag, canHostAppoint, canSetAdmin, dayOnly, flagCount, flagMember, iAmAdmin, member, requestFor,
    revokeHost, roleLabel, sendRequest, setAdmin, cancelRequest,
  } from '../../lib/model/actions/groups';
  import type { Group } from '../../lib/model/types';
  import { fullName, hueOf } from './util';

  let { g, id, onclose }: { g: Group; id: string; onclose: () => void } = $props();

  let view = $state<'actions' | 'flag' | 'ask'>('actions');
  let note = $state('');
  const m = $derived(member(g, id));
  const p = $derived(person(id));
  const fn = $derived(firstName(id));
  const isMe = $derived(id === 'me');
  const n = $derived(flagCount(g, id));
  const req = $derived(requestFor(id));
  const admin = $derived(iAmAdmin(g));
  const owner = $derived(m?.role === 'owner');

  // The row under the sheet can vanish (3rd flag removes the member): close with it.
  $effect(() => { if (!m) onclose(); });

  function message() { onclose(); const c = db.chats.find((x) => x.kind === 'direct' && x.person === id);
    if (c) (prefs.form === 'desktop' ? nav.open({ name: 'conversation', chat: c.id }) : nav.push({ name: 'conversation', chat: c.id })); else nav.push({ name: 'contact', person: id }); }
  function send() { sendRequest(g, id, note); onclose(); }
  function flag() { const r = flagMember(g, id, 'me'); if (r.ok) onclose(); }
  const since = $derived(m ? (m.invitedBy ? `Invited by ${firstName(m.invitedBy)} · ${dayOnly(m.since)}` : `Member since ${dayOnly(m.since)}`) : '');
</script>

{#if m}
<Layer variant="dialog" label={isMe ? 'Your role in this group' : `${fullName(id)} in ${g.name}`} {onclose}>
  <div class="sheet">
    {#key view}
    <div class="view">
    {#if view === 'actions'}
      <header class="hd">
        <Avatar name={fullName(id)} hue={hueOf(id)} size={48} />
        <span class="tx"><b>{isMe ? `${fullName(id)} (you)` : fullName(id)}</b>
          <span class="meta">{since}</span></span>
        <span class="pills"><Pill tone={m.role === 'member' ? 'muted' : m.role}>{roleLabel(m.role)}</Pill>{#if m.host}<Pill tone="host">Host</Pill>{/if}{#if m.callHost}<Pill tone="call">Call host</Pill>{/if}</span>
      </header>
      {#if admin && !isMe && !owner}
        <p class="flags">{n} of {FLAGS_TO_REMOVE} flags</p>
      {/if}
      <div class="list" role="group" aria-label="Actions">
        {#if !isMe}
          {#if p?.contact && !p.blocked}
            <button type="button" class="mi press" onclick={message}><Icon name="chat" size={20} /><span class="t"><b>Message {fn}</b></span></button>
          {:else if p?.blocked}
            <div class="mi off"><Icon name="off" size={20} /><span class="t"><b>You blocked {fn}</b><small>They stay in the group. Unblock them in Contacts to chat.</small></span></div>
          {:else if req?.state === 'pending'}
            <div class="mi off"><Icon name="clock" size={20} /><span class="t"><b>Request sent</b><small>Waiting for {fn} to answer</small></span>
              <button type="button" class="link" onclick={() => cancelRequest(req.id)}>Cancel</button></div>
          {:else if req?.state === 'blocked'}
            <div class="mi off"><Icon name="off" size={20} /><span class="t"><b>Blocked</b><small>{fn} can’t receive requests from you.</small></span></div>
          {:else if canAskPrivately(g, id)}
            <button type="button" class="mi press" onclick={() => (view = 'ask')}><Icon name="chat" size={20} />
              <span class="t"><b>{req?.state === 'declined' ? `Ask ${fn} again` : `Ask ${fn} for a private chat`}</b>
                <small>{req?.state === 'declined' ? `${fn} declined your last request` : 'Sends a chat request through the group’s host'}</small></span></button>
          {/if}
        {/if}
        {#if canSetAdmin(g, id)}
          <button type="button" class="mi press" onclick={() => setAdmin(g, id, m.role !== 'admin')}><Icon name="shield" size={20} />
            <span class="t"><b>{m.role === 'admin' ? 'Remove admin' : 'Make admin'}</b></span></button>
        {/if}
        {#if canHostAppoint(g, id)}
          {#if m.hostPending}
            <button type="button" class="mi press" onclick={() => revokeHost(g, id)}><Icon name="server" size={20} />
              <span class="t"><b>Cancel host request</b><small>Waiting for {isMe ? 'this device' : fn + '’s device'} to accept</small></span></button>
          {:else if m.host}
            <button type="button" class="mi press" onclick={() => revokeHost(g, id)}><Icon name="server" size={20} />
              <span class="t"><b>{isMe ? 'Stop hosting on this device' : 'Remove as host'}</b></span></button>
          {:else}
            <button type="button" class="mi press" onclick={() => { appointHost(g, id); onclose(); }}><Icon name="server" size={20} />
              <span class="t"><b>{isMe ? 'Host on this device' : 'Make host'}</b><small>{isMe ? 'This device has to accept' : `Asks ${fn} to host. They have to accept.`}</small></span></button>
          {/if}
        {/if}
        {#if canFlag(g, id)}
          <button type="button" class="mi press danger" onclick={() => (view = 'flag')}><Icon name="flag" size={20} />
            <span class="t"><b>Flag for removal</b><small>{FLAGS_TO_REMOVE} flags remove {fn} from the group</small></span></button>
        {:else if owner && !isMe}
          <div class="mi off"><Icon name="flag" size={20} /><span class="t"><b>The owner can’t be flagged</b></span></div>
        {/if}
      </div>
      <div class="acts"><Btn variant="quiet" data-autofocus onclick={onclose}>Close</Btn></div>

    {:else if view === 'flag'}
      {@const next = n + 1}
      <div class="dlg">
        <h2>Flag {fn} for removal?</h2>
        <div class="meter" role="img" aria-label="Flag {next} of {FLAGS_TO_REMOVE}">
          {#each Array(FLAGS_TO_REMOVE) as _, i (i)}<i class:on={i < n} class:yours={i === n}></i>{/each}
        </div>
        <p class="lead">Yours will be flag {next} of {FLAGS_TO_REMOVE}.
          {#if next >= FLAGS_TO_REMOVE}It’s the last one, so {fn} is removed from the group right away.{:else}At {FLAGS_TO_REMOVE} flags {fn} is removed from the group.{/if}</p>
        <p>Flags can’t be taken back, and removal is final. The same admin can flag more than once; each flag counts.</p>
        <p>Flagging is a decision for the group. It is separate from blocking, which only affects you. If {fn} is removed, they are told who flagged them and when.</p>
        <div class="acts"><Btn variant="quiet" data-autofocus onclick={() => (view = 'actions')}>Back</Btn>
          <Btn variant={next >= FLAGS_TO_REMOVE ? 'danger' : 'outline'} onclick={flag}>Flag {fn}</Btn></div>
      </div>

    {:else}
      <div class="dlg">
        <h2>Ask {fn} for a private chat</h2>
        <label class="fld"><span>Add a note (optional)</span>
          <textarea rows="2" maxlength="140" placeholder="Hi {fn}…" bind:value={note}></textarea></label>
        <p>{fn} sees your name, the group and your note. The request goes through the group’s host, sealed, so the host can’t read it.</p>
        <p>If {fn} accepts, you can message each other directly. Calls stay off until one of you allows them. You can send one request per person per day, and it expires after 7 days.</p>
        <div class="acts"><Btn variant="quiet" onclick={() => (view = 'actions')}>Back</Btn><Btn variant="primary" onclick={send} data-autofocus>Send request</Btn></div>
      </div>
    {/if}
    </div>
    {/key}
  </div>
</Layer>
{/if}

<style>
  .view { animation: vin var(--dur-crossfade) var(--ease-out); }
  @keyframes vin { from { opacity: 0; } }
  .hd { display: flex; align-items: center; gap: 12px; padding: 18px 20px 10px; }
  .tx { flex: 1; min-inline-size: 0; display: grid; gap: 1px; }
  .tx b { font-size: var(--fs-title); font-weight: 650; line-height: 22px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .meta { font-size: var(--fs-sub); color: var(--muted); line-height: 18px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .pills { display: flex; gap: 4px; flex-wrap: wrap; justify-content: flex-end; max-inline-size: 40%; }
  .flags { margin: 0 20px 4px; font-size: var(--fs-sub); color: var(--muted); font-variant-numeric: tabular-nums; }
  .list { display: grid; padding: 4px 8px 0; }
  .mi { display: flex; align-items: center; gap: 16px; min-block-size: 56px; padding: 6px 12px; border: 0; border-radius: var(--r-ctl); background: none; text-align: start; inline-size: 100%; color: var(--ink); font: inherit; cursor: pointer; }
  .mi > :global(svg) { color: var(--muted); }
  button.mi:hover { background: var(--surface-2); }
  .mi .t { flex: 1; min-inline-size: 0; display: grid; gap: 1px; }
  .mi b { font-weight: 600; font-size: var(--fs-body); line-height: 20px; }
  .mi small { font-size: var(--fs-sub); line-height: 17px; color: var(--muted); }
  .mi.off { cursor: default; }
  .mi.off b { color: var(--muted); }
  .mi.danger b, .mi.danger > :global(svg) { color: var(--danger); }
  .link { border: 0; background: none; color: var(--accent-ink); font-weight: 650; padding: 0 8px; min-block-size: var(--btn); cursor: pointer; }
  .acts { display: flex; gap: 8px; justify-content: flex-end; padding: 8px 16px 14px; }
  .dlg { padding: 20px 20px 4px; }
  .dlg h2 { margin: 0 0 12px; font-size: var(--fs-title); font-weight: 650; line-height: 24px; }
  .dlg p { margin: 8px 0 0; color: var(--muted); font-size: var(--fs-sub); line-height: 19px; }
  .dlg p.lead { color: var(--ink); font-size: var(--fs-body); line-height: 21px; margin-block-start: 12px; }
  .dlg .acts { padding: 0; margin-block-start: 18px; }
  .meter { display: flex; gap: 6px; }
  .meter i { flex: 1; block-size: 6px; border-radius: 3px; background: var(--line); transition: background-color var(--dur-enter) var(--ease-out); }
  .meter i.on { background: var(--danger-btn); }
  .meter i.yours { background: color-mix(in srgb, var(--danger-btn) 45%, var(--line)); }
  .fld { display: grid; gap: 6px; font-size: var(--fs-sub); font-weight: 600; color: var(--muted); }
  .fld textarea { resize: none; padding: 10px 14px; border-radius: var(--r-ctl); border: 1px solid var(--line); background: var(--surface); font-size: var(--fs-body); line-height: 21px; color: var(--ink); font-weight: 400; }
  .fld textarea:focus { outline: 0; border-color: var(--accent); box-shadow: 0 0 0 3px color-mix(in srgb, var(--accent-ink) 14%, transparent); }
</style>

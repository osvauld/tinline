<script lang="ts">
  /* Group info (route `groupInfo`): header, invites, hosts, members, mute, leave. Everything is read from the
     one store; what you can do depends on MY role. Removed groups show the removal view instead. */
  import Avatar from '../../lib/ui/Avatar.svelte';
  import Icon from '../../lib/ui/Icon.svelte';
  import IconButton from '../../lib/ui/IconButton.svelte';
  import Switch from '../../lib/ui/Switch.svelte';
  import Layer from '../chat/Layer.svelte';
  import Btn from './Btn.svelte';
  import Pill from './Pill.svelte';
  import MemberSheet from './MemberSheet.svelte';
  import InviteSheet from './InviteSheet.svelte';
  import Removal from './Removal.svelte';
  import FlagsOnMe from './FlagsOnMe.svelte';
  import { fold, foldOut, flipMs } from './fold';
  import { flip } from 'svelte/animate';
  import { db, chat as getChat, firstName, isGroup, person } from '../../lib/model/store.svelte';
  import { clock } from '../../lib/model/clock.svelte';
  import { nav } from '../../lib/nav/router.svelte';
  import { prefs } from '../../lib/model/prefs.svelte';
  import { gx } from '../../lib/model/actions/groups.svelte';
  import { gcall } from '../../lib/calls/gcall.svelte';
  import {
    FLAGS_TO_REMOVE, acceptCallHost, acceptHosting, callHostsReady, declineCallHost, revokeCallHost, setCallHosting, canLeave, cancelInvite, cancelLink, canHostAppoint, createLink, declineHosting, dayOnly, flagCount, hosts, iAmAdmin, leaveGroup,
    linkHost, linkLive, linkText, myRole, removedMembers, requestFor, revokeHost, roleLabel, whenLabel,
  } from '../../lib/model/actions/groups';
  import type { Group, GroupMember } from '../../lib/model/types';
  import { fullName, hueOf, plural } from './util';
  import { shape } from '../../lib/geom/action';
  import { geo, R, uniform } from '../../lib/geom/geo.svelte';
  const cardShape = () => ({ radii: uniform(R().ctl), smoothing: geo.smooth, stroke: { width: 1, color: 'var(--hairline)' } });

  let { chatId }: { chatId: string } = $props();
  const g = $derived.by(() => { const c = getChat(chatId); return isGroup(c) ? c : undefined; });

  let sheet = $state<{ kind: 'member'; id: string } | { kind: 'invite' } | { kind: 'leave' } | null>(null);
  let opener: HTMLElement | null = null;
  let copied = $state<string | null>(null);
  let copyNote = $state('');
  const COPIED_MS = 1600; // how long "Copied" stays; a UI constant, not a motion timing
  const layerOpen = $derived(sheet !== null);

  function open(s: NonNullable<typeof sheet>) { opener = document.activeElement as HTMLElement; sheet = s; }
  function close() { sheet = null; queueMicrotask(() => opener?.isConnected && opener.focus({ preventScroll: true })); }

  const role = $derived(g ? myRole(g) : null);
  const admin = $derived(g ? iAmAdmin(g) : false);
  const hostList = $derived(g ? hosts(g) : []);
  const pendingHosts = $derived(g ? g.members.filter((m) => m.hostPending) : []);
  const hostRows = $derived([...hostList, ...pendingHosts.filter((m) => !m.host)]);
  const noHost = $derived(hostList.length === 0);
  const hostsUp = $derived(db.env.hostsOnline && !noHost);
  const hostLine = $derived(noHost ? 0 : hostsUp ? 1 : 2);
  const hostLines = $derived(['This group has no host. Posts wait until a host is appointed.', `${plural(hostList.length, 'host')} online. Posts go through right away.`, 'No host is online. Your posts are saved and send when one is.']);
  /* Call hosts (U25): a separate permit; the device accepts it itself. */
  const chRows = $derived(g ? g.members.filter((m) => m.callHost || m.callHostPending) : []);
  const chReady = $derived(g ? callHostsReady(g).filter((m) => (m.id === 'me' ? db.env.networkOnline : person(m.id)?.online !== false)) : []);
  const chLine = $derived(chRows.filter((m) => m.callHost).length === 0 ? 0 : chReady.length > 0 ? 1 : 2);
  const chLines = $derived(['This group has no call host. Nobody can start a call until an admin appoints one.', `${plural(chReady.length, 'call host')} ready. Calls start through ${chReady.length === 1 ? 'it' : 'them'}.`, 'No call host is available right now. A call can’t start until one is back.']);
  const myCh = $derived(g ? g.members.find((m) => m.id === 'me') : undefined);
  const chKind = (id: string) => (gcall.hostClass(id) === 'phone' ? 'Phone' : 'Computer');
  const order = { owner: 0, admin: 1, member: 2 } as const;
  const sorted = $derived(g ? [...g.members].sort((a, b) => order[a.role] - order[b.role] || (a.id === 'me' ? -1 : b.id === 'me' ? 1 : 0)) : []);
  const roleLine = $derived(role === 'owner' ? 'You are the owner' : role === 'admin' ? 'You are an admin' : role ? 'You are a member' : '');
  const removed = $derived(g ? removedMembers(g) : []);
  const linkHostName = (id: string) => (id === 'me' ? 'this device' : firstName(id) + '’s device');

  function sub(m: GroupMember) {
    return m.invitedBy ? `Invited by ${firstName(m.invitedBy)} · ${dayOnly(m.since)}` : `Since ${dayOnly(m.since)}`;
  }
  function reqNote(id: string) {
    const r = requestFor(id, g?.id);
    return r ? (r.state === 'pending' ? 'Chat request sent' : r.state === 'declined' ? 'Declined your chat request' : r.state === 'cancelled' ? 'You cancelled your request' : r.state === 'blocked' ? 'Blocked' : '') : '';
  }

  async function copy(code: string, text: string) {
    try { await navigator.clipboard.writeText(text); copyNote = 'Link copied.'; }
    catch { copyNote = 'Copying isn’t available here. Select the link and copy it instead.'; }
    copied = code;
    setTimeout(() => { if (copied === code) copied = null; }, COPIED_MS);
  }

  function leave() { if (g && leaveGroup(g)) { sheet = null; nav.back(); } }
  const back = () => nav.back();
</script>

<div class="info" data-info={chatId}>
  <header class="bar"><IconButton icon="back" label="Back to conversation" onclick={back} /><b>{g?.status === 'removed' ? 'Removed' : 'Group info'}</b></header>

  <div class="scroll" inert={layerOpen}>
  <div class="col">
  {#if !g}
    <p class="none">Group not found.</p>
  {:else}
    <section class="hero">
      <Avatar name={g.name} group size={72} />
      <h1>{g.name}</h1>
      {#if g.status === 'active'}
        <p>{plural(g.members.length, 'member')} · {roleLine}</p>
      {:else if g.status === 'left'}
        <p>You left this group</p>
      {:else}
        <p>You’re no longer a member</p>
      {/if}
    </section>

    {#if g.status === 'removed'}
      <Removal {g} />
    {:else if g.status === 'left'}
      <p class="note pad">You can still read the posts on this device. To come back, someone in the group has to invite you again.</p>
    {:else}
      {#if flagCount(g, 'me') > 0}<FlagsOnMe {g} />{/if}
      <!-- ---------- invites (admins) ---------- -->
      {#if admin}
        <section class="sec" aria-labelledby="inv-h">
          <h2 id="inv-h">Invites</h2>
          <div class="card" use:shape={cardShape()}>
            <button type="button" class="act press" onclick={() => open({ kind: 'invite' })}><Icon name="userplus" size={20} /><span class="t"><b>Invite contacts</b><small>A private invite to each person</small></span><Icon name="chevr" size={18} /></button>
            <button type="button" class="act press" disabled={noHost} onclick={() => createLink(g)}><Icon name="link" size={20} />
              <span class="t"><b>Create invite link</b><small>{noHost ? 'Appoint a host first. A link names one host.' : 'Valid for 7 days'}</small></span><Icon name="plus" size={18} /></button>
          </div>

          {#if (g.links ?? []).length}
            <ul class="links" role="list" aria-label="Invite links">
              {#each g.links ?? [] as l (l.code)}
                {@const live = linkLive(l)}
                <li class="link" class:dead={!live} use:shape={cardShape()} in:fold out:foldOut>
                  <code class="url" data-link>{linkText(l)}</code>
                  <span class="lmeta">{#if live}Valid until {whenLabel(l.expiresAt)} · names {linkHostName(l.host)}{:else}Expired {whenLabel(l.expiresAt)}{/if}</span>
                  <span class="lacts">
                    {#if live}<Btn variant="outline" onclick={() => copy(l.code, linkText(l))}><span class="cp" class:on={copied === l.code}><span class="a"><Icon name="copy" size={18} />Copy</span><span class="b2"><Icon name="check" size={18} />Copied</span></span></Btn>{/if}
                    <Btn variant="quiet" onclick={() => cancelLink(g, l.code)}>{live ? 'Cancel link' : 'Remove'}</Btn>
                  </span>
                </li>
              {/each}
            </ul>
            {#if (g.links ?? []).some(linkLive)}
              <p class="note" in:fold out:foldOut>Anyone who has this link can join until it expires. Share it only with people you want in the group, and know that it can be passed on.
                Cancelling stops it for anyone who hasn’t opened it yet; someone who already opened it can still join until it expires. A link names one host: if that device is offline, the link won’t work.</p>
            {/if}
            <p class="vh" role="status">{copyNote}</p>
          {/if}

          {#if g.invited.length}
            <ul class="pend" role="list" aria-label="Pending invites">
              {#each g.invited as inv (inv.id)}
                {@const exp = inv.expiresAt <= clock.now}
                <li class="prow" in:fold out:foldOut>
                  <Avatar name={fullName(inv.id)} hue={hueOf(inv.id)} person={inv.id} size={40} />
                  <span class="t"><b>{fullName(inv.id)}</b><small>{exp ? 'Invite expired' : `Invite sent · expires ${dayOnly(inv.expiresAt)}`}</small></span>
                  <Btn variant="quiet" label="{exp ? 'Remove' : 'Cancel'} invite for {fullName(inv.id)}" onclick={() => cancelInvite(g, inv.id)}>{exp ? 'Remove' : 'Cancel'}</Btn>
                </li>
              {/each}
            </ul>
          {/if}
        </section>
      {/if}

      <!-- ---------- hosts ---------- -->
      <section class="sec" aria-labelledby="host-h">
        <h2 id="host-h">Hosts</h2>
        <!-- All three lines share one grid cell (the tallest sets the height), so the status changing never moves what is below. -->
        <p class="status" class:warn={!hostsUp} role="status">
          <span class="dot" class:on={hostsUp}></span>
          <span class="stk">{#each hostLines as line, i (i)}<span class:cur={i === hostLine}>{line}</span>{/each}</span>
        </p>
        {#if hostRows.length}
          <ul class="rows" role="list" aria-label="Hosts">
            {#each hostRows as m (m.id)}
              <li class="mrow" animate:flip={{ duration: flipMs() }} in:fold out:foldOut>
                <Avatar name={fullName(m.id)} hue={hueOf(m.id)} person={m.id} size={40} />
                <span class="t"><b>{m.id === 'me' ? 'This device' : fullName(m.id)}</b>
                  <small>{#if m.hostPending}Waiting to accept{:else}<span class="dot sm" class:on={db.env.hostsOnline}></span>{db.env.hostsOnline ? 'Online' : 'Offline'}{/if}{m.callHost ? ' · also hosts calls' : ''}</small></span>
                {#if m.hostPending && m.id === 'me'}
                  <Btn variant="quiet" onclick={() => declineHosting(g, m.id)}>Decline</Btn><Btn variant="primary" onclick={() => acceptHosting(g, m.id)}>Accept</Btn>
                {:else if canHostAppoint(g, m.id)}
                  <Btn variant="quiet" label="Remove {m.id === 'me' ? 'this device' : fullName(m.id)} as host" onclick={() => revokeHost(g, m.id)}>{m.hostPending ? 'Cancel' : 'Remove'}</Btn>
                {/if}
              </li>
            {/each}
          </ul>
        {/if}
        <p class="note">Hosts store the group and pass posts on when others are offline. They are members you trust: a host can read everything posted in this group.
          {#if admin}Appoint a host from a member’s menu; they have to accept. Removing a host changes the group’s address within 2 days, so that device can no longer reach it.{/if}</p>
      </section>

      <!-- ---------- call hosts ---------- -->
      <section class="sec" aria-labelledby="ch-h" data-callhosts>
        <h2 id="ch-h">Call hosts</h2>
        <p class="status" class:warn={chLine !== 1} role="status">
          <span class="dot" class:on={chLine === 1}></span>
          <span class="stk">{#each chLines as line, i (i)}<span class:cur={i === chLine}>{line}</span>{/each}</span>
        </p>
        {#if chRows.length}
          <ul class="rows" role="list" aria-label="Call hosts">
            {#each chRows as m (m.id)}
              <li class="mrow" animate:flip={{ duration: flipMs() }} in:fold out:foldOut data-callhost={m.id}>
                <Avatar name={fullName(m.id)} hue={hueOf(m.id)} person={m.id} size={40} />
                <span class="t"><b>{m.id === 'me' ? 'This device' : fullName(m.id)}</b>
                  <small>{#if m.callHostPending}Waiting to accept{:else if m.callHostPaused}Paused on this device{:else}<span class="dot sm" class:on={m.id === 'me' ? db.env.networkOnline : person(m.id)?.online !== false}></span>{(m.id === 'me' ? db.env.networkOnline : person(m.id)?.online !== false) ? 'Online' : 'Offline'} · {chKind(m.id)}{/if}</small></span>
                {#if m.callHostPending && m.id === 'me'}
                  <Btn variant="quiet" onclick={() => declineCallHost(g, m.id)}>Decline</Btn><Btn variant="primary" onclick={() => acceptCallHost(g, m.id)}>Accept</Btn>
                {:else if canHostAppoint(g, m.id)}
                  <Btn variant="quiet" label="Remove {m.id === 'me' ? 'this device' : fullName(m.id)} as call host" onclick={() => revokeCallHost(g, m.id)}>{m.callHostPending ? 'Cancel' : 'Remove'}</Btn>
                {/if}
              </li>
            {/each}
          </ul>
        {/if}
        {#if myCh?.callHost}
          <div class="card ch-sw" in:fold out:foldOut>
            <label class="act row-sw"><Icon name="phone" size={20} /><span class="t"><b>Host calls on this device</b><small>{myCh.callHostPaused ? 'Off. This device won’t be asked to carry calls.' : 'On. This device can be asked to carry calls.'}</small></span>
              <Switch label="Host calls on this device" checked={!myCh.callHostPaused} onchange={(v) => setCallHosting(g, v)} /></label>
          </div>
        {/if}
        <p class="note">A call host carries the call. Everyone in a call connects to it, and it passes the audio on, so it can hear the call. That is separate from chat hosting: a chat host is not a call host unless you also appoint it as one. Call hosts are members you trust, and each one chooses to accept.
          {#if admin}Appoint one from a member’s menu. A phone only hosts calls that its owner starts.{/if}</p>
      </section>

      <!-- ---------- members ---------- -->
      <section class="sec" aria-labelledby="mem-h">
        <h2 id="mem-h">Members <span class="n">{g.members.length}</span></h2>
        <ul class="rows" role="list" aria-label="Members">
          {#each sorted as m (m.id)}
            {@const fc = flagCount(g, m.id)}
            {@const rn = reqNote(m.id)}
            <li animate:flip={{ duration: flipMs() }} in:fold out:foldOut>
              <button type="button" class="mrow press" data-member={m.id} aria-label="{fullName(m.id)}{m.id === 'me' ? ' (you)' : ''}, {roleLabel(m.role)}{m.host ? ', host' : ''}{m.callHost ? ', call host' : ''}. Open actions" onclick={() => open({ kind: 'member', id: m.id })}>
                <Avatar name={fullName(m.id)} hue={hueOf(m.id)} person={m.id} size={40} />
                <span class="t"><b>{m.id === 'me' ? `${fullName(m.id)} (you)` : fullName(m.id)}</b>
                  <small>{sub(m)}{#if (admin || m.id === 'me') && fc > 0 && m.role !== 'owner'}<span class="fc">{' · '}{fc} of {FLAGS_TO_REMOVE} flags</span>{/if}{#if rn}<span class="rq">{' · '}{rn}</span>{/if}</small></span>
                <span class="marks">
                  {#if m.role !== 'member'}<Pill tone={m.role}>{roleLabel(m.role)}</Pill>{/if}
                  {#if m.host}<span class="mk" title="Host" role="img" aria-label="Host"><Icon name="server" size={14} /></span>{/if}
                  {#if m.callHost}<span class="mk" title="Call host" role="img" aria-label="Call host"><Icon name="phone" size={14} /></span>{/if}
                </span>
              </button>
            </li>
          {/each}
        </ul>
        {#if admin && removed.length}
          <h3 class="sub2" in:fold out:foldOut>Removed</h3>
          <ul class="rows" role="list" aria-label="Removed members">
            {#each removed as id (id)}
              <li class="mrow plain" in:fold out:foldOut>
                <Avatar name={fullName(id)} hue={hueOf(id)} person={id} size={40} />
                <span class="t"><b>{fullName(id)}</b><small>Flagged by {(g.flags[id] ?? []).map((x) => firstName(x)).join(', ')}</small></span>
              </li>
            {/each}
          </ul>
        {/if}
      </section>

      <!-- ---------- mute / leave ---------- -->
      <section class="sec" aria-label="Notifications and leaving">
        <div class="card" use:shape={cardShape()}>
          <label class="act row-sw"><Icon name="mute" size={20} /><span class="t"><b>Mute notifications</b><small>Posts still arrive; you just aren’t alerted</small></span>
            <Switch label="Mute notifications" bind:checked={g.muted} /></label>
          {#if canLeave(g)}
            <button type="button" class="act press danger" onclick={() => open({ kind: 'leave' })}><Icon name="leave" size={20} /><span class="t"><b>Leave group</b></span></button>
          {:else}
            <div class="act off"><Icon name="leave" size={20} /><span class="t"><b>Leave group</b><small>The owner can’t leave yet</small></span></div>
          {/if}
        </div>
      </section>
    {/if}
  {/if}
  </div>
  </div>

  {#if g && sheet?.kind === 'member'}
    <MemberSheet {g} id={sheet.id} onclose={close} />
  {:else if g && sheet?.kind === 'invite'}
    <InviteSheet {g} onclose={close} />
  {:else if g && sheet?.kind === 'leave'}
    <Layer variant="dialog" label="Leave group" onclose={close}>
      <div class="dlg">
        <h2>Leave {g.name}?</h2>
        <p>You’ll stop getting posts. What’s already on this device stays, and you can read it. To come back, someone in the group has to invite you again.</p>
        {#if role === 'admin'}<p>You’re an admin. The owner and the other admins stay.</p>{/if}
        <div class="acts"><Btn variant="quiet" data-autofocus onclick={close}>Cancel</Btn><Btn variant="danger" onclick={leave}>Leave</Btn></div>
      </div>
    </Layer>
  {/if}
</div>

<style>
  .info { position: relative; display: flex; flex-direction: column; block-size: 100%; min-block-size: 0; background: var(--bg); overflow: hidden; }
  .bar { display: flex; align-items: center; gap: 4px; min-block-size: 60px; padding: 8px 12px 8px 8px; background: var(--surface); border-block-end: 1px solid var(--hairline); flex: none; }
  .bar b { font-size: var(--fs-title); font-weight: 600; }
  .scroll { flex: 1; min-block-size: 0; overflow: auto; padding-block-end: 32px; }
  .col { max-inline-size: 640px; margin-inline: auto; }
  .none, .pad { padding: 24px 20px; color: var(--muted); }

  .hero { display: grid; justify-items: center; text-align: center; padding: 16px 20px 8px; gap: 2px; }
  .hero h1 { margin: 12px 0 0; font-size: 22px; line-height: 28px; font-weight: 650; overflow-wrap: anywhere; }
  .hero p { margin: 0; color: var(--muted); font-size: var(--fs-sub); line-height: 19px; }

  .sec { padding: 0 16px; margin-block-start: 24px; }
  .sec h2 { margin: 0 4px 8px; font-size: 12px; font-weight: 650; letter-spacing: .06em; text-transform: uppercase; color: var(--muted); line-height: 16px; }
  .sec h2 .n { font-variant-numeric: tabular-nums; opacity: .8; margin-inline-start: 4px; }
  .sec h2 .n::before { content: '·'; margin-inline-end: 6px; }
  .sub2 { margin: 14px 4px 4px; font-size: 12px; font-weight: 650; color: var(--muted); line-height: 16px; }
  .card { border-radius: var(--r-ctl); background: var(--surface); box-shadow: inset 0 0 0 1px var(--hairline); overflow: hidden; }
  .act { display: flex; align-items: center; gap: 14px; inline-size: 100%; min-block-size: 60px; padding: 8px 16px; border: 0; background: none; color: var(--ink); text-align: start; font: inherit; cursor: pointer; }
  .card:global([data-shaped]), .link:global([data-shaped]) { box-shadow: none; }
  .act + .act { border-block-start: 1px solid var(--hairline); }
  .act > :global(svg) { color: var(--muted); }
  button.act:hover:not(:disabled) { background: var(--surface-2); }
  button.act:disabled { opacity: .55; cursor: default; }
  .act.danger b, .act.danger > :global(svg) { color: var(--danger); }
  .act.off { cursor: default; }
  .act.off b { color: var(--muted); }
  .row-sw { cursor: default; }
  .t { flex: 1; min-inline-size: 0; display: grid; gap: 1px; }
  .t b { font-weight: 600; line-height: 20px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .t small { font-size: var(--fs-sub); line-height: 18px; color: var(--muted); display: block; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .note { margin: 10px 4px 0; color: var(--muted); font-size: var(--fs-sub); line-height: 19px; }

  .links, .pend, .rows { list-style: none; margin: 0; padding: 0; }
  .links { margin-block-start: 10px; display: grid; gap: 8px; }
  .link { padding: 12px 14px; border-radius: var(--r-ctl); background: var(--surface); box-shadow: inset 0 0 0 1px var(--hairline); display: grid; gap: 4px; }
  .link.dead { opacity: .75; }
  .url { font: 13px/18px ui-monospace, 'SF Mono', Menlo, Consolas, monospace; overflow-wrap: anywhere; user-select: all; color: var(--ink); }
  .link.dead .url { text-decoration: line-through; color: var(--muted); }
  .lmeta { font-size: var(--fs-sub); color: var(--muted); line-height: 18px; }
  /* Copy / Copied share one box (the wider sets the size), so Cancel link beside it never slides. */
  .cp { display: grid; }
  .cp > span { grid-area: 1 / 1; display: inline-flex; align-items: center; justify-content: center; gap: 8px; }
  .cp > .b2, .cp.on > .a { visibility: hidden; }
  .cp.on > .b2 { visibility: visible; }
  .lacts { display: flex; gap: 4px; flex-wrap: wrap; margin-block-start: 6px; }
  .pend { margin-block-start: 6px; }
  .prow, .mrow { display: flex; align-items: center; gap: 12px; min-block-size: 60px; padding: 6px 4px; }
  .rows > li > .mrow, .rows > .mrow { inline-size: 100%; }
  /* a trailing text button lines its label up with the role pills' right edge */
  .prow > :global(.b.quiet:last-child), li.mrow > :global(.b.quiet:last-child) { margin-inline-end: -11px; font-weight: 500; }
  button.mrow { border: 0; border-radius: var(--r-ctl); background: none; color: var(--ink); text-align: start; font: inherit; cursor: pointer; padding-inline: 8px; margin-inline: -4px; inline-size: calc(100% + 8px); }
  button.mrow:hover { background: var(--surface-2); }
  .mrow.plain { opacity: .85; }
  .fc { color: var(--danger); font-weight: 600; transition: color var(--dur-enter) var(--ease-out); }
  .rq { color: var(--muted); }
  .marks { display: flex; align-items: center; gap: 4px; flex: none; }
  .mk { inline-size: 22px; block-size: 22px; border-radius: 50%; display: grid; place-items: center; background: var(--warn-bg); color: var(--warn-ink); }
  .mk + .mk { background: var(--surface-2); color: var(--ink); box-shadow: inset 0 0 0 1px var(--line); }

  .status { display: flex; align-items: flex-start; gap: 8px; margin: 0 4px 6px; font-size: var(--fs-sub); line-height: 19px; color: var(--ink); }
  .status.warn { color: var(--warn-ink); }
  .status .dot { margin-block-start: 6px; }
  .stk { display: grid; }
  .stk > span { grid-area: 1 / 1; visibility: hidden; }
  .stk > .cur { visibility: visible; }
  .dot { inline-size: 8px; block-size: 8px; border-radius: 50%; background: var(--muted); flex: none; transition: background-color var(--dur-enter) var(--ease-out); }
  .dot.on { background: var(--accent); }
  .dot.sm { display: inline-block; vertical-align: 0; margin-inline-end: 6px; }

  .ch-sw { margin-block-start: 8px; }
  .dlg { padding: 20px 20px 16px; }
  .dlg h2 { margin: 0 0 4px; font-size: var(--fs-title); font-weight: 650; line-height: 24px; text-transform: none; letter-spacing: 0; color: var(--ink); }
  .dlg p { margin: 8px 0 0; color: var(--muted); font-size: var(--fs-sub); line-height: 19px; }
  .acts { display: flex; gap: 8px; justify-content: flex-end; margin-block-start: 18px; }
</style>

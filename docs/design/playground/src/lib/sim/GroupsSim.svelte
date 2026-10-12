<script lang="ts">
  /* Groups simulation: people acting on me and on the group. Lives only in the drawer. */
  import Switch from '../ui/Switch.svelte';
  import { db, displayName, firstName, isGroup } from '../model/store.svelte';
  import { gx } from '../model/actions/groups.svelte';
  import {
    acceptHosting, answerRequest, group, inviteeDeclines, inviteeJoins, isAdminRole, member, receiveInvitation, receiveRequest, setMyRole, simulateFlag,
    simulatePost, simulateRemoval,
  } from '../model/actions/groups';
  import type { Group, GroupRole } from '../model/types';

  let gid = $state('hike');
  const groups = $derived(db.chats.filter(isGroup).filter((g) => g.status === 'active'));
  const g = $derived<Group | undefined>(group(gid) ?? groups[0]);
  const admins = $derived(g ? g.members.filter((m) => m.id !== 'me' && isAdminRole(m.role)) : []);
  const targets = $derived(g ? g.members.filter((m) => m.role !== 'owner') : []);
  let by = $state('');
  let target = $state('');
  const byId = $derived(admins.find((a) => a.id === by)?.id ?? admins[0]?.id ?? '');
  const targetId = $derived(targets.find((t) => t.id === target)?.id ?? targets.find((t) => t.id !== byId)?.id ?? '');
  const POSTS = ['Meeting at the trailhead at 7.', 'Who has the tent?', 'Route is up on the map.', 'Running a few minutes late.'];
  let n = 0;
  const post = () => g && simulatePost(g, g.members.find((m) => m.id !== 'me')!.id, POSTS[n++ % POSTS.length]);
  const asRole = (r: GroupRole) => g && setMyRole(g, r);
  const pendingHost = $derived(g ? g.members.filter((m) => m.hostPending) : []);
</script>

<div class="rows">
  <label class="row"><span>Group</span>
    <select bind:value={gid} aria-label="Group to act on">{#each groups as x (x.id)}<option value={x.id}>{x.name}</option>{/each}</select></label>
  <label class="row"><span>Hosts online</span><Switch label="Hosts online for groups" bind:checked={db.env.hostsOnline} /></label>

  <h4>Me</h4>
  <div class="btns">
    <button type="button" onclick={() => asRole('owner')}>Make me owner</button>
    <button type="button" onclick={() => asRole('admin')}>Make me admin</button>
    <button type="button" onclick={() => asRole('member')}>Make me member</button>
    <button type="button" onclick={() => g && simulateRemoval(g, admins.map((a) => a.id))}>I get removed</button>
  </div>

  <h4>Someone invites me</h4>
  <div class="btns">
    <button type="button" onclick={() => receiveInvitation('priya', 'Trek planning')}>Priya invites me to Trek planning</button>
    <button type="button" onclick={() => receiveInvitation('sam', 'Cycling crew')}>Sam invites me to Cycling crew</button>
  </div>

  <h4>Chat requests</h4>
  <div class="btns">
    <button type="button" onclick={() => receiveRequest('kavya', 'hike', 'Hi, it’s Kavya. Can I ask about the lift?')}>Kavya sends me a request</button>
    <button type="button" onclick={() => receiveRequest('nisha', 'hike', '')}>Nisha sends me a request</button>
    <button type="button" onclick={() => receiveRequest('ravi', 'hike', 'Hello')}>Ravi (blocked) sends a request</button>
  </div>
  {#each gx.requestsOut.filter((r) => r.state === 'pending') as r (r.id)}
    <div class="sub">I asked {firstName(r.to)}:</div>
    <div class="btns">
      <button type="button" onclick={() => answerRequest(r.id, 'accepted')}>{firstName(r.to)} accepts</button>
      <button type="button" onclick={() => answerRequest(r.id, 'declined')}>{firstName(r.to)} declines</button>
      <button type="button" onclick={() => answerRequest(r.id, 'blocked')}>{firstName(r.to)} has blocked me</button>
    </div>
  {/each}

  <h4>Admin flags a member</h4>
  {#if g && admins.length}
    <label class="row"><span>Flagged by</span>
      <select value={byId} onchange={(e) => (by = e.currentTarget.value)} aria-label="Flagged by">{#each admins as a (a.id)}<option value={a.id}>{displayName(a.id)}</option>{/each}</select></label>
    <label class="row"><span>Member</span>
      <select value={targetId} onchange={(e) => (target = e.currentTarget.value)} aria-label="Flag who">{#each targets as t (t.id)}<option value={t.id}>{displayName(t.id)}</option>{/each}</select></label>
    <div class="btns"><button type="button" onclick={() => g && targetId && (targetId === 'me' ? simulateFlag(g, byId, 'me') : simulateFlag(g, byId, targetId))}>Flag</button></div>
  {/if}

  <h4>Posts and hosting</h4>
  <div class="btns">
    <button type="button" onclick={post}>A post arrives</button>
    {#if g && member(g, 'me')}<button type="button" onclick={() => { const me = member(g, 'me'); if (me && !me.host) me.hostPending = true; }}>Owner asks my device to host</button>{/if}
    {#each pendingHost.filter((m) => m.id !== 'me') as m (m.id)}
      <button type="button" onclick={() => g && acceptHosting(g, m.id)}>{firstName(m.id)} accepts hosting</button>
    {/each}
    {#each g?.invited ?? [] as inv (inv.id)}
      <button type="button" onclick={() => g && inviteeJoins(g, inv.id)}>{firstName(inv.id)} joins</button>
      <button type="button" onclick={() => g && inviteeDeclines(g, inv.id)}>{firstName(inv.id)} declines</button>
    {/each}
  </div>

  <div class="notes">
    <p><b>Proposed, not decided.</b></p>
    <ul>
      <li>A flagged member sees each flag (who and when) as it happens, not only after removal. Decided 2026-10-12.</li>
      <li>The owner leaving. Here the owner can't leave (no way to hand over ownership yet); admins and members can.</li>
      <li>PROVISIONAL (core): the creating device is the group's first host.</li>
      <li>PROVISIONAL (core): invites and short links are bearer grants valid 7 days; a link names one host; cancelling does not recall a link someone already opened.</li>
      <li>Chat requests (decided 2026-10-12, like Instagram): one request per person at a time; no follow-ups until they answer. Decided 2026-10-12: after a decline, or after you cancel, you can ask that person again only after 7 days. Proposed: unanswered requests expire after 7 days. Core follow-up: one pending request per person plus a per-person cooldown.</li>
      <li>PROVISIONAL (core): a declined request shows "declined" to the requester.</li>
      <li>Chat hosts and call hosts are separate permits (U25); a call host appoints and accepts like a chat host, and can pause hosting on its own device.</li>
      <li>Playground only: host online state is one switch for all hosts.</li>
      <li>Posts a removed member wrote before removal (U11) are not modelled.</li>
    </ul>
  </div>
</div>

<style>
  .rows { display: grid; gap: 4px; }
  .row { display: flex; align-items: center; justify-content: space-between; gap: 12px; min-height: 40px; font-size: var(--fs-sub); }
  h4 { margin: 12px 0 2px; font-size: var(--fs-sub); color: var(--muted); font-weight: 650; }
  .sub { font-size: var(--fs-sub); color: var(--muted); margin-top: 6px; }
  .btns { display: flex; flex-wrap: wrap; gap: 6px; margin-top: 4px; }
  button:not([role]) { border: 1px solid var(--line); background: var(--surface); border-radius: 10px; padding: 6px 10px; font-size: var(--fs-sub); cursor: pointer; }
  select { border: 1px solid var(--line); background: var(--surface); border-radius: 10px; padding: 6px 8px; font-size: var(--fs-sub); max-width: 60%; }
  .notes { margin-top: 12px; font-size: var(--fs-sub); color: var(--muted); }
  .notes p { margin: 0 0 4px; }
  .notes ul { margin: 0; padding-inline-start: 18px; display: grid; gap: 3px; }
</style>

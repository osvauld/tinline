/* The playground's data model. One store holds all of it (store.svelte.ts); screens never copy it.
   Times are sim-clock milliseconds (clock.svelte.ts), never display strings, so sorting and
   "Yesterday"/"12:41" formatting are derived, not stored. */

export type Id = string;

/** One profile per account DID (profile-sync). `fields` holds status, about, availability and every additional
    field; keys this version doesn't know are kept as they are. `picture` is the descriptor the avatar reads:
    ready = the bytes are cached on this device. The name stays on Person/Account (one spelling everywhere). */
export interface Profile {
  fields: Record<string, unknown>;
  picture?: { hash: string; ready: boolean; content?: string }; // content = normalized-content hash (same picture again is a no-op)
}

export interface Person {
  id: Id;
  name: string;            // full display name, one spelling everywhere
  hue: number;             // avatar pair 0..5 (tokens --avN-*)
  online: boolean;         // reachable right now (simulated)
  callsAllowed: boolean;   // "Allow calls" (message-only when false)
  blocked: boolean;        // I blocked them (visible block, U5)
  contact: boolean;        // in my contacts (vs. a group member I only know through a group)
  profile?: Profile;       // their account profile as last synced; absent = none accessible to me
  alias?: string;          // MY local name for them; never sent, never replaces their own name
}

export type Delivery = 'sending' | 'waiting' | 'delivered' | 'failed';

export interface FileInfo {
  name: string;
  size: number;            // bytes
  kind: 'image' | 'doc' | 'video' | 'other';
  progress: number;        // 0..1
  state: 'sending' | 'waiting' | 'done' | 'failed' | 'cancelled';
  /** Pictures (core chat/5): the size it is shown at; the bubble draws its true shape before it downloads. */
  w?: number;
  h?: number;
  /** An incoming picture not downloaded yet (over the auto-download limit): its box shows the colour preview. */
  remote?: boolean;
}

export interface CallRecord {
  kind: 'voice' | 'video';
  outcome: 'answered' | 'missed' | 'declined' | 'failed' | 'cancelled';
  durationS: number;
  direction: 'out' | 'in';
  group?: boolean;         // a group call (step 6): shown "Group call · 12 min · 3 people"; the pill joins / starts a call in this group
  /** Group calls: how many people were in it at most (me included when I was). Missed = it happened without me. */
  people?: number;
}

export interface Message {
  id: Id;
  from: Id;                // person id, or 'me'
  at: number;              // sim-clock ms
  text: string;
  delivery?: Delivery;     // mine only
  replyTo?: Id;
  edited?: boolean;        // 1:1 only; group posts cannot be edited (core: Post{text, reply_to})
  editedAt?: number;       // sim-clock ms of the last edit
  deliveredAt?: number;    // sim-clock ms when delivery was confirmed
  deleted?: boolean;       // 1:1 only
  file?: FileInfo;         // 1:1 only
  call?: CallRecord;       // a call left in the thread
  system?: boolean;        // "Kavya joined, invited by Priya"
}

export interface DirectChat {
  kind: 'direct';
  id: Id;                  // === person id
  person: Id;
  messages: Message[];
  unread: number;
  muted: boolean;
  draft: string;
}

export type GroupRole = 'owner' | 'admin' | 'member';

export interface GroupMember {
  id: Id;                  // person id or 'me'
  role: GroupRole;
  host: boolean;           // relays/stores posts for the group (chat host)
  callHost: boolean;       // holds a call-host permit (separate from chat host, U25)
  since: number;
  invitedBy?: Id;
  hostPending?: boolean;   // groups step: hosting is opt-in; appointed, waiting for that device to accept
  callHostPending?: boolean; // step 6: a call-host permit is out, waiting for that device to accept (own accept, U25)
  callHostPaused?: boolean;  // step 6: this device accepted but switched call hosting off (group_call_hosting_enable)
}

export interface Group {
  kind: 'group';
  id: Id;
  name: string;
  status: 'active' | 'removed' | 'left';
  members: GroupMember[];
  invited: { id: Id; expiresAt: number }[];
  flags: Record<Id, Id[]>; // member → who flagged them (3 flags, any source, removes, U15)
  messages: Message[];
  unread: number;
  muted: boolean;
  draft: string;
  flagAt?: Record<Id, number[]>;   // groups step: when each flag was made, parallel to `flags`
  links?: { code: string; host: Id; expiresAt: number }[];   // groups step: short invite links (bearer, 7 days, PROVISIONAL)
  removedAt?: number;      // groups step: when I was removed
}

export type Chat = DirectChat | Group;

export interface Device {
  id: Id;
  name: string;
  kind: 'phone' | 'desktop';
  self: boolean;
  lastSeen: number;
}

export interface Account {
  id: Id;
  name: string;
  hue: number;
  availability: 'available' | 'busy' | 'offline';
  profile?: Profile;       // my account's profile (editable); availability lives in profile.fields
  /** Before the app proper: 'onboarding' (no account on this device), 'locked' (needs passphrase), 'ready'. */
  gate: 'onboarding' | 'locked' | 'ready';
}

/** Another name (identity) on this install, beside the open one (2026-10-14: several names online at once).
 *  Each name is its own address with its own contacts and chats; `data` holds them while another name is open.
 *  online = its endpoint runs, so its calls and messages come through; offline = keys kept, nothing reaches it here.
 *  Safe limits (Proposed): phone 3 online, desktop 5. */
export interface OtherName {
  id: Id;                  // stable per name (the avatar fingerprint reads it)
  name: string;
  hue: number;
  profile?: Profile;
  online: boolean;
  remembered: boolean;     // opens without its passphrase (phone key store / desktop keyring)
  data: { people: Person[]; chats: Chat[]; devices: Device[] };
}

export type FormFactor = 'android' | 'desktop';
export type Theme = 'system' | 'light' | 'dark';

/* One wording for people notifications, on the phone and in the desktop toast (calm voice: docs/design/VOICE.md). */
import type { Social } from '../model/social';

export function socialText(n: Social): { title: string; text: string; act: string } {
  switch (n.kind) {
    case 'request': return { title: 'Contact request', text: `${n.name} would like to add you`, act: 'Open' };
    case 'accepted': return { title: n.name, text: 'Accepted your request. Your line is open.', act: 'Open' };
    case 'invite': return { title: n.groupName, text: `${n.byName} invited you to the group`, act: 'See invitation' };
  }
}

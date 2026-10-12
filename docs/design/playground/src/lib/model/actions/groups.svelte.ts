/* Groups step: state that is not part of a Group record itself (things that arrive from OTHER people:
   invitations to me, chat requests in both directions). Kept in one $state object here because the
   shared store is lead-owned; screens read `gx` directly and never copy it. */
import type { Id } from '../types';

export interface Invitation { id: Id; by: Id; groupName: string; at: number; expiresAt: number; hostName: string }
export interface RequestIn { id: Id; from: Id; groupId: Id; note: string; at: number }
export type RequestState = 'pending' | 'declined' | 'blocked' | 'accepted';
export interface RequestOut { id: Id; to: Id; groupId: Id; note: string; at: number; state: RequestState }

export const gx = $state({
  invitations: [] as Invitation[],
  requestsIn: [] as RequestIn[],
  requestsOut: [] as RequestOut[],
});

export function resetGroupsState() {
  gx.invitations = []; gx.requestsIn = []; gx.requestsOut = [];
}

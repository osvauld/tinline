/* Shared spring overrides for the mark (params of the one tinmark model, see MARK-MATH.md).
   LINE_SPRING: the "line opens" pull. Two tugs in the rhythm of the `line` cue (two plucks), so sound and
   motion stay locked: the gap is read from the cue itself. Used by sign-up Done and by the empty state when
   the first contact arrives. */
import { audio } from '../fx/audio';

const cue = audio.pattern('line').notes;
const gap = cue.length > 1 ? cue[1][1] : 0.12;
export const LINE_SPRING = { tugs: 2, recoil: 0.95, recoilMin: 0, settleHz: 1 / (2 * gap) };

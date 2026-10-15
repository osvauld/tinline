/* Simulation drawer sections. Each area registers ONE component here (append a line);
   the drawer renders them in order under their titles. Sim controls never appear inside the app frame. */
import type { Component } from 'svelte';
import CoreSim from './CoreSim.svelte';
import ChatSim from './ChatSim.svelte';
import GateSim from './GateSim.svelte';
import CallsSim from './CallsSim.svelte';
import VideoSim from './VideoSim.svelte';
import GroupsSim from './GroupsSim.svelte';
import GroupCallSim from './GroupCallSim.svelte';
import ProfileSim from './ProfileSim.svelte';
import SearchSim from './SearchSim.svelte';
import PeopleSim from './PeopleSim.svelte';
import NamesSim from './NamesSim.svelte';

export const simSections: { title: string; component: Component }[] = [
  { title: 'Environment', component: CoreSim },
  { title: 'Chat', component: ChatSim },
  { title: 'Sign-up and unlock', component: GateSim },
  { title: 'Names on this device', component: NamesSim },
  { title: 'Calls', component: CallsSim },
  { title: 'Video calls', component: VideoSim },
  { title: 'Groups', component: GroupsSim },
  { title: 'Group calls', component: GroupCallSim },
  { title: 'Profiles', component: ProfileSim },
  { title: 'Search', component: SearchSim },
  { title: 'People and devices', component: PeopleSim },
];

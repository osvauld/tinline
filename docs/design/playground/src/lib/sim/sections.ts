/* Simulation drawer sections. Each area registers ONE component here (append a line);
   the drawer renders them in order under their titles. Sim controls never appear inside the app frame. */
import type { Component } from 'svelte';
import CoreSim from './CoreSim.svelte';
import ChatSim from './ChatSim.svelte';
import GateSim from './GateSim.svelte';
import CallsSim from './CallsSim.svelte';
import VideoSim from './VideoSim.svelte';
import GroupsSim from './GroupsSim.svelte';
import ProfileSim from './ProfileSim.svelte';
import SearchSim from './SearchSim.svelte';

export const simSections: { title: string; component: Component }[] = [
  { title: 'Environment', component: CoreSim },
  { title: 'Chat', component: ChatSim },
  { title: 'Sign-up and unlock', component: GateSim },
  { title: 'Calls', component: CallsSim },
  { title: 'Video calls', component: VideoSim },
  { title: 'Groups', component: GroupsSim },
  { title: 'Profiles', component: ProfileSim },
  { title: 'Search', component: SearchSim },
];

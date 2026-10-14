// Entry for icons.html: mounts the icon lab on its own page (no audio, no gate), so main.ts is untouched.
import { mount } from 'svelte';
import '../../app.css';
import IconLab from './IconLab.svelte';

export default mount(IconLab, { target: document.getElementById('app')! });

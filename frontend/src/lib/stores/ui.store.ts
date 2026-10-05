import { writable } from 'svelte/store';

/** Whether the workspace sidebar drawer is open on small screens (always visible from `md`). */
export const sidebarOpen = writable(false);

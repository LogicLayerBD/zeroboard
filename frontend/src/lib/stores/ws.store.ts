import { writable } from 'svelte/store';

export type ConnectionState = 'idle' | 'connecting' | 'open' | 'reconnecting';

export const connectionState = writable<ConnectionState>('idle');
/** User ids with the current board open (from PRESENCE_UPDATE). */
export const activeUsers = writable<string[]>([]);

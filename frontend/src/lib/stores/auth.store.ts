import { writable } from 'svelte/store';
import * as api from '$lib/api';
import type { User } from '$lib/types';

export const currentUser = writable<User | null>(null);
/** False until the initial session check finishes; guards must wait for it. */
export const authReady = writable(false);

/** Restores the session from the refresh cookie, if there is one. */
export async function initAuth(): Promise<void> {
	try {
		if ((await api.refreshAccessToken()) === 'refreshed') {
			currentUser.set(await api.me());
		}
	} catch {
		api.clearAccessToken();
		currentUser.set(null);
	} finally {
		authReady.set(true);
	}
}

export async function signIn(email: string, password: string): Promise<void> {
	const session = await api.login(email, password);
	currentUser.set(session.user);
}

export async function signOut(): Promise<void> {
	try {
		await api.logout();
	} catch {
		// The server-side session may already be gone; clear local state regardless.
	}
	endSession();
}

/** Drops local session state without contacting the server. */
export function endSession(): void {
	api.clearAccessToken();
	currentUser.set(null);
}

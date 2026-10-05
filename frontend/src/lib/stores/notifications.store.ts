import { derived, writable } from 'svelte/store';
import * as api from '$lib/api';
import type { Notification, NotificationPayload } from '$lib/types';
import { toastError } from './toast.store';

export const notifications = writable<Notification[]>([]);
export const unreadCount = derived(notifications, (all) => all.filter((n) => !n.read).length);

export async function loadNotifications(): Promise<void> {
	try {
		notifications.set(await api.listNotifications());
	} catch (err) {
		toastError(err);
	}
}

export function addNotification(notification: Notification): void {
	notifications.update((all) =>
		all.some((n) => n.id === notification.id) ? all : [notification, ...all]
	);
}

export async function markRead(id: string): Promise<void> {
	try {
		const updated = await api.markNotificationRead(id);
		notifications.update((all) => all.map((n) => (n.id === id ? updated : n)));
	} catch (err) {
		toastError(err);
	}
}

export async function markAllRead(): Promise<void> {
	try {
		await api.markAllNotificationsRead();
		notifications.update((all) => all.map((n) => ({ ...n, read: true })));
	} catch (err) {
		toastError(err);
	}
}

export function clearNotifications(): void {
	notifications.set([]);
}

export function parsePayload(notification: Notification): NotificationPayload | null {
	try {
		const value: unknown = JSON.parse(notification.payload);
		if (
			value &&
			typeof value === 'object' &&
			'card_id' in value &&
			'board_id' in value &&
			'message' in value &&
			typeof value.card_id === 'string' &&
			typeof value.board_id === 'string' &&
			typeof value.message === 'string'
		) {
			return { card_id: value.card_id, board_id: value.board_id, message: value.message };
		}
	} catch {
		// Malformed payload; shown with a generic message.
	}
	return null;
}

import { writable } from 'svelte/store';
import { errorMessage } from '$lib/api';

const TOAST_DURATION_MS = 4000;

export type ToastKind = 'success' | 'error';

export interface Toast {
	id: number;
	kind: ToastKind;
	message: string;
}

let nextId = 0;

export const toasts = writable<Toast[]>([]);

export function dismissToast(id: number): void {
	toasts.update((all) => all.filter((t) => t.id !== id));
}

function push(kind: ToastKind, message: string): void {
	const id = nextId++;
	toasts.update((all) => [...all, { id, kind, message }]);
	setTimeout(() => dismissToast(id), TOAST_DURATION_MS);
}

export function toastSuccess(message: string): void {
	push('success', message);
}

/** Shows a safe message for `err`; raw server errors never reach the UI. */
export function toastError(err: unknown): void {
	push('error', errorMessage(err));
}

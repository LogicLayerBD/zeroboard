<script lang="ts">
	import { goto } from '$app/navigation';
	import * as api from '$lib/api';
	import { formatDateTime } from '$lib/format';
	import {
		markAllRead,
		markRead,
		notifications,
		parsePayload,
		unreadCount
	} from '$lib/stores/notifications.store';
	import { toastError } from '$lib/stores/toast.store';
	import type { Notification } from '$lib/types';

	const MAX_BADGE_COUNT = 9;

	let open = $state(false);
	let container: HTMLDivElement | undefined = $state();

	const badge = $derived($unreadCount > MAX_BADGE_COUNT ? `${MAX_BADGE_COUNT}+` : `${$unreadCount}`);

	function onWindowClick(event: MouseEvent) {
		if (open && container && !container.contains(event.target as Node)) open = false;
	}

	async function openNotification(notification: Notification) {
		open = false;
		if (!notification.read) void markRead(notification.id);
		const payload = parsePayload(notification);
		if (!payload) return;
		try {
			const board = await api.getBoard(payload.board_id);
			if (!board.workspace_id) return;
			await goto(
				`/${encodeURIComponent(board.workspace_id)}/${encodeURIComponent(board.id)}?card=${encodeURIComponent(payload.card_id)}`
			);
		} catch (err) {
			toastError(err);
		}
	}
</script>

<svelte:window onclick={onWindowClick} />

<div class="relative" bind:this={container}>
	<button
		type="button"
		class="relative rounded-full p-2 text-slate-600 hover:bg-slate-100"
		aria-label="Notifications"
		aria-expanded={open}
		onclick={() => (open = !open)}
	>
		<svg class="h-5 w-5" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
			<path d="M18 8a6 6 0 0 0-12 0c0 7-3 9-3 9h18s-3-2-3-9" />
			<path d="M13.73 21a2 2 0 0 1-3.46 0" />
		</svg>
		{#if $unreadCount > 0}
			<span
				class="absolute -right-0.5 -top-0.5 flex h-5 min-w-5 items-center justify-center rounded-full bg-red-600 px-1 text-xs font-semibold text-white"
				>{badge}</span
			>
		{/if}
	</button>

	{#if open}
		<div class="absolute right-0 z-30 mt-2 w-80 rounded-lg border border-slate-200 bg-white shadow-xl">
			<div class="flex items-center justify-between border-b border-slate-100 px-4 py-2">
				<h2 class="text-sm font-semibold">Notifications</h2>
				{#if $unreadCount > 0}
					<button type="button" class="text-xs text-indigo-600 hover:underline" onclick={markAllRead}
						>Mark all read</button
					>
				{/if}
			</div>
			<ul class="max-h-96 overflow-y-auto">
				{#each $notifications as notification (notification.id)}
					<li>
						<button
							type="button"
							class="flex w-full gap-3 px-4 py-3 text-left text-sm hover:bg-slate-50"
							onclick={() => openNotification(notification)}
						>
							<span
								class="mt-1.5 h-2 w-2 shrink-0 rounded-full {notification.read
									? 'bg-transparent'
									: 'bg-indigo-600'}"
							></span>
							<span class="flex-1">
								<span class="block {notification.read ? 'text-slate-500' : 'text-slate-900'}">
									{parsePayload(notification)?.message ?? 'You have a new notification'}
								</span>
								<span class="text-xs text-slate-400">{formatDateTime(notification.created_at)}</span>
							</span>
						</button>
					</li>
				{:else}
					<li class="px-4 py-6 text-center text-sm text-slate-500">You're all caught up.</li>
				{/each}
			</ul>
		</div>
	{/if}
</div>

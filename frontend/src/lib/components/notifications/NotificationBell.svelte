<script lang="ts">
	import { goto } from '$app/navigation';
	import * as api from '$lib/api';
	import { formatDateTime } from '$lib/format';
	import Icon from '$lib/components/ui/Icon.svelte';
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
		class="icon-btn relative p-2 text-slate-500 dark:text-slate-400 {open ? 'bg-slate-100 dark:bg-slate-800 text-slate-800 dark:text-slate-100' : ''}"
		aria-label="Notifications"
		aria-expanded={open}
		onclick={() => (open = !open)}
	>
		<Icon name="bell" class="h-5 w-5" />
		{#if $unreadCount > 0}
			<span
				class="absolute -right-0.5 -top-0.5 flex h-[18px] min-w-[18px] items-center justify-center rounded-full bg-gradient-to-br from-rose-500 to-red-600 px-1 text-[10px] font-bold text-white ring-2 ring-white dark:ring-slate-900"
				>{badge}</span
			>
		{/if}
	</button>

	{#if open}
		<div
			class="fixed inset-x-2 top-16 z-30 overflow-hidden rounded-2xl sm:absolute sm:inset-x-auto sm:right-0 sm:top-full sm:mt-2 sm:w-96 bg-white dark:bg-slate-900 shadow-lift ring-1 ring-slate-900/10 dark:ring-white/10"
		>
			<div class="flex items-center justify-between border-b border-slate-100 dark:border-slate-800 px-4 py-3">
				<h2 class="text-sm font-semibold text-slate-900 dark:text-white">Notifications</h2>
				{#if $unreadCount > 0}
					<button type="button" class="link text-xs" onclick={markAllRead}
						>Mark all read</button
					>
				{/if}
			</div>
			<ul class="max-h-96 overflow-y-auto">
				{#each $notifications as notification (notification.id)}
					<li>
						<button
							type="button"
							class="flex w-full gap-3 px-4 py-3 text-left text-sm transition hover:bg-slate-50 dark:hover:bg-slate-800/60 {notification.read
								? ''
								: 'bg-indigo-50/40 dark:bg-indigo-500/10'}"
							onclick={() => openNotification(notification)}
						>
							<span
								class="mt-1.5 h-2 w-2 shrink-0 rounded-full {notification.read
									? 'bg-transparent'
									: 'bg-indigo-600'}"
							></span>
							<span class="flex-1">
								<span class="block {notification.read ? 'text-slate-500 dark:text-slate-400' : 'text-slate-900 dark:text-white'}">
									{parsePayload(notification)?.message ?? 'You have a new notification'}
								</span>
								<span class="text-xs text-slate-400 dark:text-slate-500">{formatDateTime(notification.created_at)}</span>
							</span>
						</button>
					</li>
				{:else}
					<li class="px-4 py-10 text-center text-sm text-slate-500 dark:text-slate-400">
						<Icon name="check" class="mx-auto mb-2 h-6 w-6 text-emerald-500" />
						You're all caught up.
					</li>
				{/each}
			</ul>
		</div>
	{/if}
</div>

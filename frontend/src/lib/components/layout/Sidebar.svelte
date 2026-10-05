<script lang="ts">
	import { onDestroy } from 'svelte';
	import { fade } from 'svelte/transition';
	import { afterNavigate, goto } from '$app/navigation';
	import { page } from '$app/state';
	import * as api from '$lib/api';
	import { accentFor } from '$lib/colors';
	import Icon from '$lib/components/ui/Icon.svelte';
	import { toastError } from '$lib/stores/toast.store';
	import { sidebarOpen } from '$lib/stores/ui.store';
	import {
		boards,
		currentWorkspace,
		hasRole,
		myRole,
		upsertBoard,
		workspaceId
	} from '$lib/stores/workspace.store';

	const BACKDROP_FADE_MS = 150;

	let newBoardName = $state('');
	let creating = $state(false);

	const canCreate = $derived(hasRole($myRole, 'member'));
	const workspaceName = $derived($currentWorkspace?.name ?? 'Workspace');

	afterNavigate(() => sidebarOpen.set(false));
	onDestroy(() => sidebarOpen.set(false));

	async function createBoard(event: SubmitEvent) {
		event.preventDefault();
		const name = newBoardName.trim();
		const wsId = $workspaceId;
		if (!name || !wsId) return;
		creating = true;
		try {
			const board = await api.createBoard(wsId, name);
			upsertBoard(board);
			newBoardName = '';
			await goto(`/${encodeURIComponent(wsId)}/${encodeURIComponent(board.id)}`);
		} catch (err) {
			toastError(err);
		} finally {
			creating = false;
		}
	}
</script>

{#if $sidebarOpen}
	<button
		type="button"
		class="fixed inset-0 z-30 cursor-default bg-slate-900/50 backdrop-blur-sm md:hidden"
		aria-label="Close sidebar"
		onclick={() => sidebarOpen.set(false)}
		transition:fade={{ duration: BACKDROP_FADE_MS }}
	></button>
{/if}

<aside
	class="fixed inset-y-0 left-0 z-40 flex w-64 shrink-0 flex-col bg-slate-900 text-slate-300 shadow-2xl transition-transform duration-200 dark:border-r dark:border-white/5 md:static md:z-auto md:translate-x-0 md:shadow-none {$sidebarOpen
		? 'translate-x-0'
		: '-translate-x-full'}"
>
	<a
		href="/{encodeURIComponent($workspaceId ?? '')}"
		class="m-3 flex items-center gap-3 rounded-xl p-2 transition hover:bg-white/5 {page.params
			.boardId
			? ''
			: 'bg-white/10'}"
	>
		<span
			class="flex h-9 w-9 shrink-0 items-center justify-center rounded-lg text-sm font-bold text-white shadow-inner"
			style:background-color={accentFor($workspaceId ?? '')}
		>
			{workspaceName.charAt(0).toUpperCase()}
		</span>
		<span class="min-w-0">
			<span class="block truncate text-sm font-semibold text-white">{workspaceName}</span>
			<span class="block text-xs capitalize text-slate-400">{$myRole ?? 'member'}</span>
		</span>
	</a>

	<nav class="flex-1 overflow-y-auto px-3 pb-3">
		<h3 class="px-2 pb-2 pt-3 text-[11px] font-semibold uppercase tracking-wider text-slate-500">
			Boards
		</h3>
		<ul class="space-y-0.5">
			{#each $boards as board (board.id)}
				{@const active = page.params.boardId === board.id}
				<li>
					<a
						href="/{encodeURIComponent($workspaceId ?? '')}/{encodeURIComponent(board.id)}"
						class="group flex items-center gap-2.5 rounded-lg px-2 py-1.5 text-sm transition {active
							? 'bg-white/10 font-medium text-white'
							: 'text-slate-300 hover:bg-white/5 hover:text-white'}"
					>
						<span
							class="h-2.5 w-2.5 shrink-0 rounded-[3px] {active ? '' : 'opacity-70 group-hover:opacity-100'}"
							style:background-color={accentFor(board.id)}
						></span>
						<span class="truncate">{board.name}</span>
					</a>
				</li>
			{:else}
				<li class="px-2 py-1.5 text-sm text-slate-500">No boards yet</li>
			{/each}
		</ul>
	</nav>

	{#if canCreate}
		<form class="border-t border-white/10 p-3" onsubmit={createBoard}>
			<div class="relative">
				<Icon
					name="plus"
					class="pointer-events-none absolute left-2.5 top-1/2 h-4 w-4 -translate-y-1/2 text-slate-500"
				/>
				<input
					class="block w-full rounded-lg border-0 bg-white/5 py-2 pl-8 pr-3 text-sm text-white ring-1 ring-inset ring-white/10 transition placeholder:text-slate-500 focus:bg-white/10 focus:outline-none focus:ring-2 focus:ring-indigo-400"
					placeholder="New board"
					maxlength="255"
					bind:value={newBoardName}
					disabled={creating}
				/>
			</div>
		</form>
	{/if}
</aside>

<script lang="ts">
	import { goto } from '$app/navigation';
	import { page } from '$app/state';
	import * as api from '$lib/api';
	import { toastError } from '$lib/stores/toast.store';
	import {
		boards,
		currentWorkspace,
		hasRole,
		myRole,
		upsertBoard,
		workspaceId
	} from '$lib/stores/workspace.store';

	let newBoardName = $state('');
	let creating = $state(false);

	const canCreate = $derived(hasRole($myRole, 'member'));

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

<aside class="flex w-60 shrink-0 flex-col border-r border-slate-200 bg-white">
	<a
		href="/{encodeURIComponent($workspaceId ?? '')}"
		class="block truncate border-b border-slate-100 px-4 py-3 font-semibold hover:bg-slate-50"
	>
		{$currentWorkspace?.name ?? 'Workspace'}
	</a>
	<nav class="flex-1 overflow-y-auto p-2">
		<h3 class="px-2 pb-1 pt-2 text-xs font-semibold uppercase tracking-wide text-slate-400">Boards</h3>
		<ul class="space-y-0.5">
			{#each $boards as board (board.id)}
				<li>
					<a
						href="/{encodeURIComponent($workspaceId ?? '')}/{encodeURIComponent(board.id)}"
						class="block truncate rounded-md px-2 py-1.5 text-sm {page.params.boardId === board.id
							? 'bg-indigo-50 font-medium text-indigo-700'
							: 'text-slate-700 hover:bg-slate-100'}"
					>
						{board.name}
					</a>
				</li>
			{:else}
				<li class="px-2 py-1.5 text-sm text-slate-400">No boards yet</li>
			{/each}
		</ul>
	</nav>
	{#if canCreate}
		<form class="border-t border-slate-100 p-3" onsubmit={createBoard}>
			<input
				class="w-full rounded-md border border-slate-300 px-2 py-1.5 text-sm focus:border-indigo-500 focus:outline-none"
				placeholder="New board name"
				maxlength="255"
				bind:value={newBoardName}
				disabled={creating}
			/>
		</form>
	{/if}
</aside>

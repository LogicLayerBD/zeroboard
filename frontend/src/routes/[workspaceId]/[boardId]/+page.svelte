<script lang="ts">
	import { onDestroy, onMount } from 'svelte';
	import { goto } from '$app/navigation';
	import { page } from '$app/state';
	import * as api from '$lib/api';
	import KanbanBoard from '$lib/components/board/KanbanBoard.svelte';
	import ListView from '$lib/components/board/ListView.svelte';
	import CardModal from '$lib/components/card/CardModal.svelte';
	import Avatar from '$lib/components/ui/Avatar.svelte';
	import Spinner from '$lib/components/ui/Spinner.svelte';
	import { board, boardLoading, closeBoard, loadBoard } from '$lib/stores/board.store';
	import { toastError } from '$lib/stores/toast.store';
	import {
		hasRole,
		members,
		myRole,
		removeBoardLocally,
		upsertBoard
	} from '$lib/stores/workspace.store';
	import { activeUsers } from '$lib/stores/ws.store';
	import { joinBoard, leaveBoard, setBoardAccessLostHandler } from '$lib/ws';

	const MAX_NAME_CHARS = 255;
	const CARD_QUERY_PARAM = 'card';

	type View = 'kanban' | 'table';

	let view = $state<View>('kanban');
	let renaming = $state(false);
	let nameDraft = $state('');

	const boardId = $derived(page.params.boardId);
	const workspaceId = $derived(page.params.workspaceId);
	const openCardId = $derived(page.url.searchParams.get(CARD_QUERY_PARAM));
	const canEdit = $derived(hasRole($myRole, 'member'));
	const isAdmin = $derived($myRole === 'admin');
	const viewers = $derived($members.filter((m) => $activeUsers.includes(m.user_id)));

	$effect(() => {
		const id = boardId;
		if (!id) return;
		void loadBoard(id);
		joinBoard(id);
		return () => leaveBoard(id);
	});

	onMount(() => {
		setBoardAccessLostHandler(() => {
			toastError(new api.ApiError(403, ''));
			void goto('/');
		});
	});

	onDestroy(closeBoard);

	function boardPath(): string {
		return `/${encodeURIComponent(workspaceId ?? '')}/${encodeURIComponent(boardId ?? '')}`;
	}

	function openCard(cardId: string) {
		void goto(`${boardPath()}?${CARD_QUERY_PARAM}=${encodeURIComponent(cardId)}`, {
			noScroll: true,
			keepFocus: true
		});
	}

	function closeCard() {
		void goto(boardPath(), { noScroll: true, keepFocus: true });
	}

	function startRename() {
		nameDraft = $board?.name ?? '';
		renaming = true;
	}

	async function saveRename(event: SubmitEvent) {
		event.preventDefault();
		const current = $board;
		const name = nameDraft.trim();
		renaming = false;
		if (!current || !name || name === current.name) return;
		try {
			const renamed = await api.renameBoard(current.id, name);
			board.update((b) => (b && b.id === renamed.id ? { ...b, name: renamed.name } : b));
			upsertBoard(renamed);
		} catch (err) {
			toastError(err);
		}
	}

	async function archive() {
		const current = $board;
		if (!current || !confirm(`Archive "${current.name}"? It will disappear for everyone.`)) return;
		try {
			await api.archiveBoard(current.id);
			removeBoardLocally(current.id);
			await goto(`/${encodeURIComponent(workspaceId ?? '')}`);
		} catch (err) {
			toastError(err);
		}
	}
</script>

<svelte:head><title>{$board?.name ?? 'Board'} · ZeroBoard</title></svelte:head>

{#if $boardLoading || !$board}
	<Spinner />
{:else}
	<header class="flex flex-wrap items-center gap-3 border-b border-slate-200 bg-white px-4 py-2">
		{#if renaming}
			<form onsubmit={saveRename}>
				<input
					class="rounded-md border border-indigo-400 px-2 py-0.5 text-lg font-semibold focus:outline-none"
					maxlength={MAX_NAME_CHARS}
					bind:value={nameDraft}
					onblur={() => (renaming = false)}
				/>
			</form>
		{:else}
			<h1 class="text-lg font-semibold">{$board.name}</h1>
			{#if isAdmin}
				<button type="button" class="text-xs text-indigo-600 hover:underline" onclick={startRename}
					>Rename</button
				>
			{/if}
		{/if}

		<div class="flex -space-x-1" aria-label="Members viewing this board">
			{#each viewers as member (member.user_id)}
				<Avatar name={member.name} color={member.avatar_color} size="sm" online />
			{/each}
		</div>

		<div class="ml-auto flex items-center gap-2">
			<div class="flex rounded-md border border-slate-300 p-0.5 text-sm" role="group" aria-label="View">
				{#each [['kanban', 'Board'], ['table', 'Table']] as [value, label] (value)}
					<button
						type="button"
						class="rounded px-3 py-1 {view === value
							? 'bg-indigo-600 text-white'
							: 'text-slate-600 hover:bg-slate-100'}"
						aria-pressed={view === value}
						onclick={() => (view = value as View)}>{label}</button
					>
				{/each}
			</div>
			{#if isAdmin}
				<button
					type="button"
					class="rounded-md px-2 py-1 text-sm text-slate-500 hover:bg-slate-100 hover:text-red-600"
					onclick={archive}>Archive</button
				>
			{/if}
		</div>
	</header>

	<div class="min-h-0 flex-1">
		{#if view === 'kanban'}
			<KanbanBoard lists={$board.lists} {canEdit} onopencard={openCard} />
		{:else}
			<ListView lists={$board.lists} {canEdit} onopencard={openCard} />
		{/if}
	</div>
{/if}

{#if openCardId}
	<CardModal cardId={openCardId} {canEdit} onclose={closeCard} />
{/if}

<script lang="ts">
	import { onDestroy, onMount } from 'svelte';
	import { goto } from '$app/navigation';
	import { page } from '$app/state';
	import * as api from '$lib/api';
	import KanbanBoard from '$lib/components/board/KanbanBoard.svelte';
	import ListView from '$lib/components/board/ListView.svelte';
	import CardModal from '$lib/components/card/CardModal.svelte';
	import { accentFor } from '$lib/colors';
	import Avatar from '$lib/components/ui/Avatar.svelte';
	import Icon, { type IconName } from '$lib/components/ui/Icon.svelte';
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
	import { cardDensity, setCardDensity } from '$lib/stores/ui.store';
	import { activeUsers } from '$lib/stores/ws.store';
	import { joinBoard, leaveBoard, setBoardAccessLostHandler } from '$lib/ws';

	const MAX_NAME_CHARS = 255;
	const CARD_QUERY_PARAM = 'card';

	type View = 'kanban' | 'table';

	const VIEWS: { value: View; label: string; icon: IconName }[] = [
		{ value: 'kanban', label: 'Board', icon: 'kanban' },
		{ value: 'table', label: 'Table', icon: 'table' }
	];

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
	<header class="flex flex-wrap items-center gap-x-3 gap-y-2 border-b border-slate-200 dark:border-slate-800 bg-white dark:bg-slate-900 px-4 py-3 sm:px-6">
		<span
			class="h-7 w-7 shrink-0 rounded-lg shadow-inner"
			style:background-color={accentFor($board.id)}
		></span>
		{#if renaming}
			<form onsubmit={saveRename}>
				<input
					class="input py-1 text-lg font-bold"
					maxlength={MAX_NAME_CHARS}
					bind:value={nameDraft}
					onblur={() => (renaming = false)}
				/>
			</form>
		{:else}
			<h1 class="min-w-0 truncate text-lg font-bold tracking-tight sm:text-xl text-slate-900 dark:text-white">{$board.name}</h1>
			{#if isAdmin}
				<button type="button" class="icon-btn" aria-label="Rename board" onclick={startRename}
					><Icon name="pencil" /></button
				>
			{/if}
		{/if}

		{#if viewers.length > 0}
			<div
				class="ml-2 flex items-center gap-2 rounded-full bg-emerald-50 dark:bg-emerald-500/10 py-1 pl-1 pr-3 ring-1 ring-inset ring-emerald-200 dark:ring-emerald-500/30"
				aria-label="Members viewing this board"
			>
				<div class="flex -space-x-1.5">
					{#each viewers as member (member.user_id)}
						<Avatar name={member.name} color={member.avatar_color} size="sm" online />
					{/each}
				</div>
				<span class="hidden text-xs font-medium text-emerald-700 dark:text-emerald-400 sm:inline"
					>{viewers.length} here now</span
				>
			</div>
		{/if}

		<div class="ml-auto flex items-center gap-2">
			<div class="flex rounded-lg bg-slate-100 dark:bg-slate-800 p-1 text-sm" role="group" aria-label="View">
				{#each VIEWS as option (option.value)}
					<button
						type="button"
						class="inline-flex items-center gap-1.5 rounded-md px-3 py-1.5 font-medium transition {view ===
						option.value
							? 'bg-white dark:bg-slate-900 text-slate-900 dark:text-white shadow-sm'
							: 'text-slate-500 dark:text-slate-400 hover:text-slate-800 dark:hover:text-slate-100'}"
						aria-pressed={view === option.value}
						aria-label={option.label}
						onclick={() => (view = option.value)}
						><Icon name={option.icon} /><span class="hidden sm:inline">{option.label}</span></button
					>
				{/each}
			</div>
			{#if view === 'kanban'}
				<button
					type="button"
					class="btn-ghost btn-sm {$cardDensity === 'detailed'
						? 'bg-indigo-50 text-indigo-700 dark:bg-indigo-500/10 dark:text-indigo-300'
						: ''}"
					aria-pressed={$cardDensity === 'detailed'}
					title={$cardDensity === 'detailed' ? 'Show compact cards' : 'Show detailed cards'}
					onclick={() => setCardDensity($cardDensity === 'detailed' ? 'compact' : 'detailed')}
				>
					<Icon name={$cardDensity === 'detailed' ? 'collapse' : 'expand'} />
					<span class="hidden sm:inline">{$cardDensity === 'detailed' ? 'Compact' : 'Detailed'}</span>
				</button>
			{/if}
			{#if isAdmin}
				<button
					type="button"
					class="icon-btn hover:bg-red-50 dark:hover:bg-red-500/10 hover:text-red-600 dark:hover:text-red-400"
					aria-label="Archive board"
					title="Archive board"
					onclick={archive}><Icon name="archive" /></button
				>
			{/if}
		</div>
	</header>

	<div class="min-h-0 flex-1 bg-gradient-to-br from-slate-100 dark:from-slate-950 via-indigo-50/60 dark:via-slate-950 to-violet-50/60 dark:to-indigo-950/50">
		{#if view === 'kanban'}
			<KanbanBoard
				lists={$board.lists}
				detailed={$cardDensity === 'detailed'}
				{canEdit}
				onopencard={openCard}
			/>
		{:else}
			<ListView lists={$board.lists} {canEdit} onopencard={openCard} />
		{/if}
	</div>
{/if}

{#if openCardId}
	<CardModal cardId={openCardId} {canEdit} onclose={closeCard} />
{/if}

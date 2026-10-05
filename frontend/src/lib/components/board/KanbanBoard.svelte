<script lang="ts">
	import { flip } from 'svelte/animate';
	import { get } from 'svelte/store';
	import { dragHandleZone, TRIGGERS, type DndEvent } from 'svelte-dnd-action';
	import * as api from '$lib/api';
	import { FLIP_DURATION_MS, fitsBetweenNeighbours, isShadowItem, previousId } from '$lib/dnd';
	import {
		applyListCreated,
		applyListReordered,
		board,
		persistCardMove,
		reloadBoard,
		setLists
	} from '$lib/stores/board.store';
	import { toastError } from '$lib/stores/toast.store';
	import type { BoardCard, ListWithCards } from '$lib/types';
	import KanbanList from './KanbanList.svelte';

	const MAX_NAME_CHARS = 255;

	interface Props {
		lists: ListWithCards[];
		canEdit: boolean;
		onopencard: (cardId: string) => void;
	}

	let { lists, canEdit, onopencard }: Props = $props();

	interface DragOrigin {
		id: string;
		listId: string | null;
		afterId: string | null;
	}

	// Where the dragged item started, so a drop back in place makes no API call.
	let cardOrigin: DragOrigin | null = null;
	let listOrigin: DragOrigin | null = null;
	let newListName = $state('');

	function currentList(listId: string): ListWithCards | undefined {
		return get(board)?.lists.find((l) => l.id === listId);
	}

	function onCardDragStart(cardId: string, listId: string) {
		const list = currentList(listId);
		cardOrigin = { id: cardId, listId, afterId: list ? previousId(list.cards, cardId) : null };
	}

	async function onCardDrop(cardId: string, listId: string, cards: BoardCard[]) {
		const afterId = previousId(cards, cardId);
		const origin = cardOrigin;
		cardOrigin = null;
		if (origin?.id === cardId && origin.listId === listId && origin.afterId === afterId) return;
		await persistCardMove(cardId, listId, afterId);
	}

	function handleListConsider(event: CustomEvent<DndEvent<ListWithCards>>) {
		const { items, info } = event.detail;
		if (info.trigger === TRIGGERS.DRAG_STARTED) {
			listOrigin = { id: info.id, listId: null, afterId: previousId(lists, info.id) };
		}
		setLists(items);
	}

	async function handleListFinalize(event: CustomEvent<DndEvent<ListWithCards>>) {
		const { items, info } = event.detail;
		setLists(items);
		const afterId = previousId(items, info.id);
		const origin = listOrigin;
		listOrigin = null;
		if (origin?.id === info.id && origin.afterId === afterId) return;
		try {
			const moved = await api.reorderList(info.id, afterId);
			const current = get(board)?.lists ?? [];
			if (fitsBetweenNeighbours(current, moved.id, moved.position)) {
				applyListReordered(moved.id, moved.position);
			} else {
				await reloadBoard();
			}
		} catch (err) {
			toastError(err);
			await reloadBoard();
		}
	}

	async function addList(event: SubmitEvent) {
		event.preventDefault();
		const name = newListName.trim();
		const boardId = get(board)?.id;
		if (!name || !boardId) return;
		try {
			applyListCreated(await api.createList(boardId, name));
			newListName = '';
		} catch (err) {
			toastError(err);
		}
	}
</script>

<div class="flex h-full items-start gap-3 overflow-x-auto p-4">
	<div
		class="flex h-full items-start gap-3"
		use:dragHandleZone={{
			items: lists,
			type: 'list',
			flipDurationMs: FLIP_DURATION_MS,
			dragDisabled: !canEdit,
			dropTargetStyle: {}
		}}
		onconsider={handleListConsider}
		onfinalize={handleListFinalize}
		aria-label="Columns"
	>
		{#each lists as list (list.id)}
			<div
				animate:flip={{ duration: FLIP_DURATION_MS }}
				class="h-full {isShadowItem(list) ? 'opacity-40' : ''}"
			>
				<KanbanList
					{list}
					{canEdit}
					{onopencard}
					ondragstart={onCardDragStart}
					ondrop={onCardDrop}
				/>
			</div>
		{/each}
	</div>

	{#if canEdit}
		<form class="w-72 shrink-0 rounded-xl bg-slate-100 p-2" onsubmit={addList}>
			<input
				class="w-full rounded-md border border-slate-300 bg-white px-2 py-1.5 text-sm focus:border-indigo-500 focus:outline-none"
				placeholder="+ Add column"
				maxlength={MAX_NAME_CHARS}
				bind:value={newListName}
			/>
		</form>
	{:else if lists.length === 0}
		<p class="text-sm text-slate-500">This board has no columns yet.</p>
	{/if}
</div>
